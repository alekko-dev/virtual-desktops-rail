//! Controller policy: desktop transitions, user intent, memory updates and request issue.
use crate::admission::{self, MinimizeObservation, MinimizeRejection, SampleAdmission};
use crate::operation::{Acceptance, Action, OperationState, Outcome, Pending, Request, Settled};
use crate::{Desktop, Layout, Record, Saved, Snapshot, Window};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct ExternalRestore {
    pub layout: Layout,
    /// None retains restoration evidence while awaiting a post-drag sample.
    pub since: Option<u64>,
}

/// Controller state for one live identity. Read-only outside the crate.
#[derive(Clone, Debug)]
pub struct Runtime {
    pub(crate) window: Window,
    pub(crate) sequence: u64,
    pub(crate) desired: Option<Action>,
    pub(crate) pending: Option<Pending>,
    pub(crate) dragging: bool,
    pub(crate) ignore_geometry_through: u64,
    pub(crate) suppress_next_state: bool,
    // Only a current verified minimize can authorize restore evidence in the
    // otherwise-suppressed first post-completion snapshot.
    pub(crate) restore_after_minimize: bool,
    pub(crate) present: bool,
    pub(crate) external_restore: Option<ExternalRestore>,
    pub(crate) state_tick: u64,
    retained_arrangement: Option<Layout>,
    arrangement_tick: u64,
}

impl Runtime {
    fn observe_arrangement(&mut self, layout: &Layout, tick: u64) {
        if tick <= self.arrangement_tick {
            return;
        }
        self.arrangement_tick = tick;
        if self.retained_arrangement.as_ref().is_some_and(|old| {
            old.monitor != layout.monitor
                || old.monitor_rect != layout.monitor_rect
                || old.work_area != layout.work_area
                || old.dpi != layout.dpi
        }) {
            self.retained_arrangement = None;
        }
        if layout.visible.is_some() {
            match layout.arranged {
                Some(true) => self.retained_arrangement = Some(layout.clone()),
                Some(false) => self.retained_arrangement = None,
                None => {}
            }
        }
    }

    /// Last accepted observation of the window.
    pub fn window(&self) -> &Window {
        &self.window
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn desired(&self) -> Option<&Action> {
        self.desired.as_ref()
    }
    pub fn pending(&self) -> Option<&Pending> {
        self.pending.as_ref()
    }
    pub fn present(&self) -> bool {
        self.present
    }
    pub fn dragging(&self) -> bool {
        self.dragging
    }
    /// The outstanding operation is quarantined; rail controls stay disabled.
    pub fn blocked(&self) -> bool {
        self.pending.as_ref().is_some_and(Pending::quarantined)
    }
    pub fn external_restore(&self) -> Option<&ExternalRestore> {
        self.external_restore.as_ref()
    }
    pub fn suppress_next_state(&self) -> bool {
        self.suppress_next_state
    }
}

/// Click-time input. `minimized` is a fresh nonblocking USER-state query taken at the
/// click; it replaces the cached observation because a minimized HWND wins over stale
/// foreground reports.
#[derive(Clone, Copy, Debug)]
pub struct Click {
    pub minimized: bool,
    pub was_foreground: bool,
}

/// Request to capture a window's geometry off the UI thread after a drag ends.
#[derive(Clone, Debug)]
pub struct CaptureRequest {
    pub key: String,
    pub window: Window,
    pub generation: u64,
}

pub struct Model {
    pub(crate) saved: Saved,
    pub(crate) desktops: Vec<Desktop>,
    pub(crate) current: Option<String>,
    pub(crate) generation: u64,
    pub(crate) since: u64,
    pub(crate) windows: BTreeMap<String, Runtime>,
    pub(crate) dirty: bool,
    pub(crate) transition_serial: u64,
    pub(crate) last_snapshot_tick: u64,
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

    // Read-only views for rendering, status and diagnostics.

