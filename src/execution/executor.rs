use super::{
    adapter::AdapterState,
    backend::ProcessBackend,
    contracts::{ExecutionSnapshot, IoPathSnapshot, Request, RequestOptions},
    scheduler::BoundedScheduler,
    *,
};
use crate::{Command, CommandResourceRunOptions, CommandRunOptions, CommandRunner};
use qubit_operation::{
    GroupOptions, OperationContext, OperationManager, OperationOptions, OperationScope,
};
use std::sync::Arc;

impl CommandExecutor {
    pub fn scheduler_snapshot(&self) -> super::scheduler::SchedulerSnapshot {
        self.scheduler.snapshot()
    }
    /// Stops new local reservations and returns a manager handle for observing group shutdown.
    pub fn request_shutdown(
        &self,
        mode: qubit_operation::ShutdownMode,
    ) -> qubit_operation::ShutdownHandle {
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
            None,
            None,
            None,
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
            None,
            None,
            None,
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
                    runner,
                    command,
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
            None,
            None,
            Some(scope.context()),
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
                    runner,
                    command,
                    options,
                    reason: CommandSubmitFailureReason::GroupBinding(e),
                });
            }
        };
        self.submit_async_resources_manager(
            &manager,
            runner,
            command,
            options,
            None,
            None,
            Some(scope.context()),
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
                    runner,
                    command,
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
                    runner,
                    command,
                    options,
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
        let request = make_request(runner, command, RequestOptions::Standard(options)).map_err(
            |(runner, command, options, message)| {
                let RequestOptions::Standard(options) = options else {
                    unreachable!()
                };
                CommandSubmitFailure {
                    runner,
                    command,
                    options,
                    reason: CommandSubmitFailureReason::Snapshot(SnapshotFailure {
                        diagnostic: message,
                    }),
                }
            },
        )?;
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
                    runner,
                    command,
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
            Ok(handle) => Ok(handle),
            Err(error) => {
                let (reason, _) = error.into_parts();
                let request = prepared
                    .restore()
                    .expect("unactivated submission retains request");
                let options = match request.options {
                    RequestOptions::Standard(o) => o,
                    _ => unreachable!(),
                };
                Err(CommandSubmitFailure {
                    runner: request.runner,
                    command: request.command,
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
        let request = make_request(runner, command, RequestOptions::Resources(options)).map_err(
            |(runner, command, options, message)| {
                let RequestOptions::Resources(options) = options else {
                    unreachable!()
                };
                CommandResourceSubmitFailure {
                    runner,
                    command,
                    options,
                    reason: CommandSubmitFailureReason::Snapshot(SnapshotFailure {
                        diagnostic: message,
                    }),
                }
            },
        )?;
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
                    runner,
                    command,
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
            Ok(handle) => Ok(handle),
            Err(error) => {
                let (reason, _) = error.into_parts();
                let request = prepared
                    .restore()
                    .expect("unactivated submission retains request");
                let RequestOptions::Resources(options) = request.options else {
                    unreachable!()
                };
                Err(CommandResourceSubmitFailure {
                    runner: request.runner,
                    command: request.command,
                    options,
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
        bridge: Option<Arc<dyn crate::session::contracts::SessionBridge>>,
        keepalive: Option<super::adapter::SessionOwners>,
        deadline: Option<OperationContext>,
    ) -> Result<super::AsyncCommandOperation, CommandSubmitFailure> {
        let RequestOptions::Standard(options) = request_options else {
            unreachable!()
        };
        let request = make_request(runner, command, RequestOptions::Standard(options)).map_err(
            |(runner, command, options, message)| {
                let RequestOptions::Standard(options) = options else {
                    unreachable!()
                };
                CommandSubmitFailure {
                    runner,
                    command,
                    options,
                    reason: CommandSubmitFailureReason::Snapshot(SnapshotFailure {
                        diagnostic: message,
                    }),
                }
            },
        )?;
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
                    runner,
                    command,
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
            Ok(handle) => Ok(handle),
            Err(error) => {
                let (reason, _) = error.into_parts();
                let request = prepared
                    .restore()
                    .expect("rejected external submission retains request");
                let RequestOptions::Standard(options) = request.options else {
                    unreachable!()
                };
                Err(CommandSubmitFailure {
                    runner: request.runner,
                    command: request.command,
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
        self.start_session_managed_in_manager(
            &manager,
            runner,
            command,
            options,
            Some(scope.context()),
        )
    }
    #[cfg(feature = "async")]
    pub(crate) fn start_session_resources_managed(
        &self,
        runner: CommandRunner,
        command: Command,
        options: CommandResourceRunOptions,
    ) -> Result<crate::session::CommandSession, CommandError> {
        self.start_session_resources_managed_in_manager(
            &self.manager,
            runner,
            command,
            options,
            None,
        )
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
        self.start_session_resources_managed_in_manager(
            &manager,
            runner,
            command,
            options,
            Some(scope.context()),
        )
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
        let channels = crate::session::channel::SessionChannels::new();
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
                Some(bridge),
                Some(super::adapter::SessionOwners {
                    _stdin: stdin_reader,
                    _stdout: stdout_writer,
                    _stderr: stderr_writer,
                }),
                deadline,
            )
            .map_err(|e| {
                managed_failure(format!("managed session admission failed: {:?}", e.reason))
            })?;
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
        let channels = crate::session::channel::SessionChannels::new();
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
                Some(bridge),
                Some(super::adapter::SessionOwners {
                    _stdin: stdin_reader,
                    _stdout: stdout_writer,
                    _stderr: stderr_writer,
                }),
                deadline,
            )
            .map_err(|e| {
                managed_failure(format!(
                    "managed resource session admission failed: {:?}",
                    e.reason
                ))
            })?;
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
        bridge: Option<Arc<dyn crate::session::contracts::SessionBridge>>,
        keepalive: Option<super::adapter::SessionOwners>,
        deadline: Option<OperationContext>,
    ) -> Result<super::AsyncCommandOperation, CommandResourceSubmitFailure> {
        let request = make_request(runner, command, RequestOptions::Resources(options)).map_err(
            |(runner, command, options, message)| {
                let RequestOptions::Resources(options) = options else {
                    unreachable!()
                };
                CommandResourceSubmitFailure {
                    runner,
                    command,
                    options,
                    reason: CommandSubmitFailureReason::Snapshot(SnapshotFailure {
                        diagnostic: message,
                    }),
                }
            },
        )?;
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
                    runner,
                    command,
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
        if let Some(keepalive) = keepalive {
            prepared_state = prepared_state.keep_session_alive(keepalive)
        }
        let prepared = Arc::new(prepared_state);
        match manager
            .as_async()
            .try_submit_external(OperationOptions::default(), Arc::clone(&prepared))
        {
            Ok(handle) => Ok(handle),
            Err(error) => {
                let (reason, _) = error.into_parts();
                let request = prepared
                    .restore()
                    .expect("rejected external submission retains request");
                let RequestOptions::Resources(options) = request.options else {
                    unreachable!()
                };
                Err(CommandResourceSubmitFailure {
                    runner: request.runner,
                    command: request.command,
                    options,
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
            o.configured_stdin()
                .and_then(|f| f.metadata().ok())
                .map(|m| m.len()),
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
        CommandSubmitFailureReason::Capacity(
            CapacityFailure::ActiveFull | CapacityFailure::QueueFull,
        ) => scheduler.wait_for_capacity(bytes, deadline),
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
fn deadline_failure(
    reason: CommandSubmitFailureReason,
    deadline: &OperationContext,
) -> CommandSubmitFailureReason {
    if deadline.is_expired() {
        CommandSubmitFailureReason::Capacity(CapacityFailure::DeadlineExpired)
    } else {
        reason
    }
}
fn async_capacity_retry(reason: &CommandSubmitFailureReason) -> bool {
    matches!(
        reason,
        CommandSubmitFailureReason::Capacity(
            CapacityFailure::ActiveFull | CapacityFailure::QueueFull
        )
    )
}
fn retryable_async(reason: &CommandSubmitFailureReason) -> bool {
    async_capacity_retry(reason)
        || matches!(
            reason,
            CommandSubmitFailureReason::Admission(qubit_operation::AdmissionError::Busy)
        )
}
struct CapacityWait {
    scheduler: BoundedScheduler,
    bytes: usize,
    waiter: Option<u64>,
}
impl std::future::Future for CapacityWait {
    type Output = bool;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<bool> {
        if let Some(id) = self.waiter.take() {
            self.scheduler.cancel_waiter(id);
            return std::task::Poll::Ready(true);
        }
        match self
            .scheduler
            .register_waiter_if_full(self.bytes, cx.waker())
        {
            Ok(None) => std::task::Poll::Ready(true),
            Ok(Some(id)) => {
                self.waiter = Some(id);
                std::task::Poll::Pending
            }
            Err(_) => std::task::Poll::Ready(false),
        }
    }
}
impl Drop for CapacityWait {
    fn drop(&mut self) {
        if let Some(id) = self.waiter.take() {
            self.scheduler.cancel_waiter(id)
        }
    }
}
async fn wait_capacity_async(
    scheduler: BoundedScheduler,
    bytes: usize,
    context: Option<OperationContext>,
) -> bool {
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

struct RetryTimerMessage {
    due: std::time::Instant,
    waker: std::task::Waker,
    ready: Arc<std::sync::atomic::AtomicBool>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}
static RETRY_TIMER: std::sync::OnceLock<Option<std::sync::mpsc::SyncSender<RetryTimerMessage>>> =
    std::sync::OnceLock::new();
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
                            if pending[index]
                                .cancelled
                                .load(std::sync::atomic::Ordering::Acquire)
                            {
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
struct RetryDelay {
    ready: Arc<std::sync::atomic::AtomicBool>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    scheduled: bool,
}
impl RetryDelay {
    fn new() -> Self {
        Self {
            ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            scheduled: false,
        }
    }
}
impl std::future::Future for RetryDelay {
    type Output = bool;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<bool> {
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
impl Drop for RetryDelay {
    fn drop(&mut self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Release)
    }
}
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
        crate::CommandErrorReason::ManagedOperationFailed {
            diagnostic: message,
        },
        None,
    )
}
fn make_request(
    runner: CommandRunner,
    command: Command,
    options: RequestOptions,
) -> Result<Request, (CommandRunner, Command, RequestOptions, String)> {
    let wd = if let Some(path) = runner
        .configured_working_directory()
        .or_else(|| command.working_directory_override())
    {
        path.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(path) => path,
            Err(e) => {
                return Err((
                    runner,
                    command,
                    options,
                    format!("working directory snapshot failed: {e}"),
                ));
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
                        r = runner;
                        c = command;
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
                        r = runner;
                        c = command;
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
                        r = runner;
                        c = command;
                        o = options;
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
                        r = runner;
                        c = command;
                        o = options;
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
                    if wait_retryable_async(self.scheduler.as_ref().clone(), 0, &reason, None).await
                    {
                        r = runner;
                        c = command;
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
                    if wait_retryable_async(self.scheduler.as_ref().clone(), bytes, &reason, None)
                        .await
                    {
                        r = runner;
                        c = command;
                        o = options;
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
                    if wait_retryable_async(
                        self.scheduler.as_ref().clone(),
                        bytes,
                        &reason,
                        Some(context.clone()),
                    )
                    .await
                    {
                        r = runner;
                        c = command;
                        o = options;
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
                    if wait_retryable_async(
                        self.scheduler.as_ref().clone(),
                        0,
                        &reason,
                        Some(context.clone()),
                    )
                    .await
                    {
                        r = runner;
                        c = command;
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
    let recovery = super::adapter::cleanup::start_recovery_worker(config.max_active)
        .map_err(|_| CommandExecutorCreateError::WorkerStartFailed)?;
    for index in 0..config.max_active {
        let s = scheduler.clone();
        std::thread::Builder::new()
            .name(format!("qubit-command-worker-{index}"))
            .spawn(move || {
                while let Some((job, permit)) = s.wait_next() {
                    (job.run)(permit)
                }
            })
            .map_err(|_| CommandExecutorCreateError::WorkerStartFailed)?;
    }
    Ok((scheduler, backend, group, recovery, manager_view))
}

#[cfg(all(test, feature = "async"))]
mod retry_wait_tests {
    use super::wait_retry_tick;
    use qubit_operation::{
        ClockDomainId, ClockSource, ContextOptions, Deadline, MonotonicTime, OperationContext,
        StopReason, TimerRegistration,
    };
    use std::future::Future;
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };
    use std::task::{Context, Poll, Wake, Waker};

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
