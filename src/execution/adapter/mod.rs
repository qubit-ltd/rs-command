//! Bridge between command scheduling and the operation manager lifecycle.
pub(crate) mod cleanup;
mod control;
mod diagnostics;
mod outcome_slot;
mod state;

pub(crate) use control::CommandControl;
pub(crate) use state::{AdapterState, SessionOwners};
