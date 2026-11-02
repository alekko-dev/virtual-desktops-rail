//! Explicit, serialized pin/identity experiments on an owned native fixture only.
//! This does not certify the rail or operate on the user's applications.
use rail_compatibility_probe::{
    platform,
    subprocess::{OwnedChild, run_bounded},
};
use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::UI::WindowsAndMessaging::{GetPropW, RemovePropW, SetPropW};
use windows::core::w;

const TOKEN: windows::core::PCWSTR = w!("VirtualDesktopRail.CompatibilityToken.v1");

#[path = "../exercise/control.rs"]
mod control;
#[path = "../exercise/discovery.rs"]
mod discovery;

#[derive(Debug, Deserialize, Serialize)]
struct Window {
    hwnd: usize,
    process_id: u32,
    process_created: u64,
    token: usize,
    window_pinned: bool,
    application_pinned: bool,
    executable: String,
    session_id: u32,
}

#[derive(Debug, Deserialize, Serialize)]
struct Stage {
    operation: String,
    elapsed_ms: u64,
    windows: Vec<Window>,
    #[serde(skip_serializing_if = "Option::is_none")]
    discovery: Option<discovery::Report>,
    #[serde(skip_serializing_if = "Option::is_none")]
    control: Option<control::Report>,
    #[serde(skip_serializing_if = "Option::is_none")]
    identity_cleanup_verified: Option<bool>,
}

#[link(name = "bcrypt")]
unsafe extern "system" {
    fn BCryptGenRandom(
        algorithm: *mut std::ffi::c_void,
        data: *mut u8,
        length: u32,
        flags: u32,
    ) -> i32;
}

fn random_token() -> Result<usize, String> {
    let mut token = 0usize;
    while token == 0 || token == usize::MAX {
        // BCRYPT_USE_SYSTEM_PREFERRED_RNG. Values are opaque, never pointers to our memory.
        let status =
            unsafe { BCryptGenRandom(std::ptr::null_mut(), &mut token as *mut _ as *mut u8, 8, 2) };
        if status < 0 {
            return Err(format!("Random identity generation failed: {status}"));
        }
    }
    Ok(token)
}

fn fixture_windows(pid: u32) -> Result<Vec<platform::CandidateIdentity>, String> {
    let mut windows: Vec<_> = platform::candidates()?
        .into_iter()
        .filter(|window| window.pid == pid && window.class_name == "RailNativeSmoke")
        .collect();
    if windows.len() != 2 {
        return Err(format!(
            "Expected exactly two owned fixture windows, found {}",
            windows.len()
        ));
    }
    windows.sort_by_key(|window| window.hwnd.0 as usize);
    Ok(windows)
}

fn worker(pid: u32, operation: &str) -> Result<Stage, String> {
    let os = platform::os_version()?;
    if !os.meets_adapter_minimum() {
        return Err("Unsupported Windows build; shell COM was not invoked".into());
    }
    let began = Instant::now();
    let windows = fixture_windows(pid)?;
    for window in &windows {
        if !platform::still_same_window(window) {
            return Err("Fixture identity changed".into());
        }
    }
    let mut control = None;
    match operation {
        "assign-tokens" => {
            for window in &windows {
                if !unsafe { GetPropW(window.hwnd, TOKEN) }.is_invalid() {
                    return Err("Fixture already has a compatibility token".into());
                }
                unsafe { SetPropW(window.hwnd, TOKEN, HANDLE(random_token()? as *mut _)) }
                    .map_err(|e| e.to_string())?;
            }
        }
        "pin-window" => winvd::pin_window(windows[0].hwnd).map_err(|e| format!("{e:?}"))?,
        "unpin-window" => winvd::unpin_window(windows[0].hwnd).map_err(|e| format!("{e:?}"))?,
        "pin-app" => winvd::pin_app(windows[0].hwnd).map_err(|e| format!("{e:?}"))?,
        "unpin-app" => winvd::unpin_app(windows[0].hwnd).map_err(|e| format!("{e:?}"))?,
        "remove-tokens" => {
            for window in &windows {
                unsafe { RemovePropW(window.hwnd, TOKEN) }.map_err(|e| e.to_string())?;
            }
        }
        "observe" => {}
        op if control::handles(op) => control = Some(control::execute(&windows[0], op)?),
        _ => return Err("Unknown fixture operation".into()),
    }
    let mut records = Vec::new();
    for window in windows {
        let (executable, session_id) = platform::process_context(window.pid)?;
        let window_pinned = winvd::is_pinned_window(window.hwnd).map_err(|e| format!("{e:?}"))?;
        let application_pinned = winvd::is_pinned_app(window.hwnd).map_err(|e| format!("{e:?}"))?;
        if !platform::still_same_window(&window) {
            return Err("Fixture identity changed during query".into());
        }
        records.push(Window {
            hwnd: window.hwnd.0 as usize,
            process_id: window.pid,
            process_created: window.created,
            token: unsafe { GetPropW(window.hwnd, TOKEN) }.0 as usize,
            window_pinned,
            application_pinned,
            executable,
            session_id,
        });
    }
    records.sort_by_key(|window| window.hwnd);
    Ok(Stage {
        operation: operation.into(),
        elapsed_ms: began.elapsed().as_millis() as u64,
        windows: records,
        discovery: None,
        control,
        identity_cleanup_verified: None,
    })
}

