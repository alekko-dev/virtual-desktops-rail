//! Per-user Windows app and shell color mode. Custom mixed mode preferences are remembered.
use crate::native::wide;
use std::ptr::null_mut;

const CURRENT_USER: isize = 0x80000001u32 as i32 as isize;
const PERSONALIZE: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
const APPS: &str = "AppsUseLightTheme";
const SYSTEM: &str = "SystemUsesLightTheme";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeOutcome {
    pub custom_system_for_light: Option<u32>,
    pub custom_system_for_dark: Option<u32>,
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

/// Resolves the next theme state and custom system preferences.
///
/// Current Windows convention:
/// - `apps == 1, system == 1`: Light mode
/// - `apps == 0, system == 0`: Dark mode
/// - `apps == 1, system == 0`: Custom (Light apps, dark Windows)
/// - `apps == 0, system == 1`: Custom (Dark apps, light Windows)
///
/// When switching, if current app mode has a custom system mode (e.g. apps=1, system=0),
/// remember that system mode for that app mode. When switching back to that app mode,
/// restore the remembered custom system mode. If the user set standard matching modes
/// (e.g. apps=1, system=1), clear any custom system mode for that app mode.
pub fn resolve_toggle(
    apps: u32,
    system: u32,
    mut custom_system_for_light: Option<u32>,
    mut custom_system_for_dark: Option<u32>,
) -> (u32, u32, Option<u32>, Option<u32>) {
    let apps_is_light = apps != 0;
    let system_is_light = system != 0;

    if apps_is_light {
        if !system_is_light {
            custom_system_for_light = Some(0);
        } else {
            custom_system_for_light = None;
        }
    } else if system_is_light {
        custom_system_for_dark = Some(1);
    } else {
        custom_system_for_dark = None;
    }

    let target_apps = u32::from(!apps_is_light);
    let target_system = if target_apps == 1 {
        custom_system_for_light.unwrap_or(1)
    } else {
        custom_system_for_dark.unwrap_or(0)
    };

    (
        target_apps,
        target_system,
        custom_system_for_light,
        custom_system_for_dark,
    )
}

fn toggle(
    custom_system_for_light: Option<u32>,
    custom_system_for_dark: Option<u32>,
) -> Result<ThemeOutcome, String> {
    let apps = read(APPS)?;
    let system = read(SYSTEM)?;
    let (target_apps, target_system, new_custom_for_light, new_custom_for_dark) = resolve_toggle(
        apps,
        system,
        custom_system_for_light,
        custom_system_for_dark,
    );

    write(SYSTEM, target_system)?;
    if let Err(error) = write(APPS, target_apps) {
        let rollback = write(SYSTEM, system);
        broadcast();
        return Err(match rollback {
            Ok(()) => error,
            Err(rollback) => format!("{error}; restoring system mode also failed: {rollback}"),
        });
    }
    let result = (|| {
        if read(APPS)? == target_apps && read(SYSTEM)? == target_system {
            Ok(ThemeOutcome {
                custom_system_for_light: new_custom_for_light,
                custom_system_for_dark: new_custom_for_dark,
            })
        } else {
            let _ = write(APPS, apps);
            let _ = write(SYSTEM, system);
            Err("Windows did not retain the requested color mode.".into())
        }
    })();
    broadcast();
    result
}

pub fn start_toggle(
    custom_system_for_light: Option<u32>,
    custom_system_for_dark: Option<u32>,
) -> Result<(), String> {
    std::thread::Builder::new()
        .name("windows-theme".into())
        .spawn(move || {
            crate::notify(crate::Event::Theme(toggle(
                custom_system_for_light,
                custom_system_for_dark,
            )))
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_apps_dark_windows_remembers_and_restores() {
        // Initial custom mode: Light apps (1) and Dark Windows (0)
        let (apps, system, custom_light, custom_dark) = resolve_toggle(1, 0, None, None);
        assert_eq!(apps, 0);
        assert_eq!(system, 0);
        assert_eq!(custom_light, Some(0));
        assert_eq!(custom_dark, None);

        // Switch back: from Dark apps (0) and Dark Windows (0)
        let (apps, system, custom_light, custom_dark) =
            resolve_toggle(0, 0, custom_light, custom_dark);
        assert_eq!(apps, 1);
        assert_eq!(system, 0); // Kept Windows panels dark!
        assert_eq!(custom_light, Some(0));
        assert_eq!(custom_dark, None);
    }

    #[test]
    fn dark_apps_light_windows_remembers_and_restores() {
        // Initial custom mode: Dark apps (0) and Light Windows (1)
        let (apps, system, custom_light, custom_dark) = resolve_toggle(0, 1, None, None);
        assert_eq!(apps, 1);
        assert_eq!(system, 1);
        assert_eq!(custom_light, None);
        assert_eq!(custom_dark, Some(1));

        // Switch back: from Light apps (1) and Light Windows (1)
        let (apps, system, custom_light, custom_dark) =
            resolve_toggle(1, 1, custom_light, custom_dark);
        assert_eq!(apps, 0);
        assert_eq!(system, 1); // Kept Windows panels light!
        assert_eq!(custom_light, None);
        assert_eq!(custom_dark, Some(1));
    }

    #[test]
    fn standard_modes_toggle_without_custom_system() {
        let (apps, system, custom_light, custom_dark) = resolve_toggle(1, 1, None, None);
        assert_eq!(apps, 0);
        assert_eq!(system, 0);
        assert_eq!(custom_light, None);
        assert_eq!(custom_dark, None);

        let (apps, system, custom_light, custom_dark) = resolve_toggle(0, 0, None, None);
        assert_eq!(apps, 1);
        assert_eq!(system, 1);
        assert_eq!(custom_light, None);
        assert_eq!(custom_dark, None);
    }

    #[test]
    fn explicit_standard_mode_clears_remembered_custom_system() {
        let custom_light = Some(0);
        let (apps, system, custom_light, _) = resolve_toggle(1, 1, custom_light, None);
        assert_eq!(apps, 0);
        assert_eq!(system, 0);
        assert_eq!(custom_light, None);

        let (apps, system, _, _) = resolve_toggle(0, 0, custom_light, None);
        assert_eq!(apps, 1);
        assert_eq!(system, 1);
    }
}
