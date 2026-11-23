// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fs::File;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use qubit_operation::OperationScope;

use super::identity::validate_files;
use super::identity::validate_tee_paths;
use super::managed_preparation_failure::ManagedPreparationFailure;
use super::prepared_files::register_files;
use super::prepared_files::truncate_sinks;
use crate::execution::contracts::ExecutionSnapshot;
use crate::execution::contracts::FilePreparationFailure;
use crate::execution::contracts::PreparedFiles;
use crate::execution::contracts::ResourceInput;
use crate::resource_run_options::CommandResourceRunOptions;
use crate::resource_run_options::FileSinkPolicy;

impl ResourceInput for CommandResourceRunOptions {
    /// Performs the prepare files step in the managed command execution
    /// lifecycle.
    ///
    /// # Parameters
    /// * `self`: Supplies the self value used by this operation.
    /// * `scope`: Supplies the scope value used by this operation.
    fn prepare_files(self: Box<Self>, scope: &OperationScope) -> Result<PreparedFiles, FilePreparationFailure> {
        (*self).prepare_with_directory(scope, None, true)
    }

    /// Performs the prepare files with snapshot step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `self`: Supplies the self value used by this operation.
    /// * `scope`: Supplies the scope value used by this operation.
    /// * `snapshot`: Supplies the snapshot value used by this operation.
    fn prepare_files_with_snapshot(
        self: Box<Self>,
        scope: &OperationScope,
        snapshot: &ExecutionSnapshot,
    ) -> Result<PreparedFiles, FilePreparationFailure> {
        (*self).prepare_with_directory(scope, Some(&snapshot.working_directory), true)
    }
}

impl CommandResourceRunOptions {
    /// Performs the prepare with directory step in the managed command
    /// execution lifecycle.
    ///
    /// # Parameters
    /// * `scope`: Supplies the scope value used by this operation.
    /// * `working_directory`: Supplies the working directory value used by this
    ///   operation.
    /// * `truncate`: Whether to truncate sinks for the standalone preparation
    ///   API.
    ///
    /// # Returns
    /// Prepared files or the public error retaining recoverable owners.
    fn prepare_with_directory(
        self,
        scope: &OperationScope,
        working_directory: Option<&Path>,
        truncate: bool,
    ) -> Result<PreparedFiles, FilePreparationFailure> {
        self.prepare_internal(scope, working_directory, truncate)
            .map_err(|failure| failure.failure)
    }

