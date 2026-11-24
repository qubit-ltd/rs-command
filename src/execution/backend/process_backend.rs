// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// Adapter from activated requests to the established synchronous runner.
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
use crate::execution::backend::ReaperObserver;
use crate::execution::backend::StartGate;
use crate::execution::backend::retry_cleanup_on_worker;
use crate::execution::contracts::ActivatedRequest;
use crate::execution::contracts::Backend;
use crate::execution::contracts::FactSink;
use crate::execution::contracts::IoPathSnapshot;
use crate::execution::contracts::RecoveryEffectEvidence;
use crate::execution::contracts::RecoveryOwner;
use crate::execution::contracts::RecoveryRetryError;
use crate::execution::contracts::RequestOptions;

/// Stores the schedule recovery transfer value used by this execution
/// component.
///
/// # Parameters
/// * `sink`: Supplies the sink value used by this operation.
/// * `owner`: Supplies the owner value used by this operation.
fn schedule_recovery_transfer(
    sink: Arc<dyn FactSink>,
    mut owner: Box<dyn RecoveryOwner>,
) -> Result<(), Box<dyn RecoveryOwner>> {
    // Keep the rejected owner in this bounded per-operation frame. Its reservation
    // remains held, and the backend worker retries only the nonblocking store
    // transfer; it never runs cleanup. The scheduler's active-operation bound
    // therefore also bounds these retained frames.
    let mut retry = 0_u32;
    loop {
        match sink.retain_recovery_owner(owner) {
            Ok(()) => return Ok(()),
            Err(failure) => {
                owner = failure.owner;
                if failure.reason == crate::execution::contracts::RecoveryPublishError::StoreUnavailable {
                    log::error!("recovery store unavailable; keeping owner in the bounded backend frame");
                    return Err(owner);
                }
                let backoff_ms = 25_u64.saturating_mul(1_u64 << retry.min(6));
                std::thread::sleep(Duration::from_millis(backoff_ms));
                retry = retry.saturating_add(1);
            }
        }
    }
}

fn run_recovery_owner_locally(id: OperationId, owner: &dyn RecoveryOwner) -> RecoveryEffectEvidence {
    let evidence = owner.effect_evidence(id);
    let mut backoff = Duration::from_millis(25);
    loop {
        match retry_cleanup_on_worker(owner) {
            Ok(()) if owner.is_complete() => return evidence,
            Err(RecoveryRetryError::OwnerAlreadyConsumed) if owner.is_complete() => {
                return evidence;
            }
            _ => {
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(2));
            }
        }
    }
}

fn publish_local_recovery(sink: &dyn FactSink, id: OperationId, evidence: RecoveryEffectEvidence) {
    if let Err(error) = sink.confirm_input_release() {
        log::error!("failed to publish command input release after local recovery: {error:?}");
    }
    if let Err(error) = sink.publish_settlement(Arc::new(crate::execution::adapter::cleanup::recovery_settlement(
        id, evidence,
    ))) {
        log::error!("failed to publish local recovery settlement: {error:?}");
    }
}

fn wait_for_upstream_settlement(observer_slot: &crate::execution::adapter::OperationObserverSlot) {
    if let Err(error) = observer_slot.wait().wait_settlement() {
        log::error!("could not observe upstream settlement after local recovery: {error:?}");
    }
}

fn recover_owner_locally(
    sink: &dyn FactSink,
    id: OperationId,
    owner: &dyn RecoveryOwner,
    observer_slot: &crate::execution::adapter::OperationObserverSlot,
) {
    let evidence = run_recovery_owner_locally(id, owner);
    publish_local_recovery(sink, id, evidence);
    wait_for_upstream_settlement(observer_slot);
}

/// Preserves runner-selected errors while reporting runner-observed stops.
///
/// # Type Parameters
/// * `F`: Diagnostic retention callback used before publishing a typed stop.
///
/// # Parameters
/// * `result`: The terminal result chosen by the process runner.
/// * `stop`: A stop accepted by the operation or request cancellation source.
/// * `retain_diagnostic`: Stores a cancellation error alongside a typed stop.
fn runner_result_outcome<F>(
    result: Result<crate::CommandOutput, CommandError>,
    stop: Option<StopReason>,
    retain_diagnostic: F,
) -> OperationOutcome<crate::CommandOutput, CommandError>
where
    F: FnOnce(CommandError) -> Result<(), CommandError>,
{
    match result {
        Ok(output) => OperationOutcome::Succeeded(output),
        Err(error)
            if stop.is_some()
                && matches!(
                    error.kind(),
                    crate::CommandErrorKind::Cancelled | crate::CommandErrorKind::CancelledBeforeStart
                ) =>
        {
            let reason = stop.expect("guard checked that the stop cause is present");
            match retain_diagnostic(error) {
                Ok(()) => OperationOutcome::Stopped(qubit_operation::StopReport {
                    reason,
                    accepted_at: None,
                }),
                Err(error) => OperationOutcome::Failed(error),
            }
        }
        Err(error) => OperationOutcome::Failed(error),
    }
}

/// Represents the process backend state or contract used by managed command
/// execution.
#[derive(Clone, Default)]
pub(crate) struct ProcessBackend {
    /// Stores the gates value used by this execution component.
    gates: Arc<Mutex<HashMap<OperationId, Arc<StartGate>>>>,
    /// Observer slots retained until the scheduled execution captures its upstream handle.
    observers: Arc<Mutex<HashMap<OperationId, Arc<crate::execution::adapter::OperationObserverSlot>>>>,
}

