//! Platform-independent layout memory and action ordering. No shell or window calls.
pub mod admission;
pub mod layout;
pub mod model;
pub mod operation;
pub mod saved;

pub use admission::{MinimizeObservation, MinimizeRejection};
pub use layout::{Desktop, Identity, Layout, Snapshot, Window};
pub use model::{CaptureRequest, Click, ExternalRestore, Model, Runtime};
pub use operation::{
    Acceptance, Action, Effect, OperationState, Outcome, Pending, Request, Settled,
};
pub use saved::{Record, Saved};
