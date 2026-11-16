//! Outcome storage is shared by the typed handle and lifecycle observer.
pub(crate) type Outcome<T, E> = qubit_operation::OperationOutcome<T, E>;
