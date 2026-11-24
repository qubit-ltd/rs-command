// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fs::File;
use std::io;
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use std::sync::Barrier;
use std::sync::Mutex;
#[cfg(test)]
use std::sync::OnceLock;

use qubit_operation::OperationScope;
use qubit_operation::ResourceRef;

use super::registration_category::RegistrationCategory;
use crate::execution::contracts::PreparedFiles;
use crate::execution::contracts::PreparedSink;
use crate::resource_run_options::FileSinkPolicy;

#[cfg(test)]
pub(crate) type RegistrationGate = (qubit_operation::OperationId, Arc<(Barrier, Barrier)>);
#[cfg(test)]
type RegistrationGateSlot = OnceLock<Mutex<Option<RegistrationGate>>>;
#[cfg(test)]
static AFTER_FIRST_REGISTRATION: RegistrationGateSlot = OnceLock::new();

/// Stores the registration failure value used by this execution component.
type RegistrationFailure = (
    io::Error,
    crate::FilePreparationDirection,
    Option<PreparedFiles>,
    [Option<File>; 3],
);

#[cfg(test)]
pub(crate) fn gate_after_first_registration(gate: Option<RegistrationGate>) {
    *AFTER_FIRST_REGISTRATION
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("test registration gate lock should not poison") = gate;
}

/// Performs the register files step in the managed command execution lifecycle.
///
/// # Parameters
/// * `_scope`: Operation scope that owns all file reservations.
/// * `reservations`: Mutable list receiving the successfully registered
///   reservations.
/// * `stdin`: Optional file used as command input.
/// * `stdout`: Optional output file and its sink policy.
/// * `stderr`: Optional error file and its sink policy.
pub(super) fn register_files(
    _scope: &OperationScope,
    reservations: &mut Vec<qubit_operation::ResourceReservation>,
    stdin: Option<File>,
    stdout: Option<(File, FileSinkPolicy)>,
    stderr: Option<(File, FileSinkPolicy)>,
) -> Result<PreparedFiles, Box<RegistrationFailure>> {
    let (stdout_file, stdout_policy) = stdout.map_or((None, None), |(file, policy)| (Some(file), Some(policy)));
    let (stderr_file, stderr_policy) = stderr.map_or((None, None), |(file, policy)| (Some(file), Some(policy)));
    let mut values = [stdin, stdout_file, stderr_file];
    let policies = [None, stdout_policy, stderr_policy];
    let mut refs: [Option<ResourceRef<Mutex<File>>>; 3] = [None, None, None];
    for index in 0..3 {
        let Some(file) = values[index].take() else {
            continue;
        };
        let Some(reservation) = reservations.pop() else {
            values[index] = Some(file);
            return Err(Box::new((
                io::Error::other("missing reserved resource slot"),
                file_direction(index),
                partial(refs, policies),
                values,
            )));
        };
        match reservation.commit(Mutex::new(file)) {
            Ok(reference) => {
                refs[index] = Some(reference);
                #[cfg(test)]
                if refs.iter().filter(|reference| reference.is_some()).count() == 1 {
                    let gate = AFTER_FIRST_REGISTRATION
                        .get_or_init(|| Mutex::new(None))
                        .lock()
                        .expect("test registration gate lock should not poison")
                        .clone();
                    if let Some((operation_id, gate)) = gate
                        && _scope.id() == operation_id
                    {
                        gate.0.wait();
                        gate.1.wait();
                    }
                }
            }
            Err(error) => {
                let (category, recovered_file, _) = error.into_parts();
                values[index] = Some(recovered_file.into_inner().unwrap_or_else(|e| e.into_inner()));
                return Err(Box::new((
                    io::Error::other(RegistrationCategory(category)),
                    file_direction(index),
                    partial(refs, policies),
                    values,
                )));
            }
        }
    }
    Ok(build(refs, policies))
}