impl ProcessBackend {
    /// Performs the new step in the managed command execution lifecycle.
    pub(crate) fn new() -> Self {
        Self {
            gates: Arc::new(Mutex::new(HashMap::new())),
            observers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn register_observer_slot(
        &self,
        operation_id: OperationId,
        observer: Arc<crate::execution::adapter::OperationObserverSlot>,
    ) {
        self.observers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(operation_id, observer);
    }

    pub(crate) fn remove_observer_slot(&self, operation_id: OperationId) {
        self.observers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&operation_id);
    }

    /// Performs the stop port step in the managed command execution lifecycle.
    ///
    /// # Parameters
    /// * `operation_id`: Supplies the operation id value used by this
    ///   operation.
    pub(crate) fn stop_port(&self, operation_id: OperationId) -> Arc<dyn crate::execution::contracts::StopPort> {
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
        let observer_slot = self
            .observers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&operation_id)
            .unwrap_or_else(|| Arc::new(crate::execution::adapter::OperationObserverSlot::default()));
        let command_stdin = request.command.stdin_mode();
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
                (options, Some(resources))
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
        stop.observe_external_cancellation();
        if request
            .deadline
            .as_ref()
            .is_some_and(qubit_operation::OperationContext::is_expired)
        {
            stop.request_stop(StopReason::Timeout);
        }
        if stop.authorize().is_none() {
            finish_pre_start_stop(
                sink,
                operation_id,
                stop.stop_reason().unwrap_or(StopReason::UserCancelled),
                resource_input,
                session_bridge,
                scope,
                reservation,
            );
            return;
        }
        stop.observe_external_cancellation();
        if request
            .deadline
            .as_ref()
            .is_some_and(qubit_operation::OperationContext::is_expired)
        {
            stop.request_stop(StopReason::Timeout);
            finish_pre_start_stop(
                sink,
                operation_id,
                stop.stop_reason().unwrap_or(StopReason::Timeout),
                resource_input,
                session_bridge,
                scope,
                reservation,
            );
            return;
        }
        let prepared_files = match resource_input {
            None => Ok(None),
            Some(resources) => {
                match resources.prepare_for_managed(&scope, &request.snapshot, command_stdin, session_bridge.is_some())
                {
                    Ok(files) => Ok(Some(files)),
                    Err(managed_failure) => {
                        let error = crate::command_error::managed_file_preparation_error(
                            managed_failure.phase,
                            managed_failure.direction,
                            managed_failure.source,
                        );
                        let capacity_lease = reservation.retain();
                        let owner: Box<dyn RecoveryOwner> = Box::new(PreparationRecoveryOwner {
                            retained: Mutex::new(Some((managed_failure.failure, scope))),
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
                            match schedule_recovery_transfer(Arc::clone(&sink), rejected.owner) {
                                Ok(()) => {}
                                Err(owner) => {
                                    recover_owner_locally(&*sink, operation_id, &*owner, &observer_slot);
                                    drop(owner);
                                }
                            }
                        }
                        drop(session_bridge);
                        drop(reservation);
                        return;
                    }
                }
            }
        };
        let options = normalize_tee_paths(options, &request.io_paths);
        let capacity_lease = reservation.retain();
        let (reaper_observer, reaper_completion) = ReaperObserver::pair_with_recovery_sink(Arc::clone(&sink));
        let output = match prepared_files {
            Ok(files) => request.runner.run_with_tree_policy(
                normalize_stdin_path(request.command, &request.io_paths),
                options,
                crate::command_runner::RunTreePolicy {
                    managed_policy: Some(tree_policy),
                    start_gate: Some(Arc::clone(&stop)),
                    prepared_files: files,
                    file_scope: Some(&scope),
                    capacity_lease: Some(capacity_lease),
                    reaper_observer: Some(reaper_observer.clone()),
                    session_bridge: session_bridge.clone(),
                    operation_context: request.deadline.clone(),
                },
            ),
            Err(error) => Err(error),
        };
        let deferred_owner = reaper_observer.take_deferred_recovery_owner();
        let recovery_transferred = reaper_observer.recovery_was_transferred();
        let late_stop = stop.stop_reason();
        // Keep the runner's selected terminal result. A later stop must not
        // replace its error, partial output, or cleanup diagnostics.
        let outcome = runner_result_outcome(output, late_stop, |error| sink.publish_stop_diagnostic(error));
        if let Err(error) = sink.publish_outcome(outcome) {
            log::error!("failed to publish command outcome: {error:?}");
        }
        finish_backend_tail(
            sink,
            reaper_observer,
            reaper_completion,
            observer_slot,
            (recovery_transferred, deferred_owner),
            session_bridge,
            scope,
            reservation,
        );
    }
}

/// Completes publication and ownership transfer after the runner returns.
/// Reaper recovery can keep backend ownership after the runner has returned.
/// Keeping this tail in one helper makes the transfer-before-release rule
/// explicit for both accepted and deferred reaper owners.
///
/// # Parameters
/// * `sink`: Sink receiving cleanup, settlement, and recovery facts.
/// * `reaper_observer`: Observer tracking child and helper reaper completion.
/// * `reaper_completion`: Channel signaling completion of pending reaper work.
/// * `recovery`: Whether ownership was transferred and any deferred recovery
///   owner.
/// * `session_bridge`: Session endpoints retained until runner/reaper handoff
///   resolves.
/// * `scope`: Operation scope retaining managed resources.
/// * `reservation`: Capacity reservation retained until cleanup ownership
///   resolves.
fn finish_backend_tail(
    sink: Arc<dyn FactSink>,
    reaper_observer: ReaperObserver,
    reaper_completion: std::sync::mpsc::Receiver<()>,
    observer_slot: Arc<crate::execution::adapter::OperationObserverSlot>,
    (mut recovery_transferred, deferred_owner): (bool, Option<Box<dyn RecoveryOwner>>),
    session_bridge: Option<Arc<dyn crate::session::contracts::SessionBridge>>,
    scope: qubit_operation::OperationScope,
    reservation: crate::execution::contracts::Reservation,
) {
    let operation_id = scope.id();
    let mut local_recovery = None;
    if let Some(owner) = deferred_owner {
        match schedule_recovery_transfer(Arc::clone(&sink), owner) {
            Ok(()) => recovery_transferred = true,
            Err(owner) => {
                local_recovery = Some(owner);
                recovery_transferred = true;
            }
        }
    }
    if reaper_observer.is_pending() && !recovery_transferred && reaper_completion.recv().is_err() {
        log::error!("process reaper completion channel disconnected");
    }
    drop(session_bridge);
    drop(scope);
    if let Some(owner) = local_recovery {
        recover_owner_locally(&*sink, operation_id, &*owner, &observer_slot);
        drop(owner);
        drop(reservation);
        return;
    }
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

/// Publishes a pre-start stop after dropping all request-owned inputs.
///
/// # Parameters
/// * `sink`: Receives the terminal outcome, release, and settlement facts.
/// * `operation_id`: Operation whose stop is being published.
/// * `reason`: Stop cause selected before process startup.
/// * `resource_input`: Raw owned options retained by the request.
/// * `session_bridge`: Session endpoints retained by the request.
/// * `scope`: Scope retaining prepared operation resources.
/// * `reservation`: Executor reservation held for the request.
fn finish_pre_start_stop(
    sink: Arc<dyn FactSink>,
    operation_id: OperationId,
    reason: StopReason,
    resource_input: Option<crate::CommandResourceRunOptions>,
    session_bridge: Option<Arc<dyn crate::session::contracts::SessionBridge>>,
    scope: qubit_operation::OperationScope,
    reservation: crate::execution::contracts::Reservation,
) {
    if let Err(error) = sink.publish_outcome(OperationOutcome::Stopped(qubit_operation::StopReport {
        reason,
        accepted_at: None,
    })) {
        log::error!("failed to publish pre-start stop outcome: {error:?}");
    }
    drop(resource_input);
    drop(session_bridge);
    drop(scope);
    if let Err(error) = sink.confirm_input_release() {
        log::error!("failed to publish pre-start input release: {error:?}");
    }
    if let Err(error) = sink.publish_settlement(Arc::new(released_report(operation_id))) {
        log::error!("failed to publish pre-start settlement: {error:?}");
    }
    drop(reservation);
}

/// Performs the normalize tee paths step in the managed command execution
/// lifecycle.
///
/// # Parameters
/// * `options`: Supplies the options value used by this operation.
/// * `base`: Supplies the base value used by this operation.
fn normalize_tee_paths(mut options: crate::CommandRunOptions, paths: &IoPathSnapshot) -> crate::CommandRunOptions {
    if let Some(path) = paths.stdout.as_ref() {
        options = options.tee_stdout_to_file(path.clone());
    }
    if let Some(path) = paths.stderr.as_ref() {
        options = options.tee_stderr_to_file(path.clone());
    }
    options
}

/// Rewrites path-based stdin to the request's captured I/O target.
///
/// # Parameters
/// * `command`: Command whose stdin path may be replaced.
/// * `paths`: Resolved I/O targets captured for this request.
fn normalize_stdin_path(mut command: crate::Command, paths: &IoPathSnapshot) -> crate::Command {
    if let Some(path) = paths.stdin.as_ref() {
        command = command.stdin_file(path.clone());
    }
    command
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

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::path::PathBuf;
    use std::sync::Arc;
    #[cfg(unix)]
    use std::sync::Condvar;
    use std::sync::Mutex;
    #[cfg(unix)]
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    #[cfg(unix)]
    use std::sync::mpsc;
    #[cfg(unix)]
    use std::task::Waker;
    use std::time::Duration;

    #[cfg(unix)]
    use qubit_operation::ClockDomainId;
    #[cfg(unix)]
    use qubit_operation::ClockSource;
    #[cfg(unix)]
    use qubit_operation::ContextOptions;
    #[cfg(unix)]
    use qubit_operation::Deadline;
    #[cfg(unix)]
    use qubit_operation::MonotonicTime;
    #[cfg(unix)]
    use qubit_operation::OperationContext;
    use qubit_operation::OperationId;
    #[cfg(unix)]
    use qubit_operation::OperationManager;
    #[cfg(unix)]
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;
    use qubit_operation::SettlementReport;
    use qubit_operation::StopReason;
    #[cfg(unix)]
    use qubit_operation::TimerRegistration;

    #[cfg(unix)]
    use super::ProcessBackend;
    #[cfg(unix)]
    use super::finish_backend_tail;
    use super::normalize_tee_paths;
    #[cfg(unix)]
    use super::recover_owner_locally;
    #[cfg(unix)]
    use super::run_recovery_owner_locally;
    use super::runner_result_outcome;
    use super::schedule_recovery_transfer;
    #[cfg(unix)]
    use crate::Command;
    use crate::CommandError;
    use crate::CommandErrorReason;
    use crate::CommandOutput;
    use crate::CommandRunOptions;
    #[cfg(unix)]
    use crate::CommandRunner;
    #[cfg(unix)]
    use crate::execution::backend::ReaperObserver;
    #[cfg(unix)]
    use crate::execution::backend::ReaperOwner;
    #[cfg(unix)]
    use crate::execution::contracts::ActivatedRequest;
    #[cfg(unix)]
    use crate::execution::contracts::Backend;
    #[cfg(unix)]
    use crate::execution::contracts::ExecutionSnapshot;
    use crate::execution::contracts::FactPublishError;
    use crate::execution::contracts::FactSink;
    use crate::execution::contracts::IoPathSnapshot;
    use crate::execution::contracts::RecoveryEffectEvidence;
    use crate::execution::contracts::RecoveryOwner;
    use crate::execution::contracts::RecoveryPublishError;
    use crate::execution::contracts::RecoveryPublishFailure;
    #[cfg(unix)]
    use crate::execution::contracts::RecoveryRetryError;
    #[cfg(unix)]
    use crate::execution::contracts::Request;
    #[cfg(unix)]
    use crate::execution::contracts::RequestOptions;
    #[cfg(unix)]
    use crate::execution::contracts::Reservation;
    #[cfg(unix)]
    use crate::execution::contracts::ReservationLease;
    #[cfg(unix)]
    use crate::execution::contracts::StopPort;
    #[cfg(unix)]
    use crate::resource_run_options::CommandResourceRunOptions;
    #[cfg(unix)]
    use crate::resource_run_options::FileSinkPolicy;

    #[cfg(unix)]
    struct FallbackGateTeardown(Arc<(Mutex<bool>, Condvar)>);

    #[cfg(unix)]
    impl Drop for FallbackGateTeardown {
        fn drop(&mut self) {
            let (released, changed) = &*self.0;
            *released.lock().unwrap_or_else(|error| error.into_inner()) = true;
            changed.notify_all();
        }
    }

    #[cfg(unix)]
    struct FallbackBlockingDrop {
        started: mpsc::SyncSender<()>,
        gate: Arc<(Mutex<bool>, Condvar)>,
    }

    #[cfg(unix)]
    impl Drop for FallbackBlockingDrop {
        fn drop(&mut self) {
            let _ = self.started.send(());
            let (released, changed) = &*self.gate;
            let mut released = released.lock().unwrap_or_else(|error| error.into_inner());
            while !*released {
                released = changed.wait(released).unwrap_or_else(|error| error.into_inner());
            }
        }
    }

    #[cfg(unix)]
    struct LocalFallbackOwner {
        completed: AtomicBool,
        _lease: Mutex<Option<ReservationLease>>,
    }

    #[cfg(unix)]
    impl RecoveryOwner for LocalFallbackOwner {
        fn summary(&self) -> String {
            "local fallback test owner".to_owned()
        }

        fn retry_cleanup(&self) -> Result<(), RecoveryRetryError> {
            self.completed.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn is_complete(&self) -> bool {
            self.completed.load(Ordering::SeqCst)
        }
    }

    #[cfg(unix)]
    struct FallbackSettlementSink(mpsc::SyncSender<()>);

    #[cfg(unix)]
    impl FactSink for FallbackSettlementSink {
        fn publish_outcome(&self, _: OperationOutcome<CommandOutput, CommandError>) -> Result<(), FactPublishError> {
            Ok(())
        }

        fn confirm_input_release(&self) -> Result<(), FactPublishError> {
            Ok(())
        }

        fn publish_settlement(&self, _: Arc<SettlementReport>) -> Result<(), FactPublishError> {
            self.0.send(()).unwrap();
            Ok(())
        }

        fn retain_recovery_owner(&self, owner: Box<dyn RecoveryOwner>) -> Result<(), RecoveryPublishFailure> {
            Err(RecoveryPublishFailure {
                reason: RecoveryPublishError::StoreUnavailable,
                owner,
            })
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_local_recovery_keeps_preparation_lease_until_upstream_settlement() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let (drop_started_tx, drop_started_rx) = mpsc::sync_channel(1);
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let _teardown = FallbackGateTeardown(Arc::clone(&gate));
        let cleanup_gate = Arc::clone(&gate);
        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let resource = scope
                    .manage(FallbackBlockingDrop {
                        started: drop_started_tx,
                        gate: cleanup_gate,
                    })
                    .unwrap();
                drop(resource);
                Ok::<_, ()>(())
            })
            .unwrap();
        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        drop_started_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("upstream settlement must be blocked in resource cleanup");

        let observer_slot = Arc::new(crate::execution::adapter::OperationObserverSlot::default());
        observer_slot.set(operation.observer());
        let (settlement_tx, settlement_rx) = mpsc::sync_channel(1);
        let sink = Arc::new(FallbackSettlementSink(settlement_tx));
        let (lease_dropped_tx, lease_dropped_rx) = mpsc::sync_channel(1);
        let owner: Box<dyn RecoveryOwner> = Box::new(LocalFallbackOwner {
            completed: AtomicBool::new(false),
            _lease: Mutex::new(Some(ReservationLease::new(move || {
                let _ = lease_dropped_tx.send(());
            }))),
        });
        let operation_id = operation.id();
        let worker_observer = Arc::clone(&observer_slot);
        let worker_sink = Arc::clone(&sink);
        let worker = std::thread::spawn(move || {
            recover_owner_locally(&*worker_sink, operation_id, &*owner, &worker_observer);
            drop(owner);
        });

        settlement_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("local recovery should publish its release and settlement");
        assert!(
            lease_dropped_rx.try_recv().is_err(),
            "lease stays held until upstream settles"
        );
        assert!(matches!(
            operation.wait_settlement_for(Duration::ZERO).unwrap(),
            qubit_operation::SettlementWait::Pending(_)
        ));

        let (released, changed) = &*gate;
        *released.lock().unwrap() = true;
        changed.notify_all();
        worker.join().unwrap();
        operation.wait_settlement().unwrap();
        lease_dropped_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("the owner lease drops after upstream settlement");
    }

    #[cfg(unix)]
    #[test]
    fn test_disconnected_store_falls_back_for_preparation_owner() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let observer_slot = Arc::new(crate::execution::adapter::OperationObserverSlot::default());
        let worker_observer = Arc::clone(&observer_slot);
        let (owner_tx, owner_rx) = mpsc::sync_channel(1);
        let (settlement_tx, settlement_rx) = mpsc::sync_channel(1);
        let sink = Arc::new(FallbackSettlementSink(settlement_tx));
        let worker_sink = Arc::clone(&sink);
        let (lease_dropped_tx, lease_dropped_rx) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let (id, owner) = owner_rx.recv().unwrap();
            let sink_port: Arc<dyn FactSink> = worker_sink.clone();
            let owner = schedule_recovery_transfer(sink_port, owner)
                .err()
                .expect("disconnected recovery store should return the preparation owner");
            recover_owner_locally(&*worker_sink, id, &*owner, &worker_observer);
            drop(owner);
        });

