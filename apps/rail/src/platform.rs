use rail_compatibility_probe::platform as diagnostic;
use rail_core::{Desktop, Identity, Layout, Snapshot, Window};
use std::collections::BTreeMap;
use windows::Win32::Foundation::{BOOL, HANDLE, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MONITORINFOEXW, MonitorFromWindow,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    GA_ROOTOWNER, GWL_STYLE, GetAncestor, GetLastActivePopup, GetPropW, GetWindowLongPtrW,
    GetWindowPlacement, GetWindowRect, GetWindowThreadProcessId, IsHungAppWindow, IsIconic,
    IsWindow, IsWindowVisible, SW_SHOWMAXIMIZED, SetPropW, WINDOWPLACEMENT, WPF_RESTORETOMAXIMIZED,
    WS_DISABLED,
};
use windows::core::w;

pub const TOKEN: windows::core::PCWSTR = w!("VirtualDesktopRail.Identity.v1");
const MODAL_HINT: windows::core::PCWSTR = w!("VirtualDesktopRail.ModalHint.v1");
pub fn tick() -> u64 {
    unsafe { GetTickCount64() }
}
pub fn hwnd(value: usize) -> HWND {
    HWND(value as *mut _)
}

// Only a presentation hint, never evidence of a desktop change.
fn is_shell_switcher_window(window: HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetShellWindow};
    unsafe {
        if window.is_invalid() {
            return false;
        }
        let mut class = [0u16; 128];
        let length = GetClassNameW(window, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..length]);
        if !matches!(
            class.as_str(),
            "XamlExplorerHostIslandWindow" | "MultitaskingViewFrame"
        ) {
            return false;
        }
        let shell = GetShellWindow();
        let (mut shell_pid, mut window_pid) = (0, 0);
        GetWindowThreadProcessId(shell, Some(&mut shell_pid));
        GetWindowThreadProcessId(window, Some(&mut window_pid));
        shell_pid != 0 && shell_pid == window_pid
    }
}

fn visible_shell_switcher(window: HWND) -> Option<usize> {
    use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
    unsafe {
        if !IsWindowVisible(window).as_bool() || !is_shell_switcher_window(window) {
            return None;
        }
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(
            window,
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as *mut _,
            size_of::<u32>() as u32,
        )
        .is_err()
            || cloaked != 0
        {
            return None;
        }
        let mut bounds = RECT::default();
        let mut monitor = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetWindowRect(window, &mut bounds).is_err()
            || !GetMonitorInfoW(
                MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            )
            .as_bool()
            || bounds.left > monitor.rcMonitor.left
            || bounds.top > monitor.rcMonitor.top
            || bounds.right < monitor.rcMonitor.right
            || bounds.bottom < monitor.rcMonitor.bottom
        {
            return None;
        }
        Some(window.0 as usize)
    }
}

/// Discover a switcher while it holds foreground. As soon as foreground leaves
/// Task View, return None so the rail restores immediately without lag.
pub fn shell_switcher_visible(_tracked: Option<usize>) -> Option<usize> {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    visible_shell_switcher(unsafe { GetForegroundWindow() })
}
pub fn supported(experimental: bool) -> Result<(), String> {
    let os = diagnostic::os_version()?;
    support_version(os, experimental)
}

