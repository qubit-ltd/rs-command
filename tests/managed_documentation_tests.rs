#![cfg(feature = "async")]

#[path = "../examples/managed_collect.rs"]
mod managed_collect;
#[path = "../examples/managed_file.rs"]
mod managed_file;
#[path = "../examples/managed_session.rs"]
mod managed_session;
#[path = "../examples/managed_shutdown.rs"]
mod managed_shutdown;

#[test]
fn managed_collect_example_runs() {
    managed_collect::run();
}

#[test]
fn managed_session_example_runs() {
    managed_session::run();
}

#[test]
fn managed_shutdown_example_runs() {
    managed_shutdown::run();
}

#[test]
fn managed_file_example_runs() {
    managed_file::run();
}
