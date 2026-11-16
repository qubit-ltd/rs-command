use std::sync::Arc;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;

use qubit_operation::ChildOutcomeKind;
use qubit_operation::ChildOutcomeSummary;
use qubit_operation::ChildRelease;
use qubit_operation::ChildSnapshot;
use qubit_operation::DiagnosticSummary;
use qubit_operation::ExecutionPhase;
use qubit_operation::OperationControl;
use qubit_operation::OperationId;
use qubit_operation::OperationOutcome;
use qubit_operation::SettlementReport;
use qubit_operation::StopReason;
use qubit_operation::StopReport;

use crate::CommandError;
use crate::CommandOutput;

pub(crate) struct CommandControl {
    state: Mutex<State>,
}
struct State {
    id: Option<OperationId>,
    phase: ExecutionPhase,
    stop: Option<StopReport>,
    outcome: Option<OperationOutcome<CommandOutput, CommandError>>,
    summary: Option<ChildOutcomeSummary>,
    release: Option<ChildRelease>,
    settlement: Option<Arc<SettlementReport>>,
    outcome_wakers: Vec<Waker>,
    release_wakers: Vec<Waker>,
    settlement_wakers: Vec<Waker>,
}
impl Default for CommandControl {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                id: None,
                phase: ExecutionPhase::Queued,
                stop: None,
                outcome: None,
                summary: None,
                release: None,
                settlement: None,
                outcome_wakers: Vec::new(),
                release_wakers: Vec::new(),
                settlement_wakers: Vec::new(),
            }),
        }
    }
}
impl CommandControl {
    pub(crate) fn operation_id(&self) -> Option<OperationId> {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).id
    }
    pub(crate) fn bind(&self, id: OperationId) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.id.is_some() {
            return false;
        }
        state.id = Some(id);
        state.phase = ExecutionPhase::Queued;
        true
    }
    pub(crate) fn running(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).phase = ExecutionPhase::Running;
    }
    pub(crate) fn publish_outcome(&self, outcome: OperationOutcome<CommandOutput, CommandError>) -> bool {
        let wakers = {
            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let Some(id) = s.id else {
                return false;
            };
            if s.outcome.is_some() {
                return false;
            }
            let (kind, diagnostic): (ChildOutcomeKind, String) = match &outcome {
                OperationOutcome::Succeeded(_) => (ChildOutcomeKind::Succeeded, "command succeeded".to_owned()),
                OperationOutcome::Failed(e) => (ChildOutcomeKind::Failed, e.to_string()),
                OperationOutcome::ChildFailed(e) => (ChildOutcomeKind::Failed, e.outcome.summary.as_str().to_owned()),
                OperationOutcome::Stopped(_) => (ChildOutcomeKind::Stopped, "command stopped".to_owned()),
                OperationOutcome::Panicked(p) => (ChildOutcomeKind::Panicked, p.summary.as_str().to_owned()),
                OperationOutcome::ExecutionFailed(f) => {
                    (ChildOutcomeKind::ExecutionFailed, f.summary.as_str().to_owned())
                }
            };
            s.summary = Some(ChildOutcomeSummary {
                operation_id: id,
                kind,
                summary: DiagnosticSummary::new(&diagnostic),
            });
            s.outcome = Some(outcome);
            s.phase = ExecutionPhase::Finished;
            std::mem::take(&mut s.outcome_wakers)
        };
        for w in wakers {
            w.wake();
        }
        true
    }
    pub(crate) fn release(&self) -> bool {
        let wakes = {
            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let Some(id) = s.id else { return false };
            if s.release.is_some() {
                return false;
            }
            s.release = Some(ChildRelease {
                operation_id: id,
                confirmed_at: None,
            });
            std::mem::take(&mut s.release_wakers)
        };
        for w in wakes {
            w.wake();
        }
        true
    }
    pub(crate) fn settlement(&self, report: Arc<SettlementReport>) -> bool {
        let wakes = {
            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if s.settlement.is_some() || s.id != Some(report.operation_id) {
                return false;
            }
            s.settlement = Some(report);
            std::mem::take(&mut s.settlement_wakers)
        };
        for w in wakes {
            w.wake();
        }
        true
    }
    pub(crate) fn take(&self, cx: &mut Context<'_>) -> Poll<OperationOutcome<CommandOutput, CommandError>> {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(out) = s.outcome.take() {
            return Poll::Ready(out);
        }
        if !s.outcome_wakers.iter().any(|w| w.will_wake(cx.waker())) {
            s.outcome_wakers.push(cx.waker().clone());
        }
        Poll::Pending
    }
}
impl OperationControl for CommandControl {
    fn request_stop(&self, reason: StopReason) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if s.stop.is_none() {
            s.stop = Some(StopReport {
                reason,
                accepted_at: None,
            });
        }
    }
    fn snapshot(&self) -> ChildSnapshot {
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        ChildSnapshot {
            operation_id: s.id.expect("operation manager binds before observing child"),
            execution: s.phase,
            stop: s.stop.clone(),
            outcome: s.summary.clone(),
            input_released: s.release.is_some(),
            settlement: s.settlement.clone(),
        }
    }
    fn poll_outcome(&self, cx: &mut Context<'_>) -> Poll<ChildOutcomeSummary> {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(x) = &s.summary {
            return Poll::Ready(x.clone());
        }
        if !s.outcome_wakers.iter().any(|w| w.will_wake(cx.waker())) {
            s.outcome_wakers.push(cx.waker().clone())
        }
        Poll::Pending
    }
    fn poll_release(&self, cx: &mut Context<'_>) -> Poll<ChildRelease> {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(x) = &s.release {
            return Poll::Ready(x.clone());
        }
        if !s.release_wakers.iter().any(|w| w.will_wake(cx.waker())) {
            s.release_wakers.push(cx.waker().clone())
        }
        Poll::Pending
    }
    fn poll_settlement(&self, cx: &mut Context<'_>) -> Poll<Arc<SettlementReport>> {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(x) = &s.settlement {
            return Poll::Ready(Arc::clone(x));
        }
        if !s.settlement_wakers.iter().any(|w| w.will_wake(cx.waker())) {
            s.settlement_wakers.push(cx.waker().clone())
        }
        Poll::Pending
    }
}
