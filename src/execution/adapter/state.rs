// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;

use qubit_operation::ExecutionFailure;
use qubit_operation::ExternalBindError;
use qubit_operation::OperationId;
use qubit_operation::OperationOutcome;
use qubit_operation::OperationScope;
use qubit_operation::PreparedExternalOperation;
use qubit_operation::SettlementReport;

use super::CommandControl;
use crate::CommandError;
use crate::CommandOutput;
use crate::execution::backend::ProcessBackend;
use crate::execution::contracts::Backend;
use crate::execution::contracts::FactSink;
use crate::execution::contracts::RecoveryOwner;
use crate::execution::contracts::RecoveryPublishFailure;
use crate::execution::contracts::Request;
use crate::execution::contracts::Reservation;
use crate::execution::contracts::ScheduledJob;
use crate::execution::contracts::StopPort;

/// One-shot external operation adapter. Request and reservation remain
/// explicitly recoverable until activation transfers them to the domain
/// scheduler.
pub(crate) struct AdapterState {
    /// Stores the owned value used by this execution component.
    owned: Mutex<OwnedInputs>,
    /// Stores the scheduler value used by this execution component.
    scheduler: Arc<dyn crate::execution::Scheduler>,
    /// Stores the backend value used by this execution component.
    backend: Arc<dyn Backend>,
    /// Stores the process backend value used by this execution component.
    process_backend: Arc<ProcessBackend>,
    /// Stores the control value used by this execution component.
    control: Arc<CommandControl>,
    /// Stores command diagnostics associated with a typed stopped outcome.
    stop_diagnostic: Arc<Mutex<Option<CommandError>>>,
    /// Stores the stop value used by this execution component.
    stop: Mutex<Option<Arc<dyn StopPort>>>,
    /// Stores the recovery value used by this execution component.
    recovery: super::RecoverySender,
    /// Stores the observer value used by this execution component.
    observer: Arc<super::OperationObserverSlot>,
    /// Stores the session bridge value used by this execution component.
    session_bridge: Option<Arc<dyn crate::session::contracts::SessionBridge>>,
    /// Describes the item case handled by managed command execution.
    #[cfg(feature = "async")]
    session_keepalive: Option<SessionOwners>,
}
/// Represents the owned inputs state or contract used by managed command
/// execution.
struct OwnedInputs {
    /// Stores the request value used by this execution component.
    request: Option<Request>,
    /// Stores the reservation value used by this execution component.
    reservation: Option<Reservation>,
    /// Stores the phase value used by this execution component.
    phase: AdapterPhase,
}
/// Represents the adapter phase state or contract used by managed command
/// execution.
#[derive(Clone, Copy, Eq, PartialEq)]
enum AdapterPhase {
    /// Describes the prepared case handled by managed command execution.
    Prepared,
    /// Describes the activated case handled by managed command execution.
    Activated,
    /// Describes the abandoned case handled by managed command execution.
    Abandoned,
    /// Describes the activation failed case handled by managed command
    /// execution.
    ActivationFailed,
}
/// Represents the session owners state or contract used by managed command
/// execution.
#[cfg(feature = "async")]
pub(crate) struct SessionOwners {
    /// Stores the  stdin value used by this execution component.
    pub(crate) _stdin: crate::session::channel::ChannelReader,
    /// Stores the  stdout value used by this execution component.
    pub(crate) _stdout: crate::session::channel::ChannelWriter,
    /// Stores the  stderr value used by this execution component.
    pub(crate) _stderr: crate::session::channel::ChannelWriter,
}
impl AdapterState {
    /// Performs the new step in the managed command execution lifecycle.
    ///
    /// # Parameters
    /// * `request`: Supplies the request value used by this operation.
    /// * `reservation`: Supplies the reservation value used by this operation.
    /// * `scheduler`: Supplies the scheduler value used by this operation.
    /// * `backend`: Supplies the backend value used by this operation.
    /// * `process_backend`: Supplies the process backend value used by this
    ///   operation.
    /// * `recovery`: Supplies the recovery value used by this operation.
    /// * `session_bridge`: Supplies the session bridge value used by this
    ///   operation.
    pub(crate) fn new(
        request: Request,
        reservation: Reservation,
        scheduler: Arc<dyn crate::execution::Scheduler>,
        backend: Arc<dyn Backend>,
        process_backend: Arc<ProcessBackend>,
        recovery: super::RecoverySender,
        session_bridge: Option<Arc<dyn crate::session::contracts::SessionBridge>>,
    ) -> Self {
        Self {
            owned: Mutex::new(OwnedInputs {
                request: Some(request),
                reservation: Some(reservation),
                phase: AdapterPhase::Prepared,
            }),
            scheduler,
            backend,
            process_backend,
            control: Arc::new(CommandControl::default()),
            stop_diagnostic: Arc::new(Mutex::new(None)),
            stop: Mutex::new(None),
            recovery,
            observer: Arc::new(super::OperationObserverSlot::default()),
            session_bridge,
            #[cfg(feature = "async")]
            session_keepalive: None,
        }
    }
    /// Stores the keep session alive value used by this execution component.
    ///
    /// # Parameters
    /// * `owners`: Supplies the owners value used by this operation.
    #[cfg(feature = "async")]
    pub(crate) fn keep_session_alive(mut self, owners: SessionOwners) -> Self {
        self.session_keepalive = Some(owners);
        self
    }
    /// Performs the restore step in the managed command execution lifecycle.
    pub(crate) fn restore(&self) -> Option<Request> {
        self.owned.lock().unwrap_or_else(|e| e.into_inner()).request.take()
    }
    /// Shares the diagnostics retained for a stopped command outcome.
    pub(crate) fn stop_diagnostic_slot(&self) -> Arc<Mutex<Option<CommandError>>> {
        Arc::clone(&self.stop_diagnostic)
    }
    /// Stores the set operation observer value used by this execution
    /// component.
    ///
    /// # Parameters
    /// * `observer`: Supplies the observer value used by this operation.
    pub(crate) fn set_operation_observer(&self, observer: qubit_operation::OperationObserver) {
        self.observer.set(observer);
    }
}
impl FactSink for AdapterState {
    /// Retains the command error that explains a typed stopped outcome.
    ///
    /// # Parameters
    /// * `error`: Runner error and retained command diagnostics.
    fn publish_stop_diagnostic(&self, error: CommandError) -> Result<(), CommandError> {
        let mut diagnostic = self.stop_diagnostic.lock().unwrap_or_else(|error| error.into_inner());
        if diagnostic.is_some() {
            return Err(error);
        }
        *diagnostic = Some(error);
        Ok(())
    }
    /// Performs the publish outcome step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `outcome`: Supplies the outcome value used by this operation.
    fn publish_outcome(
        &self,
        outcome: OperationOutcome<CommandOutput, CommandError>,
    ) -> Result<(), crate::execution::contracts::FactPublishError> {
        if self.control.publish_outcome(outcome) {
            Ok(())
        } else {
            Err(crate::execution::contracts::FactPublishError::DuplicateOutcome)
        }
    }
    /// Stores the confirm input release value used by this execution component.
    fn confirm_input_release(&self) -> Result<(), crate::execution::contracts::FactPublishError> {
        if self.control.release() {
            Ok(())
        } else {
            Err(crate::execution::contracts::FactPublishError::DuplicateRelease)
        }
    }
    /// Performs the publish settlement step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `report`: Supplies the report value used by this operation.
    fn publish_settlement(
        &self,
        report: Arc<SettlementReport>,
    ) -> Result<(), crate::execution::contracts::FactPublishError> {
        let identity = report.operation_id;
        if self.control.settlement(report) {
            Ok(())
        } else {
            if self.control.operation_id().is_some_and(|id| id != identity) {
                Err(crate::execution::contracts::FactPublishError::IdentityMismatch)
            } else {
                Err(crate::execution::contracts::FactPublishError::DuplicateSettlement)
            }
        }
    }
    /// Performs the retain recovery owner step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `owner`: Supplies the owner value used by this operation.
    fn retain_recovery_owner(&self, owner: Box<dyn RecoveryOwner>) -> Result<(), RecoveryPublishFailure> {
        let Some(id) = self.control.operation_id() else {
            return Err(RecoveryPublishFailure {
                reason: crate::execution::contracts::RecoveryPublishError::StoreUnavailable,
                owner,
            });
        };
        let item = super::RecoveryItem {
            owner,
            control: Arc::clone(&self.control),
            id,
            observer: Arc::clone(&self.observer),
        };
        self.recovery.try_send(item).map_err(|error| {
            let owner = match error {
                std::sync::mpsc::TrySendError::Full(item) | std::sync::mpsc::TrySendError::Disconnected(item) => {
                    item.owner
                }
            };
            RecoveryPublishFailure {
                reason: crate::execution::contracts::RecoveryPublishError::CapacityExceeded,
                owner,
            }
        })
    }
}
impl PreparedExternalOperation<CommandOutput, CommandError> for AdapterState {
    /// Stores the bind value used by this execution component.
    ///
    /// # Parameters
    /// * `id`: Supplies the id value used by this operation.
    fn bind(&self, id: OperationId) -> Result<(), ExternalBindError> {
        if self.control.bind(id) {
            Ok(())
        } else {
            Err(ExternalBindError::AlreadyBound)
        }
    }
    /// Stores the activate value used by this execution component.
    ///
    /// # Parameters
    /// * `self`: Supplies the self value used by this operation.
    /// * `scope`: Supplies the scope value used by this operation.
    fn activate(self: Arc<Self>, scope: OperationScope) -> Result<(), ExecutionFailure> {
        let operation_id = scope.id();
        let make_failure = |message: &str| ExecutionFailure {
            operation_id,
            summary: qubit_operation::DiagnosticSummary::new(message),
            source: None,
        };
        let permit = {
            let mut owned = self.owned.lock().unwrap_or_else(|e| e.into_inner());
            if owned.phase != AdapterPhase::Prepared || owned.request.is_none() || owned.reservation.is_none() {
                owned.phase = AdapterPhase::ActivationFailed;
                None
            } else {
                owned.phase = AdapterPhase::Activated;
                let request = owned.request.as_mut().expect("checked request");
                request.operation_id = Some(operation_id);
                request.deadline = Some(scope.context());
                owned.reservation.take()
            }
        };
        let Some(permit) = permit else {
            let discarded = {
                let mut owned = self.owned.lock().unwrap_or_else(|e| e.into_inner());
                (owned.request.take(), owned.reservation.take())
            };
            drop(discarded);
            let failure = make_failure("prepared command inputs were unavailable at activation");
            let _ = self
                .control
                .publish_outcome(OperationOutcome::ExecutionFailed(failure.clone()));
            let _ = self.control.release();
            let _ = self.control.settlement(Arc::new(released_report(operation_id)));
            return Err(failure);
        };
        self.control.running();
        let stop = self.process_backend.stop_port(operation_id);
        *self.stop.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::clone(&stop) as Arc<dyn StopPort>);
        let state = Arc::clone(&self);
        let job = ScheduledJob {
            stop,
            run: Box::new(move |permit| {
                let request = state.owned.lock().unwrap_or_else(|e| e.into_inner()).request.take();
                let Some(request) = request else {
                    return;
                };
                state.backend.run(
                    crate::execution::ActivatedRequest {
                        operation_id,
                        scope,
                        request,
                        reservation: permit,
                        facts: state.clone(),
                        session_bridge: state.session_bridge.clone(),
                    },
                    state.clone(),
                );
            }),
        };
        if let Some(reason) = qubit_operation::OperationControl::snapshot(&*self.control)
            .stop
            .map(|x| x.reason)
            && let Some(stop) = self.stop.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
        {
            stop.request_stop(reason)
        }
        match self.scheduler.enqueue(job, permit) {
            Ok(()) => Ok(()),
            Err(failure) => {
                let crate::execution::contracts::EnqueueFailure { reason, job, permit } = failure;
                drop(permit);
                let discarded = {
                    let mut owned = self.owned.lock().unwrap_or_else(|e| e.into_inner());
                    owned.phase = AdapterPhase::ActivationFailed;
                    (owned.request.take(), owned.reservation.take())
                };
                drop(job);
                drop(discarded);
                let failure = make_failure("domain scheduler rejected activation");
                let _ = self
                    .control
                    .publish_outcome(OperationOutcome::ExecutionFailed(failure.clone()));
                let _ = self.control.release();
                let _ = self.control.settlement(Arc::new(released_report(operation_id)));
                let _ = reason;
                Err(failure)
            }
        }
    }
    /// Stores the abandon value used by this execution component.
    ///
    /// # Parameters
    /// * `self`: Supplies the self value used by this operation.
    fn abandon(self: Arc<Self>) {
        let abandoned = {
            let mut owned = self.owned.lock().unwrap_or_else(|e| e.into_inner());
            if owned.phase == AdapterPhase::Prepared {
                owned.phase = AdapterPhase::Abandoned;
                owned.request.take();
                owned.reservation.take();
                true
            } else {
                false
            }
        };
        if !abandoned {
            return;
        }
        if let Some(id) = self.control.operation_id() {
            let _ = self.control.release();
            let _ = self.control.settlement(Arc::new(released_report(id)));
        }
    }
    /// Performs the poll take outcome step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `cx`: Supplies the cx value used by this operation.
    fn poll_take_outcome(&self, cx: &mut Context<'_>) -> Poll<OperationOutcome<CommandOutput, CommandError>> {
        self.control.take(cx)
    }
}
/// Stores the released report value used by this execution component.
///
/// # Parameters
/// * `id`: Supplies the id value used by this operation.
fn released_report(id: OperationId) -> SettlementReport {
    SettlementReport {
        operation_id: id,
        status: qubit_operation::SettlementStatus::Released,
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
impl qubit_operation::OperationControl for AdapterState {
    /// Stores the request stop value used by this execution component.
    ///
    /// # Parameters
    /// * `reason`: Supplies the reason value used by this operation.
    fn request_stop(&self, reason: qubit_operation::StopReason) {
        self.control.request_stop(reason);
        if let Some(stop) = self.stop.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            stop.request_stop(reason)
        }
    }
    /// Stores the snapshot value used by this execution component.
    fn snapshot(&self) -> qubit_operation::ChildSnapshot {
        qubit_operation::OperationControl::snapshot(&*self.control)
    }
    /// Stores the poll outcome value used by this execution component.
    ///
    /// # Parameters
    /// * `cx`: Supplies the cx value used by this operation.
    fn poll_outcome(&self, cx: &mut Context<'_>) -> Poll<qubit_operation::ChildOutcomeSummary> {
        qubit_operation::OperationControl::poll_outcome(&*self.control, cx)
    }
    /// Stores the poll release value used by this execution component.
    ///
    /// # Parameters
    /// * `cx`: Supplies the cx value used by this operation.
    fn poll_release(&self, cx: &mut Context<'_>) -> Poll<qubit_operation::ChildRelease> {
        qubit_operation::OperationControl::poll_release(&*self.control, cx)
    }
    /// Stores the poll settlement value used by this execution component.
    ///
    /// # Parameters
    /// * `cx`: Supplies the cx value used by this operation.
    fn poll_settlement(&self, cx: &mut Context<'_>) -> Poll<Arc<SettlementReport>> {
        qubit_operation::OperationControl::poll_settlement(&*self.control, cx)
    }
}

#[cfg(test)]
mod activation_failure_tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Context;
    use std::task::Poll;

    use qubit_operation::ExecutionFailure;
    use qubit_operation::ExternalBindError;
    use qubit_operation::OperationControl;
    use qubit_operation::OperationId;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;
    use qubit_operation::OperationScope;
    use qubit_operation::PreparedExternalOperation;

    use super::AdapterState;
    use super::ProcessBackend;
    use crate::Command;
    use crate::CommandError;
    use crate::CommandErrorKind;
    use crate::CommandErrorReason;
    use crate::CommandExecutionConfig;
    use crate::CommandOutput;
    use crate::CommandRunner;
    use crate::execution::adapter::command_control::CommandControl;
    use crate::execution::contracts::ExecutionSnapshot;
    use crate::execution::contracts::IoPathSnapshot;
    use crate::execution::contracts::RecoveryOwner;
    use crate::execution::contracts::Request;
    use crate::execution::contracts::RequestOptions;
    use crate::execution::contracts::Scheduler;

    struct InlineScheduler;

    impl Scheduler for InlineScheduler {
        fn try_reserve(
            &self,
            _input_bytes: usize,
        ) -> Result<crate::execution::Reservation, crate::execution::contracts::CapacityFailure> {
            Ok(crate::execution::Reservation::new(
                7,
                11,
                |_| {},
                |_| crate::execution::contracts::ReservationLease::new(|| {}),
            ))
        }

        fn enqueue(
            &self,
            job: crate::execution::contracts::ScheduledJob,
            permit: crate::execution::Reservation,
        ) -> Result<(), crate::execution::contracts::EnqueueFailure> {
            (job.run)(permit);
            Ok(())
        }

        fn wait_next(&self) -> Option<(crate::execution::contracts::ScheduledJob, crate::execution::Reservation)> {
            None
        }

        fn close_admission(&self) {}
    }

    struct ReleaseTrackingInlineScheduler(Arc<AtomicUsize>);

    impl Scheduler for ReleaseTrackingInlineScheduler {
        fn try_reserve(
            &self,
            _input_bytes: usize,
        ) -> Result<crate::execution::Reservation, crate::execution::contracts::CapacityFailure> {
            let releases = Arc::clone(&self.0);
            Ok(crate::execution::Reservation::new(
                17,
                29,
                move |_| {
                    releases.fetch_add(1, Ordering::SeqCst);
                },
                |_| crate::execution::contracts::ReservationLease::new(|| {}),
            ))
        }

        fn enqueue(
            &self,
            job: crate::execution::contracts::ScheduledJob,
            permit: crate::execution::Reservation,
        ) -> Result<(), crate::execution::contracts::EnqueueFailure> {
            (job.run)(permit);
            Ok(())
        }

        fn wait_next(&self) -> Option<(crate::execution::contracts::ScheduledJob, crate::execution::Reservation)> {
            None
        }

        fn close_admission(&self) {}
    }

    struct ExecutingBackend;

    impl crate::execution::contracts::Backend for ExecutingBackend {
        fn run(
            &self,
            activated: crate::execution::ActivatedRequest,
            sink: Arc<dyn crate::execution::contracts::FactSink>,
        ) {
            assert_eq!(activated.request.operation_id, Some(activated.operation_id));
            assert!(activated.request.deadline.is_some());
            let output = activated.request.runner.run(activated.request.command).unwrap();
            sink.publish_outcome(OperationOutcome::Succeeded(output)).unwrap();
            sink.confirm_input_release().unwrap();
            sink.publish_settlement(Arc::new(super::released_report(activated.operation_id)))
                .unwrap();
        }
    }

    struct DropObservedRecoveryOwner(Arc<AtomicBool>);

    impl Drop for DropObservedRecoveryOwner {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    impl RecoveryOwner for DropObservedRecoveryOwner {
        fn summary(&self) -> String {
            "test recovery owner".to_owned()
        }

        fn retry_cleanup(&self) -> Result<(), crate::execution::contracts::RecoveryRetryError> {
            Ok(())
        }
    }

    struct CompleteRecoveryOwner(Arc<AtomicBool>);

    impl RecoveryOwner for CompleteRecoveryOwner {
        fn summary(&self) -> String {
            "completed test recovery owner".to_owned()
        }

        fn retry_cleanup(&self) -> Result<(), crate::execution::contracts::RecoveryRetryError> {
            self.0.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn is_complete(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }
    }
    use crate::execution::scheduler::BoundedScheduler;

    struct FailedBinder(AtomicBool, CommandControl);
    impl OperationControl for FailedBinder {
        fn request_stop(&self, reason: qubit_operation::StopReason) {
            OperationControl::request_stop(&self.1, reason)
        }
        fn snapshot(&self) -> qubit_operation::ChildSnapshot {
            OperationControl::snapshot(&self.1)
        }
        fn poll_outcome(&self, cx: &mut Context<'_>) -> Poll<qubit_operation::ChildOutcomeSummary> {
            OperationControl::poll_outcome(&self.1, cx)
        }
        fn poll_release(&self, cx: &mut Context<'_>) -> Poll<qubit_operation::ChildRelease> {
            OperationControl::poll_release(&self.1, cx)
        }
        fn poll_settlement(&self, cx: &mut Context<'_>) -> Poll<Arc<qubit_operation::SettlementReport>> {
            OperationControl::poll_settlement(&self.1, cx)
        }
    }
    impl PreparedExternalOperation<CommandOutput, CommandError> for FailedBinder {
        fn bind(&self, id: OperationId) -> Result<(), ExternalBindError> {
            Err(ExternalBindError::Failed(ExecutionFailure {
                operation_id: id,
                summary: qubit_operation::DiagnosticSummary::new("injected bind failure"),
                source: None,
            }))
        }
        fn activate(self: Arc<Self>, _: OperationScope) -> Result<(), ExecutionFailure> {
            panic!("failed bind must not activate")
        }
        fn abandon(self: Arc<Self>) {
            self.0.store(true, Ordering::SeqCst);
        }
        fn poll_take_outcome(&self, _: &mut Context<'_>) -> Poll<OperationOutcome<CommandOutput, CommandError>> {
            Poll::Pending
        }
    }

    fn request() -> Request {
        Request {
            operation_id: None,
            runner: CommandRunner::without_timeout(),
            command: Command::new("rustc").arg("--version"),
            options: RequestOptions::Standard(Default::default()),
            snapshot: ExecutionSnapshot {
                working_directory: std::env::current_dir().unwrap(),
                environment: Vec::new(),
            },
            io_paths: IoPathSnapshot::default(),
            deadline: None,
        }
    }

    fn make_state(scheduler: Arc<BoundedScheduler>, permit: crate::execution::Reservation) -> Arc<AdapterState> {
        let backend = Arc::new(ProcessBackend::new());
        let recovery = super::super::cleanup::start_recovery_worker(1).unwrap();
        Arc::new(AdapterState::new(
            request(),
            permit,
            scheduler.clone(),
            backend.clone(),
            backend,
            recovery,
            None,
        ))
    }

    fn drive_activation(state: Arc<AdapterState>) {
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut handle = manager
            .try_submit(OperationOptions::default(), move |scope| {
                state.bind(scope.id()).unwrap();
                let _ = state.clone().activate(scope);
                Ok::<_, ()>(())
            })
            .unwrap();
        assert!(matches!(
            handle.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        handle.wait_settlement().unwrap();
    }

    fn scheduler() -> Arc<BoundedScheduler> {
        Arc::new(
            BoundedScheduler::new(CommandExecutionConfig {
                max_active: 1,
                max_queued: 0,
                ..Default::default()
            })
            .unwrap(),
        )
    }

    #[test]
    fn test_adapter_bind_is_one_shot_and_reports_already_bound() {
        let scheduler = scheduler();
        let permit = scheduler.try_reserve(0).unwrap();
        let state = make_state(scheduler, permit);
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut handle = manager
            .try_submit(OperationOptions::default(), move |scope| {
                assert!(state.bind(scope.id()).is_ok());
                assert!(matches!(state.bind(scope.id()), Err(ExternalBindError::AlreadyBound)));
                Ok::<_, ()>(())
            })
            .unwrap();
        assert!(matches!(
            handle.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        handle.wait_settlement().unwrap();
    }

    #[test]
    fn test_stop_requested_before_adapter_binding_is_retained() {
        let scheduler = scheduler();
        let permit = scheduler.try_reserve(0).unwrap();
        let scheduler_snapshot = Arc::clone(&scheduler);
        let state = make_state(scheduler, permit);
        let state_for_operation = Arc::clone(&state);
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut handle = manager
            .try_submit(OperationOptions::default(), move |scope| {
                OperationControl::request_stop(&*state_for_operation, qubit_operation::StopReason::Timeout);
                state_for_operation.bind(scope.id()).unwrap();

                let snapshot = OperationControl::snapshot(&*state_for_operation);
                assert_eq!(snapshot.operation_id, scope.id());
                assert_eq!(snapshot.stop.unwrap().reason, qubit_operation::StopReason::Timeout);
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            handle.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        handle.wait_settlement().unwrap();
        assert!(
            state.restore().is_some(),
            "a stop request alone must not consume prepared input"
        );
        assert_eq!(
            scheduler_snapshot.snapshot().active,
            1,
            "restoring the prepared request must leave its reservation with the adapter"
        );
        drop(state);
        assert_eq!(
            scheduler_snapshot.snapshot().active,
            0,
            "dropping the adapter releases its still-owned reservation"
        );
    }

    #[test]
    fn test_stop_before_activation_reaches_backend_and_releases_reservation() {
        struct ReleaseTrackingScheduler(Arc<AtomicBool>);

        impl Scheduler for ReleaseTrackingScheduler {
            fn try_reserve(
                &self,
                _input_bytes: usize,
            ) -> Result<crate::execution::Reservation, crate::execution::contracts::CapacityFailure> {
                let released = Arc::clone(&self.0);
                Ok(crate::execution::Reservation::new(
                    23,
                    41,
                    move |token| {
                        assert_eq!(token, 41);
                        released.store(true, Ordering::SeqCst);
                    },
                    |_| crate::execution::contracts::ReservationLease::new(|| {}),
                ))
            }

            fn enqueue(
                &self,
                job: crate::execution::contracts::ScheduledJob,
                permit: crate::execution::Reservation,
            ) -> Result<(), crate::execution::contracts::EnqueueFailure> {
                (job.run)(permit);
                Ok(())
            }

            fn wait_next(&self) -> Option<(crate::execution::contracts::ScheduledJob, crate::execution::Reservation)> {
                None
            }

            fn close_admission(&self) {}
        }

        let released = Arc::new(AtomicBool::new(false));
        let scheduler: Arc<dyn Scheduler> = Arc::new(ReleaseTrackingScheduler(Arc::clone(&released)));
        let permit = scheduler.try_reserve(0).unwrap();
        let process_backend = Arc::new(ProcessBackend::new());
        let mut request = request();
        request.command = Command::new("true");
        let state = Arc::new(AdapterState::new(
            request,
            permit,
            scheduler,
            process_backend.clone(),
            process_backend,
            super::super::cleanup::start_recovery_worker(1).unwrap(),
            None,
        ));
        let state_for_operation = Arc::clone(&state);
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut handle = manager
            .try_submit(OperationOptions::default(), move |scope| {
                state_for_operation.bind(scope.id()).unwrap();
                OperationControl::request_stop(&*state_for_operation, qubit_operation::StopReason::Timeout);
                state_for_operation.clone().activate(scope).unwrap();
                Ok::<_, ()>(())
            })
            .unwrap();
        let operation_id = handle.id();
        assert!(matches!(
            handle.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        handle.wait_settlement().unwrap();

        let snapshot = OperationControl::snapshot(&*state);
        assert_eq!(snapshot.operation_id, operation_id);
        assert_eq!(snapshot.stop.unwrap().reason, qubit_operation::StopReason::Timeout);
        assert_eq!(
            snapshot.outcome.expect("pre-start stop should publish an outcome").kind,
            qubit_operation::ChildOutcomeKind::Stopped
        );
        assert!(snapshot.input_released, "pre-start stop must release operation inputs");
        let settlement = snapshot.settlement.expect("pre-start stop should settle");
        assert_eq!(settlement.operation_id, operation_id);
        assert_eq!(settlement.status, qubit_operation::SettlementStatus::Released);
        assert!(
            released.load(Ordering::SeqCst),
            "the scheduler reservation must be released"
        );
        assert!(
            state.restore().is_none(),
            "activation must transfer the request out of the adapter"
        );
    }

    #[test]
    fn test_activation_enqueues_and_runs_bound_request_before_settlement() {
        let scheduler = Arc::new(InlineScheduler);
        let permit = scheduler.try_reserve(0).unwrap();
        let process_backend = Arc::new(ProcessBackend::new());
        let state = Arc::new(AdapterState::new(
            request(),
            permit,
            scheduler,
            Arc::new(ExecutingBackend),
            process_backend,
            super::super::cleanup::start_recovery_worker(1).unwrap(),
            None,
        ));
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut handle = manager
            .try_submit_external(OperationOptions::default(), Arc::clone(&state))
            .unwrap();

        assert!(matches!(handle.wait_outcome().unwrap(), OperationOutcome::Succeeded(_)));
        let report = handle.wait_settlement().unwrap();
        assert_eq!(report.operation_id, handle.id());
        assert_eq!(report.status, qubit_operation::SettlementStatus::Released);
        assert!(OperationControl::snapshot(&*state).input_released);
        assert!(
            state.restore().is_none(),
            "activation transfers the request to the backend"
        );

        OperationControl::request_stop(&*state, qubit_operation::StopReason::UserCancelled);
        assert_eq!(
            OperationControl::snapshot(&*state).stop.unwrap().reason,
            qubit_operation::StopReason::UserCancelled,
            "a stop request made after activation is retained in the operation snapshot"
        );
    }

    #[test]
    fn test_accepted_activation_failure_settles_and_cannot_bind_in_another_manager() {
        let scheduler = scheduler();
        let permit = scheduler.try_reserve(0).unwrap();
        let prepared = make_state(Arc::clone(&scheduler), permit);
        scheduler.close_admission();

        let first_manager = OperationManager::new(Default::default()).unwrap();
        let mut handle = first_manager
            .try_submit_external(OperationOptions::default(), Arc::clone(&prepared))
            .unwrap();
        let outcome = handle.wait_outcome().unwrap();
        assert!(matches!(outcome, OperationOutcome::ExecutionFailed(_)));
        let report = handle.wait_settlement().unwrap();
        assert_eq!(report.status, qubit_operation::SettlementStatus::Released);
        assert_eq!(report.recovery_count, 0);

        let second_manager = OperationManager::new(Default::default()).unwrap();
        match second_manager.try_submit_external(OperationOptions::default(), Arc::clone(&prepared)) {
            Err(error) => assert_eq!(error.reason(), qubit_operation::AdmissionError::AlreadyBound),
            Ok(_) => panic!("a prepared adapter must not bind through another manager"),
        }
    }

    #[test]
    fn test_bind_failure_becomes_execution_failure_without_activation() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let prepared = Arc::new(FailedBinder(AtomicBool::new(false), CommandControl::default()));
        let mut handle = manager
            .try_submit_external(OperationOptions::default(), Arc::clone(&prepared))
            .unwrap();

        assert!(matches!(
            handle.wait_outcome().unwrap(),
            OperationOutcome::ExecutionFailed(_)
        ));
        assert!(prepared.0.load(Ordering::SeqCst));
    }

    #[test]
    fn test_activation_without_permit_drops_request_before_release() {
        let scheduler = scheduler();
        let permit = scheduler.try_reserve(0).unwrap();
        let state = make_state(Arc::clone(&scheduler), permit);
        state.owned.lock().unwrap().reservation.take();

        drive_activation(Arc::clone(&state));

        assert!(state.restore().is_none(), "request must not survive published release");
        assert!(OperationControl::snapshot(&*state).input_released);
    }

    #[test]
    fn test_activation_without_request_publishes_failure_and_preserves_recovered_request() {
        let scheduler = scheduler();
        let permit = scheduler.try_reserve(0).unwrap();
        let state = make_state(Arc::clone(&scheduler), permit);
        let recovered = state.restore().expect("prepared request should be recoverable");

        drive_activation(Arc::clone(&state));

        assert!(recovered.operation_id.is_none());
        let snapshot = OperationControl::snapshot(&*state);
        assert_eq!(snapshot.execution, qubit_operation::ExecutionPhase::Finished);
        assert_eq!(
            snapshot.outcome.expect("missing request is an execution failure").kind,
            qubit_operation::ChildOutcomeKind::ExecutionFailed
        );
        assert!(snapshot.input_released);
        assert!(snapshot.settlement.is_some());
        assert!(
            scheduler.try_reserve(0).is_ok(),
            "failed activation must return its reservation"
        );
    }

    #[test]
    fn test_enqueue_rejection_drops_request_before_release() {
        let scheduler = scheduler();
        let permit = scheduler.try_reserve(0).unwrap();
        let state = make_state(Arc::clone(&scheduler), permit);
        scheduler.close_admission();

        drive_activation(Arc::clone(&state));

        assert!(state.restore().is_none(), "request must not survive published release");
        assert!(OperationControl::snapshot(&*state).input_released);
    }

    #[test]
    fn test_abandon_prepared_adapter_releases_inputs_and_scheduler_capacity() {
        let scheduler = scheduler();
        let permit = scheduler.try_reserve(0).unwrap();
        let state = make_state(Arc::clone(&scheduler), permit);
        let state_for_operation = Arc::clone(&state);
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut handle = manager
            .try_submit(OperationOptions::default(), move |scope| {
                state_for_operation.bind(scope.id()).unwrap();
                state_for_operation.clone().abandon();
                state_for_operation.clone().abandon();
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            handle.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        handle.wait_settlement().unwrap();
        assert!(state.restore().is_none(), "abandoned request must not be restorable");
        let snapshot = OperationControl::snapshot(&*state);
        assert!(snapshot.input_released);
        assert_eq!(snapshot.execution, qubit_operation::ExecutionPhase::Queued);
        assert!(snapshot.settlement.is_some());
        assert!(
            scheduler.try_reserve(0).is_ok(),
            "abandon should return the reservation"
        );
    }

    #[test]
    fn test_unbound_abandon_recovers_poisoned_inputs_and_rejects_later_activation() {
        let scheduler = scheduler();
        let permit = scheduler.try_reserve(0).unwrap();
        let state = make_state(Arc::clone(&scheduler), permit);
        let poisoned_state = Arc::clone(&state);
        let poisoner = std::thread::spawn(move || {
            let _owned = poisoned_state.owned.lock().unwrap();
            panic!("poison adapter inputs before unbound abandonment");
        });
        assert!(poisoner.join().is_err());

        Arc::clone(&state).abandon();
        assert!(
            state.restore().is_none(),
            "abandonment permanently consumes prepared input"
        );
        assert_eq!(
            scheduler.snapshot().active,
            0,
            "unbound abandonment releases its reservation"
        );

        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit_external(OperationOptions::default(), Arc::clone(&state))
            .unwrap();
        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::ExecutionFailed(_)
        ));
        let settlement = operation.wait_settlement().unwrap();
        assert_eq!(settlement.operation_id, operation.id());
        assert_eq!(settlement.status, qubit_operation::SettlementStatus::Released);
        assert!(OperationControl::snapshot(&*state).input_released);
        assert_eq!(scheduler.snapshot().active, 0);
    }

    #[test]
    fn test_activation_recovers_poisoned_inputs_and_runs_prepared_request() {
        let scheduler: Arc<dyn Scheduler> = Arc::new(InlineScheduler);
        let permit = scheduler.try_reserve(0).unwrap();
        let process_backend = Arc::new(ProcessBackend::new());
        let mut command_request = request();
        command_request.command = Command::new("true");
        let state = Arc::new(AdapterState::new(
            command_request,
            permit,
            scheduler,
            Arc::new(ExecutingBackend),
            process_backend.clone(),
            super::super::cleanup::start_recovery_worker(1).unwrap(),
            None,
        ));
        let poisoned_state = Arc::clone(&state);
        let poisoner = std::thread::spawn(move || {
            let _owned = poisoned_state.owned.lock().unwrap();
            panic!("poison prepared inputs before activation");
        });
        assert!(poisoner.join().is_err());

        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit_external(OperationOptions::default(), Arc::clone(&state))
            .unwrap();
        let operation_id = operation.id();
        let output = match operation.wait_outcome().unwrap() {
            OperationOutcome::Succeeded(output) => output,
            other => panic!("activation should recover and execute prepared input: {other:?}"),
        };
        assert!(output.exit_status().success());
        let settlement = operation.wait_settlement().unwrap();
        assert_eq!(settlement.operation_id, operation_id);
        assert_eq!(settlement.status, qubit_operation::SettlementStatus::Released);
        assert!(state.restore().is_none(), "activation transfers the request to the backend");
    }

    #[test]
    fn test_abandon_after_activation_does_not_release_resources_twice() {
        let releases = Arc::new(AtomicUsize::new(0));
        let scheduler = Arc::new(ReleaseTrackingInlineScheduler(Arc::clone(&releases)));
        let permit = scheduler.try_reserve(0).unwrap();
        let process_backend = Arc::new(ProcessBackend::new());
        let mut command_request = request();
        command_request.command = Command::new("true");
        let state = Arc::new(AdapterState::new(
            command_request,
            permit,
            scheduler,
            Arc::new(ExecutingBackend),
            process_backend,
            super::super::cleanup::start_recovery_worker(1).unwrap(),
            None,
        ));
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut handle = manager
            .try_submit_external(OperationOptions::default(), Arc::clone(&state))
            .unwrap();

        assert!(matches!(handle.wait_outcome().unwrap(), OperationOutcome::Succeeded(_)));
        let report = handle.wait_settlement().unwrap();
        assert_eq!(report.status, qubit_operation::SettlementStatus::Released);
        assert_eq!(releases.load(Ordering::SeqCst), 1);

        state.clone().abandon();

        let snapshot = OperationControl::snapshot(&*state);
        assert_eq!(snapshot.execution, qubit_operation::ExecutionPhase::Finished);
        assert_eq!(
            snapshot.outcome.expect("completed activation keeps its outcome").kind,
            qubit_operation::ChildOutcomeKind::Succeeded
        );
        assert_eq!(
            snapshot
                .settlement
                .expect("completed activation keeps its settlement")
                .status,
            qubit_operation::SettlementStatus::Released
        );
        assert_eq!(
            releases.load(Ordering::SeqCst),
            1,
            "abandoning activated state must not release twice"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_restore_returns_unactivated_request_for_retry() {
        let scheduler = scheduler();
        let permit = scheduler.try_reserve(0).unwrap();
        let state = make_state(Arc::clone(&scheduler), permit);

        let request = state.restore().expect("prepared request should be recoverable");
        assert!(request.operation_id.is_none());
        let output = request.runner.run(request.command).unwrap();
        assert!(output.exit_status().success());

        drop(state);
        assert!(
            scheduler.try_reserve(0).is_ok(),
            "dropping restored state should release its reservation"
        );
    }

    #[test]
    fn test_fact_sink_rejects_duplicate_and_mismatched_publications() {
        let scheduler = scheduler();
        let permit = scheduler.try_reserve(0).unwrap();
        let state = make_state(scheduler, permit);
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut first_handle = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        let mut other_handle = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        let id = first_handle.id();
        let other_id = other_handle.id();
        let _ = first_handle.wait_outcome().unwrap();
        first_handle.wait_settlement().unwrap();
        let _ = other_handle.wait_outcome().unwrap();
        other_handle.wait_settlement().unwrap();

        state.bind(id).unwrap();
        let stopped = || {
            OperationOutcome::Stopped(qubit_operation::StopReport {
                reason: qubit_operation::StopReason::UserCancelled,
                accepted_at: None,
            })
        };
        assert_eq!(
            crate::execution::contracts::FactSink::publish_outcome(&*state, stopped()),
            Ok(())
        );
        assert_eq!(
            crate::execution::contracts::FactSink::publish_outcome(&*state, stopped()),
            Err(crate::execution::contracts::FactPublishError::DuplicateOutcome)
        );
        assert_eq!(
            crate::execution::contracts::FactSink::confirm_input_release(&*state),
            Ok(())
        );
        assert_eq!(
            crate::execution::contracts::FactSink::confirm_input_release(&*state),
            Err(crate::execution::contracts::FactPublishError::DuplicateRelease)
        );
        assert_eq!(
            crate::execution::contracts::FactSink::publish_settlement(
                &*state,
                Arc::new(super::released_report(other_id)),
            ),
            Err(crate::execution::contracts::FactPublishError::IdentityMismatch)
        );
        let report = Arc::new(super::released_report(id));
        assert_eq!(
            crate::execution::contracts::FactSink::publish_settlement(&*state, Arc::clone(&report)),
            Ok(())
        );
        assert_eq!(
            crate::execution::contracts::FactSink::publish_settlement(&*state, report),
            Err(crate::execution::contracts::FactPublishError::DuplicateSettlement)
        );
        assert_eq!(OperationControl::snapshot(&*state).operation_id, id);
    }

    #[test]
    fn test_stop_diagnostic_retains_first_error_and_returns_duplicate() {
        let scheduler = scheduler();
        let permit = scheduler.try_reserve(0).unwrap();
        let state = make_state(scheduler, permit);
        let first = CommandError::from_reason(
            "first stopped diagnostic",
            CommandErrorReason::WaitFailed {
                source: std::io::Error::other("primary wait failure"),
            },
            None,
        );

        assert!(matches!(
            crate::execution::contracts::FactSink::publish_stop_diagnostic(&*state, first),
            Ok(())
        ));
        let duplicate = crate::execution::contracts::FactSink::publish_stop_diagnostic(
            &*state,
            CommandError::from_reason("duplicate diagnostic", CommandErrorReason::Cancelled, None),
        )
        .expect_err("a stopped command may retain only its first diagnostic");
        assert_eq!(duplicate.kind(), CommandErrorKind::Cancelled);

        let retained = state
            .stop_diagnostic_slot()
            .lock()
            .unwrap()
            .take()
            .expect("first diagnostic should remain available to the operation wrapper");
        assert_eq!(retained.kind(), CommandErrorKind::WaitFailed);
        assert!(matches!(
            retained.reason(),
            CommandErrorReason::WaitFailed { source }
                if source.to_string() == "primary wait failure"
        ));
    }

    #[test]
    fn test_stop_diagnostic_recovers_poisoned_slot_and_retains_error() {
        let scheduler = scheduler();
        let permit = scheduler.try_reserve(0).unwrap();
        let state = make_state(scheduler, permit);
        let slot = state.stop_diagnostic_slot();
        let poison_slot = Arc::clone(&slot);
        let poisoner = std::thread::spawn(move || {
            let _guard = poison_slot.lock().unwrap();
            panic!("poison diagnostic slot for recovery test");
        });
        assert!(poisoner.join().is_err());

        let first = CommandError::from_reason(
            "retained after poison",
            CommandErrorReason::WaitFailed {
                source: std::io::Error::other("wait failed"),
            },
            None,
        );
        assert!(crate::execution::contracts::FactSink::publish_stop_diagnostic(&*state, first).is_ok());
        let duplicate = crate::execution::contracts::FactSink::publish_stop_diagnostic(
            &*state,
            CommandError::from_reason("duplicate", CommandErrorReason::Cancelled, None),
        )
        .expect_err("poison recovery must preserve the first diagnostic");
        assert_eq!(duplicate.kind(), CommandErrorKind::Cancelled);
        assert_eq!(
            slot.lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_ref()
                .unwrap()
                .kind(),
            CommandErrorKind::WaitFailed
        );
    }

    #[test]
    fn test_stop_request_recovers_poisoned_stop_port_and_retains_reason() {
        let scheduler = scheduler();
        let permit = scheduler.try_reserve(0).unwrap();
        let state = make_state(scheduler, permit);
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        let operation_id = operation.id();
        let _ = operation.wait_outcome().unwrap();
        operation.wait_settlement().unwrap();
        state.bind(operation_id).unwrap();
        let poisoned_state = Arc::clone(&state);
        let poisoner = std::thread::spawn(move || {
            let _stop = poisoned_state.stop.lock().unwrap();
            panic!("poison adapter stop-port slot for recovery test");
        });
        assert!(poisoner.join().is_err());

        OperationControl::request_stop(&*state, qubit_operation::StopReason::ManagerShutdown);
        assert_eq!(
            OperationControl::snapshot(&*state).stop.unwrap().reason,
            qubit_operation::StopReason::ManagerShutdown
        );
    }

    #[test]
    fn test_recovery_owner_is_returned_when_adapter_has_no_operation_identity() {
        let scheduler = scheduler();
        let permit = scheduler.try_reserve(0).unwrap();
        let state = make_state(scheduler, permit);
        let dropped = Arc::new(AtomicBool::new(false));
        let failure = crate::execution::contracts::FactSink::retain_recovery_owner(
            &*state,
            Box::new(DropObservedRecoveryOwner(Arc::clone(&dropped))),
        )
        .expect_err("an unbound adapter cannot retain an operation-scoped owner");

        assert_eq!(
            failure.reason,
            crate::execution::contracts::RecoveryPublishError::StoreUnavailable
        );
        assert!(!dropped.load(Ordering::SeqCst), "failure must preserve owner ownership");
        assert_eq!(failure.owner.summary(), "test recovery owner");
        drop(failure);
        assert!(dropped.load(Ordering::SeqCst), "returned owner remains caller-owned");
    }

    #[test]
    fn test_disconnected_recovery_store_returns_bound_owner_to_caller() {
        let scheduler = scheduler();
        let permit = scheduler.try_reserve(0).unwrap();
        let (recovery, receiver) = std::sync::mpsc::sync_channel::<super::super::RecoveryItem>(1);
        drop(receiver);
        let backend = Arc::new(ProcessBackend::new());
        let state = AdapterState::new(request(), permit, scheduler, backend.clone(), backend, recovery, None);
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        let id = operation.id();
        let _ = operation.wait_outcome().unwrap();
        operation.wait_settlement().unwrap();
        state.bind(id).unwrap();
        let dropped = Arc::new(AtomicBool::new(false));

        let failure = crate::execution::contracts::FactSink::retain_recovery_owner(
            &state,
            Box::new(DropObservedRecoveryOwner(Arc::clone(&dropped))),
        )
        .expect_err("a disconnected recovery store must reject the owner");

        assert_eq!(
            failure.reason,
            crate::execution::contracts::RecoveryPublishError::CapacityExceeded
        );
        assert!(!dropped.load(Ordering::SeqCst));
        assert_eq!(failure.owner.summary(), "test recovery owner");
        drop(failure);
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[test]
    fn test_full_recovery_store_returns_bound_owner_to_caller() {
        let scheduler = scheduler();
        let permit = scheduler.try_reserve(0).unwrap();
        let (recovery, _receiver) = std::sync::mpsc::sync_channel::<super::super::RecoveryItem>(0);
        let backend = Arc::new(ProcessBackend::new());
        let state = AdapterState::new(request(), permit, scheduler, backend.clone(), backend, recovery, None);
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        let id = operation.id();
        let _ = operation.wait_outcome().unwrap();
        operation.wait_settlement().unwrap();
        state.bind(id).unwrap();
        let dropped = Arc::new(AtomicBool::new(false));

        let failure = crate::execution::contracts::FactSink::retain_recovery_owner(
            &state,
            Box::new(DropObservedRecoveryOwner(Arc::clone(&dropped))),
        )
        .expect_err("a full recovery store must return the unaccepted owner");

        assert_eq!(
            failure.reason,
            crate::execution::contracts::RecoveryPublishError::CapacityExceeded
        );
        assert!(
            !dropped.load(Ordering::SeqCst),
            "failed publication must preserve owner ownership"
        );
        assert_eq!(failure.owner.summary(), "test recovery owner");
        drop(failure);
        assert!(
            dropped.load(Ordering::SeqCst),
            "dropping the returned owner must release it"
        );
    }

    #[test]
    fn test_bound_adapter_hands_recovery_owner_to_worker_and_publishes_settlement() {
        let scheduler = scheduler();
        let permit = scheduler.try_reserve(0).unwrap();
        let state = make_state(scheduler, permit);
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut handle = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        let id = handle.id();
        let observer = handle.observer();
        let _ = handle.wait_outcome().unwrap();
        handle.wait_settlement().unwrap();

        state.bind(id).unwrap();
        state.set_operation_observer(observer);
        let cleanup_finished = Arc::new(AtomicBool::new(false));
        crate::execution::contracts::FactSink::retain_recovery_owner(
            &*state,
            Box::new(CompleteRecoveryOwner(Arc::clone(&cleanup_finished))),
        )
        .expect("a bound adapter should transfer its owner to the recovery worker");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let settlement = loop {
            if let Some(report) = OperationControl::snapshot(&*state).settlement {
                break report;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "recovery worker did not publish settlement"
            );
            std::thread::yield_now();
        };
        assert!(cleanup_finished.load(Ordering::SeqCst));
        assert_eq!(settlement.operation_id, id);
        assert_eq!(settlement.status, qubit_operation::SettlementStatus::RecoveryRequired);
        assert_eq!(settlement.recovery_count, 1);
    }
}