    /// Registers the configured files and optionally applies standalone
    /// truncation.
    ///
    /// # Parameters
    /// * `scope`: Operation scope receiving file reservations.
    /// * `working_directory`: Snapshot root for configured tee paths.
    /// * `truncate`: Whether to apply standalone output truncation.
    ///
    /// # Returns
    /// Prepared file owners, or a typed error retaining all recoverable owners.
    fn prepare_internal(
        self,
        scope: &OperationScope,
        working_directory: Option<&Path>,
        truncate: bool,
    ) -> Result<PreparedFiles, ManagedPreparationFailure> {
        let parts = self.into_parts();
        let options = parts.options;
        let mut input = parts.stdin;
        let stdout = parts.stdout.map(|sink| (sink.file, sink.policy));
        let stderr = parts.stderr.map(|sink| (sink.file, sink.policy));

        let mut all = Vec::new();
        if let Some(file) = input.as_ref() {
            all.push(("stdin", file));
        }
        if let Some((file, _)) = stdout.as_ref() {
            all.push(("stdout", file));
        }
        if let Some((file, _)) = stderr.as_ref() {
            all.push(("stderr", file));
        }
        let files: Vec<(&'static str, &File)> = all;
        let base = match working_directory {
            Some(directory) => directory.to_path_buf(),
            None => match std::env::current_dir() {
                Ok(directory) => directory,
                Err(error) => {
                    return Err(managed_failure(
                        error,
                        crate::FilePreparationStage::Validation,
                        None,
                        None,
                        Some(restore_input(options, input, stdout, stderr)),
                    ));
                }
            },
        };
        let tee_paths: Vec<(&'static str, PathBuf)> = [
            options
                .configured_stdout_file()
                .map(|path| ("stdout", super::identity::resolve_io_path(&base, path))),
            options
                .configured_stderr_file()
                .map(|path| ("stderr", super::identity::resolve_io_path(&base, path))),
        ]
        .into_iter()
        .flatten()
        .collect();
        if let Err(error) = validate_files(&files).and_then(|()| {
            let paths: Vec<_> = tee_paths.iter().map(|(name, path)| (*name, path.as_path())).collect();
            validate_tee_paths(&files, &paths)
        }) {
            let direction = preparation_direction(&error);
            return Err(managed_failure(
                error,
                crate::FilePreparationStage::Validation,
                direction,
                None,
                Some(restore_input(options, input, stdout, stderr)),
            ));
        }

        // Reserve every manager cleanup slot before registration or any truncation.
        let mut reservations = Vec::with_capacity(files.len());
        for _ in 0..files.len() {
            match scope.reserve_resource() {
                Ok(reservation) => reservations.push(reservation),
                Err(error) => {
                    let kind = format!("resource capacity reservation failed: {error:?}");
                    return Err(managed_failure(
                        io::Error::other(kind),
                        crate::FilePreparationStage::ResourceReservation,
                        None,
                        None,
                        Some(restore_input(options, input, stdout, stderr)),
                    ));
                }
            }
        }

        let stdout_policy = stdout.as_ref().map(|(_, policy)| *policy);
        let stderr_policy = stderr.as_ref().map(|(_, policy)| *policy);
        let registered = register_files(scope, &mut reservations, input.take(), stdout, stderr);
        let prepared = match registered {
            Ok(prepared) => prepared,
            Err(error) => {
                let (error, recovered, mut raw) = *error;
                let raw_input = restore_input(
                    options,
                    raw[0].take(),
                    raw[1].take().zip(stdout_policy),
                    raw[2].take().zip(stderr_policy),
                );
                return Err(managed_failure(
                    error,
                    crate::FilePreparationStage::Registration,
                    None,
                    recovered,
                    Some(raw_input),
                ));
            }
        };
        if truncate && let Err(error) = truncate_sinks(&prepared, scope) {
            return Err(managed_failure(
                error,
                crate::FilePreparationStage::Truncation,
                None,
                Some(prepared),
                None,
            ));
        }
        Ok(prepared)
    }

    /// Prepares managed files without applying output side effects. The runner
    /// commits truncation after its final start authorization.
    ///
    /// # Parameters
    /// * `scope`: Operation scope that owns prepared file resources.
    /// * `snapshot`: Submission-time path snapshot used for tee identity
    ///   checks.
    /// * `session_stdin`: Whether the command uses a session stdin pipe.
    ///
    /// # Returns
    /// Registered files or a typed failure retaining all recoverable owners.
    pub(crate) fn prepare_for_managed(
        self,
        scope: &OperationScope,
        snapshot: &ExecutionSnapshot,
        session_stdin: bool,
    ) -> Result<PreparedFiles, ManagedPreparationFailure> {
        if session_stdin && self.configured_stdin().is_some() {
            let source = io::Error::new(
                io::ErrorKind::InvalidInput,
                "session stdin cannot be combined with an owned stdin file",
            );
            return Err(managed_failure(
                source,
                crate::FilePreparationStage::Validation,
                Some(crate::FilePreparationDirection::Stdin),
                None,
                Some(Box::new(self) as Box<dyn ResourceInput>),
            ));
        }
        let value = self;
        let has_owned_output = value.configured_stdout().is_some() || value.configured_stderr().is_some();
        let has_path_output = value.ordinary_options().configured_stdout_file().is_some()
            || value.ordinary_options().configured_stderr_file().is_some();
        if has_owned_output && has_path_output {
            return Err(managed_failure(
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "owned sinks cannot be combined with path tees",
                ),
                crate::FilePreparationStage::Validation,
                None,
                None,
                Some(Box::new(value) as Box<dyn ResourceInput>),
            ));
        }
        value.prepare_internal(scope, Some(&snapshot.working_directory), false)
    }
}

