/// Scheduler-local accounting identity carried by a reservation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    Active,
    Queued,
}
