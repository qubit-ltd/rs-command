// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::sync::Arc;
#[cfg(all(test, windows))]
use std::sync::mpsc::Sender;
use std::thread;

use super::captured_output::CapturedOutput;
use super::io_cancellation::IoCancellation;
use super::output_capture_error::OutputCaptureError;
use super::output_reader_state::OutputReaderState;
use super::total_output_budget::TotalOutputBudget;
use crate::OutputStream;

/// Output reader thread result type.
#[derive(Debug)]
pub(in crate::command_runner) struct OutputReader {
    /// Join handle for the worker that drains one child output pipe.
    state: OutputReaderState,
    /// Cancellation state used to interrupt the worker during cleanup.
    cancellation: IoCancellation,
    /// Budget whose bytes are transferred only after this reader joins.
    budget: Option<(Arc<TotalOutputBudget>, OutputStream)>,
}

impl OutputReader {
    /// Creates an output reader from its worker thread and cancellation handle.
    ///
    /// # Parameters
    ///
    /// * `join` - Worker thread collecting one output stream.
    /// * `cancellation` - Handle used to stop the worker during cleanup.
    ///
    /// # Returns
    ///
    /// An output reader owning the worker resources.
    #[inline]
    pub(in crate::command_runner) fn new(
        join: thread::JoinHandle<Result<CapturedOutput, OutputCaptureError>>,
        cancellation: IoCancellation,
    ) -> Self {
        Self {
            state: OutputReaderState::Running { join },
            cancellation,
            budget: None,
        }
    }

    /// Creates a budgeted output reader that transfers its bytes after join.
    ///
    /// # Parameters
    ///
    /// * `join` - Output-reader worker thread.
    /// * `cancellation` - Cancellation sender for that worker.
    /// * `budget` - Shared output budget used to retain bytes.
    /// * `stream` - Stream attributed to this reader.
    #[inline]
    pub(in crate::command_runner) fn new_with_budget(
        join: thread::JoinHandle<Result<CapturedOutput, OutputCaptureError>>,
        cancellation: IoCancellation,
        budget: Arc<TotalOutputBudget>,
        stream: OutputStream,
    ) -> Self {
        Self {
            state: OutputReaderState::Running { join },
            cancellation,
            budget: Some((budget, stream)),
        }
    }

    /// Returns whether the worker thread has stopped.
    ///
    /// # Returns
    ///
    /// `true` when the worker has stopped; otherwise, `false`.
    #[must_use]
    #[inline]
    pub(in crate::command_runner) fn is_finished(&self) -> bool {
        match &self.state {
            OutputReaderState::Running { join } => join.is_finished(),
            OutputReaderState::Finished { .. } => true,
        }
    }

    /// Caches a completed result and reports whether it contains a read
    /// failure.
    pub(in crate::command_runner) fn poll_read_failure(&mut self) -> bool {
        if matches!(&self.state, OutputReaderState::Running { join } if join.is_finished()) {
            let old = std::mem::replace(
                &mut self.state,
                OutputReaderState::Finished {
                    result: Ok(CapturedOutput::default()),
                },
            );
            if let OutputReaderState::Running { join } = old {
                self.state = OutputReaderState::Finished {
                    result: join_reader(join),
                };
                self.apply_budget();
            }
        }
        matches!(
            &self.state,
            OutputReaderState::Finished {
                result: Err(OutputCaptureError::Read { .. } | OutputCaptureError::ReadAfterWrite { .. }),
            }
        )
    }

    /// Clones the retained budget prefix when this reader cannot be joined.
    ///
    /// The snapshot is incomplete because the live reader may still append
    /// bytes after this point. Readers without a shared budget have no
    /// retained fallback data.
    pub(in crate::command_runner) fn detached_output(&self) -> CapturedOutput {
        let mut output = self
            .budget
            .as_ref()
            .map_or_else(CapturedOutput::default, |(budget, stream)| {
                budget.captured_stream(*stream)
            });
        output.complete = false;
        output
    }

    /// Requests cancellation of the worker thread.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if waking the worker fails, except when the wakeup
    /// channel is already closed, or if interrupting the worker fails.
    #[must_use = "handle output-reader cancellation failures"]
    #[inline]
    pub(in crate::command_runner) fn cancel(&self) -> std::io::Result<()> {
        match &self.state {
            OutputReaderState::Running { join } => self.cancellation.cancel(join),
            OutputReaderState::Finished { .. } => Ok(()),
        }
    }

