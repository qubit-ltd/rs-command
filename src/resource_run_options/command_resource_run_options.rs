// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public options for transferring owned command input and output files.

use std::fs::File;

use super::file_sink_policy::FileSinkPolicy;
use super::owned_sink::OwnedSink;
use super::resource_run_options_parts::ResourceRunOptionsParts;
use crate::CommandRunOptions;

/// Owned options for one managed command run.
///
/// The stdin file is read from its current cursor without an implicit seek.
/// Output sinks require an explicit policy. Truncation occurs only after
/// command capacity and resource reservations succeed, and is an observable
/// side effect. File identity checks happen during preparation and compare
/// handles, so path aliases do not bypass conflict checks. This type is
/// intentionally not `Clone` because it transfers file ownership.
#[derive(Debug, Default)]
pub struct CommandResourceRunOptions {
    /// Ordinary per-run command settings.
    options: CommandRunOptions,
    /// Optional owned input file read from its current cursor.
    stdin: Option<File>,
    /// Optional owned stdout file and its preparation policy.
    stdout: Option<OwnedSink>,
    /// Optional owned stderr file and its preparation policy.
    stderr: Option<OwnedSink>,
}

impl CommandResourceRunOptions {
    /// Creates resource options with default command settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets ordinary per-run command settings.
    ///
    /// # Parameters
    ///
    /// * `options` - Command-level settings applied to this run.
    ///
    /// # Returns
    ///
    /// Updated resource options carrying the supplied settings.
    pub fn options(mut self, options: CommandRunOptions) -> Self {
        self.options = options;
        self
    }

    /// Borrows ordinary runner settings before owned file options are consumed.
    pub fn ordinary_options(&self) -> &CommandRunOptions {
        &self.options
    }

    /// Transfers an owned stdin file, read from its current cursor.
    ///
    /// # Parameters
    ///
    /// * `file` - Input file whose current cursor is used by the child.
    ///
    /// # Returns
    ///
    /// Updated options owning the input file.
    pub fn stdin_file(mut self, file: File) -> Self {
        self.stdin = Some(file);
        self
    }

    /// Transfers an owned stdout sink with an explicit content policy.
    ///
    /// # Parameters
    ///
    /// * `file` - File receiving captured standard output.
    /// * `policy` - Treatment of existing file contents during preparation.
    ///
    /// # Returns
    ///
    /// Updated options owning the stdout sink.
    pub fn stdout_sink(mut self, file: File, policy: FileSinkPolicy) -> Self {
        self.stdout = Some(OwnedSink { file, policy });
        self
    }

    /// Transfers an owned stderr sink with an explicit content policy.
    ///
    /// # Parameters
    ///
    /// * `file` - File receiving captured standard error.
    /// * `policy` - Treatment of existing file contents during preparation.
    ///
    /// # Returns
    ///
    /// Updated options owning the stderr sink.
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

#[cfg(test)]
mod tests {
    use super::CommandResourceRunOptions;
    use crate::CommandRunOptions;
    use crate::ProcessTreePolicy;
    use crate::resource_run_options::FileSinkPolicy;

    #[test]
    fn test_accessors_report_both_owned_sinks_and_ordinary_options() {
        let stdin = tempfile::tempfile().expect("stdin fixture should be created");
        let stdout = tempfile::tempfile().expect("stdout fixture should be created");
        let stderr = tempfile::tempfile().expect("stderr fixture should be created");
        let command_options = CommandRunOptions::new().with_process_tree_policy(ProcessTreePolicy::DirectChild);
        let options = CommandResourceRunOptions::new()
            .options(command_options)
            .stdin_file(stdin)
            .stdout_sink(stdout, FileSinkPolicy::Preserve)
            .stderr_sink(stderr, FileSinkPolicy::Truncate);

        assert!(options.configured_stdin().is_some());
        assert_eq!(
            options.configured_stdout().map(|(_, policy)| policy),
            Some(FileSinkPolicy::Preserve)
        );
        assert_eq!(
            options.configured_stderr().map(|(_, policy)| policy),
            Some(FileSinkPolicy::Truncate)
        );
        assert_eq!(
            options.ordinary_options().process_tree_policy(),
            ProcessTreePolicy::DirectChild
        );
    }
}
