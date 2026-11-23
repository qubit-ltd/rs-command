// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::Path;

use qubit_command::CommandResourceRunOptions;
use qubit_command::CommandRunOptions;
use qubit_command::Command;
use qubit_command::CommandExecutionConfig;
use qubit_command::CommandExecutor;
use qubit_command::CommandRunner;
use qubit_command::FileSinkPolicy;
use qubit_command::execution::ResourceInput;
use qubit_command::execution::contracts::ExecutionSnapshot;
use qubit_operation::GroupOptions;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;
use qubit_operation::OperationOptions;
use qubit_operation::OperationOutcome;
use qubit_operation::ScopeLimits;

#[test]
fn test_owned_file_options_keep_current_cursor_and_explicit_policy() {
    let mut input = tempfile::tempfile().expect("input file should be created");
    input.write_all(b"prefix-payload").expect("fixture should be written");
    input.seek(SeekFrom::Start(7)).expect("cursor should be positioned");

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
    let (_, policy) = options.configured_stdout().expect("stdout sink should be retained");
    assert_eq!(policy, FileSinkPolicy::Preserve);
    assert!(options.configured_stderr().is_none());

    let mut configured_input = options.configured_stdin().expect("stdin should remain owned");
    let mut payload = Vec::new();
    configured_input
        .read_to_end(&mut payload)
        .expect("stdin should be readable from current cursor");
    assert_eq!(payload, b"payload");
}

#[test]
fn test_sink_policies_are_explicit_and_distinct() {
    assert_ne!(FileSinkPolicy::Preserve, FileSinkPolicy::Truncate);
}

fn with_scope<F>(limits: ScopeLimits, body: F) -> std::sync::Arc<qubit_operation::SettlementReport>
where
    F: FnOnce(qubit_operation::OperationScope) + Send + 'static,
{
    let manager = OperationManager::new(ManagerOptions::default()).expect("manager should start");
    let group = manager.group(GroupOptions { capacity: 4 }).expect("group should start");
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
    assert!(matches!(completion.outcome, OperationOutcome::Succeeded(())));
    completion.settlement
}

#[test]
fn test_preparation_preserves_stdin_cursor_and_output_contents_or_truncates_explicitly() {
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
fn test_resource_reservation_failure_keeps_sink_contents_and_returns_input_owner() {
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
        let failure = Box::new(CommandResourceRunOptions::new().stdout_sink(output_file, FileSinkPolicy::Truncate))
            .prepare_files(&scope)
            .expect_err("zero resource capacity must reject preparation");
        assert!(failure.input.is_some(), "the owned options must be recoverable");
        assert!(failure.prepared.is_none());
        assert_eq!(std::fs::read(output_path).unwrap(), b"must survive");
    });
}

