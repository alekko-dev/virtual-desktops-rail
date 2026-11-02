use std::ffi::c_void;
pub fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[repr(C)]
pub struct Paint {
    pub dc: Handle,
    pub erase: i32,
    pub rect: Rect,
    pub restore: i32,
    pub update: i32,
    pub reserved: [u8; 32],
}
#[repr(C)]
pub struct DrawItem {
    pub control_type: u32,
    pub control_id: u32,
    pub item_id: u32,
    pub action: u32,
    pub state: u32,
    pub window: Handle,
    pub dc: Handle,
    pub rect: Rect,
    pub data: usize,
}
#[repr(C)]
pub struct ToolInfo {
    pub size: u32,
    pub flags: u32,
    pub window: Handle,
    pub id: usize,
    pub rect: Rect,
    pub instance: Handle,
    pub text: *mut u16,
    pub parameter: isize,
    pub reserved: *mut c_void,
}
#[repr(C)]
pub struct CopyData {
    pub tag: usize,
    pub size: u32,
    pub data: *const u8,
}

#[link(name = "user32")]
unsafe extern "system" {
    pub fn DestroyWindow(window: Handle) -> i32;
    pub fn GetWindowLongPtrW(window: Handle, index: i32) -> isize;
    pub fn SetWindowLongPtrW(window: Handle, index: i32, value: isize) -> isize;
    pub fn IsDialogMessageW(window: Handle, message: *mut Message) -> i32;
    pub fn GetFocus() -> Handle;
    pub fn SetFocus(window: Handle) -> Handle;
    pub fn GetClientRect(window: Handle, rect: *mut Rect) -> i32;
    pub fn InvalidateRect(window: Handle, rect: *const Rect, erase: i32) -> i32;
    pub fn EnableWindow(window: Handle, enabled: i32) -> i32;
    pub fn BeginPaint(window: Handle, paint: *mut Paint) -> Handle;
    pub fn EndPaint(window: Handle, paint: *const Paint) -> i32;
    pub fn FillRect(dc: Handle, rect: *const Rect, brush: Handle) -> i32;
    pub fn DrawIconEx(
        dc: Handle,
        x: i32,
        y: i32,
        icon: Handle,
        width: i32,
        height: i32,
        step: u32,
        brush: Handle,
        flags: u32,
    ) -> i32;
    pub fn DrawFocusRect(dc: Handle, rect: *const Rect) -> i32;
    pub fn LoadIconW(instance: Handle, name: *const u16) -> Handle;
    pub fn DestroyIcon(icon: Handle) -> i32;
    pub fn SendMessageW(window: Handle, message: u32, wparam: usize, lparam: isize) -> isize;
    pub fn SetWindowTextW(window: Handle, text: *const u16) -> i32;
    pub fn RegisterWindowMessageW(name: *const u16) -> u32;
}
#[link(name = "gdi32")]
unsafe extern "system" {
    pub fn CreateSolidBrush(color: u32) -> Handle;
    pub fn DeleteObject(object: Handle) -> i32;
    pub fn SelectObject(dc: Handle, object: Handle) -> Handle;
    pub fn CreateCompatibleDC(dc: Handle) -> Handle;
    pub fn CreateCompatibleBitmap(dc: Handle, width: i32, height: i32) -> Handle;
    pub fn DeleteDC(dc: Handle) -> i32;
    pub fn BitBlt(
        target: Handle,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        source: Handle,
        sx: i32,
        sy: i32,
        operation: u32,
    ) -> i32;
    pub fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strike: u32,
        charset: u32,
        output: u32,
        clip: u32,
        quality: u32,
        pitch: u32,
        face: *const u16,
    ) -> Handle;
}
pub type Handle = *mut c_void;
#[repr(C)]
#[derive(Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
#[repr(C)]
pub struct Message {
    pub window: Handle,
    pub message: u32,
    pub wparam: usize,
    pub lparam: isize,
    pub time: u32,
    pub point: Point,
    pub private: u32,
}
#[repr(C)]
pub struct WindowClass {
    pub size: u32,
    pub style: u32,
    pub procedure: Option<unsafe extern "system" fn(Handle, u32, usize, isize) -> isize>,
    pub class_extra: i32,
    pub window_extra: i32,
    pub instance: Handle,
    pub icon: Handle,
    pub cursor: Handle,
    pub background: Handle,
    pub menu_name: *const u16,
    pub class_name: *const u16,
    pub small_icon: Handle,
}
#[link(name = "user32")]
unsafe extern "system" {
    pub fn RegisterClassExW(class: *const WindowClass) -> u16;
    pub fn CreateWindowExW(
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
    pub fn DefWindowProcW(window: Handle, message: u32, wparam: usize, lparam: isize) -> isize;
    pub fn ShowWindow(window: Handle, command: i32) -> i32;
    pub fn GetMessageW(message: *mut Message, window: Handle, min: u32, max: u32) -> i32;
    pub fn TranslateMessage(message: *const Message) -> i32;
    pub fn DispatchMessageW(message: *const Message) -> isize;
    pub fn PostQuitMessage(code: i32);
    pub fn GetForegroundWindow() -> Handle;
    pub fn GetDpiForWindow(window: Handle) -> u32;
    pub fn LoadCursorW(instance: Handle, name: *const u16) -> Handle;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    pub fn GetModuleHandleW(name: *const u16) -> Handle;
}
