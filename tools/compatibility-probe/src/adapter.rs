//! All undocumented shell calls are confined to the killable probe subprocess.
use crate::platform;
use crate::report::{AdapterReport, Candidate, Desktop, Observation, Sample};
use std::time::{Duration, Instant};
use winvd::DesktopEvent;

fn id(desktop: winvd::Desktop) -> Result<String, String> {
    desktop
        .get_id()
        .map(|guid| format!("{guid:?}"))
        .map_err(|error| format!("Desktop GUID: {error:?}"))
}

fn sample(start: Instant) -> Result<Sample, String> {
    let mut candidates = Vec::new();
    for candidate in platform::candidates()? {
        let pinned = winvd::is_pinned_window(candidate.hwnd);
        let app = winvd::is_pinned_app(candidate.hwnd);
        // Do not attribute a result to a window replaced during a COM request.
        if !platform::still_same_window(candidate) {
            continue;
        }
        let error = match (&pinned, &app) {
            (Err(error), _) | (_, Err(error)) => Some(format!("Pin query: {error:?}")),
            _ => None,
        };
        candidates.push(Candidate {
            hwnd: candidate.hwnd.0 as usize,
            process_id: candidate.pid,
            process_created: candidate.created,
            minimized: candidate.minimized,
            window_pinned: pinned.ok(),
            application_pinned: app.ok(),
            error,
        });
    }
    Ok(Sample {
        elapsed_ms: start.elapsed().as_millis() as u64,
        candidates,
    })
}

pub fn observe(seconds: u64) -> Result<AdapterReport, String> {
    // Bounded channel: shell callbacks use try_send and never block on the consumer.
    let (sender, receiver) = crossbeam_channel::bounded::<DesktopEvent>(128);
    let listener = winvd::listen_desktop_events(sender)
        .map_err(|error| format!("Notification registration: {error:?}"))?;
    let start = Instant::now();
    let desktops = winvd::get_desktops().map_err(|error| format!("Enumeration: {error:?}"))?;
    if desktops.is_empty() || desktops.len() > 64 {
        return Err("Invalid desktop enumeration length".into());
    }
    let mut records = Vec::new();
    for desktop in desktops {
        records.push(Desktop {
            id: id(desktop)?,
            name: desktop
                .get_name()
                .map_err(|error| format!("Desktop name: {error:?}"))?,
        });
    }
    let current_desktop = id(winvd::get_current_desktop().map_err(|error| format!("{error:?}"))?)?;
    if !records.iter().any(|record| record.id == current_desktop) {
        return Err(
            "Current desktop was absent from enumeration; retry in a stable session".into(),
        );
    }
    let mut report = AdapterReport {
        current_desktop,
        desktops: records,
        samples: vec![sample(start)?],
        events: Vec::new(),
        event_limit_reached: false,
        reconciliation_interval_ms: 1000,
    };
    let deadline = start + Duration::from_secs(seconds);
    let mut next_sample = start + Duration::from_secs(1);
    while Instant::now() < deadline {
        let now = Instant::now();
        if now >= next_sample {
            report.samples.push(sample(start)?);
            next_sample = Instant::now() + Duration::from_secs(1);
            continue;
        }
        let wait = next_sample.min(deadline).saturating_duration_since(now);
        match receiver.recv_timeout(wait) {
            Ok(event) => {
                // Avoid including wallpaper paths or other application content in diagnostics.
                let kind = match event {
                    DesktopEvent::DesktopCreated(_) => "desktop_created",
                    DesktopEvent::DesktopDestroyed { .. } => "desktop_destroyed",
                    DesktopEvent::DesktopChanged { .. } => "desktop_changed",
                    DesktopEvent::DesktopNameChanged(..) => "desktop_name_changed",
                    DesktopEvent::DesktopWallpaperChanged(..) => "desktop_wallpaper_changed",
                    DesktopEvent::DesktopMoved { .. } => "desktop_moved",
                    DesktopEvent::WindowChanged(_) => "window_changed",
                };
                if report.events.len() == 256 {
                    report.event_limit_reached = true;
                    break;
                }
                report.events.push(Observation {
                    elapsed_ms: start.elapsed().as_millis() as u64,
                    kind: kind.into(),
                });
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                return Err("Desktop notification channel disconnected".into());
            }
        }
    }
    // Joining a failed listener may hang; the parent's deadline also bounds this step.
    drop(listener);
    Ok(report)
}