/// Returns the diagnostic direction associated with a stream's registration
/// slot.
///
/// # Parameters
///
/// * `index` - Position in the stdin, stdout, stderr registration array.
fn file_direction(index: usize) -> crate::FilePreparationDirection {
    match index {
        0 => crate::FilePreparationDirection::Stdin,
        1 => crate::FilePreparationDirection::Stdout,
        2 => crate::FilePreparationDirection::Stderr,
        _ => unreachable!("only the three standard streams are registered"),
    }
}

/// Performs the partial step in the managed command execution lifecycle.
///
/// # Parameters
/// * `refs`: Prepared resource references for the three command streams.
/// * `policies`: Optional output policies paired with their file references.
fn partial(
    refs: [Option<ResourceRef<Mutex<File>>>; 3],
    policies: [Option<FileSinkPolicy>; 3],
) -> Option<PreparedFiles> {
    if refs.iter().all(Option::is_none) {
        None
    } else {
        Some(build(refs, policies))
    }
}

/// Performs the build step in the managed command execution lifecycle.
///
/// # Parameters
/// * `refs`: Resource references for stdin, stdout, and stderr.
/// * `policies`: Output policies paired with stdout and stderr references.
fn build(mut refs: [Option<ResourceRef<Mutex<File>>>; 3], policies: [Option<FileSinkPolicy>; 3]) -> PreparedFiles {
    PreparedFiles::new(
        refs[0].take(),
        refs[1].take().map(|file| PreparedSink {
            file,
            policy: policies[1].expect("stdout policy accompanies stdout file"),
        }),
        refs[2].take().map(|file| PreparedSink {
            file,
            policy: policies[2].expect("stderr policy accompanies stderr file"),
        }),
    )
}

/// Stores the truncate sinks value used by this execution component.
///
/// # Parameters
/// * `prepared`: Registered files whose truncate policies should be applied.
/// * `scope`: Scope used to record confirmed or uncertain side effects.
pub(crate) fn truncate_sinks(prepared: &PreparedFiles, scope: &OperationScope) -> io::Result<()> {
    for (stream, sink) in [
        (crate::OutputStream::Stdout, prepared.stdout.as_ref()),
        (crate::OutputStream::Stderr, prepared.stderr.as_ref()),
    ] {
        let Some(sink) = sink else { continue };
        truncate_sink(sink, stream, scope).map_err(|(_, error)| error)?;
    }
    Ok(())
}

/// Truncates one managed sink and retains evidence if the operation partially
/// succeeds.
///
/// # Parameters
/// * `sink`: Registered output sink to truncate.
/// * `stream`: Output stream associated with this sink.
/// * `scope`: Operation scope recording the resulting side effect.
///
/// # Returns
/// Success, or the failed preparation phase and original I/O error.
pub(crate) fn truncate_sink(
    sink: &PreparedSink,
    stream: crate::OutputStream,
    scope: &OperationScope,
) -> Result<(), (crate::FilePreparationStage, io::Error)> {
    if sink.policy != FileSinkPolicy::Truncate {
        return Ok(());
    }
    let name = match stream {
        crate::OutputStream::Stdout => "stdout",
        crate::OutputStream::Stderr => "stderr",
    };
    let mut file = sink.file.lock().map_err(|error| {
        (
            crate::FilePreparationStage::Truncation,
            io::Error::other(error.to_string()),
        )
    })?;
    if let Err(error) = file.set_len(0) {
        record_effect(
            scope,
            sink.file.id(),
            qubit_operation::EffectFact::Unknown,
            format!("{name} sink truncation could not be confirmed"),
        );
        return Err((crate::FilePreparationStage::Truncation, error));
    }
    record_effect(
        scope,
        sink.file.id(),
        qubit_operation::EffectFact::Known,
        format!("{name} sink truncated"),
    );
    use std::io::Seek;
    file.rewind()
        .map_err(|error| (crate::FilePreparationStage::Rewind, error))
}

