use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};

use qubit_command::execution::ResourceInput;
use qubit_command::{CommandResourceRunOptions, CommandRunOptions, FileSinkPolicy};
use qubit_operation::{
    GroupOptions, ManagerOptions, OperationManager, OperationOptions, OperationOutcome, ScopeLimits,
};

#[test]
fn owned_file_options_keep_current_cursor_and_explicit_policy() {
    let mut input = tempfile::tempfile().expect("input file should be created");
    input
        .write_all(b"prefix-payload")
        .expect("fixture should be written");
    input
        .seek(SeekFrom::Start(7))
        .expect("cursor should be positioned");

    let output = tempfile::NamedTempFile::new().expect("output file should be created");
    std::fs::write(output.path(), b"existing contents").expect("output should be populated");
    let output = OpenOptions::new()
        .read(true)
        .write(true)
        .open(output.path())
        .expect("output should be opened");

    let options = CommandResourceRunOptions::new()
        .stdin_file(input)
        .stdout_sink(output, FileSinkPolicy::Preserve);
    assert!(options.configured_stdin().is_some());
    let (_, policy) = options
        .configured_stdout()
        .expect("stdout sink should be retained");
    assert_eq!(policy, FileSinkPolicy::Preserve);
    assert!(options.configured_stderr().is_none());

    let mut configured_input = options
        .configured_stdin()
        .expect("stdin should remain owned");
    let mut payload = Vec::new();
    configured_input
        .read_to_end(&mut payload)
        .expect("stdin should be readable from current cursor");
    assert_eq!(payload, b"payload");
}

#[test]
fn sink_policies_are_explicit_and_distinct() {
    assert_ne!(FileSinkPolicy::Preserve, FileSinkPolicy::Truncate);
}

fn with_scope<F>(limits: ScopeLimits, body: F) -> std::sync::Arc<qubit_operation::SettlementReport>
where
    F: FnOnce(qubit_operation::OperationScope) + Send + 'static,
{
    let manager = OperationManager::new(ManagerOptions::default()).expect("manager should start");
    let group = manager
        .group(GroupOptions { capacity: 4 })
        .expect("group should start");
    let completion = group
        .run_and_settle(
            OperationOptions {
                limits,
                ..OperationOptions::default()
            },
            move |scope| {
                body(scope);
                Ok::<_, ()>(())
            },
        )
        .expect("operation should be accepted");
    assert!(matches!(
        completion.outcome,
        OperationOutcome::Succeeded(())
    ));
    completion.settlement
}

#[test]
fn preparation_preserves_stdin_cursor_and_output_contents_or_truncates_explicitly() {
    let mut input = tempfile::tempfile().expect("stdin fixture should be created");
    input
        .write_all(b"before-payload")
        .expect("stdin fixture should be written");
    input
        .seek(SeekFrom::Start(7))
        .expect("stdin cursor should be positioned");
    let input_observer = input.try_clone().expect("stdin observer should be cloned");

    let preserve = tempfile::NamedTempFile::new().expect("preserve sink should be created");
    std::fs::write(preserve.path(), b"preserved").expect("preserve fixture should be populated");
    let preserve_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(preserve.path())
        .expect("preserve sink should be opened");
    let truncate = tempfile::NamedTempFile::new().expect("truncate sink should be created");
    std::fs::write(truncate.path(), b"discard").expect("truncate fixture should be populated");
    let truncate_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(truncate.path())
        .expect("truncate sink should be opened");
    let preserve_path = preserve.path().to_path_buf();
    let truncate_path = truncate.path().to_path_buf();

    let settlement = with_scope(ScopeLimits::default(), move |scope| {
        let prepared = Box::new(
            CommandResourceRunOptions::new()
                .stdin_file(input)
                .stdout_sink(preserve_file, FileSinkPolicy::Preserve)
                .stderr_sink(truncate_file, FileSinkPolicy::Truncate),
        )
        .prepare_files(&scope)
        .expect("files should prepare");
        assert!(format!("{prepared:?}").contains("stdin: true"));
        assert_eq!(std::fs::read(&preserve_path).unwrap(), b"preserved");
        assert_eq!(std::fs::read(&truncate_path).unwrap(), b"");
        drop(prepared);
        let mut observer = input_observer;
        let mut bytes = Vec::new();
        observer
            .read_to_end(&mut bytes)
            .expect("observer should read remaining stdin");
        assert_eq!(bytes, b"payload");
    });
    assert_eq!(settlement.effects_total, 1);
    assert!(
        settlement
            .effects
            .iter()
            .any(|effect| effect.fact == qubit_operation::EffectFact::Known)
    );
}

