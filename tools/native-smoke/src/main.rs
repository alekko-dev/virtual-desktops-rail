//! Dependency-free Win32 toolchain and interactive-session smoke check.
// Keep the console subsystem: --probe output must work in release builds too.

#[cfg(not(target_os = "windows"))]
compile_error!("This fixture requires Windows.");

use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

static WINDOW_COUNT: AtomicUsize = AtomicUsize::new(0);
static DELAY_MS: AtomicU64 = AtomicU64::new(2000);

#[derive(Debug, PartialEq, Eq)]
struct Options {
    probe: bool,
    windows: usize,
    delay_ms: u64,
}

fn options(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut args = args.into_iter();
    let mut result = Options {
        probe: false,
        windows: 1,
        delay_ms: 2000,
    };
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--probe" => result.probe = true,
            "--windows" => {
                result.windows = args
                    .next()
                    .ok_or("Missing --windows value")?
                    .parse()
                    .map_err(|_| "--windows must be 1..4")?;
                if !(1..=4).contains(&result.windows) {
                    return Err("--windows must be 1..4".into());
                }
            }
            "--delay-ms" => {
                result.delay_ms = args
                    .next()
                    .ok_or("Missing --delay-ms value")?
                    .parse()
                    .map_err(|_| "--delay-ms must be 0..10000")?;
                if result.delay_ms > 10000 {
                    return Err("--delay-ms must be 0..10000".into());
                }
            }
            _ => return Err(format!("Unknown argument: {argument}")),
        }
    }
    Ok(result)
}

type Handle = *mut c_void;
#[repr(C)]
#[derive(Default)]
struct Point {
    x: i32,
    y: i32,
}
#[repr(C)]
#[derive(Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[repr(C)]
struct Message {
    window: Handle,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    point: Point,
    private: u32,
}
#[repr(C)]
struct WindowClass {
    size: u32,
    style: u32,
    procedure: Option<unsafe extern "system" fn(Handle, u32, usize, isize) -> isize>,
    class_extra: i32,
    window_extra: i32,
    instance: Handle,
    icon: Handle,
    cursor: Handle,
    background: Handle,
    menu_name: *const u16,
    class_name: *const u16,
    small_icon: Handle,
}
#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassExW(class: *const WindowClass) -> u16;
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Handle,
        menu: Handle,
        instance: Handle,
        parameter: Handle,
    ) -> Handle;
    fn DefWindowProcW(window: Handle, message: u32, wparam: usize, lparam: isize) -> isize;
    fn ShowWindow(window: Handle, command: i32) -> i32;
    fn GetMessageW(message: *mut Message, window: Handle, min: u32, max: u32) -> i32;
    fn TranslateMessage(message: *const Message) -> i32;
    fn DispatchMessageW(message: *const Message) -> isize;
    fn PostQuitMessage(code: i32);
    fn GetForegroundWindow() -> Handle;
    fn GetWindowRect(window: Handle, rect: *mut Rect) -> i32;
    fn GetWindowThreadProcessId(window: Handle, process: *mut u32) -> u32;
    fn GetSystemMetrics(index: i32) -> i32;
    fn GetDpiForWindow(window: Handle) -> u32;
    fn SetProcessDpiAwarenessContext(context: Handle) -> i32;
    fn LoadCursorW(instance: Handle, name: *const u16) -> Handle;
    fn MessageBoxW(owner: Handle, text: *const u16, caption: *const u16, kind: u32) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Handle;
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

