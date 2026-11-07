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
    SW_SHOWMAXIMIZED, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, SetForegroundWindow,
    SetWindowPlacement, ShowWindowAsync, WINDOWPLACEMENT, WPF_ASYNCWINDOWPLACEMENT,
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
}

struct Active {
    job: Job,
    acceptance: Acceptance,
    began_ms: u64,
    matches: u8,
    timed_out: bool,
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
        let effect = if matches!(job.request.action, Action::Restore(_))
            && !self.native.is_iconic(job.request.window.hwnd)
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
                    },
                );
            }
            Err(error) => {
                out.push(Event::Notice(error));
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
                let arranged = self.native.layout(hwnd)?.arranged == Some(true);
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
                    target: None,
                })
            }
            Effect::Restore => {
                let saved = match &request.action {
                    Action::Restore(saved) | Action::RestoreAutomatic(saved) => saved,
                    _ => unreachable!(),
                };
                let target = self.native.adjusted(saved, hwnd)?;
                if !valid(job) || !self.native.validate(&request.window) {
                    return Err("Restore superseded before dispatch".into());
                }
                let observed = self.native.layout(hwnd)?;
                if preserves_arrangement(&target, &observed) {
                    // A show request on a visible arranged window can unsnap it.
                    if self.native.is_iconic(hwnd) {
                        self.native.show_no_activate(hwnd)?;
                    }
                    if let Some(popup) = self.native.popup(&request.window) {
                        let _ = self.native.show_no_activate(popup);
                    }
                    return Ok(Acceptance {
                        effect: Effect::Restore,
                        target: Some(target),
                    });
                }
                if saved.arranged == Some(true) || observed.arranged == Some(true) {
                    return Err("Restoration would replace or lose a native Snap arrangement in the current window/display state. Automatic Snap group reconstruction is unavailable.".into());
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
                if let Some(popup) = self.native.popup(&request.window) {
                    let _ = self.native.show_no_activate(popup);
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
        let keys: Vec<String> = self.active.keys().cloned().collect();
        for key in keys {
            let mut remove = false;
            if let Some(active) = self.active.get_mut(&key) {
                let sequence = active.job.request.sequence;
                if !self.native.validate(&active.job.request.window) {
                    let tick = self.native.tick_ms();
                    out.push(Event::ActionResult {
                        key: key.clone(),
                        sequence,
                        tick,
                        outcome: Outcome::IdentityLost,
                    });
                    remove = true;
                } else {
                    if verified(&mut self.native, active) {
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

fn verified<N: Native>(native: &mut N, active: &Active) -> bool {
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

fn preserves_arrangement(saved: &Layout, observed: &Layout) -> bool {
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

fn restored_layout_matches(expected: &Layout, observed: &Layout) -> bool {
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
    use rail_core::{Click, Desktop, Identity, Model, OperationState, Saved, Snapshot};
    use std::collections::VecDeque;

    fn layout(left: i32) -> Layout {
        Layout {
            normal: [left, 80, left + 600, 500],
            maximized: false,
            visible: Some([left + 25, 80, left + 625, 500]),
            arranged: Some(false),
            dpi: 96,
            monitor: "main".into(),
            monitor_rect: [0, 0, 1920, 1080],
            work_area: [25, 0, 1920, 1040],
        }
    }

    fn snapped() -> Layout {
        Layout {
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

    fn window(token: &str, hwnd: usize) -> Window {
        Window {
            identity: Identity {
                token: token.into(),
                process_created: 100,
                executable: "fixture.exe".into(),
                session_id: 2,
            },
            hwnd,
            pid: 45,
            minimized: false,
            responsive: true,
            layout: layout(100),
            popup: None,
        }
    }

    fn snapshot(desktop: &str, tick: u64, windows: Vec<Window>) -> Snapshot {
        Snapshot {
            desktops: vec![
                Desktop {
                    id: "A".into(),
                    name: "A".into(),
                },
                Desktop {
                    id: "B".into(),
                    name: "B".into(),
                },
            ],
            current: desktop.into(),
            windows,
            tick,
            transition_serial: 0,
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Call {
        ShowMinNoActive(usize),
        MinimizeArranged(usize),
        ShowNoActivate(usize),
        SetPlacement {
            hwnd: usize,
            normal: [i32; 4],
            stage: PlacementStage,
        },
        SetForeground(usize),
    }

    #[derive(Clone)]
    struct FakeWindow {
        identity: Identity,
        iconic: bool,
        visible: bool,
        disabled: bool,
        popup: Option<usize>,
        popup_visible: bool,
        layout: Layout,
        adjusted: Option<Layout>,
        /// Automatically apply native effects to simulated state when accepted.
        auto_apply: bool,
        placement_results: VecDeque<Result<(), String>>,
        show_results: VecDeque<Result<(), String>>,
    }

    impl FakeWindow {
        fn from(window: &Window) -> Self {
            Self {
                identity: window.identity.clone(),
                iconic: window.minimized,
                visible: !window.minimized,
                disabled: false,
                popup: window.popup,
                popup_visible: window.popup.is_some() && !window.minimized,
                layout: window.layout.clone(),
                adjusted: None,
                auto_apply: true,
                placement_results: VecDeque::new(),
                show_results: VecDeque::new(),
            }
        }
    }

    struct FakeNative {
        tick_ms: u64,
        clock_us: Option<u64>,
        self_pid: u32,
        foreground: (usize, u32),
        set_foreground_ok: bool,
        windows: BTreeMap<usize, FakeWindow>,
        calls: Vec<Call>,
    }

    impl FakeNative {
        fn new(tick_ms: u64) -> Self {
            Self {
                tick_ms,
                clock_us: Some(tick_ms * 1000),
                self_pid: 999,
                foreground: (1, 999),
                set_foreground_ok: true,
                windows: BTreeMap::new(),
                calls: Vec::new(),
            }
        }

        fn insert(&mut self, window: &Window) {
            self.windows.insert(window.hwnd, FakeWindow::from(window));
        }

        fn window_mut(&mut self, hwnd: usize) -> &mut FakeWindow {
            self.windows.get_mut(&hwnd).unwrap()
        }
    }

    impl Native for FakeNative {
        fn tick_ms(&mut self) -> u64 {
            self.tick_ms
        }
        fn clock_us(&mut self) -> Option<u64> {
            self.clock_us
        }
        fn pid(&self) -> u32 {
            self.self_pid
        }
        fn validate(&mut self, window: &Window) -> bool {
            self.windows
                .get(&window.hwnd)
                .is_some_and(|live| live.identity == window.identity)
        }
        fn disabled_without_popup(&mut self, window: &Window) -> bool {
            self.windows
                .get(&window.hwnd)
                .is_some_and(|live| live.disabled && live.popup.is_none())
        }
        fn popup(&mut self, window: &Window) -> Option<usize> {
            self.windows.get(&window.hwnd).and_then(|live| live.popup)
        }
        fn is_iconic(&mut self, hwnd: usize) -> bool {
            if let Some(live) = self.windows.get(&hwnd) {
                return live.iconic;
            }
            false
        }
        fn is_visible(&mut self, hwnd: usize) -> bool {
            if let Some(live) = self.windows.get(&hwnd) {
                return live.visible;
            }
            self.windows
                .values()
                .find(|live| live.popup == Some(hwnd))
                .is_some_and(|live| live.popup_visible)
        }
        fn layout(&mut self, hwnd: usize) -> Result<Layout, String> {
            self.windows
                .get(&hwnd)
                .map(|live| live.layout.clone())
                .ok_or_else(|| "Missing window".into())
        }
        fn adjusted(&mut self, saved: &Layout, hwnd: usize) -> Result<Layout, String> {
            let live = self
                .windows
                .get(&hwnd)
                .ok_or_else(|| String::from("Missing window"))?;
            Ok(live.adjusted.clone().unwrap_or_else(|| {
                saved.adjusted(
                    &live.layout.monitor,
                    live.layout.monitor_rect,
                    live.layout.work_area,
                    live.layout.dpi,
                )
            }))
        }
        fn show_min_no_active(&mut self, hwnd: usize) -> Result<(), String> {
            self.calls.push(Call::ShowMinNoActive(hwnd));
            let live = self
                .windows
                .get_mut(&hwnd)
                .ok_or_else(|| String::from("Missing window"))?;
            if let Some(result) = live.show_results.pop_front() {
                result?;
            }
            if live.auto_apply {
                live.iconic = true;
                live.visible = false;
                live.popup_visible = false;
            }
            Ok(())
        }
        fn minimize_arranged(&mut self, hwnd: usize) -> Result<(), String> {
            let result = self.show_min_no_active(hwnd);
            *self.calls.last_mut().unwrap() = Call::MinimizeArranged(hwnd);
            result
        }
        fn show_no_activate(&mut self, hwnd: usize) -> Result<(), String> {
            self.calls.push(Call::ShowNoActivate(hwnd));
            if let Some(live) = self.windows.get_mut(&hwnd) {
                if let Some(result) = live.show_results.pop_front() {
                    result?;
                }
                if live.auto_apply {
                    live.iconic = false;
                    live.visible = true;
                }
                return Ok(());
            }
            if let Some(owner) = self
                .windows
                .values_mut()
                .find(|live| live.popup == Some(hwnd))
            {
                if owner.auto_apply {
                    owner.popup_visible = true;
                }
                return Ok(());
            }
            Err("Missing window".into())
        }
        fn set_placement(
            &mut self,
            hwnd: usize,
            normal: [i32; 4],
            stage: PlacementStage,
        ) -> Result<(), String> {
            self.calls.push(Call::SetPlacement {
                hwnd,
                normal,
                stage,
            });
            let live = self
                .windows
                .get_mut(&hwnd)
                .ok_or_else(|| String::from("Missing window"))?;
            if let Some(result) = live.placement_results.pop_front() {
                result?;
            }
            if live.auto_apply {
                live.layout.normal = normal;
                let x = live.layout.work_area[0] - live.layout.monitor_rect[0];
                let y = live.layout.work_area[1] - live.layout.monitor_rect[1];
                match stage {
                    PlacementStage::Minimize {
                        restore_to_maximized,
                    } => {
                        live.iconic = true;
                        live.visible = false;
                        live.layout.maximized = restore_to_maximized;
                    }
                    PlacementStage::ShowNoActivate => {
                        live.iconic = false;
                        live.visible = true;
                        if !live.layout.maximized {
                            live.layout.visible =
                                Some([normal[0] + x, normal[1] + y, normal[2] + x, normal[3] + y]);
                        } else {
                            live.layout.visible = Some(live.layout.work_area);
                        }
                    }
                    PlacementStage::ShowMaximized => {
                        live.iconic = false;
                        live.visible = true;
                        live.layout.maximized = true;
                        live.layout.visible = Some(live.layout.work_area);
                    }
                }
            }
            Ok(())
        }
        fn foreground(&mut self) -> (usize, u32) {
            self.foreground
        }
        fn set_foreground(&mut self, hwnd: usize) -> bool {
            self.calls.push(Call::SetForeground(hwnd));
            self.set_foreground_ok
        }
    }

    struct Harness {
        model: Model,
        executor: Executor<FakeNative>,
        tickets: BTreeMap<String, Arc<AtomicU64>>,
        generation: Arc<AtomicU64>,
        paused: Arc<AtomicBool>,
        focus_from: BTreeMap<String, usize>,
        notices: Vec<String>,
    }

    impl Harness {
        fn new(window: &Window) -> Self {
            let mut model = Model::new(Saved::default());
            model.snapshot(snapshot("A", 100, vec![window.clone()]));
            let mut native = FakeNative::new(100);
            native.insert(window);
            Self {
                model,
                executor: Executor::new(native),
                tickets: BTreeMap::new(),
                generation: Arc::new(AtomicU64::new(1)),
                paused: Arc::new(AtomicBool::new(false)),
                focus_from: BTreeMap::new(),
                notices: Vec::new(),
            }
        }

        fn sync_tickets(&mut self) {
            self.generation
                .store(self.model.generation(), Ordering::Release);
            self.paused
                .store(self.model.saved().paused, Ordering::Release);
            for (key, sequence) in self.model.tickets() {
                self.tickets
                    .entry(key.to_owned())
                    .or_insert_with(|| Arc::new(AtomicU64::new(sequence)))
                    .store(sequence, Ordering::Release);
            }
        }

        fn issue(&mut self) -> Vec<Work> {
            self.sync_tickets();
            let tick = self.executor.native_mut().tick_ms;
            self.model
                .take_requests(tick)
                .into_iter()
                .map(|request| {
                    let key = request.key.clone();
                    Work::Action(Job {
                        sequence: self.tickets[&key].clone(),
                        generation: self.generation.clone(),
                        paused: self.paused.clone(),
                        rail: 1,
                        focus_from: self.focus_from.get(&key).copied().unwrap_or(0),
                        transition_received_us: None,
                        request,
                    })
                })
                .collect()
        }

        fn step(&mut self, work: Vec<Work>) {
            self.sync_tickets();
            let mut events = Vec::new();
            assert!(self.executor.step(work, &mut events));
            for event in events {
                match event {
                    Event::Accepted {
                        key,
                        sequence,
                        acceptance,
                    } => {
                        self.model.accepted(&key, sequence, acceptance);
                    }
                    Event::ActionResult {
                        key,
                        sequence,
                        tick,
                        outcome,
                    } => {
                        self.model.settle(&key, sequence, tick, outcome);
                    }
                    Event::Captured {
                        key,
                        generation,
                        tick,
                        layout: Ok(layout),
                    } => {
                        self.model.captured(&key, generation, tick, layout);
                    }
                    Event::Notice(notice) => self.notices.push(notice),
                    _ => {}
                }
            }
        }

        fn advance(&mut self, delta_ms: u64) {
            self.executor.native_mut().tick_ms += delta_ms;
            self.step(vec![]);
        }
    }

    #[test]
    fn snap_dispatch_uses_live_state_for_minimize() {
        for (cached_snap, live_snap) in [(false, true), (true, false)] {
            let mut win = window("one", 10);
            win.layout.arranged = Some(cached_snap);
            let mut executor = Executor::new(FakeNative::new(100));
            executor.native_mut().insert(&win);
            executor.native_mut().window_mut(10).layout.arranged = Some(live_snap);
            let job = test_job(win, Action::Minimize { automatic: true });
            let mut events = Vec::new();
            executor.step(vec![Work::Action(job)], &mut events);
            assert_eq!(
                executor.native_mut().calls,
                vec![if live_snap {
                    Call::MinimizeArranged(10)
                } else {
                    Call::ShowMinNoActive(10)
                }]
            );
            executor.step(vec![], &mut events);
            assert!(events.iter().any(|event| matches!(
                event,
                Event::ActionResult {
                    outcome: Outcome::Verified,
                    ..
                }
            )));
        }
    }

    fn test_job(window: Window, action: Action) -> Job {
        Job {
            request: Request {
                key: window.identity.key(),
                window,
                action,
                generation: 1,
                sequence: 1,
                tick: 100,
            },
            sequence: Arc::new(AtomicU64::new(1)),
            generation: Arc::new(AtomicU64::new(1)),
            paused: Arc::new(AtomicBool::new(false)),
            rail: 1,
            focus_from: 0,
            transition_received_us: None,
        }
    }

    #[test]
    fn snap_reveal_never_places_or_focuses_and_requires_visible_snap_evidence() {
        for automatic in [false, true] {
            let mut win = window("one", 10);
            let saved = snapped();
            win.layout = saved.clone();
            win.layout.visible = None;
            win.layout.arranged = Some(false);
            win.minimized = true;
            let mut executor = Executor::new(FakeNative::new(100));
            executor.native_mut().foreground = if automatic { (1, 999) } else { (77, 500) };
            executor.native_mut().insert(&win);
            let action = if automatic {
                Action::RestoreAutomatic(saved.clone())
            } else {
                Action::Restore(saved.clone())
            };
            let mut events = Vec::new();
            executor.step(vec![Work::Action(test_job(win, action))], &mut events);
            assert_eq!(executor.native_mut().calls, vec![Call::ShowNoActivate(10)]);
            assert!(!executor.native_mut().is_iconic(10));
            executor.step(vec![], &mut events);
            assert!(!events.iter().any(|event| matches!(
                event,
                Event::ActionResult {
                    outcome: Outcome::Verified,
                    ..
                }
            )));
            executor.native_mut().window_mut(10).layout = saved;
            executor.step(vec![], &mut events);
            executor.step(vec![], &mut events);
            assert!(events.iter().any(|event| matches!(
                event,
                Event::ActionResult {
                    outcome: Outcome::Verified,
                    ..
                }
            )));
            assert_eq!(executor.native_mut().calls, vec![Call::ShowNoActivate(10)]);
        }
    }

    #[test]
    fn accepted_snap_reveal_that_stays_minimized_times_out_until_observed_restored() {
        for automatic in [false, true] {
            let mut win = window("one", 10);
            let saved = snapped();
            win.layout = saved.clone();
            win.layout.visible = None;
            win.layout.arranged = Some(false);
            win.minimized = true;
            let key = win.identity.key();
            let mut executor = Executor::new(FakeNative::new(100));
            executor.native_mut().insert(&win);
            // Model a successfully posted command that never restores the window.
            // API acceptance must not fabricate visible/native arrangement evidence.
            executor.native_mut().window_mut(10).auto_apply = false;
            let action = if automatic {
                Action::RestoreAutomatic(saved.clone())
            } else {
                Action::Restore(saved.clone())
            };
            let mut events = Vec::new();
            executor.step(
                vec![Work::Action(test_job(win.clone(), action.clone()))],
                &mut events,
            );
            assert!(
                events
                    .iter()
                    .any(|event| matches!(event, Event::Accepted { .. }))
            );
            assert!(executor.native_mut().is_iconic(10));
            executor.native_mut().tick_ms += 2000;
            executor.step(vec![], &mut events);
            assert!(events.iter().any(|event| matches!(
                event,
                Event::ActionResult {
                    outcome: Outcome::TimedOut,
                    ..
                }
            )));
            assert!(!events.iter().any(|event| matches!(
                event,
                Event::ActionResult {
                    outcome: Outcome::Verified,
                    ..
                }
            )));
            assert!(executor.active[&key].timed_out);

            // Keep the outstanding slot rather than dispatch another native command.
            events.clear();
            executor.step(vec![Work::Action(test_job(win, action))], &mut events);
            assert!(events.iter().any(|event| matches!(
                event,
                Event::ActionResult {
                    outcome: Outcome::Busy,
                    ..
                }
            )));
            assert_eq!(executor.native_mut().calls, vec![Call::ShowNoActivate(10)]);

            // A late external restore releases the slot only after two visible matches.
            events.clear();
            let live = executor.native_mut().window_mut(10);
            live.iconic = false;
            live.visible = true;
            live.layout = saved;
            executor.step(vec![], &mut events);
            assert!(executor.active.contains_key(&key));
            assert!(events.is_empty());
            executor.step(vec![], &mut events);
            assert!(events.iter().any(|event| matches!(
                event,
                Event::ActionResult {
                    outcome: Outcome::Verified,
                    ..
                }
            )));
            assert!(!executor.active.contains_key(&key));
            assert_eq!(executor.native_mut().calls, vec![Call::ShowNoActivate(10)]);
        }
    }

    #[test]
    fn visible_matching_snap_needs_no_show_or_placement() {
        let mut win = window("one", 10);
        win.layout = snapped();
        let mut executor = Executor::new(FakeNative::new(100));
        executor.native_mut().insert(&win);
        let action = Action::RestoreAutomatic(win.layout.clone());
        executor.step(vec![Work::Action(test_job(win, action))], &mut Vec::new());
        assert!(executor.native_mut().calls.is_empty());
    }

    #[test]
    fn incompatible_snap_and_topology_changes_never_dispatch_placement() {
        for case in 0..3 {
            let mut win = window("one", 10);
            win.layout = snapped();
            let mut saved = win.layout.clone();
            match case {
                0 => saved.arranged = Some(false), // Normal destination, live snap.
                1 => saved.normal[0] += 100,       // Different saved arrangement.
                _ => {
                    // Adjustment discards saved evidence, still do not place.
                    win.minimized = true;
                    win.layout.visible = None;
                    win.layout.arranged = Some(false);
                    win.layout.work_area[0] += 100;
                }
            }
            let mut executor = Executor::new(FakeNative::new(100));
            executor.native_mut().insert(&win);
            let mut events = Vec::new();
            executor.step(
                vec![Work::Action(test_job(win, Action::RestoreAutomatic(saved)))],
                &mut events,
            );
            assert!(executor.native_mut().calls.is_empty());
            assert!(events.iter().any(|event| matches!(
                event,
                Event::ActionResult {
                    outcome: Outcome::Rejected,
                    ..
                }
            )));
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

    #[test]
    fn delayed_effect_requires_two_consecutive_matching_observations() {
        let win = window("one", 10);
        let key = win.identity.key();
        let mut h = Harness::new(&win);
        h.executor.native_mut().window_mut(10).auto_apply = false;
        h.model.click(
            &key,
            Click {
                minimized: false,
                was_foreground: true,
            },
        );
        let work = h.issue();
        h.step(work);
        assert_eq!(
            h.model.runtime(&key).unwrap().pending().unwrap().state(),
            OperationState::Accepted
        );
        // First matching sample, then an intermediate non-matching sample resets the counter.
        h.executor.native_mut().window_mut(10).iconic = true;
        h.advance(20);
        assert!(h.model.runtime(&key).unwrap().pending().is_some());
        h.executor.native_mut().window_mut(10).iconic = false;
        h.advance(20);
        h.executor.native_mut().window_mut(10).iconic = true;
        h.advance(20);
        assert!(h.model.runtime(&key).unwrap().pending().is_some());
        h.advance(20);
        assert!(h.model.runtime(&key).unwrap().pending().is_none());
        assert!(
            h.model.saved().windows[&key]
                .minimized_desktops
                .contains("A")
        );
    }

    #[test]
    fn partial_automatic_restore_failure_retains_quarantine_slot() {
        let mut win = window("one", 10);
        let key = win.identity.key();
        let mut h = Harness::new(&win);
        h.model.moving(10, false, 110, Some(layout(400)));
        h.model.set_auto_restore(true);
        win.minimized = true;
        h.executor.native_mut().window_mut(10).iconic = true;
        h.executor.native_mut().window_mut(10).visible = false;
        h.model.snapshot(snapshot("B", 120, vec![win.clone()]));
        // Stage 1 (minimize placement) is accepted; stage 2 (show) is declined.
        h.executor.native_mut().window_mut(10).auto_apply = false;
        h.executor.native_mut().window_mut(10).placement_results =
            VecDeque::from([Ok(()), Err("reveal declined".into())]);
        h.executor.native_mut().tick_ms = 130;
        h.model.snapshot(snapshot("A", 130, vec![win]));
        let work = h.issue();
        h.step(work);
        assert_eq!(
            h.model.runtime(&key).unwrap().pending().unwrap().state(),
            OperationState::Accepted
        );
        assert!(
            h.notices
                .iter()
                .any(|notice| notice.contains("Automatic restore reveal was declined"))
        );
        h.advance(2000);
        assert!(h.model.has_quarantined());
        assert!(h.model.runtime(&key).unwrap().blocked());
    }

    #[test]
    fn timeout_quarantines_and_late_completion_releases_without_focus_or_memory() {
        let mut win = window("one", 10);
        win.minimized = true;
        let key = win.identity.key();
        let mut h = Harness::new(&win);
        h.executor.native_mut().window_mut(10).auto_apply = false;
        h.model.click(
            &key,
            Click {
                minimized: true,
                was_foreground: false,
            },
        );
        let work = h.issue();
        h.step(work);
        assert_eq!(h.executor.poll_interval(), Some(Duration::from_millis(20)));
        h.advance(2000);
        assert!(h.model.has_quarantined());
        assert_eq!(
            h.executor.poll_interval(),
            Some(Duration::from_millis(1000))
        );
        // Late effect arrives after quarantine: two matching observations release the slot,
        // never call SetForeground, and do not commit the layout.
        h.executor.native_mut().window_mut(10).iconic = false;
        h.executor.native_mut().window_mut(10).visible = true;
        h.advance(20);
        assert!(h.model.has_quarantined());
        h.advance(20);
        assert!(!h.model.has_quarantined());
        assert!(h.model.runtime(&key).unwrap().pending().is_none());
        assert!(h.model.saved().windows[&key].layouts.is_empty());
        assert!(
            !h.executor
                .native_mut()
                .calls
                .iter()
                .any(|call| matches!(call, Call::SetForeground(_)))
        );
    }

    #[test]
    fn temporary_disappearance_keeps_slot_until_identity_dies() {
        let win = window("one", 10);
        let key = win.identity.key();
        let mut h = Harness::new(&win);
        h.executor.native_mut().window_mut(10).auto_apply = false;
        h.model.click(
            &key,
            Click {
                minimized: false,
                was_foreground: true,
            },
        );
        let work = h.issue();
        h.step(work);
        // Snapshot omits the window, but its live identity still validates: keep the slot.
        h.model.snapshot(snapshot("A", 120, vec![]));
        h.advance(20);
        assert!(h.model.runtime(&key).unwrap().pending().is_some());
        // Now the live identity dies (HWND destroyed or recycled to another process).
        h.executor
            .native_mut()
            .window_mut(10)
            .identity
            .process_created = 999;
        h.advance(20);
        assert!(h.model.runtime(&key).is_none());
    }

    #[test]
    fn superseding_intent_cancels_queued_work_before_dispatch() {
        let win = window("one", 10);
        let key = win.identity.key();
        let mut h = Harness::new(&win);
        h.executor.native_mut().tick_ms = 200;
        h.model.snapshot(snapshot("B", 200, vec![win]));
        let work = h.issue();
        // User pauses before the worker drains its queue.
        h.model.pause(true);
        h.step(work);
        assert!(h.executor.native_mut().calls.is_empty());
        assert!(h.model.runtime(&key).unwrap().pending().is_none());
    }

    #[test]
    fn restore_converted_to_raise_after_external_reveal_commits_observed_layout() {
        let mut win = window("one", 10);
        win.minimized = true;
        let key = win.identity.key();
        let mut h = Harness::new(&win);
        h.model.click(
            &key,
            Click {
                minimized: true,
                was_foreground: false,
            },
        );
        let work = h.issue();
        // Windows reveals and moves the window before the worker dispatches the request.
        let external = layout(520);
        {
            let live = h.executor.native_mut().window_mut(10);
            live.iconic = false;
            live.visible = true;
            live.layout = external.clone();
        }
        h.step(work);
        h.advance(20);
        assert_eq!(h.model.saved().windows[&key].layouts["A"], external);
        assert!(
            !h.executor
                .native_mut()
                .calls
                .iter()
                .any(|call| matches!(call, Call::SetPlacement { .. }))
        );
        assert!(
            h.executor
                .native_mut()
                .calls
                .contains(&Call::SetForeground(10))
        );
    }

    #[test]
    fn guarded_focus_skips_when_user_moved_to_an_unrelated_foreground_window() {
        for (foreground, pid, popup, expected) in [
            ((1, 999), 999, None, Some(10)),
            ((55, 500), 999, None, Some(10)),
            ((77, 500), 999, None, None),
            ((1, 999), 999, Some(20), Some(20)),
        ] {
            let mut win = window("one", 10);
            win.minimized = true;
            win.popup = popup;
            let key = win.identity.key();
            let mut h = Harness::new(&win);
            h.focus_from.insert(key.clone(), 55);
            h.executor.native_mut().self_pid = pid;
            h.executor.native_mut().foreground = foreground;
            h.model.click(
                &key,
                Click {
                    minimized: true,
                    was_foreground: false,
                },
            );
            let work = h.issue();
            h.step(work);
            h.advance(20);
            let focused = h.executor.native_mut().calls.iter().find_map(|call| {
                if let Call::SetForeground(target) = call {
                    Some(*target)
                } else {
                    None
                }
            });
            assert_eq!(focused, expected);
        }
    }
}
