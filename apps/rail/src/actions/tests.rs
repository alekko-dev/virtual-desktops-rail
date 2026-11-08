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
        z_above: Vec::new(),
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
    PlaceBehind {
        hwnd: usize,
        above: usize,
    },
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
    z_above: Vec<usize>,
    cloaked: bool,
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
            z_above: window.z_above.clone(),
            cloaked: false,
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
            let auto_apply = live.auto_apply;
            if auto_apply {
                live.iconic = false;
                live.visible = true;
                live.z_above.clear();
                for (&other_hwnd, other) in &mut self.windows {
                    if other_hwnd != hwnd {
                        other.z_above.retain(|&peer| peer != hwnd);
                        other.z_above.insert(0, hwnd);
                    }
                }
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
        let auto_apply = live.auto_apply;
        if auto_apply {
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
                    live.z_above.clear();
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
                    live.z_above.clear();
                    live.layout.maximized = true;
                    live.layout.visible = Some(live.layout.work_area);
                }
            }
            if matches!(
                stage,
                PlacementStage::ShowNoActivate | PlacementStage::ShowMaximized
            ) {
                for (&other_hwnd, other) in &mut self.windows {
                    if other_hwnd != hwnd {
                        other.z_above.retain(|&peer| peer != hwnd);
                        other.z_above.insert(0, hwnd);
                    }
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
    fn z_above(&mut self, hwnd: usize) -> Vec<usize> {
        self.windows
            .get(&hwnd)
            .filter(|live| !live.iconic)
            .map(|live| live.z_above.clone())
            .unwrap_or_default()
    }
    fn is_z_order_target(&mut self, hwnd: usize) -> bool {
        self.windows
            .get(&hwnd)
            .is_some_and(|live| live.visible && !live.iconic && !live.cloaked)
    }
    fn is_below(&mut self, hwnd: usize, above: usize) -> bool {
        self.windows
            .get(&hwnd)
            .is_some_and(|live| live.z_above.contains(&above))
    }
    fn place_behind(&mut self, hwnd: usize, above: usize) -> Result<(), String> {
        self.calls.push(Call::PlaceBehind { hwnd, above });
        if let Some(auto_apply) = self.windows.get(&hwnd).map(|live| live.auto_apply) {
            if auto_apply {
                for other in self.windows.values_mut() {
                    other.z_above.retain(|&peer| peer != hwnd);
                }
                let above_chain = self
                    .windows
                    .get(&above)
                    .map(|live| live.z_above.clone())
                    .unwrap_or_default();
                let mut z_above = vec![above];
                for peer in above_chain {
                    if peer != hwnd && !z_above.contains(&peer) {
                        z_above.push(peer);
                    }
                }
                self.windows.get_mut(&hwnd).unwrap().z_above = z_above;
                for (&other_hwnd, other) in &mut self.windows {
                    if other_hwnd != hwnd
                        && other_hwnd != above
                        && let Some(pos) = other.z_above.iter().position(|&peer| peer == above)
                    {
                        other.z_above.insert(pos + 1, hwnd);
                    }
                }
            }
            return Ok(());
        }
        if self.windows.values().any(|live| live.popup == Some(hwnd)) {
            return Ok(());
        }
        Err("Missing window".into())
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
    snap_notice: crate::snap_notice::Lifecycle,
    notice: String,
}

impl Harness {
    fn new(window: &Window) -> Self {
        // Existing scenarios explicitly exercise the legacy decline policy.
        let mut model = Model::new(Saved {
            minimize_snapped_elsewhere: false,
            ..Saved::default()
        });
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
            snap_notice: crate::snap_notice::Lifecycle::default(),
            notice: String::new(),
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
        self.deliver(events);
    }

    fn deliver(&mut self, events: Vec<Event>) {
        for event in events {
            match event {
                Event::ObservedArrangement {
                    z_above,
                    request,
                    layout,
                    tick,
                } => {
                    if layout.visible.is_some() {
                        self.model.observed_z_above(&request, z_above, tick);
                    }
                    self.model.observed_arrangement(&request, &layout, tick);
                }
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
                    self.result(&key, sequence, tick, outcome);
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
                Event::SnapPreservation(notice) => {
                    self.notices.push(crate::snap_notice::TEXT.into());
                    self.snap_notice
                        .rejected(*notice, &self.model, &mut self.notice);
                }
                _ => {}
            }
        }
    }

    fn advance(&mut self, delta_ms: u64) {
        self.executor.native_mut().tick_ms += delta_ms;
        self.step(vec![]);
    }

    fn result(&mut self, key: &str, sequence: u64, tick: u64, outcome: Outcome) {
        let settled = self.model.settle(key, sequence, tick, outcome);
        self.snap_notice.settled(
            &self.model,
            (key, sequence, tick, outcome),
            settled.as_ref(),
            &mut self.notice,
        );
        self.snap_notice.reconcile(&self.model, &mut self.notice);
    }

    fn frame(&mut self, frame: Snapshot) {
        self.snap_notice
            .observe(&self.model, &frame, &mut self.notice);
        self.model.snapshot(frame);
        self.snap_notice.reconcile(&self.model, &mut self.notice);
    }
}

fn snap_notice_fixture() -> (Harness, Window) {
    let mut win = window("one", 10);
    win.layout = snapped();
    win.layout.maximized = true;
    win.layout.visible = None;
    win.layout.arranged = Some(false);
    win.minimized = true;
    let key = win.identity.key();
    let mut h = Harness::new(&win);
    h.model
        .record_mut(&key)
        .unwrap()
        .layouts
        .insert("A".into(), snapped());
    h.model.click(
        &key,
        Click {
            minimized: true,
            was_foreground: false,
        },
    );
    h.executor.native_mut().tick_ms = 120;
    assert_rejected_snap_request(&mut h, &key, false);
    assert_eq!(h.notice, crate::snap_notice::TEXT);
    (h, win)
}

#[test]
fn snap_notice_clears_only_after_current_verified_restore_or_raise() {
    for raise in [false, true] {
        let (mut h, mut win) = snap_notice_fixture();
        let key = win.identity.key();
        win.layout = snapped();
        if !raise {
            win.layout.visible = None;
        }
        win.minimized = !raise;
        h.executor.native_mut().insert(&win);
        h.model.click(
            &key,
            Click {
                minimized: !raise,
                was_foreground: false,
            },
        );
        let work = h.issue();
        h.step(work);
        // Native acceptance alone must leave the notice intact.
        assert_eq!(h.notice, crate::snap_notice::TEXT);
        h.executor.native_mut().window_mut(10).layout = snapped();
        h.advance(30);
        h.advance(30);
        assert!(h.notice.is_empty());
        assert!(h.model.runtime(&key).unwrap().pending().is_none());
    }
}

#[test]
fn snap_notice_visible_observation_requires_resolved_target_and_fresh_evidence() {
    for mismatch in 0..8 {
        let (mut h, mut win) = snap_notice_fixture();
        win.minimized = false;
        win.layout = snapped();
        match mismatch {
            0 => win.layout.arranged = Some(false),
            1 => win.layout.arranged = None,
            2 => win.layout.normal[0] += 40,
            3 => win.layout.visible = Some([100, 100, 740, 460]),
            4 => win.layout.maximized = true,
            5 => win.layout.dpi += 24,
            6 => win.layout.work_area[0] += 20,
            _ => win.minimized = true,
        }
        h.frame(snapshot("A", 2300, vec![win.clone()]));
        assert_eq!(h.notice, crate::snap_notice::TEXT, "mismatch {mismatch}");
        win.minimized = false;
        win.layout = snapped();
        h.frame(snapshot("A", 2300, vec![win.clone()]));
        assert_eq!(h.notice, crate::snap_notice::TEXT, "equal tick");
        h.frame(snapshot("A", 2301, vec![win]));
        assert!(h.notice.is_empty());
    }
    let (mut h, mut win) = snap_notice_fixture();
    win.minimized = false;
    win.layout = snapped();
    h.frame(snapshot("A", 120, vec![win.clone()]));
    assert_eq!(h.notice, crate::snap_notice::TEXT, "pre-rejection snapshot");
    h.model
        .captured(&win.identity.key(), h.model.generation(), 2500, snapped());
    h.frame(snapshot("A", 2400, vec![win.clone()]));
    assert_eq!(h.notice, crate::snap_notice::TEXT, "older than capture");
    h.frame(snapshot("A", 2501, vec![win]));
    assert!(h.notice.is_empty());
}

#[test]
fn snap_notice_unrelated_success_and_stale_late_results_do_not_clear() {
    let (mut h, win) = snap_notice_fixture();
    let key = win.identity.key();
    let other = window("other", 20);
    h.frame(snapshot("A", 2300, vec![win.clone(), other.clone()]));
    h.executor.native_mut().insert(&other);
    h.model.click(
        &other.identity.key(),
        Click {
            minimized: false,
            was_foreground: false,
        },
    );
    let work = h.issue();
    h.step(work);
    h.advance(30);
    assert_eq!(h.notice, crate::snap_notice::TEXT);
    h.result(&key, 1, 2400, Outcome::Verified); // already released rejection
    assert_eq!(h.notice, crate::snap_notice::TEXT);
    for mode in 0..3 {
        let (mut h, win) = snap_notice_fixture();
        let key = win.identity.key();
        h.model.click(
            &key,
            Click {
                minimized: true,
                was_foreground: false,
            },
        );
        let request = h.model.take_requests(2300).pop().unwrap();
        h.model.accepted(
            &key,
            request.sequence,
            Acceptance::as_requested(&request.action),
        );
        match mode {
            0 => h.result(&key, request.sequence, 2350, Outcome::TimedOut),
            1 => h.model.click(
                &key,
                Click {
                    minimized: false,
                    was_foreground: false,
                },
            ),
            _ => h.model.snapshot(snapshot("B", 2350, vec![win])),
        }
        // Keep Snap displayed to isolate its lifecycle from the timeout UI.
        h.result(&key, request.sequence, 2400, Outcome::Verified);
        assert_eq!(h.notice, crate::snap_notice::TEXT, "mode {mode}");
    }
}

#[test]
fn snap_notice_identity_removal_replacement_and_other_notices() {
    for replacement in 0..3 {
        let (mut h, mut win) = snap_notice_fixture();
        let windows = match replacement {
            0 => vec![],
            1 => {
                win.identity.token = "replacement".into();
                vec![win]
            }
            _ => {
                win.hwnd = 11;
                vec![win]
            }
        };
        h.frame(snapshot("A", 2300, windows));
        assert!(h.notice.is_empty());
    }
    for text in [
        crate::ui::ACTION_TIMEOUT_NOTICE,
        "Saved layouts could not be written",
        "Diagnostic report saved",
    ] {
        let (mut h, _) = snap_notice_fixture();
        h.notice = text.into();
        h.frame(snapshot("A", 2300, vec![]));
        assert_eq!(h.notice, text);
        assert_eq!(h.snap_notice.previous(text.into()), text);
        assert!(
            h.snap_notice
                .previous(crate::snap_notice::TEXT.into())
                .is_empty()
        );
    }
    let (mut h, win) = snap_notice_fixture();
    let key = win.identity.key();
    h.model.click(
        &key,
        Click {
            minimized: true,
            was_foreground: false,
        },
    );
    let request = h.model.take_requests(2300).pop().unwrap();
    h.frame(snapshot("A", 2301, vec![]));
    assert_eq!(
        h.notice,
        crate::snap_notice::TEXT,
        "temporary pending omission"
    );
    h.result(&key, request.sequence, 2302, Outcome::IdentityLost);
    assert!(h.notice.is_empty());
}

#[test]
fn snap_notice_stale_rejection_cannot_replace_a_newer_notice() {
    for stale in 0..3 {
        let (mut h, win) = snap_notice_fixture();
        let key = win.identity.key();
        h.model.click(
            &key,
            Click {
                minimized: true,
                was_foreground: false,
            },
        );
        let request = h.model.take_requests(2300).pop().unwrap();
        match stale {
            0 => h.model.click(
                &key,
                Click {
                    minimized: false,
                    was_foreground: false,
                },
            ),
            1 => h.model.snapshot(snapshot("B", 2301, vec![win])),
            _ => h.result(&key, request.sequence, 2301, Outcome::TimedOut),
        }
        h.notice = "Newer storage warning".into();
        h.snap_notice.rejected(
            crate::snap_notice::SnapNotice::new(request, 2302),
            &h.model,
            &mut h.notice,
        );
        assert_eq!(h.notice, "Newer storage warning");
    }
}

#[test]
fn snap_notice_normal_target_requires_confirmed_unsnap_and_matching_geometry() {
    let (mut h, mut win) = snap_notice_fixture();
    let key = win.identity.key();
    // Establish a new rejection against an ordinary destination while live Snap
    // evidence still forbids placement.
    h.model
        .record_mut(&key)
        .unwrap()
        .layouts
        .insert("B".into(), layout(100));
    h.model.set_auto_restore(true);
    win.layout = snapped();
    win.minimized = false;
    h.executor.native_mut().insert(&win);
    h.model.snapshot(snapshot("B", 2300, vec![win.clone()]));
    h.executor.native_mut().tick_ms = 2300;
    let work = h.issue();
    h.step(work);
    assert_eq!(h.notice, crate::snap_notice::TEXT);
    win.layout = layout(150);
    h.frame(snapshot("B", 2310, vec![win.clone()]));
    assert_eq!(h.notice, crate::snap_notice::TEXT);
    win.layout = layout(100);
    win.layout.arranged = None;
    h.frame(snapshot("B", 2320, vec![win.clone()]));
    assert_eq!(h.notice, crate::snap_notice::TEXT);
    win.layout.arranged = Some(false);
    h.frame(snapshot("B", 2330, vec![win]));
    assert!(h.notice.is_empty());
}

#[test]
fn snap_notice_automatic_verified_recovery_clears() {
    let (mut h, mut win) = snap_notice_fixture();
    let key = win.identity.key();
    h.model
        .record_mut(&key)
        .unwrap()
        .layouts
        .insert("B".into(), snapped());
    h.model.set_auto_restore(true);
    win.layout = snapped();
    win.layout.visible = None;
    h.model.snapshot(snapshot("B", 2300, vec![win.clone()]));
    h.executor.native_mut().insert(&win);
    h.executor.native_mut().tick_ms = 2300;
    let work = h.issue();
    h.step(work);
    assert_eq!(h.notice, crate::snap_notice::TEXT);
    h.executor.native_mut().window_mut(10).layout = snapped();
    h.advance(30);
    h.advance(30);
    assert!(h.notice.is_empty());
}

#[test]
fn minimized_snap_declines_ordinary_saved_layout_for_manual_and_automatic_requests() {
    for automatic in [false, true] {
        let mut win = window("one", 10);
        win.layout = snapped();
        let mut h = Harness::new(&win);
        let mut ordinary = win.layout.clone();
        ordinary.arranged = Some(false);
        ordinary.visible = Some([131, 100, 771, 460]);
        let key = win.identity.key();
        h.model
            .record_mut(&key)
            .unwrap()
            .layouts
            .insert(if automatic { "B" } else { "A" }.into(), ordinary);
        h.model.set_auto_restore(automatic);
        win.minimized = true;
        win.layout.visible = None;
        win.layout.arranged = Some(false);
        h.model.snapshot(snapshot("A", 110, vec![win.clone()]));
        h.executor.native_mut().insert(&win);
        h.executor.native_mut().tick_ms = 120;
        if automatic {
            h.model.snapshot(snapshot("B", 120, vec![win]));
        } else {
            h.model.click(
                &key,
                Click {
                    minimized: true,
                    was_foreground: false,
                },
            );
        }
        assert_rejected_snap_request(&mut h, &key, automatic);
    }
}

#[test]
fn snap_desktop_policy_defaults_to_minimize_and_legacy_option_retains_visible_snap() {
    assert!(Saved::default().minimize_snapped_elsewhere);
    for enabled in [false, true] {
        for iconic in [false, true] {
            let mut win = window("one", 10);
            win.layout = snapped();
            let key = win.identity.key();
            let mut h = Harness::new(&win);
            h.model.set_minimize_snapped_elsewhere(enabled);
            h.model.set_auto_restore(true);
            let mut ordinary = snapped();
            ordinary.arranged = Some(false);
            ordinary.visible = Some([131, 100, 771, 460]);
            let record = h.model.record_mut(&key).unwrap();
            record.layouts.insert("A".into(), ordinary);
            record.layouts.insert("B".into(), snapped());
            h.model.snapshot(snapshot("B", 110, vec![win.clone()]));
            if iconic {
                win.minimized = true;
                win.layout.visible = None;
                win.layout.arranged = Some(false);
            }
            h.executor.native_mut().insert(&win);
            h.model.snapshot(snapshot("A", 120, vec![win]));
            h.executor.native_mut().tick_ms = 120;
            let saved = serde_json::to_value(h.model.saved()).unwrap();
            let work = h.issue();
            h.step(work);
            h.advance(30);
            assert!(h.model.runtime(&key).unwrap().pending().is_none());
            assert!(!h.model.has_quarantined());
            assert_eq!(
                h.model.runtime(&key).unwrap().window().minimized,
                enabled || iconic
            );
            assert_eq!(
                h.executor.native_mut().calls,
                if enabled && !iconic {
                    vec![Call::MinimizeArranged(10)]
                } else {
                    vec![]
                }
            );
            assert_eq!(serde_json::to_value(h.model.saved()).unwrap(), saved);
            assert_eq!(h.notices.is_empty(), enabled);
        }
    }
}

#[test]
fn snap_desktop_policy_queries_fresh_native_state_and_preserves_manual_reveal() {
    let mut win = window("one", 10);
    let key = win.identity.key();
    let mut h = Harness::new(&win);
    h.model.set_minimize_snapped_elsewhere(true);
    h.model.set_auto_restore(true);
    h.model
        .record_mut(&key)
        .unwrap()
        .layouts
        .insert("B".into(), win.layout.clone());
    h.model.snapshot(snapshot("B", 120, vec![win.clone()]));
    // Native snap is newer than the controller snapshot, with no retained evidence.
    h.executor.native_mut().window_mut(10).layout = snapped();
    h.executor.native_mut().tick_ms = 120;
    let work = h.issue();
    h.step(work);
    h.advance(30);
    assert_eq!(
        h.executor.native_mut().calls,
        vec![Call::MinimizeArranged(10)]
    );
    assert!(h.model.runtime(&key).unwrap().window().minimized);
    let saved_layout = h.model.saved().windows[&key].layouts["B"].clone();
    let fallback = h.model.saved().windows[&key].last_usable.clone();
    h.model
        .record_mut(&key)
        .unwrap()
        .minimized_desktops
        .insert("B".into());
    h.model.click(
        &key,
        Click {
            minimized: true,
            was_foreground: false,
        },
    );
    let work = h.issue();
    assert!(
        matches!(&work[0], Work::Action(job) if matches!(job.request.action, Action::RevealSnap(_)))
    );
    h.step(work);
    h.advance(30);
    assert!(!h.model.runtime(&key).unwrap().window().minimized);
    assert_eq!(
        h.executor.native_mut().calls,
        vec![
            Call::MinimizeArranged(10),
            Call::ShowNoActivate(10),
            Call::SetForeground(10)
        ]
    );
    assert_eq!(h.model.saved().windows[&key].layouts["B"], saved_layout);
    assert_eq!(h.model.saved().windows[&key].last_usable, fallback);
    assert!(
        !h.model.saved().windows[&key]
            .minimized_desktops
            .contains("B")
    );
    assert!(h.notices.is_empty());
    // After a genuine visible unsnap, normal destination restoration works again.
    win.layout = layout(100);
    h.frame(snapshot("B", 200, vec![win.clone()]));
    h.model
        .record_mut(&key)
        .unwrap()
        .layouts
        .insert("A".into(), layout(200));
    h.executor.native_mut().insert(&win);
    h.model.snapshot(snapshot("A", 210, vec![win]));
    h.executor.native_mut().tick_ms = 210;
    let work = h.issue();
    h.step(work);
    h.advance(30);
    assert!(!h.model.runtime(&key).unwrap().window().minimized);
    assert!(
        h.executor
            .native_mut()
            .calls
            .iter()
            .any(|call| matches!(call, Call::SetPlacement { .. }))
    );
}

#[test]
fn snap_desktop_policy_restores_matching_snap_without_placement() {
    let mut win = window("one", 10);
    win.layout = snapped();
    let key = win.identity.key();
    let mut h = Harness::new(&win);
    h.model.set_minimize_snapped_elsewhere(true);
    h.model.set_auto_restore(true);
    h.model
        .record_mut(&key)
        .unwrap()
        .layouts
        .insert("B".into(), snapped());
    h.model.snapshot(snapshot("B", 120, vec![win]));
    h.executor.native_mut().tick_ms = 120;
    let work = h.issue();
    h.step(work);
    h.advance(30);
    assert!(!h.model.runtime(&key).unwrap().window().minimized);
    assert!(h.executor.native_mut().calls.is_empty());
    assert!(h.notices.is_empty());
}

#[test]
fn matching_snap_uses_native_restore_bounds_even_outside_work_area() {
    let mut win = window("one", 10);
    win.layout = snapped();
    win.layout.normal = [-8, 100, 632, 460];
    let saved = win.layout.clone();
    let mut executor = Executor::new(FakeNative::new(100));
    executor.native_mut().insert(&win);
    let mut events = vec![];
    executor.step(
        vec![Work::Action(test_job(
            win,
            Action::RestoreAutomatic(saved.clone()),
        ))],
        &mut events,
    );
    executor.step(vec![], &mut events);
    assert!(executor.native_mut().calls.is_empty());
    assert!(events.iter().any(|event| matches!(event, Event::Accepted { acceptance, .. } if acceptance.target.as_ref() == Some(&saved))));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::ActionResult {
            outcome: Outcome::Verified,
            ..
        }
    )));
}

