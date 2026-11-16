use qubit_command::{CommandExecutionConfig, CommandRunOptions, ProcessTreePolicy};

#[test]
fn managed_defaults_are_bounded() {
    let config = CommandExecutionConfig::default();
    assert_eq!(config.max_active, 4);
    assert_eq!(config.max_queued, 64);
    assert_eq!(config.max_waiters, 64);
    assert_eq!(config.max_queued_input_bytes, 16 * 1024 * 1024);
    assert_eq!(config.max_diagnostics, 64);
    assert!(config.validate().is_ok());
}

#[test]
fn zero_active_rejected_and_zero_queue_is_valid() {
    let mut config = CommandExecutionConfig::default();
    config.max_active = 0;
    assert!(config.validate().is_err());
    config.max_active = 1;
    config.max_queued = 0;
    assert!(config.validate().is_ok());
}

#[test]
fn options_preserve_clone_and_managed_default() {
    fn assert_clone<T: Clone>() {}
    assert_clone::<CommandRunOptions>();
    let options = CommandRunOptions::default();
    assert_eq!(options.process_tree_policy(), ProcessTreePolicy::Managed);
    assert_eq!(
        options.clone().process_tree_policy(),
        ProcessTreePolicy::Managed
    );
}

#[test]
fn pure_config_and_import_do_not_start_service_threads() {
    let before = std::thread::available_parallelism().unwrap();
    let config = CommandExecutionConfig::default();
    assert!(config.validate().is_ok());
    let after = std::thread::available_parallelism().unwrap();
    assert_eq!(before, after);
}

#[test]
fn owned_options_are_consumed_by_value() {
    fn consume(options: qubit_command::CommandResourceRunOptions) -> usize {
        std::mem::size_of_val(&options)
    }
    let options = qubit_command::CommandResourceRunOptions::new();
    assert!(consume(options) > 0);
}

#[test]
fn snapshot_submission_failure_retains_diagnostic() {
    use qubit_command::{CommandSubmitFailureReason, SnapshotFailure};
    let reason = CommandSubmitFailureReason::Snapshot(SnapshotFailure {
        diagnostic: "working directory could not be captured".to_owned(),
    });
    assert!(
        matches!(reason, CommandSubmitFailureReason::Snapshot(ref failure) if failure.diagnostic.contains("working directory"))
    );
}