#[test]
fn test_owned_sink_cannot_alias_a_legacy_tee_path() {
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
fn test_snapshot_directory_resolves_relative_tee_identity() {
    let snapshot_dir = tempfile::tempdir().expect("snapshot directory should be created");
    let output = tempfile::NamedTempFile::new().expect("owned sink should be created");
    std::fs::write(output.path(), b"must survive").expect("owned sink should be populated");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(output.path())
        .expect("owned sink should open");
    let alias_name = format!("owned-file-alias-{}", std::process::id());
    let alias_path = snapshot_dir.path().join(&alias_name);
    std::fs::hard_link(output.path(), &alias_path).expect("snapshot alias should be linked");
    assert!(
        !Path::new(&alias_name).exists(),
        "current directory should differ from snapshot path"
    );
    let output_path = output.path().to_path_buf();
    let snapshot = ExecutionSnapshot {
        working_directory: snapshot_dir.path().to_path_buf(),
        environment: Vec::new(),
    };

    with_scope(ScopeLimits::default(), move |scope| {
        let failure = Box::new(
            CommandResourceRunOptions::new()
                .options(CommandRunOptions::new().tee_stdout_to_file(alias_name))
                .stdout_sink(file, FileSinkPolicy::Truncate),
        )
        .prepare_files_with_snapshot(&scope, &snapshot)
        .expect_err("submission-time relative tee alias must be rejected");
        assert!(failure.input.is_some());
        assert_eq!(std::fs::read(output_path).unwrap(), b"must survive");
    });
}

#[cfg(any(unix, windows))]
#[test]
fn test_managed_relative_paths_use_parent_directory_and_child_cwd_priority() {
    const WORKER_ENV: &str = "QUBIT_MANAGED_PATH_BASE_WORKER";
    if let Some(root) = std::env::var_os(WORKER_ENV) {
        let root = std::path::PathBuf::from(root);
        assert_eq!(std::env::current_dir().expect("worker directory should resolve"), root);
        let runner_dir = root.join("runner-default");
        let command_dir = root.join("command-override");
        std::fs::create_dir_all(&runner_dir).expect("runner directory should be created");
        std::fs::create_dir_all(&command_dir).expect("command directory should be created");

        let manager = OperationManager::new(ManagerOptions::default()).expect("manager should start");
        let executor = CommandExecutor::new(manager, CommandExecutionConfig::default())
            .expect("executor should start");
        let input = root.join("input.txt");
        std::fs::write(&input, b"parent-input").expect("parent input should be populated");

        let mut command_override = executor
            .submit(
                CommandRunner::without_timeout().working_directory(&runner_dir),
                path_probe_command()
                    .stdin_file("input.txt")
                    .working_directory(&command_dir),
                CommandRunOptions::new()
                    .tee_stdout_to_file("command-stdout.log")
                    .tee_stderr_to_file("command-stderr.log"),
            )
            .expect("standard request should be admitted");
        let output = match command_override.wait_outcome().expect("outcome should arrive") {
            OperationOutcome::Succeeded(output) => output,
            other => panic!("command override request failed: {other:?}"),
        };
        assert!(String::from_utf8_lossy(output.stdout()).starts_with(command_dir.to_str().unwrap()));
        assert!(String::from_utf8_lossy(output.stdout()).contains("parent-input"));
        assert_eq!(std::fs::read(root.join("command-stdout.log")).unwrap(), output.stdout());
        assert!(std::fs::read(root.join("command-stderr.log")).unwrap().is_empty());
        assert!(!runner_dir.join("command-stdout.log").exists());
        assert!(!command_dir.join("command-stdout.log").exists());
        assert!(command_override.wait_settlement().unwrap().all_released());

        let mut runner_default = executor
            .submit(
                CommandRunner::without_timeout().working_directory(&runner_dir),
                path_probe_command().stdin_file("input.txt"),
                CommandRunOptions::new().tee_stdout_to_file("runner-stdout.log"),
            )
            .expect("runner default request should be admitted");
        let output = match runner_default.wait_outcome().expect("outcome should arrive") {
            OperationOutcome::Succeeded(output) => output,
            other => panic!("runner default request failed: {other:?}"),
        };
        assert!(String::from_utf8_lossy(output.stdout()).starts_with(runner_dir.to_str().unwrap()));
        assert!(String::from_utf8_lossy(output.stdout()).contains("parent-input"));
        assert_eq!(std::fs::read(root.join("runner-stdout.log")).unwrap(), output.stdout());
        assert!(!runner_dir.join("runner-stdout.log").exists());
        assert!(runner_default.wait_settlement().unwrap().all_released());

        for directory in [&runner_dir, &command_dir] {
            std::fs::write(directory.join("resource-stdout.log"), b"child sentinel")
                .expect("child stdout sentinel should be written");
            std::fs::write(directory.join("resource-stderr.log"), b"child sentinel")
                .expect("child stderr sentinel should be written");
        }
        std::fs::write(root.join("resource-stdout.log"), b"parent seed")
            .expect("parent stdout tee should exist before resource preflight");
        std::fs::write(root.join("resource-stderr.log"), b"parent seed")
            .expect("parent stderr tee should exist before resource preflight");
        let mut resources = executor
            .submit_resources(
                CommandRunner::without_timeout().working_directory(&runner_dir),
                path_probe_command()
                    .stdin_file("input.txt")
                    .working_directory(&command_dir),
                CommandResourceRunOptions::new().options(
                    CommandRunOptions::new()
                        .tee_stdout_to_file("resource-stdout.log")
                        .tee_stderr_to_file("resource-stderr.log"),
                ),
            )
            .expect("resource request should be admitted");
        let output = match resources.wait_outcome().expect("outcome should arrive") {
            OperationOutcome::Succeeded(output) => output,
            other => panic!("resource request failed: {other:?}"),
        };
        assert!(String::from_utf8_lossy(output.stdout()).starts_with(command_dir.to_str().unwrap()));
        assert!(String::from_utf8_lossy(output.stdout()).contains("parent-input"));
        assert_eq!(std::fs::read(root.join("resource-stdout.log")).unwrap(), output.stdout());
        assert!(std::fs::read(root.join("resource-stderr.log")).unwrap().is_empty());
        for directory in [&runner_dir, &command_dir] {
            assert_eq!(std::fs::read(directory.join("resource-stdout.log")).unwrap(), b"child sentinel");
            assert_eq!(std::fs::read(directory.join("resource-stderr.log")).unwrap(), b"child sentinel");
        }
        assert!(resources.wait_settlement().unwrap().all_released());
        return;
    }

    let directory = tempfile::tempdir().expect("path fixture should be created");
    let output = std::process::Command::new(std::env::current_exe().expect("test executable should resolve"))
        .arg("--exact")
        .arg("test_managed_relative_paths_use_parent_directory_and_child_cwd_priority")
        .arg("--nocapture")
        .env(WORKER_ENV, directory.path())
        .current_dir(directory.path())
        .output()
        .expect("isolated path worker should start");
    assert!(
        output.status.success(),
        "isolated path worker failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
fn path_probe_command() -> Command {
    Command::new("sh").arg("-c").arg("pwd; cat")
}

#[cfg(windows)]
fn path_probe_command() -> Command {
    Command::new("cmd.exe").args(&["/C", "cd & findstr /R ."])
}

#[cfg(windows)]
#[test]
fn test_windows_drive_relative_resource_paths_keep_parent_drive_cwd() {
    use std::path::Component;

    const WORKER_ENV: &str = "QUBIT_WINDOWS_DRIVE_RELATIVE_WORKER";
    if let Some(root) = std::env::var_os(WORKER_ENV) {
        let root = std::path::PathBuf::from(root);
        assert_eq!(std::env::current_dir().expect("worker directory should resolve"), root);
        let drive_prefix = root
            .components()
            .find_map(|component| match component {
                Component::Prefix(prefix) => Some(prefix.as_os_str().to_string_lossy().into_owned()),
                _ => None,
            })
            .expect("temporary directory should have a drive prefix");
        let drive_relative = |name: &str| std::path::PathBuf::from(format!("{drive_prefix}{name}"));
        let input = drive_relative("input.txt");
        let stdout_tee = drive_relative("drive-relative-stdout.log");
        let stderr_tee = drive_relative("drive-relative-stderr.log");
        std::fs::write(root.join("input.txt"), b"parent-drive-input")
            .expect("parent drive input should be written");
        std::fs::write(&stdout_tee, b"parent seed").expect("stdout tee should exist for preflight");
        std::fs::write(&stderr_tee, b"parent seed").expect("stderr tee should exist for preflight");

        let child_dir = root.join("drive-relative-child");
        std::fs::create_dir_all(&child_dir).expect("child directory should be created");
        std::fs::write(child_dir.join("drive-relative-stdout.log"), b"child sentinel")
            .expect("child stdout sentinel should be written");
        std::fs::write(child_dir.join("drive-relative-stderr.log"), b"child sentinel")
            .expect("child stderr sentinel should be written");

        let manager = OperationManager::new(ManagerOptions::default()).expect("manager should start");
        let executor = CommandExecutor::new(manager, CommandExecutionConfig::default())
            .expect("executor should start");
        let mut operation = executor
            .submit_resources(
                CommandRunner::without_timeout(),
                path_probe_command().stdin_file(input).working_directory(&child_dir),
                CommandResourceRunOptions::new().options(
                    CommandRunOptions::new()
                        .tee_stdout_to_file(stdout_tee)
                        .tee_stderr_to_file(stderr_tee),
                ),
            )
            .expect("drive-relative resource request should be admitted");
        let output = match operation.wait_outcome().expect("outcome should arrive") {
            OperationOutcome::Succeeded(output) => output,
            other => panic!("drive-relative request failed: {other:?}"),
        };
        let stdout = String::from_utf8_lossy(output.stdout());
        assert!(stdout.starts_with(child_dir.to_str().unwrap()));
        assert!(stdout.contains("parent-drive-input"));
        assert_eq!(std::fs::read(&root.join("drive-relative-stdout.log")).unwrap(), output.stdout());
        assert!(std::fs::read(root.join("drive-relative-stderr.log")).unwrap().is_empty());
        assert_eq!(
            std::fs::read(child_dir.join("drive-relative-stdout.log")).unwrap(),
            b"child sentinel"
        );
        assert_eq!(
            std::fs::read(child_dir.join("drive-relative-stderr.log")).unwrap(),
            b"child sentinel"
        );
        assert!(operation.wait_settlement().unwrap().all_released());
        return;
    }

    let directory = tempfile::tempdir().expect("drive-relative fixture should be created");
    let output = std::process::Command::new(std::env::current_exe().expect("test executable should resolve"))
        .arg("--exact")
        .arg("test_windows_drive_relative_resource_paths_keep_parent_drive_cwd")
        .arg("--nocapture")
        .env(WORKER_ENV, directory.path())
        .current_dir(directory.path())
        .output()
        .expect("isolated Windows path worker should start");
    assert!(
        output.status.success(),
        "isolated Windows path worker failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_partial_truncation_failure_retains_prepared_owners_and_effects() {
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
        assert!(failure.prepared.is_some(), "registered owners must remain available");
        assert!(failure.input.is_none(), "prepared owners retain the consumed files");
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
fn test_effect_record_overflow_does_not_prevent_sink_truncation_or_owner_retention() {
    let output = tempfile::NamedTempFile::new().expect("output should be created");
    std::fs::write(output.path(), b"discard me").expect("output should be populated");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(output.path())
        .expect("output should open");
    let path = output.path().to_path_buf();
    let limits = ScopeLimits {
        effect_records: 0,
        ..ScopeLimits::default()
    };

    let settlement = with_scope(limits, move |scope| {
        let prepared = Box::new(CommandResourceRunOptions::new().stdout_sink(file, FileSinkPolicy::Truncate))
            .prepare_files(&scope)
            .expect("effect overflow must not fail preparation");
        assert!(format!("{prepared:?}").contains("stdout: true"));
        assert_eq!(std::fs::read(path).unwrap(), b"");
    });
    assert_eq!(settlement.effects_total, 1);
    assert!(settlement.effects.is_empty());
    assert_eq!(settlement.effect_overflow, 1);
}
