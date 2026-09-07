// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unit tests for run-event classification and stop-reason mapping.

use std::io;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(windows)]
use std::os::windows::process::ExitStatusExt;
use std::process::ExitStatus;
use std::time::Duration;

use qubit_clock::TimeError;

use super::run_event::RunEvent;
use super::stop_reason::StopReason;
use crate::CommandErrorKind;

#[cfg(unix)]
fn status(code: i32) -> ExitStatus {
    ExitStatus::from_raw(code << 8)
}

#[cfg(windows)]
fn status(code: i32) -> ExitStatus {
    ExitStatus::from_raw(code as u32)
}

#[derive(Clone, Copy)]
enum ExpectedStop {
    TimedOut,
    Cancelled,
    WaitFailed,
    TimeFailed,
}

#[test]
fn test_run_event_maps_non_exit_events_to_stop_reasons() {
    let timeout = Duration::from_secs(3);
    let cases = [
        (
            RunEvent::TimedOut {
                timeout,
                status: None,
            },
            ExpectedStop::TimedOut,
            None,
        ),
        (
            RunEvent::TimedOut {
                timeout,
                status: Some(status(21)),
            },
            ExpectedStop::TimedOut,
            Some(21),
        ),
        (
            RunEvent::Cancelled { status: None },
            ExpectedStop::Cancelled,
            None,
        ),
        (
            RunEvent::Cancelled {
                status: Some(status(22)),
            },
            ExpectedStop::Cancelled,
            Some(22),
        ),
        (
            RunEvent::WaitFailed(io::Error::other("wait failed")),
            ExpectedStop::WaitFailed,
            None,
        ),
        (
            RunEvent::TimeFailed {
                source: TimeError::InstantOverflow,
                status: None,
            },
            ExpectedStop::TimeFailed,
            None,
        ),
        (
            RunEvent::TimeFailed {
                source: TimeError::InstantOverflow,
                status: Some(status(23)),
            },
            ExpectedStop::TimeFailed,
            Some(23),
        ),
    ];

    for (event, expected, expected_code) in cases {
        let reason = event
            .into_exit_status()
            .expect_err("non-exit event should map to a stop reason");
        assert_eq!(
            reason.observed_status().and_then(|status| status.code()),
            expected_code
        );
        assert!(match (reason, expected) {
            (StopReason::TimedOut { .. }, ExpectedStop::TimedOut)
            | (StopReason::Cancelled { .. }, ExpectedStop::Cancelled)
            | (StopReason::WaitFailed(_), ExpectedStop::WaitFailed)
            | (StopReason::TimeFailed { .. }, ExpectedStop::TimeFailed) => true,
            _ => false,
        });
    }
}

#[test]
fn test_run_event_keeps_normal_exit_separate_from_stop_reasons() {
    let event = RunEvent::Exited(status(7));

    let exit_status = event
        .into_exit_status()
        .expect("normal exit should retain its status");

    assert_eq!(exit_status.code(), Some(7));
}

#[test]
fn test_stop_reason_builds_each_primary_error_kind() {
    let cases = [
        (
            StopReason::TimedOut {
                timeout: Duration::from_secs(3),
                status: None,
            },
            CommandErrorKind::TimedOut,
        ),
        (
            StopReason::Cancelled { status: None },
            CommandErrorKind::Cancelled,
        ),
        (
            StopReason::WaitFailed(io::Error::other("wait failed")),
            CommandErrorKind::WaitFailed,
        ),
        (
            StopReason::TimeFailed {
                source: TimeError::InstantOverflow,
                status: None,
            },
            CommandErrorKind::TimeFailed,
        ),
    ];

    for (reason, expected_kind) in cases {
        let error = reason.into_primary_error("command", None);
        assert_eq!(error.kind(), expected_kind);
    }
}
