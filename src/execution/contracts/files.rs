//! Owned file handoff contracts.
use std::fs::File;
use std::sync::Mutex;

use qubit_operation::OperationScope;
use qubit_operation::ResourceRef;

use super::ExecutionSnapshot;
use crate::resource_run_options::FileSinkPolicy;

/// Resource inputs consumed by preparation.
pub trait ResourceInput: Send {
    fn prepare_files(self: Box<Self>, scope: &OperationScope) -> Result<PreparedFiles, FilePreparationFailure>;

    /// Prepares files using the immutable submission-time path/environment
    /// snapshot.
    ///
    /// Implementations with path-based identity checks should override this
    /// method. The default preserves compatibility for resource inputs that
    /// only contain owned handles.
    fn prepare_files_with_snapshot(
        self: Box<Self>,
        scope: &OperationScope,
        _snapshot: &ExecutionSnapshot,
    ) -> Result<PreparedFiles, FilePreparationFailure> {
        self.prepare_files(scope)
    }
}

/// One prepared sink with shared write-safe access and its explicit
/// disposition.
pub(crate) struct PreparedSink {
    pub(crate) file: ResourceRef<Mutex<File>>,
    pub(crate) policy: FileSinkPolicy,
}

/// Prepared files retain registered owners; helpers share files through
/// `Mutex<File>`.
pub struct PreparedFiles {
    pub(crate) stdin: Option<ResourceRef<Mutex<File>>>,
    pub(crate) stdout: Option<PreparedSink>,
    pub(crate) stderr: Option<PreparedSink>,
}
impl PreparedFiles {
    /// Constructs a prepared owner set from resources registered in the
    /// operation scope.
    pub(crate) fn new(
        stdin: Option<ResourceRef<Mutex<File>>>,
        stdout: Option<PreparedSink>,
        stderr: Option<PreparedSink>,
    ) -> Self {
        Self { stdin, stdout, stderr }
    }
}
impl std::fmt::Debug for PreparedFiles {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedFiles")
            .field("stdin", &self.stdin.is_some())
            .field("stdout", &self.stdout.is_some())
            .field("stderr", &self.stderr.is_some())
            .finish()
    }
}

/// Preparation error retains its partial input and any files already
/// registered.
pub struct FilePreparationFailure {
    pub message: String,
    pub input: Option<Box<dyn ResourceInput>>,
    pub prepared: Option<Box<PreparedFiles>>,
}
impl std::fmt::Debug for FilePreparationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilePreparationFailure")
            .field("message", &self.message)
            .field("input_retained", &self.input.is_some())
            .field("prepared_retained", &self.prepared.is_some())
            .finish()
    }
}
