//! Outcome storage is shared by the typed handle and lifecycle observer.
/// Stores the outcome value used by this execution component.
///
/// # Type Parameters
/// * `T`: Outcome value returned by the operation.
/// * `E`: Error value returned by the operation.
pub(crate) type Outcome<T, E> = qubit_operation::OperationOutcome<T, E>;
