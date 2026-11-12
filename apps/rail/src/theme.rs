//! Per-user Windows app and shell color mode. No rail-specific preference is saved.
use crate::native::wide;
use std::ptr::null_mut;

const CURRENT_USER: isize = 0x80000001u32 as i32 as isize;
const PERSONALIZE: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
const APPS: &str = "AppsUseLightTheme";
const SYSTEM: &str = "SystemUsesLightTheme";

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
    fn RegSetKeyValueW(
        key: isize,
        subkey: *const u16,
        value: *const u16,
        kind: u32,
        data: *const u32,
        size: u32,
    ) -> i32;
}

fn read(name: &str) -> Result<u32, String> {
    let mut value = 1;
    let mut size = 4;
    let status = unsafe {
        RegGetValueW(
            CURRENT_USER,
            wide(PERSONALIZE).as_ptr(),
            wide(name).as_ptr(),
            0x10, // RRF_RT_REG_DWORD
            null_mut(),
            &mut value,
            &mut size,
        )
    };
    match status {
        0 => Ok(value),
        // Windows defaults to light when the preference is absent.
        2 => Ok(1),
        _ => Err(format!("Reading {name}: Windows error {status}")),
    }
}

fn write(name: &str, value: u32) -> Result<(), String> {
    let status = unsafe {
        RegSetKeyValueW(
            CURRENT_USER,
            wide(PERSONALIZE).as_ptr(),
            wide(name).as_ptr(),
            4, // REG_DWORD
            &value,
            4,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(format!("Writing {name}: Windows error {status}"))
    }
}

pub fn dark() -> bool {
    read(APPS).unwrap_or(1) == 0
}

pub fn system_dark() -> bool {
    read(SYSTEM).unwrap_or(1) == 0
}

fn broadcast() {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };
    let area = wide("ImmersiveColorSet");
    // A recipient can stall. Keep this synchronous broadcast on the worker so the
    // string remains alive until delivery completes without blocking rail input.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(area.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            200,
            None,
        );
    }
}

fn toggle(target: rail_core::LightSwitchTarget) -> Result<(), String> {
    match target {
        rail_core::LightSwitchTarget::Applications => {
            let apps = read(APPS)?;
            let target_val = u32::from(apps == 0);
            write(APPS, target_val)?;
            let result = if read(APPS)? == target_val {
                Ok(())
            } else {
                Err("Windows did not retain the requested application color mode.".into())
            };
            broadcast();
            result
        }
        rail_core::LightSwitchTarget::Windows => {
            let system = read(SYSTEM)?;
            let target_val = u32::from(system == 0);
            write(SYSTEM, target_val)?;
            let result = if read(SYSTEM)? == target_val {
                Ok(())
            } else {
                Err("Windows did not retain the requested Windows color mode.".into())
            };
            broadcast();
            result
        }
        rail_core::LightSwitchTarget::Both => {
            let apps = read(APPS)?;
            let system = read(SYSTEM)?;
            let target_val = u32::from(apps == 0);
            write(SYSTEM, target_val)?;
            if let Err(error) = write(APPS, target_val) {
                let rollback = write(SYSTEM, system);
                broadcast();
                return Err(match rollback {
                    Ok(()) => error,
                    Err(rollback) => {
                        format!("{error}; restoring system mode also failed: {rollback}")
                    }
                });
            }
            let result = (|| {
                if read(APPS)? == target_val && read(SYSTEM)? == target_val {
                    Ok(())
                } else {
                    Err("Windows did not retain the requested color mode.".into())
                }
            })();
            broadcast();
            result
        }
    }
}

pub fn start_toggle(target: rail_core::LightSwitchTarget) -> Result<(), String> {
    std::thread::Builder::new()
        .name("windows-theme".into())
        .spawn(move || crate::notify(crate::Event::Theme(toggle(target))))
        .map(|_| ())
        .map_err(|error| error.to_string())
}
