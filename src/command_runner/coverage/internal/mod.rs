// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private helpers for coverage-only command-runner probes.

mod failing_reader;
mod failing_writer;

pub(super) use failing_reader::FailingReader;
pub(super) use failing_writer::FailingWriter;
