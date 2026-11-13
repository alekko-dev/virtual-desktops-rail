//! Modeless desktop picker; switching remains on the existing integration path.
use crate::native::*;
use rail_core::layout::Desktop;
use std::ptr::{null, null_mut};
use windows::Win32::Foundation::{BOOL, HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};

const CLASS: &str = "VirtualDesktopRailDesktopPicker";

pub struct Popover {
    pub window: Handle,
    list: Handle,
    fonts: [Handle; 1],
    desktops: Vec<Desktop>,
    current: String,
}

impl Popover {
    pub fn show(owner: Handle, anchor: RECT, desktops: &[Desktop], current: &str) -> Option<Self> {
        unsafe {
            let instance = GetModuleHandleW(null());
            let class_name = wide(CLASS);
            RegisterClassExW(&WindowClass {
                size: std::mem::size_of::<WindowClass>() as u32,
                style: 0,
                procedure: Some(procedure),
                class_extra: 0,
                window_extra: 0,
                instance,
                icon: null_mut(),
                cursor: LoadCursorW(null_mut(), 32512usize as *const u16),
                background: 6usize as Handle,
                menu_name: null(),
                class_name: class_name.as_ptr(),
                small_icon: null_mut(),
            });
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
            let width = scale(280).min(monitor.rcWork.right - monitor.rcWork.left);
            let padding = scale(4);
            let row_height = scale(28);
            // Let ordinary inventories fit naturally. Only large lists scroll, capped
            // at 480 DIPs or two thirds of the monitor work area.
            let available = (monitor.rcWork.bottom - monitor.rcWork.top) * 2 / 3;
            let max_rows = ((available.min(scale(480)) - padding * 2) / row_height).max(1);
            let rows = (desktops.len() as i32).clamp(1, max_rows);
            let height = padding * 2 + row_height * rows;
            let scrolling = desktops.len() > rows as usize;
            let window = CreateWindowExW(
                0x00010088,
                class_name.as_ptr(),
                wide("Switch desktop").as_ptr(),
                0x82000000,
                (anchor.right + scale(6)).clamp(
                    monitor.rcWork.left,
                    (monitor.rcWork.right - width).max(monitor.rcWork.left),
                ),
                (anchor.top - scale(6)).clamp(
                    monitor.rcWork.top,
                    (monitor.rcWork.bottom - height).max(monitor.rcWork.top),
                ),
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
            let dark = BOOL(i32::from(crate::theme::dark()));
            let corner = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                HWND(window),
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark as *const _ as *const _,
                std::mem::size_of::<BOOL>() as u32,
            );
            let _ = DwmSetWindowAttribute(
                HWND(window),
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner as *const _ as *const _,
                std::mem::size_of::<u32>() as u32,
            );
            let make_font = |weight| {
                CreateFontW(
                    -scale(13),
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
            let fonts = [make_font(400)];
            SetWindowLongPtrW(
                window,
                -21,
                desktops
                    .iter()
                    .position(|d| d.id == current)
                    .map_or(0, |index| index as isize + 1),
            );
            // Native listbox supplies scrolling, keyboard navigation and accessibility;
            // owner drawing keeps the active desktop separate from hover/focus.
            let list = CreateWindowExW(
                0,
                wide("LISTBOX").as_ptr(),
                wide("Desktops").as_ptr(),
                // LBS_NOINTEGRALHEIGHT prevents creation-time font metrics from
                // rounding the list shorter before LB_SETITEMHEIGHT is applied.
                0x50010150 | if scrolling { 0x00200000 } else { 0 },
                padding,
                padding,
                width - padding * 2,
                row_height * rows,
                window,
                11usize as Handle,
                instance,
                null_mut(),
            );
            if list.is_null() {
                DestroyWindow(window);
                for font in fonts {
                    DeleteObject(font);
                }
                return None;
            }
            SendMessageW(list, 0x0030, fonts[0] as usize, 1);
            SendMessageW(list, 0x01a0, 0, row_height as isize); // LB_SETITEMHEIGHT
            for (index, desktop) in desktops.iter().enumerate() {
                let name = if desktop.name.is_empty() {
                    format!("Desktop {}", index + 1)
                } else {
                    desktop.name.clone()
                };
                // Position distinguishes duplicate names; the current desktop stays explicit
                // even when keyboard navigation moves the selection highlight.
                let label = format!(
                    " {}. {}{}",
                    index + 1,
                    name,
                    if desktop.id == current {
                        " (current)"
                    } else {
                        ""
                    }
                );
                SendMessageW(list, 0x0180, 0, wide(&label).as_ptr() as isize);
            }
            if let Some(index) = desktops.iter().position(|desktop| desktop.id == current) {
                SendMessageW(list, 0x0186, index, 0); // LB_SETCURSEL
            }
            ShowWindow(window, 5);
            let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(HWND(window));
            SetFocus(list);
            Some(Self {
                window,
                list,
                fonts,
                desktops: desktops.to_vec(),
                current: current.into(),
            })
        }
    }

    /// Dismiss stale inventories instead of silently reinterpreting list positions.
    pub fn is_current(&self, desktops: &[Desktop], current: Option<&str>, ready: bool) -> bool {
        ready && current == Some(self.current.as_str()) && self.desktops == desktops
    }

    /// Returns whether the message was consumed and a GUID selected by real input.
    pub fn input(&self, message: &mut Message) -> (bool, Option<String>) {
        use windows::Win32::UI::WindowsAndMessaging::{IsChild, IsWindowVisible};
        unsafe {
            if !IsWindowVisible(HWND(self.window)).as_bool() {
                return (false, None);
            }
            let inside = message.window == self.window
                || IsChild(HWND(self.window), HWND(message.window)).as_bool();
            if !inside {
                if matches!(message.message, 0x0201 | 0x0204 | 0x0207 | 0x00a1 | 0x00a4) {
                    ShowWindow(self.window, 0);
                }
                return (false, None);
            }
            if message.window == self.list && matches!(message.message, 0x0200 | 0x02a3) {
                let hover = if message.message == 0x0200 {
                    crate::ui::track_mouse_leave(self.list);
                    let hit = SendMessageW(self.list, 0x01a9, 0, message.lparam);
                    if hit >> 16 == 0 && (hit & 0xffff) < self.desktops.len() as isize {
                        (hit & 0xffff) + 1
                    } else {
                        0
                    }
                } else {
                    0
                };
                if GetWindowLongPtrW(self.list, -21) != hover {
                    SetWindowLongPtrW(self.list, -21, hover);
                    InvalidateRect(self.list, null(), 0);
                }
            }
            if message.message == 0x0100 && message.wparam == 0x1b {
                ShowWindow(self.window, 0);
                return (true, None);
            }
            let index = if message.message == 0x0202 && message.window == self.list {
                // Let the native control release its mouse capture before hiding it.
                SendMessageW(self.list, message.message, message.wparam, message.lparam);
                let hit = SendMessageW(self.list, 0x01a9, 0, message.lparam); // LB_ITEMFROMPOINT
                if hit >> 16 != 0 {
                    return (false, None);
                }
                Some((hit & 0xffff) as usize)
            } else if message.message == 0x0100 && message.wparam == 0x0d {
                Some(SendMessageW(self.list, 0x0188, 0, 0) as usize) // LB_GETCURSEL
            } else {
                None
            };
            if let Some(index) = index {
                ShowWindow(self.window, 0);
                return (
                    true,
                    self.desktops.get(index).map(|desktop| desktop.id.clone()),
                );
            }
            (
                IsDialogMessageW(self.window, message as *mut Message) != 0,
                None,
            )
        }
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
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::Graphics::Gdi::{
        COLOR_WINDOW, COLOR_WINDOWTEXT, GetSysColor, GetSysColorBrush, HDC, SetBkColor,
        SetTextColor,
    };
    match message {
        0x002b if lparam != 0 => unsafe {
            paint_row(window, &*(lparam as *const DrawItem));
            1
        },
        0x0134 | 0x0138 => unsafe {
            let dc = HDC(wparam as *mut _);
            let color = if message == 0x0138 && !crate::ui::high_contrast() {
                0x0058524f
            } else {
                GetSysColor(COLOR_WINDOWTEXT)
            };
            SetTextColor(dc, COLORREF(color));
            SetBkColor(dc, COLORREF(GetSysColor(COLOR_WINDOW)));
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
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

/// Current desktop retains the selected colors independently of pointer and keyboard focus.
unsafe fn paint_row(window: Handle, item: &DrawItem) {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::Graphics::Gdi::{
        COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT, DT_END_ELLIPSIS,
        DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DrawTextW, GetSysColor, HDC, SetBkMode,
        SetTextColor, TRANSPARENT,
    };
    if item.item_id == u32::MAX {
        return;
    }
    unsafe {
        let active = GetWindowLongPtrW(window, -21) == item.item_id as isize + 1;
        let hovered = GetWindowLongPtrW(item.window, -21) == item.item_id as isize + 1;
        let background = GetSysColor(if active {
            COLOR_HIGHLIGHT
        } else {
            COLOR_WINDOW
        });
        let text = GetSysColor(if active {
            COLOR_HIGHLIGHTTEXT
        } else {
            COLOR_WINDOWTEXT
        });
        let background = if hovered && !active {
            // Neutral tint follows the system palette, including dark/high-contrast colors.
            let blend = |shift: u32| {
                let bg = (background >> shift) & 255u32;
                let fg = (text >> shift) & 255u32;
                (bg * 15u32 + fg) / 16u32
            };
            blend(0) | (blend(8) << 8) | (blend(16) << 16)
        } else {
            background
        };
        let brush = CreateSolidBrush(background);
        FillRect(item.dc, &item.rect, brush);
        DeleteObject(brush);
        let dc = HDC(item.dc);
        let font = SendMessageW(item.window, 0x0031, 0, 0) as Handle; // WM_GETFONT
        let previous_font = SelectObject(item.dc, font);
        SetTextColor(dc, COLORREF(text));
        SetBkMode(dc, TRANSPARENT);
        let len = SendMessageW(item.window, 0x018a, item.item_id as usize, 0);
        if (0..=1024).contains(&len) {
            let mut label = vec![0u16; len as usize + 1];
            SendMessageW(
                item.window,
                0x0189,
                item.item_id as usize,
                label.as_mut_ptr() as isize,
            );
            let mut rect = RECT {
                left: item.rect.left,
                top: item.rect.top,
                right: item.rect.right,
                bottom: item.rect.bottom,
            };
            DrawTextW(
                dc,
                &mut label[..len as usize],
                &mut rect,
                DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS,
            );
        }
        SelectObject(item.dc, previous_font);
        if !active && item.state & 0x0210 == 0x0010 {
            DrawFocusRect(item.dc, &item.rect);
        }
    }
}
