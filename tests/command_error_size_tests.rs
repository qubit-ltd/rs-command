// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Cross-platform size tests for [`CommandError`](qubit_command::CommandError).

use std::mem::size_of;

use qubit_command::CommandError;

const MAX_COMMAND_ERROR_SIZE: usize = 96;
const _: () = assert!(size_of::<CommandError>() <= MAX_COMMAND_ERROR_SIZE);

#[test]
fn test_command_error_remains_small_enough_for_result_returns() {
    assert!(size_of::<CommandError>() <= MAX_COMMAND_ERROR_SIZE);
}
