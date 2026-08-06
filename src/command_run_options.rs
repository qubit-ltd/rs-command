// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
use std::path::{Path, PathBuf};

use crate::CommandCancellation;

/// Per-run options for [`CommandRunner`](crate::command_runner::CommandRunner).
///
/// Each [`CommandRunner`](crate::command_runner::CommandRunner) has process-level
/// defaults (timeout, logging, capture policy, etc.). This type carries run-level
/// configuration that must not be shared across concurrent runs, such as cancellation
/// and tee destinations.
#[derive(Clone, Default)]
#[must_use]
pub struct CommandRunOptions {
    /// Optional per-run cancellation handle.
    cancellation: Option<CommandCancellation>,
    /// Optional per-run stdout tee path.
    stdout_file: Option<PathBuf>,
    /// Optional per-run stderr tee path.
    stderr_file: Option<PathBuf>,
}

impl std::fmt::Debug for CommandRunOptions {
    /// Render redacted debug output.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommandRunOptions")
            .field("cancellation_configured", &self.cancellation.is_some())
            .field(
                "stdout_file",
                &self.stdout_file.as_ref().map(|_| "<redacted path>"),
            )
            .field(
                "stderr_file",
                &self.stderr_file.as_ref().map(|_| "<redacted path>"),
            )
            .finish()
    }
}

impl CommandRunOptions {
    /// Creates an empty run options value.
    ///
    /// # Returns
    ///
    /// A run option set with no cancellation and no tee files.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Configures a one-shot cancellation handle for this run.
    ///
    /// Clone this handle before passing it to multiple runs that should share a
    /// single cancellation request channel.
    pub fn cancellation(mut self, cancellation: CommandCancellation) -> Self {
        self.cancellation = Some(cancellation);
        self
    }

    /// Streams stdout to a file while still capturing it in memory.
    ///
    /// The path is validated by the runner before spawning. Paths are recorded
    /// per run so concurrent runs can use independent tee destinations.
    #[inline]
    pub fn tee_stdout_to_file<P>(mut self, path: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.stdout_file = Some(path.into());
        self
    }

    /// Streams stderr to a file while still capturing it in memory.
    ///
    /// The path is validated by the runner before spawning. Paths are recorded
    /// per run so concurrent runs can use independent tee destinations.
    #[inline]
    pub fn tee_stderr_to_file<P>(mut self, path: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.stderr_file = Some(path.into());
        self
    }

    /// Returns configured cancellation, if any.
    #[inline]
    pub fn configured_cancellation(&self) -> Option<&CommandCancellation> {
        self.cancellation.as_ref()
    }

    /// Returns the configured stdout tee path, if any.
    #[inline]
    pub fn configured_stdout_file(&self) -> Option<&Path> {
        self.stdout_file.as_deref()
    }

    /// Returns the configured stderr tee path, if any.
    #[inline]
    pub fn configured_stderr_file(&self) -> Option<&Path> {
        self.stderr_file.as_deref()
    }

    pub(crate) fn into_parts(self) -> CommandRunOptionsParts {
        CommandRunOptionsParts {
            cancellation: self.cancellation,
            stdout_file: self.stdout_file,
            stderr_file: self.stderr_file,
        }
    }
}

#[derive(Clone)]
pub(crate) struct CommandRunOptionsParts {
    pub(crate) cancellation: Option<CommandCancellation>,
    pub(crate) stdout_file: Option<PathBuf>,
    pub(crate) stderr_file: Option<PathBuf>,
}
