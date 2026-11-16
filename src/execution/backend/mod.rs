//! Contract namespace; behavior is implemented by the assigned module task.
mod process_backend;
mod reaper;
mod start_gate;

pub(crate) use process_backend::ProcessBackend;
pub(crate) use reaper::{Reaper, ReaperObserver, ReaperOwner};
pub(crate) use start_gate::StartGate;
