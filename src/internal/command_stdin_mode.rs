// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stdin categories used to validate managed resource compatibility.

/// Describes whether command stdin supplies an explicit file or byte input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommandStdinMode {
    /// Null or inherited stdin, with no explicit file or byte input.
    Default,
    /// Buffered bytes supplied as command stdin.
    Bytes,
    /// A file path supplied as command stdin.
    File,
}
