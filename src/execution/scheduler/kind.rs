/// Scheduler-local accounting identity carried by a reservation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    /// Describes the active case handled by managed command execution.
    Active,
    /// Describes the queued case handled by managed command execution.
    Queued,
}