fn call_worker(pid: u32, operation: &str) -> Result<Stage, String> {
    use std::os::windows::process::CommandExt;
    let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    command
        .args(["--worker", &pid.to_string(), operation])
        .creation_flags(0x08000000);
    let bytes = run_bounded(command, Duration::from_secs(10))?;
    serde_json::from_slice::<Result<Stage, String>>(&bytes).map_err(|e| e.to_string())?
}

fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn exercise(mode: &str) -> Result<Vec<Stage>, String> {
    use std::os::windows::process::CommandExt;
    require(
        platform::os_version()?.meets_adapter_minimum(),
        "Unsupported build; fixture was not launched",
    )?;
    require(
        !platform::candidates()?
            .iter()
            .any(|window| window.class_name == "RailNativeSmoke"),
        "Close existing native fixtures before running this serialized experiment",
    )?;
    let executable = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name("rail-native-smoke.exe");
    let mut fixture = OwnedChild(
        Command::new(executable)
            .args(["--windows", "2", "--delay-ms", "1500"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000)
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    let pid = fixture.0.id();
    let deadline = Instant::now() + Duration::from_secs(5);
    while fixture_windows(pid).is_err() {
        if Instant::now() >= deadline {
            return Err("Fixture did not expose two windows within five seconds".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let baseline = call_worker(pid, "observe")?;
    require(
        baseline
            .windows
            .iter()
            .all(|window| !window.window_pinned && !window.application_pinned),
        "Fixture was already pinned; preserving the existing preference and refusing experiment",
    )?;
    let result: Result<Vec<Stage>, String> = (|| {
        let mut stages = vec![baseline];
        stages.push(call_worker(pid, "assign-tokens")?);
        // A distinct worker process must recover the same independent tokens.
        stages.push(call_worker(pid, "observe")?);
        let assigned = &stages[1].windows;
        let recovered = &stages[2].windows;
        require(
            assigned[0].token != 0 && assigned[0].token != assigned[1].token,
            "Duplicate windows did not receive distinct nonzero tokens",
        )?;
        require(
            assigned.iter().zip(recovered).all(|(a, b)| {
                a.hwnd == b.hwnd
                    && a.process_created == b.process_created
                    && a.token == b.token
                    && a.executable == b.executable
                    && a.session_id == b.session_id
            }),
            "Tokens did not survive the first worker's exit",
        )?;
        for (operation, app, count) in [
            ("pin-window", false, Some(1)),
            ("unpin-window", false, Some(0)),
            // Windows may also report every app-pinned view as individually pinned.
            // The app query, rather than a mutually-exclusive interpretation, is authoritative here.
            ("pin-app", true, None),
            ("unpin-app", false, Some(0)),
        ] {
            let stage = call_worker(pid, operation)?;
            require(
                stage
                    .windows
                    .iter()
                    .all(|window| window.application_pinned == app),
                &format!("Application pin state did not converge: {stage:?}"),
            )?;
            require(
                count.is_none_or(|count| {
                    stage
                        .windows
                        .iter()
                        .filter(|window| window.window_pinned)
                        .count()
                        == count
                }),
                &format!("Individual window pin state did not converge: {stage:?}"),
            )?;
            stages.push(stage);
        }
        if mode == "--run-pin-discovery" || mode == "--record-task-view" {
            let discovery = discovery::exercise(pid, mode == "--record-task-view")?;
            let mut stage = call_worker(pid, "observe")?;
            stage.operation = if mode == "--record-task-view" {
                "task-view-recording"
            } else {
                "pin-discovery"
            }
            .into();
            stage.discovery = Some(discovery);
            stages.push(stage);
        }
        if mode == "--run-window-control" {
            for operation in [
                "open-modal",
                "minimize-owner",
                "restore-owner",
                "close-modal",
                "delayed-minimize",
                "restore-owner",
            ] {
                stages.push(call_worker(pid, operation)?);
            }
        }
        stages.push(call_worker(pid, "remove-tokens")?);
        require(
            stages
                .last()
                .unwrap()
                .windows
                .iter()
                .all(|window| window.token == 0),
            "Token removal failed",
        )?;
        if mode == "--run-identity-lifetime" {
            stages.push(call_worker(pid, "assign-tokens")?);
        }
        Ok(stages)
    })();
    // Always try to restore the fixture's original unpinned state, even after failure.
    // Every attempt is isolated and bounded; only the owned fixture is targeted.
    let mut cleanup = Vec::new();
    for operation in ["unpin-app", "unpin-window"] {
        if let Err(error) = call_worker(pid, operation) {
            cleanup.push(format!("{operation}: {error}"));
        }
    }
    match call_worker(pid, "observe") {
        Ok(stage)
            if stage
                .windows
                .iter()
                .all(|window| !window.window_pinned && !window.application_pinned) => {}
        Ok(stage) => cleanup.push(format!("Pinning was not restored: {stage:?}")),
        Err(error) => cleanup.push(format!("Cleanup verification: {error}")),
    }
    let identity_result = if mode == "--run-identity-lifetime" && result.is_ok() {
        (|| -> Result<Stage, String> {
            use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
            use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
            let records = &result.as_ref().unwrap().last().unwrap().windows;
            for record in records {
                let window = HWND(record.hwnd as *mut _);
                require(
                    unsafe { GetPropW(window, TOKEN) }.0 as usize == record.token
                        && platform::process_created(record.process_id)
                            == Some(record.process_created),
                    "Identity changed before owned-window close",
                )?;
                unsafe { PostMessageW(window, WM_CLOSE, WPARAM(0), LPARAM(0)) }
                    .map_err(|e| e.to_string())?;
            }
            let deadline = Instant::now() + Duration::from_secs(3);
            while fixture.0.try_wait().map_err(|e| e.to_string())?.is_none() {
                if Instant::now() >= deadline {
                    return Err("Fixture did not exit normally after closing its windows".into());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            require(
                records.iter().all(|record| {
                    unsafe { GetPropW(HWND(record.hwnd as *mut _), TOKEN) }
                        .0
                        .is_null()
                }),
                "A destroyed window retained a compatibility token",
            )?;
            let replacement = OwnedChild(
                Command::new(
                    std::env::current_exe()
                        .map_err(|e| e.to_string())?
                        .with_file_name("rail-native-smoke.exe"),
                )
                .args(["--windows", "2"])
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?,
            );
            let deadline = Instant::now() + Duration::from_secs(3);
            while fixture_windows(replacement.0.id()).is_err() {
                if Instant::now() >= deadline {
                    return Err("Replacement fixture did not start".into());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let mut stage = call_worker(replacement.0.id(), "observe")?;
            require(
                stage.windows.iter().all(|new| {
                    new.token == 0
                        && records.iter().all(|old| {
                            new.process_id != old.process_id
                                || new.process_created != old.process_created
                        })
                        && new.executable == records[0].executable
                        && new.session_id == records[0].session_id
                }),
                "Application restart did not establish independent unassociated windows",
            )?;
            stage.operation = "application-restart-unassociated".into();
            stage.identity_cleanup_verified = Some(true);
            Ok(stage)
        })()
    } else {
        Ok(Stage {
            operation: "identity-lifetime-not-requested".into(),
            elapsed_ms: 0,
            windows: vec![],
            discovery: None,
            control: None,
            identity_cleanup_verified: None,
        })
    };
    drop(fixture);
    if !cleanup.is_empty() {
        return Err(format!(
            "Experiment: {result:?}; cleanup failed: {}",
            cleanup.join("; ")
        ));
    }
    let mut stages = result?;
    if mode == "--run-identity-lifetime" {
        stages.push(identity_result?);
    }
    Ok(stages)
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] || args == ["-h"] {
        println!(
            "rail-compatibility-exercise --run-owned-fixture | --run-pin-discovery | --run-window-control\nRuns serialized window/app pin and cross-process identity experiments on two newly launched native fixture windows.\n--run-pin-discovery also measures a separate observer's one-second candidate reconciliation and idle CPU (about 42 seconds).\n--run-window-control also tests modal minimize/restore and async minimization during a 1500 ms fixture stall.\nTemporarily changes only fixture state; closes its fixture on exit. No desktop switching.\nRequires rail-native-smoke.exe alongside this executable. Not product certification; API mutations do not replace Task View tests."
        );
        return;
    }
    if args.len() == 3 && (args[0] == "--watch" || args[0] == "--watch-manual") {
        let result = args[1]
            .parse::<u32>()
            .map_err(|e| e.to_string())
            .and_then(|pid| {
                discovery::watch(
                    pid,
                    std::path::Path::new(&args[2]),
                    args[0] == "--watch-manual",
                )
            });
        println!("{}", serde_json::to_string(&result).unwrap());
        return;
    }
    if args.len() == 3 && args[0] == "--worker" {
        let result = args[1]
            .parse::<u32>()
            .map_err(|e| e.to_string())
            .and_then(|pid| worker(pid, &args[2]));
        println!("{}", serde_json::to_string(&result).unwrap());
        return;
    }
    if args.len() != 1
        || ![
            "--run-owned-fixture",
            "--run-pin-discovery",
            "--run-window-control",
            "--record-task-view",
            "--run-identity-lifetime",
        ]
        .contains(&args[0].as_str())
    {
        eprintln!("Use --help to choose an explicit fixture experiment or Task View recording");
        std::process::exit(1);
    }
    let result = exercise(&args[0]);
    let report = serde_json::json!({
        "schema_version": 1,
        "os": platform::os_version().ok(),
        "adapter": "winvd 0.0.49",
        "product_certified": false,
        "scope": "Owned native fixture pin queries, property tokens and optional pin-discovery, window-control, identity-lifetime or operator Task View experiments; no product certification.",
        "result": &result,
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    std::process::exit(if result.is_ok() { 0 } else { 2 });
}
