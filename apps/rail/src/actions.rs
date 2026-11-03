use crate::{Event, notify, platform};
use crossbeam_channel::Sender;
use rail_core::{Action, Request, Window};
use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowPlacement, IsIconic, IsWindowVisible, SW_SHOWMAXIMIZED,
    SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, SetForegroundWindow, SetWindowPlacement,
    ShowWindowAsync, WINDOWPLACEMENT, WPF_ASYNCWINDOWPLACEMENT, WPF_RESTORETOMAXIMIZED,
};

pub struct Job {
    pub request: Request,
    pub sequence: Arc<AtomicU64>,
    pub generation: Arc<AtomicU64>,
    pub paused: Arc<AtomicBool>,
    pub rail: usize,
    pub focus_from: usize,
    pub transition_received_us: Option<u64>,
}
pub enum Work {
    Action(Job),
    Capture {
        key: String,
        window: Window,
        generation: u64,
        tick: u64,
    },
    Stop,
}
struct Active {
    job: Job,
    target: Option<rail_core::Layout>,
    began: Instant,
    matches: u8,
    timed_out: bool,
}

pub fn start() -> Sender<Work> {
    let (sender, incoming) = crossbeam_channel::bounded(64);
    std::thread::spawn(move || {
        let mut active = BTreeMap::<String, Active>::new();
        loop {
            let first = if active.is_empty() {
                match incoming.recv() {
                    Ok(work) => Some(work),
                    Err(_) => return,
                }
            } else {
                match incoming.recv_timeout(Duration::from_millis(
                    if active.values().all(|operation| operation.timed_out) {
                        1000
                    } else {
                        20
                    },
                )) {
                    Ok(work) => Some(work),
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => None,
                    Err(_) => return,
                }
            };
            let mut work: Vec<_> = first.into_iter().chain(incoming.try_iter()).collect();
            if work.iter().any(|w| matches!(w, Work::Stop)) {
                return;
            }
            work.sort_by_key(|w| matches!(w, Work::Action(job) if job.request.action.automatic()));
            for work in work {
                match work {
                    Work::Capture {
                        key,
                        window,
                        generation,
                        tick,
                    } => {
                        let layout = if platform::validate(&window) {
                            platform::layout(platform::hwnd(window.hwnd))
                        } else {
                            Err("Window identity changed".into())
                        };
                        notify(Event::Captured {
                            key,
                            generation,
                            tick,
                            layout,
                        });
                    }
                    Work::Action(mut job) => {
                        // External restoration between the click and dispatch must not reposition a visible window.
                        if matches!(job.request.action, Action::Restore(_))
                            && !unsafe { IsIconic(platform::hwnd(job.request.window.hwnd)) }
                                .as_bool()
                        {
                            job.request.action = Action::Raise;
                        }
                        let request = &job.request;
                        if active.contains_key(&request.key) {
                            notify(Event::ActionResult(
                                request.key.clone(),
                                request.sequence,
                                platform::tick(),
                                false,
                                false,
                            ));
                            continue;
                        }
                        if !valid(&job)
                            || platform::tick().saturating_sub(request.tick) > 2000
                            || !platform::validate(&request.window)
                        {
                            notify(Event::ActionResult(
                                request.key.clone(),
                                request.sequence,
                                platform::tick(),
                                false,
                                true,
                            ));
                            continue;
                        }
                        match dispatch(&job) {
                            Ok(target) => {
                                notify(Event::Accepted(
                                    request.key.clone(),
                                    request.sequence,
                                    target.clone(),
                                ));
                                active.insert(
                                    request.key.clone(),
                                    Active {
                                        job,
                                        target,
                                        began: Instant::now(),
                                        matches: 0,
                                        timed_out: false,
                                    },
                                );
                            }
                            Err(error) => {
                                notify(Event::Notice(error));
                                notify(Event::ActionResult(
                                    request.key.clone(),
                                    request.sequence,
                                    platform::tick(),
                                    false,
                                    true,
                                ));
                            }
                        }
                    }
                    Work::Stop => return,
                }
            }
            active.retain(|key, active| {
                let request = &active.job.request;
                if !platform::validate(&request.window) {
                    notify(Event::ActionResult(
                        key.clone(),
                        request.sequence,
                        platform::tick(),
                        false,
                        true,
                    ));
                    return false;
                }
                if verified(active) {
                    active.matches += 1;
                } else {
                    active.matches = 0;
                }
                if active.matches >= 2 {
                    if !active.timed_out
                        && !request.action.automatic()
                        && !matches!(request.action, Action::Minimize { .. })
                        && valid(&active.job)
                    {
                        let foreground = unsafe { GetForegroundWindow() }.0 as usize;
                        let mut foreground_pid = 0;
                        unsafe {
                            windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
                                platform::hwnd(foreground),
                                Some(&mut foreground_pid),
                            );
                        }
                        // A delayed request may not take focus after the user moved elsewhere.
                        if foreground_pid == std::process::id()
                            || (active.job.focus_from != 0 && foreground == active.job.focus_from)
                            || foreground == active.job.rail
                            || foreground == request.window.hwnd
                            || Some(foreground) == request.window.popup
                        {
                            let target = platform::popup(&request.window)
                                .unwrap_or(platform::hwnd(request.window.hwnd));
                            if !unsafe { SetForegroundWindow(target) }.as_bool() {
                                notify(Event::Notice(
                                    "Windows declined the requested foreground change.".into(),
                                ));
                            }
                        }
                    }
                    notify(Event::ActionResult(
                        key.clone(),
                        request.sequence,
                        platform::tick(),
                        true,
                        false,
                    ));
                    return false;
                }
                if !active.timed_out && active.began.elapsed() >= Duration::from_secs(2) {
                    active.timed_out = true;
                    notify(Event::ActionResult(
                        key.clone(),
                        request.sequence,
                        platform::tick(),
                        false,
                        false,
                    ));
                }
                // Retain only the single outstanding operation. No retries/backlog for a hung target.
                true
            });
        }
    });
    sender
}

