// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Adapter from activated requests to the established synchronous runner.
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use qubit_operation::OperationId;
use qubit_operation::OperationOutcome;
use qubit_operation::SettlementReport;
use qubit_operation::SettlementStatus;
use qubit_operation::StopReason;

use super::internal::OperationStopPort;
use super::internal::PreparationRecoveryOwner;
use crate::CommandError;
use crate::CommandErrorReason;
use crate::OutputStream;
use crate::execution::backend::ReaperObserver;
use crate::execution::backend::StartGate;
use crate::execution::contracts::ActivatedRequest;
use crate::execution::contracts::Backend;
use crate::execution::contracts::FactSink;
use crate::execution::contracts::RecoveryOwner;
use crate::execution::contracts::RequestOptions;
use crate::execution::contracts::ResourceInput;

/// Stores the schedule recovery transfer value used by this execution
/// component.
///
/// # Parameters
/// * `sink`: Supplies the sink value used by this operation.
/// * `owner`: Supplies the owner value used by this operation.
fn schedule_recovery_transfer(sink: Arc<dyn FactSink>, mut owner: Box<dyn RecoveryOwner>) {
    // Keep the rejected owner in this bounded per-operation frame. Its reservation
    // remains held, and the backend worker retries only the nonblocking store
    // transfer; it never runs cleanup. The scheduler's active-operation bound
    // therefore also bounds these retained frames.
    let mut retry = 0_u32;
    loop {
        match sink.retain_recovery_owner(owner) {
            Ok(()) => return,
            Err(failure) => {
                owner = failure.owner;
                let backoff_ms = 25_u64.saturating_mul(1_u64 << retry.min(6));
                std::thread::sleep(Duration::from_millis(backoff_ms));
                retry = retry.saturating_add(1);
            }
        }
    }
}

/// Represents the process backend state or contract used by managed command
/// execution.
#[derive(Clone, Debug, Default)]
pub(crate) struct ProcessBackend {
    /// Stores the gates value used by this execution component.
    gates: Arc<Mutex<HashMap<OperationId, Arc<StartGate>>>>,
}