#[test]
fn snap_policy_minimize_acceptance_requires_verification_and_late_result_preserves_memory() {
    let mut win = window("one", 10);
    win.layout = snapped();
    let key = win.identity.key();
    let mut h = Harness::new(&win);
    h.model.set_minimize_snapped_elsewhere(true);
    h.model.set_auto_restore(true);
    h.model
        .record_mut(&key)
        .unwrap()
        .layouts
        .insert("B".into(), layout(100));
    h.model.snapshot(snapshot("B", 120, vec![win]));
    let saved = serde_json::to_value(h.model.saved()).unwrap();
    h.executor.native_mut().window_mut(10).auto_apply = false;
    h.executor.native_mut().tick_ms = 120;
    let work = h.issue();
    h.step(work);
    assert!(!h.model.runtime(&key).unwrap().window().minimized);
    assert_eq!(
        h.model
            .runtime(&key)
            .unwrap()
            .pending()
            .unwrap()
            .execution()
            .unwrap()
            .effect,
        Effect::Minimize
    );
    h.advance(2000);
    assert!(h.model.has_quarantined());
    h.executor.native_mut().window_mut(10).iconic = true;
    h.executor.native_mut().window_mut(10).visible = false;
    h.advance(20);
    h.advance(20);
    assert!(h.model.runtime(&key).unwrap().pending().is_none());
    assert!(h.model.runtime(&key).unwrap().window().minimized);
    assert_eq!(serde_json::to_value(h.model.saved()).unwrap(), saved);
    assert_eq!(
        h.executor.native_mut().calls,
        vec![Call::MinimizeArranged(10)]
    );
}

