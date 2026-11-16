//! Adapter from activated requests to the established synchronous runner.
use crate::execution::contracts::{ActivatedRequest, Backend, FactSink, RequestOptions};
use qubit_operation::OperationOutcome;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ProcessBackend;

impl Backend for ProcessBackend {
    fn run(&self, activated: ActivatedRequest, sink: Arc<dyn FactSink>) {
        let ActivatedRequest { request, .. } = activated;
        let options = match request.options {
            RequestOptions::Standard(options) => options,
            RequestOptions::Resources(resources) => resources.options,
        };
        let tree_policy = options.process_tree_policy();
        let output =
            request
                .runner
                .run_with_tree_policy(request.command, options, Some(tree_policy));
        let outcome = match output {
            Ok(output) => OperationOutcome::Succeeded(output),
            Err(error) => OperationOutcome::Failed(error),
        };
        let _ = sink.publish_outcome(outcome);
        let _ = sink.confirm_input_release();
    }
}