fn support_version(
    os: rail_compatibility_probe::report::OsVersion,
    _experimental: bool,
) -> Result<(), String> {
    // Keep accepting the legacy preference, but eligibility now follows winvd's
    // minimum. Successful initialization and runtime queries are still required.
    if os.meets_adapter_minimum() {
        Ok(())
    } else {
        Err(format!(
            "Windows {}.{}.{}.{} does not meet the desktop integration minimum: Windows 11 24H2 build 26100.2605 or newer (Windows version 10.0).",
            os.major, os.minor, os.build, os.revision
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rail_compatibility_probe::report::OsVersion;

    #[test]
    fn integration_accepts_the_adapter_minimum_and_newer_builds_without_opt_in() {
        let minimum = OsVersion {
            major: 10,
            minor: 0,
            build: 26100,
            revision: 2605,
        };
        for os in [
            minimum,
            OsVersion {
                revision: 2606,
                ..minimum
            },
            OsVersion {
                build: 26200,
                revision: 0,
                ..minimum
            },
            OsVersion {
                build: 26300,
                revision: 9456,
                ..minimum
            },
            OsVersion {
                build: 26300,
                revision: 9818,
                ..minimum
            },
            OsVersion {
                build: 26301,
                revision: 0,
                ..minimum
            },
        ] {
            for experimental in [false, true] {
                assert!(support_version(os, experimental).is_ok(), "{os:?}");
            }
        }
    }

    #[test]
    fn legacy_opt_in_cannot_bypass_the_minimum_or_windows_version_guard() {
        let minimum = OsVersion {
            major: 10,
            minor: 0,
            build: 26100,
            revision: 2605,
        };
        for os in [
            OsVersion {
                build: 26099,
                revision: u32::MAX,
                ..minimum
            },
            OsVersion {
                revision: 2604,
                ..minimum
            },
            OsVersion {
                major: 6,
                ..minimum
            },
            OsVersion {
                minor: 1,
                ..minimum
            },
            OsVersion {
                major: 11,
                ..minimum
            },
        ] {
            for experimental in [false, true] {
                let error = support_version(os, experimental).unwrap_err();
                assert!(error.contains("26100.2605"));
            }
        }
    }
}

#[link(name = "bcrypt")]
unsafe extern "system" {
    fn BCryptGenRandom(
        algorithm: *mut std::ffi::c_void,
        data: *mut u8,
        size: u32,
        flags: u32,
    ) -> i32;
}

fn identity_token(window: HWND) -> Result<String, String> {
    let mut token = unsafe { GetPropW(window, TOKEN) }.0 as usize;
    if token == 0 {
        loop {
            if unsafe {
                BCryptGenRandom(std::ptr::null_mut(), &mut token as *mut _ as *mut u8, 8, 2)
            } < 0
            {
                return Err("Identity randomness unavailable".into());
            }
            if token != 0 && token != usize::MAX {
                break;
            }
        }
        unsafe { SetPropW(window, TOKEN, HANDLE(token as *mut _)) }.map_err(|e| e.to_string())?;
    }
    Ok(format!("{token:016x}"))
}

pub fn validate(window: &Window) -> bool {
    let handle = hwnd(window.hwnd);
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(handle, Some(&mut pid));
    }
    unsafe { IsWindow(handle) }.as_bool()
        && pid == window.pid
        && diagnostic::process_created(pid) == Some(window.identity.process_created)
        && format!("{:016x}", unsafe { GetPropW(handle, TOKEN) }.0 as usize)
            == window.identity.token
}

fn rect(rect: RECT) -> [i32; 4] {
    [rect.left, rect.top, rect.right, rect.bottom]
}
pub fn monitor(handle: HMONITOR) -> Result<(String, [i32; 4], [i32; 4], u32), String> {
    let mut info = MONITORINFOEXW {
        monitorInfo: MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe { GetMonitorInfoW(handle, &mut info as *mut _ as *mut MONITORINFO) }
        .ok()
        .map_err(|e| e.to_string())?;
    let length = info
        .szDevice
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(info.szDevice.len());
    let mut x = 96;
    let mut y = 96;
    let _ = unsafe { GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut x, &mut y) };
    Ok((
        String::from_utf16_lossy(&info.szDevice[..length]),
        rect(info.monitorInfo.rcMonitor),
        rect(info.monitorInfo.rcWork),
        x,
    ))
}

// Missing optional OS support must not become false snap evidence.
fn arranged(handle: HWND) -> Option<bool> {
    type Query = unsafe extern "system" fn(HWND) -> BOOL;
    static QUERY: std::sync::OnceLock<Option<Query>> = std::sync::OnceLock::new();
    QUERY
        .get_or_init(|| unsafe {
            use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
            let module = GetModuleHandleW(w!("user32.dll")).ok()?;
            GetProcAddress(module, windows::core::s!("IsWindowArranged")).map(|address| {
                std::mem::transmute::<unsafe extern "system" fn() -> isize, Query>(address)
            })
        })
        .map(|query| unsafe { query(handle) }.as_bool())
}

pub fn layout(handle: HWND) -> Result<Layout, String> {
    let mut placement = WINDOWPLACEMENT {
        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    unsafe { GetWindowPlacement(handle, &mut placement) }.map_err(|e| e.to_string())?;
    let (name, monitor_rect, work_area, _) =
        monitor(unsafe { MonitorFromWindow(handle, MONITOR_DEFAULTTONEAREST) })?;
    let visible = if unsafe { IsIconic(handle) }.as_bool() {
        None
    } else {
        let mut bounds = RECT::default();
        unsafe { GetWindowRect(handle, &mut bounds) }.map_err(|e| e.to_string())?;
        Some(rect(bounds))
    };
    Ok(Layout {
        visible,
        arranged: arranged(handle),
        normal: rect(placement.rcNormalPosition),
        maximized: placement.showCmd == SW_SHOWMAXIMIZED.0 as u32
            || (unsafe { IsIconic(handle) }.as_bool()
                && placement.flags.0 & WPF_RESTORETOMAXIMIZED.0 != 0),
        dpi: unsafe { GetDpiForWindow(handle) }.max(96),
        monitor: name,
        monitor_rect,
        work_area,
    })
}

pub fn adjusted(saved: &Layout, handle: HWND) -> Result<Layout, String> {
    unsafe extern "system" fn callback(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let monitors = unsafe { &mut *(data.0 as *mut Vec<isize>) };
        monitors.push(monitor.0 as isize);
        BOOL(1)
    }
    let mut handles = Vec::<isize>::new();
    unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(callback),
            LPARAM(&mut handles as *mut _ as isize),
        )
    }
    .ok()
    .map_err(|e| e.to_string())?;
    let mut destination = monitor(unsafe { MonitorFromWindow(handle, MONITOR_DEFAULTTONEAREST) })?;
    for handle in handles {
        let candidate = monitor(HMONITOR(handle as *mut _))?;
        if candidate.0 == saved.monitor {
            destination = candidate;
            break;
        }
    }
    Ok(saved.adjusted(&destination.0, destination.1, destination.2, destination.3))
}

pub fn popup(window: &Window) -> Option<HWND> {
    let handle = hwnd(window.hwnd);
    let active = unsafe { GetLastActivePopup(handle) };
    let candidate = if active != handle {
        active
    } else {
        HWND(unsafe { GetPropW(handle, MODAL_HINT) }.0)
    };
    if candidate.is_invalid() || candidate == handle {
        return None;
    }
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(candidate, Some(&mut pid));
    }
    if pid != window.pid
        || !unsafe { IsWindow(candidate) }.as_bool()
        || unsafe { GetAncestor(candidate, GA_ROOTOWNER) } != handle
    {
        return None;
    }
    let mut token = unsafe { GetPropW(candidate, TOKEN) }.0 as usize;
    if token == 0 && candidate == active && unsafe { IsWindowVisible(candidate) }.as_bool() {
        token = usize::from_str_radix(&identity_token(candidate).ok()?, 16).ok()?;
        let _ = unsafe { SetPropW(handle, MODAL_HINT, HANDLE(candidate.0)) };
    }
    if token == 0 {
        return None;
    }
    Some(candidate)
}

