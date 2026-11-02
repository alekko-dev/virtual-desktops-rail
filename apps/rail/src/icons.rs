use crate::{Event, native, notify, platform};
use crossbeam_channel::Sender;
use rail_core::Window;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CopyIcon, GCLP_HICON, GCLP_HICONSM, GetClassLongPtrW, HICON, SMTO_ABORTIFHUNG,
    SendMessageTimeoutW, WM_GETICON,
};

#[link(name = "shell32")]
unsafe extern "system" {
    fn ExtractIconExW(
        file: *const u16,
        index: i32,
        large: *mut native::Handle,
        small: *mut native::Handle,
        count: u32,
    ) -> u32;
}

pub fn start() -> Sender<(Window, u32)> {
    let (sender, incoming) = crossbeam_channel::bounded::<(Window, u32)>(16);
    std::thread::spawn(move || {
        for (window, dpi) in incoming {
            if !platform::validate(&window) {
                continue;
            }
            let handle = platform::hwnd(window.hwnd);
            let mut icon = 0usize;
            for kind in [2, 0, 1] {
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
                    break;
                }
            }
            if icon == 0 {
                icon = unsafe { GetClassLongPtrW(handle, GCLP_HICONSM) };
            }
            if icon == 0 {
                icon = unsafe { GetClassLongPtrW(handle, GCLP_HICON) };
            }
            let mut owned = if icon != 0 {
                unsafe { CopyIcon(HICON(icon as *mut _)) }
                    .map(|icon| icon.0 as usize)
                    .unwrap_or(0)
            } else {
                0
            };
            if owned == 0 {
                let mut small = std::ptr::null_mut();
                unsafe {
                    ExtractIconExW(
                        native::wide(&window.identity.executable).as_ptr(),
                        0,
                        std::ptr::null_mut(),
                        &mut small,
                        1,
                    );
                }
                owned = small as usize;
            }
            if !platform::validate(&window) {
                if owned != 0 {
                    unsafe {
                        native::DestroyIcon(owned as native::Handle);
                    }
                }
                continue;
            }
            notify(Event::Icon(window.identity.key(), dpi, owned));
        }
    });
    sender
}
