// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for [`CommandCancellation`](qubit_command::CommandCancellation).

use qubit_command::CommandCancellation;

#[test]
fn test_command_cancellation_is_shared_and_one_shot() {
    let cancellation = CommandCancellation::new();
    let shared = cancellation.clone();

    assert!(!cancellation.is_cancelled());
    shared.cancel();

    assert!(cancellation.is_cancelled());
    cancellation.cancel();
    assert!(shared.is_cancelled());
}