fn is_z_order_peer(window: HWND, exclude_owner: HWND, allow_shell_cloaked: bool) -> bool {
    use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetClassNameW, GetShellWindow, WS_CHILD, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST,
    };
    const DWM_CLOAKED_SHELL: u32 = 2;
    unsafe {
        if window.is_invalid()
            || window == exclude_owner
            || !IsWindowVisible(window).as_bool()
            || IsIconic(window).as_bool()
            || !IsWindow(window).as_bool()
        {
            return false;
        }
        let style = GetWindowLongPtrW(window, GWL_STYLE) as u32;
        let extended = GetWindowLongPtrW(window, GWL_EXSTYLE) as u32;
        if style & WS_CHILD.0 != 0
            || extended & (WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0 | WS_EX_NOACTIVATE.0) != 0
        {
            return false;
        }
        if !exclude_owner.is_invalid() && GetAncestor(window, GA_ROOTOWNER) == exclude_owner {
            return false;
        }
        if window == GetShellWindow() {
            return false;
        }
        let mut bounds = RECT::default();
        if GetWindowRect(window, &mut bounds).is_err()
            || bounds.right <= bounds.left
            || bounds.bottom <= bounds.top
        {
            return false;
        }
        let mut class = [0u16; 128];
        let length = GetClassNameW(window, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..length]);
        if matches!(
            class.as_str(),
            "Progman"
                | "WorkerW"
                | "Shell_TrayWnd"
                | "Shell_SecondaryTrayWnd"
                | "XamlExplorerHostIslandWindow"
                | "MultitaskingViewFrame"
        ) {
            return false;
        }
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(
            window,
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as *mut _,
            std::mem::size_of::<u32>() as u32,
        )
        .is_ok()
        {
            let allowed = if allow_shell_cloaked {
                DWM_CLOAKED_SHELL
            } else {
                0
            };
            if cloaked & !allowed != 0 {
                return false;
            }
        }
        true
    }
}