    pub fn saved(&self) -> &Saved {
        &self.saved
    }
    pub fn record_mut(&mut self, key: &str) -> Option<&mut Record> {
        self.saved.windows.get_mut(key)
    }
    pub fn desktops(&self) -> &[Desktop] {
        &self.desktops
    }
    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn transition_serial(&self) -> u64 {
        self.transition_serial
    }
    pub fn runtime(&self, key: &str) -> Option<&Runtime> {
        self.windows.get(key)
    }
    /// All tracked identities, including absent ones retained by outstanding work.
    pub fn runtimes(&self) -> impl Iterator<Item = (&String, &Runtime)> {
        self.windows.iter()
    }
    /// Identities in the latest inventory.
    pub fn present(&self) -> impl Iterator<Item = (&String, &Runtime)> {
        self.windows.iter().filter(|(_, runtime)| runtime.present)
    }
    /// Current cancellation tickets: the latest sequence per identity.
    pub fn tickets(&self) -> impl Iterator<Item = (&str, u64)> {
        self.windows
            .iter()
            .map(|(key, runtime)| (key.as_str(), runtime.sequence))
    }
    /// Any outstanding operation passed its verification deadline.
    pub fn has_quarantined(&self) -> bool {
        self.windows.values().any(Runtime::blocked)
    }

    // Persistence bookkeeping.

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    /// The current memory was handed to the storage worker.
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }
    /// A write failed; resend the current memory later.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    // Preferences.

    pub fn set_startup(&mut self, enabled: bool) {
        if self.saved.startup != enabled {
            self.saved.startup = enabled;
            self.saved.bump_preferences();
            self.dirty = true;
        }
    }

    /// Legacy preference retained for compatibility. Returns whether it changed.
    pub fn set_experimental_integration(&mut self, enabled: bool) -> bool {
        if self.saved.experimental_integration == enabled {
            return false;
        }
        self.saved.experimental_integration = enabled;
        self.saved.bump_preferences();
        self.dirty = true;
        true
    }

    pub fn set_auto_restore(&mut self, enabled: bool) {
        if self.saved.auto_restore == enabled {
            return;
        }
        self.saved.auto_restore = enabled;
        self.saved.bump_preferences();
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
        self.saved.bump_preferences();
        self.dirty = true;
        if paused {
            for runtime in self.windows.values_mut() {
                if runtime.desired.as_ref().is_some_and(Action::automatic) {
                    runtime.desired = None;
                }
                // Only undispatched work can still be cancelled through the ticket.
                if runtime.pending.as_ref().is_some_and(|pending| {
                    pending.request.action.automatic() && pending.execution.is_none()
                }) {
                    runtime.sequence += 1;
                }
            }
        }
    }

