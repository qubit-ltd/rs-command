// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![cfg(feature = "async")]

use std::io;

use qubit_command::CommandStderr;
use qubit_command::CommandStdin;
use qubit_command::CommandStdout;
use qubit_io::AsyncClose;
use qubit_io::AsyncInput;
use qubit_io::AsyncOutput;

struct BridgeDefaults;
impl qubit_command::session::contracts::SessionBridge for BridgeDefaults {
    fn read_stdin(&self, _buffer: &mut [u8]) -> io::Result<usize> {
        Ok(0)
    }
    fn write_stdout(&self, _bytes: &[u8]) -> io::Result<()> {
        Ok(())
    }
    fn write_stderr(&self, _bytes: &[u8]) -> io::Result<()> {
        Ok(())
    }
    fn close_stdin(&self) {}
}

#[test]
fn test_public_stream_types_implement_qubit_io_traits() {
    fn input<T: AsyncInput<Item = u8>>() {}
    fn output<T: AsyncOutput<Item = u8> + AsyncClose>() {}
    input::<CommandStdout>();
    input::<CommandStderr>();
    output::<CommandStdin>();
}

#[test]
fn test_bridge_status_contract_defaults_to_no_reported_stream_failure() {
    use qubit_command::session::contracts::SessionBridge;
    let bridge = BridgeDefaults;
    assert!(!bridge.stdout_incomplete());
    assert!(!bridge.stderr_incomplete());
    assert!(!bridge.stdin_broken_pipe());
    assert!(!bridge.output_budget_exceeded());
    assert_eq!(bridge.output_budget_limit(), None);
    assert_eq!(bridge.output_bytes_observed(), 0);
}
