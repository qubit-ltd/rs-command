// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::PreparedFiles;
use super::ResourceInput;

/// Preparation error retains its partial input and any files already
/// registered.
pub struct FilePreparationFailure {
    /// Stores the message value used by this execution component.
    pub message: String,
    /// Stores the input value used by this execution component.
    pub input: Option<Box<dyn ResourceInput>>,
    /// Stores the prepared value used by this execution component.
    pub prepared: Option<Box<PreparedFiles>>,
}
impl std::fmt::Debug for FilePreparationFailure {
    /// Stores the fmt value used by this execution component.
    ///
    /// # Parameters
    /// * `f`: Supplies the f value used by this operation.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilePreparationFailure")
            .field("message", &self.message)
            .field("input_retained", &self.input.is_some())
            .field("prepared_retained", &self.prepared.is_some())
            .finish()
    }
}
