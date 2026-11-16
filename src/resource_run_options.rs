//! Owned-resource options for managed command execution.
use crate::CommandRunOptions;

/// Options which transfer file ownership into a managed command request.
#[derive(Debug)]
pub struct CommandResourceRunOptions {
    /// Ordinary per-run command settings.
    pub options: CommandRunOptions,
}
impl Default for CommandResourceRunOptions {
    fn default() -> Self {
        Self {
            options: CommandRunOptions::default(),
        }
    }
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
}
