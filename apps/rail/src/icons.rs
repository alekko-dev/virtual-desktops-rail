use crate::{Event, native, notify, platform};
use crossbeam_channel::Sender;
use rail_core::Window;
use windows::Win32::Foundation::{E_INVALIDARG, HANDLE, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject, GetObjectW, HBITMAP, HDC,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
    WICBitmapInterpolationModeFant,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CopyIcon, CreateIconIndirect, DestroyIcon, GCLP_HICON, GCLP_HICONSM, GetClassLongPtrW,
    GetIconInfo, HICON, ICONINFO, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_GETICON,
};

#[link(name = "shell32")]
unsafe extern "system" {
    fn SHDefExtractIconW(
        file: *const u16,
        index: i32,
        flags: u32,
        large: *mut HICON,
        small: *mut HICON,
        size: u32,
    ) -> i32;
}

// Shared by extraction and painting: no second resize on the UI thread.
pub fn pixel_size(dpi: u32) -> i32 {
    ((16 * dpi.max(96) + 48) / 96) as i32
}

struct Icon(HICON);
impl Drop for Icon {
    fn drop(&mut self) {
        let _ = unsafe { DestroyIcon(self.0) };
    }
}
struct Bitmap(HBITMAP);
impl Drop for Bitmap {
    fn drop(&mut self) {
        let _ = unsafe { DeleteObject(self.0) };
    }
}
struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn dimensions(icon: HICON) -> Option<(i32, i32)> {
    let mut info = ICONINFO::default();
    unsafe { GetIconInfo(icon, &mut info) }.ok()?;
    let color = Bitmap(info.hbmColor);
    let mask = Bitmap(info.hbmMask);
    let mut bitmap = BITMAP::default();
    let has_color = !color.0.is_invalid();
    let result = unsafe {
        GetObjectW(
            if has_color { color.0 } else { mask.0 },
            std::mem::size_of::<BITMAP>() as i32,
            Some((&mut bitmap as *mut BITMAP).cast()),
        )
    };
    (result != 0).then_some((
        bitmap.bmWidth,
        bitmap.bmHeight / if has_color { 1 } else { 2 },
    ))
}

// Exact size first, then the smallest source large enough to downsample, then
// the largest undersized source. Preserve window-specific artwork over EXE icons.
fn rank((width, height): (i32, i32), size: i32) -> (u8, i32) {
    if width == size && height == size {
        (0, 0)
    } else if width >= size && height >= size {
        (1, width.max(height))
    } else {
        (2, -width.min(height))
    }
}

fn resize(factory: &IWICImagingFactory, icon: HICON, size: i32) -> windows::core::Result<Icon> {
    unsafe {
        let source = factory.CreateBitmapFromHICON(icon)?;
        let (mut width, mut height) = (0, 0);
        source.GetSize(&mut width, &mut height)?;
        if width == 0 || height == 0 || width > 1024 || height > 1024 {
            return Err(windows::core::Error::from_hresult(E_INVALIDARG));
        }
        // HICON pixels are already premultiplied. WIC's HICON import labels its
        // raw pixels BGRA; converting that to PBGRA would multiply alpha twice.
        // Re-label the imported bytes before filtering, retaining mask-derived alpha.
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        source.CopyPixels(std::ptr::null(), width * 4, &mut pixels)?;
        let source = factory.CreateBitmapFromMemory(
            width,
            height,
            &GUID_WICPixelFormat32bppPBGRA,
            width * 4,
            &pixels,
        )?;
        let scaler = factory.CreateBitmapScaler()?;
        scaler.Initialize(
            &source,
            size as u32,
            size as u32,
            WICBitmapInterpolationModeFant,
        )?;
        let mut pixels = vec![0u8; (size * size * 4) as usize];
        scaler.CopyPixels(std::ptr::null(), size as u32 * 4, &mut pixels)?;
        from_pixels(size, &pixels)
    }
}

// Pixels are top-down, premultiplied BGRA, as expected by alpha-aware DrawIconEx.
fn from_pixels(size: i32, pixels: &[u8]) -> windows::core::Result<Icon> {
    assert_eq!(pixels.len(), (size * size * 4) as usize);
    unsafe {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let color = Bitmap(CreateDIBSection(
            HDC::default(),
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            HANDLE::default(),
            0,
        )?);
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast(), pixels.len());
        let mask_bytes = vec![0u8; (size as usize).div_ceil(16) * 2 * size as usize];
        let mask = Bitmap(CreateBitmap(
            size,
            size,
            1,
            1,
            Some(mask_bytes.as_ptr().cast()),
        ));
        if mask.0.is_invalid() {
            return Err(windows::core::Error::from_win32());
        }
        CreateIconIndirect(&ICONINFO {
            fIcon: true.into(),
            hbmColor: color.0,
            hbmMask: mask.0,
            ..Default::default()
        })
        .map(Icon)
    }
}

