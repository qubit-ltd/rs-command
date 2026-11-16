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
use qubit_io::AsyncOutput;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;
use qubit_operation::OperationOutcome;
use qubit_operation::SettlementStatus;
use qubit_operation::ShutdownMode;
use qubit_operation::ShutdownWait;

pub fn run() {
    block_on(async {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = CommandExecutor::new(manager, CommandExecutionConfig::default()).expect("executor");
        let runner = CommandRunner::without_timeout();
        // Keep stdin open so the process remains active across the zero-time
        // shutdown snapshot. `more` consumes the redirected pipe on Windows.
        let command = if cfg!(windows) {
            Command::new("cmd").args(&["/C", "more"])
        } else {
            Command::new("cat")
        };
        let mut session = runner
            .start_session(&executor, command, CommandRunOptions::new())
            .await
            .expect("start controlled session");
        let mut stdin = session.take_stdin().expect("stdin endpoint");
        let mut stdout = session.take_stdout().expect("stdout endpoint");
        let mut stderr = session.take_stderr().expect("stderr endpoint");
        let mut operation = session.take_operation().expect("operation handle");

        // Wait for the executor's admission snapshot to confirm active work;
        // an operation body can be Running while its domain request is queued.
        loop {
            if executor.scheduler_snapshot().active > 0 {
                break;
            }
            std::thread::yield_now();
        }

        // Deliver a small request while the session is active and wait until
        // output comes back. This confirms the child has started and consumed
        // input; keep stdin open so it remains gated until after Pending.
        stdin
            .write_async(b"hello from the application\n")
            .await
            .expect("write session input");
        let mut out_prefix = [0_u8; 128];
        let out_count = stdout
            .read_async(&mut out_prefix)
            .await
            .expect("read initial child output");
        assert!(out_count > 0, "child must respond before shutdown begins");

        let shutdown = executor.request_shutdown(ShutdownMode::Drain);
        match shutdown
            .wait_for(Duration::ZERO)
            .expect("nonblocking shutdown snapshot")
        {
            ShutdownWait::Pending(snapshot) => {
                println!("shutdown is pending while stdin remains open: {snapshot:?}");
            }
            ShutdownWait::Completed(_) => {
                panic!("controlled session should keep shutdown pending until stdin closes");
            }
        }

        let (mut out, err) = std::thread::scope(|threads| {
            let out_reader = threads.spawn(|| block_on(read_all(&mut stdout)));
            let err_reader = threads.spawn(|| block_on(read_all(&mut stderr)));
            block_on(std::future::poll_fn(|cx| Pin::new(&mut stdin).poll_close(cx))).expect("close stdin");
            (
                out_reader.join().expect("stdout reader").expect("stdout read"),
                err_reader.join().expect("stderr reader").expect("stderr read"),
            )
        });
        let mut output = out_prefix[..out_count].to_vec();
        output.append(&mut out);
        let out = output;

        match operation.wait_outcome().expect("command outcome") {
            OperationOutcome::Succeeded(output) => assert_eq!(output.exit_code(), Some(0)),
            other => panic!("streaming command did not succeed: {other:?}"),
        }
        drop(stdin);
        drop(stdout);
        drop(stderr);
        let settlement = operation.wait_settlement().expect("operation settlement");
        match settlement.status {
            SettlementStatus::RecoveryRequired => {
                eprintln!("{} resource(s) need recovery", settlement.recovery_count);
                for recovery in settlement.recoveries.iter() {
                    eprintln!("recovery handle: {recovery:?}");
                }
            }
            status => println!("resource settlement: {status:?}"),
        }
        assert!(out.starts_with(b"hello from the application"));

        // Continue observing the same shutdown handle after the gate has been
        // released by stdin EOF and endpoint settlement.
        let completed = shutdown.wait().expect("shutdown completion");
        println!(
            "shutdown complete: {:?}; stderr bytes: {}",
            completed.snapshot,
            err.len()
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
