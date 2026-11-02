use super::*;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GWL_STYLE, GetClassNameW, GetForegroundWindow, GetLastActivePopup,
    GetWindow, GetWindowLongPtrW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    PostMessageW, SMTO_ABORTIFHUNG, SMTO_BLOCK, SW_MINIMIZE, SW_RESTORE, SW_SHOWNOACTIVATE,
    SendMessageTimeoutW, SetForegroundWindow, ShowWindowAsync, WM_CLOSE, WM_COMMAND, WM_NULL,
    WS_DISABLED,
};

#[derive(Debug, Deserialize, Serialize)]
pub struct Report {
    operation: String,
    owner_hwnd: usize,
    active_popup_hwnd: usize,
    owner_minimized: bool,
    owner_enabled: bool,
    popup_visible: bool,
    foreground_hwnd: usize,
    focus_request_accepted: Option<bool>,
    focus_verified: Option<bool>,
    stalled_probe_responded: Option<bool>,
    stalled_probe_ms: Option<u64>,
    async_dispatch_ms: Option<u64>,
    verification_ms: u64,
}

pub fn handles(operation: &str) -> bool {
    [
        "open-modal",
        "minimize-owner",
        "restore-owner",
        "close-modal",
        "delayed-minimize",
        "start-delay",
    ]
    .contains(&operation)
}

fn enabled(window: HWND) -> bool {
    unsafe { GetWindowLongPtrW(window, GWL_STYLE) as u32 & WS_DISABLED.0 == 0 }
}

fn popup(candidate: &platform::CandidateIdentity) -> Result<HWND, String> {
    let mut popup = unsafe { GetLastActivePopup(candidate.hwnd) };
    if popup == candidate.hwnd && !enabled(candidate.hwnd) {
        // Hidden modals are omitted by GetLastActivePopup. This fallback is deliberately
        // specific to the fixture's unique directly-owned standard dialog.
        struct Search {
            owner: HWND,
            matches: Vec<HWND>,
        }
        unsafe extern "system" fn callback(window: HWND, parameter: LPARAM) -> BOOL {
            let search = unsafe { &mut *(parameter.0 as *mut Search) };
            if unsafe { GetWindow(window, GW_OWNER) }.is_ok_and(|owner| owner == search.owner) {
                let mut class = [0u16; 32];
                let length = unsafe { GetClassNameW(window, &mut class) } as usize;
                if String::from_utf16_lossy(&class[..length]) == "#32770" {
                    search.matches.push(window);
                }
            }
            BOOL(1)
        }
        let mut search = Search {
            owner: candidate.hwnd,
            matches: Vec::new(),
        };
        unsafe { EnumWindows(Some(callback), LPARAM(&mut search as *mut _ as isize)) }
            .map_err(|e| e.to_string())?;
        if search.matches.len() > 1 {
            return Err("Fixture has ambiguous owned dialogs".into());
        }
        if let Some(found) = search.matches.first() {
            popup = *found;
        }
    }
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(popup, Some(&mut pid));
    }
    if pid != candidate.pid {
        return Err("Active popup is outside the owned fixture process".into());
    }
    Ok(popup)
}

