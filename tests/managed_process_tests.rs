#[test]
fn managed_process_backend_contract_is_available() {
    use qubit_command::Command;
    use qubit_command::CommandRunner;
    let output = CommandRunner::without_timeout()
        .run(Command::new("rustc").arg("--version"))
        .expect("rustc should run");
    assert!(!output.stdout().is_empty());
}
