//! Owned file handoff contracts.
use qubit_operation::OperationScope;
/// Resource inputs consumed by preparation.
pub trait ResourceInput: Send {
    fn prepare_files(
        self: Box<Self>,
        scope: &OperationScope,
    ) -> Result<PreparedFiles, FilePreparationFailure>;
}
/// Files prepared for a process backend; concrete file layout remains private to its owner.
#[derive(Debug, Default)]
pub struct PreparedFiles {
    pub(crate) _private: (),
}
/// Preparation error retains its partial owner for recovery.
pub struct FilePreparationFailure {
    pub message: String,
    pub input: Option<Box<dyn ResourceInput>>,
    pub prepared: Option<PreparedFiles>,
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
