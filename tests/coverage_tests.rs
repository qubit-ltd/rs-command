// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Coverage-only integration entry points.

#[cfg(coverage)]
use qubit_command::__coverage_internal;

#[cfg(coverage)]
#[test]
fn test_internal_coverage_probes() {
    __coverage_internal();
}