unsafe extern "system" fn procedure(
    window: Handle,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    if message == 1 {
        // Count only top-level windows registered with our custom class.
        WINDOW_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    if message == 0x0111 {
        match wparam & 0xffff {
            1 => {
                unsafe {
                    MessageBoxW(window,
                        wide("This modal dialog belongs to the fixture window.\r\nTest foreground membership and minimize-through-owner behavior.").as_ptr(),
                        wide("Rail fixture - Owned modal dialog").as_ptr(), 0);
                }
                return 0;
            }
            2 => {
                // Deliberate bounded stall on the fixture UI thread, activated by a test click.
                std::thread::sleep(std::time::Duration::from_millis(
                    DELAY_MS.load(Ordering::Relaxed),
                ));
                return 0;
            }
            _ => {}
        }
    }
    if message == 2 {
        // WM_DESTROY: closing one duplicate must not terminate the others.
        if WINDOW_COUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                PostQuitMessage(0);
            }
        }
        return 0;
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] || args == ["-h"] {
        println!(
            "rail-native-smoke [--probe] [--windows 1..4] [--delay-ms 0..10000]\n\
            --probe prints foreground-window JSON without creating a window.\n\
            The dialog button opens an owned modal; the delay button stalls the fixture UI thread."
        );
        return Ok(());
    }
    let options = options(args)?;
    DELAY_MS.store(options.delay_ms, Ordering::Relaxed);
    // All pointers reference live local values; Win32 retains only the registered
    // callback. Handles are used on this thread and messages outlive dispatch.
    unsafe {
        SetProcessDpiAwarenessContext((-4_isize) as Handle); // Per-monitor v2.
        if options.probe {
            let window = GetForegroundWindow();
            let mut rect = Rect::default();
            let mut process = 0;
            if window.is_null() || GetWindowRect(window, &mut rect) == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            GetWindowThreadProcessId(window, &mut process);
            println!(
                "{{\"foreground_hwnd\":{},\"process_id\":{},\"rect\":[{},{},{},{}],\"dpi\":{},\"primary_display\":[{},{}]}}",
                window as usize,
                process,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                GetDpiForWindow(window),
                GetSystemMetrics(0),
                GetSystemMetrics(1)
            );
            return Ok(());
        }
        let instance = GetModuleHandleW(null());
        let class_name = wide("RailNativeSmoke");
        let class = WindowClass {
            size: size_of::<WindowClass>() as u32,
            style: 0,
            procedure: Some(procedure),
            class_extra: 0,
            window_extra: 0,
            instance,
            icon: null_mut(),
            cursor: LoadCursorW(null_mut(), 32512_usize as *const u16),
            background: 6_usize as Handle,
            menu_name: null(),
            class_name: class_name.as_ptr(),
            small_icon: null_mut(),
        };
        if RegisterClassExW(&class) == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let title = wide("Virtual Desktop Rail - Native Smoke Fixture");
        for index in 0..options.windows {
            let window = CreateWindowExW(
                0,
                class_name.as_ptr(),
                title.as_ptr(),
                0x00cf0000,
                150 + index as i32 * 40,
                150 + index as i32 * 40,
                640,
                360,
                null_mut(),
                null_mut(),
                instance,
                null_mut(),
            );
            if window.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            let label = wide(
                "Native x64 Rust / MSVC / Windows SDK check.\r\nMove, resize, minimize or maximize this window.\r\nDuplicate windows share one process when using --windows.\r\nClose all fixture windows to exit.",
            );
            let control = CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                label.as_ptr(),
                0x50000000,
                20,
                20,
                580,
                140,
                window,
                null_mut(),
                instance,
                null_mut(),
            );
            if control.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            for (identifier, x, text) in [
                (1, 20, "Open owned modal dialog".to_owned()),
                (2, 300, format!("Delay responses ({} ms)", options.delay_ms)),
            ] {
                let button = CreateWindowExW(
                    0,
                    wide("BUTTON").as_ptr(),
                    wide(&text).as_ptr(),
                    0x50010000,
                    x,
                    170,
                    260,
                    36,
                    window,
                    identifier as Handle,
                    instance,
                    null_mut(),
                );
                if button.is_null() {
                    return Err(std::io::Error::last_os_error().into());
                }
            }
            ShowWindow(window, 5);
            // A hidden-console launcher supplies STARTUPINFO that overrides the
            // first ShowWindow. Subsequent calls honor our actual fixture state.
            ShowWindow(window, 5);
        }
        let mut message: Message = zeroed();
        loop {
            let result = GetMessageW(&mut message, null_mut(), 0, 0);
            if result == -1 {
                return Err(std::io::Error::last_os_error().into());
            }
            if result == 0 {
                break;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_rejects_unbounded_stalls_and_duplicate_counts() {
        let parse = |args: &[&str]| options(args.iter().map(|s| s.to_string()));
        assert_eq!(
            parse(&["--windows", "2", "--delay-ms", "500"]).unwrap(),
            Options {
                probe: false,
                windows: 2,
                delay_ms: 500
            }
        );
        assert!(parse(&["--windows", "0"]).is_err());
        assert!(parse(&["--windows", "5"]).is_err());
        assert!(parse(&["--delay-ms", "10001"]).is_err());
        assert!(parse(&["--delay-ms", "-1"]).is_err());
        assert!(parse(&["--delay-ms"]).is_err());
        assert!(parse(&["--unknown"]).is_err());
    }
}
