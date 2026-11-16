//! Adapter from activated requests to the established synchronous runner.
use crate::execution::backend::{ReaperObserver, StartGate};
use crate::execution::contracts::{ActivatedRequest, Backend, FactSink, RequestOptions};
use crate::{CommandError, CommandErrorReason, OutputStream};
use qubit_operation::{
    OperationId, OperationOutcome, ResourceRef, SettlementReport, SettlementStatus, StopReason,
};
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;

pub(crate) struct ManagedSink {
    pub(crate) file: ResourceRef<Mutex<File>>,
    pub(crate) policy: crate::FileSinkPolicy,
}

pub(crate) struct ManagedFileSet {
    pub(crate) stdin: Option<ResourceRef<Mutex<File>>>,
    pub(crate) stdout: Option<ManagedSink>,
    pub(crate) stderr: Option<ManagedSink>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ProcessBackend {
    gates: Arc<Mutex<HashMap<OperationId, Arc<StartGate>>>>,
}

struct OperationStopPort {
    operation_id: OperationId,
    gate: Arc<StartGate>,
    gates: std::sync::Weak<Mutex<HashMap<OperationId, Arc<StartGate>>>>,
}

impl crate::execution::contracts::StopPort for OperationStopPort {
    fn request_stop(&self, reason: StopReason) {
        self.gate.request_stop(reason);
    }
}

impl Drop for OperationStopPort {
    fn drop(&mut self) {
        if let Some(gates) = self.gates.upgrade() {
            gates
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&self.operation_id);
        }
    }
}

impl ProcessBackend {
    pub(crate) fn new() -> Self {
        Self {
            gates: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn stop_port(
        &self,
        operation_id: OperationId,
    ) -> Arc<dyn crate::execution::contracts::StopPort> {
        let gate = Arc::new(StartGate::default());
        self.gates
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(operation_id, Arc::clone(&gate));
        Arc::new(OperationStopPort {
            operation_id,
            gate,
            gates: Arc::downgrade(&self.gates),
        })
    }
}

impl Backend for ProcessBackend {
    fn run(&self, activated: ActivatedRequest, sink: Arc<dyn FactSink>) {
        let ActivatedRequest {
            operation_id,
            scope,
            request,
            reservation,
            session_bridge,
            ..
        } = activated;
        let stop = self
            .gates
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&operation_id)
            .unwrap_or_else(|| Arc::new(StartGate::default()));
        let (options, prepared_files) = match request.options {
            RequestOptions::Standard(options) => (options, Ok(None)),
            RequestOptions::Resources(resources) => {
                let crate::resource_run_options::ResourceRunOptionsParts {
                    options,
                    stdin,
                    stdout,
                    stderr,
                } = resources.into_parts();
                (options, register_owned_files(&scope, stdin, stdout, stderr))
            }
        };
        let tree_policy = options.process_tree_policy();
        let original_cancellation = options.clone().into_parts().cancellation;
        stop.attach_external_cancellation(original_cancellation.clone());
        let options = if original_cancellation.is_none() {
            options.cancellation(stop.cancellation())
        } else {
            options
        };
        let capacity_lease = reservation.retain();
        let (reaper_observer, reaper_completion) = ReaperObserver::pair();
        let output = match prepared_files {
            Ok(files) => request.runner.run_with_tree_policy(
                request.command,
                options,
                Some(tree_policy),
                Some(&stop),
                files,
                Some(capacity_lease),
                Some(reaper_observer.clone()),
            ),
            Err(error) => Err(error),
        };
        if reaper_observer.is_pending() && reaper_completion.recv().is_err() {
            log::error!("process reaper completion channel disconnected");
        }
        let outcome = match output {
            Ok(output) => OperationOutcome::Succeeded(output),
            Err(error) => match stop.stop_reason().or_else(|| {
                original_cancellation
                    .as_ref()
                    .filter(|cancellation| cancellation.is_cancelled())
                    .map(|_| StopReason::UserCancelled)
            }) {
                Some(reason) => OperationOutcome::Stopped(qubit_operation::StopReport {
                    reason,
                    accepted_at: None,
                }),
                None => OperationOutcome::Failed(error),
            },
        };
        if let Err(error) = sink.publish_outcome(outcome) {
            log::error!("failed to publish command outcome: {error:?}");
        }
        drop(session_bridge);
        drop(scope);
        if let Err(error) = sink.confirm_input_release() {
            log::error!("failed to publish command input release: {error:?}");
        }
        let report = Arc::new(released_report(operation_id));
        if let Err(error) = sink.publish_settlement(report) {
            log::error!("failed to publish command settlement: {error:?}");
        }
        drop(reservation);
    }
}

fn register_owned_files(
    scope: &qubit_operation::OperationScope,
    stdin: Option<File>,
    stdout: Option<crate::resource_run_options::OwnedSink>,
    stderr: Option<crate::resource_run_options::OwnedSink>,
) -> Result<Option<ManagedFileSet>, CommandError> {
    let stdin = stdin.map(|file| manage_file(scope, file)).transpose()?;
    let stdout = stdout
        .map(|sink| {
            manage_file(scope, sink.file).map(|file| ManagedSink {
                file,
                policy: sink.policy,
            })
        })
        .transpose()?;
    let stderr = stderr
        .map(|sink| {
            manage_file(scope, sink.file).map(|file| ManagedSink {
                file,
                policy: sink.policy,
            })
        })
        .transpose()?;
    if stdin.is_none() && stdout.is_none() && stderr.is_none() {
        Ok(None)
    } else {
        Ok(Some(ManagedFileSet {
            stdin,
            stdout,
            stderr,
        }))
    }
}

fn manage_file(
    scope: &qubit_operation::OperationScope,
    file: File,
) -> Result<ResourceRef<Mutex<File>>, CommandError> {
    scope.manage(Mutex::new(file)).map_err(|error| {
        let (_, _file) = error.into_parts();
        CommandError::from_reason(
            "<managed command>",
            CommandErrorReason::WriteOutputFailed {
                stream: OutputStream::Stdout,
                path: std::path::PathBuf::from("<owned resource>"),
                source: io::Error::other("operation scope refused owned command resource"),
            },
            None,
        )
    })
}

fn released_report(operation_id: OperationId) -> SettlementReport {
    SettlementReport {
        operation_id,
        status: SettlementStatus::Released,
        released_count: 0,
        transferred_count: 0,
        recovery_count: 0,
        children: Arc::from([]),
        children_total: 0,
        children_truncated: false,
        cleanup_failures: Arc::from([]),
        cleanup_failures_total: 0,
        cleanup_failures_truncated: false,
        resource_reports: Arc::from([]),
        resource_reports_total: 0,
        resource_reports_truncated: false,
        transfers: Arc::from([]),
        transfers_total: 0,
        transfers_truncated: false,
        recoveries: Arc::from([]),
        recoveries_total: 0,
        recoveries_truncated: false,
        effects: Arc::from([]),
        effects_total: 0,
        effect_overflow: 0,
        external_settlement: None,
    }
}
