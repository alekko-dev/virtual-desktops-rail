//! Dependency-free Win32 toolchain and interactive-session smoke check.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_os = "windows"))]
compile_error!("This fixture requires Windows.");

use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};

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
    if message == 2 {
        // WM_DESTROY: only this fixture's main window uses this class.
        unsafe {
            PostQuitMessage(0);
        }
        return 0;
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // All pointers reference live local values; Win32 retains only the registered
    // callback. Handles are used on this thread and messages outlive dispatch.
    unsafe {
        SetProcessDpiAwarenessContext((-4_isize) as Handle); // Per-monitor v2.
        if std::env::args().any(|argument| argument == "--probe") {
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
        let window = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            0x00cf0000,
            150,
            150,
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
            "Native x64 Rust / MSVC / Windows SDK check.\r\nMove, resize, minimize or maximize this window.\r\nRun a second instance for multiple-window testing.\r\nClose the window to exit.",
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
        ShowWindow(window, 5);
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