#[test]
fn minimized_from_maximized_declines_saved_snap_for_manual_and_automatic_requests() {
    for automatic in [false, true] {
        let mut win = window("one", 10);
        win.layout = snapped();
        win.layout.maximized = true;
        win.layout.visible = None;
        win.layout.arranged = Some(false);
        win.minimized = true;
        let key = win.identity.key();
        let mut h = Harness::new(&win);
        h.model
            .record_mut(&key)
            .unwrap()
            .layouts
            .insert(if automatic { "B" } else { "A" }.into(), snapped());
        h.model.set_auto_restore(automatic);
        if automatic {
            h.model.snapshot(snapshot("B", 120, vec![win]));
        } else {
            h.model.click(
                &key,
                Click {
                    minimized: true,
                    was_foreground: false,
                },
            );
        }
        h.executor.native_mut().tick_ms = 120;
        assert_rejected_snap_request(&mut h, &key, automatic);
    }
}

fn assert_rejected_snap_request(h: &mut Harness, key: &str, automatic: bool) {
    let saved = h.model.saved().clone();
    let work = h.issue();
    assert_eq!(work.len(), 1);
    assert!(matches!(&work[0], Work::Action(job) if job.request.action.automatic() == automatic));
    h.step(work);
    h.advance(2100);
    assert!(h.executor.native_mut().calls.is_empty());
    assert!(h.executor.native_mut().window_mut(10).iconic);
    assert!(h.model.runtime(key).unwrap().pending().is_none());
    assert!(!h.model.has_quarantined());
    assert_eq!(h.executor.poll_interval(), None);
    assert!(h.issue().is_empty());
    assert_eq!(
        serde_json::to_value(h.model.saved()).unwrap(),
        serde_json::to_value(saved).unwrap()
    );
    assert!(h.notices.iter().any(|notice| {
        notice.contains("Restoration would replace or lose a native Snap arrangement")
    }));
}

