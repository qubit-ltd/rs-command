use super::CommandControl;
use crate::execution::backend::ProcessBackend;
use crate::execution::contracts::{
    Backend, FactSink, RecoveryOwner, RecoveryPublishFailure, Request, Reservation, ScheduledJob,
    StopPort,
};
use crate::{CommandError, CommandOutput};
use qubit_operation::{
    ExecutionFailure, ExternalBindError, OperationId, OperationOutcome, OperationScope,
    PreparedExternalOperation, SettlementReport,
};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

/// One-shot external operation adapter. Request and reservation remain explicitly recoverable
/// until activation transfers them to the domain scheduler.
pub(crate) struct AdapterState {
    owned: Mutex<OwnedInputs>,
    scheduler: Arc<dyn crate::execution::Scheduler>,
    backend: Arc<dyn Backend>,
    process_backend: Arc<ProcessBackend>,
    control: Arc<CommandControl>,
    stop: Mutex<Option<Arc<dyn StopPort>>>,
    recovery: super::cleanup::RecoverySender,
    session_bridge: Option<Arc<dyn crate::session::contracts::SessionBridge>>,
    #[cfg(feature = "async")]
    session_keepalive: Option<SessionOwners>,
}
struct OwnedInputs {
    request: Option<Request>,
    reservation: Option<Reservation>,
    phase: AdapterPhase,
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum AdapterPhase {
    Prepared,
    Activated,
    Abandoned,
    ActivationFailed,
}
#[cfg(feature = "async")]
pub(crate) struct SessionOwners {
    pub(crate) _stdin: crate::session::channel::ChannelReader,
    pub(crate) _stdout: crate::session::channel::ChannelWriter,
    pub(crate) _stderr: crate::session::channel::ChannelWriter,
}
impl AdapterState {
    pub(crate) fn new(
        request: Request,
        reservation: Reservation,
        scheduler: Arc<dyn crate::execution::Scheduler>,
        backend: Arc<dyn Backend>,
        process_backend: Arc<ProcessBackend>,
        recovery: super::cleanup::RecoverySender,
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
            stop: Mutex::new(None),
            recovery,
            session_bridge,
            #[cfg(feature = "async")]
            session_keepalive: None,
        }
    }
    #[cfg(feature = "async")]
    pub(crate) fn keep_session_alive(mut self, owners: SessionOwners) -> Self {
        self.session_keepalive = Some(owners);
        self
    }
    pub(crate) fn restore(&self) -> Option<Request> {
        self.owned
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .request
            .take()
    }
}
impl FactSink for AdapterState {
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
    fn confirm_input_release(&self) -> Result<(), crate::execution::contracts::FactPublishError> {
        if self.control.release() {
            Ok(())
        } else {
            Err(crate::execution::contracts::FactPublishError::DuplicateRelease)
        }
    }
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
    fn retain_recovery_owner(
        &self,
        owner: Box<dyn RecoveryOwner>,
    ) -> Result<(), RecoveryPublishFailure> {
        let Some(id) = self.control.operation_id() else {
            return Err(RecoveryPublishFailure {
                reason: crate::execution::contracts::RecoveryPublishError::StoreUnavailable,
                owner,
            });
        };
        let item = super::cleanup::RecoveryItem {
            owner,
            control: Arc::clone(&self.control),
            id,
        };
        self.recovery.try_send(item).map_err(|error| {
            let owner = match error {
                std::sync::mpsc::TrySendError::Full(item)
                | std::sync::mpsc::TrySendError::Disconnected(item) => item.owner,
            };
            RecoveryPublishFailure {
                reason: crate::execution::contracts::RecoveryPublishError::CapacityExceeded,
                owner,
            }
        })
    }
}
impl PreparedExternalOperation<CommandOutput, CommandError> for AdapterState {
    fn bind(&self, id: OperationId) -> Result<(), ExternalBindError> {
        if self.control.bind(id) {
            Ok(())
        } else {
            Err(ExternalBindError::AlreadyBound)
        }
    }
    fn activate(self: Arc<Self>, scope: OperationScope) -> Result<(), ExecutionFailure> {
        let operation_id = scope.id();
        let make_failure = |message: &str| ExecutionFailure {
            operation_id,
            summary: qubit_operation::DiagnosticSummary::new(message),
            source: None,
        };
        let permit = {
            let mut owned = self.owned.lock().unwrap_or_else(|e| e.into_inner());
            if owned.phase != AdapterPhase::Prepared
                || owned.request.is_none()
                || owned.reservation.is_none()
            {
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
            let failure = make_failure("prepared command inputs were unavailable at activation");
            let _ = self
                .control
                .publish_outcome(OperationOutcome::ExecutionFailed(failure.clone()));
            let _ = self.control.release();
            let _ = self
                .control
                .settlement(Arc::new(released_report(operation_id)));
            return Err(failure);
        };
        self.control.running();
        let stop = self.process_backend.stop_port(operation_id);
        *self.stop.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(Arc::clone(&stop) as Arc<dyn StopPort>);
        let state = Arc::clone(&self);
        let job = ScheduledJob {
            stop,
            run: Box::new(move |permit| {
                let request = state
                    .owned
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .request
                    .take();
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
        {
            if let Some(stop) = self.stop.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                stop.request_stop(reason)
            }
        }
        match self.scheduler.enqueue(job, permit) {
            Ok(()) => Ok(()),
            Err(failure) => {
                let crate::execution::contracts::EnqueueFailure {
                    reason,
                    job,
                    permit,
                } = failure;
                drop(permit);
                self.owned.lock().unwrap_or_else(|e| e.into_inner()).phase =
                    AdapterPhase::ActivationFailed;
                drop(job);
                let failure = make_failure("domain scheduler rejected activation");
                let _ = self
                    .control
                    .publish_outcome(OperationOutcome::ExecutionFailed(failure.clone()));
                let _ = self.control.release();
                let _ = self
                    .control
                    .settlement(Arc::new(released_report(operation_id)));
                let _ = reason;
                Err(failure)
            }
        }
    }
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
    fn poll_take_outcome(
        &self,
        cx: &mut Context<'_>,
    ) -> Poll<OperationOutcome<CommandOutput, CommandError>> {
        self.control.take(cx)
    }
}
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
    fn request_stop(&self, reason: qubit_operation::StopReason) {
        self.control.request_stop(reason);
        if let Some(stop) = self.stop.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            stop.request_stop(reason)
        }
    }
    fn snapshot(&self) -> qubit_operation::ChildSnapshot {
        qubit_operation::OperationControl::snapshot(&*self.control)
    }
    fn poll_outcome(&self, cx: &mut Context<'_>) -> Poll<qubit_operation::ChildOutcomeSummary> {
        qubit_operation::OperationControl::poll_outcome(&*self.control, cx)
    }
    fn poll_release(&self, cx: &mut Context<'_>) -> Poll<qubit_operation::ChildRelease> {
        qubit_operation::OperationControl::poll_release(&*self.control, cx)
    }
    fn poll_settlement(&self, cx: &mut Context<'_>) -> Poll<Arc<SettlementReport>> {
        qubit_operation::OperationControl::poll_settlement(&*self.control, cx)
    }
}
