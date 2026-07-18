// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::{
    ffi::{
        OsStr,
        OsString,
    },
    fmt,
    path::{
        Path,
        PathBuf,
    },
};

use qubit_sanitize::{
    ArgvSanitizer,
    EnvSanitizer,
    FieldSanitizer,
    NameMatchMode,
    SensitivityLevel,
};

use crate::command_argument::CommandArgument;
use crate::command_env::env_key_eq;
use crate::command_stdin::CommandStdin;

const COMMAND_LOG_MATCH_MODE: NameMatchMode = NameMatchMode::ExactOrSuffix;
const SHELL_COMMAND_REPLACEMENT: &str = "<shell command>";

/// Structured description of an external command to run.
///
/// `Command` stores a program and argument vector instead of parsing a
/// shell-like command line. This avoids quoting ambiguity and accidental shell
/// injection. Use [`Self::shell`] only when shell parsing, redirection,
/// expansion, or pipes are intentionally required.
///
/// # Examples
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_command::Command;
///
/// Command::new("true");
/// ```
#[derive(Clone, PartialEq, Eq)]
#[must_use]
pub struct Command {
    /// Program executable name or path.
    program: OsString,
    /// Positional arguments passed to the program.
    args: Vec<CommandArgument>,
    /// Working directory override for this command.
    working_directory: Option<PathBuf>,
    /// Whether the command should clear inherited environment variables.
    clear_environment: bool,
    /// Environment variables added or overridden for this command.
    envs: Vec<(OsString, OsString)>,
    /// Environment variables removed for this command.
    removed_envs: Vec<OsString>,
    /// Standard input configuration for this command.
    stdin: CommandStdin,
}

impl fmt::Debug for Command {
    /// Formats this command without exposing sensitive log values.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let field_sanitizer = FieldSanitizer::default();
        formatter
            .debug_struct("Command")
            .field("argv", &self.sanitized_argv(&field_sanitizer))
            .field("working_directory", &self.working_directory)
            .field("clear_environment", &self.clear_environment)
            .field(
                "env",
                &self.sanitized_environment_assignments(&field_sanitizer),
            )
            .field("unset", &self.removed_environment_names())
            .field("stdin", &self.stdin)
            .finish()
    }
}

impl Command {
    /// Creates a command from a program name or path.
    ///
    /// # Parameters
    ///
    /// * `program` - Executable name or path to run.
    ///
    /// # Returns
    ///
    /// A command with no arguments or per-command overrides.
    #[inline(always)]
    pub fn new(program: &str) -> Self {
        Self::new_os(program)
    }

    /// Creates a command from a program name or path that may not be UTF-8.
    ///
    /// # Parameters
    ///
    /// * `program` - Executable name or path to run.
    ///
    /// # Returns
    ///
    /// A command with no arguments or per-command overrides.
    #[inline]
    pub fn new_os<S>(program: S) -> Self
    where
        S: AsRef<OsStr>,
    {
        Self {
            program: program.as_ref().to_owned(),
            args: Vec::new(),
            working_directory: None,
            clear_environment: false,
            envs: Vec::new(),
            removed_envs: Vec::new(),
            stdin: CommandStdin::Null,
        }
    }

    /// Creates a command executed through the platform shell.
    ///
    /// On Unix-like platforms this creates `sh -c <command_line>`. On Windows
    /// this creates `cmd /C <command_line>`. Prefer [`Self::new`] with explicit
    /// arguments when shell parsing is not required.
    ///
    /// # Parameters
    ///
    /// * `command_line` - Shell command line to execute.
    ///
    /// # Returns
    ///
    /// A command that invokes the platform shell.
    #[cfg(not(windows))]
    #[inline]
    pub fn shell(command_line: &str) -> Self {
        Self::new("sh").arg("-c").arg(command_line)
    }

    /// Creates a command executed through the platform shell.
    ///
    /// On Windows this creates `cmd /C <command_line>`. Prefer [`Self::new`]
    /// with explicit arguments when shell parsing is not required.
    ///
    /// # Parameters
    ///
    /// * `command_line` - Shell command line to execute.
    ///
    /// # Returns
    ///
    /// A command that invokes the platform shell.
    #[cfg(windows)]
    #[inline]
    pub fn shell(command_line: &str) -> Self {
        Self::new("cmd").arg("/C").arg(command_line)
    }