fn valid(job: &Job) -> bool {
    job.sequence.load(Ordering::Acquire) == job.request.sequence
        && job.generation.load(Ordering::Acquire) == job.request.generation
        && !(job.request.action.automatic() && job.paused.load(Ordering::Acquire))
}

fn dispatch(job: &Job) -> Result<Option<rail_core::Layout>, String> {
    let request = &job.request;
    let window = platform::hwnd(request.window.hwnd);
    if unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
            window,
            windows::Win32::UI::WindowsAndMessaging::GWL_STYLE,
        )
    } as u32
        & windows::Win32::UI::WindowsAndMessaging::WS_DISABLED.0
        != 0
        && platform::popup(&request.window).is_none()
    {
        return Err("This owned-dialog group cannot be controlled reliably.".into());
    }
    match &request.action {
        Action::Minimize { .. } => {
            let issued = crate::metrics::clock_us();
            unsafe { ShowWindowAsync(window, SW_SHOWMINNOACTIVE) }
                .ok()
                .map_err(|e| e.to_string())?;
            if request.action.automatic()
                && let (Some(received), Some(issued)) = (job.transition_received_us, issued)
                && let Some(elapsed) = issued.checked_sub(received)
            {
                notify(Event::DispatchLatency(elapsed));
            }
            Ok(None)
        }
        Action::Restore(saved) | Action::RestoreAutomatic(saved) => {
            let target = platform::adjusted(saved, window)?;
            if !valid(job) || !platform::validate(&request.window) {
                return Err("Restore superseded before dispatch".into());
            }
            let observed = platform::layout(window)?;
            if preserves_arrangement(&target, &observed) {
                // A show request on a visible arranged window can unsnap it.
                if unsafe { IsIconic(window) }.as_bool() {
                    unsafe { ShowWindowAsync(window, SW_SHOWNOACTIVATE) }
                        .ok()
                        .map_err(|error| error.to_string())?;
                }
                if let Some(popup) = platform::popup(&request.window) {
                    let _ = unsafe { ShowWindowAsync(popup, SW_SHOWNOACTIVATE) };
                }
                return Ok(Some(target));
            }
            if target.arranged == Some(true) {
                return Err("The saved Snap arrangement cannot be preserved in the current window/display state. Automatic Snap group reconstruction is unavailable.".into());
            }
            let mut placement = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            unsafe { GetWindowPlacement(window, &mut placement) }.map_err(|e| e.to_string())?;
            placement.flags = WPF_ASYNCWINDOWPLACEMENT;
            placement.showCmd = if target.maximized {
                SW_SHOWMAXIMIZED.0 as u32
            } else {
                SW_SHOWNOACTIVATE.0 as u32
            };
            placement.rcNormalPosition = windows::Win32::Foundation::RECT {
                left: target.normal[0],
                top: target.normal[1],
                right: target.normal[2],
                bottom: target.normal[3],
            };
            if request.action.automatic() {
                // Establish the destination restore state while minimized,
                // then reveal it without activating the application. Never use
                // SW_SHOWMAXIMIZED for automatic work: that activates the target.
                placement.showCmd = SW_SHOWMINNOACTIVE.0 as u32;
                if target.maximized {
                    placement.flags |= WPF_RESTORETOMAXIMIZED;
                }
                unsafe { SetWindowPlacement(window, &placement) }.map_err(|e| e.to_string())?;
                // Once the intermediate native request is accepted, keep this
                // operation outstanding until observed. Superseding intent is
                // reconciled afterward; it cannot cancel already posted work.
                placement.showCmd = SW_SHOWNOACTIVATE.0 as u32;
            }
            if let Err(error) = unsafe { SetWindowPlacement(window, &placement) } {
                if !request.action.automatic() {
                    return Err(error.to_string());
                }
                // The minimize stage may still be queued. Never release the
                // per-window slot merely because the reveal stage was declined.
                notify(Event::Notice(format!(
                    "Automatic restore reveal was declined; waiting for the outstanding operation: {error}"
                )));
                return Ok(Some(target));
            }
            if let Some(popup) = platform::popup(&request.window) {
                let _ = unsafe { ShowWindowAsync(popup, SW_SHOWNOACTIVATE) };
            }
            Ok(Some(target))
        }
        // A manual restore can become a raise if Windows already revealed the
        // window between click and dispatch. Report its actual resulting layout.
        Action::Raise => Ok(Some(platform::layout(window)?)),
    }
}