#[test]
fn user_minimized_snap_destinations_are_not_automatically_dispatched() {
    for maximized in [false, true] {
        let mut win = window("one", 10);
        win.minimized = true;
        win.layout = snapped();
        win.layout.maximized = maximized;
        win.layout.visible = None;
        win.layout.arranged = Some(false);
        let key = win.identity.key();
        let mut h = Harness::new(&win);
        let record = h.model.record_mut(&key).unwrap();
        record.layouts.insert("B".into(), snapped());
        record.minimized_desktops.insert("B".into());
        h.model.set_auto_restore(true);
        h.model.snapshot(snapshot("B", 120, vec![win]));
        let work = h.issue();
        assert!(work.is_empty());
        h.step(work);
        assert!(h.executor.native_mut().calls.is_empty());
        assert!(h.executor.native_mut().window_mut(10).iconic);
        assert!(h.model.runtime(&key).unwrap().pending().is_none());
    }
}

#[test]
fn snap_evidence_does_not_follow_changed_display_or_replaced_identity() {
    for replace_identity in [false, true] {
        let mut win = window("one", 10);
        win.layout = snapped();
        let mut h = Harness::new(&win);
        win.minimized = true;
        win.layout.visible = None;
        win.layout.arranged = Some(false);
        if replace_identity {
            win.identity.process_created += 1;
        } else {
            win.layout.work_area[0] += 10;
        }
        h.model.snapshot(snapshot("A", 120, vec![win.clone()]));
        h.model.click(
            &win.identity.key(),
            Click {
                minimized: true,
                was_foreground: false,
            },
        );
        let work = h.issue();
        assert_eq!(work.len(), 1);
        assert!(
            matches!(&work[0], Work::Action(job) if job.request.retained_arrangement.is_none())
        );
    }
}

