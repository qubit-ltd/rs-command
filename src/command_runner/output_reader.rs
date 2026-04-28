/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
use std::thread;

use super::{
    captured_output::CapturedOutput,
    output_capture_error::OutputCaptureError,
};

/// Output reader thread result type.
pub(crate) type OutputReader = thread::JoinHandle<Result<CapturedOutput, OutputCaptureError>>;