    // Desktop integration and observations.

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
        self.prune_deleted_desktops(&snapshot.windows);
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
            if !self.saved.windows.contains_key(&key) {
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
                ignore_geometry_through: snapshot.tick,
                suppress_next_state: false,
                restore_after_minimize: false,
                present: true,
                external_restore: None,
                state_tick: snapshot.tick,
                retained_arrangement: (window.layout.visible.is_some()
                    && window.layout.arranged == Some(true))
                .then(|| window.layout.clone()),
                arrangement_tick: snapshot.tick,
            });
            if snapshot.tick >= runtime.state_tick {
                if runtime.window.hwnd != window.hwnd {
                    runtime.retained_arrangement = None;
                }
                // Initial observations also establish live evidence. Equal-tick
                // repeats must not replace an already accepted observation.
                if snapshot.tick > runtime.state_tick {
                    runtime.observe_arrangement(&window.layout, snapshot.tick);
                }
            }
            // Stable external restores and geometry changes are user intent.
            // Startup/recovery and our own pending/late work are not.
            let admit =
                SampleAdmission::evaluate(!baseline && !changed, runtime, &window, snapshot.tick);
            if admit.state && window.minimized && !runtime.window.minimized {
                let record = self.saved.windows.get_mut(&key).unwrap();
                self.dirty |= record.minimized_desktops.insert(snapshot.current.clone());
            }
            let mut external_restored = false;
            if admit.edit {
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
            } else if admit.visibility
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
            } else if !admit.context
                || runtime.suppress_next_state
                || snapshot.tick > runtime.state_tick
            {
                // Equal-tick samples add no new state evidence. Keep the
                // candidate unless its desktop/action context was invalidated.
                runtime.external_restore = None;
            }
            let user_state_change = admit.edit
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

    fn prune_deleted_desktops(&mut self, windows: &[Window]) {
        let known_desktops: BTreeSet<_> = self
            .desktops
            .iter()
            .map(|desktop| desktop.id.as_str())
            .collect();
        let sessions: BTreeSet<_> = windows
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
    }

    /// Build a native minimize probe for a minimize-start WinEvent, or None when the
    /// window cannot currently establish external intent.
    pub fn minimize_probe(&self, hwnd: usize, tick: u64) -> Option<MinimizeObservation> {
        let desktop = self.current.as_ref()?;
        let runtime = self
            .windows
            .values()
            .find(|runtime| runtime.present && runtime.window.hwnd == hwnd)?;
        if runtime.pending.is_some() || runtime.dragging {
            return None;
        }
        Some(MinimizeObservation {
            window: runtime.window.clone(),
            desktop: desktop.clone(),
            generation: self.generation,
            sequence: runtime.sequence,
            event_tick: tick,
            tick,
            transition_serial: self.transition_serial,
        })
    }

    /// Admit identity- and desktop-validated native minimize evidence.
    pub fn observe_minimized(
        &mut self,
        observation: MinimizeObservation,
    ) -> Result<(), MinimizeRejection> {
        let key = observation.window.identity.key();
        let Some(runtime) = self.windows.get_mut(&key) else {
            return Err(MinimizeRejection::UnknownWindow);
        };
        admission::admit_minimize(
            &admission::Context {
                current: self.current.as_deref(),
                generation: self.generation,
                transition_serial: self.transition_serial,
            },
            runtime,
            &observation,
        )?;
        runtime.window.minimized = true;
        runtime.state_tick = observation.tick;
        self.last_snapshot_tick = self.last_snapshot_tick.max(observation.tick);
        runtime.external_restore = None;
        let record = self.saved.windows.get_mut(&key).unwrap();
        self.dirty |= record.minimized_desktops.insert(observation.desktop);
        Ok(())
    }

    // User input.

    pub fn click(&mut self, key: &str, click: Click) {
        if let Some(runtime) = self.windows.get_mut(key) {
            runtime.window.minimized = click.minimized;
        }
        let Some(desktop) = &self.current else {
            return;
        };
        let Some(runtime) = self.windows.get_mut(key) else {
            return;
        };
        if !runtime.present || runtime.blocked() || runtime.dragging || !runtime.window.responsive {
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
        } else if click.was_foreground {
            Action::Minimize { automatic: false }
        } else {
            Action::Raise
        });
    }

    /// Move/resize start or end for an HWND reported by a WinEvent.
    pub fn moving(&mut self, hwnd: usize, started: bool, tick: u64, layout: Option<Layout>) {
        if let Some(key) = self
            .windows
            .iter()
            .find(|(_, runtime)| runtime.window.hwnd == hwnd)
            .map(|(key, _)| key.clone())
        {
            self.moving_key(&key, started, tick, layout);
        }
    }

    /// The window to capture after a drag ends, if it is in the current inventory.
    pub fn capture_request(&self, hwnd: usize) -> Option<CaptureRequest> {
        self.windows
            .iter()
            .find(|(_, runtime)| runtime.present && runtime.window.hwnd == hwnd)
            .map(|(key, runtime)| CaptureRequest {
                key: key.clone(),
                window: runtime.window.clone(),
                generation: self.generation,
            })
    }

    /// Identity-validated geometry captured after a drag ended.
    pub fn captured(&mut self, key: &str, generation: u64, tick: u64, layout: Layout) {
        if generation == self.generation && self.windows.contains_key(key) {
            self.moving_key(key, false, tick, Some(layout));
        }
    }

    fn moving_key(&mut self, key: &str, started: bool, tick: u64, layout: Option<Layout>) {
        if self.current.is_none() || tick < self.since {
            return;
        }
        let Some(runtime) = self.windows.get_mut(key) else {
            return;
        };
        if tick <= runtime.ignore_geometry_through {
            return;
        }
        runtime.sequence += 1;
        runtime.desired = None;
        if runtime.pending.is_some() || runtime.suppress_next_state {
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
            && layout.reachable()
            && let Some(record) = self.saved.windows.get_mut(key)
        {
            if tick > runtime.state_tick {
                runtime.observe_arrangement(&layout, tick);
            }
            record
                .layouts
                .insert(self.current.clone().unwrap(), layout.clone());
            record.last_usable = layout;
            self.dirty = true;
        }
    }

    // Operation lifecycle.

    pub fn take_requests(&mut self, tick: u64) -> Vec<Request> {
        if self.current.is_none() {
            return vec![];
        }
        let mut requests = Vec::new();
        for (key, runtime) in &mut self.windows {
            if !runtime.present
                || runtime.pending.is_some()
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
                    retained_arrangement: runtime.retained_arrangement.clone().map(Box::new),
                    generation: self.generation,
                    sequence: runtime.sequence,
                    action,
                    tick,
                };
                runtime.pending = Some(Pending::new(request.clone()));
                requests.push(request);
            }
        }
        requests.sort_by_key(|request| request.action.automatic());
        requests
    }

    /// The executor accepted a native effect for the outstanding request.
    pub fn accepted(&mut self, key: &str, sequence: u64, acceptance: Acceptance) {
        if let Some(runtime) = self.windows.get_mut(key)
            && let Some(pending) = &mut runtime.pending
            && pending.request.sequence == sequence
        {
            if pending.state == OperationState::Queued {
                pending.state = OperationState::Accepted;
            }
            pending.execution = Some(acceptance);
        }
    }

    /// Apply an executor outcome to the outstanding request. Reports for any other
    /// sequence are ignored and return None.
    pub fn settle(
        &mut self,
        key: &str,
        sequence: u64,
        tick: u64,
        outcome: Outcome,
    ) -> Option<Settled> {
        let runtime = self.windows.get_mut(key)?;
        let pending = runtime
            .pending
            .as_ref()
            .filter(|pending| pending.request.sequence == sequence)?;
        let intent = pending.request.action.clone();
        let execution = pending.execution.clone();
        let effect = execution.as_ref().map(|execution| execution.effect);
        let late = pending.quarantined();
        let request_generation = pending.request.generation;
        runtime.ignore_geometry_through = runtime.ignore_geometry_through.max(tick);
        let verified = outcome == Outcome::Verified;
        let current_verified = verified
            && runtime.present
            && runtime.sequence == sequence
            && !late
            && request_generation == self.generation;
        if current_verified
            && let Some(layout) = execution
                .as_ref()
                .and_then(|execution| execution.target.as_ref())
        {
            runtime.observe_arrangement(layout, tick);
        }
        let mut committed = false;
        if current_verified && let Some(record) = self.saved.windows.get_mut(key) {
            let desktop = self.current.as_ref().unwrap();
            match &intent {
                Action::Restore(requested) => {
                    // Intent authorizes the update; the verified execution target
                    // (display-adjusted, or observed for a raise) is what is remembered.
                    let layout = execution
                        .and_then(|execution| execution.target)
                        .unwrap_or_else(|| requested.clone());
                    record.last_usable = layout.clone();
                    record.layouts.insert(desktop.clone(), layout);
                    record.minimized_desktops.remove(desktop);
                    self.dirty = true;
                    committed = true;
                }
                Action::Minimize { automatic: false } => {
                    self.dirty |= record.minimized_desktops.insert(desktop.clone());
                    committed = true;
                }
                _ => {}
            }
        }
        let released = outcome.releases();
        if released {
            if verified {
                match intent {
                    Action::Minimize { .. } => runtime.window.minimized = true,
                    Action::Restore(_) | Action::RestoreAutomatic(_) => {
                        runtime.window.minimized = false
                    }
                    Action::Raise => {}
                }
            }
            runtime.pending = None;
            runtime.suppress_next_state = verified;
            runtime.restore_after_minimize =
                current_verified && matches!(intent, Action::Minimize { .. });
            runtime.state_tick = runtime.state_tick.max(tick);
            if !runtime.present {
                self.windows.remove(key);
            }
        } else if let Some(pending) = &mut runtime.pending {
            // Keep the single latest intent while quarantined. A late restore
            // still has to reconcile to a user-minimized destination afterward.
            pending.state = OperationState::Quarantined;
        }
        Some(Settled {
            intent,
            effect,
            late,
            committed,
            released,
        })
    }
}
