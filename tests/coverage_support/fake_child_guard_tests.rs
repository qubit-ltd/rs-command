/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for coverage fake-child guard behavior.

use super::coverage_support_subject;

#[test]
fn test_fake_child_guard_restores_previous_thread_state() {
    assert!(!coverage_support_subject::fake_children_enabled());
    coverage_support_subject::with_fake_children_enabled(|| {
        assert!(coverage_support_subject::fake_children_enabled());
    });
    assert!(!coverage_support_subject::fake_children_enabled());
}
