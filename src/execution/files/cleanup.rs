//! Owned files use the operation scope's default resource lifecycle.
//!
//! `OperationScope::reserve_resource` reserves cleanup capacity before preparation has
//! side effects, and `ResourceReservation::commit` registers each `Mutex<File>` owner.
//! A helper clones the corresponding `ResourceRef`; the manager cannot settle the
//! resource until the prepared owner and every helper reference have been dropped.
