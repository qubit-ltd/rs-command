// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Timer that deterministically rejects every deadline registration.

use std::io;

use qubit_clock::{
    ManualMonotonicClock,
    MonotonicClock,
    MonotonicInstant,
    TimeError,
    Timer,
    TimerFuture,
    TimerUnavailableError,
};

pub(crate) struct FailingTimer {
    clock: ManualMonotonicClock,
}

impl FailingTimer {
    pub(crate) fn new() -> Self {
        Self {
            clock: ManualMonotonicClock::new(),
        }
    }
}

impl Timer for FailingTimer {
    fn clock(&self) -> &dyn MonotonicClock {
        &self.clock
    }

    fn at(
        &self,
        _deadline: MonotonicInstant,
    ) -> Result<TimerFuture, TimeError> {
        Err(TimeError::TimerUnavailable {
            source: TimerUnavailableError::BackendUnavailable {
                backend: "test",
                source: Box::new(io::Error::other(
                    "test timer backend unavailable",
                )),
            },
        })
    }
}
