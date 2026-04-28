/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
use std::{
    io::Write,
    path::PathBuf,
};

/// Streaming destination for captured output.
pub(crate) struct OutputTee {
    /// Writer receiving all emitted bytes.
    pub(crate) writer: Box<dyn Write + Send>,
    /// Path used for diagnostics if writes fail.
    pub(crate) path: PathBuf,
}
