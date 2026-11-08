//! Asynchronous window action execution, verification, quarantine and guarded focus.
//!
//! [`Executor`] is the pure progression state machine over a [`Native`] window and
//! clock boundary; [`start`] runs it on a dedicated worker thread with [`Win32Native`].
use crate::{Event, notify, platform};
use crossbeam_channel::Sender;
use rail_core::{Acceptance, Action, Effect, Layout, Outcome, Request, Window};
use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowPlacement, IsIconic, IsWindowVisible, SW_MINIMIZE,
    SW_SHOWMAXIMIZED, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SetForegroundWindow, SetWindowPlacement,
    SetWindowPos, ShowWindowAsync, WINDOWPLACEMENT, WPF_ASYNCWINDOWPLACEMENT,
    WPF_RESTORETOMAXIMIZED,
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

#[allow(clippy::large_enum_variant)]
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

/// Desired normal placement issued to `SetWindowPlacement`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacementStage {
    /// Establish normal/maximized restore state while keeping the window minimized.
    Minimize { restore_to_maximized: bool },
    /// Reveal the window without activating it.
    ShowNoActivate,
    /// Reveal and maximize the window (explicit actions only).
    ShowMaximized,
}

/// The clock and native window operations required by [`Executor`].
pub trait Native {
    fn tick_ms(&mut self) -> u64;
    fn clock_us(&mut self) -> Option<u64>;
    fn pid(&self) -> u32;
    fn validate(&mut self, window: &Window) -> bool;
    fn disabled_without_popup(&mut self, window: &Window) -> bool;
    fn popup(&mut self, window: &Window) -> Option<usize>;
    fn is_iconic(&mut self, hwnd: usize) -> bool;
    fn is_visible(&mut self, hwnd: usize) -> bool;
    fn layout(&mut self, hwnd: usize) -> Result<Layout, String>;
    fn adjusted(&mut self, saved: &Layout, hwnd: usize) -> Result<Layout, String>;
    fn show_min_no_active(&mut self, hwnd: usize) -> Result<(), String>;
    fn minimize_arranged(&mut self, hwnd: usize) -> Result<(), String>;
    fn show_no_activate(&mut self, hwnd: usize) -> Result<(), String>;
    fn set_placement(
        &mut self,
        hwnd: usize,
        normal: [i32; 4],
        stage: PlacementStage,
    ) -> Result<(), String>;
    /// Current foreground `(hwnd, pid)`.
    fn foreground(&mut self) -> (usize, u32);
    fn set_foreground(&mut self, hwnd: usize) -> bool;
    fn z_above(&mut self, hwnd: usize) -> Vec<usize>;
    fn is_z_order_target(&mut self, hwnd: usize) -> bool;
    fn is_below(&mut self, hwnd: usize, above: usize) -> bool;
    fn place_behind(&mut self, hwnd: usize, above: usize) -> Result<(), String>;
}

struct Active {
    job: Job,
    acceptance: Acceptance,
    began_ms: u64,
    matches: u8,
    timed_out: bool,
    z_placed_behind: Option<usize>,
}

pub struct Executor<N> {
    native: N,
    active: BTreeMap<String, Active>,
}

impl<N: Native> Executor<N> {
    pub fn new(native: N) -> Self {
        Self {
            native,
            active: BTreeMap::new(),
        }
    }

    pub fn poll_interval(&self) -> Option<Duration> {
        if self.active.is_empty() {
            None
        } else if self.active.values().all(|operation| operation.timed_out) {
            Some(Duration::from_millis(1000))
        } else {
            Some(Duration::from_millis(20))
        }
    }

    #[cfg(test)]
    pub fn native_mut(&mut self) -> &mut N {
        &mut self.native
    }

    /// Process a batch of queued work and advance active verification once. Returns
    /// `false` when `Work::Stop` terminates the executor.
    pub fn step(&mut self, mut batch: Vec<Work>, out: &mut Vec<Event>) -> bool {
        if batch.iter().any(|work| matches!(work, Work::Stop)) {
            return false;
        }
        batch.sort_by_key(
            |work| matches!(work, Work::Action(job) if job.request.action.automatic()),
        );
        for work in batch {
            match work {
                Work::Capture {
                    key,
                    window,
                    generation,
                    tick,
                } => {
                    let layout = if self.native.validate(&window) {
                        self.native.layout(window.hwnd)
                    } else {
                        Err("Window identity changed".into())
                    };
                    out.push(Event::Captured {
                        key,
                        generation,
                        tick,
                        layout,
                    });
                }
                Work::Action(job) => self.accept_or_reject(job, out),
                Work::Stop => return false,
            }
        }
        self.poll_active(out);
        true
    }