#[test]
fn resource_reservation_failure_keeps_sink_contents_and_returns_input_owner() {
    let output = tempfile::NamedTempFile::new().expect("output should be created");
    std::fs::write(output.path(), b"must survive").expect("output should be populated");
    let output_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(output.path())
        .expect("output should open");
    let output_path = output.path().to_path_buf();

    with_scope(ScopeLimits::default(), move |scope| {
        let mut reservations = Vec::new();
        while let Ok(reservation) = scope.reserve_resource() {
            reservations.push(reservation);
        }
        assert!(!reservations.is_empty());
        let failure = Box::new(
            CommandResourceRunOptions::new().stdout_sink(output_file, FileSinkPolicy::Truncate),
        )
        .prepare_files(&scope)
        .expect_err("zero resource capacity must reject preparation");
        assert!(
            failure.input.is_some(),
            "the owned options must be recoverable"
        );
        assert!(failure.prepared.is_none());
        assert_eq!(std::fs::read(output_path).unwrap(), b"must survive");
    });
}

#[test]
fn owned_sink_cannot_alias_a_legacy_tee_path() {
    let output = tempfile::NamedTempFile::new().expect("output should be created");
    std::fs::write(output.path(), b"must survive").expect("output should be populated");
    let output_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(output.path())
        .expect("output should open");
    let path = output.path().to_path_buf();
    let tee_path = path.clone();

    with_scope(ScopeLimits::default(), move |scope| {
        let failure = Box::new(
            CommandResourceRunOptions::new()
                .options(CommandRunOptions::new().tee_stdout_to_file(tee_path))
                .stdout_sink(output_file, FileSinkPolicy::Truncate),
        )
        .prepare_files(&scope)
        .expect_err("owned sink must not alias the legacy tee");
        assert!(failure.input.is_some());
        assert_eq!(std::fs::read(path).unwrap(), b"must survive");
    });
}

#[test]
fn partial_truncation_failure_retains_prepared_owners_and_effects() {
    let first = tempfile::NamedTempFile::new().expect("first sink should be created");
    std::fs::write(first.path(), b"first data").expect("first sink should be populated");
    let first_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(first.path())
        .expect("first sink should open for writing");
    let first_path = first.path().to_path_buf();

    let second = tempfile::NamedTempFile::new().expect("second sink should be created");
    std::fs::write(second.path(), b"second data").expect("second sink should be populated");
    let second_file = File::open(second.path()).expect("second sink should open read-only");
    let second_path = second.path().to_path_buf();

    let settlement = with_scope(ScopeLimits::default(), move |scope| {
        let failure = Box::new(
            CommandResourceRunOptions::new()
                .stdout_sink(first_file, FileSinkPolicy::Truncate)
                .stderr_sink(second_file, FileSinkPolicy::Truncate),
        )
        .prepare_files(&scope)
        .expect_err("read-only second sink should fail truncation");
        assert!(
            failure.prepared.is_some(),
            "registered owners must remain available"
        );
        assert!(
            failure.input.is_none(),
            "prepared owners retain the consumed files"
        );
        assert_eq!(std::fs::read(first_path).unwrap(), b"");
        assert_eq!(std::fs::read(second_path).unwrap(), b"second data");
    });
    assert_eq!(settlement.effects_total, 2);
    assert!(
        settlement
            .effects
            .iter()
            .any(|effect| effect.fact == qubit_operation::EffectFact::Known)
    );
    assert!(
        settlement
            .effects
            .iter()
            .any(|effect| effect.fact == qubit_operation::EffectFact::Unknown)
    );
}

#[test]
fn effect_record_overflow_does_not_prevent_sink_truncation_or_owner_retention() {
    let output = tempfile::NamedTempFile::new().expect("output should be created");
    std::fs::write(output.path(), b"discard me").expect("output should be populated");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(output.path())
        .expect("output should open");
    let path = output.path().to_path_buf();
    let mut limits = ScopeLimits::default();
    limits.effect_records = 0;

    let settlement = with_scope(limits, move |scope| {
        let prepared =
            Box::new(CommandResourceRunOptions::new().stdout_sink(file, FileSinkPolicy::Truncate))
                .prepare_files(&scope)
                .expect("effect overflow must not fail preparation");
        assert!(format!("{prepared:?}").contains("stdout: true"));
        assert_eq!(std::fs::read(path).unwrap(), b"");
    });
    assert_eq!(settlement.effects_total, 1);
    assert!(settlement.effects.is_empty());
    assert_eq!(settlement.effect_overflow, 1);
}
