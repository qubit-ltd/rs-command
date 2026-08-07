// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::{
    io,
    thread,
};

use super::io_cancellation::IoCancellation;

/// Stdin writer thread and its cancellation state.
#[derive(Debug)]
pub(in crate::command_runner) struct StdinWriter {
    pub(in crate::command_runner) join: thread::JoinHandle<io::Result<()>>,
    pub(in crate::command_runner) cancellation: IoCancellation,
}

impl StdinWriter {
    pub(in crate::command_runner) fn new(
        join: thread::JoinHandle<io::Result<()>>,
        cancellation: IoCancellation,
    ) -> Self {
        Self { join, cancellation }
    }

    #[must_use]
    pub(in crate::command_runner) fn is_finished(&self) -> bool {
        self.join.is_finished()
    }

    pub(in crate::command_runner) fn cancel(&self) {
        self.cancellation.cancel(&self.join);
    }

    pub(in crate::command_runner) fn join(
        self,
    ) -> thread::Result<io::Result<()>> {
        self.join.join()
    }
}

/// Optional stdin writer helper.
pub(in crate::command_runner) type OptionalStdinWriter = Option<StdinWriter>;
