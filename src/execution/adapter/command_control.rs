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

use super::internal::State;
use crate::CommandError;
use crate::CommandOutput;

/// Represents the command control state or contract used by managed command
/// execution.
pub(crate) struct CommandControl {
    /// Stores the state value used by this execution component.
    state: Mutex<State>,
}
impl Default for CommandControl {
    /// Performs the default step in the managed command execution lifecycle.
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
    /// Performs the operation id step in the managed command execution
    /// lifecycle.
    pub(crate) fn operation_id(&self) -> Option<OperationId> {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).id
    }
    /// Stores the bind value used by this execution component.
    ///
    /// # Parameters
    /// * `id`: Supplies the id value used by this operation.
    pub(crate) fn bind(&self, id: OperationId) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.id.is_some() {
            return false;
        }
        state.id = Some(id);
        state.phase = ExecutionPhase::Queued;
        true
    }
    /// Performs the running step in the managed command execution lifecycle.
    pub(crate) fn running(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).phase = ExecutionPhase::Running;
    }
    /// Performs the publish outcome step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `outcome`: Supplies the outcome value used by this operation.
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
    /// Performs the release step in the managed command execution lifecycle.
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
    /// Stores the settlement value used by this execution component.
    ///
    /// # Parameters
    /// * `report`: Supplies the report value used by this operation.
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
    /// Performs the take step in the managed command execution lifecycle.
    ///
    /// # Parameters
    /// * `cx`: Supplies the cx value used by this operation.
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
    /// Stores the request stop value used by this execution component.
    ///
    /// # Parameters
    /// * `reason`: Supplies the reason value used by this operation.
    fn request_stop(&self, reason: StopReason) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if s.stop.is_none() {
            s.stop = Some(StopReport {
                reason,
                accepted_at: None,
            });
        }
    }
    /// Performs the snapshot step in the managed command execution lifecycle.
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
    /// Stores the poll outcome value used by this execution component.
    ///
    /// # Parameters
    /// * `cx`: Supplies the cx value used by this operation.
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
    /// Stores the poll release value used by this execution component.
    ///
    /// # Parameters
    /// * `cx`: Supplies the cx value used by this operation.
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
    /// Stores the poll settlement value used by this execution component.
    ///
    /// # Parameters
    /// * `cx`: Supplies the cx value used by this operation.
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use qubit_operation::ChildFailureSummary;
    use qubit_operation::ChildOutcomeKind;
    use qubit_operation::ChildOutcomeSummary;
    use qubit_operation::DiagnosticSummary;
    use qubit_operation::ExecutionFailure;
    use qubit_operation::ExecutionPhase;
    use qubit_operation::OperationControl;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;
    use qubit_operation::PanicSummary;
    use qubit_operation::StopReason;
    use qubit_operation::StopReport;

    use super::CommandControl;
    use crate::Command;
    use crate::CommandRunner;

    struct CountWake(AtomicUsize);
    impl Wake for CountWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn settlement_report() -> Arc<qubit_operation::SettlementReport> {
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
            .unwrap();
        let _ = operation.wait_outcome().unwrap();
        operation.wait_settlement().unwrap()
    }

    #[test]
    fn test_control_summarizes_each_terminal_failure_kind() {
        let id = settlement_report().operation_id;
        let outcomes = [
            (
                OperationOutcome::Failed(crate::CommandError::from_reason(
                    "failed command",
                    crate::CommandErrorReason::Cancelled,
                    None,
                )),
                ChildOutcomeKind::Failed,
            ),
            (
                OperationOutcome::ChildFailed(ChildFailureSummary {
                    operation_id: id,
                    outcome: ChildOutcomeSummary {
                        operation_id: id,
                        kind: ChildOutcomeKind::Failed,
                        summary: DiagnosticSummary::new("child failed"),
                    },
                }),
                ChildOutcomeKind::Failed,
            ),
            (
                OperationOutcome::Stopped(StopReport {
                    reason: StopReason::Timeout,
                    accepted_at: None,
                }),
                ChildOutcomeKind::Stopped,
            ),
            (
                OperationOutcome::Panicked(PanicSummary {
                    operation_id: id,
                    phase: ExecutionPhase::Running,
                    summary: DiagnosticSummary::new("worker panicked"),
                }),
                ChildOutcomeKind::Panicked,
            ),
            (
                OperationOutcome::ExecutionFailed(ExecutionFailure {
                    operation_id: id,
                    summary: DiagnosticSummary::new("operation execution failed"),
                    source: None,
                }),
                ChildOutcomeKind::ExecutionFailed,
            ),
        ];

        for (outcome, expected_kind) in outcomes {
            let control = CommandControl::default();
            assert!(control.bind(id));
            assert!(control.publish_outcome(outcome));
            let snapshot = OperationControl::snapshot(&control);
            assert_eq!(
                snapshot.outcome.expect("terminal summary should be stored").kind,
                expected_kind
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_control_publishes_terminal_facts_once_and_wakes_observers() {
        let control = CommandControl::default();
        let other_report = settlement_report();
        let (operation_id, report) = {
            let manager = OperationManager::new(Default::default()).unwrap();
            let mut operation = manager
                .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(()))
                .unwrap();
            let id = operation.id();
            let _ = operation.wait_outcome().unwrap();
            (id, operation.wait_settlement().unwrap())
        };
        assert_eq!(control.operation_id(), None);
        assert!(
            !control.publish_outcome(OperationOutcome::Succeeded(
                CommandRunner::new(std::time::Duration::from_secs(5))
                    .run(Command::new("true"))
                    .unwrap(),
            ))
        );
        assert!(!control.release());
        assert!(!control.settlement(Arc::clone(&report)));

        assert!(control.bind(operation_id));
        assert!(!control.bind(other_report.operation_id));
        assert_eq!(control.operation_id(), Some(operation_id));
        control.running();
        assert_eq!(OperationControl::snapshot(&control).execution, ExecutionPhase::Running);

        let wake = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&wake));
        let mut context = Context::from_waker(&waker);
        assert!(matches!(control.take(&mut context), Poll::Pending));
        assert!(matches!(
            OperationControl::poll_outcome(&control, &mut context),
            Poll::Pending
        ));
        assert!(matches!(
            OperationControl::poll_release(&control, &mut context),
            Poll::Pending
        ));
        assert!(matches!(
            OperationControl::poll_settlement(&control, &mut context),
            Poll::Pending
        ));

        let output = CommandRunner::new(std::time::Duration::from_secs(5))
            .run(Command::new("sh").arg("-c").arg("printf controlled"))
            .unwrap();
        assert!(control.publish_outcome(OperationOutcome::Succeeded(output)));
        assert!(
            !control.publish_outcome(OperationOutcome::Stopped(qubit_operation::StopReport {
                reason: StopReason::UserCancelled,
                accepted_at: None,
            }))
        );
        let snapshot = OperationControl::snapshot(&control);
        assert_eq!(snapshot.execution, ExecutionPhase::Finished);
        assert_eq!(
            snapshot.outcome.unwrap().kind,
            qubit_operation::ChildOutcomeKind::Succeeded
        );
        assert!(wake.0.load(Ordering::SeqCst) >= 1);

        assert!(control.release());
        assert!(!control.release());
        assert!(OperationControl::poll_release(&control, &mut context).is_ready());
        assert!(wake.0.load(Ordering::SeqCst) >= 2);

        assert!(!control.settlement(Arc::new((*other_report).clone())));
        assert!(control.settlement(Arc::clone(&report)));
        assert!(!control.settlement(Arc::clone(&report)));
        assert!(OperationControl::poll_settlement(&control, &mut context).is_ready());
        assert!(wake.0.load(Ordering::SeqCst) >= 3);

        assert!(matches!(
            control.take(&mut context),
            Poll::Ready(OperationOutcome::Succeeded(_))
        ));
        assert!(matches!(control.take(&mut context), Poll::Pending));
        control.request_stop(StopReason::Timeout);
        control.request_stop(StopReason::UserCancelled);
        assert_eq!(
            OperationControl::snapshot(&control).stop.unwrap().reason,
            StopReason::Timeout
        );
    }

    #[test]
    fn test_control_wakes_each_distinct_observer_once() {
        let report = settlement_report();
        let control = CommandControl::default();
        assert!(control.bind(report.operation_id));

        let first_wake = Arc::new(CountWake(AtomicUsize::new(0)));
        let first_waker = Waker::from(Arc::clone(&first_wake));
        let mut first_context = Context::from_waker(&first_waker);
        let second_wake = Arc::new(CountWake(AtomicUsize::new(0)));
        let second_waker = Waker::from(Arc::clone(&second_wake));
        let mut second_context = Context::from_waker(&second_waker);

        assert!(matches!(control.take(&mut first_context), Poll::Pending));
        for context in [&mut first_context, &mut second_context] {
            assert!(matches!(
                OperationControl::poll_outcome(&control, context),
                Poll::Pending
            ));
            assert!(matches!(
                OperationControl::poll_release(&control, context),
                Poll::Pending
            ));
            assert!(matches!(
                OperationControl::poll_settlement(&control, context),
                Poll::Pending
            ));
        }
        assert!(matches!(
            OperationControl::poll_outcome(&control, &mut first_context),
            Poll::Pending
        ));
        assert!(matches!(
            OperationControl::poll_release(&control, &mut first_context),
            Poll::Pending
        ));
        assert!(matches!(
            OperationControl::poll_settlement(&control, &mut first_context),
            Poll::Pending
        ));

        assert!(control.publish_outcome(OperationOutcome::Stopped(StopReport {
            reason: StopReason::UserCancelled,
            accepted_at: None,
        })));
        assert_eq!(first_wake.0.load(Ordering::SeqCst), 1);
        assert_eq!(second_wake.0.load(Ordering::SeqCst), 1);
        assert!(OperationControl::poll_outcome(&control, &mut second_context).is_ready());

        assert!(control.release());
        assert_eq!(first_wake.0.load(Ordering::SeqCst), 2);
        assert_eq!(second_wake.0.load(Ordering::SeqCst), 2);
        assert!(OperationControl::poll_release(&control, &mut second_context).is_ready());

        assert!(control.settlement(report));
        assert_eq!(first_wake.0.load(Ordering::SeqCst), 3);
        assert_eq!(second_wake.0.load(Ordering::SeqCst), 3);
        assert!(OperationControl::poll_settlement(&control, &mut second_context).is_ready());
        assert!(matches!(
            control.take(&mut first_context),
            Poll::Ready(OperationOutcome::Stopped(_))
        ));
    }

    #[test]
    fn test_stop_requested_before_binding_is_preserved_in_snapshot() {
        let control = CommandControl::default();
        control.request_stop(StopReason::Timeout);

        let report = settlement_report();
        assert!(control.bind(report.operation_id));
        assert_eq!(control.operation_id(), Some(report.operation_id));

        let snapshot = OperationControl::snapshot(&control);
        assert_eq!(snapshot.execution, ExecutionPhase::Queued);
        assert_eq!(
            snapshot.stop.expect("stop request should be retained").reason,
            StopReason::Timeout
        );
        assert!(snapshot.outcome.is_none());
        assert!(!snapshot.input_released);
        assert!(snapshot.settlement.is_none());
    }
}
