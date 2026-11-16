use crate::execution::contracts::{FilePreparationFailure, PreparedFiles, ResourceInput};
use crate::resource_run_options::{CommandResourceRunOptions, FileSinkPolicy};
use qubit_operation::OperationScope;
use std::fs::File;
use std::io;

use super::identity::{validate_files, validate_tee_paths};
use super::prepared_files::{register_files, truncate_sinks};

impl ResourceInput for CommandResourceRunOptions {
    fn prepare_files(
        self: Box<Self>,
        scope: &OperationScope,
    ) -> Result<PreparedFiles, FilePreparationFailure> {
        let parts = (*self).into_parts();
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
        let tee_paths: Vec<(&'static str, &std::path::Path)> = [
            options
                .configured_stdout_file()
                .map(|path| ("stdout", path)),
            options
                .configured_stderr_file()
                .map(|path| ("stderr", path)),
        ]
        .into_iter()
        .flatten()
        .collect();
        if let Err(error) =
            validate_files(&files).and_then(|()| validate_tee_paths(&files, &tee_paths))
        {
            return Err(failure(
                error,
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
                    return Err(failure(
                        io::Error::other(kind),
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
            Err((error, recovered, mut raw)) => {
                let raw_input = restore_input(
                    options,
                    raw[0].take(),
                    raw[1].take().zip(stdout_policy),
                    raw[2].take().zip(stderr_policy),
                );
                return Err(failure(error, recovered, Some(raw_input)));
            }
        };
        if let Err(error) = truncate_sinks(&prepared, scope) {
            return Err(failure(error, Some(prepared), None));
        }
        Ok(prepared)
    }
}

fn failure(
    error: io::Error,
    prepared: Option<PreparedFiles>,
    input: Option<Box<dyn ResourceInput>>,
) -> FilePreparationFailure {
    FilePreparationFailure {
        message: error.to_string(),
        input,
        prepared,
    }
}

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

#[allow(dead_code)]
fn _policy_is_exhaustive(policy: FileSinkPolicy) {
    match policy {
        FileSinkPolicy::Preserve | FileSinkPolicy::Truncate => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qubit_operation::{GroupOptions, ManagerOptions, OperationManager, OperationOptions};
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn helper_resource_reference_delays_scope_settlement_until_dropped() {
        let manager =
            OperationManager::new(ManagerOptions::default()).expect("manager should start");
        let group = manager
            .group(GroupOptions { capacity: 2 })
            .expect("group should start");
        let (send, receive) = mpsc::sync_channel(1);
        let input = tempfile::tempfile().expect("stdin file should be created");
        let mut handle = group
            .submit(OperationOptions::default(), move |scope| {
                let prepared = Box::new(CommandResourceRunOptions::new().stdin_file(input))
                    .prepare_files(&scope)
                    .expect("stdin should be registered");
                send.send(
                    prepared
                        .stdin
                        .as_ref()
                        .expect("stdin owner should exist")
                        .clone(),
                )
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
        assert!(matches!(
            pending,
            qubit_operation::SettlementWait::Pending(_)
        ));
        drop(helper_reference);
        let settled = handle
            .wait_settlement_for(Duration::from_secs(5))
            .expect("settlement observation should work");
        assert!(matches!(settled, qubit_operation::SettlementWait::Ready(_)));
    }
}
