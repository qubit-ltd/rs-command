// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for command environment behavior.

use qubit_command::Command;

#[test]
fn test_command_env_readding_removed_key_clears_removal() {
    let command = Command::new("env")
        .env_remove("QUBIT_COMMAND_ENV_TEST")
        .env("QUBIT_COMMAND_ENV_TEST", "restored");

    assert!(command.removed_environment().is_empty());
    assert_eq!(command.environment().len(), 1);
    assert_eq!(
        command.environment()[0].0.to_string_lossy(),
        "QUBIT_COMMAND_ENV_TEST",
    );
    assert_eq!(command.environment()[0].1.to_string_lossy(), "restored");
}
