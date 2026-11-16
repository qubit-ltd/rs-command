// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared ownership for resources referenced by process helpers.
use std::sync::Arc;

/// Represents the resource owner state or contract used by managed command
/// execution.
///
/// # Type Parameters
/// * `T`: Resource value retained through shared ownership.
#[derive(Debug)]
pub(crate) struct ResourceOwner<T>(#[doc = "Shared reference counted resource retained by process helpers."] Arc<T>);

impl<T> Clone for ResourceOwner<T> {
    /// Performs the clone step in the managed command execution lifecycle.
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T> ResourceOwner<T> {
    /// Stores the new value used by this execution component.
    ///
    /// # Parameters
    /// * `value`: Supplies the value value used by this operation.
    pub(crate) fn new(value: T) -> Self {
        Self(Arc::new(value))
    }
    /// Performs the get step in the managed command execution lifecycle.
    pub(crate) fn get(&self) -> &T {
        &self.0
    }
    /// Performs the strong count step in the managed command execution
    /// lifecycle.
    pub(crate) fn strong_count(&self) -> usize {
        Arc::strong_count(&self.0)
    }
}
