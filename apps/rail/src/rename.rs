//! Small, modeless editor. Shell calls stay in the integration subprocess.
use crate::native::*;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU64, Ordering};

pub const OPEN: u32 = 0x8060;
pub const SAVE: u32 = 0x8061;
const CLASS: &str = "VirtualDesktopRailRename";
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
const MAX_NAME: usize = 256;

pub fn validated_name(value: &str) -> Result<String, String> {
    let name = value.trim();
    if name.is_empty() {
        return Err("Enter a desktop name.".into());
    }
    if name.encode_utf16().count() > MAX_NAME || name.chars().any(char::is_control) {
        return Err("Use a single line of up to 256 characters.".into());
    }
    Ok(name.into())
}

pub struct Popover {
    pub window: Handle,
    pub token: u64,
    pub guid: String,
    edit: Handle,
    status: Handle,
    cancel: Handle,
    save: Handle,
    fonts: [Handle; 3],
    pending: bool,
}

impl Popover {
    pub fn show(
        owner: Handle,
        anchor_rect: windows::Win32::Foundation::RECT,
        guid: String,
        name: &str,
    ) -> Option<Self> {
        use windows::Win32::Foundation::{BOOL, HWND};
        use windows::Win32::Graphics::Dwm::{
            DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
            DwmSetWindowAttribute,
        };
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
        };
        use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
        unsafe {
            let instance = GetModuleHandleW(null());
            let class_name = wide(CLASS);
            let class = WindowClass {
                size: std::mem::size_of::<WindowClass>() as u32,
                style: 0,
                procedure: Some(procedure),
                class_extra: 0,
                window_extra: 0,
                instance,
                icon: null_mut(),
                cursor: LoadCursorW(null_mut(), 32512usize as *const u16),
                background: 6usize as Handle, // COLOR_WINDOW + 1
                menu_name: null(),
                class_name: class_name.as_ptr(),
                small_icon: null_mut(),
            };
            RegisterClassExW(&class);
            let dpi = GetDpiForWindow(owner).max(96) as i32;
            let scale = |n: i32| (n * dpi + 48) / 96;
            let mut monitor = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(
                MonitorFromWindow(HWND(owner), MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            )
            .as_bool()
            {
                return None;
            }
            let width = scale(280);
            let height = scale(114);
            let x = (anchor_rect.right + scale(6)).clamp(
                monitor.rcWork.left,
                (monitor.rcWork.right - width).max(monitor.rcWork.left),
            );
            let y = (anchor_rect.top - scale(6)).clamp(
                monitor.rcWork.top,
                (monitor.rcWork.bottom - height).max(monitor.rcWork.top),
            );
            let window = CreateWindowExW(
                0x00010088,
                wide(CLASS).as_ptr(),
                wide("Rename desktop").as_ptr(),
                0x82000000,
                x,
                y,
                width,
                height,
                owner,
                null_mut(),
                instance,
                null_mut(),
            );
            if window.is_null() {
                return None;
            }
            let dark_mode = BOOL(i32::from(crate::theme::dark()));
            let corner = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                HWND(window),
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark_mode as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<BOOL>() as u32,
            );
            let _ = DwmSetWindowAttribute(
                HWND(window),
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            );
            let make_font = |size: i32, weight: i32| {
                CreateFontW(
                    -scale(size),
                    0,
                    0,
                    0,
                    weight,
                    0,
                    0,
                    0,
                    1,
                    0,
                    0,
                    5,
                    0,
                    wide("Segoe UI").as_ptr(),
                )
            };
            let fonts = [make_font(13, 500), make_font(13, 400), make_font(12, 400)];
            let control =
                |class: &str, text: &str, style: u32, id: usize, x, y, w, h, font: Handle| {
                    let child = CreateWindowExW(
                        if class == "EDIT" { 0x200 } else { 0 },
                        wide(class).as_ptr(),
                        wide(text).as_ptr(),
                        0x50000000 | style,
                        scale(x),
                        scale(y),
                        scale(w),
                        scale(h),
                        window,
                        id as Handle,
                        instance,
                        null_mut(),
                    );
                    SendMessageW(child, 0x0030, font as usize, 1);
                    child
                };
            control("STATIC", "Rename desktop", 0, 10, 14, 12, 252, 20, fonts[0]);
            let edit = control("EDIT", name, 0x10080, 11, 14, 38, 252, 24, fonts[1]);
            let margin = scale(5) as isize;
            SendMessageW(edit, 0x00d3, 3, margin | (margin << 16)); // EM_SETMARGINS
            SendMessageW(edit, 0x00c5, MAX_NAME, 0); // EM_SETLIMITTEXT
            let status = control("STATIC", "", 0, 12, 14, 68, 252, 32, fonts[2]);
            ShowWindow(status, 0);
            let cancel = control("BUTTON", "Cancel", 0x10000, 2, 98, 74, 80, 28, fonts[1]);
            let save = control("BUTTON", "Rename", 0x10001, 1, 186, 74, 80, 28, fonts[1]);
            ShowWindow(window, 5);
            let _ = SetForegroundWindow(HWND(window));
            SetFocus(edit);
            SendMessageW(edit, 0x00b1, 0, -1); // EM_SETSEL
            Some(Self {
                window,
                token: NEXT_TOKEN.fetch_add(1, Ordering::Relaxed),
                guid,
                edit,
                status,
                cancel,
                save,
                fonts,
                pending: false,
            })
        }
    }

    fn set_status(&self, message: &str, is_error: bool) {
        use windows::Win32::Foundation::{HWND, RECT};
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
            RDW_ALLCHILDREN, RDW_ERASE, RDW_INVALIDATE, RedrawWindow,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowRect, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
        };
        unsafe {
            let dpi = GetDpiForWindow(self.window).max(96) as i32;
            let scale = |n: i32| (n * dpi + 48) / 96;
            SetWindowLongPtrW(self.status, -21, isize::from(is_error));
            SetWindowTextW(self.status, wide(message).as_ptr());
            let has_status = !message.is_empty();
            ShowWindow(self.status, if has_status { 5 } else { 0 });
            InvalidateRect(self.status, null(), 1);
            let button_y = scale(if has_status { 106 } else { 74 });
            for (button, x) in [(self.cancel, scale(98)), (self.save, scale(186))] {
                let _ = SetWindowPos(
                    HWND(button),
                    None,
                    x,
                    button_y,
                    scale(80),
                    scale(28),
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
            let height = scale(if has_status { 146 } else { 114 });
            let mut rect = RECT::default();
            if GetWindowRect(HWND(self.window), &mut rect).is_ok() {
                let mut monitor = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                let top = if GetMonitorInfoW(
                    MonitorFromWindow(HWND(self.window), MONITOR_DEFAULTTONEAREST),
                    &mut monitor,
                )
                .as_bool()
                {
                    rect.top.clamp(
                        monitor.rcWork.top,
                        (monitor.rcWork.bottom - height).max(monitor.rcWork.top),
                    )
                } else {
                    rect.top
                };
                let _ = SetWindowPos(
                    HWND(self.window),
                    None,
                    rect.left,
                    top,
                    rect.right - rect.left,
                    height,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
            let _ = RedrawWindow(
                HWND(self.window),
                None,
                None,
                RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
            );
        }
    }

    pub fn request(&mut self) -> Option<(String, String, u64)> {
        if self.pending {
            return None;
        }
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};
        let name = unsafe {
            let mut text = vec![
                0u16;
                GetWindowTextLengthW(windows::Win32::Foundation::HWND(self.edit)).max(0)
                    as usize
                    + 1
            ];
            let len = GetWindowTextW(windows::Win32::Foundation::HWND(self.edit), &mut text);
            String::from_utf16_lossy(&text[..len as usize])
        };
        let name = match validated_name(&name) {
            Ok(name) => name,
            Err(error) => {
                self.fail(&error);
                return None;
            }
        };
        self.token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
        self.pending = true;
        unsafe {
            EnableWindow(self.edit, 0);
            EnableWindow(self.save, 0);
        }
        self.set_status("Renaming…", false);
        Some((self.guid.clone(), name, self.token))
    }

    pub fn fail(&mut self, error: &str) {
        self.token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
        self.pending = false;
        unsafe {
            EnableWindow(self.edit, 1);
            EnableWindow(self.save, 1);
            self.set_status(error, true);
            if GetForegroundWindow() == self.window {
                SetFocus(self.edit);
            }
        }
    }

    pub fn pending(&self) -> bool {
        self.pending
    }
}

impl Drop for Popover {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.window);
            for font in self.fonts {
                DeleteObject(font);
            }
        }
    }
}

