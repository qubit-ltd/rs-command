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
    /// Environment variables added or overridden for this command.
    envs: Vec<(OsString, OsString)>,
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
        Self {
            program: OsString::from(program),
            args: Vec::new(),
            working_directory: None,
            envs: Vec::new(),
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
        self.envs.push((OsString::from(key), OsString::from(value)));
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

    /// Formats this command for diagnostics.
    ///
    /// # Returns
    ///
    /// A lossy, human-readable command string suitable for logs and errors.
    pub(crate) fn display_command(&self) -> String {
        let mut text = self.program.to_string_lossy().into_owned();
        for arg in &self.args {
            text.push(' ');
            text.push_str(&arg.to_string_lossy());
        }
        text
    }
}
