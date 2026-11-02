//! Opt-in, serialized desktop compatibility experiment. Never renames existing desktops.
use rail_compatibility_probe::{platform, subprocess::run_bounded};
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::{Duration, Instant};
use windows::core::GUID;
use winvd::{Desktop, DesktopEvent};
#[path = "../exercise/appbar.rs"]
mod appbar;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Operation {
    Snapshot,
    Create,
    Rename,
    Switch,
    Remove,
    CloseAppBar,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Request {
    operation: Operation,
    desktop: Option<String>,
    fallback: Option<String>,
    name: Option<String>,
    appbar_pid: Option<u32>,
}

impl Request {
    fn new(operation: Operation) -> Self {
        Self {
            operation,
            desktop: None,
            fallback: None,
            name: None,
            appbar_pid: None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct DesktopRecord {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct Event {
    kind: String,
    desktop: String,
    previous: Option<String>,
    received_ms: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct Stage {
    request: Request,
    affected_desktop: Option<String>,
    current_desktop: String,
    desktops: Vec<DesktopRecord>,
    events: Vec<Event>,
    expected_event_seen: bool,
    elapsed_ms: u64,
    work_area: [i32; 4],
    appbar: Option<appbar::Observation>,
}

fn guid(value: &str) -> Result<GUID, String> {
    if value.len() != 36
        || !value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
    {
        return Err("Invalid desktop GUID".into());
    }
    Ok(GUID::from(value))
}

fn id(desktop: Desktop) -> Result<String, String> {
    desktop
        .get_id()
        .map(|id| format!("{id:?}"))
        .map_err(|e| format!("{e:?}"))
}

fn desktop(request: &Request) -> Result<Desktop, String> {
    Ok(winvd::get_desktop(guid(
        request.desktop.as_deref().ok_or("Missing desktop")?,
    )?))
}

fn event(event: DesktopEvent, began: Instant) -> Result<Option<Event>, String> {
    let (kind, desktop, previous) = match event {
        DesktopEvent::DesktopCreated(d) => ("created", d, None),
        DesktopEvent::DesktopDestroyed {
            destroyed,
            fallback,
        } => ("destroyed", destroyed, Some(fallback)),
        DesktopEvent::DesktopChanged { new, old } => ("changed", new, Some(old)),
        DesktopEvent::DesktopNameChanged(d, _) => ("renamed", d, None),
        _ => return Ok(None),
    };
    Ok(Some(Event {
        kind: kind.into(),
        desktop: id(desktop)?,
        previous: previous.map(id).transpose()?,
        received_ms: began.elapsed().as_millis() as u64,
    }))
}

fn remove_if_empty(target: Desktop, fallback: Desktop) -> Result<(), String> {
    let target_id = id(target)?;
    if target_id == id(fallback)? {
        return Err("Refusing to remove the fallback desktop".into());
    }
    // A user may have opened/moved a window while the test ran. Preserve that desktop.
    // Shell views absent from the adapter cannot be assigned to a desktop; real query errors fail closed.
    for window in platform::all_window_handles()? {
        match winvd::get_desktop_by_window(window) {
            Ok(owner) if id(owner)? == target_id => {
                return Err(format!(
                    "Desktop {target_id} contains a window; leaving it intact"
                ));
            }
            Ok(_) | Err(winvd::Error::WindowNotFound) => {}
            Err(e) => {
                return Err(format!(
                    "Cannot establish that desktop {target_id} is empty: {e:?}"
                ));
            }
        }
    }
    winvd::remove_desktop(target, fallback).map_err(|e| format!("{e:?}"))
}

fn worker(request: Request) -> Result<Stage, String> {
    if !platform::os_version()?.meets_adapter_minimum() {
        return Err("Unsupported build; no shell calls made".into());
    }
    // Upstream starts registration asynchronously. A delay is not proof of registration:
    // the matching event below is the actual evidence, and absence fails the experiment.
    let (sender, receiver) = crossbeam_channel::bounded::<DesktopEvent>(128);
    let listener = winvd::listen_desktop_events(sender).map_err(|e| format!("{e:?}"))?;
    std::thread::sleep(Duration::from_millis(250));
    let previous = id(winvd::get_current_desktop().map_err(|e| format!("{e:?}"))?)?;
    let began = Instant::now();
    let (affected_desktop, expected) = match request.operation {
        Operation::Snapshot => (None, None),
        Operation::Create => (
            Some(id(winvd::create_desktop().map_err(|e| format!("{e:?}"))?)?),
            Some("created"),
        ),
        Operation::Rename => {
            let target = desktop(&request)?;
            target
                .set_name(request.name.as_deref().ok_or("Missing name")?)
                .map_err(|e| format!("{e:?}"))?;
            (Some(id(target)?), Some("renamed"))
        }
        Operation::Switch => {
            let target = desktop(&request)?;
            winvd::switch_desktop(target).map_err(|e| format!("{e:?}"))?;
            let target_id = id(target)?;
            let expected = (target_id != previous).then_some("changed");
            (Some(target_id), expected)
        }
        Operation::Remove => {
            let target = desktop(&request)?;
            let fallback = winvd::get_desktop(guid(
                request.fallback.as_deref().ok_or("Missing fallback")?,
            )?);
            remove_if_empty(target, fallback)?;
            (request.desktop.clone(), Some("destroyed"))
        }
        Operation::CloseAppBar => {
            appbar::close(
                request.appbar_pid.ok_or("Missing AppBar fixture PID")?,
                true,
            )?;
            (None, None)
        }
    };
    let mut events = Vec::new();
    let mut expected_event_seen = expected.is_none();
    let deadline = began + Duration::from_secs(3);
    while !expected_event_seen && Instant::now() < deadline && events.len() < 128 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if let Ok(incoming) = receiver.recv_timeout(remaining)
            && let Some(incoming) = event(incoming, began)?
        {
            expected_event_seen = Some(incoming.kind.as_str()) == expected
                && Some(&incoming.desktop) == affected_desktop.as_ref()
                && (incoming.kind != "changed" || incoming.previous.as_ref() == Some(&previous));
            events.push(incoming);
        }
    }
    let current_desktop = id(winvd::get_current_desktop().map_err(|e| format!("{e:?}"))?)?;
    let desktops = winvd::get_desktops()
        .map_err(|e| format!("{e:?}"))?
        .into_iter()
        .map(|d| {
            Ok(DesktopRecord {
                id: id(d)?,
                name: d.get_name().map_err(|e| format!("{e:?}"))?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    drop(listener); // The parent watchdog also bounds listener teardown.
    let appbar = if matches!(request.operation, Operation::CloseAppBar) {
        None
    } else {
        request.appbar_pid.map(appbar::inspect).transpose()?
    };
    Ok(Stage {
        request,
        affected_desktop,
        current_desktop,
        desktops,
        events,
        expected_event_seen,
        elapsed_ms: began.elapsed().as_millis() as u64,
        work_area: appbar::work_area()?,
        appbar,
    })
}

fn call(request: Request) -> Result<Stage, String> {
    use std::os::windows::process::CommandExt;
    let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    command
        .args([
            "--worker",
            &serde_json::to_string(&request).map_err(|e| e.to_string())?,
        ])
        .creation_flags(0x08000000);
    let bytes = run_bounded(command, Duration::from_secs(10))?;
    serde_json::from_slice::<Result<Stage, String>>(&bytes).map_err(|e| e.to_string())?
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    os: Option<rail_compatibility_probe::report::OsVersion>,
    product_certified: bool,
    stages: Vec<Stage>,
    error: Option<String>,
    cleanup_errors: Vec<String>,
    original_desktop: Option<String>,
    created_desktops: Vec<String>,
    maximized_probe: Option<appbar::MaximizedObservation>,
}

fn exercise(test_appbar: bool) -> Report {
    let mut report = Report {
        schema_version: 1,
        os: platform::os_version().ok(),
        product_certified: false,
        stages: vec![],
        error: None,
        cleanup_errors: vec![],
        original_desktop: None,
        created_desktops: vec![],
        maximized_probe: None,
    };
    let mut fixture = None;
    let result = (|| -> Result<(), String> {
        let baseline = call(Request::new(Operation::Snapshot))?;
        report.original_desktop = Some(baseline.current_desktop.clone());
        report.stages.push(baseline);
        if test_appbar {
            fixture = Some(appbar::launch(true)?);
            let mut snapshot = Request::new(Operation::Snapshot);
            snapshot.appbar_pid = fixture.as_ref().map(|child| child.0.id());
            let reserved = call(snapshot)?;
            let check = appbar::verify(
                reserved
                    .appbar
                    .as_ref()
                    .ok_or("Missing AppBar observation")?,
            );
            report.stages.push(reserved);
            check?;
            let maximized = appbar::maximize_probe()?;
            let fits = maximized.fits_work_area;
            report.maximized_probe = Some(maximized);
            if !fits {
                return Err("Maximized fixture did not fit beside the AppBar".into());
            }
        }
        for index in 1..=3 {
            let created = call(Request::new(Operation::Create))?;
            let created_id = created
                .affected_desktop
                .clone()
                .ok_or("Create returned no GUID")?;
            // Track ownership before checking the notification so a missing event still gets cleanup.
            report.created_desktops.push(created_id.clone());
            let received = created.expected_event_seen;
            report.stages.push(created);
            if !received {
                return Err("Desktop created but its matching event was not received".into());
            }
            let mut rename = Request::new(Operation::Rename);
            rename.desktop = Some(created_id.clone());
            rename.name = Some(format!("Rail compatibility {}-{index}", std::process::id()));
            let named = call(rename.clone())?;
            let matches = named
                .desktops
                .iter()
                .any(|d| d.id == created_id && Some(&d.name) == rename.name.as_ref());
            let received = named.expected_event_seen;
            report.stages.push(named);
            if !matches || !received {
                return Err("Desktop name or rename-event GUID did not match".into());
            }
        }
        let original = report.original_desktop.clone().unwrap();
        for target in report
            .created_desktops
            .clone()
            .into_iter()
            .chain([original])
        {
            let mut request = Request::new(Operation::Switch);
            request.desktop = Some(target.clone());
            request.appbar_pid = fixture.as_ref().map(|child| child.0.id());
            let switched = call(request)?;
            let appbar_check = switched.appbar.as_ref().map(appbar::verify).transpose();
            let matches = switched.current_desktop == target && switched.expected_event_seen;
            report.stages.push(switched);
            appbar_check?;
            if !matches {
                return Err("Switch/current GUID or old/new event GUID did not match".into());
            }
        }
        Ok(())
    })();
    report.error = result.err();
    if let Some(child) = fixture {
        let mut close = Request::new(Operation::CloseAppBar);
        close.appbar_pid = Some(child.0.id());
        match call(close) {
            Ok(stage) => report.stages.push(stage),
            Err(error) => report.cleanup_errors.push(format!("Close AppBar: {error}")),
        }
        drop(child);
    }
    if let Some(original) = &report.original_desktop {
        let mut restore = Request::new(Operation::Switch);
        restore.desktop = Some(original.clone());
        match call(restore) {
            Ok(stage) if stage.current_desktop == *original => report.stages.push(stage),
            Ok(_) => report
                .cleanup_errors
                .push("Original desktop was not restored".into()),
            Err(e) => report
                .cleanup_errors
                .push(format!("Restore original desktop: {e}")),
        }
        for target in report.created_desktops.iter().rev() {
            let mut remove = Request::new(Operation::Remove);
            remove.desktop = Some(target.clone());
            remove.fallback = Some(original.clone());
            match call(remove) {
                Ok(stage) if !stage.desktops.iter().any(|d| d.id == *target) => {
                    report.stages.push(stage)
                }
                Ok(_) => report
                    .cleanup_errors
                    .push(format!("Created desktop {target} remains")),
                Err(e) => report
                    .cleanup_errors
                    .push(format!("Remove created desktop {target}: {e}")),
            }
        }
        match call(Request::new(Operation::Snapshot)) {
            Ok(stage) => {
                let initial = &report.stages[0].desktops;
                if stage.current_desktop != *original
                    || stage.desktops.len() != initial.len()
                    || !initial.iter().all(|a| {
                        stage
                            .desktops
                            .iter()
                            .any(|b| a.id == b.id && a.name == b.name)
                    })
                {
                    report.cleanup_errors.push("Final desktop state differs from baseline; inspect preserved desktop GUIDs".into());
                }
                if stage.work_area != report.stages[0].work_area {
                    report
                        .cleanup_errors
                        .push("Original work area was not restored".into());
                }
                report.stages.push(stage);
            }
            Err(e) => report
                .cleanup_errors
                .push(format!("Final verification: {e}")),
        }
    }
    report
}

fn main() {
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] || args == ["-h"] {
        println!(
            "rail-desktop-exercise --run-desktop-fixture | --run-appbar-fixture\nCreates three temporary desktops, names only those desktops, switches through them and the original, checks GUID events, then restores the original desktop and removes empty created desktops.\n--run-appbar-fixture also launches a passive AppBar and an ordinary maximize fixture; checks reservation, DWM visibility across desktops and work-area cleanup.\nRun serially in an unlocked Explorer session. Existing desktop names are preserved. Not product certification."
        );
        return;
    }
    if args.len() == 2 && args[0] == "--worker" {
        let result = serde_json::from_str::<Request>(&args[1])
            .map_err(|e| e.to_string())
            .and_then(worker);
        println!("{}", serde_json::to_string(&result).unwrap());
        return;
    }
    if args != ["--run-desktop-fixture"] && args != ["--run-appbar-fixture"] {
        eprintln!("Use --help, --run-desktop-fixture or --run-appbar-fixture");
        std::process::exit(1);
    }
    let report = exercise(args == ["--run-appbar-fixture"]);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    std::process::exit(
        if report.error.is_none() && report.cleanup_errors.is_empty() {
            0
        } else {
            2
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_desktop_identity_is_rejected_without_panicking() {
        for value in [
            "",
            "not-a-guid",
            "7CC33836-BC98-4DEB-9005-E971C422CC9Z",
            "7CC338360BC98-4DEB-9005-E971C422CC93",
        ] {
            assert!(guid(value).is_err());
        }
        assert_eq!(
            format!(
                "{:?}",
                guid("7CC33836-BC98-4DEB-9005-E971C422CC93").unwrap()
            ),
            "7CC33836-BC98-4DEB-9005-E971C422CC93"
        );
    }
}
