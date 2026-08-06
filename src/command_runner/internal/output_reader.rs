// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::{
    sync::{
        Arc,
        atomic::{
            AtomicBool,
            Ordering,
        },
    },
    thread,
};

use super::{
    captured_output::CapturedOutput,
    output_capture_error::OutputCaptureError,
};

/// Output reader thread result type.
#[derive(Debug)]
pub(in crate::command_runner) struct OutputReader {
    join: thread::JoinHandle<Result<CapturedOutput, OutputCaptureError>>,
    cancellation: Arc<AtomicBool>,
}

impl OutputReader {
    pub(in crate::command_runner) fn new(
        join: thread::JoinHandle<Result<CapturedOutput, OutputCaptureError>>,
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
    ) -> thread::Result<Result<CapturedOutput, OutputCaptureError>> {
        self.join.join()
    }
}
