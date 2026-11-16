//! Linearizes cancellation against the first operation with external effects.
use std::sync::Mutex;

#[derive(Debug, Default)]
pub(crate) struct StartGate {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    stopped: bool,
    authorized: bool,
}

#[derive(Debug)]
pub(crate) struct StartPermit(());

impl StartGate {
    /// Returns a permit only when no stop request has won the start race.
    pub(crate) fn authorize(&self) -> Option<StartPermit> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped || state.authorized {
            return None;
        }
        state.authorized = true;
        Some(StartPermit(()))
    }

    /// Records stop without waiting for the authorized system call to finish.
    pub(crate) fn request_stop(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let won_before_start = !state.authorized;
        state.stopped = true;
        won_before_start
    }

    pub(crate) fn stop_requested(&self) -> bool {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).stopped
    }
}

#[cfg(test)]
mod tests {
    use super::StartGate;

    #[test]
    fn stop_wins_start_gate() {
        let gate = StartGate::default();
        assert!(gate.request_stop());
        assert!(gate.authorize().is_none());
    }

    #[test]
    fn side_effect_authorization_wins_stop_gate() {
        let gate = StartGate::default();
        assert!(gate.authorize().is_some());
        assert!(!gate.request_stop());
        assert!(gate.stop_requested());
    }
}
