// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// File stream associated with managed preparation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FilePreparationDirection {
    /// Owned stdin.
    Stdin,
    /// Owned stdout sink.
    Stdout,
    /// Owned stderr sink.
    Stderr,
}