    fn accept_or_reject(&mut self, job: Job, out: &mut Vec<Event>) {
        // External restoration between the click and dispatch must not reposition a
        // visible window. Keep the original intent in `job.request`; record the raise
        // as the executed effect.
        let effect = if matches!(
            job.request.action,
            Action::Restore(_) | Action::RevealSnap(_)
        ) && !self.native.is_iconic(job.request.window.hwnd)
        {
            Effect::Raise
        } else {
            job.request.action.effect()
        };
        let key = job.request.key.clone();
        let sequence = job.request.sequence;
        if self.active.contains_key(&key) {
            let tick = self.native.tick_ms();
            out.push(Event::ActionResult {
                key,
                sequence,
                tick,
                outcome: Outcome::Busy,
            });
            return;
        }
        let now = self.native.tick_ms();
        if !valid(&job)
            || now.saturating_sub(job.request.tick) > 2000
            || !self.native.validate(&job.request.window)
        {
            let tick = self.native.tick_ms();
            out.push(Event::ActionResult {
                key,
                sequence,
                tick,
                outcome: Outcome::Rejected,
            });
            return;
        }
        match self.dispatch(&job, effect, out) {
            Ok(acceptance) => {
                out.push(Event::Accepted {
                    key: key.clone(),
                    sequence,
                    acceptance: acceptance.clone(),
                });
                let began_ms = self.native.tick_ms();
                self.active.insert(
                    key,
                    Active {
                        job,
                        acceptance,
                        began_ms,
                        matches: 0,
                        timed_out: false,
                        z_placed_behind: None,
                    },
                );
            }
            Err(error) => {
                if error != crate::snap_notice::TEXT {
                    out.push(Event::Notice(error));
                }
                let tick = self.native.tick_ms();
                out.push(Event::ActionResult {
                    key,
                    sequence,
                    tick,
                    outcome: Outcome::Rejected,
                });
            }
        }
    }

