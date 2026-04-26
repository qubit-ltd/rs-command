/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
use std::{
    ffi::{
        OsStr,
        OsString,
    },
    path::{
        Path,
        PathBuf,
    },
};

/// Structured description of an external command to run.
///
/// `Command` stores a program and argument vector instead of parsing a
/// shell-like command line. This avoids quoting ambiguity and accidental shell
/// injection. Use [`Self::shell`] only when shell parsing, redirection,
/// expansion, or pipes are intentionally required.
///
/// # Author
///
/// Haixing Hu
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// Program executable name or path.
    program: OsString,
    /// Positional arguments passed to the program.
    args: Vec<OsString>,
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

/// Standard input configuration for a command.
///
/// This type stays internal so the public builder API can evolve without
/// exposing process-spawning details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandStdin {
    /// Connect stdin to null input.
    Null,
    /// Inherit stdin from the parent process.
    Inherit,
    /// Write these bytes to the child process stdin.
    Bytes(Vec<u8>),
    /// Read stdin bytes from this file.
    File(PathBuf),
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
    #[inline]
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

    /// Adds one positional argument.
    ///
    /// # Parameters
    ///
    /// * `arg` - Argument to append.
    ///
    /// # Returns
    ///
    /// The updated command.
    #[inline]
    pub fn arg(mut self, arg: &str) -> Self {
        self.args.push(OsString::from(arg));
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
    #[inline]
    pub fn arg_os<S>(mut self, arg: S) -> Self
    where
        S: AsRef<OsStr>,
    {
        self.args.push(arg.as_ref().to_owned());
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
        self.args.extend(args.iter().map(OsString::from));
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
        self.args
            .extend(args.into_iter().map(|arg| arg.as_ref().to_owned()));
        self
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
    #[inline]
    pub fn working_directory<P>(mut self, working_directory: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.working_directory = Some(working_directory.into());
        self
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
    #[inline]
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
    #[inline]
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
    /// Environment variables added after this call are still passed to the child
    /// process.
    ///
    /// # Returns
    ///
    /// The updated command.
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
    pub fn stdin_null(mut self) -> Self {
        self.stdin = CommandStdin::Null;
        self
    }

    /// Inherits stdin from the parent process.
    ///
    /// # Returns
    ///
    /// The updated command.
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
    pub fn stdin_file<P>(mut self, path: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.stdin = CommandStdin::File(path.into());
        self
    }

    /// Returns the executable name or path.
    ///
    /// # Returns
    ///
    /// Program executable name or path as an [`OsStr`].
    #[inline]
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    /// Returns the configured argument list.
    ///
    /// # Returns
    ///
    /// Borrowed argument list in submission order.
    #[inline]
    pub fn arguments(&self) -> &[OsString] {
        &self.args
    }

    /// Returns the per-command working directory override.
    ///
    /// # Returns
    ///
    /// `Some(path)` when the command has a working directory override, or
    /// `None` when the runner default should be used.
    #[inline]
    pub fn working_directory_override(&self) -> Option<&Path> {
        self.working_directory.as_deref()
    }

    /// Returns environment variable overrides.
    ///
    /// # Returns
    ///
    /// Borrowed environment variable entries in insertion order.
    #[inline]
    pub fn environment(&self) -> &[(OsString, OsString)] {
        &self.envs
    }

    /// Returns environment variable removals.
    ///
    /// # Returns
    ///
    /// Borrowed environment variable names removed before spawning the command.
    #[inline]
    pub fn removed_environment(&self) -> &[OsString] {
        &self.removed_envs
    }

    /// Returns whether the inherited environment is cleared.
    ///
    /// # Returns
    ///
    /// `true` when the command should start from an empty environment.
    #[inline]
    pub const fn clears_environment(&self) -> bool {
        self.clear_environment
    }

    /// Consumes the command and returns the configured stdin behavior.
    ///
    /// # Returns
    ///
    /// Owned stdin configuration used by the runner.
    #[inline]
    pub(crate) fn into_stdin_configuration(self) -> CommandStdin {
        self.stdin
    }

    /// Formats this command for diagnostics.
    ///
    /// # Returns
    ///
    /// An argv-style command string suitable for logs and errors.
    pub(crate) fn display_command(&self) -> String {
        let mut parts = Vec::with_capacity(self.args.len() + 1);
        parts.push(self.program.as_os_str());
        for arg in &self.args {
            parts.push(arg.as_os_str());
        }
        format!("{parts:?}")
    }
}

/// Compares environment variable names using platform semantics.
///
/// # Parameters
///
/// * `left` - First environment variable name.
/// * `right` - Second environment variable name.
///
/// # Returns
///
/// `true` when both names refer to the same environment entry on the current
/// platform. Unix uses byte-preserving exact comparison; Windows uses
/// case-insensitive comparison because Windows environment variable names are
/// case-insensitive.
#[cfg(not(windows))]
fn env_key_eq(left: &OsStr, right: &OsStr) -> bool {
    left == right
}

/// Compares environment variable names using Windows semantics.
///
/// # Parameters
///
/// * `left` - First environment variable name.
/// * `right` - Second environment variable name.
///
/// # Returns
///
/// `true` when both names are equal after Unicode uppercase folding.
#[cfg(windows)]
fn env_key_eq(left: &OsStr, right: &OsStr) -> bool {
    let left = left.to_string_lossy();
    let right = right.to_string_lossy();
    left.chars()
        .flat_map(char::to_uppercase)
        .eq(right.chars().flat_map(char::to_uppercase))
}
