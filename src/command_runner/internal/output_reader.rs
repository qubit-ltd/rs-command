// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::thread;

use super::{
    captured_output::CapturedOutput,
    io_cancellation::IoCancellation,
    output_capture_error::OutputCaptureError,
};

/// Output reader thread result type.
#[derive(Debug)]
pub(in crate::command_runner) struct OutputReader {
    join: thread::JoinHandle<Result<CapturedOutput, OutputCaptureError>>,
    cancellation: IoCancellation,
}

impl OutputReader {
    pub(in crate::command_runner) fn new(
        join: thread::JoinHandle<Result<CapturedOutput, OutputCaptureError>>,
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
    ) -> thread::Result<Result<CapturedOutput, OutputCaptureError>> {
        self.join.join()
    }
}
