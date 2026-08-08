// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for [`CommandErrorKind`](qubit_command::CommandErrorKind).

use qubit_command::CommandErrorKind;

#[test]
fn test_command_error_kind_is_copy_and_comparable() {
    let original = CommandErrorKind::UnexpectedExit;
    let copied = original;
    assert_eq!(original, copied);
    assert_ne!(original, CommandErrorKind::TimedOut);
}