    fn dispatch(
        &mut self,
        job: &Job,
        effect: Effect,
        out: &mut Vec<Event>,
    ) -> Result<Acceptance, String> {
        let request = &job.request;
        let hwnd = request.window.hwnd;
        if self.native.disabled_without_popup(&request.window) {
            return Err("This owned-dialog group cannot be controlled reliably.".into());
        }
        match effect {
            Effect::Minimize => {
                // Query immediately before dispatch; cached snapshots can predate a snap.
                let observed = self.native.layout(hwnd)?;
                let observed_tick = self.native.tick_ms();
                if !valid(job) || !self.native.validate(&request.window) {
                    return Err("Minimize superseded before dispatch".into());
                }
                out.push(Event::ObservedArrangement {
                    z_above: if !request.action.automatic() && observed.visible.is_some() {
                        self.native.z_above(hwnd)
                    } else {
                        Vec::new()
                    },
                    request: Box::new(request.clone()),
                    layout: observed.clone(),
                    tick: observed_tick,
                });
                let arranged = observed.arranged == Some(true);
                let issued = self.native.clock_us();
                if arranged {
                    self.native.minimize_arranged(hwnd)?;
                } else {
                    self.native.show_min_no_active(hwnd)?;
                }
                if request.action.automatic()
                    && let (Some(received), Some(issued)) = (job.transition_received_us, issued)
                    && let Some(elapsed) = issued.checked_sub(received)
                {
                    out.push(Event::DispatchLatency(elapsed));
                }
                Ok(Acceptance {
                    effect: Effect::Minimize,
                    target: Some(observed),
                })
            }
            Effect::Restore => {
                let saved = match &request.action {
                    Action::Restore(saved)
                    | Action::RestoreAutomatic(saved)
                    | Action::RevealSnap(saved) => saved,
                    _ => unreachable!(),
                };
                // Snap paths never write placement. Compare the original native
                // restore bounds, which can legitimately extend past the work area;
                // clamping them would turn a matching arrangement into a mismatch.
                let target = if saved.arranged == Some(true)
                    || matches!(request.action, Action::RevealSnap(_))
                {
                    saved.clone()
                } else {
                    self.native.adjusted(saved, hwnd)?
                };
                if !valid(job) || !self.native.validate(&request.window) {
                    return Err("Restore superseded before dispatch".into());
                }
                let observed = self.native.layout(hwnd)?;
                if preserves_arrangement(&target, &observed) {
                    let was_iconic = self.native.is_iconic(hwnd);
                    // A show request on a visible arranged window can unsnap it.
                    if was_iconic {
                        self.native.show_no_activate(hwnd)?;
                    }
                    let popup = self.native.popup(&request.window);
                    if let Some(popup) = popup {
                        let _ = self.native.show_no_activate(popup);
                    }
                    if matches!(request.action, Action::RestoreAutomatic(_))
                        && let Some(above) =
                            resolve_z_above(&mut self.native, request, job.rail, true)
                        && (was_iconic || !self.native.is_below(hwnd, above))
                    {
                        place_window_behind(&mut self.native, hwnd, popup, above);
                    }
                    return Ok(Acceptance {
                        effect: Effect::Restore,
                        target: Some(target),
                    });
                }
                let retained_snap = observed.visible.is_none()
                    && request
                        .retained_arrangement
                        .as_ref()
                        .is_some_and(|evidence| {
                            evidence.monitor == observed.monitor
                                && evidence.monitor_rect == observed.monitor_rect
                                && evidence.work_area == observed.work_area
                                && evidence.dpi == observed.dpi
                        });
                if saved.arranged == Some(true) || observed.arranged == Some(true) || retained_snap
                {
                    if request.minimize_snapped_elsewhere
                        && request.action.automatic()
                        && (observed.arranged == Some(true) || retained_snap)
                    {
                        // Preserve the live native arrangement while hiding it on
                        // an incompatible destination. No placement or focus request.
                        if !valid(job) || !self.native.validate(&request.window) {
                            return Err("Restore superseded before dispatch".into());
                        }
                        if self.native.is_iconic(hwnd) {
                            return Ok(Acceptance {
                                effect: Effect::Minimize,
                                target: Some(observed),
                            });
                        }
                        return self.dispatch(job, Effect::Minimize, out);
                    }
                    out.push(Event::SnapPreservation(Box::new(
                        crate::snap_notice::SnapNotice::new(request.clone(), self.native.tick_ms()),
                    )));
                    return Err(crate::snap_notice::TEXT.into());
                }
                if request.action.automatic() {
                    // Establish the destination restore state while minimized,
                    // then reveal it without activating the application. Never use
                    // SW_SHOWMAXIMIZED for automatic work: that activates the target.
                    self.native.set_placement(
                        hwnd,
                        target.normal,
                        PlacementStage::Minimize {
                            restore_to_maximized: target.maximized,
                        },
                    )?;
                    // Once the intermediate native request is accepted, keep this
                    // operation outstanding until observed. Superseding intent is
                    // reconciled afterward; it cannot cancel already posted work.
                    if let Err(error) = self.native.set_placement(
                        hwnd,
                        target.normal,
                        PlacementStage::ShowNoActivate,
                    ) {
                        // The minimize stage may still be queued. Never release the
                        // per-window slot merely because the reveal stage was declined.
                        out.push(Event::Notice(format!(
                            "Automatic restore reveal was declined; waiting for the outstanding operation: {error}"
                        )));
                        return Ok(Acceptance {
                            effect: Effect::Restore,
                            target: Some(target),
                        });
                    }
                } else {
                    let stage = if target.maximized {
                        PlacementStage::ShowMaximized
                    } else {
                        PlacementStage::ShowNoActivate
                    };
                    self.native.set_placement(hwnd, target.normal, stage)?;
                }
                let popup = self.native.popup(&request.window);
                if let Some(popup) = popup {
                    let _ = self.native.show_no_activate(popup);
                }
                if matches!(request.action, Action::RestoreAutomatic(_))
                    && let Some(above) = resolve_z_above(&mut self.native, request, job.rail, true)
                {
                    place_window_behind(&mut self.native, hwnd, popup, above);
                }
                Ok(Acceptance {
                    effect: Effect::Restore,
                    target: Some(target),
                })
            }
            // A manual restore can become a raise if Windows already revealed the
            // window between click and dispatch. Report its actual resulting layout.
            Effect::Raise => Ok(Acceptance {
                effect: Effect::Raise,
                target: Some(self.native.layout(hwnd)?),
            }),
        }
    }

