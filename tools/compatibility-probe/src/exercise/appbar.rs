use rail_compatibility_probe::subprocess::OwnedChild;
use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, IsWindow,
    IsWindowVisible, IsZoomed, PostMessageW, SW_MAXIMIZE, ShowWindowAsync, WM_CLOSE,
};
use windows::core::w;

fn rect(value: RECT) -> [i32; 4] {
    [value.left, value.top, value.right, value.bottom]
}

pub fn work_area() -> Result<[i32; 4], String> {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        GetMonitorInfoW(
            MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY),
            &mut info,
        )
    }
    .ok()
    .map_err(|e| e.to_string())?;
    Ok(rect(info.rcWork))
}

fn owned_window(pid: u32, appbar: bool) -> Result<HWND, String> {
    let window = unsafe {
        FindWindowW(
            if appbar {
                w!("RailAppBarSmoke")
            } else {
                w!("RailNativeSmoke")
            },
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    let mut actual = 0;
    unsafe {
        GetWindowThreadProcessId(window, Some(&mut actual));
    }
    if actual != pid {
        return Err("Fixture window belongs to a different process".into());
    }
    Ok(window)
}

pub fn launch(appbar: bool) -> Result<OwnedChild, String> {
    use std::os::windows::process::CommandExt;
    if unsafe {
        FindWindowW(
            if appbar {
                w!("RailAppBarSmoke")
            } else {
                w!("RailNativeSmoke")
            },
            None,
        )
    }
    .is_ok()
    {
        return Err("Close existing native fixtures before the AppBar experiment".into());
    }
    let executable = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name("rail-native-smoke.exe");
    let mut command = Command::new(executable);
    if appbar {
        command.arg("--appbar");
    }
    let child = OwnedChild(
        command
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while owned_window(child.0.id(), appbar).is_err() {
        if Instant::now() >= deadline {
            return Err("Fixture startup timed out".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(child)
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Observation {
    pub hwnd: usize,
    pub dpi: u32,
    pub rect: [i32; 4],
    pub work_area: [i32; 4],
    pub cloaked: u32,
    pub visible: bool,
    pub foreground: bool,
    pub window_pinned: Result<bool, String>,
    pub app_pinned: Result<bool, String>,
}

pub fn inspect(pid: u32) -> Result<Observation, String> {
    let window = owned_window(pid, true)?;
    // Let the shell's desktop transition settle before checking DWM cloaking.
    std::thread::sleep(Duration::from_millis(500));
    let mut bounds = RECT::default();
    let mut cloaked = 0u32;
    unsafe {
        GetWindowRect(window, &mut bounds).map_err(|e| e.to_string())?;
        DwmGetWindowAttribute(window, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4)
            .map_err(|e| e.to_string())?;
    }
    Ok(Observation {
        hwnd: window.0 as usize,
        dpi: unsafe { GetDpiForWindow(window) },
        rect: rect(bounds),
        work_area: work_area()?,
        cloaked,
        visible: unsafe { IsWindowVisible(window) }.as_bool(),
        foreground: unsafe { GetForegroundWindow() } == window,
        window_pinned: winvd::is_pinned_window(window).map_err(|e| format!("{e:?}")),
        app_pinned: winvd::is_pinned_app(window).map_err(|e| format!("{e:?}")),
    })
}

pub fn verify(observation: &Observation) -> Result<(), String> {
    let width = ((25 * observation.dpi + 48) / 96) as i32;
    if observation.cloaked != 0
        || !observation.visible
        || observation.foreground
        || observation.rect[2] - observation.rect[0] != width
        || observation.work_area[0] < observation.rect[2]
    {
        return Err(format!(
            "AppBar reservation, presence or passive focus check failed: {observation:?}"
        ));
    }
    Ok(())
}

pub fn close(pid: u32, appbar: bool) -> Result<(), String> {
    let window = owned_window(pid, appbar)?;
    unsafe { PostMessageW(window, WM_CLOSE, WPARAM(0), LPARAM(0)) }.map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(2);
    while unsafe { IsWindow(window) }.as_bool() {
        if Instant::now() >= deadline {
            return Err("Fixture did not close within two seconds".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct MaximizedObservation {
    pub frame: [i32; 4],
    pub work_area: [i32; 4],
    pub fits_work_area: bool,
}

pub fn maximize_probe() -> Result<MaximizedObservation, String> {
    let child = launch(false)?;
    let window = owned_window(child.0.id(), false)?;
    unsafe { ShowWindowAsync(window, SW_MAXIMIZE) }
        .ok()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(2);
    while !unsafe { IsZoomed(window) }.as_bool() {
        if Instant::now() >= deadline {
            return Err("Fixture did not maximize".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(300));
    let mut frame = RECT::default();
    unsafe {
        DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut frame as *mut _ as *mut _,
            std::mem::size_of::<RECT>() as u32,
        )
    }
    .map_err(|e| e.to_string())?;
    let frame = rect(frame);
    let work_area = work_area()?;
    let fits_work_area = frame[0] >= work_area[0]
        && frame[1] >= work_area[1]
        && frame[2] <= work_area[2]
        && frame[3] <= work_area[3];
    close(child.0.id(), false)?;
    Ok(MaximizedObservation {
        frame,
        work_area,
        fits_work_area,
    })
}