    /// Attaches a Windows-only probe observer to the next cancellation call.
    #[cfg(all(test, windows))]
    /// # Parameters
    pub(in crate::command_runner) fn observe_cancellation_for_test(&mut self, observer: Sender<Option<i32>>) {
        self.cancellation.observe_test_cancellation(observer);
    }

    /// Joins the worker thread and returns its capture result.
    ///
    /// # Returns
    ///
    /// Returns the captured stream bytes and metadata after the worker joins.
    ///
    /// # Errors
    ///
    /// Returns an output-capture error when reading or teeing failed. A worker
    /// panic is mapped to a read error; its original panic payload is not
    /// returned.
    ///
    /// When a total budget is configured, retained bytes are moved from that
    /// budget after joining and replace the capture data in either the success
    /// or error result.
    #[must_use = "handle both thread and output capture failures"]
    #[inline]
    pub(in crate::command_runner) fn join(self) -> Result<CapturedOutput, OutputCaptureError> {
        let Self { state, budget, .. } = self;
        let result = match state {
            OutputReaderState::Running { join } => join_reader(join),
            OutputReaderState::Finished { result } => result,
        };
        apply_budget_to_result(budget, result)
    }

    /// Transfers the retained budget bytes into the cached result once.
    fn apply_budget(&mut self) {
        if let OutputReaderState::Finished { result } = std::mem::replace(
            &mut self.state,
            OutputReaderState::Finished {
                result: Ok(CapturedOutput::default()),
            },
        ) {
            self.state = OutputReaderState::Finished {
                result: apply_budget_to_result(self.budget.take(), result),
            };
        }
    }
}

/// Joins one completed worker and maps a panic to a read error.
///
/// # Parameters
///
/// * `join` - Worker handle known to have completed.
///
/// # Returns
///
/// The worker's capture result, or a read error if it panicked.
fn join_reader(
    join: thread::JoinHandle<Result<CapturedOutput, OutputCaptureError>>,
) -> Result<CapturedOutput, OutputCaptureError> {
    match join.join() {
        Ok(result) => result,
        Err(_) => Err(OutputCaptureError::Read {
            source: std::io::Error::other("output reader thread panicked"),
            output: CapturedOutput {
                complete: false,
                ..CapturedOutput::default()
            },
        }),
    }
}

/// Replaces result bytes with the retained prefix owned by the total budget.
///
/// # Parameters
///
/// * `budget` - Optional budget and stream whose retained bytes are
///   transferred.
/// * `result` - Capture result whose error cause must remain intact.
///
/// # Returns
///
/// The result with bytes sourced from the budget when one is configured.
fn apply_budget_to_result(
    budget: Option<(Arc<TotalOutputBudget>, OutputStream)>,
    result: Result<CapturedOutput, OutputCaptureError>,
) -> Result<CapturedOutput, OutputCaptureError> {
    match budget {
        Some((budget, stream)) => {
            let output = budget.take_captured_stream(stream);
            match result {
                Ok(_) => Ok(output),
                Err(error) => Err(replace_error_output(error, output)),
            }
        }
        None => result,
    }
}

