use std::sync::Arc;

use qubit_operation::GroupOptions;
use qubit_operation::OperationContext;
use qubit_operation::OperationManager;
use qubit_operation::OperationOptions;
use qubit_operation::OperationScope;

use super::adapter::AdapterState;
use super::backend::ProcessBackend;
use super::contracts::ExecutionSnapshot;
use super::contracts::IoPathSnapshot;
use super::contracts::Request;
use super::contracts::RequestOptions;
use super::scheduler::BoundedScheduler;
use super::*;
use crate::Command;
use crate::CommandResourceRunOptions;
use crate::CommandRunOptions;
use crate::CommandRunner;

#[cfg(feature = "async")]
fn session_channels(runner: &CommandRunner) -> crate::session::channel::SessionChannels {
    match runner.configured_max_total_output_bytes() {
        Some(limit) => crate::session::channel::SessionChannels::with_policy(
            crate::session::contracts::SessionStreamPolicy::TerminateAfterBytes(limit),
        ),
        None => crate::session::channel::SessionChannels::new(),
    }
}

#[cfg(feature = "async")]
struct AsyncSubmissionContext {
    bridge: Option<Arc<dyn crate::session::contracts::SessionBridge>>,
    keepalive: Option<super::adapter::SessionOwners>,
    deadline: Option<OperationContext>,
}

#[cfg(feature = "async")]
impl AsyncSubmissionContext {
    fn plain(deadline: Option<OperationContext>) -> Self {
        Self {
            bridge: None,
            keepalive: None,
            deadline,
        }
    }
}