/// Builds a typed preparation failure and its public recovery carrier.
///
/// # Parameters
/// * `error`: Supplies the error value used by this operation.
/// * `prepared`: Supplies the prepared value used by this operation.
/// * `input`: Supplies the input value used by this operation.
/// * `phase`: Preparation phase that failed.
/// * `direction`: Affected stream, when known.
///
/// # Returns
/// A failure containing the original source and retained recovery owners.
fn managed_failure(
    error: io::Error,
    phase: crate::FilePreparationStage,
    direction: Option<crate::FilePreparationDirection>,
    prepared: Option<PreparedFiles>,
    input: Option<Box<dyn ResourceInput>>,
) -> ManagedPreparationFailure {
    let message = error.to_string();
    ManagedPreparationFailure {
        phase,
        direction,
        source: error,
        failure: FilePreparationFailure {
            message,
            input,
            prepared: prepared.map(Box::new),
        },
    }
}

/// Infers a stream from a validation error's stable safe prefix.
///
/// # Parameters
/// * `error`: Validation error returned by the file identity checks.
///
/// # Returns
/// The direction named by the validation message, when available.
fn preparation_direction(error: &io::Error) -> Option<crate::FilePreparationDirection> {
    let message = error.to_string();
    if message.starts_with("stdin") {
        Some(crate::FilePreparationDirection::Stdin)
    } else if message.starts_with("stdout") {
        Some(crate::FilePreparationDirection::Stdout)
    } else if message.starts_with("stderr") {
        Some(crate::FilePreparationDirection::Stderr)
    } else {
        None
    }
}

/// Performs the restore input step in the managed command execution lifecycle.
///
/// # Parameters
/// * `options`: Supplies the options value used by this operation.
/// * `stdin`: Supplies the stdin value used by this operation.
/// * `stdout`: Supplies the stdout value used by this operation.
fn restore_input(
    options: crate::CommandRunOptions,
    stdin: Option<File>,
    stdout: Option<(File, FileSinkPolicy)>,
    stderr: Option<(File, FileSinkPolicy)>,
) -> Box<dyn ResourceInput> {
    let mut restored = CommandResourceRunOptions::new().options(options);
    if let Some(file) = stdin {
        restored = restored.stdin_file(file);
    }
    if let Some((file, policy)) = stdout {
        restored = restored.stdout_sink(file, policy);
    }
    if let Some((file, policy)) = stderr {
        restored = restored.stderr_sink(file, policy);
    }
    Box::new(restored)
}