#[test]
fn older_snapshots_cannot_clear_newer_identity_validated_snap_capture() {
    let mut win = window("one", 10);
    win.layout = snapped();
    win.layout.arranged = Some(false);
    win.layout.visible = Some([131, 100, 771, 460]);
    let key = win.identity.key();
    let mut h = Harness::new(&win);
    h.model.captured(&key, h.model.generation(), 200, snapped());
    // A queued pre-capture visible unsnapped observation is not current evidence.
    h.model.snapshot(snapshot("A", 150, vec![win.clone()]));
    win.minimized = true;
    win.layout.visible = None;
    h.model.snapshot(snapshot("A", 210, vec![win]));
    h.model.click(
        &key,
        Click {
            minimized: true,
            was_foreground: false,
        },
    );
    let work = h.issue();
    assert_eq!(work.len(), 1);
    assert!(matches!(&work[0], Work::Action(job) if job.request.retained_arrangement.is_some()));
}

#[test]
fn confirmed_visible_unsnap_clears_retained_evidence_before_minimized_restore() {
    let mut win = window("one", 10);
    win.layout = snapped();
    let mut h = Harness::new(&win);
    win.layout.arranged = Some(false);
    win.layout.visible = Some([131, 100, 771, 460]);
    h.model
        .record_mut(&win.identity.key())
        .unwrap()
        .layouts
        .insert("A".into(), win.layout.clone());
    h.model.snapshot(snapshot("A", 110, vec![win.clone()]));
    win.minimized = true;
    win.layout.visible = None;
    h.model.snapshot(snapshot("A", 120, vec![win.clone()]));
    h.executor.native_mut().insert(&win);
    h.model.click(
        &win.identity.key(),
        Click {
            minimized: true,
            was_foreground: false,
        },
    );
    let work = h.issue();
    assert!(matches!(&work[0], Work::Action(job) if job.request.retained_arrangement.is_none()));
    h.step(work);
    assert!(
        h.executor
            .native_mut()
            .calls
            .iter()
            .any(|call| matches!(call, Call::SetPlacement { .. }))
    );
}

