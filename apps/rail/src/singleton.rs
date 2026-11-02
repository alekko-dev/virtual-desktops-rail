use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcess, OpenProcessToken, ReleaseMutex, WaitForSingleObject,
};

pub struct Instance(HANDLE);
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}
pub fn sid() -> Result<String, String> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
        .map_err(|e| e.to_string())?;
    let result = (|| {
        let mut needed = 0;
        let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut needed) };
        if needed == 0 || needed > 1024 * 1024 {
            return Err("Cannot obtain user identity".into());
        }
        let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                Some(buffer.as_mut_ptr() as *mut _),
                needed,
                &mut needed,
            )
        }
        .map_err(|e| e.to_string())?;
        let user = unsafe { &*(buffer.as_ptr() as *const TOKEN_USER) };
        let mut text = windows::core::PWSTR::null();
        unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) }.map_err(|e| e.to_string())?;
        let value = unsafe { text.to_string() }.map_err(|e| e.to_string());
        let _ = unsafe { LocalFree(HLOCAL(text.0 as *mut _)) };
        value
    })();
    let _ = unsafe { CloseHandle(token) };
    result
}
pub fn acquire() -> Result<Option<Instance>, String> {
    let (_, session) = rail_compatibility_probe::platform::process_context(std::process::id())?;
    acquire_named(
        &format!("Local\\VirtualDesktopRail-{}-{session}", sid()?),
        0,
    )
}
pub fn coordination(scope: &str) -> Result<Instance, String> {
    acquire_named(
        &format!("Global\\VirtualDesktopRail.{scope}-{}", sid()?),
        2000,
    )?
    .ok_or_else(|| {
        "Another session is using this preference file; the operation will be retried.".into()
    })
}
fn acquire_named(name: &str, timeout: u32) -> Result<Option<Instance>, String> {
    let name = crate::native::wide(name);
    let mutex = unsafe { CreateMutexW(None, false, windows::core::PCWSTR(name.as_ptr())) }
        .map_err(|e| e.to_string())?;
    match unsafe { WaitForSingleObject(mutex, timeout) }.0 {
        0 | 128 => Ok(Some(Instance(mutex))),
        258 => {
            let _ = unsafe { CloseHandle(mutex) };
            Ok(None)
        }
        _ => {
            let error = format!("Single-instance synchronization failed: {:?}", unsafe {
                GetLastError()
            });
            let _ = unsafe { CloseHandle(mutex) };
            Err(error)
        }
    }
}
