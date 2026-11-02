use super::*;
use std::path::Path;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn QueryPerformanceCounter(counter: *mut i64) -> i32;
    fn QueryPerformanceFrequency(frequency: *mut i64) -> i32;
}

fn clock_ms() -> Result<u64, String> {
    let mut counter = 0i64;
    let mut frequency = 0i64;
    if unsafe { QueryPerformanceCounter(&mut counter) } == 0
        || unsafe { QueryPerformanceFrequency(&mut frequency) } == 0
        || frequency <= 0
        || counter < 0
    {
        return Err("Monotonic performance clock failed".into());
    }
    Ok((counter as u128 * 1000 / frequency as u128) as u64)
}

fn cpu_100ns() -> Result<u64, String> {
    let mut created = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    }
    .map_err(|e| e.to_string())?;
    let ticks =
        |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    Ok(ticks(kernel) + ticks(user))
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Sample {
    began_ms: u64,
    observed_ms: u64,
    states: Vec<(usize, bool, bool)>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Notification {
    observed_ms: u64,
    kind: String,
    hwnd: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Observation {
    interval_ms: u64,
    baseline_wall_ms: u64,
    baseline_cpu_ms: f64,
    total_wall_ms: u64,
    total_cpu_ms: f64,
    samples: Vec<Sample>,
    notifications: Vec<Notification>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Transition {
    operation: String,
    requested_ms: u64,
    completed_ms: u64,
    detected_ms: Option<u64>,
    conservative_latency_ms: Option<u64>,
    expected: Vec<(usize, bool, bool)>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Report {
    passed: bool,
    limit_ms: u64,
    mutation_source: String,
    transitions: Vec<Transition>,
    observation: Observation,
    manual_coverage: Option<ManualCoverage>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ManualCoverage {
    window_pin_then_unpin: bool,
    app_pin_then_unpin: bool,
    note: String,
}

pub fn watch(pid: u32, ready: &Path, manual: bool) -> Result<Observation, String> {
    if !platform::os_version()?.meets_adapter_minimum() {
        return Err("Unsupported build".into());
    }
    let candidates = fixture_windows(pid)?;
    let (sender, receiver) = crossbeam_channel::bounded::<winvd::DesktopEvent>(128);
    let listener = winvd::listen_desktop_events(sender).map_err(|e| format!("{e:?}"))?;
    let began = Instant::now();
    let cpu_start = cpu_100ns()?;
    let mut warm_cpu = cpu_start;
    let mut warm_wall = began;
    let mut observation = Observation {
        interval_ms: 1000,
        baseline_wall_ms: 0,
        baseline_cpu_ms: 0.0,
        total_wall_ms: 0,
        total_cpu_ms: 0.0,
        samples: vec![],
        notifications: vec![],
    };
    for index in 0..if manual { 120 } else { 42 } {
        let next = began + Duration::from_secs(index);
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
        let sample_start = clock_ms()?;
        let mut states = Vec::new();
        for candidate in &candidates {
            if !platform::still_same_window(candidate) {
                return Err("Cached fixture identity changed".into());
            }
            let window = winvd::is_pinned_window(candidate.hwnd).map_err(|e| format!("{e:?}"))?;
            let app = winvd::is_pinned_app(candidate.hwnd).map_err(|e| format!("{e:?}"))?;
            if !platform::still_same_window(candidate) {
                return Err("Fixture identity changed during discovery query".into());
            }
            states.push((candidate.hwnd.0 as usize, window, app));
        }
        observation.samples.push(Sample {
            began_ms: sample_start,
            observed_ms: clock_ms()?,
            states,
        });
        for event in receiver.try_iter() {
            let (kind, hwnd) = match event {
                winvd::DesktopEvent::WindowChanged(hwnd) => {
                    ("window_changed", Some(hwnd.0 as usize))
                }
                _ => ("other_desktop_event", None),
            };
            if observation.notifications.len() == 256 {
                return Err("Notification observation limit reached".into());
            }
            observation.notifications.push(Notification {
                observed_ms: clock_ms()?,
                kind: kind.into(),
                hwnd,
            });
        }
        if index == 1 {
            warm_cpu = cpu_100ns()?;
            warm_wall = Instant::now();
        }
        if index == if manual { 1 } else { 30 } {
            observation.baseline_wall_ms = warm_wall.elapsed().as_millis() as u64;
            observation.baseline_cpu_ms = (cpu_100ns()? - warm_cpu) as f64 / 10_000.0;
            // Exclusive creation avoids overwriting a path supplied to the internal worker.
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(ready)
                .map_err(|e| e.to_string())?;
        }
    }
    observation.total_wall_ms = began.elapsed().as_millis() as u64;
    observation.total_cpu_ms = (cpu_100ns()? - cpu_start) as f64 / 10_000.0;
    drop(listener);
    Ok(observation)
}

pub fn exercise(pid: u32, manual: bool) -> Result<Report, String> {
    use std::os::windows::process::CommandExt;
    let directory = std::env::temp_dir().join(format!(
        "rail-discovery-{:?}",
        windows::core::GUID::new().map_err(|e| e.to_string())?
    ));
    std::fs::create_dir(&directory).map_err(|e| e.to_string())?;
    let ready = directory.join("ready");
    let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    command
        .arg(if manual { "--watch-manual" } else { "--watch" })
        .arg(pid.to_string())
        .arg(&ready)
        .creation_flags(0x08000000);
    let observer = std::thread::spawn(move || {
        run_bounded(command, Duration::from_secs(if manual { 130 } else { 50 }))
    });
    let result = (|| -> Result<Vec<Transition>, String> {
        let deadline = Instant::now() + Duration::from_secs(if manual { 7 } else { 33 });
        while !ready.exists() {
            if observer.is_finished() {
                return Err("Pin observer exited before readiness".into());
            }
            if Instant::now() >= deadline {
                return Err("Pin observer readiness timed out".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if manual {
            eprintln!(
                "Task View recorder ready for 120 seconds: pin ONE fixture window to all desktops, unpin it, pin ALL fixture windows to all desktops, then unpin them. Pause at least two seconds between changes."
            );
            return Ok(Vec::new());
        }
        // Start off the sample boundary to exercise the polling delay.
        std::thread::sleep(Duration::from_millis(200));
        let mut transitions = Vec::new();
        for operation in ["pin-window", "unpin-window", "pin-app", "unpin-app"] {
            let requested_ms = clock_ms()?;
            let stage = call_worker(pid, operation)?;
            let completed_ms = clock_ms()?;
            transitions.push(Transition {
                operation: operation.into(),
                requested_ms,
                completed_ms,
                detected_ms: None,
                conservative_latency_ms: None,
                expected: stage
                    .windows
                    .into_iter()
                    .map(|w| (w.hwnd, w.window_pinned, w.application_pinned))
                    .collect(),
            });
            std::thread::sleep(Duration::from_millis(2100));
        }
        Ok(transitions)
    })();
    // Joining is bounded by the owned observer subprocess's watchdog, including failure paths.
    let observed = observer
        .join()
        .map_err(|_| "Pin observer thread panicked".to_string())?;
    let _ = std::fs::remove_file(&ready);
    let _ = std::fs::remove_dir(&directory);
    let mut transitions = result?;
    let observation = serde_json::from_slice::<Result<Observation, String>>(&observed?)
        .map_err(|e| e.to_string())??;
    for index in 0..transitions.len() {
        let next = transitions
            .get(index + 1)
            .map(|t| t.requested_ms)
            .unwrap_or(u64::MAX);
        let transition = &mut transitions[index];
        if let Some(sample) = observation.samples.iter().find(|sample| {
            sample.began_ms >= transition.completed_ms
                && sample.observed_ms < next
                && sample.states == transition.expected
        }) {
            transition.detected_ms = Some(sample.observed_ms);
            transition.conservative_latency_ms = Some(sample.observed_ms - transition.requested_ms);
        }
    }
    let manual_coverage = manual.then(|| {
        let window_pin = observation.samples.iter().position(|sample| sample.states.iter().all(|(_, _, app)| !app)
            && sample.states.iter().filter(|(_, window, _)| *window).count() == 1);
        let app_pin = observation.samples.iter().position(|sample| sample.states.iter().all(|(_, _, app)| *app));
        let unpinned_after = |index: Option<usize>| index.is_some_and(|index| observation.samples.iter().skip(index + 1)
            .any(|sample| sample.states.iter().all(|(_, window, app)| !window && !app)));
        ManualCoverage { window_pin_then_unpin: unpinned_after(window_pin), app_pin_then_unpin: unpinned_after(app_pin),
            note: "Observed state cycles only. The operator must confirm Task View was the source; no click timestamp or click-to-discovery latency is inferred.".into() }
    });
    let report = Report {
        passed: if let Some(coverage) = &manual_coverage {
            coverage.window_pin_then_unpin && coverage.app_pin_then_unpin
        } else {
            transitions
                .iter()
                .all(|t| t.conservative_latency_ms.is_some_and(|ms| ms <= 2000))
        },
        limit_ms: 2000,
        mutation_source: if manual {
            "Operator Task View recording; no automated pin mutations during recording"
        } else {
            "API calls in independent actor subprocesses; Task View coverage remains separate"
        }
        .into(),
        transitions,
        observation,
        manual_coverage,
    };
    if !report.passed {
        return Err(format!(
            "Pin discovery acceptance or manual state coverage was incomplete: {}",
            serde_json::to_string(&report).unwrap()
        ));
    }
    Ok(report)
}