pub fn z_above(handle: HWND) -> Vec<usize> {
    use windows::Win32::UI::WindowsAndMessaging::{GW_HWNDPREV, GetWindow};
    if unsafe { IsIconic(handle) }.as_bool() {
        return Vec::new();
    }
    let mut above = Vec::new();
    let mut current = handle;
    for _ in 0..4096 {
        let Ok(prev) = (unsafe { GetWindow(current, GW_HWNDPREV) }) else {
            break;
        };
        if prev.is_invalid() || prev == current {
            break;
        }
        current = prev;
        if is_z_order_peer(current, handle, true) {
            let value = current.0 as usize;
            if !above.contains(&value) {
                above.push(value);
                if above.len() >= 16 {
                    break;
                }
            }
        }
    }
    above
}

pub fn is_z_order_target(handle: HWND) -> bool {
    is_z_order_peer(handle, HWND::default(), false)
}

pub fn is_below(handle: HWND, above: HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GW_HWNDPREV, GetWindow};
    if handle.is_invalid() || above.is_invalid() || handle == above {
        return false;
    }
    let mut current = handle;
    for _ in 0..4096 {
        let Ok(prev) = (unsafe { GetWindow(current, GW_HWNDPREV) }) else {
            break;
        };
        if prev.is_invalid() || prev == current {
            break;
        }
        if prev == above {
            return true;
        }
        current = prev;
    }
    false
}

