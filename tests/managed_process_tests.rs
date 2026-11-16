// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#[test]
fn test_managed_process_backend_contract_is_available() {
    use qubit_command::Command;
    use qubit_command::CommandRunner;
    let output = CommandRunner::without_timeout()
        .run(Command::new("rustc").arg("--version"))
        .expect("rustc should run");
    assert!(!output.stdout().is_empty());
}