        let worker_owner_tx = owner_tx.clone();
        let worker_lease_dropped = lease_dropped_tx.clone();
        let operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let id = scope.id();
                let owner: Box<dyn RecoveryOwner> = Box::new(super::PreparationRecoveryOwner {
                    retained: Mutex::new(Some((
                        crate::execution::contracts::FilePreparationFailure {
                            message: "injected partial resource preparation".to_owned(),
                            input: None,
                            prepared: None,
                        },
                        scope,
                    ))),
                    _capacity_lease: Mutex::new(Some(ReservationLease::new(move || {
                        let _ = worker_lease_dropped.send(());
                    }))),
                });
                worker_owner_tx.send((id, owner)).unwrap();
                Ok::<_, ()>(())
            })
            .unwrap();
        observer_slot.set(operation.observer());

        settlement_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("local preparation recovery should publish settlement");
        operation.wait_settlement().unwrap();
        worker.join().unwrap();
        lease_dropped_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("preparation owner lease drops after upstream settlement");
    }

    #[cfg(unix)]
    fn run_release_observation_test(test: impl FnOnce()) {
        const CHILD_MARKER: &str = "QUBIT_COMMAND_RELEASE_OBSERVATION_CHILD";
        if std::env::var_os(CHILD_MARKER).is_some() {
            test();
            return;
        }

        let test_name = std::thread::current()
            .name()
            .expect("release observation test should have a name")
            .to_owned();
        let output = std::process::Command::new(std::env::current_exe().expect("test binary should be available"))
            .arg("--exact")
            .arg(test_name)
            .arg("--nocapture")
            .env(CHILD_MARKER, "1")
            .output()
            .expect("isolated release observation test should start");
        assert!(
            output.status.success(),
            "isolated release observation test failed\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn test_late_stop_does_not_replace_runner_error_or_its_diagnostics() {
        let error = CommandError::from_reason(
            "command",
            CommandErrorReason::UnexpectedExit {
                exit_code: Some(23),
                expected: vec![0],
            },
            None,
        )
        .with_captured_output(crate::CommandCapturedOutput::new(
            (b"partial stdout".to_vec(), false, true),
            (b"partial stderr".to_vec(), false, true),
        ));

        let outcome = runner_result_outcome(Err(error), Some(StopReason::Timeout), |_| Ok(()));
        let OperationOutcome::Failed(error) = outcome else {
            panic!("a later stop must not replace the runner's terminal failure");
        };
        assert_eq!(error.kind(), crate::CommandErrorKind::UnexpectedExit);
        let captured = error.captured_output().expect("partial capture should survive");
        assert_eq!(captured.stdout(), b"partial stdout");
        assert_eq!(captured.stderr(), b"partial stderr");
    }

    #[test]
    fn test_runner_observed_stop_keeps_typed_stop_and_diagnostic() {
        let error = CommandError::from_reason("command", CommandErrorReason::Cancelled, None).with_captured_output(
            crate::CommandCapturedOutput::new(
                (b"partial stdout".to_vec(), false, true),
                (b"partial stderr".to_vec(), false, true),
            ),
        );
        let retained = Mutex::new(None);

        let outcome = runner_result_outcome(Err(error), Some(StopReason::UserCancelled), |error| {
            *retained.lock().unwrap() = Some(error);
            Ok(())
        });

        assert!(matches!(outcome, OperationOutcome::Stopped(report) if report.reason == StopReason::UserCancelled));
        let retained = retained.into_inner().unwrap().expect("stop error should be retained");
        let captured = retained.captured_output().expect("partial capture should survive");
        assert_eq!(captured.stdout(), b"partial stdout");
        assert_eq!(captured.stderr(), b"partial stderr");
    }

    #[test]
    fn test_runner_stop_becomes_failure_when_diagnostic_cannot_be_retained() {
        let cancellation = CommandError::from_reason("command", CommandErrorReason::Cancelled, None);
        let outcome = runner_result_outcome(Err(cancellation), Some(StopReason::Timeout), |_| {
            Err(CommandError::from_reason(
                "operation",
                CommandErrorReason::ManagedOperationFailed {
                    diagnostic: "injected diagnostic storage failure".into(),
                },
                None,
            ))
        });

        assert!(matches!(
            outcome,
            OperationOutcome::Failed(error)
                if error.kind() == crate::CommandErrorKind::ManagedOperationFailed
        ));
    }

    #[test]
    fn test_late_stop_does_not_replace_runner_timeout() {
        let error = CommandError::from_reason(
            "command",
            CommandErrorReason::TimedOut {
                timeout: Duration::from_millis(10),
            },
            None,
        );

        let outcome = runner_result_outcome(Err(error), Some(StopReason::ManagerShutdown), |_| {
            panic!("a late stop must not claim a runner-selected timeout")
        });

        let OperationOutcome::Failed(error) = outcome else {
            panic!("a natural runner timeout must remain a failure");
        };
        assert_eq!(error.kind(), crate::CommandErrorKind::TimedOut);
    }

    #[cfg(unix)]
    struct ExpiredClock(ClockDomainId);

    #[cfg(unix)]
    impl ClockSource for ExpiredClock {
        fn now(&self) -> MonotonicTime {
            MonotonicTime::new(self.0, 1)
        }

        fn register_deadline(&self, _deadline: MonotonicTime, _waker: Waker) -> Box<dyn TimerRegistration> {
            Box::new(NoopRegistration)
        }
    }

    #[cfg(unix)]
    struct DeadlineExpiresBeforeAuthorization(
        ClockDomainId,
        Arc<AtomicUsize>,
        Option<(usize, crate::CommandCancellation)>,
    );

    #[cfg(unix)]
    impl ClockSource for DeadlineExpiresBeforeAuthorization {
        fn now(&self) -> MonotonicTime {
            let call = self.1.fetch_add(1, Ordering::SeqCst);
            if let Some((cancel_at, cancellation)) = &self.2
                && call == *cancel_at
            {
                cancellation.cancel();
            }
            let ticks = if call < 2 { 0 } else { 1 };
            MonotonicTime::new(self.0, ticks)
        }

        fn register_deadline(&self, _deadline: MonotonicTime, _waker: Waker) -> Box<dyn TimerRegistration> {
            Box::new(NoopRegistration)
        }
    }

    #[cfg(unix)]
    struct DeadlineExpiresAtRunnerCheck(
        ClockDomainId,
        Arc<AtomicUsize>,
        Option<(usize, crate::CommandCancellation)>,
    );

    #[cfg(unix)]
    impl ClockSource for DeadlineExpiresAtRunnerCheck {
        fn now(&self) -> MonotonicTime {
            let call = self.1.fetch_add(1, Ordering::SeqCst);
            if let Some((cancel_at, cancellation)) = &self.2
                && call == *cancel_at
            {
                cancellation.cancel();
            }
            let ticks = if call < 3 { 0 } else { 1 };
            MonotonicTime::new(self.0, ticks)
        }

        fn register_deadline(&self, _deadline: MonotonicTime, _waker: Waker) -> Box<dyn TimerRegistration> {
            Box::new(NoopRegistration)
        }
    }

    #[cfg(unix)]
    struct NoopRegistration;

    #[cfg(unix)]
    impl TimerRegistration for NoopRegistration {
        fn cancel(self: Box<Self>) {}
    }

    #[cfg(unix)]
    #[derive(Default)]
    struct RecordingSink {
        outcome: Mutex<Option<OperationOutcome<CommandOutput, CommandError>>>,
        stop_diagnostic: Mutex<Option<CommandError>>,
        released: AtomicBool,
        release_probe_path: Mutex<Option<PathBuf>>,
        release_observed_unlocked: AtomicBool,
        settlement: Mutex<Option<Arc<SettlementReport>>>,
        recovery_owner: Mutex<Option<Box<dyn RecoveryOwner>>>,
        reject_recovery_once: AtomicBool,
        recovery_attempts: AtomicUsize,
        reject_outcome_once: AtomicBool,
        reject_release_once: AtomicBool,
        reject_settlement_once: AtomicBool,
    }

    #[cfg(unix)]
    impl FactSink for RecordingSink {
        fn publish_stop_diagnostic(&self, error: CommandError) -> Result<(), CommandError> {
            *self.stop_diagnostic.lock().unwrap() = Some(error);
            Ok(())
        }

        fn publish_outcome(
            &self,
            outcome: OperationOutcome<CommandOutput, CommandError>,
        ) -> Result<(), FactPublishError> {
            if self.reject_outcome_once.swap(false, Ordering::SeqCst) {
                return Err(FactPublishError::DuplicateOutcome);
            }
            *self.outcome.lock().unwrap() = Some(outcome);
            Ok(())
        }

        fn confirm_input_release(&self) -> Result<(), FactPublishError> {
            if self.reject_release_once.swap(false, Ordering::SeqCst) {
                return Err(FactPublishError::DuplicateRelease);
            }
            if let Some(path) = self.release_probe_path.lock().unwrap().as_ref() {
                let file = std::fs::File::open(path).expect("release probe file should open");
                let unlocked =
                    unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&file), libc::LOCK_EX | libc::LOCK_NB) == 0 };
                self.release_observed_unlocked.store(unlocked, Ordering::SeqCst);
            }
            self.released.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn publish_settlement(&self, report: Arc<SettlementReport>) -> Result<(), FactPublishError> {
            if self.reject_settlement_once.swap(false, Ordering::SeqCst) {
                return Err(FactPublishError::DuplicateSettlement);
            }
            *self.settlement.lock().unwrap() = Some(report);
            Ok(())
        }

        fn retain_recovery_owner(&self, owner: Box<dyn RecoveryOwner>) -> Result<(), RecoveryPublishFailure> {
            self.recovery_attempts.fetch_add(1, Ordering::SeqCst);
            if self.reject_recovery_once.swap(false, Ordering::SeqCst) {
                return Err(RecoveryPublishFailure {
                    reason: RecoveryPublishError::CapacityExceeded,
                    owner,
                });
            }
            *self.recovery_owner.lock().unwrap() = Some(owner);
            Ok(())
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_stop_before_backend_start_publishes_stopped_release_and_settlement() {
        run_release_observation_test(|| {
            let manager = OperationManager::new(Default::default()).unwrap();
            let backend = Arc::new(ProcessBackend::new());
            let sink = Arc::new(RecordingSink::default());
            let reservation_released = Arc::new(AtomicBool::new(false));
            let operation_backend = Arc::clone(&backend);
            let operation_sink = Arc::clone(&sink);
            let operation_reservation_released = Arc::clone(&reservation_released);
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("must-not-start");
            let operation_path = path.clone();
            let input = tempfile::NamedTempFile::new().unwrap();
            let input_path = input.path().to_path_buf();
            let input_file = input.reopen().unwrap();
            assert_eq!(
                unsafe {
                    libc::flock(
                        std::os::fd::AsRawFd::as_raw_fd(&input_file),
                        libc::LOCK_EX | libc::LOCK_NB,
                    )
                },
                0
            );
            *sink.release_probe_path.lock().unwrap() = Some(input_path);

            let mut operation = manager
                .try_submit(OperationOptions::default(), move |scope| {
                    let operation_id = scope.id();
                    let stop = operation_backend.stop_port(operation_id);
                    StopPort::request_stop(&*stop, StopReason::Timeout);
                    let reservation = Reservation::new(
                        1,
                        1,
                        move |_| operation_reservation_released.store(true, Ordering::SeqCst),
                        |_| crate::execution::contracts::ReservationLease::new(|| {}),
                    );
                    let request = Request {
                        operation_id: None,
                        runner: CommandRunner::without_timeout(),
                        command: Command::new("sh")
                            .arg("-c")
                            .arg(&format!("touch {}", operation_path.display())),
                        options: RequestOptions::Resources(CommandResourceRunOptions::new().stdin_file(input_file)),
                        snapshot: ExecutionSnapshot {
                            working_directory: std::env::current_dir().unwrap(),
                            environment: Vec::new(),
                        },
                        io_paths: IoPathSnapshot::default(),
                        deadline: None,
                    };
                    operation_backend.run(
                        ActivatedRequest {
                            operation_id,
                            scope,
                            request,
                            reservation,
                            facts: operation_sink.clone(),
                            session_bridge: None,
                        },
                        operation_sink,
                    );
                    Ok::<_, ()>(())
                })
                .unwrap();

            assert!(matches!(
                operation.wait_outcome().unwrap(),
                OperationOutcome::Succeeded(())
            ));
            operation.wait_settlement().unwrap();
            assert!(
                !path.exists(),
                "a stop that wins before authorization must prevent spawn"
            );
            assert!(reservation_released.load(Ordering::SeqCst));
            assert!(sink.released.load(Ordering::SeqCst));
            assert!(sink.release_observed_unlocked.load(Ordering::SeqCst));
            assert!(matches!(
                sink.outcome.lock().unwrap().take(),
                Some(OperationOutcome::Stopped(report)) if report.reason == StopReason::Timeout
            ));
            let report = sink.settlement.lock().unwrap().take().unwrap();
            assert!(report.all_released());
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_pre_start_stop_publication_failures_still_release_reservation() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let backend = Arc::new(ProcessBackend::new());
        let sink = Arc::new(RecordingSink::default());
        sink.reject_outcome_once.store(true, Ordering::SeqCst);
        sink.reject_release_once.store(true, Ordering::SeqCst);
        sink.reject_settlement_once.store(true, Ordering::SeqCst);
        let reservation_released = Arc::new(AtomicBool::new(false));
        let operation_backend = Arc::clone(&backend);
        let operation_sink = Arc::clone(&sink);
        let operation_reservation_released = Arc::clone(&reservation_released);
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("must-not-start-after-publication-errors");
        let operation_marker = marker.clone();

        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let operation_id = scope.id();
                let stop = operation_backend.stop_port(operation_id);
                StopPort::request_stop(&*stop, StopReason::UserCancelled);
                let reservation = Reservation::new(
                    1,
                    1,
                    move |_| operation_reservation_released.store(true, Ordering::SeqCst),
                    |_| crate::execution::contracts::ReservationLease::new(|| {}),
                );
                let request = Request {
                    operation_id: None,
                    runner: CommandRunner::without_timeout(),
                    command: Command::new("sh")
                        .arg("-c")
                        .arg(&format!("touch {}", operation_marker.display())),
                    options: RequestOptions::Standard(CommandRunOptions::default()),
                    snapshot: ExecutionSnapshot {
                        working_directory: std::env::current_dir().unwrap(),
                        environment: Vec::new(),
                    },
                    io_paths: IoPathSnapshot::default(),
                    deadline: None,
                };
                operation_backend.run(
                    ActivatedRequest {
                        operation_id,
                        scope,
                        request,
                        reservation,
                        facts: operation_sink.clone(),
                        session_bridge: None,
                    },
                    operation_sink,
                );
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        operation.wait_settlement().unwrap();
        assert!(!marker.exists(), "a pre-start stop must still prevent process spawn");
        assert!(reservation_released.load(Ordering::SeqCst));
        assert!(!sink.reject_outcome_once.load(Ordering::SeqCst));
        assert!(!sink.reject_release_once.load(Ordering::SeqCst));
        assert!(!sink.reject_settlement_once.load(Ordering::SeqCst));
        assert!(sink.outcome.lock().unwrap().is_none());
        assert!(!sink.released.load(Ordering::SeqCst));
        assert!(sink.settlement.lock().unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_terminal_publication_failures_do_not_skip_process_or_reservation_cleanup() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let backend = Arc::new(ProcessBackend::new());
        let sink = Arc::new(RecordingSink::default());
        sink.reject_outcome_once.store(true, Ordering::SeqCst);
        sink.reject_release_once.store(true, Ordering::SeqCst);
        sink.reject_settlement_once.store(true, Ordering::SeqCst);
        let reservation_released = Arc::new(AtomicBool::new(false));
        let lease_released = Arc::new(AtomicBool::new(false));
        let operation_backend = Arc::clone(&backend);
        let operation_sink = Arc::clone(&sink);
        let operation_reservation_released = Arc::clone(&reservation_released);
        let operation_lease_released = Arc::clone(&lease_released);
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("process-ran-despite-publication-errors");
        let operation_marker = marker.clone();

        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let operation_id = scope.id();
                let reservation = Reservation::new(
                    1,
                    1,
                    move |_| operation_reservation_released.store(true, Ordering::SeqCst),
                    move |_| {
                        let released = Arc::clone(&operation_lease_released);
                        crate::execution::contracts::ReservationLease::new(move || {
                            released.store(true, Ordering::SeqCst);
                        })
                    },
                );
                let request = Request {
                    operation_id: None,
                    runner: CommandRunner::without_timeout(),
                    command: Command::new("sh")
                        .arg("-c")
                        .arg(&format!("touch {}", operation_marker.display())),
                    options: RequestOptions::Standard(CommandRunOptions::default()),
                    snapshot: ExecutionSnapshot {
                        working_directory: std::env::current_dir().unwrap(),
                        environment: Vec::new(),
                    },
                    io_paths: IoPathSnapshot::default(),
                    deadline: None,
                };
                operation_backend.run(
                    ActivatedRequest {
                        operation_id,
                        scope,
                        request,
                        reservation,
                        facts: operation_sink.clone(),
                        session_bridge: None,
                    },
                    operation_sink,
                );
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        operation.wait_settlement().unwrap();
        assert!(marker.exists(), "the authorized process should run before publication");
        assert!(reservation_released.load(Ordering::SeqCst));
        assert!(lease_released.load(Ordering::SeqCst));
        assert!(!sink.reject_outcome_once.load(Ordering::SeqCst));
        assert!(!sink.reject_release_once.load(Ordering::SeqCst));
        assert!(!sink.reject_settlement_once.load(Ordering::SeqCst));
        assert!(sink.outcome.lock().unwrap().is_none());
        assert!(!sink.released.load(Ordering::SeqCst));
        assert!(sink.settlement.lock().unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_pre_cancelled_external_token_stops_backend_before_spawn() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let backend = Arc::new(ProcessBackend::new());
        let sink = Arc::new(RecordingSink::default());
        let cancellation = crate::CommandCancellation::new();
        cancellation.cancel();
        let operation_backend = Arc::clone(&backend);
        let operation_sink = Arc::clone(&sink);
        let operation_cancellation = cancellation.clone();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("must-not-start-with-cancelled-token");
        let operation_path = path.clone();

        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let operation_id = scope.id();
                let reservation = Reservation::new(
                    1,
                    1,
                    |_| {},
                    |_| crate::execution::contracts::ReservationLease::new(|| {}),
                );
                let request = Request {
                    operation_id: None,
                    runner: CommandRunner::without_timeout(),
                    command: Command::new("sh")
                        .arg("-c")
                        .arg(&format!("touch {}", operation_path.display())),
                    options: RequestOptions::Standard(CommandRunOptions::new().cancellation(operation_cancellation)),
                    snapshot: ExecutionSnapshot {
                        working_directory: std::env::current_dir().unwrap(),
                        environment: Vec::new(),
                    },
                    io_paths: IoPathSnapshot::default(),
                    deadline: None,
                };
                operation_backend.run(
                    ActivatedRequest {
                        operation_id,
                        scope,
                        request,
                        reservation,
                        facts: operation_sink.clone(),
                        session_bridge: None,
                    },
                    operation_sink,
                );
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        operation.wait_settlement().unwrap();
        assert!(!path.exists(), "a pre-cancelled request must not spawn");
        assert!(matches!(
            sink.outcome.lock().unwrap().take(),
            Some(OperationOutcome::Stopped(report)) if report.reason == StopReason::UserCancelled
        ));
        assert!(sink.released.load(Ordering::SeqCst));
        assert!(sink.settlement.lock().unwrap().as_ref().unwrap().all_released());
    }

    #[cfg(unix)]
    #[test]
    fn test_external_cancellation_after_spawn_publishes_user_stop_and_releases() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let backend = Arc::new(ProcessBackend::new());
        let sink = Arc::new(RecordingSink::default());
        let cancellation = crate::CommandCancellation::new();
        let operation_backend = Arc::clone(&backend);
        let operation_sink = Arc::clone(&sink);
        let operation_cancellation = cancellation.clone();
        let directory = tempfile::tempdir().unwrap();
        let ready = directory.path().join("external-cancel-ready");
        let output = directory.path().join("external-cancel-output");
        let operation_ready = ready.clone();
        let operation_output = output.clone();

        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let operation_id = scope.id();
                let request = Request {
                    operation_id: None,
                    runner: CommandRunner::without_timeout(),
                    command: Command::new("sh").arg("-c").arg(&format!(
                        "printf partial; touch {}; exec sleep 30",
                        operation_ready.display()
                    )),
                    options: RequestOptions::Standard(
                        CommandRunOptions::new()
                            .cancellation(operation_cancellation)
                            .tee_stdout_to_file(&operation_output),
                    ),
                    snapshot: ExecutionSnapshot {
                        working_directory: std::env::current_dir().unwrap(),
                        environment: Vec::new(),
                    },
                    io_paths: IoPathSnapshot::default(),
                    deadline: None,
                };
                let reservation = Reservation::new(
                    1,
                    1,
                    |_| {},
                    |_| crate::execution::contracts::ReservationLease::new(|| {}),
                );
                operation_backend.run(
                    ActivatedRequest {
                        operation_id,
                        scope,
                        request,
                        reservation,
                        facts: operation_sink.clone(),
                        session_bridge: None,
                    },
                    operation_sink,
                );
                Ok::<_, ()>(())
            })
            .unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut output_reached_reader = false;
        while std::time::Instant::now() < deadline {
            if ready.exists() && std::fs::read(&output).is_ok_and(|bytes| bytes == b"partial") {
                output_reached_reader = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        cancellation.cancel();
        assert!(ready.exists(), "the command must start before external cancellation");
        assert!(
            output_reached_reader,
            "stdout should reach the reader before external cancellation"
        );

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        operation.wait_settlement().unwrap();
        assert!(matches!(
            sink.outcome.lock().unwrap().take(),
            Some(OperationOutcome::Stopped(report)) if report.reason == StopReason::UserCancelled
        ));
        let diagnostic = sink
            .stop_diagnostic
            .lock()
            .unwrap()
            .take()
            .expect("external cancellation should preserve the runner diagnostic");
        assert_eq!(diagnostic.kind(), crate::CommandErrorKind::Cancelled);
        assert_eq!(diagnostic.captured_output().unwrap().stdout(), b"partial");
        assert!(sink.released.load(Ordering::SeqCst));
        assert!(sink.settlement.lock().unwrap().as_ref().unwrap().all_released());
    }

    #[cfg(unix)]
    #[test]
    fn test_spawn_failure_publishes_error_and_releases_reservation() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let backend = Arc::new(ProcessBackend::new());
        let sink = Arc::new(RecordingSink::default());
        let reservation_released = Arc::new(AtomicBool::new(false));
        let lease_released = Arc::new(AtomicBool::new(false));
        let operation_backend = Arc::clone(&backend);
        let operation_sink = Arc::clone(&sink);
        let operation_reservation_released = Arc::clone(&reservation_released);
        let operation_lease_released = Arc::clone(&lease_released);
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("missing-command");
        let input = tempfile::NamedTempFile::new().expect("managed input should be available before spawn");
        let input_path = input.path().to_path_buf();
        let input_file = input.reopen().expect("managed input should reopen before spawn");
        let input_fd = std::os::fd::AsRawFd::as_raw_fd(&input_file);
        assert_eq!(unsafe { libc::flock(input_fd, libc::LOCK_EX | libc::LOCK_NB) }, 0);
        assert!(!executable.exists());

        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let operation_id = scope.id();
                let reservation = Reservation::new(
                    1,
                    1,
                    move |_| operation_reservation_released.store(true, Ordering::SeqCst),
                    move |_| {
                        let released = Arc::clone(&operation_lease_released);
                        crate::execution::contracts::ReservationLease::new(move || {
                            released.store(true, Ordering::SeqCst);
                        })
                    },
                );
                let request = Request {
                    operation_id: None,
                    runner: CommandRunner::without_timeout(),
                    command: Command::new(executable.to_str().expect("temporary path should be UTF-8")),
                    options: RequestOptions::Resources(CommandResourceRunOptions::new().stdin_file(input_file)),
                    snapshot: ExecutionSnapshot {
                        working_directory: std::env::current_dir().unwrap(),
                        environment: Vec::new(),
                    },
                    io_paths: IoPathSnapshot::default(),
                    deadline: None,
                };
                operation_backend.run(
                    ActivatedRequest {
                        operation_id,
                        scope,
                        request,
                        reservation,
                        facts: operation_sink.clone(),
                        session_bridge: None,
                    },
                    operation_sink,
                );
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        operation.wait_settlement().unwrap();

        let Some(OperationOutcome::Failed(error)) = sink.outcome.lock().unwrap().take() else {
            panic!("spawn failure should be published as a command failure");
        };
        assert_eq!(error.kind(), crate::CommandErrorKind::SpawnFailed);
        assert!(matches!(error.reason(), CommandErrorReason::SpawnFailed { .. }));
        assert!(sink.released.load(Ordering::SeqCst));
        assert!(reservation_released.load(Ordering::SeqCst));
        assert!(lease_released.load(Ordering::SeqCst));
        assert!(sink.settlement.lock().unwrap().as_ref().unwrap().all_released());
        let input_probe = std::fs::File::open(input_path).expect("managed input path should remain available");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            let locked = unsafe {
                libc::flock(
                    std::os::fd::AsRawFd::as_raw_fd(&input_probe),
                    libc::LOCK_EX | libc::LOCK_NB,
                )
            };
            if locked == 0 {
                break;
            }
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::EWOULDBLOCK),
                "lock probe should fail only while the request-owned descriptor remains open"
            );
            assert!(
                std::time::Instant::now() < deadline,
                "spawn failure should eventually release the request-owned stdin descriptor"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_invalid_working_directory_spawn_failure_releases_reservation() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let backend = Arc::new(ProcessBackend::new());
        let sink = Arc::new(RecordingSink::default());
        let reservation_released = Arc::new(AtomicBool::new(false));
        let lease_released = Arc::new(AtomicBool::new(false));
        let operation_backend = Arc::clone(&backend);
        let operation_sink = Arc::clone(&sink);
        let operation_reservation_released = Arc::clone(&reservation_released);
        let operation_lease_released = Arc::clone(&lease_released);
        let directory = tempfile::tempdir().unwrap();
        let missing_directory = directory.path().join("missing-working-directory");
        assert!(!missing_directory.exists());

        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let operation_id = scope.id();
                let reservation = Reservation::new(
                    1,
                    1,
                    move |_| operation_reservation_released.store(true, Ordering::SeqCst),
                    move |_| {
                        let released = Arc::clone(&operation_lease_released);
                        crate::execution::contracts::ReservationLease::new(move || {
                            released.store(true, Ordering::SeqCst);
                        })
                    },
                );
                let request = Request {
                    operation_id: None,
                    runner: CommandRunner::without_timeout().working_directory(&missing_directory),
                    command: Command::new("sh").arg("-c").arg("exit 0"),
                    options: RequestOptions::Standard(CommandRunOptions::default()),
                    snapshot: ExecutionSnapshot {
                        working_directory: std::env::current_dir().unwrap(),
                        environment: Vec::new(),
                    },
                    io_paths: IoPathSnapshot::default(),
                    deadline: None,
                };
                operation_backend.run(
                    ActivatedRequest {
                        operation_id,
                        scope,
                        request,
                        reservation,
                        facts: operation_sink.clone(),
                        session_bridge: None,
                    },
                    operation_sink,
                );
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        operation.wait_settlement().unwrap();

        let Some(OperationOutcome::Failed(error)) = sink.outcome.lock().unwrap().take() else {
            panic!("invalid working directory should fail process startup");
        };
        assert_eq!(error.kind(), crate::CommandErrorKind::SpawnFailed);
        assert!(matches!(
            error.reason(),
            CommandErrorReason::SpawnFailed { source } if source.kind() == std::io::ErrorKind::NotFound
        ));
        assert!(sink.released.load(Ordering::SeqCst));
        assert!(reservation_released.load(Ordering::SeqCst));
        assert!(lease_released.load(Ordering::SeqCst));
        assert!(sink.settlement.lock().unwrap().as_ref().unwrap().all_released());
    }

    #[cfg(unix)]
    #[test]
    fn test_expired_deadline_stops_backend_before_spawn() {
        run_release_observation_test(|| {
            let manager = OperationManager::new(Default::default()).unwrap();
            let backend = Arc::new(ProcessBackend::new());
            let sink = Arc::new(RecordingSink::default());
            let operation_backend = Arc::clone(&backend);
            let operation_sink = Arc::clone(&sink);
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("must-not-start-after-deadline");
            let operation_path = path.clone();
            let input = tempfile::NamedTempFile::new().unwrap();
            let input_path = input.path().to_path_buf();
            let input_file = input.reopen().unwrap();
            assert_eq!(
                unsafe {
                    libc::flock(
                        std::os::fd::AsRawFd::as_raw_fd(&input_file),
                        libc::LOCK_EX | libc::LOCK_NB,
                    )
                },
                0
            );
            *sink.release_probe_path.lock().unwrap() = Some(input_path);
            let domain = ClockDomainId::new_unique();
            let deadline = OperationContext::new(ContextOptions {
                clock: Some(Arc::new(ExpiredClock(domain))),
                deadline: Some(Deadline::at(MonotonicTime::new(domain, 0))),
                budget: None,
            })
            .unwrap();

            let mut operation = manager
                .try_submit(OperationOptions::default(), move |scope| {
                    let operation_id = scope.id();
                    let reservation = Reservation::new(
                        1,
                        1,
                        |_| {},
                        |_| crate::execution::contracts::ReservationLease::new(|| {}),
                    );
                    let request = Request {
                        operation_id: None,
                        runner: CommandRunner::without_timeout(),
                        command: Command::new("sh")
                            .arg("-c")
                            .arg(&format!("touch {}", operation_path.display())),
                        options: RequestOptions::Resources(CommandResourceRunOptions::new().stdin_file(input_file)),
                        snapshot: ExecutionSnapshot {
                            working_directory: std::env::current_dir().unwrap(),
                            environment: Vec::new(),
                        },
                        io_paths: IoPathSnapshot::default(),
                        deadline: Some(deadline),
                    };
                    operation_backend.run(
                        ActivatedRequest {
                            operation_id,
                            scope,
                            request,
                            reservation,
                            facts: operation_sink.clone(),
                            session_bridge: None,
                        },
                        operation_sink,
                    );
                    Ok::<_, ()>(())
                })
                .unwrap();

            assert!(matches!(
                operation.wait_outcome().unwrap(),
                OperationOutcome::Succeeded(())
            ));
            operation.wait_settlement().unwrap();
            assert!(!path.exists(), "an expired request must not spawn");
            assert!(matches!(
                sink.outcome.lock().unwrap().take(),
                Some(OperationOutcome::Stopped(report)) if report.reason == StopReason::Timeout
            ));
            assert!(sink.released.load(Ordering::SeqCst));
            assert!(sink.release_observed_unlocked.load(Ordering::SeqCst));
            assert!(sink.settlement.lock().unwrap().as_ref().unwrap().all_released());
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_external_cancel_between_deadline_checks_keeps_first_reason() {
        run_release_observation_test(|| {
            let manager = OperationManager::new(Default::default()).unwrap();
            let backend = Arc::new(ProcessBackend::new());
            let sink = Arc::new(RecordingSink::default());
            let reservation_released = Arc::new(AtomicBool::new(false));
            let lease_created = Arc::new(AtomicBool::new(false));
            let operation_backend = Arc::clone(&backend);
            let operation_sink = Arc::clone(&sink);
            let operation_reservation_released = Arc::clone(&reservation_released);
            let operation_lease_created = Arc::clone(&lease_created);
            let directory = tempfile::tempdir().unwrap();
            let marker = directory.path().join("must-not-start-after-second-deadline-check");
            let operation_marker = marker.clone();
            let input = tempfile::NamedTempFile::new().unwrap();
            let input_path = input.path().to_path_buf();
            let input_file = input.reopen().unwrap();
            assert_eq!(
                unsafe {
                    libc::flock(
                        std::os::fd::AsRawFd::as_raw_fd(&input_file),
                        libc::LOCK_EX | libc::LOCK_NB,
                    )
                },
                0
            );
            *sink.release_probe_path.lock().unwrap() = Some(input_path);
            let domain = ClockDomainId::new_unique();
            let clock_calls = Arc::new(AtomicUsize::new(0));
            let cancellation = crate::CommandCancellation::new();
            let clock: Arc<dyn ClockSource> = Arc::new(DeadlineExpiresBeforeAuthorization(
                domain,
                Arc::clone(&clock_calls),
                Some((1, cancellation.clone())),
            ));
            let deadline = OperationContext::new(ContextOptions {
                clock: Some(Arc::clone(&clock)),
                deadline: Some(Deadline::at(MonotonicTime::new(domain, 1))),
                budget: None,
            })
            .unwrap();

            let mut operation = manager
                .try_submit(OperationOptions::default(), move |scope| {
                    let operation_id = scope.id();
                    let reservation = Reservation::new(
                        1,
                        1,
                        move |_| operation_reservation_released.store(true, Ordering::SeqCst),
                        move |_| {
                            operation_lease_created.store(true, Ordering::SeqCst);
                            crate::execution::contracts::ReservationLease::new(|| {})
                        },
                    );
                    let request = Request {
                        operation_id: None,
                        runner: CommandRunner::without_timeout(),
                        command: Command::new("sh")
                            .arg("-c")
                            .arg(&format!("touch {}", operation_marker.display())),
                        options: RequestOptions::Resources(
                            CommandResourceRunOptions::new()
                                .options(CommandRunOptions::new().cancellation(cancellation.clone()))
                                .stdin_file(input_file),
                        ),
                        snapshot: ExecutionSnapshot {
                            working_directory: std::env::current_dir().unwrap(),
                            environment: Vec::new(),
                        },
                        io_paths: IoPathSnapshot::default(),
                        deadline: Some(deadline),
                    };
                    operation_backend.run(
                        ActivatedRequest {
                            operation_id,
                            scope,
                            request,
                            reservation,
                            facts: operation_sink.clone(),
                            session_bridge: None,
                        },
                        operation_sink,
                    );
                    Ok::<_, ()>(())
                })
                .unwrap();

            assert!(matches!(
                operation.wait_outcome().unwrap(),
                OperationOutcome::Succeeded(())
            ));
            operation.wait_settlement().unwrap();

            assert!(
                !marker.exists(),
                "deadline expiry after authorization must still prevent spawn"
            );
            assert_eq!(
                clock_calls.load(Ordering::SeqCst),
                2,
                "external cancellation after the first backend deadline check should stop before the second check"
            );
            assert!(reservation_released.load(Ordering::SeqCst));
            assert!(
                !lease_created.load(Ordering::SeqCst),
                "pre-start stop should not retain process capacity"
            );
            assert!(matches!(
                sink.outcome.lock().unwrap().take(),
                Some(OperationOutcome::Stopped(report)) if report.reason == StopReason::UserCancelled
            ));
            assert!(sink.released.load(Ordering::SeqCst));
            assert!(sink.release_observed_unlocked.load(Ordering::SeqCst));
            assert!(sink.settlement.lock().unwrap().as_ref().unwrap().all_released());
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_deadline_expiring_at_runner_final_check_stops_before_spawn_and_retains_diagnostic() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let backend = Arc::new(ProcessBackend::new());
        let sink = Arc::new(RecordingSink::default());
        let operation_backend = Arc::clone(&backend);
        let operation_sink = Arc::clone(&sink);
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("must-not-start-after-runner-deadline");
        let operation_marker = marker.clone();
        let domain = ClockDomainId::new_unique();
        let clock_calls = Arc::new(AtomicUsize::new(0));
        let cancellation = crate::CommandCancellation::new();
        let clock: Arc<dyn ClockSource> = Arc::new(DeadlineExpiresAtRunnerCheck(
            domain,
            Arc::clone(&clock_calls),
            Some((3, cancellation.clone())),
        ));
        let deadline = OperationContext::new(ContextOptions {
            clock: Some(clock),
            deadline: Some(Deadline::at(MonotonicTime::new(domain, 1))),
            budget: None,
        })
        .unwrap();

        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let operation_id = scope.id();
                let request = Request {
                    operation_id: None,
                    runner: CommandRunner::without_timeout(),
                    command: Command::new("sh")
                        .arg("-c")
                        .arg(&format!("touch {}", operation_marker.display())),
                    options: RequestOptions::Standard(CommandRunOptions::new().cancellation(cancellation.clone())),
                    snapshot: ExecutionSnapshot {
                        working_directory: std::env::current_dir().unwrap(),
                        environment: Vec::new(),
                    },
                    io_paths: IoPathSnapshot::default(),
                    deadline: Some(deadline),
                };
                let reservation = Reservation::new(
                    1,
                    1,
                    |_| {},
                    |_| crate::execution::contracts::ReservationLease::new(|| {}),
                );
                operation_backend.run(
                    ActivatedRequest {
                        operation_id,
                        scope,
                        request,
                        reservation,
                        facts: operation_sink.clone(),
                        session_bridge: None,
                    },
                    operation_sink,
                );
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        operation.wait_settlement().unwrap();

        assert_eq!(
            clock_calls.load(Ordering::SeqCst),
            4,
            "the clock should expire only at the runner's final pre-spawn check"
        );
        assert!(!marker.exists(), "runner-side deadline expiry must prevent spawn");
        assert!(matches!(
            sink.outcome.lock().unwrap().take(),
            Some(OperationOutcome::Stopped(report)) if report.reason == StopReason::Timeout
        ));
        let diagnostic = sink
            .stop_diagnostic
            .lock()
            .unwrap()
            .take()
            .expect("runner-side deadline stop should retain its diagnostic");
        assert_eq!(diagnostic.kind(), crate::CommandErrorKind::CancelledBeforeStart);
        assert!(sink.stop_diagnostic.lock().unwrap().is_none());
        assert!(sink.released.load(Ordering::SeqCst));
        assert!(sink.settlement.lock().unwrap().as_ref().unwrap().all_released());
    }

    #[cfg(unix)]
    #[test]
    fn test_stop_after_backend_check_before_runner_authorization_stops_before_spawn() {
        use std::sync::Barrier;

        let _preparation_guard = crate::execution::files::input::tests::PREPARATION_TEST_LOCK
            .lock()
            .expect("preparation test lock should be available");
        let manager = OperationManager::new(Default::default()).unwrap();
        let backend = Arc::new(ProcessBackend::new());
        let sink = Arc::new(RecordingSink::default());
        let operation_backend = Arc::clone(&backend);
        let operation_sink = Arc::clone(&sink);
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("must-not-start-after-late-stop");
        let operation_marker = marker.clone();
        let output = tempfile::NamedTempFile::new().unwrap();
        let output_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(output.path())
            .unwrap();
        let gate = Arc::new((Barrier::new(2), Barrier::new(2)));
        let operation_gate = Arc::clone(&gate);
        let (stop_port_send, stop_port_receive) = std::sync::mpsc::sync_channel(1);

        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let operation_id = scope.id();
                stop_port_send
                    .send(operation_backend.stop_port(operation_id))
                    .expect("test should receive the backend stop port");
                crate::execution::files::prepared_files::gate_after_first_registration(Some((
                    operation_id,
                    operation_gate,
                )));
                let request = Request {
                    operation_id: None,
                    runner: CommandRunner::without_timeout(),
                    command: Command::new("sh")
                        .arg("-c")
                        .arg(&format!("touch {}", operation_marker.display())),
                    options: RequestOptions::Resources(
                        CommandResourceRunOptions::new().stdout_sink(output_file, FileSinkPolicy::Preserve),
                    ),
                    snapshot: ExecutionSnapshot {
                        working_directory: std::env::current_dir().unwrap(),
                        environment: Vec::new(),
                    },
                    io_paths: IoPathSnapshot::default(),
                    deadline: None,
                };
                let reservation = Reservation::new(
                    1,
                    1,
                    |_| {},
                    |_| crate::execution::contracts::ReservationLease::new(|| {}),
                );
                operation_backend.run(
                    ActivatedRequest {
                        operation_id,
                        scope,
                        request,
                        reservation,
                        facts: operation_sink.clone(),
                        session_bridge: None,
                    },
                    operation_sink,
                );
                crate::execution::files::prepared_files::gate_after_first_registration(None);
                Ok::<_, ()>(())
            })
            .unwrap();

        gate.0.wait();
        stop_port_receive
            .recv()
            .expect("backend stop port should be available")
            .request_stop(StopReason::UserCancelled);
        gate.1.wait();

        let _operation_outcome = operation.wait_outcome().unwrap();
        operation.wait_settlement().unwrap();
        assert!(
            !marker.exists(),
            "a stop before runner authorization must prevent spawn"
        );
        assert!(matches!(
            sink.outcome.lock().unwrap().take(),
            Some(OperationOutcome::Stopped(report)) if report.reason == StopReason::UserCancelled
        ));
        let diagnostic = sink
            .stop_diagnostic
            .lock()
            .unwrap()
            .take()
            .expect("late pre-start stop should retain its runner diagnostic");
        assert_eq!(diagnostic.kind(), crate::CommandErrorKind::CancelledBeforeStart);
        assert!(sink.stop_diagnostic.lock().unwrap().is_none());
        assert!(sink.released.load(Ordering::SeqCst));
        assert!(sink.settlement.lock().unwrap().as_ref().unwrap().all_released());
    }

    #[cfg(unix)]
    #[test]
    fn test_second_deadline_check_publication_failures_still_release_reservation() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let backend = Arc::new(ProcessBackend::new());
        let sink = Arc::new(RecordingSink::default());
        sink.reject_outcome_once.store(true, Ordering::SeqCst);
        sink.reject_release_once.store(true, Ordering::SeqCst);
        sink.reject_settlement_once.store(true, Ordering::SeqCst);
        let reservation_released = Arc::new(AtomicBool::new(false));
        let lease_created = Arc::new(AtomicBool::new(false));
        let operation_backend = Arc::clone(&backend);
        let operation_sink = Arc::clone(&sink);
        let operation_reservation_released = Arc::clone(&reservation_released);
        let operation_lease_created = Arc::clone(&lease_created);
        let directory = tempfile::tempdir().unwrap();
        let marker = directory
            .path()
            .join("must-not-start-after-deadline-publication-errors");
        let operation_marker = marker.clone();
        let domain = ClockDomainId::new_unique();
        let clock_calls = Arc::new(AtomicUsize::new(0));
        let clock: Arc<dyn ClockSource> = Arc::new(DeadlineExpiresBeforeAuthorization(
            domain,
            Arc::clone(&clock_calls),
            None,
        ));
        let deadline = OperationContext::new(ContextOptions {
            clock: Some(Arc::clone(&clock)),
            deadline: Some(Deadline::at(MonotonicTime::new(domain, 1))),
            budget: None,
        })
        .unwrap();

        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let operation_id = scope.id();
                let reservation = Reservation::new(
                    1,
                    1,
                    move |_| operation_reservation_released.store(true, Ordering::SeqCst),
                    move |_| {
                        operation_lease_created.store(true, Ordering::SeqCst);
                        crate::execution::contracts::ReservationLease::new(|| {})
                    },
                );
                let request = Request {
                    operation_id: None,
                    runner: CommandRunner::without_timeout(),
                    command: Command::new("sh")
                        .arg("-c")
                        .arg(&format!("touch {}", operation_marker.display())),
                    options: RequestOptions::Standard(CommandRunOptions::default()),
                    snapshot: ExecutionSnapshot {
                        working_directory: std::env::current_dir().unwrap(),
                        environment: Vec::new(),
                    },
                    io_paths: IoPathSnapshot::default(),
                    deadline: Some(deadline),
                };
                operation_backend.run(
                    ActivatedRequest {
                        operation_id,
                        scope,
                        request,
                        reservation,
                        facts: operation_sink.clone(),
                        session_bridge: None,
                    },
                    operation_sink,
                );
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        operation.wait_settlement().unwrap();
        assert!(!marker.exists(), "the second deadline check must still prevent spawn");
        assert_eq!(clock_calls.load(Ordering::SeqCst), 3);
        assert!(reservation_released.load(Ordering::SeqCst));
        assert!(!lease_created.load(Ordering::SeqCst));
        assert!(!sink.reject_outcome_once.load(Ordering::SeqCst));
        assert!(!sink.reject_release_once.load(Ordering::SeqCst));
        assert!(!sink.reject_settlement_once.load(Ordering::SeqCst));
        assert!(sink.outcome.lock().unwrap().is_none());
        assert!(!sink.released.load(Ordering::SeqCst));
        assert!(sink.settlement.lock().unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_nonzero_child_exit_publishes_failure_and_releases_inputs() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let backend = Arc::new(ProcessBackend::new());
        let sink = Arc::new(RecordingSink::default());
        let operation_backend = Arc::clone(&backend);
        let operation_sink = Arc::clone(&sink);

        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let operation_id = scope.id();
                let request = Request {
                    operation_id: None,
                    runner: CommandRunner::without_timeout(),
                    command: Command::new("sh").arg("-c").arg("exit 23"),
                    options: RequestOptions::Standard(CommandRunOptions::default()),
                    snapshot: ExecutionSnapshot {
                        working_directory: std::env::current_dir().unwrap(),
                        environment: Vec::new(),
                    },
                    io_paths: IoPathSnapshot::default(),
                    deadline: None,
                };
                let reservation = Reservation::new(
                    1,
                    1,
                    |_| {},
                    |_| crate::execution::contracts::ReservationLease::new(|| {}),
                );
                operation_backend.run(
                    ActivatedRequest {
                        operation_id,
                        scope,
                        request,
                        reservation,
                        facts: operation_sink.clone(),
                        session_bridge: None,
                    },
                    operation_sink,
                );
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        operation.wait_settlement().unwrap();
        assert!(matches!(
            sink.outcome.lock().unwrap().take(),
            Some(OperationOutcome::Failed(_))
        ));
        assert!(sink.released.load(Ordering::SeqCst));
        assert!(sink.settlement.lock().unwrap().as_ref().unwrap().all_released());
    }

    #[cfg(unix)]
    #[test]
    fn test_successful_child_publishes_output_and_releases_inputs() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let backend = Arc::new(ProcessBackend::new());
        let sink = Arc::new(RecordingSink::default());
        let operation_backend = Arc::clone(&backend);
        let operation_sink = Arc::clone(&sink);
        let domain = ClockDomainId::new_unique();
        // The clock is at tick 1 while the deadline is at tick 2. This drives
        // the backend's late-stop check with a live, unexpired context.
        let deadline = OperationContext::new(ContextOptions {
            clock: Some(Arc::new(ExpiredClock(domain))),
            deadline: Some(Deadline::at(MonotonicTime::new(domain, 2))),
            budget: None,
        })
        .unwrap();

        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let operation_id = scope.id();
                let request = Request {
                    operation_id: None,
                    runner: CommandRunner::without_timeout(),
                    command: Command::new("sh").arg("-c").arg("printf managed-ok"),
                    options: RequestOptions::Standard(CommandRunOptions::default()),
                    snapshot: ExecutionSnapshot {
                        working_directory: std::env::current_dir().unwrap(),
                        environment: Vec::new(),
                    },
                    io_paths: IoPathSnapshot::default(),
                    deadline: Some(deadline),
                };
                let reservation = Reservation::new(
                    1,
                    1,
                    |_| {},
                    |_| crate::execution::contracts::ReservationLease::new(|| {}),
                );
                operation_backend.run(
                    ActivatedRequest {
                        operation_id,
                        scope,
                        request,
                        reservation,
                        facts: operation_sink.clone(),
                        session_bridge: None,
                    },
                    operation_sink,
                );
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        operation.wait_settlement().unwrap();
        assert!(matches!(
            sink.outcome.lock().unwrap().take(),
            Some(OperationOutcome::Succeeded(output)) if output.stdout() == b"managed-ok"
        ));
        assert!(sink.released.load(Ordering::SeqCst));
        assert!(sink.settlement.lock().unwrap().as_ref().unwrap().all_released());
    }

    #[cfg(unix)]
    #[test]
    fn test_prepared_resource_input_reaches_child_and_releases_reservation() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let backend = Arc::new(ProcessBackend::new());
        let sink = Arc::new(RecordingSink::default());
        let capacity_released = Arc::new(AtomicBool::new(false));
        let operation_backend = Arc::clone(&backend);
        let operation_sink = Arc::clone(&sink);
        let operation_capacity_released = Arc::clone(&capacity_released);
        let mut input = tempfile::tempfile().unwrap();
        std::io::Write::write_all(&mut input, b"resource-input").unwrap();
        std::io::Seek::rewind(&mut input).unwrap();

        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let operation_id = scope.id();
                let reservation = Reservation::new(
                    1,
                    1,
                    |_| {},
                    move |_| {
                        let released = Arc::clone(&operation_capacity_released);
                        crate::execution::contracts::ReservationLease::new(move || {
                            released.store(true, Ordering::SeqCst);
                        })
                    },
                );
                let request = Request {
                    operation_id: None,
                    runner: CommandRunner::without_timeout(),
                    command: Command::new("sh").arg("-c").arg("cat"),
                    options: RequestOptions::Resources(CommandResourceRunOptions::new().stdin_file(input)),
                    snapshot: ExecutionSnapshot {
                        working_directory: std::env::current_dir().unwrap(),
                        environment: Vec::new(),
                    },
                    io_paths: IoPathSnapshot::default(),
                    deadline: None,
                };
                operation_backend.run(
                    ActivatedRequest {
                        operation_id,
                        scope,
                        request,
                        reservation,
                        facts: operation_sink.clone(),
                        session_bridge: None,
                    },
                    operation_sink,
                );
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        operation.wait_settlement().unwrap();
        assert!(matches!(
            sink.outcome.lock().unwrap().take(),
            Some(OperationOutcome::Succeeded(output)) if output.stdout() == b"resource-input"
        ));
        assert!(sink.released.load(Ordering::SeqCst));
        assert!(sink.settlement.lock().unwrap().as_ref().unwrap().all_released());
        assert!(capacity_released.load(Ordering::SeqCst));
    }

    #[cfg(unix)]
    #[test]
    fn test_preparation_failure_retains_recovery_owner_until_scope_cleanup() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let backend = Arc::new(ProcessBackend::new());
        let sink = Arc::new(RecordingSink::default());
        let capacity_released = Arc::new(AtomicBool::new(false));
        let operation_backend = Arc::clone(&backend);
        let operation_sink = Arc::clone(&sink);
        let operation_capacity_released = Arc::clone(&capacity_released);
        let file = tempfile::tempfile().unwrap();
        let duplicate = file.try_clone().unwrap();

        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let operation_id = scope.id();
                let reservation = Reservation::new(
                    1,
                    1,
                    |_| {},
                    move |_| {
                        let released = Arc::clone(&operation_capacity_released);
                        crate::execution::contracts::ReservationLease::new(move || {
                            released.store(true, Ordering::SeqCst);
                        })
                    },
                );
                let request = Request {
                    operation_id: None,
                    runner: CommandRunner::without_timeout(),
                    command: Command::new("sh").arg("-c").arg("exit 99"),
                    options: RequestOptions::Resources(
                        CommandResourceRunOptions::new()
                            .stdin_file(duplicate)
                            .stdout_sink(file, FileSinkPolicy::Truncate),
                    ),
                    snapshot: ExecutionSnapshot {
                        working_directory: std::env::current_dir().unwrap(),
                        environment: Vec::new(),
                    },
                    io_paths: IoPathSnapshot::default(),
                    deadline: None,
                };
                operation_backend.run(
                    ActivatedRequest {
                        operation_id,
                        scope,
                        request,
                        reservation,
                        facts: operation_sink.clone(),
                        session_bridge: None,
                    },
                    operation_sink,
                );
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert!(!sink.released.load(Ordering::SeqCst));
        assert!(sink.settlement.lock().unwrap().is_none());
        let outcome = sink.outcome.lock().unwrap().take().unwrap();
        assert!(matches!(
            outcome,
            OperationOutcome::Failed(error)
                if matches!(
                    error.reason(),
                    CommandErrorReason::FilePreparationFailed {
                        stage: crate::FilePreparationStage::Validation,
                        direction: Some(crate::FilePreparationDirection::Stdout),
                        ..
                    }
                )
        ));

        let owner = sink
            .recovery_owner
            .lock()
            .unwrap()
            .take()
            .expect("partial prepared resources should remain under recovery ownership");
        assert_eq!(owner.summary(), "command file preparation retained partial resources");
        assert!(!owner.is_complete());
        owner.retry_cleanup().unwrap();
        assert!(owner.is_complete());
        assert_eq!(
            owner.retry_cleanup().unwrap_err(),
            RecoveryRetryError::OwnerAlreadyConsumed
        );
        operation.wait_settlement().unwrap();
        assert!(!capacity_released.load(Ordering::SeqCst));
        drop(owner);
        assert!(capacity_released.load(Ordering::SeqCst));
    }

    #[cfg(unix)]
    #[test]
    fn test_preparation_recovery_retries_rejected_owner_before_settlement() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let backend = Arc::new(ProcessBackend::new());
        let sink = Arc::new(RecordingSink::default());
        sink.reject_recovery_once.store(true, Ordering::SeqCst);
        let capacity_released = Arc::new(AtomicBool::new(false));
        let operation_backend = Arc::clone(&backend);
        let operation_sink = Arc::clone(&sink);
        let operation_capacity_released = Arc::clone(&capacity_released);
        let file = tempfile::tempfile().expect("aliased resource file should be created");
        let duplicate = file.try_clone().expect("aliased resource file should clone");

        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let operation_id = scope.id();
                let reservation = Reservation::new(
                    1,
                    1,
                    |_| {},
                    move |_| {
                        let released = Arc::clone(&operation_capacity_released);
                        crate::execution::contracts::ReservationLease::new(move || {
                            released.store(true, Ordering::SeqCst);
                        })
                    },
                );
                let request = Request {
                    operation_id: None,
                    runner: CommandRunner::without_timeout(),
                    command: Command::new("sh").arg("-c").arg("exit 99"),
                    options: RequestOptions::Resources(
                        CommandResourceRunOptions::new()
                            .stdin_file(duplicate)
                            .stdout_sink(file, FileSinkPolicy::Truncate),
                    ),
                    snapshot: ExecutionSnapshot {
                        working_directory: std::env::current_dir().unwrap(),
                        environment: Vec::new(),
                    },
                    io_paths: IoPathSnapshot::default(),
                    deadline: None,
                };
                operation_backend.run(
                    ActivatedRequest {
                        operation_id,
                        scope,
                        request,
                        reservation,
                        facts: operation_sink.clone(),
                        session_bridge: None,
                    },
                    operation_sink,
                );
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert_eq!(sink.recovery_attempts.load(Ordering::SeqCst), 2);
        assert!(!sink.released.load(Ordering::SeqCst));
        assert!(sink.settlement.lock().unwrap().is_none());
        assert!(matches!(
            sink.outcome.lock().unwrap().take(),
            Some(OperationOutcome::Failed(error))
                if matches!(
                    error.reason(),
                    CommandErrorReason::FilePreparationFailed {
                        stage: crate::FilePreparationStage::Validation,
                        direction: Some(crate::FilePreparationDirection::Stdout),
                        ..
                    }
                )
        ));

        let owner = sink
            .recovery_owner
            .lock()
            .unwrap()
            .take()
            .expect("retried owner should be retained by the recovery store");
        assert!(!owner.is_complete());
        owner.retry_cleanup().unwrap();
        operation.wait_settlement().unwrap();
        assert!(!capacity_released.load(Ordering::SeqCst));
        drop(owner);
        assert!(capacity_released.load(Ordering::SeqCst));
    }

    #[cfg(unix)]
    #[test]
    fn test_rejected_preparation_recovery_transfer_retries_without_publishing_release_early() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let backend = Arc::new(ProcessBackend::new());
        let sink = Arc::new(RecordingSink::default());
        sink.reject_outcome_once.store(true, Ordering::SeqCst);
        sink.reject_recovery_once.store(true, Ordering::SeqCst);
        let capacity_released = Arc::new(AtomicBool::new(false));
        let operation_backend = Arc::clone(&backend);
        let operation_sink = Arc::clone(&sink);
        let operation_capacity_released = Arc::clone(&capacity_released);
        let file = tempfile::tempfile().expect("aliased resource file should be created");
        let duplicate = file.try_clone().expect("aliased resource file should clone");

        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let operation_id = scope.id();
                let reservation = Reservation::new(
                    1,
                    1,
                    |_| {},
                    move |_| {
                        let released = Arc::clone(&operation_capacity_released);
                        crate::execution::contracts::ReservationLease::new(move || {
                            released.store(true, Ordering::SeqCst);
                        })
                    },
                );
                let request = Request {
                    operation_id: None,
                    runner: CommandRunner::without_timeout(),
                    command: Command::new("sh").arg("-c").arg("exit 99"),
                    options: RequestOptions::Resources(
                        CommandResourceRunOptions::new()
                            .stdin_file(duplicate)
                            .stdout_sink(file, FileSinkPolicy::Truncate),
                    ),
                    snapshot: ExecutionSnapshot {
                        working_directory: std::env::current_dir().unwrap(),
                        environment: Vec::new(),
                    },
                    io_paths: IoPathSnapshot::default(),
                    deadline: None,
                };
                operation_backend.run(
                    ActivatedRequest {
                        operation_id,
                        scope,
                        request,
                        reservation,
                        facts: operation_sink.clone(),
                        session_bridge: None,
                    },
                    operation_sink,
                );
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert_eq!(sink.recovery_attempts.load(Ordering::SeqCst), 2);
        assert!(!sink.released.load(Ordering::SeqCst));
        assert!(sink.settlement.lock().unwrap().is_none());
        assert!(!sink.reject_outcome_once.load(Ordering::SeqCst));
        assert!(sink.outcome.lock().unwrap().is_none());

        let owner = sink
            .recovery_owner
            .lock()
            .unwrap()
            .take()
            .expect("accepted retry should retain the preparation recovery owner");
        assert_eq!(owner.summary(), "command file preparation retained partial resources");
        assert!(!owner.is_complete());
        owner
            .retry_cleanup()
            .expect("recovery owner should release retained preparation inputs");
        assert!(owner.is_complete());
        assert!(!capacity_released.load(Ordering::SeqCst));
        drop(owner);
        assert!(capacity_released.load(Ordering::SeqCst));
        assert!(operation.wait_settlement().unwrap().all_released());
    }

    #[cfg(unix)]
    #[test]
    fn test_deferred_reaper_owner_transfers_before_backend_release_and_settlement() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let sink = Arc::new(RecordingSink::default());
        sink.reject_recovery_once.store(true, Ordering::SeqCst);
        let capacity_released = Arc::new(AtomicBool::new(false));
        let reservation_released = Arc::new(AtomicBool::new(false));
        let operation_sink = Arc::clone(&sink);
        let operation_capacity_released = Arc::clone(&capacity_released);
        let operation_reservation_released = Arc::clone(&reservation_released);
        let observer_slot = Arc::new(crate::execution::adapter::OperationObserverSlot::default());
        let operation_observer_slot = Arc::clone(&observer_slot);
        let (cleanup_tx, cleanup_rx) = std::sync::mpsc::channel();

        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let reservation = Reservation::new(
                    1,
                    1,
                    move |_| operation_reservation_released.store(true, Ordering::SeqCst),
                    move |_| {
                        let released = Arc::clone(&operation_capacity_released);
                        ReservationLease::new(move || {
                            released.store(true, Ordering::SeqCst);
                        })
                    },
                );
                let lease = reservation.retain();
                let sink_port: Arc<dyn FactSink> = operation_sink.clone();
                let (observer, completion) = ReaperObserver::pair_with_recovery_sink(Arc::clone(&sink_port));
                observer.mark_pending();
                let cleanup_observer = observer.clone();
                let operation_cleanup_tx = cleanup_tx.clone();
                observer.retain_or_defer_reaper_owner(ReaperOwner::new(move || {
                    drop(lease);
                    cleanup_observer.complete();
                    operation_cleanup_tx.send(()).unwrap();
                }));
                let deferred_owner = observer
                    .take_deferred_recovery_owner()
                    .expect("first recovery transfer should be rejected and deferred");
                let transferred = observer.recovery_was_transferred();
                assert!(!transferred);
                operation_sink
                    .publish_outcome(OperationOutcome::Failed(CommandError::from_reason(
                        "backend test",
                        CommandErrorReason::ManagedOperationFailed {
                            diagnostic: "injected command result".into(),
                        },
                        None,
                    )))
                    .unwrap();

                finish_backend_tail(
                    sink_port,
                    observer,
                    completion,
                    operation_observer_slot,
                    (transferred, Some(deferred_owner)),
                    None,
                    scope,
                    reservation,
                );
                Ok::<_, ()>(())
            })
            .unwrap();
        observer_slot.set(operation.observer());

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert_eq!(sink.recovery_attempts.load(Ordering::SeqCst), 2);
        assert!(reservation_released.load(Ordering::SeqCst));
        assert!(!capacity_released.load(Ordering::SeqCst));
        assert!(!sink.released.load(Ordering::SeqCst));
        assert!(sink.settlement.lock().unwrap().is_none());
        operation.wait_settlement().unwrap();

        let owner = sink
            .recovery_owner
            .lock()
            .unwrap()
            .take()
            .expect("retried reaper owner should stay in recovery storage");
        assert_eq!(
            owner.summary(),
            "managed child and I/O helpers awaiting bounded reaper capacity"
        );
        assert!(!owner.is_complete());
        owner.retry_cleanup().expect("cleanup should hand off to reaper");
        cleanup_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(owner.is_complete());
        drop(owner);
        assert!(capacity_released.load(Ordering::SeqCst));
    }

    struct RetrySink {
        rejections_left: AtomicUsize,
        attempts: AtomicUsize,
        retained: Mutex<Option<Box<dyn RecoveryOwner>>>,
        rejection_reason: RecoveryPublishError,
    }

    impl FactSink for RetrySink {
        fn publish_outcome(&self, _: OperationOutcome<CommandOutput, CommandError>) -> Result<(), FactPublishError> {
            Ok(())
        }

        fn confirm_input_release(&self) -> Result<(), FactPublishError> {
            Ok(())
        }

        fn publish_settlement(&self, _: Arc<SettlementReport>) -> Result<(), FactPublishError> {
            Ok(())
        }

        fn retain_recovery_owner(&self, owner: Box<dyn RecoveryOwner>) -> Result<(), RecoveryPublishFailure> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            if self
                .rejections_left
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| remaining.checked_sub(1))
                .is_ok()
            {
                return Err(RecoveryPublishFailure {
                    reason: self.rejection_reason,
                    owner,
                });
            }
            *self.retained.lock().unwrap() = Some(owner);
            Ok(())
        }
    }

    struct TestOwner;
    impl RecoveryOwner for TestOwner {
        fn summary(&self) -> String {
            "transiently rejected owner".into()
        }
        fn effect_evidence(&self, _: OperationId) -> RecoveryEffectEvidence {
            RecoveryEffectEvidence::default()
        }
        fn retry_cleanup(&self) -> Result<(), crate::execution::contracts::RecoveryRetryError> {
            Ok(())
        }
    }

    #[test]
    fn test_recovery_transfer_retries_rejected_owner_until_store_accepts_it() {
        let sink = Arc::new(RetrySink {
            rejections_left: AtomicUsize::new(3),
            attempts: AtomicUsize::new(0),
            retained: Mutex::new(None),
            rejection_reason: RecoveryPublishError::CapacityExceeded,
        });
        let sink_port: Arc<dyn FactSink> = sink.clone();

        assert!(schedule_recovery_transfer(sink_port, Box::new(TestOwner)).is_ok());

        assert_eq!(sink.attempts.load(Ordering::SeqCst), 4);
        let retained = sink
            .retained
            .lock()
            .unwrap()
            .take()
            .expect("accepted owner should remain in store");
        assert_eq!(retained.summary(), "transiently rejected owner");
    }

    #[cfg(unix)]
    #[test]
    fn test_disconnected_recovery_store_returns_owner_for_local_backend_cleanup() {
        let sink = Arc::new(RetrySink {
            rejections_left: AtomicUsize::new(1),
            attempts: AtomicUsize::new(0),
            retained: Mutex::new(None),
            rejection_reason: RecoveryPublishError::StoreUnavailable,
        });
        let sink_port: Arc<dyn FactSink> = sink.clone();
        let owner: Box<dyn RecoveryOwner> = Box::new(LocalFallbackOwner {
            completed: AtomicBool::new(false),
            _lease: Mutex::new(None),
        });
        let owner = schedule_recovery_transfer(sink_port, owner)
            .err()
            .expect("disconnected store must return owner to backend");
        assert_eq!(
            sink.attempts.load(Ordering::SeqCst),
            1,
            "disconnect is not retried as capacity"
        );

        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        let operation_id = operation.id();
        let _ = operation.wait_outcome().unwrap();
        operation.wait_settlement().unwrap();
        let evidence = run_recovery_owner_locally(operation_id, &*owner);
        assert_eq!(evidence.effects_total, 0);
        assert!(
            owner.is_complete(),
            "the backend worker completes returned ownership locally"
        );
        assert!(sink.retained.lock().unwrap().is_none());
    }

    #[test]
    fn test_normalize_tee_paths_resolves_relative_paths_and_keeps_absolute_paths() {
        let absolute_stderr = std::path::PathBuf::from("/logs/stderr.log");
        let options = CommandRunOptions::new()
            .tee_stdout_to_file("stdout.log")
            .tee_stderr_to_file(absolute_stderr.clone());
        let paths = IoPathSnapshot {
            stdin: None,
            stdout: Some(std::path::PathBuf::from("/work/dir/stdout.log")),
            stderr: Some(absolute_stderr.clone()),
        };

        let normalized = normalize_tee_paths(options, &paths);

        assert_eq!(
            normalized.configured_stdout_file(),
            Some(std::path::Path::new("/work/dir/stdout.log"))
        );
        assert_eq!(normalized.configured_stderr_file(), Some(absolute_stderr.as_path()));
    }

    #[test]
    fn test_normalize_tee_paths_preserves_options_without_tee_files() {
        let normalized = normalize_tee_paths(CommandRunOptions::new(), &IoPathSnapshot::default());

        assert_eq!(normalized.configured_stdout_file(), None);
        assert_eq!(normalized.configured_stderr_file(), None);
    }
}
