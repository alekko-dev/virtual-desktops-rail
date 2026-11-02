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
                && platform::layout(window).is_ok_and(|layout| {
                    active.target.as_ref().is_some_and(|expected| {
                        layout.maximized == expected.maximized
                            && layout
                                .normal
                                .iter()
                                .zip(expected.normal)
                                .all(|(a, b)| a.abs_diff(b) <= 2)
                    })
                })
                && platform::popup(&request.window)
                    .is_none_or(|popup| unsafe { IsWindowVisible(popup) }.as_bool())
        }
    }
}