impl CommandExecutor {
    pub fn scheduler_snapshot(&self) -> super::scheduler::SchedulerSnapshot {
        self.scheduler.snapshot()
    }
    /// Stops new local reservations and returns a manager handle for observing
    /// group shutdown.
    pub fn request_shutdown(&self, mode: qubit_operation::ShutdownMode) -> qubit_operation::ShutdownHandle {
        self.scheduler.close_admission();
        self.group.request_shutdown(mode)
    }
    pub fn submit(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure> {
        self.submit_in_manager(&self.manager, runner, command, options, None)
    }
    pub fn submit_resources(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure> {
        self.submit_resources_manager(&self.manager, runner, command, options, None)
    }
    #[cfg(feature = "async")]
    pub fn submit_async(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandSubmitFailure> {
        self.submit_async_in_manager(
            &self.manager,
            runner,
            command,
            RequestOptions::Standard(options),
            AsyncSubmissionContext::plain(None),
        )
    }
    #[cfg(feature = "async")]
    pub fn submit_async_resources(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandResourceSubmitFailure> {
        self.submit_async_resources_manager(
            &self.manager,
            runner,
            command,
            options,
            AsyncSubmissionContext::plain(None),
        )
    }
    #[cfg(feature = "async")]
    pub fn submit_async_in(
        &self,
        scope: &OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandSubmitFailure> {
        let manager = match scope.manager().with_group(&self.group) {
            Ok(m) => m,
            Err(e) => {
                return Err(CommandSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options,
                    reason: CommandSubmitFailureReason::GroupBinding(e),
                });
            }
        };
        self.submit_async_in_manager(
            &manager,
            runner,
            command,
            RequestOptions::Standard(options),
            AsyncSubmissionContext::plain(Some(scope.context())),
        )
    }
    #[cfg(feature = "async")]
    pub fn submit_async_resources_in(
        &self,
        scope: &OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandResourceSubmitFailure> {
        let manager = match scope.manager().with_group(&self.group) {
            Ok(m) => m,
            Err(e) => {
                return Err(CommandResourceSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options: Box::new(options),
                    reason: CommandSubmitFailureReason::GroupBinding(e),
                });
            }
        };
        self.submit_async_resources_manager(
            &manager,
            runner,
            command,
            options,
            AsyncSubmissionContext::plain(Some(scope.context())),
        )
    }
    pub fn submit_in(
        &self,
        scope: &OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure> {
        let manager = match scope.manager().with_group(&self.group) {
            Ok(m) => m,
            Err(e) => {
                return Err(CommandSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options,
                    reason: CommandSubmitFailureReason::GroupBinding(e),
                });
            }
        };
        self.submit_in_manager(&manager, runner, command, options, Some(scope.context()))
    }
    pub fn submit_resources_in(
        &self,
        scope: &OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure> {
        let manager = match scope.manager().with_group(&self.group) {
            Ok(m) => m,
            Err(e) => {
                return Err(CommandResourceSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options: Box::new(options),
                    reason: CommandSubmitFailureReason::GroupBinding(e),
                });
            }
        };
        self.submit_resources_manager(&manager, runner, command, options, Some(scope.context()))
    }
    fn submit_in_manager(
        &self,
        manager: &OperationManager,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
        deadline: Option<OperationContext>,
    ) -> Result<CommandOperation, CommandSubmitFailure> {
        let request = make_request(runner, command, RequestOptions::Standard(options)).map_err(|error| {
            let (runner, command, options, message) = *error;
            let RequestOptions::Standard(options) = options else {
                unreachable!()
            };
            CommandSubmitFailure {
                runner: Box::new(runner),
                command: Box::new(command),
                options,
                reason: CommandSubmitFailureReason::Snapshot(SnapshotFailure { diagnostic: message }),
            }
        })?;
        let permit = match self.scheduler.try_reserve_until(0, deadline.clone()) {
            Ok(p) => p,
            Err(reason) => {
                let Request {
                    runner,
                    command,
                    options,
                    ..
                } = request;
                let RequestOptions::Standard(options) = options else {
                    unreachable!()
                };
                return Err(CommandSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options,
                    reason: CommandSubmitFailureReason::Capacity(reason),
                });
            }
        };
        let mut request = request;
        request.deadline = deadline;
        let prepared = Arc::new(AdapterState::new(
            request,
            permit,
            self.scheduler.clone(),
            self.backend.clone(),
            self.backend.clone(),
            self.recovery.clone(),
            None,
        ));
        match manager.try_submit_external(OperationOptions::default(), Arc::clone(&prepared)) {
            Ok(handle) => {
                prepared.set_operation_observer(handle.observer());
                Ok(handle)
            }
            Err(error) => {
                let (reason, _) = error.into_parts();
                let request = prepared.restore().expect("unactivated submission retains request");
                let options = match request.options {
                    RequestOptions::Standard(o) => o,
                    _ => unreachable!(),
                };
                Err(CommandSubmitFailure {
                    runner: Box::new(request.runner),
                    command: Box::new(request.command),
                    options,
                    reason: CommandSubmitFailureReason::Admission(reason),
                })
            }
        }
    }
    fn submit_resources_manager(
        &self,
        manager: &OperationManager,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
        deadline: Option<OperationContext>,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure> {
        let request = make_request(runner, command, RequestOptions::Resources(options)).map_err(|error| {
            let (runner, command, options, message) = *error;
            let RequestOptions::Resources(options) = options else {
                unreachable!()
            };
            CommandResourceSubmitFailure {
                runner: Box::new(runner),
                command: Box::new(command),
                options: Box::new(options),
                reason: CommandSubmitFailureReason::Snapshot(SnapshotFailure { diagnostic: message }),
            }
        })?;
        let bytes = resource_bytes(&request.options);
        let permit = match self.scheduler.try_reserve_until(bytes, deadline.clone()) {
            Ok(p) => p,
            Err(reason) => {
                let Request {
                    runner,
                    command,
                    options,
                    ..
                } = request;
                let RequestOptions::Resources(options) = options else {
                    unreachable!()
                };
                return Err(CommandResourceSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options: Box::new(options),
                    reason: CommandSubmitFailureReason::Capacity(reason),
                });
            }
        };
        let mut request = request;
        request.deadline = deadline;
        let prepared = Arc::new(AdapterState::new(
            request,
            permit,
            self.scheduler.clone(),
            self.backend.clone(),
            self.backend.clone(),
            self.recovery.clone(),
            None,
        ));
        match manager.try_submit_external(OperationOptions::default(), Arc::clone(&prepared)) {
            Ok(handle) => {
                prepared.set_operation_observer(handle.observer());
                Ok(handle)
            }
            Err(error) => {
                let (reason, _) = error.into_parts();
                let request = prepared.restore().expect("unactivated submission retains request");
                let RequestOptions::Resources(options) = request.options else {
                    unreachable!()
                };
                Err(CommandResourceSubmitFailure {
                    runner: Box::new(request.runner),
                    command: Box::new(request.command),
                    options: Box::new(options),
                    reason: CommandSubmitFailureReason::Admission(reason),
                })
            }
        }
    }
    #[cfg(feature = "async")]
    fn submit_async_in_manager(
        &self,
        manager: &OperationManager,
        runner: CommandRunner,
        command: Command,
        request_options: RequestOptions,
        context: AsyncSubmissionContext,
    ) -> Result<super::AsyncCommandOperation, CommandSubmitFailure> {
        let AsyncSubmissionContext {
            bridge,
            keepalive,
            deadline,
        } = context;
        let RequestOptions::Standard(options) = request_options else {
            unreachable!()
        };
        let request = make_request(runner, command, RequestOptions::Standard(options)).map_err(|error| {
            let (runner, command, options, message) = *error;
            let RequestOptions::Standard(options) = options else {
                unreachable!()
            };
            CommandSubmitFailure {
                runner: Box::new(runner),
                command: Box::new(command),
                options,
                reason: CommandSubmitFailureReason::Snapshot(SnapshotFailure { diagnostic: message }),
            }
        })?;
        let permit = match self.scheduler.try_reserve_until(0, deadline.clone()) {
            Ok(p) => p,
            Err(reason) => {
                let Request {
                    runner,
                    command,
                    options,
                    ..
                } = request;
                let RequestOptions::Standard(options) = options else {
                    unreachable!()
                };
                return Err(CommandSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options,
                    reason: CommandSubmitFailureReason::Capacity(reason),
                });
            }
        };
        let mut request = request;
        request.deadline = deadline;
        let mut prepared_state = AdapterState::new(
            request,
            permit,
            self.scheduler.clone(),
            self.backend.clone(),
            self.backend.clone(),
            self.recovery.clone(),
            bridge,
        );
        #[cfg(feature = "async")]
        if let Some(keepalive) = keepalive {
            prepared_state = prepared_state.keep_session_alive(keepalive);
        }
        let prepared = Arc::new(prepared_state);
        match manager
            .as_async()
            .try_submit_external(OperationOptions::default(), Arc::clone(&prepared))
        {
            Ok(handle) => {
                prepared.set_operation_observer(handle.observer().as_sync());
                Ok(handle)
            }
            Err(error) => {
                let (reason, _) = error.into_parts();
                let request = prepared
                    .restore()
                    .expect("rejected external submission retains request");
                let RequestOptions::Standard(options) = request.options else {
                    unreachable!()
                };
                Err(CommandSubmitFailure {
                    runner: Box::new(request.runner),
                    command: Box::new(request.command),
                    options,
                    reason: CommandSubmitFailureReason::Admission(reason),
                })
            }
        }
    }
    #[cfg(feature = "async")]
    pub(crate) fn start_session_managed(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<crate::session::CommandSession, CommandError> {
        self.start_session_managed_in_manager(&self.manager, runner, command, options, None)
    }
    #[cfg(feature = "async")]
    pub(crate) fn start_session_managed_in(
        &self,
        scope: &OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<crate::session::CommandSession, CommandError> {
        let manager = scope
            .manager()
            .with_group(&self.group)
            .map_err(|e| managed_failure(format!("group binding failed: {e}")))?;
        self.start_session_managed_in_manager(&manager, runner, command, options, Some(scope.context()))
    }
    #[cfg(feature = "async")]
    pub(crate) fn start_session_resources_managed(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<crate::session::CommandSession, CommandError> {
        self.start_session_resources_managed_in_manager(&self.manager, runner, command, options, None)
    }
    #[cfg(feature = "async")]
    pub(crate) fn start_session_resources_managed_in(
        &self,
        scope: &OperationScope,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<crate::session::CommandSession, CommandError> {
        let manager = scope
            .manager()
            .with_group(&self.group)
            .map_err(|e| managed_failure(format!("group binding failed: {e}")))?;
        self.start_session_resources_managed_in_manager(&manager, runner, command, options, Some(scope.context()))
    }
    #[cfg(feature = "async")]
    fn start_session_managed_in_manager(
        &self,
        manager: &OperationManager,
        runner: CommandRunner,
        command: Command,
        options: CommandRunOptions,
        deadline: Option<OperationContext>,
    ) -> Result<crate::session::CommandSession, CommandError> {
        let channels = session_channels(&runner);
        let crate::session::channel::SessionChannels {
            bridge,
            stdin,
            stdout,
            stderr,
            stdin_reader,
            stdout_writer,
            stderr_writer,
        } = channels;
        let operation = self
            .submit_async_in_manager(
                manager,
                runner,
                command,
                RequestOptions::Standard(options),
                AsyncSubmissionContext {
                    bridge: Some(bridge),
                    keepalive: Some(super::adapter::SessionOwners {
                        _stdin: stdin_reader,
                        _stdout: stdout_writer,
                        _stderr: stderr_writer,
                    }),
                    deadline,
                },
            )
            .map_err(|e| managed_failure(format!("managed session admission failed: {:?}", e.reason)))?;
        Ok(crate::session::CommandSession {
            operation: Some(operation.into_sync()),
            stdin: Some(stdin),
            stdout: Some(stdout),
            stderr: Some(stderr),
        })
    }
    #[cfg(feature = "async")]
    fn start_session_resources_managed_in_manager(
        &self,
        manager: &OperationManager,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
        deadline: Option<OperationContext>,
    ) -> Result<crate::session::CommandSession, CommandError> {
        let channels = session_channels(&runner);
        let crate::session::channel::SessionChannels {
            bridge,
            stdin,
            stdout,
            stderr,
            stdin_reader,
            stdout_writer,
            stderr_writer,
        } = channels;
        let operation = self
            .submit_async_resources_manager(
                manager,
                runner,
                command,
                options,
                AsyncSubmissionContext {
                    bridge: Some(bridge),
                    keepalive: Some(super::adapter::SessionOwners {
                        _stdin: stdin_reader,
                        _stdout: stdout_writer,
                        _stderr: stderr_writer,
                    }),
                    deadline,
                },
            )
            .map_err(|e| managed_failure(format!("managed resource session admission failed: {:?}", e.reason)))?;
        Ok(crate::session::CommandSession {
            operation: Some(operation.into_sync()),
            stdin: Some(stdin),
            stdout: Some(stdout),
            stderr: Some(stderr),
        })
    }
    #[cfg(feature = "async")]
    fn submit_async_resources_manager(
        &self,
        manager: &OperationManager,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
        context: AsyncSubmissionContext,
    ) -> Result<super::AsyncCommandOperation, CommandResourceSubmitFailure> {
        let AsyncSubmissionContext {
            bridge,
            keepalive,
            deadline,
        } = context;
        let request = make_request(runner, command, RequestOptions::Resources(options)).map_err(|error| {
            let (runner, command, options, message) = *error;
            let RequestOptions::Resources(options) = options else {
                unreachable!()
            };
            CommandResourceSubmitFailure {
                runner: Box::new(runner),
                command: Box::new(command),
                options: Box::new(options),
                reason: CommandSubmitFailureReason::Snapshot(SnapshotFailure { diagnostic: message }),
            }
        })?;
        let bytes = resource_bytes(&request.options);
        let permit = match self.scheduler.try_reserve_until(bytes, deadline.clone()) {
            Ok(p) => p,
            Err(reason) => {
                let Request {
                    runner,
                    command,
                    options,
                    ..
                } = request;
                let RequestOptions::Resources(options) = options else {
                    unreachable!()
                };
                return Err(CommandResourceSubmitFailure {
                    runner: Box::new(runner),
                    command: Box::new(command),
                    options: Box::new(options),
                    reason: CommandSubmitFailureReason::Capacity(reason),
                });
            }
        };
        let mut request = request;
        request.deadline = deadline;
        let mut prepared_state = AdapterState::new(
            request,
            permit,
            self.scheduler.clone(),
            self.backend.clone(),
            self.backend.clone(),
            self.recovery.clone(),
            bridge,
        );
        if let Some(keepalive) = keepalive {
            prepared_state = prepared_state.keep_session_alive(keepalive)
        }
        let prepared = Arc::new(prepared_state);
        match manager
            .as_async()
            .try_submit_external(OperationOptions::default(), Arc::clone(&prepared))
        {
            Ok(handle) => {
                prepared.set_operation_observer(handle.observer().as_sync());
                Ok(handle)
            }
            Err(error) => {
                let (reason, _) = error.into_parts();
                let request = prepared
                    .restore()
                    .expect("rejected external submission retains request");
                let RequestOptions::Resources(options) = request.options else {
                    unreachable!()
                };
                Err(CommandResourceSubmitFailure {
                    runner: Box::new(request.runner),
                    command: Box::new(request.command),
                    options: Box::new(options),
                    reason: CommandSubmitFailureReason::Admission(reason),
                })
            }
        }
    }
}
fn resource_bytes(options: &RequestOptions) -> usize {
    match options {
        RequestOptions::Standard(_) => 0,
        RequestOptions::Resources(o) => [
            o.configured_stdin().and_then(|f| f.metadata().ok()).map(|m| m.len()),
            o.configured_stdout()
                .and_then(|(f, _)| f.metadata().ok())
                .map(|m| m.len()),
            o.configured_stderr()
                .and_then(|(f, _)| f.metadata().ok())
                .map(|m| m.len()),
        ]
        .into_iter()
        .flatten()
        .fold(0usize, |sum, n| {
            sum.saturating_add(usize::try_from(n).unwrap_or(usize::MAX))
        }),
    }
}
fn resource_option_bytes(options: &CommandResourceRunOptions) -> usize {
    [
        options
            .configured_stdin()
            .and_then(|f| f.metadata().ok())
            .map(|m| m.len()),
        options
            .configured_stdout()
            .and_then(|(f, _)| f.metadata().ok())
            .map(|m| m.len()),
        options
            .configured_stderr()
            .and_then(|(f, _)| f.metadata().ok())
            .map(|m| m.len()),
    ]
    .into_iter()
    .flatten()
    .fold(0usize, |sum, n| {
        sum.saturating_add(usize::try_from(n).unwrap_or(usize::MAX))
    })
}
fn retry_wait(
    reason: &CommandSubmitFailureReason,
    scheduler: &BoundedScheduler,
    bytes: usize,
    deadline: Option<OperationContext>,
) -> bool {
    match reason {
        CommandSubmitFailureReason::Capacity(CapacityFailure::ActiveFull | CapacityFailure::QueueFull) => {
            scheduler.wait_for_capacity(bytes, deadline)
        }
        CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy) => {
            if deadline.as_ref().is_some_and(OperationContext::is_expired) {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            !deadline.as_ref().is_some_and(OperationContext::is_expired)
        }
        _ => false,
    }
}
fn deadline_failure(reason: CommandSubmitFailureReason, deadline: &OperationContext) -> CommandSubmitFailureReason {
    if deadline.is_expired() {
        CommandSubmitFailureReason::Capacity(CapacityFailure::DeadlineExpired)
    } else {
        reason
    }
}
#[cfg(feature = "async")]
fn async_capacity_retry(reason: &CommandSubmitFailureReason) -> bool {
    matches!(
        reason,
        CommandSubmitFailureReason::Capacity(CapacityFailure::ActiveFull | CapacityFailure::QueueFull)
    )
}
#[cfg(feature = "async")]
fn retryable_async(reason: &CommandSubmitFailureReason) -> bool {
    async_capacity_retry(reason)
        || matches!(
            reason,
            CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy)
        )
}
#[cfg(feature = "async")]
struct CapacityWait {
    scheduler: BoundedScheduler,
    bytes: usize,
    waiter: Option<u64>,
}
#[cfg(feature = "async")]
impl std::future::Future for CapacityWait {
    type Output = bool;
    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<bool> {
        if let Some(id) = self.waiter.take() {
            self.scheduler.cancel_waiter(id);
            return std::task::Poll::Ready(true);
        }
        match self.scheduler.register_waiter_if_full(self.bytes, cx.waker()) {
            Ok(None) => std::task::Poll::Ready(true),
            Ok(Some(id)) => {
                self.waiter = Some(id);
                std::task::Poll::Pending
            }
            Err(_) => std::task::Poll::Ready(false),
        }
    }
}
#[cfg(feature = "async")]
impl Drop for CapacityWait {
    fn drop(&mut self) {
        if let Some(id) = self.waiter.take() {
            self.scheduler.cancel_waiter(id)
        }
    }
}
#[cfg(feature = "async")]
async fn wait_capacity_async(scheduler: BoundedScheduler, bytes: usize, context: Option<OperationContext>) -> bool {
    let mut capacity = Box::pin(CapacityWait {
        scheduler,
        bytes,
        waiter: None,
    });
    if let Some(context) = context {
        let mut stop = Box::pin(context.stop_requested());
        let mut deadline = context.deadline_signal().map(Box::pin);
        std::future::poll_fn(|cx| {
            if stop.as_mut().poll(cx).is_ready() {
                return std::task::Poll::Ready(false);
            }
            if deadline
                .as_mut()
                .is_some_and(|signal| signal.as_mut().poll(cx).is_ready())
            {
                return std::task::Poll::Ready(false);
            }
            if let std::task::Poll::Ready(value) = capacity.as_mut().poll(cx) {
                return std::task::Poll::Ready(value);
            }
            std::task::Poll::Pending
        })
        .await
    } else {
        capacity.await
    }
}

#[cfg(feature = "async")]
async fn wait_retryable_async(
    scheduler: BoundedScheduler,
    bytes: usize,
    reason: &CommandSubmitFailureReason,
    context: Option<OperationContext>,
) -> bool {
    if async_capacity_retry(reason) {
        wait_capacity_async(scheduler, bytes, context).await
    } else if matches!(
        reason,
        CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy)
    ) {
        wait_retry_tick(context).await
    } else {
        false
    }
}

#[cfg(feature = "async")]
struct RetryTimerMessage {
    due: std::time::Instant,
    waker: std::task::Waker,
    ready: Arc<std::sync::atomic::AtomicBool>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}
#[cfg(feature = "async")]
static RETRY_TIMER: std::sync::OnceLock<Option<std::sync::mpsc::SyncSender<RetryTimerMessage>>> =
    std::sync::OnceLock::new();
#[cfg(feature = "async")]
fn retry_timer() -> Option<&'static std::sync::mpsc::SyncSender<RetryTimerMessage>> {
    RETRY_TIMER
        .get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::sync_channel::<RetryTimerMessage>(1024);
            std::thread::Builder::new()
                .name("qubit-command-admission-timer".to_owned())
                .spawn(move || {
                    let mut pending = Vec::<RetryTimerMessage>::new();
                    loop {
                        while let Ok(item) = rx.try_recv() {
                            pending.push(item)
                        }
                        let now = std::time::Instant::now();
                        let mut index = 0;
                        while index < pending.len() {
                            if pending[index].cancelled.load(std::sync::atomic::Ordering::Acquire) {
                                pending.swap_remove(index);
                            } else if pending[index].due <= now {
                                let item = pending.swap_remove(index);
                                item.ready.store(true, std::sync::atomic::Ordering::Release);
                                item.waker.wake()
                            } else {
                                index += 1
                            }
                        }
                        let wait = pending
                            .iter()
                            .map(|x| x.due.saturating_duration_since(std::time::Instant::now()))
                            .min()
                            .unwrap_or(std::time::Duration::from_secs(30));
                        if let Ok(item) = rx.recv_timeout(wait) {
                            pending.push(item)
                        }
                    }
                })
                .ok()?;
            Some(tx)
        })
        .as_ref()
}
#[cfg(feature = "async")]
struct RetryDelay {
    ready: Arc<std::sync::atomic::AtomicBool>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    scheduled: bool,
}
#[cfg(feature = "async")]
impl RetryDelay {
    fn new() -> Self {
        Self {
            ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            scheduled: false,
        }
    }
}
#[cfg(feature = "async")]
impl std::future::Future for RetryDelay {
    type Output = bool;
    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<bool> {
        if self.ready.load(std::sync::atomic::Ordering::Acquire) {
            return std::task::Poll::Ready(true);
        }
        if self.scheduled {
            return std::task::Poll::Pending;
        }
        let Some(tx) = retry_timer() else {
            return std::task::Poll::Ready(false);
        };
        let item = RetryTimerMessage {
            due: std::time::Instant::now() + std::time::Duration::from_millis(10),
            waker: cx.waker().clone(),
            ready: Arc::clone(&self.ready),
            cancelled: Arc::clone(&self.cancelled),
        };
        match tx.try_send(item) {
            Ok(()) => {
                self.scheduled = true;
                std::task::Poll::Pending
            }
            Err(_) => std::task::Poll::Ready(false),
        }
    }
}
#[cfg(feature = "async")]
impl Drop for RetryDelay {
    fn drop(&mut self) {
        self.cancelled.store(true, std::sync::atomic::Ordering::Release)
    }
}
#[cfg(feature = "async")]
async fn wait_retry_tick(context: Option<OperationContext>) -> bool {
    let mut delay = Box::pin(RetryDelay::new());
    if let Some(context) = context {
        let mut stop = Box::pin(context.stop_requested());
        let mut deadline = context.deadline_signal().map(Box::pin);
        std::future::poll_fn(|cx| {
            if stop.as_mut().poll(cx).is_ready() {
                return std::task::Poll::Ready(false);
            }
            if deadline
                .as_mut()
                .is_some_and(|signal| signal.as_mut().poll(cx).is_ready())
            {
                return std::task::Poll::Ready(false);
            }
            if let std::task::Poll::Ready(value) = delay.as_mut().poll(cx) {
                return std::task::Poll::Ready(value);
            }
            std::task::Poll::Pending
        })
        .await
    } else {
        delay.await
    }
}
#[cfg(feature = "async")]
fn managed_failure(message: String) -> CommandError {
    CommandError::from_reason(
        "<managed command>",
        crate::CommandErrorReason::ManagedOperationFailed { diagnostic: message },
        None,
    )
}
fn make_request(
    runner: CommandRunner,
    command: Command,
    options: RequestOptions,
) -> Result<Request, Box<(CommandRunner, Command, RequestOptions, String)>> {
    let wd = if let Some(path) = runner
        .configured_working_directory()
        .or_else(|| command.working_directory_override())
    {
        path.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(path) => path,
            Err(e) => {
                return Err(Box::new((
                    runner,
                    command,
                    options,
                    format!("working directory snapshot failed: {e}"),
                )));
            }
        }
    };
    let env = std::env::vars_os().map(|(k, v)| (k, Some(v))).collect();
    let paths = match &options {
        RequestOptions::Standard(o) => IoPathSnapshot {
            stdin: None,
            stdout: o.configured_stdout_file().map(ToOwned::to_owned),
            stderr: o.configured_stderr_file().map(ToOwned::to_owned),
        },
        RequestOptions::Resources(_) => IoPathSnapshot::default(),
    };
    Ok(Request {
        operation_id: None,
        runner,
        command,
        options,
        snapshot: ExecutionSnapshot {
            working_directory: wd,
            environment: env,
        },
        io_paths: paths,
        deadline: None,
    })
}