fn verified(active: &Active) -> bool {
    let request = &active.job.request;
    let window = platform::hwnd(request.window.hwnd);
    if unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
            window,
            windows::Win32::UI::WindowsAndMessaging::GWL_STYLE,
        )
    } as u32
        & windows::Win32::UI::WindowsAndMessaging::WS_DISABLED.0
        != 0
        && platform::popup(&request.window).is_none()
    {
        return false;
    }
    match &request.action {
        Action::Minimize { .. } => {
            unsafe { IsIconic(window) }.as_bool()
                && platform::popup(&request.window)
                    .is_none_or(|popup| !unsafe { IsWindowVisible(popup) }.as_bool())
        }
        Action::Raise => true,
        Action::Restore(_) | Action::RestoreAutomatic(_) => {
            !unsafe { IsIconic(window) }.as_bool()
                && unsafe { IsWindowVisible(window) }.as_bool()
                && platform::layout(window).is_ok_and(|layout| {
                    active
                        .target
                        .as_ref()
                        .is_some_and(|expected| restored_layout_matches(expected, &layout))
                })
                && platform::popup(&request.window)
                    .is_none_or(|popup| unsafe { IsWindowVisible(popup) }.as_bool())
        }
    }
}

fn close_bounds(actual: [i32; 4], expected: [i32; 4]) -> bool {
    actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2)
}

fn preserves_arrangement(saved: &rail_core::Layout, observed: &rail_core::Layout) -> bool {
    saved.arranged == Some(true)
        // IsWindowArranged may be false while iconic. Saved evidence selects
        // a reveal candidate; only the visible-result verifier certifies it.
        && (observed.visible.is_none() || observed.arranged == Some(true))
        && saved.visible.is_some()
        && saved.monitor == observed.monitor
        && saved.monitor_rect == observed.monitor_rect
        && saved.work_area == observed.work_area
        && saved.dpi == observed.dpi
        && close_bounds(observed.normal, saved.normal)
        && observed.visible.is_none_or(|bounds| {
            saved
                .visible
                .is_some_and(|saved| close_bounds(bounds, saved))
        })
}

fn restored_layout_matches(expected: &rail_core::Layout, observed: &rail_core::Layout) -> bool {
    observed.maximized == expected.maximized
        && close_bounds(observed.normal, expected.normal)
        && expected
            .arranged
            .is_none_or(|state| observed.arranged == Some(state))
        && observed.visible.is_some_and(|bounds| {
            if let Some(expected) = expected.visible {
                close_bounds(bounds, expected)
            } else if !expected.maximized {
                let x = expected.work_area[0] - expected.monitor_rect[0];
                let y = expected.work_area[1] - expected.monitor_rect[1];
                close_bounds(
                    bounds,
                    [
                        expected.normal[0] + x,
                        expected.normal[1] + y,
                        expected.normal[2] + x,
                        expected.normal[3] + y,
                    ],
                )
            } else {
                observed.arranged != Some(true)
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapped() -> rail_core::Layout {
        rail_core::Layout {
            normal: [100, 100, 740, 460],
            maximized: false,
            visible: Some([23, 0, 970, 1088]),
            arranged: Some(true),
            dpi: 120,
            monitor: "primary".into(),
            monitor_rect: [0, 0, 1918, 1140],
            work_area: [31, 0, 1918, 1080],
        }
    }
    #[test]
    fn normal_bounds_with_stale_arranged_flag_do_not_certify_snap_restoration() {
        let saved = snapped();
        let mut observed = saved.clone();
        observed.visible = Some([131, 100, 771, 460]);
        assert!(!restored_layout_matches(&saved, &observed));
        assert!(!preserves_arrangement(&saved, &observed));
        observed.visible = saved.visible;
        assert!(restored_layout_matches(&saved, &observed));
    }
    #[test]
    fn minimized_snap_can_be_revealed_but_not_verified_until_visible() {
        let saved = snapped();
        let mut observed = saved.clone();
        observed.visible = None;
        observed.arranged = Some(false);
        assert!(preserves_arrangement(&saved, &observed));
        assert!(!restored_layout_matches(&saved, &observed));
        observed.work_area[0] += 31;
        assert!(!preserves_arrangement(&saved, &observed));
    }
    #[test]
    fn legacy_bounds_are_not_snap_evidence_and_normal_restore_checks_screen_bounds() {
        let mut saved = snapped();
        saved.arranged = None;
        saved.visible = None;
        assert!(!preserves_arrangement(&saved, &saved));
        let mut observed = saved.clone();
        observed.visible = Some([131, 100, 771, 460]);
        assert!(restored_layout_matches(&saved, &observed));
        observed.visible = Some([100, 100, 740, 460]);
        assert!(!restored_layout_matches(&saved, &observed));
    }
}
