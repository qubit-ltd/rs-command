use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandExecutionConfig;
use qubit_command::CommandExecutor;
use qubit_command::CommandRunOptions;
use qubit_command::CommandRunner;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;
use qubit_operation::OperationOptions;
use qubit_operation::OperationOutcome;

pub fn run() {
    let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
    let executor = CommandExecutor::new(manager.clone(), CommandExecutionConfig::default()).expect("executor");
    let child_executor = executor.clone();
    let child_runner = CommandRunner::without_timeout();

    // The parent scope submits the command as its child and returns its handle.
    let mut parent = manager
        .submit::<qubit_command::CommandOperation, String, _>(OperationOptions::default(), move |scope| {
            child_executor
                .submit_in(
                    &scope,
                    child_runner,
                    Command::new("rustc").arg("--version"),
                    CommandRunOptions::new(),
                )
                .map_err(|failure| format!("submission rejected: {:?}", failure.reason))
        })
        .expect("submit parent operation");

    let command = match parent.wait_outcome().expect("parent outcome") {
        OperationOutcome::Succeeded(handle) => handle,
        _ => panic!("parent did not return the command handle"),
    };
    // The typed outcome receiver can move between sync and async interfaces.
    let mut async_command = command.into_async();
    let outcome = block_on(async { async_command.wait_outcome().await }).expect("command outcome");
    let command = async_command.into_sync();
    let settlement = command.wait_settlement().expect("command settlement");

    match outcome {
        OperationOutcome::Succeeded(output) => {
            assert!(!output.stdout().is_empty());
            println!("{}", String::from_utf8_lossy(output.stdout()));
        }
        other => panic!("command failed: {other:?}"),
    }
    println!("settlement: {:?}", settlement.status);
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::sync::Arc;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;
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
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park_timeout(Duration::from_millis(50)),
        }
    }
}

#[allow(dead_code)]
fn main() {
    run();
}