#[test]
fn minimize_retains_live_snap_newer_than_the_controller_snapshot() {
    let mut win = window("one", 10);
    win.layout = snapped();
    win.layout.arranged = Some(false);
    win.layout.visible = Some([131, 100, 771, 460]);
    let key = win.identity.key();
    let mut h = Harness::new(&win);
    h.model
        .record_mut(&key)
        .unwrap()
        .layouts
        .insert("A".into(), win.layout.clone());
    // Windows snapped after the last controller observation. The PR's fresh
    // pre-minimize query must retain that identity-validated evidence too.
    h.executor.native_mut().window_mut(10).layout = snapped();
    h.executor.native_mut().tick_ms = 110;
    h.model.click(
        &key,
        Click {
            minimized: false,
            was_foreground: true,
        },
    );
    let work = h.issue();
    h.step(work);
    h.advance(20);
    assert_eq!(
        h.executor.native_mut().calls,
        vec![Call::MinimizeArranged(10)]
    );
    assert!(h.model.runtime(&key).unwrap().pending().is_none());
    win.minimized = true;
    win.layout.visible = None;
    h.model.snapshot(snapshot("A", 130, vec![win.clone()]));
    h.executor.native_mut().window_mut(10).layout = win.layout;
    h.executor.native_mut().tick_ms = 130;
    h.executor.native_mut().calls.clear();
    h.model.click(
        &key,
        Click {
            minimized: true,
            was_foreground: false,
        },
    );
    assert_rejected_snap_request(&mut h, &key, false);
}

