// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Captures log records emitted by command-runner integration tests.

use std::sync::Mutex;
use std::sync::Once;

use log::Level;
use log::LevelFilter;
use log::Log;
use log::Metadata;
use log::Record;
use log::set_logger;
use log::set_max_level;

/// Stores formatted log records for assertions in integration tests.
struct CapturedLogger {
    /// Captured level and message pairs.
    records: Mutex<Vec<(Level, String)>>,
}

impl Log for CapturedLogger {
    /// Reports whether the supplied metadata should be recorded.
    ///
    /// # Parameters
    ///
    /// * `metadata` - Metadata for the candidate record.
    ///
    /// # Returns
    ///
    /// Always `true` so tests can inspect every enabled level.
    #[inline(always)]
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    /// Stores one enabled record.
    ///
    /// # Parameters
    ///
    /// * `record` - Log record whose level and formatted message are retained.
    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            self.records
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((record.level(), record.args().to_string()));
        }
    }

    /// Flushes buffered records.
    ///
    /// Records are stored synchronously, so no flush work is required.
    #[inline(always)]
    fn flush(&self) {}
}

/// Shared logger used by this integration-test process.
static CAPTURED_LOGGER: CapturedLogger = CapturedLogger {
    records: Mutex::new(Vec::new()),
};

/// One-time guard for installing [`CAPTURED_LOGGER`].
static INSTALL_LOGGER: Once = Once::new();

/// Installs the captured logger and enables all log levels.
///
/// Repeated calls are harmless. The first call installs the process-global
/// logger, and later calls only observe the completed initialization.
pub(crate) fn initialize_captured_logger() {
    INSTALL_LOGGER.call_once(|| {
        set_logger(&CAPTURED_LOGGER).expect("captured test logger should install exactly once");
        set_max_level(LevelFilter::Trace);
    });
}

/// Returns captured records whose formatted message contains `marker`.
///
/// Marker filtering isolates assertions from unrelated tests that may run in
/// parallel in the same integration-test process.
///
/// # Parameters
///
/// * `marker` - Unique text identifying records from one test command.
///
/// # Returns
///
/// Cloned level and message pairs containing `marker`.
pub(crate) fn captured_log_records_containing(marker: &str) -> Vec<(Level, String)> {
    CAPTURED_LOGGER
        .records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .filter(|(_, message)| message.contains(marker))
        .cloned()
        .collect()
}
