//! Read-only OS and candidate discovery. No window text or application content.
use crate::report::OsVersion;
use std::mem::size_of;
use windows::Win32::Foundation::{BOOL, CloseHandle, FILETIME, HWND, LPARAM};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GWL_EXSTYLE, GWL_STYLE, GetClassNameW, GetWindow, GetWindowLongPtrW,
    GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, WS_CHILD, WS_EX_TOOLWINDOW,
};

#[repr(C)]
struct VersionInfo {
    size: u32,
    major: u32,
    minor: u32,
    build: u32,
    platform: u32,
    service_pack: [u16; 128],
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn RtlGetVersion(version: *mut VersionInfo) -> i32;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegGetValueW(
        key: isize,
        subkey: *const u16,
        value: *const u16,
        flags: u32,
        kind: *mut u32,
        data: *mut u32,
        size: *mut u32,
    ) -> i32;
}

pub fn os_version() -> Result<OsVersion, String> {
    let mut info = VersionInfo {
        size: size_of::<VersionInfo>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform: 0,
        service_pack: [0; 128],
    };
    // RtlGetVersion does not depend on a compatibility manifest.
    if unsafe { RtlGetVersion(&mut info) } != 0 {
        return Err("RtlGetVersion failed".into());
    }
    let subkey: Vec<u16> = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\0"
        .encode_utf16()
        .collect();
    let name: Vec<u16> = "UBR\0".encode_utf16().collect();
    let mut revision = 0;
    let mut size = 4;
    // HKLM is a sign-extended predefined HKEY. Require a DWORD; unknown means fail closed.
    let code = unsafe {
        RegGetValueW(
            0x80000002_u32 as i32 as isize,
            subkey.as_ptr(),
            name.as_ptr(),
            0x10,
            std::ptr::null_mut(),
            &mut revision,
            &mut size,
        )
    };
    if code != 0 {
        return Err(format!("Cannot read Windows update revision: {code}"));
    }
    Ok(OsVersion {
        major: info.major,
        minor: info.minor,
        build: info.build,
        revision,
    })
}

#[derive(Clone, Copy)]
pub struct CandidateIdentity {
    pub hwnd: HWND,
    pub pid: u32,
    pub created: u64,
    pub minimized: bool,
}

pub fn process_created(pid: u32) -> Option<u64> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut created = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let result = GetProcessTimes(process, &mut created, &mut exit, &mut kernel, &mut user);
        let _ = CloseHandle(process);
        result.ok()?;
        Some((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }
}

pub fn still_same_window(candidate: CandidateIdentity) -> bool {
    let mut pid = 0;
    unsafe {
        if !IsWindow(candidate.hwnd).as_bool() {
            return false;
        }
        GetWindowThreadProcessId(candidate.hwnd, Some(&mut pid));
    }
    pid == candidate.pid && process_created(pid) == Some(candidate.created)
}

pub fn candidates() -> Result<Vec<CandidateIdentity>, String> {
    unsafe extern "system" fn callback(hwnd: HWND, data: LPARAM) -> BOOL {
        // EnumWindows calls synchronously; the vector remains live until it returns.
        let result = unsafe { &mut *(data.0 as *mut Vec<CandidateIdentity>) };
        // Keep enumeration bounded without treating an intentional callback stop as an error.
        if result.len() >= 512 {
            return BOOL(1);
        }
        unsafe {
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            let extended = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            let mut class = [0u16; 256];
            let length = GetClassNameW(hwnd, &mut class) as usize;
            let class = String::from_utf16_lossy(&class[..length]);
            if pid == std::process::id()
                || !IsWindowVisible(hwnd).as_bool()
                || style & WS_CHILD.0 != 0
                || extended & WS_EX_TOOLWINDOW.0 != 0
                || GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid())
                || matches!(
                    class.as_str(),
                    "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
                )
            {
                return BOOL(1);
            }
            if let Some(created) = process_created(pid) {
                result.push(CandidateIdentity {
                    hwnd,
                    pid,
                    created,
                    minimized: IsIconic(hwnd).as_bool(),
                });
            }
        }
        BOOL(1)
    }
    let mut result = Vec::new();
    unsafe { EnumWindows(Some(callback), LPARAM(&mut result as *mut _ as isize)) }
        .map_err(|error| error.to_string())?;
    if result.len() >= 512 {
        return Err("Candidate limit reached; enumeration was incomplete".into());
    }
    Ok(result)
}
