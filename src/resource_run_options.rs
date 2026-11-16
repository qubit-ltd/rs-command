//! Owned-resource options for managed command execution.
use crate::CommandRunOptions;
use std::fs::File;

/// Explicit treatment of an owned output sink during preparation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileSinkPolicy {
    /// Preserve existing contents and the file's current cursor; never seek implicitly.
    Preserve,
    /// Truncate to zero and position at the beginning during worker preparation.
    Truncate,
}

/// Owned options for one managed command run.
///
/// The stdin file is read from its current cursor without an implicit seek. Output sinks
/// require an explicit policy. Truncation occurs only after command capacity and resource
/// reservations succeed, and is an observable side effect. File identity checks happen
/// during preparation and compare handles, so path aliases do not bypass conflict checks.
/// This type is intentionally not `Clone` because it transfers file ownership.
#[derive(Debug, Default)]
pub struct CommandResourceRunOptions {
    /// Ordinary per-run command settings.
    options: CommandRunOptions,
    stdin: Option<File>,
    stdout: Option<OwnedSink>,
    stderr: Option<OwnedSink>,
}

#[derive(Debug)]
pub(crate) struct OwnedSink {
    pub(crate) file: File,
    pub(crate) policy: FileSinkPolicy,
}

/// Crate-private moved parts consumed by owned-file preparation.
#[derive(Debug)]
pub(crate) struct ResourceRunOptionsParts {
    pub(crate) options: CommandRunOptions,
    pub(crate) stdin: Option<File>,
    pub(crate) stdout: Option<OwnedSink>,
    pub(crate) stderr: Option<OwnedSink>,
}

impl CommandResourceRunOptions {
    /// Creates resource options with default command settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets ordinary per-run command settings.
    pub fn options(mut self, options: CommandRunOptions) -> Self {
        self.options = options;
        self
    }

    /// Transfers an owned stdin file, read from its current cursor.
    pub fn stdin_file(mut self, file: File) -> Self {
        self.stdin = Some(file);
        self
    }

    /// Transfers an owned stdout sink with an explicit content policy.
    pub fn stdout_sink(mut self, file: File, policy: FileSinkPolicy) -> Self {
        self.stdout = Some(OwnedSink { file, policy });
        self
    }

    /// Transfers an owned stderr sink with an explicit content policy.
    pub fn stderr_sink(mut self, file: File, policy: FileSinkPolicy) -> Self {
        self.stderr = Some(OwnedSink { file, policy });
        self
    }

    /// Borrows the configured stdin file without transferring it.
    pub fn configured_stdin(&self) -> Option<&File> {
        self.stdin.as_ref()
    }

    /// Returns the configured stdout path and explicit sink policy.
    pub fn configured_stdout(&self) -> Option<(&File, FileSinkPolicy)> {
        self.stdout.as_ref().map(|sink| (&sink.file, sink.policy))
    }

    /// Returns the configured stderr path and explicit sink policy.
    pub fn configured_stderr(&self) -> Option<(&File, FileSinkPolicy)> {
        self.stderr.as_ref().map(|sink| (&sink.file, sink.policy))
    }

    /// Transfers all resource options to worker-side preparation.
    pub(crate) fn into_parts(self) -> ResourceRunOptionsParts {
        ResourceRunOptionsParts {
            options: self.options,
            stdin: self.stdin,
            stdout: self.stdout,
            stderr: self.stderr,
        }
    }
}
