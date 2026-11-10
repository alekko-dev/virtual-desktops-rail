//! Primary-monitor AppBar reservation and shell z-order lifecycle.
use super::{Handle, Point, Rect, size_of, wide};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

const CALLBACK: u32 = 0x8001;
static REGISTERED: AtomicBool = AtomicBool::new(false);
static STOPPING: AtomicBool = AtomicBool::new(false);
static NEGOTIATING: AtomicBool = AtomicBool::new(false);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static FULLSCREEN: AtomicBool = AtomicBool::new(false);
static POSITION_ERROR: AtomicU32 = AtomicU32::new(0);

pub fn diagnostics(window: Handle) -> serde_json::Value {
    use windows::Win32::{Foundation::RECT, UI::WindowsAndMessaging::*};
    unsafe {
        let foreground = GetForegroundWindow();
        let root = GetAncestor(foreground, GA_ROOTOWNER);
        let mut bounds = RECT::default();
        let _ = GetWindowRect(root, &mut bounds);
        serde_json::json!({"automation_ui":GetWindowLongPtrW(windows::Win32::Foundation::HWND(window), GWL_EXSTYLE) as u32 & WS_EX_APPWINDOW.0 != 0,
            "hwnd":window as usize,"position_error":POSITION_ERROR.load(Ordering::Relaxed),"yielding": FULLSCREEN.load(Ordering::Relaxed),
            "topmost": GetWindowLongPtrW(windows::Win32::Foundation::HWND(window), GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0,
            "foreground":foreground.0 as usize,"root":root.0 as usize,
            "root_bounds":[bounds.left,bounds.top,bounds.right,bounds.bottom]})
    }
}

/// Reconcile the actual foreground after shell notifications and queued activation
/// events race. Also called by the existing one-second reconciliation; no extra
/// timer or cross-process window message is needed.
pub fn foreground_changed(window: Handle) {
    use windows::Win32::{Foundation::RECT, Graphics::Gdi::*, UI::WindowsAndMessaging::*};
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() {
            return;
        }
        let root = GetAncestor(foreground, GA_ROOTOWNER);
        let mut bounds = RECT::default();
        if GetWindowRect(root, &mut bounds).is_err() {
            return;
        }
        let monitor = MonitorFromWindow(root, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !windows::Win32::Graphics::Gdi::GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let mut class = [0u16; 128];
        let length = GetClassNameW(root, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..length]);
        let full = info.dwFlags & 1 != 0
            && !matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd")
            && IsWindowVisible(root).as_bool()
            && !IsIconic(root).as_bool()
            && bounds.left <= info.rcMonitor.left
            && bounds.top <= info.rcMonitor.top
            && bounds.right >= info.rcMonitor.right
            && bounds.bottom >= info.rcMonitor.bottom;
        let changed = FULLSCREEN.swap(full, Ordering::Relaxed) != full;
        let topmost = GetWindowLongPtrW(windows::Win32::Foundation::HWND(window), GWL_EXSTYLE)
            as u32
            & WS_EX_TOPMOST.0
            != 0;
        if changed || topmost == full {
            // Z-order-only work must not reorder an owner or reenter position
            // negotiation through WM_WINDOWPOSCHANGING.
            let result = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                windows::Win32::Foundation::HWND(window),
                if full { HWND_BOTTOM } else { HWND_TOPMOST },
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOSENDCHANGING,
            );
            POSITION_ERROR.store(
                result.err().map(|e| e.code().0 as u32).unwrap_or(0),
                Ordering::Relaxed,
            );
        }
    }
}

#[repr(C)]
struct AppBarData {
    size: u32,
    window: Handle,
    callback: u32,
    edge: u32,
    rect: Rect,
    parameter: isize,
}

