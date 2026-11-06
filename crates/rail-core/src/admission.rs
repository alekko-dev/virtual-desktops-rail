//! Observation admission: which evidence may change controller state or user intent.
//!
//! Every evidence path is validated against the same context before it can update
//! memory. The table records which fields each path uses and where validity is
//! established.
//!
//! | Evidence | Identity | Generation / transition serial | Operation sequence | Event time | Sampling time |
//! | --- | --- | --- | --- | --- | --- |
//! | Snapshot | identity key per window | snapshot desktop and serial change start a new generation | outstanding operation blocks learning | — | `Snapshot::tick`, global watermark plus per-window `state_tick` |
//! | Native minimize | key and HWND; validated live by the shell worker | must equal the controller's | must equal the window's | after own-action suppression and last state | sampled after the event, between matching desktop queries |
//! | Drag start/end | HWND of a known window (WinEvent) | ignored before the generation start (`since`) | each drag event supersedes queued intent | after own-action suppression | — |
//! | Geometry capture | identity key; validated live by the executor | must equal the controller's | outstanding operation blocks learning | capture request time | — |
//! | Action result | identity key | request generation decides whether memory may commit | must match the outstanding request | — | result tick extends own-action suppression |
//!
//! The global snapshot watermark discards whole snapshots older than any accepted
//! snapshot or native minimize evidence. See the refactoring plan for the open
//! freshness-domain decision; per-window state is additionally guarded by `state_tick`.
use crate::Window;
use crate::model::Runtime;
use serde::{Deserialize, Serialize};

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

/// Why native minimize evidence was not admitted. Rejection is conservative: when the
/// source desktop or action context cannot be established, intent is not inferred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MinimizeRejection {
    /// No runtime for the identity key (unknown or replaced identity).
    UnknownWindow,
    /// Desktop, generation or transition serial differs from the controller's.
    StaleContext,
    /// The window's sequence advanced (click, drag or switch) after the probe.
    StaleSequence,
    /// The HWND no longer belongs to this identity.
    IdentityChanged,
    /// The window is absent from the latest inventory.
    Absent,
    /// Our own operation is queued, accepted or quarantined.
    OperationOutstanding,
    Dragging,
    /// The controller already observed the window minimized.
    AlreadyMinimized,
    /// The sampled window is not minimized.
    NotMinimized,
    /// The event precedes the end of our own action's suppression window.
    OwnAction,
    /// Newer state evidence for the window has already been accepted.
    Stale,
    /// The sample precedes the event it claims to describe.
    InvalidTiming,
}

pub(crate) struct Context<'a> {
    pub current: Option<&'a str>,
    pub generation: u64,
    pub transition_serial: u64,
}

pub(crate) fn admit_minimize(
    context: &Context,
    runtime: &Runtime,
    observation: &MinimizeObservation,
) -> Result<(), MinimizeRejection> {
    use MinimizeRejection::*;
    let checks = [
        (
            context.current != Some(observation.desktop.as_str())
                || context.generation != observation.generation
                || context.transition_serial != observation.transition_serial,
            StaleContext,
        ),
        (runtime.sequence != observation.sequence, StaleSequence),
        (
            runtime.window.hwnd != observation.window.hwnd,
            IdentityChanged,
        ),
        (!runtime.present, Absent),
        (runtime.pending.is_some(), OperationOutstanding),
        (runtime.dragging, Dragging),
        (runtime.window.minimized, AlreadyMinimized),
        (!observation.window.minimized, NotMinimized),
        (
            observation.event_tick <= runtime.ignore_geometry_through,
            OwnAction,
        ),
        (observation.event_tick <= runtime.state_tick, Stale),
        (observation.tick < observation.event_tick, InvalidTiming),
    ];
    match checks.into_iter().find(|(failed, _)| *failed) {
        Some((_, rejection)) => Err(rejection),
        None => Ok(()),
    }
}

/// What one snapshot sample may establish about one window. Each level implies the
/// previous one.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SampleAdmission {
    /// Stable desktop (not a baseline or transition), no outstanding operation, and
    /// newer than our own action's suppression window. Without this, a pending
    /// external-restore candidate is invalidated.
    pub context: bool,
    /// Additionally newer than the window's last accepted state evidence, and not the
    /// suppressed first sample after our own verified action. A current verified
    /// minimize followed by a visible sample is the single exception.
    pub visibility: bool,
    /// Additionally outside a drag: minimize and maximize transitions are user intent.
    pub state: bool,
    /// Additionally visible: geometry may start, advance or commit an external restore.
    pub edit: bool,
}

impl SampleAdmission {
    pub(crate) fn evaluate(
        stable_desktop: bool,
        runtime: &Runtime,
        sample: &Window,
        tick: u64,
    ) -> Self {
        let context =
            stable_desktop && runtime.pending.is_none() && tick > runtime.ignore_geometry_through;
        let visibility = context
            && (!runtime.suppress_next_state
                || (runtime.restore_after_minimize
                    && runtime.window.minimized
                    && !sample.minimized))
            && tick > runtime.state_tick;
        let state = visibility && !runtime.dragging;
        let edit = state && !sample.minimized;
        Self {
            context,
            visibility,
            state,
            edit,
        }
    }
}