#[test]
fn superseded_minimize_keeps_fresh_snap_evidence_before_or_after_destination_snapshot() {
    for enabled in [false, true] {
        for delayed in [false, true] {
            let mut win = window("one", 10);
            win.layout = snapped();
            win.layout.arranged = Some(false);
            win.layout.visible = Some([131, 100, 771, 460]);
            let key = win.identity.key();
            let mut h = Harness::new(&win);
            h.model.set_minimize_snapped_elsewhere(enabled);
            h.model.set_auto_restore(true);
            h.model
                .record_mut(&key)
                .unwrap()
                .layouts
                .insert("B".into(), win.layout.clone());
            h.model.click(
                &key,
                Click {
                    minimized: false,
                    was_foreground: true,
                },
            );
            h.executor.native_mut().window_mut(10).layout = snapped();
            h.executor.native_mut().tick_ms = 120;
            let work = h.issue();
            let mut events = vec![];
            assert!(h.executor.step(work, &mut events));
            if !delayed {
                h.deliver(std::mem::take(&mut events));
            }
            win.minimized = true;
            win.layout.visible = None;
            h.model.snapshot(snapshot("B", 130, vec![win.clone()]));
            let saved = serde_json::to_value(h.model.saved()).unwrap();
            h.deliver(events);
            h.advance(20); // Superseded minimize verifies after the desktop switch.
            assert!(h.model.runtime(&key).unwrap().pending().is_none());
            assert_eq!(serde_json::to_value(h.model.saved()).unwrap(), saved);
            h.executor.native_mut().window_mut(10).layout = win.layout;
            let work = h.issue();
            assert!(
                matches!(&work[0], Work::Action(job) if job.request.retained_arrangement.is_some())
            );
            h.step(work);
            h.advance(30);
            assert_eq!(
                h.executor.native_mut().calls,
                vec![Call::MinimizeArranged(10)]
            );
            assert!(h.model.runtime(&key).unwrap().pending().is_none());
            assert!(!h.model.has_quarantined());
            assert_eq!(serde_json::to_value(h.model.saved()).unwrap(), saved);
            assert_eq!(h.notices.is_empty(), enabled);
        }
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
            z_above: window.z_above.clone(),
            minimize_snapped_elsewhere: false,
            key: window.identity.key(),
            window,
            retained_arrangement: None,
            arrangement_revision: 0,
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
fn automatic_snap_restore_places_window_back_behind_uncloaked_foreground_peer() {
    let mut managed = window("managed", 10);
    managed.layout = snapped();
    managed.z_above = vec![30, 20];
    let peer_a = window("peer-a", 20);
    let peer_b = window("peer-b", 30);
    let key = managed.identity.key();
    let mut h = Harness::new(&managed);
    h.executor.native_mut().insert(&peer_a);
    h.executor.native_mut().insert(&peer_b);
    h.executor.native_mut().window_mut(30).cloaked = true;
    h.model.set_minimize_snapped_elsewhere(true);
    h.model.set_auto_restore(true);
    h.model
        .record_mut(&key)
        .unwrap()
        .layouts
        .insert("A".into(), snapped());

    // Switch away to Desktop B: snapped managed window minimizes automatically.
    h.model.snapshot(snapshot("B", 120, vec![managed.clone()]));
    h.executor.native_mut().tick_ms = 120;
    let work = h.issue();
    h.step(work);
    h.advance(20);
    assert_eq!(
        h.executor.native_mut().calls,
        vec![Call::MinimizeArranged(10)]
    );
    assert!(h.model.runtime(&key).unwrap().window().minimized);

    // Switch back to Desktop A: window 30 (from Desktop B) is cloaked, while
    // window 20 (on Desktop A) is uncloaked and remains in front of window 10.
    h.executor.native_mut().calls.clear();
    managed.minimized = true;
    managed.layout.visible = None;
    managed.layout.arranged = Some(false);
    h.model.snapshot(snapshot("A", 200, vec![managed]));
    h.executor.native_mut().tick_ms = 200;
    let work = h.issue();
    h.step(work);
    h.executor.native_mut().window_mut(10).layout = snapped();
    h.advance(20);
    h.advance(20);
    assert!(!h.model.runtime(&key).unwrap().window().minimized);
    assert!(h.model.runtime(&key).unwrap().pending().is_none());
    assert_eq!(
        h.executor.native_mut().calls,
        vec![
            Call::ShowNoActivate(10),
            Call::PlaceBehind {
                hwnd: 10,
                above: 20,
            }
        ]
    );
    assert!(h.executor.native_mut().is_below(10, 20));
}

#[test]
fn delayed_async_snap_reveal_reapplies_background_z_order_once_visible() {
    let mut managed = window("managed", 10);
    let saved = snapped();
    managed.layout = saved.clone();
    managed.layout.visible = None;
    managed.layout.arranged = Some(false);
    managed.minimized = true;
    managed.z_above = vec![20];
    let peer_a = window("peer-a", 20);
    let mut executor = Executor::new(FakeNative::new(100));
    executor.native_mut().insert(&managed);
    executor.native_mut().insert(&peer_a);
    executor.native_mut().window_mut(10).auto_apply = false;

    let mut events = Vec::new();
    executor.step(
        vec![Work::Action(test_job(
            managed,
            Action::RestoreAutomatic(saved.clone()),
        ))],
        &mut events,
    );
    assert_eq!(
        executor.native_mut().calls,
        vec![
            Call::ShowNoActivate(10),
            Call::PlaceBehind {
                hwnd: 10,
                above: 20,
            }
        ]
    );

    // Simulate target thread processing SW_SHOWNOACTIVATE asynchronously,
    // which un-minimizes the window and resets its Win32 Z-order to HWND_TOP.
    {
        let live = executor.native_mut().window_mut(10);
        live.auto_apply = true;
        live.iconic = false;
        live.visible = true;
        live.layout = saved;
        live.z_above.clear();
    }
    executor.step(vec![], &mut events);
    assert!(!events.iter().any(|event| matches!(
        event,
        Event::ActionResult {
            outcome: Outcome::Verified,
            ..
        }
    )));
    assert!(executor.native_mut().is_below(10, 20));

    // Two consecutive verified polls complete the operation in the background.
    executor.step(vec![], &mut events);
    executor.step(vec![], &mut events);
    assert!(events.iter().any(|event| matches!(
        event,
        Event::ActionResult {
            outcome: Outcome::Verified,
            ..
        }
    )));
    assert_eq!(
        executor.native_mut().calls,
        vec![
            Call::ShowNoActivate(10),
            Call::PlaceBehind {
                hwnd: 10,
                above: 20,
            },
            Call::PlaceBehind {
                hwnd: 10,
                above: 20,
            }
        ]
    );
}

#[test]
fn stalled_or_superseded_z_order_correction_respects_verification_and_authority() {
    // 1. If the corrective PlaceBehind remains queued on a slow target thread,
    // verification must not succeed while the window still covers its peer.
    let mut managed = window("managed", 10);
    let saved = snapped();
    managed.layout = saved.clone();
    managed.layout.visible = None;
    managed.layout.arranged = Some(false);
    managed.minimized = true;
    managed.z_above = vec![20];
    let peer_a = window("peer-a", 20);
    let mut executor = Executor::new(FakeNative::new(100));
    executor.native_mut().insert(&managed);
    executor.native_mut().insert(&peer_a);
    executor.native_mut().window_mut(10).auto_apply = false;

    let mut events = Vec::new();
    executor.step(
        vec![Work::Action(test_job(
            managed.clone(),
            Action::RestoreAutomatic(saved.clone()),
        ))],
        &mut events,
    );
    {
        let live = executor.native_mut().window_mut(10);
        live.iconic = false;
        live.visible = true;
        live.layout = saved.clone();
        live.z_above.clear();
    }
    // First poll dispatches the single corrective PlaceBehind, but auto_apply is still false.
    executor.step(vec![], &mut events);
    executor.step(vec![], &mut events);
    executor.step(vec![], &mut events);
    assert!(!events.iter().any(|event| matches!(
        event,
        Event::ActionResult {
            outcome: Outcome::Verified,
            ..
        }
    )));
    assert_eq!(
        executor
            .native_mut()
            .calls
            .iter()
            .filter(|call| matches!(call, Call::PlaceBehind { .. }))
            .count(),
        2
    );

    // If the correction never takes effect before the deadline, it times out.
    executor.native_mut().tick_ms = 2200;
    executor.step(vec![], &mut events);
    assert!(events.iter().any(|event| matches!(
        event,
        Event::ActionResult {
            outcome: Outcome::TimedOut,
            ..
        }
    )));

    // Once the queued Z-order change finally takes effect, two polls verify and release.
    executor.native_mut().window_mut(10).z_above = vec![20];
    executor.step(vec![], &mut events);
    executor.step(vec![], &mut events);
    assert!(events.iter().any(|event| matches!(
        event,
        Event::ActionResult {
            outcome: Outcome::Verified,
            ..
        }
    )));

    // 2. If the job is superseded (e.g. desktop generation changed) before the delayed
    // reveal, late-completion polling stays observational and does not issue PlaceBehind.
    let mut executor = Executor::new(FakeNative::new(100));
    executor.native_mut().insert(&managed);
    executor.native_mut().insert(&peer_a);
    executor.native_mut().window_mut(10).auto_apply = false;
    let job = test_job(managed, Action::RestoreAutomatic(saved.clone()));
    let generation = job.generation.clone();
    let mut events = Vec::new();
    executor.step(vec![Work::Action(job)], &mut events);
    assert_eq!(
        executor
            .native_mut()
            .calls
            .iter()
            .filter(|call| matches!(call, Call::PlaceBehind { .. }))
            .count(),
        1
    );
    generation.store(2, Ordering::Release);
    {
        let live = executor.native_mut().window_mut(10);
        live.iconic = false;
        live.visible = true;
        live.layout = saved;
        live.z_above.clear();
    }
    executor.step(vec![], &mut events);
    executor.step(vec![], &mut events);
    assert!(events.iter().any(|event| matches!(
        event,
        Event::ActionResult {
            outcome: Outcome::Verified,
            ..
        }
    )));
    assert_eq!(
        executor
            .native_mut()
            .calls
            .iter()
            .filter(|call| matches!(call, Call::PlaceBehind { .. }))
            .count(),
        1
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

#[test]
fn multiple_managed_windows_restore_behind_foreground_in_relative_z_order() {
    for snapped_on_top in [true, false] {
        let mut m_snap = window("snap", 10);
        m_snap.layout = snapped();
        let mut m_norm = window("norm", 11);
        m_norm.layout = layout(200);
        let peer_f = window("foreground-peer", 20);
        if snapped_on_top {
            m_snap.z_above = vec![20];
            m_norm.z_above = vec![20, 10];
        } else {
            m_norm.z_above = vec![20];
            m_snap.z_above = vec![20, 11];
        }
        let key_snap = m_snap.identity.key();
        let key_norm = m_norm.identity.key();
        let mut h = Harness::new(&m_snap);
        h.executor.native_mut().insert(&m_norm);
        h.executor.native_mut().insert(&peer_f);
        h.executor.native_mut().foreground = (20, 500);
        h.model.set_minimize_snapped_elsewhere(true);
        h.model.set_auto_restore(true);
        h.model
            .snapshot(snapshot("A", 110, vec![m_snap.clone(), m_norm.clone()]));
        h.model
            .record_mut(&key_snap)
            .unwrap()
            .layouts
            .insert("A".into(), snapped());
        h.model
            .record_mut(&key_norm)
            .unwrap()
            .layouts
            .insert("A".into(), layout(200));

        // Switch to Desktop B: Windows cloaks Desktop A's unpinned foreground window (20)
        // before automatic minimize runs. Neither managed window's Desktop A z_above may be
        // overwritten with the post-switch cloaked Z-order.
        h.executor.native_mut().window_mut(20).cloaked = true;
        m_snap.z_above.clear();
        m_norm.z_above.clear();
        h.model
            .snapshot(snapshot("B", 120, vec![m_snap.clone(), m_norm.clone()]));
        h.executor.native_mut().tick_ms = 120;
        let work = h.issue();
        h.step(work);
        h.advance(20);
        assert!(h.model.runtime(&key_snap).unwrap().window().minimized);
        assert!(h.model.runtime(&key_norm).unwrap().window().minimized);

        // Switch back to Desktop A: window 20 is uncloaked and focused. Simulate delayed
        // async reveal on the top managed window so the lower managed window must wait
        // rather than anchoring to it while it sits at HWND_TOP.
        h.executor.native_mut().window_mut(20).cloaked = false;
        h.executor.native_mut().window_mut(20).z_above.clear();
        let top_hwnd = if snapped_on_top { 10 } else { 11 };
        let bot_hwnd = if snapped_on_top { 11 } else { 10 };
        h.executor.native_mut().window_mut(top_hwnd).auto_apply = false;
        m_snap.minimized = true;
        m_snap.layout.visible = None;
        m_snap.layout.arranged = Some(false);
        m_norm.minimized = true;
        m_norm.layout.visible = None;
        h.model
            .snapshot(snapshot("A", 200, vec![m_snap.clone(), m_norm.clone()]));
        h.executor.native_mut().tick_ms = 200;
        let work = h.issue();
        h.step(work);
        // The fast member must not finish while its peer's reveal is queued.
        for _ in 0..4 {
            h.advance(20);
            assert!(h.model.runtime(&key_snap).unwrap().pending().is_some());
            assert!(h.model.runtime(&key_norm).unwrap().pending().is_some());
        }
        {
            let live = h.executor.native_mut().window_mut(top_hwnd);
            live.auto_apply = true;
            live.iconic = false;
            live.visible = true;
            live.layout = if snapped_on_top {
                snapped()
            } else {
                layout(200)
            };
            live.z_above.clear();
        }
        h.executor.native_mut().window_mut(10).layout = snapped();
        h.advance(20);
        h.advance(20);
        h.advance(20);
        assert!(!h.model.runtime(&key_snap).unwrap().window().minimized);
        assert!(!h.model.runtime(&key_norm).unwrap().window().minimized);
        assert!(h.model.runtime(&key_snap).unwrap().pending().is_none());
        assert!(h.model.runtime(&key_norm).unwrap().pending().is_none());
        assert!(h.executor.native_mut().is_below(top_hwnd, 20));
        assert!(h.executor.native_mut().is_below(bot_hwnd, 20));
        assert!(h.executor.native_mut().is_below(bot_hwnd, top_hwnd));
    }
}

#[test]
fn automatic_restore_with_empty_z_above_falls_back_behind_foreground_window() {
    let mut managed = window("managed", 10);
    managed.layout = snapped();
    managed.z_above.clear();
    let peer_f = window("foreground-peer", 20);
    let key = managed.identity.key();
    let mut h = Harness::new(&managed);
    h.executor.native_mut().insert(&peer_f);
    h.executor.native_mut().foreground = (20, 500);
    h.model.set_minimize_snapped_elsewhere(true);
    h.model.set_auto_restore(true);
    h.model
        .record_mut(&key)
        .unwrap()
        .layouts
        .insert("A".into(), snapped());

    h.model.snapshot(snapshot("B", 120, vec![managed.clone()]));
    h.executor.native_mut().tick_ms = 120;
    let work = h.issue();
    h.step(work);
    h.advance(20);

    managed.minimized = true;
    managed.layout.visible = None;
    managed.layout.arranged = Some(false);
    h.model.snapshot(snapshot("A", 200, vec![managed]));
    h.executor.native_mut().tick_ms = 200;
    let work = h.issue();
    h.step(work);
    h.executor.native_mut().window_mut(10).layout = snapped();
    h.advance(20);
    h.advance(20);
    assert!(h.executor.native_mut().is_below(10, 20));
}