/// Stores the  policy is exhaustive value used by this execution component.
///
/// # Parameters
/// * `policy`: Supplies the policy value used by this operation.
#[allow(dead_code)]
fn _policy_is_exhaustive(policy: FileSinkPolicy) {
    match policy {
        FileSinkPolicy::Preserve | FileSinkPolicy::Truncate => {}
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::fs::OpenOptions;
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::Duration;

    use qubit_operation::GroupOptions;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::ScopeLimits;

    use super::ResourceInput;
    use crate::CommandRunOptions;
    use crate::execution::contracts::ExecutionSnapshot;
    use crate::resource_run_options::CommandResourceRunOptions;
    use crate::resource_run_options::FileSinkPolicy;
    static PREPARATION_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_file_sink_policy_exhaustiveness_covers_every_supported_policy() {
        super::_policy_is_exhaustive(FileSinkPolicy::Preserve);
        super::_policy_is_exhaustive(FileSinkPolicy::Truncate);
    }

    #[test]
    fn test_managed_reservation_failure_keeps_typed_stage_and_owner() {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager should start");
        let output = tempfile::NamedTempFile::new().expect("sink should be created");
        std::fs::write(output.path(), b"must survive").expect("sink should be populated");
        let sink_path = output.path().to_path_buf();
        let outcome = manager
            .run(
                OperationOptions {
                    limits: ScopeLimits {
                        resources: 0,
                        ..ScopeLimits::default()
                    },
                    ..OperationOptions::default()
                },
                move |scope| {
                    let file = OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(&sink_path)
                        .expect("sink should open");
                    let error = match CommandResourceRunOptions::new()
                        .stdout_sink(file, FileSinkPolicy::Truncate)
                        .prepare_for_managed(&scope, &ExecutionSnapshot::default(), false)
                    {
                        Ok(_) => panic!("zero resource capacity must reject preparation"),
                        Err(error) => error,
                    };
                    assert_eq!(error.phase, crate::FilePreparationStage::ResourceReservation);
                    assert_eq!(error.direction, None);
                    assert!(error.failure.input.is_some());
                    assert!(error.failure.prepared.is_none());
                    assert_eq!(error.source.kind(), std::io::ErrorKind::Other);
                    assert_eq!(std::fs::read(&sink_path).unwrap(), b"must survive");
                    Ok::<_, std::convert::Infallible>(())
                },
            )
            .expect("preparation operation should finish");
        assert!(matches!(outcome, qubit_operation::OperationOutcome::Succeeded(())));
    }

    #[test]
    fn test_helper_resource_reference_delays_scope_settlement_until_dropped() {
        let _guard = PREPARATION_TEST_LOCK.lock().expect("test lock should be available");
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager should start");
        let group = manager.group(GroupOptions { capacity: 2 }).expect("group should start");
        let (send, receive) = mpsc::sync_channel(1);
        let input = tempfile::tempfile().expect("stdin file should be created");
        let mut handle = group
            .submit(OperationOptions::default(), move |scope| {
                let prepared = Box::new(CommandResourceRunOptions::new().stdin_file(input))
                    .prepare_files(&scope)
                    .expect("stdin should be registered");
                send.send(prepared.stdin.as_ref().expect("stdin owner should exist").clone())
                    .expect("test should receive retained helper reference");
                Ok::<_, ()>(())
            })
            .expect("operation should be accepted");
        let helper_reference = receive
            .recv_timeout(Duration::from_secs(5))
            .expect("helper reference should arrive");
        assert!(matches!(
            handle.wait_outcome(),
            Ok(qubit_operation::OperationOutcome::Succeeded(()))
        ));
        let pending = handle
            .wait_settlement_for(Duration::from_millis(20))
            .expect("settlement observation should work");
        assert!(matches!(pending, qubit_operation::SettlementWait::Pending(_)));
        drop(helper_reference);
        let settled = handle
            .wait_settlement_for(Duration::from_secs(5))
            .expect("settlement observation should work");
        assert!(matches!(settled, qubit_operation::SettlementWait::Ready(_)));
    }

    #[test]
    fn test_commit_failure_retains_registered_and_unregistered_file_owners() {
        let _guard = PREPARATION_TEST_LOCK.lock().expect("test lock should be available");
        use std::sync::Arc;
        use std::sync::Barrier;

        let manager = OperationManager::new(ManagerOptions::default()).expect("manager should start");
        let group = manager.group(GroupOptions { capacity: 4 }).expect("group should start");
        let gate = Arc::new((Barrier::new(2), Barrier::new(2)));
        let operation_gate = Arc::clone(&gate);
        let first = tempfile::NamedTempFile::new().expect("first file should be created");
        let second = tempfile::NamedTempFile::new().expect("second file should be created");
        let (send, receive) = mpsc::sync_channel(1);
        let mut handle = group
            .submit(OperationOptions::default(), move |scope| {
                super::super::prepared_files::gate_after_first_registration(Some((scope.id(), operation_gate)));
                let failure = Box::new(
                    CommandResourceRunOptions::new()
                        .stdout_sink(
                            File::open(first.path()).expect("first file should open"),
                            FileSinkPolicy::Preserve,
                        )
                        .stderr_sink(
                            File::open(second.path()).expect("second file should open"),
                            FileSinkPolicy::Preserve,
                        ),
                )
                .prepare_files(&scope)
                .expect_err("stop between commits should reject the second commit");
                send.send(failure).expect("failure should reach test thread");
                Ok::<_, ()>(())
            })
            .expect("operation should be accepted");

        gate.0.wait();
        assert_eq!(
            handle.request_stop(qubit_operation::StopReason::UserCancelled),
            qubit_operation::StopRequestStatus::Accepted
        );
        gate.1.wait();
        let mut failure = receive
            .recv_timeout(Duration::from_secs(5))
            .expect("preparation failure should arrive");
        super::super::prepared_files::gate_after_first_registration(None);
        assert!(failure.prepared.is_some(), "first committed owner must be retained");
        assert!(failure.input.is_some(), "second uncommitted owner must be recoverable");
        let _ = handle.wait_outcome();

        let recovered_input = failure.input.take().expect("raw second file owner should be returned");
        let recovery_manager = OperationManager::new(ManagerOptions::default()).expect("recovery manager should start");
        let recovery_group = recovery_manager
            .group(GroupOptions { capacity: 2 })
            .expect("recovery group should start");
        let completion = recovery_group
            .run_and_settle(OperationOptions::default(), move |scope| {
                let prepared = recovered_input
                    .prepare_files(&scope)
                    .expect("unregistered owner should be retryable in a live scope");
                Ok::<_, ()>(format!("{prepared:?}"))
            })
            .expect("recovery operation should be accepted");
        assert!(matches!(
            completion.outcome,
            qubit_operation::OperationOutcome::Succeeded(ref state)
                if state.contains("stderr: true") && state.contains("stdout: false")
        ));
    }

    #[test]
    fn test_snapshot_directory_rejects_relative_stderr_alias_without_truncating_sink() {
        let _guard = PREPARATION_TEST_LOCK.lock().expect("test lock should be available");
        let snapshot_dir = tempfile::tempdir().expect("snapshot directory should be created");
        let output = tempfile::NamedTempFile::new().expect("stderr sink should be created");
        std::fs::write(output.path(), b"must survive").expect("sink should be populated");
        let output_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(output.path())
            .expect("sink should be opened");
        let alias_name = format!("stderr-sink-alias-{}", std::process::id());
        let alias_path = snapshot_dir.path().join(&alias_name);
        std::fs::hard_link(output.path(), &alias_path).expect("snapshot alias should be linked");
        assert!(
            !Path::new(&alias_name).exists(),
            "snapshot path should be resolved explicitly"
        );
        let output_path = output.path().to_path_buf();
        let snapshot = ExecutionSnapshot {
            working_directory: snapshot_dir.path().to_path_buf(),
            environment: Vec::new(),
        };

        let manager = OperationManager::new(ManagerOptions::default()).expect("manager should start");
        let group = manager.group(GroupOptions { capacity: 2 }).expect("group should start");
        let completion = group
            .run_and_settle(OperationOptions::default(), move |scope| {
                let failure = Box::new(
                    CommandResourceRunOptions::new()
                        .options(CommandRunOptions::new().tee_stderr_to_file(alias_name))
                        .stderr_sink(output_file, FileSinkPolicy::Truncate),
                )
                .prepare_files_with_snapshot(&scope, &snapshot)
                .expect_err("snapshot-relative stderr alias must be rejected");
                assert!(failure.input.is_some(), "the stderr owner must remain recoverable");
                assert!(failure.prepared.is_none(), "validation must fail before registration");
                assert_eq!(std::fs::read(output_path).unwrap(), b"must survive");
                Ok::<_, ()>(())
            })
            .expect("operation should be accepted");
        assert!(matches!(
            completion.outcome,
            qubit_operation::OperationOutcome::Succeeded(())
        ));
    }
}
