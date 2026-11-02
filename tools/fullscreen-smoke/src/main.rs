//! Owned borderless fullscreen fixture for the AppBar notification protocol.
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
        },
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
            Shell::{ITaskbarList2, TaskbarList},
            WindowsAndMessaging::*,
        },
    },
    core::w,
};
unsafe extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_CLOSE {
        unsafe {
            if let Ok(taskbar) =
                CoCreateInstance::<_, ITaskbarList2>(&TaskbarList, None, CLSCTX_INPROC_SERVER)
            {
                let _ = taskbar.HrInit();
                let _ = taskbar.MarkFullscreenWindow(window, false);
            }
            let _ = ShowWindow(window, SW_HIDE);
        }
    }
    if message == WM_DESTROY {
        unsafe {
            PostQuitMessage(0);
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}
fn rail_topmost() -> Option<bool> {
    let rail = unsafe { FindWindowW(w!("VirtualDesktopRail"), w!("Virtual Desktop Rail")) }.ok()?;
    Some(unsafe { GetWindowLongPtrW(rail, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST.0 != 0)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--inspect-rail"] {
        println!(
            "{}",
            rail_topmost()
                .map(|value| value.to_string())
                .unwrap_or("null".into())
        );
        return Ok(());
    }
    if args == ["--help"] {
        println!(
            "rail-fullscreen-smoke --run-owned-fullscreen | --inspect-rail\nAn owned borderless fullscreen window exercises the AppBar fullscreen protocol. Close it with Alt+F4. No desktop switching or pin changes. --inspect-rail reads only the current topmost style."
        );
        return Ok(());
    }
    if args != ["--run-owned-fullscreen"] {
        return Err("Use --help or --run-owned-fullscreen".into());
    }
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            lpszClassName: w!("RailFullscreenSmoke"),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(6usize as *mut _),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(windows::core::Error::from_win32().into());
        }
        let width = GetSystemMetrics(SM_CXSCREEN);
        let height = GetSystemMetrics(SM_CYSCREEN);
        let window = CreateWindowExW(
            WS_EX_APPWINDOW,
            w!("RailFullscreenSmoke"),
            w!("Rail fullscreen compatibility fixture"),
            WS_POPUP | WS_SYSMENU,
            0,
            0,
            width,
            height,
            None,
            None,
            None,
            None,
        )?;
        let _ = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!("Fullscreen AppBar test fixture.\r\nPress Alt+F4 to close."),
            WS_CHILD | WS_VISIBLE,
            60,
            60,
            600,
            100,
            window,
            None,
            None,
            None,
        )?;
        let _ = ShowWindow(window, SW_SHOW);
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        let taskbar: ITaskbarList2 = CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)?;
        taskbar.HrInit()?;
        taskbar.MarkFullscreenWindow(window, true)?;
        std::thread::sleep(std::time::Duration::from_millis(600));
        println!(
            "{{\"phase\":\"opened\",\"hwnd\":{},\"foreground\":{},\"rail_topmost\":{},\"display\":[{},{}]}}",
            window.0 as usize,
            GetForegroundWindow() == window,
            rail_topmost()
                .map(|value| value.to_string())
                .unwrap_or("null".into()),
            width,
            height
        );
        let mut message = MSG::default();
        loop {
            let result = GetMessageW(&mut message, None, 0, 0).0;
            if result == -1 {
                return Err(windows::core::Error::from_win32().into());
            }
            if result == 0 {
                break;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let _ = taskbar.MarkFullscreenWindow(window, false);
        drop(taskbar);
        CoUninitialize();
        std::thread::sleep(std::time::Duration::from_millis(600));
        println!(
            "{{\"phase\":\"closed\",\"rail_topmost\":{}}}",
            rail_topmost()
                .map(|value| value.to_string())
                .unwrap_or("null".into())
        );
    }
    Ok(())
}