    /// Returns the executable name or path.
    ///
    /// # Returns
    ///
    /// Program executable name or path as an [`OsStr`].
    #[must_use]
    #[inline(always)]
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    /// Returns the configured argument list.
    ///
    /// # Returns
    ///
    /// Borrowed raw argument values in submission order.
    #[inline(always)]
    pub fn arguments(&self) -> impl ExactSizeIterator<Item = &OsStr> {
        self.args.iter().map(CommandArgument::value)
    }

    /// Adds one positional argument.
    ///
    /// # Parameters
    ///
    /// * `arg` - Argument to append.
    ///
    /// # Returns
    ///
    /// The updated command.
    #[inline(always)]
    pub fn arg(mut self, arg: &str) -> Self {
        self.args
            .push(CommandArgument::visible(OsString::from(arg)));
        self
    }

    /// Adds one positional argument that may not be UTF-8.
    ///
    /// # Parameters
    ///
    /// * `arg` - Argument to append.
    ///
    /// # Returns
    ///
    /// The updated command.
    #[inline(always)]
    pub fn arg_os<S>(mut self, arg: S) -> Self
    where
        S: AsRef<OsStr>,
    {
        self.args
            .push(CommandArgument::visible(arg.as_ref().to_owned()));
        self
    }

    /// Adds one positional argument whose value is redacted in diagnostics.
    ///
    /// The original value is still passed unchanged to the child process.
    ///
    /// # Parameters
    ///
    /// * `arg` - Sensitive argument to append.
    ///
    /// # Returns
    ///
    /// The updated command.
    #[inline(always)]
    pub fn sensitive_arg(mut self, arg: &str) -> Self {
        self.args
            .push(CommandArgument::sensitive(OsString::from(arg)));
        self
    }

    /// Adds a possibly non-UTF-8 argument redacted in diagnostics.
    ///
    /// The original value is still passed unchanged to the child process.
    ///
    /// # Parameters
    ///
    /// * `arg` - Sensitive argument to append.
    ///
    /// # Returns
    ///
    /// The updated command.
    #[inline(always)]
    pub fn sensitive_arg_os<S>(mut self, arg: S) -> Self
    where
        S: AsRef<OsStr>,
    {
        self.args
            .push(CommandArgument::sensitive(arg.as_ref().to_owned()));
        self
    }

    /// Adds multiple positional arguments.
    ///
    /// # Parameters
    ///
    /// * `args` - Arguments to append in order.
    ///
    /// # Returns
    ///
    /// The updated command.
    #[inline]
    pub fn args(mut self, args: &[&str]) -> Self {
        self.args.extend(
            args.iter()
                .map(|arg| CommandArgument::visible(OsString::from(arg))),
        );
        self
    }

