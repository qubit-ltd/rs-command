use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;
use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandExecutionConfig;
use qubit_command::CommandExecutor;
use qubit_command::CommandRunOptions;
use qubit_command::CommandRunner;
use qubit_command::CommandSessionApi;
use qubit_command::ManagedCommandRunnerApi;
use qubit_io::AsyncClose;
use qubit_io::AsyncInput;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;
use qubit_operation::OperationOutcome;

pub fn run() {
    block_on(async {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).expect("executor");
        let runner = CommandRunner::without_timeout();
        let command = if cfg!(windows) {
            Command::new("cmd").args(&["/C", "echo out & echo err 1>&2"])
        } else {
            Command::new("sh").args(&["-c", "printf out; printf err >&2"])
        };
        let mut session = runner
            .start_session(&executor, command, CommandRunOptions::new())
            .await
            .expect("start session");
        let mut stdout = session.take_stdout().expect("stdout endpoint");
        let mut stderr = session.take_stderr().expect("stderr endpoint");
        let mut stdin = session.take_stdin().expect("stdin endpoint");
        let operation = session.take_operation().expect("operation handle");

        let (out, err) = std::thread::scope(|threads| {
            let out_reader = threads.spawn(|| block_on(read_all(&mut stdout)));
            let err_reader = threads.spawn(|| block_on(read_all(&mut stderr)));
            block_on(std::future::poll_fn(|cx| Pin::new(&mut stdin).poll_close(cx))).expect("close stdin");
            (
                out_reader.join().expect("stdout reader").expect("stdout read"),
                err_reader.join().expect("stderr reader").expect("stderr read"),
            )
        });

        let mut operation = operation;
        match operation.wait_outcome().expect("command outcome") {
            OperationOutcome::Succeeded(output) => assert_eq!(output.exit_code(), Some(0)),
            other => panic!("command failed: {other:?}"),
        }
        // Endpoints are released before settlement: unread buffered bytes and
        // endpoint references are part of the resource lifetime.
        drop(stdout);
        drop(stderr);
        drop(stdin);
        let settlement = operation.wait_settlement().expect("settlement");
        assert!(out.starts_with(b"out"));
        assert!(err.starts_with(b"err"));
        println!(
            "stdout={}, stderr={}, settlement={:?}",
            out.len(),
            err.len(),
            settlement.status
        );
    });
}

async fn read_all(reader: &mut (impl AsyncInput<Item = u8> + Unpin)) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 128];
    loop {
        let count = reader.read_async(&mut chunk).await?;
        if count == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park_timeout(Duration::from_millis(50)),
        }
    }
}

#[allow(dead_code)]
fn main() {
    run();
}