/// Performs the record effect step in the managed command execution lifecycle.
///
/// # Parameters
/// * `scope`: Scope receiving the side effect evidence.
/// * `resource_id`: Identity of the affected registered file.
/// * `fact`: Whether the side effect is known or uncertain.
/// * `description`: Bounded diagnostic description of the side effect.
fn record_effect(
    scope: &OperationScope,
    resource_id: qubit_operation::ResourceId,
    fact: qubit_operation::EffectFact,
    description: String,
) {
    let record = qubit_operation::EffectRecord {
        operation_id: scope.id(),
        resource_id: Some(resource_id),
        fact,
        description: qubit_operation::DiagnosticSummary::new(&description),
    };
    if let Err(error) = scope.record_effect(record) {
        log::warn!("file side-effect evidence was not retained ({error:?}); file owner remains registered");
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::io::Write;
    use std::panic::AssertUnwindSafe;

    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;

    use super::RegistrationCategory;
    use super::register_files;
    use super::truncate_sinks;
    use crate::resource_run_options::FileSinkPolicy;

    #[test]
    fn test_registration_category_is_preserved_in_source_chain() {
        let error = io::Error::other(RegistrationCategory(qubit_operation::ResourceManageErrorKind::Stopped));
        let category = error
            .get_ref()
            .and_then(|source| source.downcast_ref::<RegistrationCategory>())
            .expect("non-error manager category should have an inspectable source carrier");
        assert_eq!(category.0, qubit_operation::ResourceManageErrorKind::Stopped);
    }

    #[test]
    fn test_truncating_prepared_sink_records_known_effect_and_clears_file() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.path().to_owned();
        let mut writer = file.reopen().unwrap();
        writer.write_all(b"existing output").unwrap();
        writer.flush().unwrap();
        drop(writer);

        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let output = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
                let mut reservations = vec![scope.reserve_resource().unwrap()];
                let prepared = register_files(
                    &scope,
                    &mut reservations,
                    None,
                    Some((output, FileSinkPolicy::Truncate)),
                    None,
                )
                .map_err(|error| error.0.to_string())?;
                assert!(reservations.is_empty());
                truncate_sinks(&prepared, &scope).map_err(|error| error.to_string())?;
                assert_eq!(
                    prepared
                        .stdout
                        .as_ref()
                        .unwrap()
                        .file
                        .lock()
                        .unwrap()
                        .metadata()
                        .unwrap()
                        .len(),
                    0
                );
                Ok::<_, String>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        let settlement = operation.wait_settlement().unwrap();
        assert!(settlement.all_released());
        assert_eq!(settlement.effects_total, 1);
        assert_eq!(settlement.effects.len(), 1);
        assert_eq!(settlement.effects[0].fact, qubit_operation::EffectFact::Known);
        assert_eq!(settlement.effects[0].description.as_str(), "stdout sink truncated");
    }

    #[test]
    fn test_poisoned_sink_lock_returns_error_and_releases_registered_owner() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let output = tempfile::tempfile().unwrap();
                let mut reservations = vec![scope.reserve_resource().unwrap()];
                let prepared = register_files(
                    &scope,
                    &mut reservations,
                    None,
                    Some((output, FileSinkPolicy::Truncate)),
                    None,
                )
                .map_err(|error| error.0.to_string())?;
                let sink = prepared.stdout.as_ref().expect("stdout sink should be registered");

                let panic = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    let _guard = sink.file.lock().unwrap();
                    panic!("poison the sink lock to exercise truncation recovery");
                }));
                assert!(panic.is_err());
                let error = truncate_sinks(&prepared, &scope)
                    .expect_err("a poisoned sink must fail truncation without losing its owner");
                assert!(error.to_string().contains("poisoned"));
                drop(prepared);
                Ok::<_, String>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert!(operation.wait_settlement().unwrap().all_released());
    }

    #[cfg(unix)]
    #[test]
    fn test_directory_sink_failure_records_uncertain_effect_and_releases_owner() {
        use std::fs::File;
        use std::io::ErrorKind;

        let directory = tempfile::tempdir().expect("directory sink should be created");
        let directory_path = directory.path().to_owned();
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let sink = File::open(&directory_path).expect("directory can be opened as a file");
                let mut reservations = vec![scope.reserve_resource().unwrap()];
                let prepared = register_files(
                    &scope,
                    &mut reservations,
                    None,
                    Some((sink, FileSinkPolicy::Truncate)),
                    None,
                )
                .expect("sink owner should register before truncation");
                let sink = prepared.stdout.as_ref().expect("stdout owner should be retained");
                let resource_id = sink.file.id();

                let error =
                    truncate_sinks(&prepared, &scope).expect_err("a directory cannot be truncated as an output file");
                assert_eq!(error.kind(), ErrorKind::InvalidInput);
                assert_eq!(prepared.stdout.as_ref().unwrap().file.id(), resource_id);
                assert!(
                    directory_path.is_dir(),
                    "failed truncation must leave the directory present"
                );
                drop(prepared);
                Ok::<_, String>(resource_id)
            })
            .unwrap();

        let resource_id = match operation.wait_outcome().unwrap() {
            OperationOutcome::Succeeded(resource_id) => resource_id,
            other => panic!("directory sink truncation should be handled as a recorded effect: {other:?}"),
        };
        let settlement = operation.wait_settlement().unwrap();
        assert!(settlement.all_released());
        assert_eq!(settlement.effects_total, 1);
        assert_eq!(settlement.effects[0].resource_id, Some(resource_id));
        assert_eq!(settlement.effects[0].fact, qubit_operation::EffectFact::Unknown);
        assert_eq!(
            settlement.effects[0].description.as_str(),
            "stdout sink truncation could not be confirmed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_stdout_truncation_failure_preserves_later_stderr_sink() {
        let directory = tempfile::tempdir().expect("directory sink should be created");
        let directory_path = directory.path().to_owned();
        let stderr = tempfile::NamedTempFile::new().expect("stderr sink should be created");
        let stderr_path = stderr.path().to_owned();
        std::fs::write(&stderr_path, b"preserve after stdout failure").unwrap();
        let operation_stderr_path = stderr_path.clone();

        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let stdout = std::fs::File::open(&directory_path).expect("directory can be opened as a file");
                let stderr = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&operation_stderr_path)
                    .unwrap();
                let mut reservations = vec![scope.reserve_resource().unwrap(), scope.reserve_resource().unwrap()];
                let prepared = register_files(
                    &scope,
                    &mut reservations,
                    None,
                    Some((stdout, FileSinkPolicy::Truncate)),
                    Some((stderr, FileSinkPolicy::Truncate)),
                )
                .expect("both sink owners should register");

                let error = truncate_sinks(&prepared, &scope)
                    .expect_err("stdout directory truncation should stop the sink pass");
                assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
                assert!(prepared.stdout.is_some());
                assert!(prepared.stderr.is_some());
                drop(prepared);
                Ok::<_, String>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        let settlement = operation.wait_settlement().unwrap();
        assert!(settlement.all_released());
        assert_eq!(settlement.effects_total, 1);
        assert_eq!(settlement.effects[0].fact, qubit_operation::EffectFact::Unknown);
        assert_eq!(
            std::fs::read(&stderr_path).unwrap(),
            b"preserve after stdout failure",
            "a later sink must remain untouched when stdout truncation fails"
        );
    }

    #[test]
    fn test_effect_record_overflow_does_not_block_sink_truncation() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.path().to_owned();
        let mut writer = file.reopen().unwrap();
        writer.write_all(b"existing output").unwrap();
        writer.flush().unwrap();
        drop(writer);

        let manager = OperationManager::new(Default::default()).unwrap();
        let mut options = OperationOptions::default();
        options.limits.effect_records = 1;
        let mut operation = manager
            .try_submit(options, move |scope| {
                let output = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
                let mut reservations = vec![scope.reserve_resource().unwrap()];
                let prepared = register_files(
                    &scope,
                    &mut reservations,
                    None,
                    Some((output, FileSinkPolicy::Truncate)),
                    None,
                )
                .map_err(|error| error.0.to_string())?;
                scope
                    .record_effect(qubit_operation::EffectRecord {
                        operation_id: scope.id(),
                        resource_id: None,
                        fact: qubit_operation::EffectFact::Known,
                        description: qubit_operation::DiagnosticSummary::new("fill effect capacity"),
                    })
                    .unwrap();

                truncate_sinks(&prepared, &scope).map_err(|error| error.to_string())?;
                assert_eq!(
                    prepared
                        .stdout
                        .as_ref()
                        .unwrap()
                        .file
                        .lock()
                        .unwrap()
                        .metadata()
                        .unwrap()
                        .len(),
                    0
                );
                Ok::<_, String>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        let settlement = operation.wait_settlement().unwrap();
        assert!(settlement.all_released());
        assert_eq!(settlement.effects_total, 2);
        assert_eq!(settlement.effect_overflow, 1);
    }

    #[test]
    fn test_register_files_restores_unregistered_input_when_reservation_is_missing() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let input = tempfile::tempfile().unwrap();
                let mut reservations = Vec::new();
                let failure = register_files(&scope, &mut reservations, Some(input), None, None)
                    .expect_err("input registration requires a reserved resource slot");
                let (error, direction, prepared, mut raw_files) = *failure;

                assert!(error.to_string().contains("missing reserved resource slot"));
                assert_eq!(direction, crate::FilePreparationDirection::Stdin);
                assert!(prepared.is_none());
                assert!(raw_files[0].take().is_some());
                assert!(raw_files[1].is_none());
                assert!(raw_files[2].is_none());
                Ok::<_, String>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert!(operation.wait_settlement().unwrap().all_released());
    }

    #[test]
    fn test_public_command_error_preserves_manager_registration_category() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let (send_reservation, receive_reservation) = std::sync::mpsc::sync_channel(1);
        let mut reservation_owner = manager
            .try_submit(OperationOptions::default(), move |scope| {
                send_reservation
                    .send(scope.reserve_resource().expect("resource slot should be reservable"))
                    .expect("stale reservation should reach the test");
                Ok::<_, std::convert::Infallible>(())
            })
            .unwrap();
        let stale_reservation = receive_reservation
            .recv()
            .expect("resource reservation should be returned");
        assert!(matches!(
            reservation_owner.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));

        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let mut reservations = vec![stale_reservation];
                let failure = register_files(
                    &scope,
                    &mut reservations,
                    Some(tempfile::tempfile().expect("input fixture should be created")),
                    None,
                    None,
                )
                .expect_err("a reservation from an ended scope must fail manager registration");
                let (source, direction, prepared, raw_files) = *failure;
                assert!(prepared.is_none());
                assert!(raw_files[0].is_some());
                assert_eq!(direction, crate::FilePreparationDirection::Stdin);

                let error = crate::command_error::managed_file_preparation_error(
                    crate::FilePreparationStage::Registration,
                    Some(direction),
                    source,
                );
                assert_eq!(
                    error.resource_registration_error_kind(),
                    Some(qubit_operation::ResourceManageErrorKind::ScopeEnded)
                );
                assert!(matches!(
                    error.reason(),
                    crate::CommandErrorReason::FilePreparationFailed {
                        stage: crate::FilePreparationStage::Registration,
                        direction: Some(crate::FilePreparationDirection::Stdin),
                        source,
                    } if source.kind() == io::ErrorKind::Other
                        && source.get_ref().is_some_and(|source| source.downcast_ref::<RegistrationCategory>()
                            .is_some_and(|category| category.0 == qubit_operation::ResourceManageErrorKind::ScopeEnded))
                ));
                Ok::<_, std::convert::Infallible>(())
            })
            .unwrap();
        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
    }

    #[test]
    fn test_register_files_returns_partial_owner_and_remaining_output_file() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let input = tempfile::tempfile().unwrap();
                let output = tempfile::tempfile().unwrap();
                let mut reservations = vec![scope.reserve_resource().unwrap()];
                let failure = register_files(
                    &scope,
                    &mut reservations,
                    Some(input),
                    Some((output, FileSinkPolicy::Preserve)),
                    None,
                )
                .expect_err("stdout registration should fail after the input takes the only slot");
                let (error, direction, prepared, mut raw_files) = *failure;
                let partial = prepared.expect("successfully registered stdin stays owned");

                assert!(error.to_string().contains("missing reserved resource slot"));
                assert_eq!(direction, crate::FilePreparationDirection::Stdout);
                assert!(reservations.is_empty());
                assert!(partial.stdin.is_some());
                assert!(partial.stdout.is_none());
                assert!(partial.stderr.is_none());
                assert!(raw_files[0].is_none());
                assert!(raw_files[1].take().is_some());
                assert!(raw_files[2].is_none());
                drop(partial);
                Ok::<_, String>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert!(operation.wait_settlement().unwrap().all_released());
    }

    #[test]
    fn test_register_files_returns_partial_stdin_stdout_and_recovers_stderr_on_missing_slot() {
        use std::io::Read;
        use std::io::Seek;
        use std::io::SeekFrom;

        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let stdin = tempfile::tempfile().unwrap();
                let stdout = tempfile::tempfile().unwrap();
                let stderr = tempfile::tempfile().unwrap();
                let mut reservations = vec![scope.reserve_resource().unwrap(), scope.reserve_resource().unwrap()];

                let failure = register_files(
                    &scope,
                    &mut reservations,
                    Some(stdin),
                    Some((stdout, FileSinkPolicy::Truncate)),
                    Some((stderr, FileSinkPolicy::Preserve)),
                )
                .expect_err("stderr registration should fail after stdin and stdout consume both slots");
                let (error, direction, prepared, mut raw_files) = *failure;
                let partial = prepared.expect("registered stdin and stdout owners should be returned");

                assert!(error.to_string().contains("missing reserved resource slot"));
                assert_eq!(direction, crate::FilePreparationDirection::Stderr);
                assert!(reservations.is_empty());
                assert!(partial.stdin.is_some());
                let stdout = partial.stdout.as_ref().expect("stdout owner should remain registered");
                assert_eq!(stdout.policy, FileSinkPolicy::Truncate);
                assert!(partial.stderr.is_none());
                assert!(raw_files[0].is_none());
                assert!(raw_files[1].is_none());

                let mut recovered_stderr = raw_files[2].take().expect("unregistered stderr owner should return");
                recovered_stderr.write_all(b"recovered stderr").unwrap();
                recovered_stderr.seek(SeekFrom::Start(0)).unwrap();
                let mut contents = Vec::new();
                recovered_stderr.read_to_end(&mut contents).unwrap();
                assert_eq!(contents, b"recovered stderr");

                drop(partial);
                drop(recovered_stderr);
                Ok::<_, String>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert!(operation.wait_settlement().unwrap().all_released());
    }

    #[test]
    fn test_register_files_transfers_all_streams_and_preserves_selected_sink() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let mut operation = manager
            .try_submit(OperationOptions::default(), move |scope| {
                let mut reservations = (0..3).map(|_| scope.reserve_resource().unwrap()).collect::<Vec<_>>();
                let prepared = register_files(
                    &scope,
                    &mut reservations,
                    Some(tempfile::tempfile().unwrap()),
                    Some((tempfile::tempfile().unwrap(), FileSinkPolicy::Preserve)),
                    Some((tempfile::tempfile().unwrap(), FileSinkPolicy::Truncate)),
                )
                .expect("all three reserved files should be registered");

                assert!(reservations.is_empty());
                assert!(prepared.stdin.is_some());
                assert_eq!(prepared.stdout.as_ref().unwrap().policy, FileSinkPolicy::Preserve);
                assert_eq!(prepared.stderr.as_ref().unwrap().policy, FileSinkPolicy::Truncate);
                truncate_sinks(&prepared, &scope).map_err(|error| error.to_string())?;
                Ok::<_, String>(())
            })
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        let settlement = operation.wait_settlement().unwrap();
        assert!(settlement.all_released());
        assert_eq!(settlement.effects_total, 1);
        assert_eq!(settlement.effects[0].description.as_str(), "stderr sink truncated");
    }
}
