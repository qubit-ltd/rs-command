/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
use std::thread;

use super::{
    captured_output::CapturedOutput,
    output_capture_error::OutputCaptureError,
};

/// Output reader thread result type.
pub(crate) type OutputReader = thread::JoinHandle<Result<CapturedOutput, OutputCaptureError>>;
