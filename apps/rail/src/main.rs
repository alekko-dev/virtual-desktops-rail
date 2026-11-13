#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
compile_error!("The rail requires Windows x64.");
mod actions;
mod appbar;
mod bridge;
mod icons;
mod metrics;
mod native;
mod platform;
mod rename;
mod singleton;
mod snap_notice;
mod store;
mod theme;
mod ui;
mod version;
use native::{Handle, Point, Rect, wide};
use rail_core::{Layout, Saved, Snapshot};
use std::mem::size_of;
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
const WAKE: u32 = 0x8050;
static EVENTS: OnceLock<crossbeam_channel::Sender<Event>> = OnceLock::new();
static MAIN_WINDOW: AtomicUsize = AtomicUsize::new(0);
static OVERRUN: AtomicBool = AtomicBool::new(false);

enum Event {
    Command(ui::Command),
    Renamed {
        token: u64,
        result: Result<(), String>,
    },
    Frame(Snapshot, Option<u64>, bool),
    Minimized(rail_core::MinimizeObservation),
    DispatchLatency(u64),
    Status(String, bool),
    Notice(String),
    SnapPreservation(Box<snap_notice::SnapNotice>),
    ObservedArrangement {
        request: Box<rail_core::Request>,
        z_above: Vec<usize>,
        layout: Layout,
        tick: u64,
    },
    Accepted {
        key: String,
        sequence: u64,
        acceptance: rail_core::Acceptance,
    },
    ActionResult {
        key: String,
        sequence: u64,
        tick: u64,
        outcome: rail_core::Outcome,
    },
    Captured {
        key: String,
        generation: u64,
        tick: u64,
        layout: Result<Layout, String>,
    },
    Loaded {
        saved: Saved,
        path: std::path::PathBuf,
        warning: Option<String>,
    },
    Stored(Result<(), String>),
    Report(Result<std::path::PathBuf, String>),
    Startup(bool, Result<(), String>),
    Theme(Result<(), String>),
    Icon(String, u32, usize),
    NativeWindow(u32, usize, u64),
}

fn notify(event: Event) {
    if let Some(sender) = EVENTS.get()
        && let Err(error) = sender.try_send(event)
    {
        if let Event::Icon(_, _, icon) = error.into_inner()
            && icon != 0
        {
            unsafe {
                native::DestroyIcon(icon as native::Handle);
            }
        }
        OVERRUN.store(true, Ordering::Release);
    }
    let window = MAIN_WINDOW.load(Ordering::Acquire);
    if window != 0 {
        let _ = unsafe {
            windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                platform::hwnd(window),
                WAKE,
                windows::Win32::Foundation::WPARAM(0),
                windows::Win32::Foundation::LPARAM(0),
            )
        };
    }
}

fn main() {
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    if (args.len() == 2 || (args.len() == 3 && args[2] == "--experimental-integration"))
        && args[0] == "--integration-worker"
    {
        let result = args[1]
            .parse::<u32>()
            .map_err(|e| e.to_string())
            .and_then(|parent| bridge::worker(parent, args.len() == 3));
        if let Err(error) = result {
            println!("{}", serde_json::json!({"Failure":error}));
        }
        return;
    }
    let result = run(args);
    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("Virtual Desktop Rail: {error}");
            std::process::exit(1);
        }
    }
}

fn run(args: Vec<String>) -> Result<i32, String> {
    if args == ["--version"] {
        println!("virtual-desktop-rail {}", version::label());
        return Ok(0);
    }
    if args == ["--version", "--json"] {
        println!("{}", version::info());
        return Ok(0);
    }
    if args == ["--help"] || args == ["-h"] {
        println!(
            "virtual-desktop-rail [--settings]\nBuild identity: --version [--json] (this executable; no running instance required).\nControls for an existing instance: --status, --pause, --resume, --retry, --exit.\nAuto-restore preference: --auto-restore on|off (persisted; Settings also offers this option).\nLight switch target: --light-switch apps|windows|both (persisted; Settings also offers this option).\nLegacy preference: --experimental-integration on|off (persisted for compatibility; no longer affects build eligibility).\nDiagnostic controls: --click WINDOW_KEY, --switch DESKTOP_GUID, --automation-ui on|off.\nAutomation UI temporarily exposes the passive rail to desktop test tools; restore off after testing.\nRuns locally as the signed-in user. Desktop integration requires Windows 11 24H2 build 26100.2605 or newer and successful runtime checks."
        );
        return Ok(0);
    }
    if args == ["--status"] {
        let nonce = format!(
            "{:032x}",
            windows::core::GUID::new()
                .map_err(|e| e.to_string())?
                .to_u128()
        );
        let path = store::export_path(&nonce)?;
        if !ui::remote(&ui::Command::Status { nonce })? {
            return Err("The rail is not running.".into());
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !path.exists() {
            if std::time::Instant::now() >= deadline {
                return Err("Diagnostic output timed out.".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&path);
        println!("{}", String::from_utf8(bytes).map_err(|e| e.to_string())?);
        return Ok(0);
    }
    let command = match args.as_slice() {
        [arg] if arg == "--pause" => Some(ui::Command::Pause),
        [arg] if arg == "--resume" => Some(ui::Command::Resume),
        [arg] if arg == "--retry" => Some(ui::Command::Retry),
        [arg] if arg == "--exit" => Some(ui::Command::Exit),
        [arg, key] if arg == "--click" => Some(ui::Command::Click { key: key.clone() }),
        [arg, guid] if arg == "--switch" => Some(ui::Command::Switch { guid: guid.clone() }),
        [arg, value] if arg == "--auto-restore" && ["on", "off"].contains(&value.as_str()) => {
            Some(ui::Command::AutoRestore {
                enabled: value == "on",
            })
        }
        [arg, value]
            if arg == "--light-switch"
                && ["apps", "applications", "windows", "both"].contains(&value.as_str()) =>
        {
            let target = match value.as_str() {
                "apps" | "applications" => rail_core::LightSwitchTarget::Applications,
                "windows" => rail_core::LightSwitchTarget::Windows,
                _ => rail_core::LightSwitchTarget::Both,
            };
            Some(ui::Command::LightSwitchTarget { target })
        }
        [arg, value] if arg == "--automation-ui" && ["on", "off"].contains(&value.as_str()) => {
            Some(ui::Command::AutomationUi {
                enabled: value == "on",
            })
        }
        [arg, value]
            if arg == "--experimental-integration" && ["on", "off"].contains(&value.as_str()) =>
        {
            Some(ui::Command::ExperimentalIntegration {
                enabled: value == "on",
            })
        }
        _ => None,
    };
    if let Some(command) = command {
        if !ui::remote(&command)? {
            return Err("The rail is not running.".into());
        }
        return Ok(0);
    }
    let settings = args == ["--settings"];
    if !args.is_empty() && !settings {
        return Err("Unknown arguments; use --help.".into());
    }
    if let Some(_instance) = singleton::acquire()? {
        ui::run(settings)?;
    } else if settings {
        let _ = ui::remote(&ui::Command::Settings)?;
    }
    Ok(0)
}
