// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Timer that changes clock domains after its first clock observation.

use std::io;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use qubit_clock::ManualMonotonicClock;
use qubit_clock::MonotonicClock;
use qubit_clock::MonotonicInstant;
use qubit_clock::TimeError;
use qubit_clock::Timer;
use qubit_clock::TimerFuture;
use qubit_clock::TimerUnavailableError;

pub(crate) struct SwitchingTimer {
    first: ManualMonotonicClock,
    second: ManualMonotonicClock,
    observations: AtomicUsize,
    stable_observations: usize,
}

impl SwitchingTimer {
    pub(crate) fn new() -> Self {
        Self::after_observations(1)
    }

    pub(crate) fn after_observations(stable_observations: usize) -> Self {
        Self {
            first: ManualMonotonicClock::new(),
            second: ManualMonotonicClock::new(),
            observations: AtomicUsize::new(0),
            stable_observations,
        }
    }
}

impl Timer for SwitchingTimer {
    fn clock(&self) -> &dyn MonotonicClock {
        if self.observations.fetch_add(1, Ordering::Relaxed)
            < self.stable_observations
        {
            &self.first
        } else {
            &self.second
        }
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