    fn poll_active(&mut self, out: &mut Vec<Event>) {
        // Observe/reconcile the whole restore cohort before releasing any member.
        // An independently completed member could otherwise use a peer whose
        // asynchronous reveal or correction has not finished moving it yet.
        let foreground = self.native.foreground().0;
        let allow_foreground = !self.active.values().any(|active| {
            active.job.request.window.hwnd == foreground
                || active.job.request.window.popup == Some(foreground)
        });
        let mut observations = BTreeMap::new();
        let mut restores_ready = true;
        for (key, active) in &mut self.active {
            let identity_valid = self.native.validate(&active.job.request.window);
            let matches = identity_valid && verified(&mut self.native, active, allow_foreground);
            if coordinated_restore(active) {
                restores_ready &= matches;
            }
            observations.insert(key.clone(), (identity_valid, matches));
        }
        for (key, (identity_valid, matches)) in observations {
            let mut remove = false;
            if let Some(active) = self.active.get_mut(&key) {
                let sequence = active.job.request.sequence;
                if !identity_valid {
                    let tick = self.native.tick_ms();
                    out.push(Event::ActionResult {
                        key: key.clone(),
                        sequence,
                        tick,
                        outcome: Outcome::IdentityLost,
                    });
                    remove = true;
                } else {
                    if matches && (!coordinated_restore(active) || restores_ready) {
                        active.matches += 1;
                    } else {
                        active.matches = 0;
                    }
                    if active.matches >= 2 {
                        request_guarded_focus(&mut self.native, active, out);
                        let tick = self.native.tick_ms();
                        out.push(Event::ActionResult {
                            key: key.clone(),
                            sequence,
                            tick,
                            outcome: Outcome::Verified,
                        });
                        remove = true;
                    } else {
                        let now = self.native.tick_ms();
                        if !active.timed_out && now.saturating_sub(active.began_ms) >= 2000 {
                            active.timed_out = true;
                            out.push(Event::ActionResult {
                                key: key.clone(),
                                sequence,
                                tick: now,
                                outcome: Outcome::TimedOut,
                            });
                        }
                    }
                }
            }
            if remove {
                self.active.remove(&key);
            }
        }
    }
}

fn request_guarded_focus<N: Native>(native: &mut N, active: &Active, out: &mut Vec<Event>) {
    let request = &active.job.request;
    if active.timed_out
        || request.action.automatic()
        || active.acceptance.effect == Effect::Minimize
        || !valid(&active.job)
    {
        return;
    }
    let (foreground, foreground_pid) = native.foreground();
    // A delayed request may not take focus after the user moved elsewhere.
    if foreground_pid == native.pid()
        || (active.job.focus_from != 0 && foreground == active.job.focus_from)
        || foreground == active.job.rail
        || foreground == request.window.hwnd
        || Some(foreground) == request.window.popup
    {
        let target = native.popup(&request.window).unwrap_or(request.window.hwnd);
        if !native.set_foreground(target) {
            out.push(Event::Notice(
                "Windows declined the requested foreground change.".into(),
            ));
        }
    }
}

fn valid(job: &Job) -> bool {
    job.sequence.load(Ordering::Acquire) == job.request.sequence
        && job.generation.load(Ordering::Acquire) == job.request.generation
        && !(job.request.action.automatic() && job.paused.load(Ordering::Acquire))
}

fn coordinated_restore(active: &Active) -> bool {
    !active.timed_out
        && valid(&active.job)
        && matches!(active.job.request.action, Action::RestoreAutomatic(_))
        && active.acceptance.effect == Effect::Restore
}

fn resolve_z_above<N: Native>(
    native: &mut N,
    request: &Request,
    rail: usize,
    allow_foreground: bool,
) -> Option<usize> {
    let hwnd = request.window.hwnd;
    let popup = request.window.popup;
    let is_valid = |native: &mut N, candidate: usize| {
        candidate != 0
            && candidate != hwnd
            && candidate != rail
            && Some(candidate) != popup
            && native.is_z_order_target(candidate)
    };
    let mut lowest: Option<usize> = None;
    for &candidate in &request.z_above {
        if is_valid(native, candidate)
            && lowest.is_none_or(|current| native.is_below(candidate, current))
        {
            lowest = Some(candidate);
        }
    }
    if lowest.is_none() && allow_foreground {
        let (foreground, _) = native.foreground();
        if is_valid(native, foreground) {
            lowest = Some(foreground);
        }
    }
    lowest
}