unsafe extern "system" fn procedure(
    window: Handle,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        COLOR_GRAYTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT, GetSysColor, GetSysColorBrush, HDC,
        SetBkColor, SetBkMode, SetTextColor, TRANSPARENT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GW_OWNER, GetDlgCtrlID, GetWindow, PostMessageW,
    };
    match message {
        0x0135 | 0x0138 => unsafe {
            let control = GetDlgCtrlID(HWND(lparam as *mut _));
            let dc = HDC(wparam as *mut _);
            let text_color = if control == 12 {
                if GetWindowLongPtrW(lparam as Handle, -21) != 0 {
                    if crate::ui::high_contrast() {
                        COLORREF(GetSysColor(COLOR_WINDOWTEXT))
                    } else {
                        COLORREF(0x00201cc4)
                    }
                } else {
                    COLORREF(GetSysColor(COLOR_GRAYTEXT))
                }
            } else if control == 10 && !crate::ui::high_contrast() {
                COLORREF(0x0058524f)
            } else {
                COLORREF(GetSysColor(COLOR_WINDOWTEXT))
            };
            SetTextColor(dc, text_color);
            SetBkColor(dc, COLORREF(GetSysColor(COLOR_WINDOW)));
            SetBkMode(dc, TRANSPARENT);
            GetSysColorBrush(COLOR_WINDOW).0 as isize
        },
        0x0006 if wparam & 0xffff == 0 => {
            unsafe {
                ShowWindow(window, 0);
            }
            0
        }
        0x0010 => {
            unsafe {
                ShowWindow(window, 0);
            }
            0
        }
        0x0111 if wparam & 0xffff == 1 && wparam >> 16 == 0 => {
            if let Ok(owner) = unsafe { GetWindow(HWND(window), GW_OWNER) } {
                let _ = unsafe { PostMessageW(owner, SAVE, WPARAM(window as usize), LPARAM(0)) };
            }
            0
        }
        0x0111 if wparam & 0xffff == 2 && wparam >> 16 == 0 => {
            unsafe {
                ShowWindow(window, 0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_names_are_bounded_single_line_unicode() {
        assert_eq!(
            validated_name("  \u{0420}\u{0430}\u{0431}\u{043e}\u{0442}\u{0430} \u{1f9d1}  ")
                .unwrap(),
            "\u{0420}\u{0430}\u{0431}\u{043e}\u{0442}\u{0430} \u{1f9d1}"
        );
        assert!(validated_name("   ").is_err());
        assert!(validated_name("a\0b").is_err());
        assert!(validated_name("a\nb").is_err());
        assert!(validated_name(&"a".repeat(257)).is_err());
        assert!(validated_name(&"\u{1f9d1}".repeat(129)).is_err());
        assert!(validated_name(&"a".repeat(256)).is_ok());
    }
}
