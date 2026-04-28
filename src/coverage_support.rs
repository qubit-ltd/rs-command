/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Coverage-only hooks for exercising defensive process-runner branches.

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(windows)]
use std::os::windows::process::ExitStatusExt;
use std::{
    cell::{Cell, RefCell},
    ffi::OsStr,
    process::ExitStatus,
};

mod defensive_paths;
mod failing_flush;
mod failing_reader;
mod failing_write;
mod fake_child_guard;
mod no_stdin_child;
mod synthetic_children;

use failing_flush::FailingFlush;
use failing_reader::FailingReader;
use failing_write::FailingWrite;
use fake_child_guard::FakeChildGuard;
use no_stdin_child::NoStdinChild;

use crate::{OutputStream, command_runner::managed_child_process::ManagedChildProcess};

thread_local! {
    /// Whether synthetic children are enabled on this test thread.
    static FAKE_CHILDREN_ENABLED: Cell<bool> = const { Cell::new(false) };
    /// Commands whose output collection path has been reached.
    static COLLECT_OUTPUT_COMMANDS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Runs an operation with coverage-only synthetic children enabled.
///
/// # Parameters
///
/// * `operation` - Operation that may run magic coverage-only command names.
///
/// # Returns
///
/// The value returned by `operation`.
pub fn with_fake_children_enabled<T>(operation: impl FnOnce() -> T) -> T {
    let _guard = enable_fake_children();
    operation()
}

/// Returns whether coverage-only synthetic children are enabled.
///
/// # Returns
///
/// `true` only within [`with_fake_children_enabled`] on the current thread.
pub(crate) fn fake_children_enabled() -> bool {
    FAKE_CHILDREN_ENABLED.get()
}

/// Records that output collection was reached for a command.
///
/// # Parameters
///
/// * `command` - Human-readable command text passed to output collection.
pub(crate) fn record_collect_output(command: &str) {
    COLLECT_OUTPUT_COMMANDS.with_borrow_mut(|commands| commands.push(command.to_owned()));
}

/// Takes and clears recorded output-collection commands.
///
/// # Returns
///
/// Recorded command texts since the previous call on this thread.
pub fn take_collect_output_commands() -> Vec<String> {
    COLLECT_OUTPUT_COMMANDS.take()
}

/// Enables coverage-only synthetic children for the current thread.
///
/// # Returns
///
/// Guard restoring the previous state when dropped.
fn enable_fake_children() -> FakeChildGuard {
    let previous = fake_children_enabled();
    FAKE_CHILDREN_ENABLED.set(true);
    FakeChildGuard { previous }
}

/// Exercises internal error helpers that cannot be reached reliably through
/// real OS process execution.
///
/// # Returns
///
/// Diagnostic strings built from each exercised error path.
pub fn exercise_defensive_paths() -> Vec<String> {
    defensive_paths::exercise_defensive_paths()
}

/// Creates a successful exit status for coverage-only helper calls.
fn success_status() -> ExitStatus {
    ExitStatus::from_raw(0)
}

/// Creates a synthetic child for coverage-only run-loop branches.
///
/// # Parameters
///
/// * `program` - Program name passed to the process command.
///
/// # Returns
///
/// A synthetic child for known coverage-only program names, otherwise `None` so
/// normal process spawning proceeds.
pub(crate) fn fake_child_for(program: &OsStr) -> Option<ManagedChildProcess> {
    synthetic_children::fake_child_for(program)
}

/// Checks whether output collection should fail for a synthetic command.
///
/// # Parameters
///
/// * `command` - Human-readable command text built by the runner.
///
/// # Returns
///
/// The stream to report as failed for known synthetic command names, otherwise
/// `None`.
pub(crate) fn forced_collect_output_error(command: &str) -> Option<OutputStream> {
    synthetic_children::forced_collect_output_error(command)
}