impl ProcessBackend {
    /// Performs the new step in the managed command execution lifecycle.
    pub(crate) fn new() -> Self {
        Self {
            gates: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Performs the stop port step in the managed command execution lifecycle.
    ///
    /// # Parameters
    /// * `operation_id`: Supplies the operation id value used by this
    ///   operation.
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
    /// Stores the run value used by this execution component.
    ///
    /// # Parameters
    /// * `activated`: Supplies the activated value used by this operation.
    /// * `sink`: Supplies the sink value used by this operation.
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
        let (options, resource_input) = match request.options {
            RequestOptions::Standard(options) => (options, None),
            RequestOptions::Resources(resources) => {
                let options = resources.ordinary_options().clone();
                (options, Some(Box::new(resources) as Box<dyn ResourceInput>))
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
        if request
            .deadline
            .as_ref()
            .is_some_and(qubit_operation::OperationContext::is_expired)
        {
            stop.request_stop(StopReason::Timeout);
        }
        if stop.authorize().is_none() {
            if let Err(error) =
                sink.publish_outcome(OperationOutcome::Stopped(qubit_operation::StopReport {
                    reason: stop.stop_reason().unwrap_or(StopReason::UserCancelled),
                    accepted_at: None,
                }))
            {
                log::error!("failed to publish pre-start stop outcome: {error:?}");
            }
            drop(session_bridge);
            drop(scope);
            if let Err(error) = sink.confirm_input_release() {
                log::error!("failed to publish pre-start input release: {error:?}");
            }
            if let Err(error) = sink.publish_settlement(Arc::new(released_report(operation_id))) {
                log::error!("failed to publish pre-start settlement: {error:?}");
            }
            return;
        }
        if request
            .deadline
            .as_ref()
            .is_some_and(qubit_operation::OperationContext::is_expired)
        {
            stop.request_stop(StopReason::Timeout);
            if let Err(error) =
                sink.publish_outcome(OperationOutcome::Stopped(qubit_operation::StopReport {
                    reason: StopReason::Timeout,
                    accepted_at: None,
                }))
            {
                log::error!("failed to publish expired-deadline outcome: {error:?}");
            }
            drop(session_bridge);
            drop(scope);
            if let Err(error) = sink.confirm_input_release() {
                log::error!("failed to publish expired-deadline input release: {error:?}");
            }
            if let Err(error) = sink.publish_settlement(Arc::new(released_report(operation_id))) {
                log::error!("failed to publish expired-deadline settlement: {error:?}");
            }
            return;
        }
        let prepared_files = match resource_input {
            None => Ok(None),
            Some(resources) => {
                match resources.prepare_files_with_snapshot(&scope, &request.snapshot) {
                    Ok(files) => Ok(Some(files)),
                    Err(failure) => {
                        let error = CommandError::from_reason(
                            "<managed command>",
                            CommandErrorReason::WriteOutputFailed {
                                stream: OutputStream::Stdout,
                                path: std::path::PathBuf::from("<owned resource>"),
                                source: std::io::Error::other(failure.message.clone()),
                            },
                            None,
                        );
                        let capacity_lease = reservation.retain();
                        let owner: Box<dyn RecoveryOwner> = Box::new(PreparationRecoveryOwner {
                            retained: Mutex::new(Some((failure, scope))),
                            _capacity_lease: Mutex::new(Some(capacity_lease)),
                        });
                        if let Err(error) = sink.publish_outcome(OperationOutcome::Failed(error)) {
                            log::error!("failed to publish preparation failure outcome: {error:?}");
                        }
                        if let Err(rejected) = sink.retain_recovery_owner(owner) {
                            log::warn!(
                                "recovery store rejected owner ({}); retaining it in the bounded active frame",
                                rejected.reason as u8
                            );
                            schedule_recovery_transfer(Arc::clone(&sink), rejected.owner);
                        }
                        drop(session_bridge);
                        drop(reservation);
                        return;
                    }
                }
            }
        };
        let options = normalize_tee_paths(options, &request.snapshot.working_directory);
        let capacity_lease = reservation.retain();
        let (reaper_observer, reaper_completion) =
            ReaperObserver::pair_with_recovery_sink(Arc::clone(&sink));
        let output = match prepared_files {
            Ok(files) => request.runner.run_with_tree_policy(
                request.command,
                options,
                crate::command_runner::RunTreePolicy {
                    managed_policy: Some(tree_policy),
                    start_gate: Some(&stop),
                    prepared_files: files,
                    capacity_lease: Some(capacity_lease),
                    reaper_observer: Some(reaper_observer.clone()),
                    session_bridge: session_bridge.clone(),
                    operation_context: request.deadline.clone(),
                },
            ),
            Err(error) => Err(error),
        };
        let deferred_owner = reaper_observer.take_deferred_recovery_owner();
        let mut recovery_transferred = reaper_observer.recovery_was_transferred();
        let outcome = match output {
            Ok(output) => OperationOutcome::Succeeded(output),
            Err(error) => match stop
                .stop_reason()
                .or_else(|| {
                    original_cancellation
                        .as_ref()
                        .filter(|cancellation| cancellation.is_cancelled())
                        .map(|_| StopReason::UserCancelled)
                })
                .or_else(|| {
                    request
                        .deadline
                        .as_ref()
                        .and_then(|context| context.stop_reason())
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
        if let Some(owner) = deferred_owner {
            schedule_recovery_transfer(Arc::clone(&sink), owner);
            recovery_transferred = true;
        }
        if reaper_observer.is_pending()
            && !recovery_transferred
            && reaper_completion.recv().is_err()
        {
            log::error!("process reaper completion channel disconnected");
        }
        drop(session_bridge);
        drop(scope);
        if recovery_transferred {
            // T6 owns release and RecoveryRequired settlement after the retained
            // child/helper recovery owner completes. Publishing either fact
            // here would claim cleanup early.
            drop(reservation);
            return;
        }
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

/// Performs the normalize tee paths step in the managed command execution
/// lifecycle.
///
/// # Parameters
/// * `options`: Supplies the options value used by this operation.
/// * `base`: Supplies the base value used by this operation.
fn normalize_tee_paths(
    mut options: crate::CommandRunOptions,
    base: &std::path::Path,
) -> crate::CommandRunOptions {
    if let Some(path) = options.configured_stdout_file() {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            base.join(path)
        };
        options = options.tee_stdout_to_file(absolute);
    }
    if let Some(path) = options.configured_stderr_file() {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            base.join(path)
        };
        options = options.tee_stderr_to_file(absolute);
    }
    options
}

/// Stores the released report value used by this execution component.
///
/// # Parameters
/// * `operation_id`: Supplies the operation id value used by this operation.
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
