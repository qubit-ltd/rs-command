#![cfg(feature = "async")]

use qubit_command::{
    Command, CommandExecutionConfig, CommandExecutor, CommandRunner, ManagedCommandRunnerApi,
    session::CommandSessionApi,
};
use qubit_io::{AsyncClose, AsyncInput};
use qubit_operation::{ManagerOptions, OperationManager, OperationOutcome};
use std::time::Duration;
use std::{
    future::poll_fn,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread,
};

struct ThreadWake(thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark()
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark()
    }
}
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
        thread::park_timeout(Duration::from_secs(2));
    }
}

#[cfg(unix)]
#[test]
fn managed_session_returns_endpoints_and_streams_both_pipes() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).unwrap();
    let runner = CommandRunner::new(Duration::from_secs(5));
    let mut session = block_on(
        runner.start_session(
            &executor,
            Command::new("sh")
                .arg("-c")
                .arg("printf out; printf err >&2"),
            Default::default(),
        ),
    )
    .unwrap();
    let mut stdin = session.take_stdin().unwrap();
    block_on(poll_fn(|cx| Pin::new(&mut stdin).poll_close(cx))).unwrap();
    drop(stdin);
    let mut stdout = session.take_stdout().unwrap();
    let mut stderr = session.take_stderr().unwrap();
    let output = block_on(async move {
        let out = async {
            let mut all = Vec::new();
            let mut buf = [0; 128];
            loop {
                let n = poll_fn(|cx| Pin::new(&mut stdout).poll_read(cx, &mut buf))
                    .await
                    .unwrap();
                if n == 0 {
                    break;
                }
                all.extend_from_slice(&buf[..n]);
            }
            all
        };
        let err = async {
            let mut all = Vec::new();
            let mut buf = [0; 128];
            loop {
                let n = poll_fn(|cx| Pin::new(&mut stderr).poll_read(cx, &mut buf))
                    .await
                    .unwrap();
                if n == 0 {
                    break;
                }
                all.extend_from_slice(&buf[..n]);
            }
            all
        };
        futures_join(out, err).await
    });
    assert_eq!(output.0, b"out");
    assert_eq!(output.1, b"err");
    let mut operation = session.take_operation().unwrap();
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Succeeded(_)
    ));
    operation.wait_settlement().unwrap();
}

async fn futures_join<A: std::future::Future, B: std::future::Future>(
    a: A,
    b: B,
) -> (A::Output, B::Output) {
    let mut a = Box::pin(a);
    let mut b = Box::pin(b);
    let mut ao = None;
    let mut bo = None;
    poll_fn(|cx| {
        if ao.is_none() {
            if let Poll::Ready(x) = a.as_mut().poll(cx) {
                ao = Some(x)
            }
        }
        if bo.is_none() {
            if let Poll::Ready(x) = b.as_mut().poll(cx) {
                bo = Some(x)
            }
        }
        if ao.is_some() && bo.is_some() {
            Poll::Ready((ao.take().unwrap(), bo.take().unwrap()))
        } else {
            Poll::Pending
        }
    })
    .await
}
