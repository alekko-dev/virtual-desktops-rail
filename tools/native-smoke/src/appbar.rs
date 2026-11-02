//! Minimal AppBar compatibility fixture, not the product UI.
use super::{Handle, Point, Rect, size_of, wide};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

const CALLBACK: u32 = 0x8001;
static REGISTERED: AtomicBool = AtomicBool::new(false);
static NEGOTIATING: AtomicBool = AtomicBool::new(false);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

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
                -1_isize as Handle,
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
    TASKBAR_CREATED.store(
        unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) },
        Ordering::Relaxed,
    );
    if unsafe { SHAppBarMessage(0, &mut data(window)) } == 0 {
        return Err(std::io::Error::other("AppBar registration failed"));
    }
    REGISTERED.store(true, Ordering::Relaxed);
    if let Err(error) = negotiate(window) {
        unsafe {
            SHAppBarMessage(1, &mut data(window));
        }
        REGISTERED.store(false, Ordering::Relaxed);
        return Err(error);
    }
    Ok(())
}

pub fn message(window: Handle, message: u32, wparam: usize, lparam: isize) -> Option<isize> {
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
                        0x13,
                    );
                }
            }
            Some(0)
        }
        0x007e | 0x02e0 => {
            let _ = negotiate(window);
            Some(0)
        } // Display / DPI change.
        0x0047 => {
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
            unsafe {
                SHAppBarMessage(1, &mut data(window));
            }
            REGISTERED.store(false, Ordering::Relaxed);
            None // Main fixture procedure still owns its normal destruction/exit.
        }
        _ => None,
    }
}