pub fn capture(
    candidate: &diagnostic::CandidateIdentity,
    context: &(String, u32),
) -> Result<Option<Window>, String> {
    let token = match identity_token(candidate.hwnd) {
        Ok(token) => token,
        Err(_) => return Ok(None),
    };
    let mut window = Window {
        identity: Identity {
            token,
            process_created: candidate.created,
            executable: context.0.clone(),
            session_id: context.1,
        },
        hwnd: candidate.hwnd.0 as usize,
        pid: candidate.pid,
        minimized: unsafe { IsIconic(candidate.hwnd) }.as_bool(),
        responsive: !unsafe { IsHungAppWindow(candidate.hwnd) }.as_bool(),
        layout: layout(candidate.hwnd)?,
        popup: None,
        z_above: z_above(candidate.hwnd),
    };
    let active = unsafe { GetLastActivePopup(candidate.hwnd) };
    if active != candidate.hwnd && unsafe { IsWindowVisible(active) }.as_bool() {
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(active, Some(&mut pid));
        }
        if pid != candidate.pid || identity_token(active).is_err() {
            return Ok(None);
        }
        let _ = unsafe { SetPropW(candidate.hwnd, MODAL_HINT, HANDLE(active.0)) };
    }
    window.popup = popup(&window).map(|value| value.0 as usize);
    if unsafe { GetWindowLongPtrW(candidate.hwnd, GWL_STYLE) } as u32 & WS_DISABLED.0 != 0
        && window.popup.is_none()
    {
        return Ok(None);
    }
    Ok(validate(&window).then_some(window))
}

pub fn snapshot(
    parent: u32,
    candidates: &[diagnostic::CandidateIdentity],
    serial: u64,
) -> Result<Snapshot, String> {
    let desktops = winvd::get_desktops().map_err(|e| format!("Desktop enumeration: {e:?}"))?;
    if desktops.is_empty() || desktops.len() > 64 {
        return Err("Desktop enumeration exceeded its supported bounds".into());
    }
    let current = winvd::get_current_desktop()
        .and_then(|d| d.get_id())
        .map_err(|e| format!("{e:?}"))?;
    let records = desktops
        .into_iter()
        .map(|d| {
            Ok(Desktop {
                id: format!("{:?}", d.get_id().map_err(|e| format!("{e:?}"))?),
                name: d
                    .get_name()
                    .map_err(|e| format!("{e:?}"))?
                    .chars()
                    .take(512)
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if !records
        .iter()
        .any(|desktop| desktop.id == format!("{current:?}"))
    {
        return Err("Desktop changed during enumeration; retrying".into());
    }
    let mut contexts = BTreeMap::new();
    let mut windows = Vec::new();
    for candidate in candidates {
        if candidate.pid == parent || !diagnostic::still_same_window(candidate) {
            continue;
        }
        let window_pin = winvd::is_pinned_window(candidate.hwnd);
        let app_pin = winvd::is_pinned_app(candidate.hwnd);
        // A candidate can disappear while the shell answers either query. Do not
        // attribute its results to a destroyed or replaced window.
        if !diagnostic::still_same_window(candidate) {
            continue;
        }
        use rail_compatibility_probe::{pin::disposition, report::CandidateDisposition};
        match disposition(&window_pin, &app_pin) {
            CandidateDisposition::Unsupported => continue,
            CandidateDisposition::Observed => {
                if !matches!(window_pin, Ok(true)) && !matches!(app_pin, Ok(true)) {
                    continue;
                }
            }
            CandidateDisposition::QueryFailed => {
                return Err(format!(
                    "Pin eligibility unavailable: hwnd={:#x} pid={} created={} class={:?} window={window_pin:?} app={app_pin:?}",
                    candidate.hwnd.0 as usize,
                    candidate.pid,
                    candidate.created,
                    candidate.class_name.chars().take(256).collect::<String>(),
                ));
            }
        }
        if let std::collections::btree_map::Entry::Vacant(entry) = contexts.entry(candidate.pid) {
            let Ok(context) = diagnostic::process_context(candidate.pid) else {
                continue;
            };
            entry.insert(context);
        }
        if let Some(window) = capture(candidate, &contexts[&candidate.pid])? {
            windows.push(window);
        }
    }
    let after = winvd::get_current_desktop()
        .and_then(|d| d.get_id())
        .map_err(|e| format!("{e:?}"))?;
    if current != after {
        return Err("Desktop changed during enumeration; establishing a fresh baseline".into());
    }
    Ok(Snapshot {
        desktops: records,
        current: format!("{current:?}"),
        windows,
        tick: tick(),
        transition_serial: serial,
    })
}
