// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for [`CommandCleanupFailure`](qubit_command::CommandCleanupFailure).

use std::fmt::Debug;

use qubit_command::CommandCleanupFailure;

#[test]
fn test_command_cleanup_failure_is_debuggable() {
    fn assert_debug<T: Debug>() {}

    assert_debug::<CommandCleanupFailure>();
}