fn place_window_behind<N: Native>(native: &mut N, hwnd: usize, popup: Option<usize>, above: usize) {
    if let Some(popup) = popup {
        let _ = native.place_behind(popup, above);
        let _ = native.place_behind(hwnd, popup);
    } else {
        let _ = native.place_behind(hwnd, above);
    }
}

fn verified_z_order<N: Native>(
    native: &mut N,
    active: &mut Active,
    allow_foreground: bool,
) -> bool {
    let request = &active.job.request;
    if matches!(request.action, Action::RestoreAutomatic(_)) && valid(&active.job) {
        let hwnd = request.window.hwnd;
        if let Some(above) = resolve_z_above(native, request, active.job.rail, allow_foreground)
            && !native.is_below(hwnd, above)
        {
            if !active.timed_out && active.z_placed_behind != Some(above) {
                active.z_placed_behind = Some(above);
                let popup = native.popup(&request.window);
                place_window_behind(native, hwnd, popup, above);
            }
            return false;
        }
        // A confirmed correction may later be disturbed by another member's
        // delayed reveal. Allow another correction then, but never flood an
        // unresponsive target with repeated identical asynchronous requests.
        active.z_placed_behind = None;
    }
    true
}

fn verified<N: Native>(native: &mut N, active: &mut Active, allow_foreground: bool) -> bool {
    let request = &active.job.request;
    let hwnd = request.window.hwnd;
    if native.disabled_without_popup(&request.window) {
        return false;
    }
    match active.acceptance.effect {
        Effect::Minimize => {
            native.is_iconic(hwnd)
                && native
                    .popup(&request.window)
                    .is_none_or(|popup| !native.is_visible(popup))
        }
        Effect::Raise => true,
        Effect::Restore => {
            !native.is_iconic(hwnd)
                && native.is_visible(hwnd)
                && native.layout(hwnd).is_ok_and(|layout| {
                    active
                        .acceptance
                        .target
                        .as_ref()
                        .is_some_and(|expected| restored_layout_matches(expected, &layout))
                })
                && native
                    .popup(&request.window)
                    .is_none_or(|popup| native.is_visible(popup))
                && verified_z_order(native, active, allow_foreground)
        }
    }
}

struct Win32Native;

