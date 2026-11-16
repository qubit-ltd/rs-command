//! Process lifetime policy for managed command execution.

/// Controls whether managed execution owns the process tree or only its direct child.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProcessTreePolicy {
    /// Manage descendants as part of the command lifetime.
    #[default]
    Managed,
    /// Stop and wait for only the direct child.
    DirectChild,
}
