use std::mem::size_of;
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn owner() -> windows::Win32::Foundation::HWND {
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};
    let title = wide("Virtual Desktop Rail — Settings");
    let window =
        unsafe { FindWindowW(None, windows::core::PCWSTR(title.as_ptr())) }.unwrap_or_default();
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(window, Some(&mut pid));
    }
    if pid == std::process::id() {
        window
    } else {
        Default::default()
    }
}
pub fn save() -> Result<Option<String>, String> {
    use windows::Win32::UI::Controls::Dialogs::{
        CommDlgExtendedError, GetSaveFileNameW, OFN_EXPLORER, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT,
        OFN_PATHMUSTEXIST, OPEN_FILENAME_FLAGS, OPENFILENAMEW,
    };
    use windows::core::{PCWSTR, PWSTR};

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut filename = {
        let mut buffer = vec![0u16; 32768];
        let name = wide(&format!("VirtualDesktopRail-diagnostic-{timestamp}.zip"));
        buffer[..name.len()].copy_from_slice(&name);
        buffer
    };
    let filter: Vec<u16> = "ZIP archive (*.zip)\0*.zip\0All files (*.*)\0*.*\0\0"
        .encode_utf16()
        .collect();
    let title = wide("Save diagnostic report");
    let extension = wide("zip");
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner(),
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(filename.as_mut_ptr()),
        nMaxFile: filename.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        lpstrDefExt: PCWSTR(extension.as_ptr()),
        Flags: OPEN_FILENAME_FLAGS(
            OFN_EXPLORER.0 | OFN_NOCHANGEDIR.0 | OFN_OVERWRITEPROMPT.0 | OFN_PATHMUSTEXIST.0,
        ),
        ..Default::default()
    };
    if !unsafe { GetSaveFileNameW(&mut dialog) }.as_bool() {
        let error = unsafe { CommDlgExtendedError() };
        return if error.0 == 0 {
            Ok(None)
        } else {
            Err(format!("Windows common-dialog error 0x{:04x}", error.0))
        };
    }
    let Some(end) = filename.iter().position(|value| *value == 0) else {
        return Err("The selected path was not terminated.".into());
    };
    Ok(Some(String::from_utf16_lossy(&filename[..end])))
}
