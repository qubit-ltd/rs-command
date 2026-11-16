//! Contract namespace; behavior is implemented by the assigned module task.
mod internal;
mod process_backend;
mod reaper;
mod start_gate;

pub(crate) use internal::ReaperObserver;
pub(crate) use internal::ReaperOwner;
pub(crate) use internal::StartPermit;
pub(crate) use process_backend::ProcessBackend;
pub(crate) use reaper::Reaper;
pub(crate) use start_gate::StartGate;