    /// Adds multiple positional arguments that may not be UTF-8.
    ///
    /// # Parameters
    ///
    /// * `args` - Arguments to append in order.
    ///
    /// # Returns
    ///
    /// The updated command.
    pub fn args_os<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args.extend(
            args.into_iter()
                .map(|arg| CommandArgument::visible(arg.as_ref().to_owned())),
        );
        self
    }

    /// Returns the per-command working directory override.
    ///
    /// # Returns
    ///
    /// `Some(path)` when the command has a working directory override, or
    /// `None` when the runner default should be used.
    #[inline(always)]
    pub fn working_directory_override(&self) -> Option<&Path> {
        self.working_directory.as_deref()
    }

    /// Sets a per-command working directory.
    ///
    /// # Parameters
    ///
    /// * `working_directory` - Directory used as the child process working
    ///   directory.
    ///
    /// # Returns
    ///
    /// The updated command.
    #[inline(always)]
    pub fn working_directory<P>(mut self, working_directory: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.working_directory = Some(working_directory.into());
        self
    }

    /// Returns environment variable overrides.
    ///
    /// # Returns
    ///
    /// Borrowed environment variable entries in insertion order.
    #[must_use]
    #[inline(always)]
    pub fn environment(&self) -> &[(OsString, OsString)] {
        &self.envs
    }

    /// Returns environment variable removals.
    ///
    /// # Returns
    ///
    /// Borrowed environment variable names removed before spawning the command.
    #[must_use]
    #[inline(always)]
    pub fn removed_environment(&self) -> &[OsString] {
        &self.removed_envs
    }

    /// Returns whether the inherited environment is cleared.
    ///
    /// # Returns
    ///
    /// `true` when the command should start from an empty environment.
    #[must_use]
    #[inline(always)]
    pub const fn clears_environment(&self) -> bool {
        self.clear_environment
    }

    /// Adds or overrides an environment variable for this command.
    ///
    /// # Parameters
    ///
    /// * `key` - Environment variable name.
    /// * `value` - Environment variable value.
    ///
    /// # Returns
    ///
    /// The updated command.
    #[inline(always)]
    pub fn env(mut self, key: &str, value: &str) -> Self {
        self = self.env_os(key, value);
        self
    }

    /// Adds or overrides an environment variable that may not be UTF-8.
    ///
    /// # Parameters
    ///
    /// * `key` - Environment variable name.
    /// * `value` - Environment variable value.
    ///
    /// # Returns
    ///
    /// The updated command.
    pub fn env_os<K, V>(mut self, key: K, value: V) -> Self
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        let key = key.as_ref().to_owned();
        let value = value.as_ref().to_owned();
        self.removed_envs
            .retain(|removed| !env_key_eq(removed, &key));
        self.envs
            .retain(|(existing_key, _)| !env_key_eq(existing_key, &key));
        self.envs.push((key, value));
        self
    }

    /// Removes an inherited or previously configured environment variable.
    ///
    /// # Parameters
    ///
    /// * `key` - Environment variable name to remove.
    ///
    /// # Returns
    ///
    /// The updated command.
    #[inline(always)]
    pub fn env_remove(mut self, key: &str) -> Self {
        self = self.env_remove_os(key);
        self
    }

    /// Removes an environment variable whose name may not be UTF-8.
    ///
    /// # Parameters
    ///
    /// * `key` - Environment variable name to remove.
    ///
    /// # Returns
    ///
    /// The updated command.
    pub fn env_remove_os<S>(mut self, key: S) -> Self
    where
        S: AsRef<OsStr>,
    {
        let key = key.as_ref().to_owned();
        self.envs
            .retain(|(existing_key, _)| !env_key_eq(existing_key, &key));
        self.removed_envs
            .retain(|removed| !env_key_eq(removed, &key));
        self.removed_envs.push(key);
        self
    }

    /// Clears all inherited environment variables for this command.
    ///
    /// Environment variables added after this call are still passed to the
    /// child process.
    ///
    /// # Returns
    ///
    /// The updated command.
    #[inline(always)]
    pub fn env_clear(mut self) -> Self {
        self.clear_environment = true;
        self.envs.clear();
        self.removed_envs.clear();
        self
    }

    /// Connects the command stdin to null input.
    ///
    /// # Returns
    ///
    /// The updated command.
    #[inline(always)]
    pub fn stdin_null(mut self) -> Self {
        self.stdin = CommandStdin::Null;
        self
    }

    /// Inherits stdin from the parent process.
    ///
    /// # Returns
    ///
    /// The updated command.
    #[inline(always)]
    pub fn stdin_inherit(mut self) -> Self {
        self.stdin = CommandStdin::Inherit;
        self
    }

    /// Writes bytes to the child process stdin.
    ///
    /// The runner writes the bytes on a helper thread after spawning the child
    /// process, then closes stdin so the child can observe EOF.
    ///
    /// # Parameters
    ///
    /// * `bytes` - Bytes to send to stdin.
    ///
    /// # Returns
    ///
    /// The updated command.
    #[inline(always)]
    pub fn stdin_bytes<B>(mut self, bytes: B) -> Self
    where
        B: Into<Vec<u8>>,
    {
        self.stdin = CommandStdin::Bytes(bytes.into());
        self
    }

    /// Reads child process stdin from a file.
    ///
    /// # Parameters
    ///
    /// * `path` - File path to open and connect to stdin.
    ///
    /// # Returns
    ///
    /// The updated command.
    #[inline(always)]
    pub fn stdin_file<P>(mut self, path: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.stdin = CommandStdin::File(path.into());
        self
    }

    /// Consumes the command and returns the configured stdin behavior.
    ///
    /// # Returns
    ///
    /// Owned stdin configuration used by the runner.
    #[inline(always)]
    pub(crate) fn into_stdin_configuration(self) -> CommandStdin {
        self.stdin
    }

    /// Formats this command for diagnostics.
    ///
    /// # Returns
    ///
    /// A sanitized command string suitable for logs and errors.
    #[must_use]
    pub(crate) fn display_command(
        &self,
        field_sanitizer: &FieldSanitizer,
    ) -> String {
        let argv = self.sanitized_argv(field_sanitizer);
        if self.envs.is_empty() && self.removed_envs.is_empty() {
            return format!("{argv:?}");
        }

        let env = self.sanitized_environment_assignments(field_sanitizer);
        let unset = self.removed_environment_names();
        format!("Command {{ env: {env:?}, unset: {unset:?}, argv: {argv:?} }}")
    }

    /// Builds sanitized argv tokens for diagnostics.
    ///
    /// # Returns
    ///
    /// Sanitized argv tokens with secret-looking values masked.
    #[must_use]
    fn sanitized_argv(&self, field_sanitizer: &FieldSanitizer) -> Vec<String> {
        ArgvSanitizer::new(field_sanitizer.clone())
            .sanitize_argv_with_sensitivity(
                self.argv_for_display(),
                COMMAND_LOG_MATCH_MODE,
            )
    }

    /// Builds argv tokens with opaque shell payloads hidden.
    ///
    /// # Returns
    ///
    /// Owned argv tokens suitable for structured sanitization.
    #[must_use]
    fn argv_for_display(&self) -> Vec<(OsString, Option<SensitivityLevel>)> {
        let shell_payload_index = self.shell_payload_arg_index();
        let mut argv = Vec::with_capacity(self.args.len() + 1);
        argv.push((self.program.clone(), None));
        for (index, arg) in self.args.iter().enumerate() {
            if Some(index) == shell_payload_index {
                argv.push((OsString::from(SHELL_COMMAND_REPLACEMENT), None));
            } else {
                argv.push((arg.value().to_owned(), arg.sensitivity()));
            }
        }
        argv
    }

    /// Locates the shell script argument generated by [`Self::shell`].
    ///
    /// # Returns
    ///
    /// `Some(index)` for the argument containing shell script text, or `None`
    /// when this command is not a recognized shell invocation.
    fn shell_payload_arg_index(&self) -> Option<usize> {
        if self.args.len() < 2 {
            return None;
        }
        let first_arg = self.args.first()?.value();
        if self.program.as_os_str() == OsStr::new("sh")
            && first_arg == OsStr::new("-c")
        {
            return Some(1);
        }

        let program = self.program.to_string_lossy();
        let first_arg = first_arg.to_string_lossy();
        if (program.eq_ignore_ascii_case("cmd")
            || program.eq_ignore_ascii_case("cmd.exe"))
            && first_arg.eq_ignore_ascii_case("/C")
        {
            return Some(1);
        }
        None
    }

    /// Builds sanitized environment assignments for diagnostics.
    ///
    /// # Returns
    ///
    /// Sanitized `KEY=value` entries for explicit environment overrides.
    #[must_use]
    fn sanitized_environment_assignments(
        &self,
        field_sanitizer: &FieldSanitizer,
    ) -> Vec<String> {
        let sanitizer = EnvSanitizer::new(field_sanitizer.clone());
        self.envs
            .iter()
            .map(|(key, value)| {
                let (key, value) = sanitizer.sanitize_os_pair(
                    key,
                    value,
                    COMMAND_LOG_MATCH_MODE,
                );
                format!("{key}={value}")
            })
            .collect()
    }

    /// Builds display names for removed environment variables.
    ///
    /// # Returns
    ///
    /// Environment variable names rendered lossily for diagnostics.
    #[must_use]
    fn removed_environment_names(&self) -> Vec<String> {
        self.removed_envs
            .iter()
            .map(|key| key.to_string_lossy().into_owned())
            .collect()
    }
}