impl CommandExecutorApi for CommandExecutor {
    fn submit(
        &self,
        r: CommandRunner,
        c: Command,
        o: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure> {
        CommandExecutor::submit(self, r, c, o)
    }
    fn submit_resources(
        &self,
        r: CommandRunner,
        c: Command,
        o: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure> {
        CommandExecutor::submit_resources(self, r, c, o)
    }
    fn submit_in(
        &self,
        s: &OperationScope,
        r: CommandRunner,
        c: Command,
        o: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure> {
        CommandExecutor::submit_in(self, s, r, c, o)
    }
    fn submit_resources_in(
        &self,
        s: &OperationScope,
        r: CommandRunner,
        c: Command,
        o: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure> {
        CommandExecutor::submit_resources_in(self, s, r, c, o)
    }
    fn submit_wait_blocking_in(
        &self,
        s: &OperationScope,
        mut r: CommandRunner,
        mut c: Command,
        mut o: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure> {
        let deadline = s.context();
        loop {
            match self.submit_in(s, r, c, o) {
                Ok(h) => return Ok(h),
                Err(failure) => {
                    let CommandSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    } = failure;
                    if retry_wait(&reason, &self.scheduler, 0, Some(deadline.clone())) {
                        r = *runner;
                        c = *command;
                        o = options;
                        continue;
                    }
                    return Err(CommandSubmitFailure {
                        runner,
                        command,
                        options,
                        reason: deadline_failure(reason, &deadline),
                    });
                }
            }
        }
    }
    fn submit_wait_blocking(
        &self,
        mut r: CommandRunner,
        mut c: Command,
        mut o: CommandRunOptions,
    ) -> Result<CommandOperation, CommandSubmitFailure> {
        loop {
            match self.submit(r, c, o) {
                Ok(h) => return Ok(h),
                Err(failure) => {
                    let CommandSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    } = failure;
                    if retry_wait(&reason, &self.scheduler, 0, None) {
                        r = *runner;
                        c = *command;
                        o = options;
                        continue;
                    }
                    return Err(CommandSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    });
                }
            }
        }
    }
    fn submit_wait_resources_blocking(
        &self,
        mut r: CommandRunner,
        mut c: Command,
        mut o: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure> {
        loop {
            let bytes = resource_option_bytes(&o);
            match self.submit_resources(r, c, o) {
                Ok(h) => return Ok(h),
                Err(failure) => {
                    let CommandResourceSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    } = failure;
                    if retry_wait(&reason, &self.scheduler, bytes, None) {
                        r = *runner;
                        c = *command;
                        o = *options;
                        continue;
                    }
                    return Err(CommandResourceSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    });
                }
            }
        }
    }
    fn submit_wait_resources_blocking_in(
        &self,
        s: &OperationScope,
        mut r: CommandRunner,
        mut c: Command,
        mut o: CommandResourceRunOptions,
    ) -> Result<CommandOperation, CommandResourceSubmitFailure> {
        let deadline = s.context();
        loop {
            let bytes = resource_option_bytes(&o);
            match self.submit_resources_in(s, r, c, o) {
                Ok(h) => return Ok(h),
                Err(failure) => {
                    let CommandResourceSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    } = failure;
                    if retry_wait(&reason, &self.scheduler, bytes, Some(deadline.clone())) {
                        r = *runner;
                        c = *command;
                        o = *options;
                        continue;
                    }
                    return Err(CommandResourceSubmitFailure {
                        runner,
                        command,
                        options,
                        reason: deadline_failure(reason, &deadline),
                    });
                }
            }
        }
    }
    #[cfg(feature = "async")]
    async fn submit_wait(
        &self,
        mut r: CommandRunner,
        mut c: Command,
        mut o: CommandRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandSubmitFailure> {
        loop {
            match self.submit_async(r, c, o) {
                Ok(h) => return Ok(h),
                Err(f) => {
                    let CommandSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    } = f;
                    if !retryable_async(&reason) {
                        return Err(CommandSubmitFailure {
                            runner,
                            command,
                            options,
                            reason,
                        });
                    }
                    if wait_retryable_async(self.scheduler.as_ref().clone(), 0, &reason, None).await {
                        r = *runner;
                        c = *command;
                        o = options;
                        continue;
                    }
                    return Err(CommandSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    });
                }
            }
        }
    }
    #[cfg(feature = "async")]
    async fn submit_wait_resources(
        &self,
        mut r: CommandRunner,
        mut c: Command,
        mut o: CommandResourceRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandResourceSubmitFailure> {
        loop {
            let bytes = resource_option_bytes(&o);
            match self.submit_async_resources(r, c, o) {
                Ok(h) => return Ok(h),
                Err(f) => {
                    let CommandResourceSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    } = f;
                    if !retryable_async(&reason) {
                        return Err(CommandResourceSubmitFailure {
                            runner,
                            command,
                            options,
                            reason,
                        });
                    }
                    if wait_retryable_async(self.scheduler.as_ref().clone(), bytes, &reason, None).await {
                        r = *runner;
                        c = *command;
                        o = *options;
                        continue;
                    }
                    return Err(CommandResourceSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    });
                }
            }
        }
    }
    #[cfg(feature = "async")]
    async fn submit_wait_resources_in<'a>(
        &'a self,
        s: &'a OperationScope,
        mut r: CommandRunner,
        mut c: Command,
        mut o: CommandResourceRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandResourceSubmitFailure> {
        let context = s.context();
        loop {
            let bytes = resource_option_bytes(&o);
            match self.submit_async_resources_in(s, r, c, o) {
                Ok(h) => return Ok(h),
                Err(f) => {
                    let CommandResourceSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    } = f;
                    if !retryable_async(&reason) {
                        return Err(CommandResourceSubmitFailure {
                            runner,
                            command,
                            options,
                            reason,
                        });
                    }
                    if wait_retryable_async(self.scheduler.as_ref().clone(), bytes, &reason, Some(context.clone()))
                        .await
                    {
                        r = *runner;
                        c = *command;
                        o = *options;
                        continue;
                    }
                    return Err(CommandResourceSubmitFailure {
                        runner,
                        command,
                        options,
                        reason: deadline_failure(reason, &context),
                    });
                }
            }
        }
    }
    #[cfg(feature = "async")]
    fn submit_async(
        &self,
        r: CommandRunner,
        c: Command,
        o: CommandRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandSubmitFailure> {
        CommandExecutor::submit_async(self, r, c, o)
    }
    #[cfg(feature = "async")]
    fn submit_async_resources(
        &self,
        r: CommandRunner,
        c: Command,
        o: CommandResourceRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandResourceSubmitFailure> {
        CommandExecutor::submit_async_resources(self, r, c, o)
    }
    #[cfg(feature = "async")]
    fn submit_async_in(
        &self,
        s: &OperationScope,
        r: CommandRunner,
        c: Command,
        o: CommandRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandSubmitFailure> {
        CommandExecutor::submit_async_in(self, s, r, c, o)
    }
    #[cfg(feature = "async")]
    fn submit_async_resources_in(
        &self,
        s: &OperationScope,
        r: CommandRunner,
        c: Command,
        o: CommandResourceRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandResourceSubmitFailure> {
        CommandExecutor::submit_async_resources_in(self, s, r, c, o)
    }
    #[cfg(feature = "async")]
    async fn submit_wait_in<'a>(
        &'a self,
        s: &'a OperationScope,
        mut r: CommandRunner,
        mut c: Command,
        mut o: CommandRunOptions,
    ) -> Result<super::AsyncCommandOperation, CommandSubmitFailure> {
        let context = s.context();
        loop {
            match self.submit_async_in(s, r, c, o) {
                Ok(h) => return Ok(h),
                Err(f) => {
                    let CommandSubmitFailure {
                        runner,
                        command,
                        options,
                        reason,
                    } = f;
                    if !retryable_async(&reason) {
                        return Err(CommandSubmitFailure {
                            runner,
                            command,
                            options,
                            reason,
                        });
                    }
                    if wait_retryable_async(self.scheduler.as_ref().clone(), 0, &reason, Some(context.clone())).await {
                        r = *runner;
                        c = *command;
                        o = options;
                        continue;
                    }
                    return Err(CommandSubmitFailure {
                        runner,
                        command,
                        options,
                        reason: deadline_failure(reason, &context),
                    });
                }
            }
        }
    }
}
pub(crate) fn build_executor(
    manager: OperationManager,
    config: CommandExecutionConfig,
) -> Result<
    (
        BoundedScheduler,
        ProcessBackend,
        qubit_operation::OperationGroup,
        super::adapter::cleanup::RecoverySender,
        OperationManager,
    ),
    CommandExecutorCreateError,
> {
    config.validate()?;
    let group = manager
        .group(GroupOptions {
            capacity: config.max_active.saturating_add(config.max_queued).max(1),
        })
        .map_err(|_| CommandExecutorCreateError::ZeroActiveCapacity)?;
    let manager_view = manager
        .with_group(&group)
        .map_err(CommandExecutorCreateError::GroupBinding)?;
    let scheduler = BoundedScheduler::new(config.clone())?;
    let backend = ProcessBackend::new();
    let worker_handles = spawn_domain_workers_with(&scheduler, config.max_active, |index, worker| {
        std::thread::Builder::new()
            .name(format!("qubit-command-worker-{index}"))
            .spawn(worker)
    })
    .map_err(|_| CommandExecutorCreateError::WorkerStartFailed)?;
    let recovery = match super::adapter::cleanup::start_recovery_worker(config.max_active) {
        Ok(recovery) => recovery,
        Err(_) => {
            scheduler.close_admission();
            for worker in worker_handles {
                let _ = worker.join();
            }
            return Err(CommandExecutorCreateError::WorkerStartFailed);
        }
    };
    drop(worker_handles);
    Ok((scheduler, backend, group, recovery, manager_view))
}

fn spawn_domain_workers_with(
    scheduler: &BoundedScheduler,
    count: usize,
    mut spawn: impl FnMut(usize, Box<dyn FnOnce() + Send + 'static>) -> std::io::Result<std::thread::JoinHandle<()>>,
) -> std::io::Result<Vec<std::thread::JoinHandle<()>>> {
    let mut workers = Vec::with_capacity(count);
    for index in 0..count {
        let worker_scheduler = scheduler.clone();
        let worker: Box<dyn FnOnce() + Send + 'static> = Box::new(move || {
            while let Some((job, permit)) = worker_scheduler.wait_next() {
                (job.run)(permit);
            }
        });
        match spawn(index, worker) {
            Ok(handle) => workers.push(handle),
            Err(error) => {
                scheduler.close_admission();
                for worker in workers {
                    let _ = worker.join();
                }
                return Err(error);
            }
        }
    }
    Ok(workers)
}

#[cfg(all(test, feature = "async"))]
mod retry_wait_tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use qubit_operation::ClockDomainId;
    use qubit_operation::ClockSource;
    use qubit_operation::ContextOptions;
    use qubit_operation::Deadline;
    use qubit_operation::MonotonicTime;
    use qubit_operation::OperationContext;
    use qubit_operation::StopReason;
    use qubit_operation::TimerRegistration;

    use super::wait_retry_tick;

    struct NoopWake;
    impl Wake for NoopWake {
        fn wake(self: Arc<Self>) {}
    }
    struct NoopRegistration;
    impl TimerRegistration for NoopRegistration {
        fn cancel(self: Box<Self>) {}
    }
    struct ManualClock {
        domain: ClockDomainId,
        ticks: AtomicU64,
    }
    impl ClockSource for ManualClock {
        fn now(&self) -> MonotonicTime {
            MonotonicTime::new(self.domain, self.ticks.load(Ordering::SeqCst))
        }
        fn register_deadline(&self, _: MonotonicTime, _: Waker) -> Box<dyn TimerRegistration> {
            Box::new(NoopRegistration)
        }
    }

    #[test]
    fn retry_wait_observes_operation_cancellation() {
        let context = OperationContext::new(ContextOptions::default()).unwrap();
        let mut retry = Box::pin(wait_retry_tick(Some(context.clone())));
        let waker = Waker::from(Arc::new(NoopWake));
        let mut cx = Context::from_waker(&waker);
        assert!(retry.as_mut().poll(&mut cx).is_pending());
        assert!(context.request_stop(StopReason::UserCancelled));
        assert!(matches!(retry.as_mut().poll(&mut cx), Poll::Ready(false)));
    }

    #[test]
    fn retry_wait_observes_inherited_deadline() {
        let clock = Arc::new(ManualClock {
            domain: ClockDomainId::new_unique(),
            ticks: AtomicU64::new(0),
        });
        let source: Arc<dyn ClockSource> = clock.clone();
        let context = OperationContext::new(ContextOptions {
            clock: Some(source),
            deadline: Some(Deadline::at(MonotonicTime::new(clock.domain, 5))),
            budget: None,
        })
        .unwrap();
        let mut retry = Box::pin(wait_retry_tick(Some(context.clone())));
        let waker = Waker::from(Arc::new(NoopWake));
        let mut cx = Context::from_waker(&waker);
        assert!(retry.as_mut().poll(&mut cx).is_pending());
        clock.ticks.store(5, Ordering::SeqCst);
        assert!(matches!(retry.as_mut().poll(&mut cx), Poll::Ready(false)));
        assert_eq!(context.stop_reason(), Some(StopReason::Timeout));
    }
}

#[cfg(test)]
mod worker_start_tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::thread;

    use super::spawn_domain_workers_with;
    use crate::CommandExecutionConfig;
    use crate::execution::scheduler::BoundedScheduler;

    #[test]
    fn partial_worker_spawn_failure_closes_and_joins_started_workers() {
        let scheduler = BoundedScheduler::new(CommandExecutionConfig {
            max_active: 1,
            max_queued: 0,
            ..Default::default()
        })
        .unwrap();
        let exited = Arc::new(AtomicUsize::new(0));
        let worker_exited = Arc::clone(&exited);
        let result = spawn_domain_workers_with(&scheduler, 2, move |index, worker| {
            if index == 1 {
                return Err(std::io::Error::other("injected worker startup failure"));
            }
            let worker_exited = Arc::clone(&worker_exited);
            thread::Builder::new().spawn(move || {
                worker();
                worker_exited.fetch_add(1, Ordering::SeqCst);
            })
        });

        assert!(result.is_err());
        assert_eq!(exited.load(Ordering::SeqCst), 1);
        assert_eq!(scheduler.snapshot().active, 0);
        assert_eq!(scheduler.snapshot().queued, 0);
    }
}
