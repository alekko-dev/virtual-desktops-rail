//! Operation lifecycle and the controller/executor protocol.
//!
//! The controller owns policy and the single latest intent per identity. The executor
//! owns native dispatch progress and verification evidence. Each request moves through
//! [`OperationState`] and settles with exactly one releasing [`Outcome`]; acceptance by
//! a Windows API is reported separately and is never completion.
use crate::{Layout, Window};

/// Requested intent. The original request is never rewritten by execution.
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

    /// The native effect that carries out this intent when nothing changed since it was issued.
    pub fn effect(&self) -> Effect {
        match self {
            Self::Minimize { .. } => Effect::Minimize,
            Self::Restore(_) | Self::RestoreAutomatic(_) => Effect::Restore,
            Self::Raise => Effect::Raise,
        }
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

/// The native effect the executor actually requested. An explicit restore becomes a
/// raise when Windows already revealed the window between the click and dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Minimize,
    Restore,
    Raise,
}

impl Effect {
    pub fn kind(self) -> &'static str {
        match self {
            Self::Minimize => "minimize",
            Self::Restore => "restore",
            Self::Raise => "raise",
        }
    }
}

/// Executor report that a native effect was accepted. This is not completion.
///
/// `target` is the layout the executor will verify: the display-adjusted restore
/// target, or the observed layout for a raise. The request's intent decides whether a
/// verified result may change memory; this target decides which layout is committed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Acceptance {
    pub effect: Effect,
    pub target: Option<Layout>,
}

impl Acceptance {
    /// Acceptance of exactly the requested action, without display adjustment.
    pub fn as_requested(action: &Action) -> Self {
        Self {
            effect: action.effect(),
            target: match action {
                Action::Restore(layout) | Action::RestoreAutomatic(layout) => Some(layout.clone()),
                Action::Minimize { .. } | Action::Raise => None,
            },
        }
    }
}

/// Controller-side state of the single outstanding operation for an identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationState {
    /// Sent to the executor; no native effect has been accepted. A ticket change can
    /// still prevent dispatch.
    Queued,
    /// A native effect was accepted and awaits verification within the deadline.
    Accepted,
    /// No verification within the deadline (or the executor still owned an older
    /// operation). The slot remains held until observation or identity death.
    Quarantined,
}

/// Final or intermediate result reported by the executor for one request.
///
/// | Outcome | Slot | Memory |
/// | --- | --- | --- |
/// | `Verified` | released | committed only for current, non-late explicit intent |
/// | `Rejected` | released | none; no native effect was applied |
/// | `IdentityLost` | released | none; the outstanding identity no longer exists |
/// | `TimedOut` | quarantined | none |
/// | `Busy` | quarantined | none; an older operation for the identity is outstanding |
///
/// Desired intent is always retained so it can be reconciled once the slot is free.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Verified,
    Rejected,
    IdentityLost,
    TimedOut,
    Busy,
}

impl Outcome {
    /// Whether this outcome frees the identity's operation slot.
    pub fn releases(self) -> bool {
        matches!(self, Self::Verified | Self::Rejected | Self::IdentityLost)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Rejected => "rejected",
            Self::IdentityLost => "identity_lost",
            Self::TimedOut => "timed_out",
            Self::Busy => "busy",
        }
    }
}

/// The outstanding operation for one identity.
#[derive(Clone, Debug)]
pub struct Pending {
    pub(crate) request: Request,
    pub(crate) state: OperationState,
    pub(crate) execution: Option<Acceptance>,
}

impl Pending {
    pub(crate) fn new(request: Request) -> Self {
        Self {
            request,
            state: OperationState::Queued,
            execution: None,
        }
    }

    pub fn request(&self) -> &Request {
        &self.request
    }

    pub fn state(&self) -> OperationState {
        self.state
    }

    /// What the executor reported accepting, if anything.
    pub fn execution(&self) -> Option<&Acceptance> {
        self.execution.as_ref()
    }

    pub fn quarantined(&self) -> bool {
        self.state == OperationState::Quarantined
    }
}

/// Summary of a settled report, for diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settled {
    pub intent: Action,
    pub effect: Option<Effect>,
    /// The report arrived after the operation was quarantined.
    pub late: bool,
    /// The report changed remembered layout or minimized intent.
    pub committed: bool,
    pub released: bool,
}
