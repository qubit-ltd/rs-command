//! Bridge between command scheduling and the operation manager lifecycle.
pub(crate) mod cleanup;
mod control;
mod diagnostics;
mod state;

pub(crate) use control::CommandControl;
pub(crate) use state::AdapterState;
#[cfg(feature = "async")]
pub(crate) use state::SessionOwners;
