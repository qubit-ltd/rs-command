// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::{
    io,
    sync::{
        Arc,
        atomic::{
            AtomicBool,
            Ordering,
        },
    },
    thread,
};

/// Stdin writer thread and its cancellation state.
#[derive(Debug)]
pub(in crate::command_runner) struct StdinWriter {
    pub(in crate::command_runner) join: thread::JoinHandle<io::Result<()>>,
    pub(in crate::command_runner) cancellation: Arc<AtomicBool>,
}

impl StdinWriter {
    pub(in crate::command_runner) fn new(
        join: thread::JoinHandle<io::Result<()>>,
        cancellation: Arc<AtomicBool>,
    ) -> Self {
        Self { join, cancellation }
    }

    #[must_use]
    pub(in crate::command_runner) fn is_finished(&self) -> bool {
        self.join.is_finished()
    }

    pub(in crate::command_runner) fn cancel(&self) {
        self.cancellation.store(true, Ordering::Release);
        super::cancel::cancel_synchronous_io(&self.join);
    }

    pub(in crate::command_runner) fn join(
        self,
    ) -> thread::Result<io::Result<()>> {
        self.join.join()
    }
}

/// Optional stdin writer helper.
pub(in crate::command_runner) type OptionalStdinWriter = Option<StdinWriter>;