impl Native for Win32Native {
    fn tick_ms(&mut self) -> u64 {
        platform::tick()
    }
    fn clock_us(&mut self) -> Option<u64> {
        crate::metrics::clock_us()
    }
    fn pid(&self) -> u32 {
        std::process::id()
    }
    fn validate(&mut self, window: &Window) -> bool {
        platform::validate(window)
    }
    fn disabled_without_popup(&mut self, window: &Window) -> bool {
        let handle = platform::hwnd(window.hwnd);
        (unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                handle,
                windows::Win32::UI::WindowsAndMessaging::GWL_STYLE,
            )
        } as u32)
            & windows::Win32::UI::WindowsAndMessaging::WS_DISABLED.0
            != 0
            && platform::popup(window).is_none()
    }
    fn popup(&mut self, window: &Window) -> Option<usize> {
        platform::popup(window).map(|handle| handle.0 as usize)
    }
    fn is_iconic(&mut self, hwnd: usize) -> bool {
        unsafe { IsIconic(platform::hwnd(hwnd)) }.as_bool()
    }
    fn is_visible(&mut self, hwnd: usize) -> bool {
        unsafe { IsWindowVisible(platform::hwnd(hwnd)) }.as_bool()
    }
    fn layout(&mut self, hwnd: usize) -> Result<Layout, String> {
        platform::layout(platform::hwnd(hwnd))
    }
    fn adjusted(&mut self, saved: &Layout, hwnd: usize) -> Result<Layout, String> {
        platform::adjusted(saved, platform::hwnd(hwnd))
    }
    fn show_min_no_active(&mut self, hwnd: usize) -> Result<(), String> {
        unsafe { ShowWindowAsync(platform::hwnd(hwnd), SW_SHOWMINNOACTIVE) }
            .ok()
            .map_err(|error| error.to_string())
    }
    fn minimize_arranged(&mut self, hwnd: usize) -> Result<(), String> {
        unsafe { ShowWindowAsync(platform::hwnd(hwnd), SW_MINIMIZE) }
            .ok()
            .map_err(|error| error.to_string())
    }
    fn show_no_activate(&mut self, hwnd: usize) -> Result<(), String> {
        unsafe { ShowWindowAsync(platform::hwnd(hwnd), SW_SHOWNOACTIVATE) }
            .ok()
            .map_err(|error| error.to_string())
    }
    fn set_placement(
        &mut self,
        hwnd: usize,
        normal: [i32; 4],
        stage: PlacementStage,
    ) -> Result<(), String> {
        let handle = platform::hwnd(hwnd);
        let mut placement = WINDOWPLACEMENT {
            length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        unsafe { GetWindowPlacement(handle, &mut placement) }.map_err(|e| e.to_string())?;
        placement.flags = WPF_ASYNCWINDOWPLACEMENT;
        placement.rcNormalPosition = windows::Win32::Foundation::RECT {
            left: normal[0],
            top: normal[1],
            right: normal[2],
            bottom: normal[3],
        };
        match stage {
            PlacementStage::Minimize {
                restore_to_maximized,
            } => {
                placement.showCmd = SW_SHOWMINNOACTIVE.0 as u32;
                if restore_to_maximized {
                    placement.flags |= WPF_RESTORETOMAXIMIZED;
                }
            }
            PlacementStage::ShowNoActivate => {
                placement.showCmd = SW_SHOWNOACTIVATE.0 as u32;
            }
            PlacementStage::ShowMaximized => {
                placement.showCmd = SW_SHOWMAXIMIZED.0 as u32;
            }
        }
        unsafe { SetWindowPlacement(handle, &placement) }.map_err(|e| e.to_string())
    }
    fn foreground(&mut self) -> (usize, u32) {
        let foreground = unsafe { GetForegroundWindow() }.0 as usize;
        let mut pid = 0;
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
                platform::hwnd(foreground),
                Some(&mut pid),
            );
        }
        (foreground, pid)
    }
    fn set_foreground(&mut self, hwnd: usize) -> bool {
        unsafe { SetForegroundWindow(platform::hwnd(hwnd)) }.as_bool()
    }
    fn z_above(&mut self, hwnd: usize) -> Vec<usize> {
        platform::z_above(platform::hwnd(hwnd))
    }
    fn is_z_order_target(&mut self, hwnd: usize) -> bool {
        platform::is_z_order_target(platform::hwnd(hwnd))
    }
    fn is_below(&mut self, hwnd: usize, above: usize) -> bool {
        platform::is_below(platform::hwnd(hwnd), platform::hwnd(above))
    }
    fn place_behind(&mut self, hwnd: usize, above: usize) -> Result<(), String> {
        unsafe {
            SetWindowPos(
                platform::hwnd(hwnd),
                platform::hwnd(above),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_ASYNCWINDOWPOS,
            )
        }
        .map_err(|error| error.to_string())
    }
}

pub fn start() -> Sender<Work> {
    let (sender, incoming) = crossbeam_channel::bounded(64);
    std::thread::spawn(move || {
        let mut executor = Executor::new(Win32Native);
        let mut out = Vec::new();
        loop {
            let first = match executor.poll_interval() {
                None => match incoming.recv() {
                    Ok(work) => Some(work),
                    Err(_) => return,
                },
                Some(timeout) => match incoming.recv_timeout(timeout) {
                    Ok(work) => Some(work),
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => None,
                    Err(_) => return,
                },
            };
            let batch: Vec<_> = first.into_iter().chain(incoming.try_iter()).collect();
            if !executor.step(batch, &mut out) {
                return;
            }
            for event in out.drain(..) {
                notify(event);
            }
        }
    });
    sender
}

fn close_bounds(actual: [i32; 4], expected: [i32; 4]) -> bool {
    actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2)
}

pub(crate) fn preserves_arrangement(saved: &Layout, observed: &Layout) -> bool {
    saved.arranged == Some(true)
        && !saved.maximized
        && !observed.maximized
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

pub(crate) fn restored_layout_matches(expected: &Layout, observed: &Layout) -> bool {
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
mod tests;