#[repr(C)]
struct MonitorInfo {
    size: u32,
    monitor: Rect,
    work: Rect,
    flags: u32,
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn SHAppBarMessage(message: u32, data: *mut AppBarData) -> usize;
}
#[link(name = "user32")]
unsafe extern "system" {
    fn MonitorFromPoint(point: Point, flags: u32) -> Handle;
    fn GetMonitorInfoW(monitor: Handle, info: *mut MonitorInfo) -> i32;
    fn GetDpiForWindow(window: Handle) -> u32;
    fn SetWindowPos(
        window: Handle,
        after: Handle,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
    fn RegisterWindowMessageW(name: *const u16) -> u32;
}

fn data(window: Handle) -> AppBarData {
    AppBarData {
        size: size_of::<AppBarData>() as u32,
        window,
        callback: CALLBACK,
        edge: 0,
        rect: Rect::default(),
        parameter: 0,
    }
}

fn negotiate(window: Handle) -> Result<(), std::io::Error> {
    if NEGOTIATING.swap(true, Ordering::Relaxed) {
        return Ok(());
    }
    let result = (|| {
        let mut info = MonitorInfo {
            size: size_of::<MonitorInfo>() as u32,
            monitor: Rect::default(),
            work: Rect::default(),
            flags: 0,
        };
        unsafe {
            let monitor = MonitorFromPoint(Point { x: 0, y: 0 }, 1);
            if GetMonitorInfoW(monitor, &mut info) == 0 {
                return Err(std::io::Error::last_os_error());
            }
            let dpi = GetDpiForWindow(window).max(96);
            let width = ((25 * dpi + 48) / 96) as i32;
            let mut bar = data(window);
            bar.rect = info.monitor;
            bar.rect.right = bar.rect.left + width;
            SHAppBarMessage(2, &mut bar); // ABM_QUERYPOS
            bar.rect.right = bar.rect.left + width;
            SHAppBarMessage(3, &mut bar); // ABM_SETPOS
            if SetWindowPos(
                window,
                if FULLSCREEN.load(Ordering::Relaxed) {
                    1_isize as Handle
                } else {
                    -1_isize as Handle
                },
                bar.rect.left,
                bar.rect.top,
                bar.rect.right - bar.rect.left,
                bar.rect.bottom - bar.rect.top,
                0x10,
            ) == 0
            {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(())
    })();
    NEGOTIATING.store(false, Ordering::Relaxed);
    result
}

pub fn register(window: Handle) -> Result<(), std::io::Error> {
    if STOPPING.load(Ordering::Relaxed) {
        return Ok(());
    }
    TASKBAR_CREATED.store(
        unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) },
        Ordering::Relaxed,
    );
    if unsafe { SHAppBarMessage(0, &mut data(window)) } == 0 {
        return Err(std::io::Error::other("AppBar registration failed"));
    }
    REGISTERED.store(true, Ordering::Relaxed);
    if let Err(error) = negotiate(window) {
        REGISTERED.store(false, Ordering::Relaxed);
        unsafe {
            SHAppBarMessage(1, &mut data(window));
        }
        return Err(error);
    }
    Ok(())
}

/// Release the reservation while the window is still alive. Shell calls can
/// reenter our procedure: disable negotiation before notifying the shell.
pub fn unregister(window: Handle) {
    STOPPING.store(true, Ordering::Relaxed);
    if REGISTERED.swap(false, Ordering::Relaxed) {
        crate::shutdown_trace::measure("ABM_REMOVE", || unsafe {
            SHAppBarMessage(1, &mut data(window));
        });
    }
}

pub fn message(window: Handle, message: u32, wparam: usize, lparam: isize) -> Option<isize> {
    if STOPPING.load(Ordering::Relaxed) {
        return None;
    }
    let taskbar = TASKBAR_CREATED.load(Ordering::Relaxed);
    if taskbar != 0 && message == taskbar {
        REGISTERED.store(false, Ordering::Relaxed);
        let _ = register(window);
        return Some(0);
    }
    if !REGISTERED.load(Ordering::Relaxed) {
        return None;
    }
    match message {
        CALLBACK => {
            if wparam == 1 {
                let _ = negotiate(window);
            } // ABN_POSCHANGED
            if wparam == 2 {
                FULLSCREEN.store(lparam != 0, Ordering::Relaxed);
                // ABN_FULLSCREENAPP: yield z-order, never activate.
                unsafe {
                    SetWindowPos(
                        window,
                        if lparam != 0 {
                            1_isize as Handle
                        } else {
                            -1_isize as Handle
                        },
                        0,
                        0,
                        0,
                        0,
                        0x613, // NOSIZE | NOMOVE | NOACTIVATE | NOOWNERZORDER | NOSENDCHANGING.
                    );
                }
                // Notification delivery can trail activation or destruction. The
                // actual foreground wins; its next event also reconciles entry.
                foreground_changed(window);
            }
            Some(0)
        }
        0x007e | 0x02e0 => {
            let _ = negotiate(window);
            Some(0)
        } // Display / DPI change.
        0x0047 => {
            let geometry_changed = if lparam == 0 {
                true
            } else {
                let position = unsafe {
                    &*(lparam as *const windows::Win32::UI::WindowsAndMessaging::WINDOWPOS)
                };
                position.flags.0 & 3 != 3
            };
            if !geometry_changed {
                return None;
            }
            unsafe {
                SHAppBarMessage(9, &mut data(window));
            }
            None
        } // WINDOWPOSCHANGED.
        0x0006 => {
            unsafe {
                SHAppBarMessage(6, &mut data(window));
            }
            None
        } // ACTIVATE.
        0x0002 => {
            unregister(window);
            None // Main fixture procedure still owns its normal destruction/exit.
        }
        _ => None,
    }
}
