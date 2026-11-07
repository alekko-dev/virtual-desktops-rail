//! Lifecycle of the existing single Snap preservation notice. Other notices retain
//! their existing ownership and timeout behavior.
use rail_core::{Action, Effect, Model, Outcome, Request, Settled, Snapshot, Window};

pub const TEXT: &str = "Restoration would replace or lose a native Snap arrangement in the current window/display state. Automatic Snap group reconstruction is unavailable.";

pub struct SnapNotice {
    request: Request,
    tick: u64,
}

impl SnapNotice {
    pub fn new(request: Request, tick: u64) -> Self {
        Self { request, tick }
    }

    fn same_window(&self, window: &Window) -> bool {
        self.request.window.identity == window.identity
            && self.request.window.hwnd == window.hwnd
            && self.request.window.pid == window.pid
    }
}

#[derive(Default)]
pub struct Lifecycle(Option<SnapNotice>);

impl Lifecycle {
    /// A report picker can temporarily cover the notice while recovery arrives.
    pub fn previous(&self, text: String) -> String {
        if text == TEXT && self.0.is_none() {
            String::new()
        } else {
            text
        }
    }

    pub fn rejected(&mut self, notice: SnapNotice, model: &Model, text: &mut String) -> bool {
        let request = &notice.request;
        if request.generation == model.generation()
            && let Some(runtime) = model.runtime(&request.key)
            && runtime.present()
            && runtime.sequence() == request.sequence
            && notice.same_window(runtime.window())
            && runtime.pending().is_some_and(|pending| {
                pending.request().sequence == request.sequence && !pending.quarantined()
            })
        {
            *text = TEXT.into();
            self.0 = Some(notice);
            return true;
        }
        false
    }

    fn clear(&mut self, text: &mut String) {
        self.0 = None;
        if text == TEXT {
            text.clear();
        }
    }

    /// Called before admitting a fresh, identity-validated integration snapshot.
    pub fn observe(&mut self, model: &Model, frame: &Snapshot, text: &mut String) {
        let Some(notice) = &self.0 else { return };
        let Some(runtime) = model.runtime(&notice.request.key) else {
            return;
        };
        if frame.tick <= notice.tick
            || frame.tick <= model.last_snapshot_tick()
            || frame.tick <= runtime.observation_through()
            || model.current() != Some(frame.current.as_str())
            || model.transition_serial() != frame.transition_serial
            || runtime.pending().is_some()
            || runtime.dragging()
        {
            return;
        }
        let Some(window) = frame
            .windows
            .iter()
            .find(|window| notice.same_window(window))
        else {
            return;
        };
        if window.minimized || window.layout.visible.is_none() {
            return;
        }
        let saved = match &notice.request.action {
            Action::Restore(saved)
            | Action::RestoreAutomatic(saved)
            | Action::RevealSnap(saved) => saved,
            _ => return,
        };
        let observed = &window.layout;
        let target = if saved.arranged == Some(true) {
            saved.clone()
        } else {
            saved.adjusted(
                &observed.monitor,
                observed.monitor_rect,
                observed.work_area,
                observed.dpi,
            )
        };
        // A saved Snap target must still match native arranged state, normal and
        // visible bounds, maximized state and the original display context.
        // An ordinary target requires a confirmed unsnap and matching adjusted
        // geometry/state. Visibility alone (including arranged=unknown) is insufficient.
        let resolved = if saved.arranged == Some(true) {
            crate::actions::preserves_arrangement(&target, observed)
                && crate::actions::restored_layout_matches(&target, observed)
        } else {
            observed.arranged == Some(false)
                && crate::actions::restored_layout_matches(&target, observed)
        };
        if resolved {
            self.clear(text);
        }
    }

    /// Inventory removal/replacement is authoritative only after model admission;
    /// pending identities survive temporary omission and must not clear here.
    pub fn reconcile(&mut self, model: &Model, text: &mut String) {
        if self.0.as_ref().is_some_and(|notice| {
            model
                .runtime(&notice.request.key)
                .is_none_or(|runtime| !notice.same_window(runtime.window()))
        }) {
            self.clear(text);
        }
    }

    pub fn settled(
        &mut self,
        model: &Model,
        report: (&str, u64, u64, Outcome),
        settled: Option<&Settled>,
        text: &mut String,
    ) {
        let (key, sequence, tick, outcome) = report;
        let Some(notice) = &self.0 else { return };
        let Some(settled) = settled else { return };
        if key != notice.request.key
            || tick <= notice.tick
            || sequence <= notice.request.sequence
            || model
                .runtime(key)
                .is_some_and(|runtime| !notice.same_window(runtime.window()))
        {
            return;
        }
        if outcome == Outcome::IdentityLost
            || (settled.current_verified
                && matches!(settled.effect, Some(Effect::Restore | Effect::Raise)))
        {
            self.clear(text);
        }
    }
}
