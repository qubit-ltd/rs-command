//! Process backend contract.
use super::{ActivatedRequest, FactSink};
/// Executes an activated command on a domain worker and publishes lifecycle facts.
pub trait Backend: Send + Sync {
    fn run(&self, request: ActivatedRequest, sink: std::sync::Arc<dyn FactSink>);
}