/// Replaces an error's captured data while preserving its failure reason.
///
/// The replacement carries the bytes retained by the shared budget after this
/// reader joins; the original `Read`, `Write`, or `ReadAfterWrite` cause and
/// its associated source/path information remain authoritative.
/// # Parameters
///
/// * `error` - Existing capture error whose failure cause and source
///   information are preserved.
/// * `output` - Captured data produced by the reader.
fn replace_error_output(error: OutputCaptureError, output: CapturedOutput) -> OutputCaptureError {
    match error {
        OutputCaptureError::Read { source, .. } => OutputCaptureError::Read { source, output },
        OutputCaptureError::Write { path, source, .. } => OutputCaptureError::Write { path, source, output },
        OutputCaptureError::ReadAfterWrite {
            path,
            write_source,
            read_source,
            ..
        } => OutputCaptureError::ReadAfterWrite {
            path,
            write_source,
            read_source,
            output,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::io::Cursor;
    use std::sync::Arc;
    use std::thread;

    use super::super::captured_output::CapturedOutput;
    use super::super::io_cancellation::IoCancellation;
    use super::super::output_capture_error::OutputCaptureError;
    use super::super::total_output_budget::TotalOutputBudget;
    use super::OutputReader;
    use crate::OutputStream;

    #[test]
    fn test_poll_read_failure_caches_result_and_consumes_it_once() {
        let (cancellation, _) = IoCancellation::pair().expect("cancellation should create");
        let reader = OutputReader::new(
            thread::spawn(|| {
                Err(OutputCaptureError::Read {
                    source: io::Error::other("injected read failure"),
                    output: CapturedOutput {
                        bytes: b"partial".to_vec(),
                        truncated: false,
                        complete: false,
                    },
                })
            }),
            cancellation,
        );
        let mut reader = reader;
        while !reader.is_finished() {
            thread::yield_now();
        }

        assert!(reader.poll_read_failure());
        assert!(reader.poll_read_failure());
        assert!(reader.is_finished());
        assert!(reader.cancel().is_ok());
        let error = reader.join().expect_err("cached read failure must be returned");
        assert!(matches!(error, OutputCaptureError::Read { output, .. } if output.bytes == b"partial"));
    }

    #[test]
    fn test_poll_read_failure_ignores_success_and_write_only_failure() {
        let (cancellation, _) = IoCancellation::pair().expect("cancellation should create");
        let mut reader = OutputReader::new(thread::spawn(|| Ok(CapturedOutput::default())), cancellation);
        while !reader.is_finished() {
            thread::yield_now();
        }
        assert!(!reader.poll_read_failure());
        assert!(reader.join().is_ok());

        let (cancellation, _) = IoCancellation::pair().expect("cancellation should create");
        let mut reader = OutputReader::new(
            thread::spawn(|| {
                Err(OutputCaptureError::Write {
                    path: "tee".into(),
                    source: io::Error::other("tee failed"),
                    output: CapturedOutput::default(),
                })
            }),
            cancellation,
        );
        while !reader.is_finished() {
            thread::yield_now();
        }
        assert!(!reader.poll_read_failure());
        assert!(matches!(reader.join(), Err(OutputCaptureError::Write { .. })));
    }

    #[test]
    fn test_poll_read_failure_flags_read_after_write_and_preserves_tee_error() {
        let (cancellation, _) = IoCancellation::pair().expect("cancellation should create");
        let mut reader = OutputReader::new(
            thread::spawn(|| {
                Err(OutputCaptureError::ReadAfterWrite {
                    path: "tee".into(),
                    write_source: io::Error::other("tee failed"),
                    read_source: io::Error::other("read failed later"),
                    output: CapturedOutput::default(),
                })
            }),
            cancellation,
        );
        while !reader.is_finished() {
            thread::yield_now();
        }
        assert!(reader.poll_read_failure());
        assert!(matches!(
            reader.join(),
            Err(OutputCaptureError::ReadAfterWrite { write_source, .. })
                if write_source.to_string() == "tee failed"
        ));
    }

    #[test]
    fn test_poll_read_failure_maps_worker_panic_to_read_failure() {
        let (cancellation, _) = IoCancellation::pair().expect("cancellation should create");
        let mut reader = OutputReader::new(
            thread::spawn(|| -> Result<CapturedOutput, OutputCaptureError> {
                panic!("injected reader panic");
            }),
            cancellation,
        );
        while !reader.is_finished() {
            thread::yield_now();
        }
        assert!(reader.poll_read_failure());
        assert!(
            matches!(reader.join(), Err(OutputCaptureError::Read { source, .. }) if source.to_string() == "output reader thread panicked")
        );
    }

    #[test]
    fn test_poll_read_failure_transfers_budget_capture_once() {
        let budget = Arc::new(TotalOutputBudget::new(16, None, None));
        let mut source = Cursor::new(b"budget bytes".to_vec());
        let mut buffer = [0; 32];
        let buffer_len = buffer.len();
        let read = budget
            .read_ready(&mut source, &mut buffer, buffer_len, OutputStream::Stdout)
            .expect("budget fixture read should succeed");
        assert_eq!(read, b"budget bytes".len());
        let (cancellation, _) = IoCancellation::pair().expect("cancellation should create");
        let mut reader = OutputReader::new_with_budget(
            thread::spawn(|| {
                Err(OutputCaptureError::Read {
                    source: io::Error::other("read failed"),
                    output: CapturedOutput::default(),
                })
            }),
            cancellation,
            Arc::clone(&budget),
            OutputStream::Stdout,
        );
        while !reader.is_finished() {
            thread::yield_now();
        }
        assert!(reader.poll_read_failure());
        let Err(OutputCaptureError::Read { output, .. }) = reader.join() else {
            panic!("cached budget reader failure should be returned");
        };
        assert_eq!(output.bytes, b"budget bytes");
        assert!(budget.take_captured_stream(OutputStream::Stdout).bytes.is_empty());
    }
}