fn wait(mut condition: impl FnMut() -> bool) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !condition() {
        if Instant::now() >= deadline {
            return Err("Fixture action was not verified within three seconds".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

pub fn execute(candidate: &platform::CandidateIdentity, operation: &str) -> Result<Report, String> {
    if !platform::still_same_window(candidate) {
        return Err("Fixture identity changed before action".into());
    }
    let began = Instant::now();
    let owner = candidate.hwnd;
    let mut focus_request_accepted = None;
    let mut focus_verified = None;
    let mut stalled_probe_responded = None;
    let mut stalled_probe_ms = None;
    let mut async_dispatch_ms = None;
    match operation {
        "start-delay" => {
            unsafe { PostMessageW(owner, WM_COMMAND, WPARAM(2), LPARAM(0)) }
                .map_err(|e| e.to_string())?;
        }
        "open-modal" => {
            unsafe { PostMessageW(owner, WM_COMMAND, WPARAM(1), LPARAM(0)) }
                .map_err(|e| e.to_string())?;
            wait(|| !enabled(owner) && unsafe { GetLastActivePopup(owner) } != owner)?;
        }
        "minimize-owner" | "delayed-minimize" => {
            if operation == "delayed-minimize" {
                unsafe { PostMessageW(owner, WM_COMMAND, WPARAM(2), LPARAM(0)) }
                    .map_err(|e| e.to_string())?;
                std::thread::sleep(Duration::from_millis(100));
                let probe = Instant::now();
                let responded = unsafe {
                    SendMessageTimeoutW(
                        owner,
                        WM_NULL,
                        WPARAM(0),
                        LPARAM(0),
                        SMTO_ABORTIFHUNG | SMTO_BLOCK,
                        50,
                        None,
                    )
                }
                .0 != 0;
                stalled_probe_ms = Some(probe.elapsed().as_millis() as u64);
                stalled_probe_responded = Some(responded);
                if responded {
                    return Err("Fixture UI stall was not established by the bounded probe".into());
                }
            }
            let active = popup(candidate)?;
            let dispatch = Instant::now();
            unsafe { ShowWindowAsync(owner, SW_MINIMIZE) }
                .ok()
                .map_err(|e| e.to_string())?;
            async_dispatch_ms = Some(dispatch.elapsed().as_millis() as u64);
            wait(|| unsafe { IsIconic(owner) }.as_bool())?;
            if active != owner {
                wait(|| !unsafe { IsWindowVisible(active) }.as_bool())?;
            }
        }
        "restore-owner" => {
            let active = popup(candidate)?;
            let dispatch = Instant::now();
            unsafe { ShowWindowAsync(owner, SW_RESTORE) }
                .ok()
                .map_err(|e| e.to_string())?;
            async_dispatch_ms = Some(dispatch.elapsed().as_millis() as u64);
            wait(|| !unsafe { IsIconic(owner) }.as_bool())?;
            if !enabled(owner) && active == owner {
                return Err("Disabled owner has no controllable modal".into());
            }
            if active != owner {
                unsafe { ShowWindowAsync(active, SW_SHOWNOACTIVATE) }
                    .ok()
                    .map_err(|e| e.to_string())?;
            }
            wait(|| unsafe { IsWindowVisible(active) }.as_bool())?;
            // This experiment reports Windows' foreground decision. A synthetic action is
            // not proof that a real rail click receives foreground permission.
            focus_request_accepted = Some(unsafe { SetForegroundWindow(active) }.as_bool());
            let deadline = Instant::now() + Duration::from_millis(500);
            while unsafe { GetForegroundWindow() } != active && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            focus_verified = Some(unsafe { GetForegroundWindow() } == active);
        }
        "close-modal" => {
            let active = popup(candidate)?;
            if active == owner {
                return Err("No owned modal to close".into());
            }
            unsafe { PostMessageW(active, WM_CLOSE, WPARAM(0), LPARAM(0)) }
                .map_err(|e| e.to_string())?;
            wait(|| enabled(owner) && unsafe { GetLastActivePopup(owner) } == owner)?;
        }
        _ => return Err("Unknown control experiment".into()),
    }
    if !platform::still_same_window(candidate) {
        return Err("Fixture identity changed during action".into());
    }
    let active = popup(candidate)?;
    Ok(Report {
        operation: operation.into(),
        owner_hwnd: owner.0 as usize,
        active_popup_hwnd: active.0 as usize,
        owner_minimized: unsafe { IsIconic(owner) }.as_bool(),
        owner_enabled: enabled(owner),
        popup_visible: active != owner && unsafe { IsWindowVisible(active) }.as_bool(),
        foreground_hwnd: unsafe { GetForegroundWindow() }.0 as usize,
        focus_request_accepted,
        focus_verified,
        stalled_probe_responded,
        stalled_probe_ms,
        async_dispatch_ms,
        verification_ms: began.elapsed().as_millis() as u64,
    })
}