fn load(window: &Window, dpi: u32, factory: Option<&IWICImagingFactory>) -> Option<Icon> {
    let handle = platform::hwnd(window.hwnd);
    let size = pixel_size(dpi);
    let mut candidates = Vec::new();
    for kind in [2, 0, 1] {
        let mut icon = 0usize;
        let result = unsafe {
            SendMessageTimeoutW(
                handle,
                WM_GETICON,
                WPARAM(kind),
                LPARAM(dpi as isize),
                SMTO_ABORTIFHUNG,
                100,
                Some(&mut icon),
            )
        };
        if result.0 != 0 && icon != 0 {
            candidates.push(HICON(icon as *mut _));
        }
    }
    if candidates.is_empty() {
        for field in [GCLP_HICONSM, GCLP_HICON] {
            let icon = unsafe { GetClassLongPtrW(handle, field) };
            if icon != 0 {
                candidates.push(HICON(icon as *mut _));
            }
        }
    }
    // Copy borrowed handles before measuring/processing them; only owned copies
    // are ever destroyed. Stable ordering retains the WM_GETICON preference on ties.
    let mut best = None;
    let mut best_rank = (u8::MAX, i32::MAX);
    for borrowed in candidates {
        if let Ok(owned) = unsafe { CopyIcon(borrowed) } {
            let owned = Icon(owned);
            let candidate_rank = dimensions(owned.0).map(|d| rank(d, size)).unwrap_or((3, 0));
            if candidate_rank < best_rank {
                best_rank = candidate_rank;
                best = Some(owned);
            }
        }
    }
    if best.is_none() {
        let mut icon = HICON::default();
        unsafe {
            SHDefExtractIconW(
                native::wide(&window.identity.executable).as_ptr(),
                0,
                0,
                &mut icon,
                std::ptr::null_mut(),
                size as u32,
            );
        }
        if !icon.is_invalid() {
            best = Some(Icon(icon));
        }
    }
    let icon = best?;
    if dimensions(icon.0) != Some((size, size))
        && let Some(factory) = factory
        && let Ok(resized) = resize(factory, icon.0, size)
    {
        return Some(resized);
    }
    Some(icon)
}

pub fn start() -> Sender<(Window, u32)> {
    let (sender, incoming) = crossbeam_channel::bounded::<(Window, u32)>(16);
    std::thread::spawn(move || {
        let apartment = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .is_ok()
            .then(|| Apartment);
        let factory: Option<IWICImagingFactory> = apartment.as_ref().and_then(|_| unsafe {
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()
        });
        for (window, dpi) in incoming {
            if !platform::validate(&window) {
                continue;
            }
            let icon = load(&window, dpi, factory.as_ref());
            if !platform::validate(&window) {
                continue;
            }
            let owned = icon
                .map(|icon| {
                    let handle = icon.0.0 as usize;
                    std::mem::forget(icon);
                    handle
                })
                .unwrap_or(0);
            notify(Event::Icon(window.identity.key(), dpi, owned));
        }
    });
    sender
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsampling_retains_alpha_and_filters_instead_of_dropping_pixels() {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .unwrap();
        let _apartment = Apartment;
        let factory: IWICImagingFactory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
                .unwrap();
        // Every 2x2 block contains two opaque red and two transparent pixels.
        // Nearest-neighbor sampling would incorrectly give only 0 or 255 alpha.
        let pixels: Vec<u8> = (0..32 * 32)
            .flat_map(|i| {
                if (i % 32 + i / 32) % 2 == 0 {
                    [0, 0, 255, 255]
                } else {
                    [0; 4]
                }
            })
            .collect();
        let original = from_pixels(32, &pixels).unwrap();
        let intermediate = resize(&factory, original.0, 16).unwrap();
        let scaled = resize(&factory, intermediate.0, 8).unwrap();
        assert_eq!(dimensions(scaled.0), Some((8, 8)));
        let bitmap = unsafe { factory.CreateBitmapFromHICON(scaled.0) }.unwrap();
        let mut output = vec![0u8; 8 * 8 * 4];
        unsafe { bitmap.CopyPixels(std::ptr::null(), 8 * 4, &mut output) }.unwrap();
        for pixel in output.as_chunks::<4>().0 {
            assert_eq!(&pixel[..2], &[0, 0]);
            assert!((126..=129).contains(&pixel[3]), "{pixel:?}");
            assert!(pixel[2].abs_diff(pixel[3]) <= 1, "{pixel:?}");
        }
    }

    #[test]
    fn source_selection_avoids_upscaling_when_larger_artwork_exists() {
        assert!(rank((20, 20), 20) < rank((32, 32), 20));
        assert!(rank((24, 24), 20) < rank((32, 32), 20));
        assert!(rank((32, 32), 20) < rank((16, 16), 20));
        assert!(rank((16, 16), 20) < rank((8, 8), 20));
        assert_eq!([96, 120, 144, 192].map(pixel_size), [16, 20, 24, 32]);
    }
}
