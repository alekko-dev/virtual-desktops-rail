//! Platform-independent layout memory and action ordering. No shell or window calls.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Identity {
    pub token: String,
    pub process_created: u64,
    pub executable: String,
    pub session_id: u32,
}

impl Identity {
    pub fn key(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.token,
            self.process_created,
            self.session_id,
            self.executable.to_lowercase()
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Layout {
    /// WINDOWPLACEMENT workspace coordinates, never silently treated as screen coordinates.
    pub normal: [i32; 4],
    pub maximized: bool,
    /// Screen bounds, absent for minimized parking rectangles and legacy records.
    #[serde(default)]
    pub visible: Option<[i32; 4]>,
    /// None means arranged-state detection was unavailable or the record predates it.
    #[serde(default)]
    pub arranged: Option<bool>,
    pub dpi: u32,
    pub monitor: String,
    pub monitor_rect: [i32; 4],
    pub work_area: [i32; 4],
}

impl Layout {
    fn reachable(&self) -> bool {
        let x = self.work_area[0] - self.monitor_rect[0];
        let y = self.work_area[1] - self.monitor_rect[1];
        self.normal[2].saturating_add(x) > self.work_area[0]
            && self.normal[0].saturating_add(x) < self.work_area[2]
            && self.normal[3].saturating_add(y) > self.work_area[1]
            && self.normal[1].saturating_add(y) < self.work_area[3]
    }
    /// Convert saved workspace coordinates to a reachable workspace rectangle on a current monitor.
    pub fn adjusted(
        &self,
        monitor: &str,
        monitor_rect: [i32; 4],
        work: [i32; 4],
        dpi: u32,
    ) -> Self {
        let scale = |value: i32| {
            (i64::from(value) * i64::from(dpi.max(1)) / i64::from(self.dpi.max(1))) as i32
        };
        let old_offset = [
            self.work_area[0] - self.monitor_rect[0],
            self.work_area[1] - self.monitor_rect[1],
        ];
        let new_offset = [work[0] - monitor_rect[0], work[1] - monitor_rect[1]];
        let width = scale(self.normal[2].saturating_sub(self.normal[0]))
            .clamp(100, (work[2] - work[0]).max(100));
        let height = scale(self.normal[3].saturating_sub(self.normal[1]))
            .clamp(80, (work[3] - work[1]).max(80));
        let left_screen = work[0] + scale(self.normal[0] + old_offset[0] - self.work_area[0]);
        let top_screen = work[1] + scale(self.normal[1] + old_offset[1] - self.work_area[1]);
        let left = left_screen.clamp(work[0], (work[2] - width).max(work[0])) - new_offset[0];
        let top = top_screen.clamp(work[1], (work[3] - height).max(work[1])) - new_offset[1];
        Self {
            normal: [left, top, left + width, top + height],
            maximized: self.maximized,
            visible: if self.monitor == monitor
                && self.monitor_rect == monitor_rect
                && self.work_area == work
                && self.dpi == dpi
            {
                self.visible
            } else {
                None
            },
            arranged: if self.monitor == monitor
                && self.monitor_rect == monitor_rect
                && self.work_area == work
                && self.dpi == dpi
            {
                self.arranged
            } else {
                None
            },
            dpi,
            monitor: monitor.into(),
            monitor_rect,
            work_area: work,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Desktop {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Window {
    pub identity: Identity,
    pub hwnd: usize,
    pub pid: u32,
    pub minimized: bool,
    pub responsive: bool,
    pub layout: Layout,
    pub popup: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Snapshot {
    pub desktops: Vec<Desktop>,
    pub current: String,
    pub windows: Vec<Window>,
    pub tick: u64,
    pub transition_serial: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Record {
    pub identity: Identity,
    pub last_usable: Layout,
    pub layouts: BTreeMap<String, Layout>,
    /// User intent, independent of Rail's automatic switch minimization.
    #[serde(default)]
    pub minimized_desktops: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Saved {
    pub schema_version: u32,
    pub paused: bool,
    pub startup: bool,
    #[serde(default)]
    pub experimental_integration: bool,
    #[serde(default)]
    pub auto_restore: bool,
    #[serde(default)]
    pub preferences_revision: u64,
    pub windows: BTreeMap<String, Record>,
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            schema_version: 1,
            paused: false,
            startup: false,
            experimental_integration: false,
            auto_restore: false,
            preferences_revision: 0,
            windows: BTreeMap::new(),
        }
    }
}

impl Saved {
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 4 * 1024 * 1024 {
            return Err("Configuration exceeds 4 MiB".into());
        }
        let saved: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if saved.schema_version != 1
            || saved.windows.len() > 512
            || saved.windows.iter().any(|(key, record)| {
                *key != record.identity.key()
                    || record
                        .layouts
                        .keys()
                        .chain(record.minimized_desktops.iter())
                        .collect::<BTreeSet<_>>()
                        .len()
                        > 64
                    || !valid_layout(&record.last_usable)
                    || record.layouts.values().any(|layout| !valid_layout(layout))
            })
        {
            return Err("Unsupported or invalid configuration schema/limits".into());
        }
        Ok(saved)
    }
}

fn valid_layout(layout: &Layout) -> bool {
    [layout.normal, layout.monitor_rect, layout.work_area]
        .into_iter()
        .chain(layout.visible)
        .all(|rect| {
            rect.iter().all(|value| value.unsigned_abs() <= 1_000_000)
                && rect[2] > rect[0]
                && rect[3] > rect[1]
        })
        && (48..=768).contains(&layout.dpi)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Minimize { automatic: bool },
    Restore(Layout),
    RestoreAutomatic(Layout),
    Raise,
}

impl Action {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Minimize { .. } => "minimize",
            Self::Restore(_) | Self::RestoreAutomatic(_) => "restore",
            Self::Raise => "raise",
        }
    }

    pub fn source(&self) -> &'static str {
        if self.automatic() {
            "automatic"
        } else {
            "explicit"
        }
    }

    pub fn automatic(&self) -> bool {
        matches!(
            self,
            Self::Minimize { automatic: true } | Self::RestoreAutomatic(_)
        )
    }
}

#[derive(Clone, Debug)]
pub struct Request {
    pub key: String,
    pub window: Window,
    pub generation: u64,
    pub sequence: u64,
    pub action: Action,
    pub tick: u64,
}

#[derive(Clone, Debug)]
pub struct Pending {
    pub request: Request,
    pub dispatched: bool,
    pub timed_out: bool,
}

#[derive(Clone, Debug)]
pub struct ExternalRestore {
    pub layout: Layout,
    /// None retains restoration evidence while awaiting a post-drag sample.
    pub since: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct Runtime {
    pub window: Window,
    pub sequence: u64,
    pub desired: Option<Action>,
    pub pending: Option<Pending>,
    pub dragging: bool,
    pub blocked: bool,
    pub ignore_geometry_through: u64,
    pub suppress_next_state: bool,
    // Only a current verified minimize can authorize restore evidence in the
    // otherwise-suppressed first post-completion snapshot.
    restore_after_minimize: bool,
    pub present: bool,
    pub external_restore: Option<ExternalRestore>,
    pub state_tick: u64,
}

/// Identity- and desktop-validated native minimize evidence from the shell worker.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MinimizeObservation {
    pub window: Window,
    pub desktop: String,
    pub generation: u64,
    pub sequence: u64,
    pub event_tick: u64,
    pub tick: u64,
    pub transition_serial: u64,
}

pub struct Model {
    pub saved: Saved,
    pub desktops: Vec<Desktop>,
    pub current: Option<String>,
    pub generation: u64,
    pub since: u64,
    pub windows: BTreeMap<String, Runtime>,
    pub dirty: bool,
    pub transition_serial: u64,
    last_snapshot_tick: u64,
}

impl Model {
    pub fn new(saved: Saved) -> Self {
        Self {
            saved,
            desktops: vec![],
            current: None,
            generation: 0,
            since: 0,
            windows: BTreeMap::new(),
            dirty: false,
            transition_serial: 0,
            last_snapshot_tick: 0,
        }
    }

    pub fn unavailable(&mut self) {
        self.current = None;
        self.generation += 1;
        for runtime in self.windows.values_mut() {
            runtime.desired = None;
            runtime.external_restore = None;
            runtime.restore_after_minimize = false;
            runtime.sequence += 1;
        }
    }

    pub fn snapshot(&mut self, snapshot: Snapshot) {
        if snapshot.tick < self.last_snapshot_tick {
            return;
        }
        self.last_snapshot_tick = snapshot.tick;
        let baseline = self.current.is_none();
        let changed = self.current.as_ref().is_some_and(|old| {
            *old != snapshot.current || self.transition_serial != snapshot.transition_serial
        });
        self.transition_serial = snapshot.transition_serial;
        if baseline || changed {
            self.generation += 1;
            self.since = snapshot.tick;
        }
        self.current = Some(snapshot.current.clone());
        self.desktops = snapshot.desktops;
        let known_desktops: BTreeSet<_> = self
            .desktops
            .iter()
            .map(|desktop| desktop.id.as_str())
            .collect();
        let sessions: BTreeSet<_> = snapshot
            .windows
            .iter()
            .map(|window| window.identity.session_id)
            .collect();
        for record in self
            .saved
            .windows
            .values_mut()
            .filter(|record| sessions.contains(&record.identity.session_id))
        {
            let before = record.layouts.len();
            record
                .layouts
                .retain(|id, _| known_desktops.contains(id.as_str()));
            let minimized_before = record.minimized_desktops.len();
            record
                .minimized_desktops
                .retain(|id| known_desktops.contains(id.as_str()));
            if before != record.layouts.len() || minimized_before != record.minimized_desktops.len()
            {
                self.dirty = true;
            }
        }
        let keys: BTreeSet<_> = snapshot
            .windows
            .iter()
            .map(|window| window.identity.key())
            .collect();
        self.windows
            .retain(|key, runtime| keys.contains(key) || runtime.pending.is_some());
        for (key, runtime) in &mut self.windows {
            runtime.present = keys.contains(key);
        }
        for window in snapshot.windows {
            let key = window.identity.key();
            if !self.saved.windows.contains_key(&key) && self.saved.windows.len() >= 512 {
                continue;
            }
            if !self.saved.windows.contains_key(&key) && self.saved.windows.len() < 512 {
                self.saved.windows.insert(
                    key.clone(),
                    Record {
                        identity: window.identity.clone(),
                        last_usable: window.layout.clone(),
                        layouts: BTreeMap::new(),
                        minimized_desktops: BTreeSet::new(),
                    },
                );
                self.dirty = true;
            }
            let runtime = self.windows.entry(key.clone()).or_insert_with(|| Runtime {
                window: window.clone(),
                sequence: 0,
                desired: None,
                pending: None,
                dragging: false,
                blocked: false,
                ignore_geometry_through: snapshot.tick,
                suppress_next_state: false,
                restore_after_minimize: false,
                present: true,
                external_restore: None,
                state_tick: snapshot.tick,
            });
            // Stable external restores and geometry changes are user intent.
            // Startup/recovery and our own pending/late work are not.
            let can_observe_context = !baseline
                && !changed
                && runtime.pending.is_none()
                && !runtime.blocked
                && snapshot.tick > runtime.ignore_geometry_through;
            let can_observe_visibility = can_observe_context
                && (!runtime.suppress_next_state
                    || (runtime.restore_after_minimize
                        && runtime.window.minimized
                        && !window.minimized))
                && snapshot.tick > runtime.state_tick;
            let can_observe_state = can_observe_visibility && !runtime.dragging;
            if can_observe_state && window.minimized && !runtime.window.minimized {
                let record = self.saved.windows.get_mut(&key).unwrap();
                self.dirty |= record.minimized_desktops.insert(snapshot.current.clone());
            }
            let can_observe_edit = can_observe_state && !window.minimized;
            let mut external_restored = false;
            if can_observe_edit {
                if runtime.window.minimized
                    || (runtime.external_restore.is_none()
                        && window.layout != runtime.window.layout)
                {
                    runtime.external_restore = Some(ExternalRestore {
                        layout: window.layout.clone(),
                        since: Some(snapshot.tick),
                    });
                } else if let Some(candidate) = &mut runtime.external_restore {
                    if candidate.layout != window.layout || candidate.since.is_none() {
                        candidate.layout = window.layout.clone();
                        candidate.since = Some(snapshot.tick);
                    } else if candidate
                        .since
                        .is_some_and(|since| snapshot.tick.saturating_sub(since) >= 100)
                        && window.layout.reachable()
                    {
                        external_restored = true;
                        runtime.external_restore = None;
                    }
                }
            } else if can_observe_visibility
                && runtime.dragging
                && !window.minimized
                && (runtime.window.minimized || runtime.external_restore.is_some())
            {
                // A restore can first be seen during a drag. Retain that
                // evidence, but restart geometry stability after dragging ends.
                runtime.external_restore = Some(ExternalRestore {
                    layout: window.layout.clone(),
                    since: None,
                });
            } else if !can_observe_context
                || runtime.suppress_next_state
                || snapshot.tick > runtime.state_tick
            {
                // Equal-tick samples add no new state evidence. Keep the
                // candidate unless its desktop/action context was invalidated.
                runtime.external_restore = None;
            }
            let user_state_change = can_observe_edit
                && window.layout.reachable()
                && (external_restored
                    || (!runtime.window.minimized
                        && window.layout.maximized != runtime.window.layout.maximized));
            if user_state_change && let Some(record) = self.saved.windows.get_mut(&key) {
                if external_restored {
                    record.minimized_desktops.remove(&snapshot.current);
                }
                record
                    .layouts
                    .insert(snapshot.current.clone(), window.layout.clone());
                record.last_usable = window.layout.clone();
                self.dirty = true;
            }
            if snapshot.tick > runtime.state_tick {
                runtime.window = window;
                runtime.state_tick = snapshot.tick;
            }
            runtime.present = true;
            if runtime.pending.is_none() && snapshot.tick > runtime.ignore_geometry_through {
                runtime.suppress_next_state = false;
                runtime.restore_after_minimize = false;
            }
            if changed {
                runtime.sequence += 1;
                runtime.desired = None;
                runtime.ignore_geometry_through = snapshot.tick;
                if !self.saved.paused && !runtime.dragging {
                    if self.saved.auto_restore
                        && !self.saved.windows[&key]
                            .minimized_desktops
                            .contains(&snapshot.current)
                        && let Some(layout) =
                            self.saved.windows[&key].layouts.get(&snapshot.current)
                    {
                        runtime.desired = Some(Action::RestoreAutomatic(layout.clone()));
                    } else if !runtime.window.minimized
                        || runtime.pending.as_ref().is_some_and(|pending| {
                            matches!(
                                pending.request.action,
                                Action::Restore(_) | Action::RestoreAutomatic(_)
                            )
                        })
                    {
                        // Even a currently minimized window may have an older
                        // restore in flight. Reconcile it once that completes.
                        runtime.desired = Some(Action::Minimize { automatic: true });
                    }
                }
            }
        }
    }

    pub fn observe_minimized(&mut self, observation: MinimizeObservation) -> bool {
        let key = observation.window.identity.key();
        let Some(runtime) = self.windows.get_mut(&key) else {
            return false;
        };
        if self.current.as_ref() != Some(&observation.desktop)
            || self.generation != observation.generation
            || self.transition_serial != observation.transition_serial
            || runtime.sequence != observation.sequence
            || runtime.window.hwnd != observation.window.hwnd
            || !runtime.present
            || runtime.pending.is_some()
            || runtime.blocked
            || runtime.dragging
            || runtime.window.minimized
            || !observation.window.minimized
            || observation.event_tick <= runtime.ignore_geometry_through
            || observation.event_tick <= runtime.state_tick
            || observation.tick < observation.event_tick
        {
            return false;
        }
        runtime.window.minimized = true;
        runtime.state_tick = observation.tick;
        self.last_snapshot_tick = self.last_snapshot_tick.max(observation.tick);
        runtime.external_restore = None;
        let record = self.saved.windows.get_mut(&key).unwrap();
        self.dirty |= record.minimized_desktops.insert(observation.desktop);
        true
    }

    pub fn set_auto_restore(&mut self, enabled: bool) {
        if self.saved.auto_restore == enabled {
            return;
        }
        self.saved.auto_restore = enabled;
        self.saved.preferences_revision = self.saved.preferences_revision.saturating_add(1);
        self.dirty = true;
        // A preference change does not move windows. Cancel queued automatic
        // intent; the next confirmed switch applies the new policy.
        for runtime in self.windows.values_mut() {
            if runtime.desired.as_ref().is_some_and(Action::automatic) {
                runtime.desired = None;
            }
            if runtime
                .pending
                .as_ref()
                .is_some_and(|p| p.request.action.automatic())
            {
                runtime.sequence += 1;
            }
        }
    }

    pub fn pause(&mut self, paused: bool) {
        self.saved.paused = paused;
        self.saved.preferences_revision = self.saved.preferences_revision.saturating_add(1);
        self.dirty = true;
        if paused {
            for runtime in self.windows.values_mut() {
                if runtime.desired.as_ref().is_some_and(Action::automatic) {
                    runtime.desired = None;
                }
                if runtime.pending.as_ref().is_some_and(|pending| {
                    pending.request.action.automatic() && !pending.dispatched
                }) {
                    runtime.sequence += 1;
                }
            }
        }
    }

    pub fn click(&mut self, key: &str, was_foreground: bool) {
        let Some(desktop) = &self.current else {
            return;
        };
        let Some(runtime) = self.windows.get_mut(key) else {
            return;
        };
        if !runtime.present || runtime.blocked || runtime.dragging || !runtime.window.responsive {
            return;
        }
        runtime.sequence += 1;
        runtime.desired = Some(if runtime.window.minimized {
            let Some(record) = self.saved.windows.get(key) else {
                return;
            };
            Action::Restore(
                record
                    .layouts
                    .get(desktop)
                    .unwrap_or(&record.last_usable)
                    .clone(),
            )
        } else if was_foreground {
            Action::Minimize { automatic: false }
        } else {
            Action::Raise
        });
    }

    pub fn moving(&mut self, hwnd: usize, started: bool, tick: u64, layout: Option<Layout>) {
        if self.current.is_none() || tick < self.since {
            return;
        }
        let Some((key, runtime)) = self
            .windows
            .iter_mut()
            .find(|(_, runtime)| runtime.window.hwnd == hwnd)
        else {
            return;
        };
        if tick <= runtime.ignore_geometry_through {
            return;
        }
        runtime.sequence += 1;
        runtime.desired = None;
        if runtime.pending.is_some() || runtime.blocked || runtime.suppress_next_state {
            runtime.external_restore = None;
        } else if (started || runtime.dragging)
            && let Some(candidate) = &mut runtime.external_restore
        {
            // The first post-drag visible sample starts the stability interval.
            // A late geometry capture is not another drag boundary.
            candidate.since = None;
        }
        runtime.dragging = started;
        if !started
            && runtime.pending.is_none()
            && tick > runtime.ignore_geometry_through
            && let Some(layout) = layout
            && let Some(record) = self.saved.windows.get_mut(key)
        {
            record
                .layouts
                .insert(self.current.clone().unwrap(), layout.clone());
            record.last_usable = layout;
            runtime.blocked = false;
            self.dirty = true;
        }
    }

    pub fn take_requests(&mut self, tick: u64) -> Vec<Request> {
        if self.current.is_none() {
            return vec![];
        }
        let mut requests = Vec::new();
        for (key, runtime) in &mut self.windows {
            if !runtime.present
                || runtime.pending.is_some()
                || runtime.blocked
                || runtime.dragging
                || !runtime.window.responsive
            {
                continue;
            }
            if let Some(action) = runtime.desired.take() {
                if action.automatic() && self.saved.paused {
                    continue;
                }
                let request = Request {
                    key: key.clone(),
                    window: runtime.window.clone(),
                    generation: self.generation,
                    sequence: runtime.sequence,
                    action,
                    tick,
                };
                runtime.pending = Some(Pending {
                    request: request.clone(),
                    dispatched: false,
                    timed_out: false,
                });
                requests.push(request);
            }
        }
        requests.sort_by_key(|request| request.action.automatic());
        requests
    }

    pub fn dispatched(&mut self, key: &str, sequence: u64) {
        if let Some(runtime) = self.windows.get_mut(key)
            && let Some(pending) = &mut runtime.pending
            && pending.request.sequence == sequence
        {
            pending.dispatched = true;
        }
    }

    pub fn completed(
        &mut self,
        key: &str,
        sequence: u64,
        tick: u64,
        verified: bool,
        cancelled: bool,
    ) {
        let Some(runtime) = self.windows.get_mut(key) else {
            return;
        };
        if !runtime
            .pending
            .as_ref()
            .is_some_and(|pending| pending.request.sequence == sequence)
        {
            return;
        }
        runtime.ignore_geometry_through = runtime.ignore_geometry_through.max(tick);
        let current_verified = verified
            && !cancelled
            && runtime.present
            && runtime.sequence == sequence
            && runtime.pending.as_ref().is_some_and(|pending| {
                !pending.timed_out && pending.request.generation == self.generation
            });
        let current_minimize = current_verified
            && runtime
                .pending
                .as_ref()
                .is_some_and(|pending| matches!(pending.request.action, Action::Minimize { .. }));
        if current_verified
            && let Some(pending) = &runtime.pending
            && let Some(record) = self.saved.windows.get_mut(key)
        {
            let desktop = self.current.as_ref().unwrap();
            match &pending.request.action {
                Action::Restore(layout) => {
                    record.last_usable = layout.clone();
                    record.layouts.insert(desktop.clone(), layout.clone());
                    record.minimized_desktops.remove(desktop);
                    self.dirty = true;
                }
                Action::Minimize { automatic: false } => {
                    self.dirty |= record.minimized_desktops.insert(desktop.clone());
                }
                _ => {}
            }
        }
        if verified || cancelled {
            if verified && let Some(pending) = &runtime.pending {
                match pending.request.action {
                    Action::Minimize { .. } => runtime.window.minimized = true,
                    Action::Restore(_) | Action::RestoreAutomatic(_) => {
                        runtime.window.minimized = false
                    }
                    Action::Raise => {}
                }
            }
            runtime.pending = None;
            runtime.blocked = false;
            runtime.suppress_next_state = verified;
            runtime.restore_after_minimize = current_minimize;
            runtime.state_tick = runtime.state_tick.max(tick);
        } else {
            runtime.blocked = true;
            // Keep the single latest intent while quarantined. A late restore
            // still has to reconcile to a user-minimized destination afterward.
            if let Some(pending) = &mut runtime.pending {
                pending.timed_out = true;
            }
        }
    }
}
