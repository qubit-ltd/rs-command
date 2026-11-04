// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::sync::Mutex;

mod total_output_budget_state;
use self::total_output_budget_state::TotalOutputBudgetState;
use super::captured_output::CapturedOutput;
use crate::CommandCapturedOutput;
use crate::OutputStream;

/// Per-run aggregate stdout/stderr hard limit and bounded failure snapshot.
///
/// The `limit + 1` sentinel byte is charged to the stream that observes it so
/// the runner can distinguish an exact-limit EOF from exceeded output. Each
/// stream retains only its configured prefix, while `complete` is set only
/// after that stream reaches EOF. Snapshots clone retained data because a
/// reader may still be running when cancellation or process termination fails;
/// callers that have successfully joined a reader should use its returned
/// output as the authoritative stream result.
#[derive(Debug)]
pub(in crate::command_runner) struct TotalOutputBudget {
    /// Configured combined byte limit; one extra byte is read as a sentinel.
    limit: u64,
    /// Serializes combined accounting and both retained stream prefixes.
    state: Mutex<TotalOutputBudgetState>,
    /// Independent stdout retention cap.
    max_stdout_bytes: Option<usize>,
    /// Independent stderr retention cap.
    max_stderr_bytes: Option<usize>,
}

impl TotalOutputBudget {
    /// Creates an empty budget with configured combined and per-stream limits.
    ///
    /// # Parameters
    ///
    /// * `limit` - Maximum combined number of bytes accepted before the
    ///   sentinel byte. The budget may read and charge one additional byte to
    ///   detect exceeded output.
    /// * `max_stdout_bytes` - Optional maximum number of stdout bytes retained
    ///   in snapshots.
    /// * `max_stderr_bytes` - Optional maximum number of stderr bytes retained
    ///   in snapshots.
    ///
    /// # Returns
    ///
    /// A shared accounting budget with empty stream buffers and no stream
    /// marked complete, halted, exceeded, or transferred.
    pub(in crate::command_runner) fn new(
        limit: u64,
        max_stdout_bytes: Option<usize>,
        max_stderr_bytes: Option<usize>,
    ) -> Self {
        Self {
            limit,
            max_stdout_bytes,
            max_stderr_bytes,
            state: Mutex::new(TotalOutputBudgetState {
                used: 0,
                exceeded: false,
                halted: false,
                stdout: Vec::new(),
                stderr: Vec::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                stdout_complete: false,
                stderr_complete: false,
                stdout_transferred: false,
                stderr_transferred: false,
            }),
        }
    }

    /// Reads at most the bytes already reported ready and allowed by the
    /// remaining combined budget, then accounts for and retains those bytes.
    ///
    /// The caller must establish readiness before calling and provide the
    /// exclusive reader for `stream`; the mutex serializes accounting and
    /// retained prefixes across stdout and stderr. It remains locked while
    /// `reader.read`, accounting, and retention run. The budget permits one
    /// extra sentinel byte beyond `limit`; observing it marks the budget
    /// exceeded and halted. Per-stream retention caps may truncate stored
    /// bytes without changing the number charged to the combined budget.
    ///
    /// A zero return does not by itself mean EOF: it can also mean the budget
    /// was already halted or the requested read length was zero. Only a zero
    /// returned by the underlying reader establishes EOF for the caller, which
    /// records that separately with [`Self::mark_complete`].
    ///
    /// # Parameters
    ///
    /// * `reader` - The stream reader, which must have been reported ready.
    /// * `buffer` - Destination buffer for bytes read.
    /// * `ready_capacity` - Maximum number of bytes the readiness check
    ///   established can be read without blocking.
    /// * `stream` - Stream whose bytes are charged and retained.
    ///
    /// # Returns
    ///
    /// The number of bytes read and accounted, possibly zero as described
    /// above.
    ///
    /// # Errors
    ///
    /// Returns the underlying reader's error. No bytes from a failed read are
    /// accounted or retained.
    pub(in crate::command_runner) fn read_ready(
        &self,
        reader: &mut dyn std::io::Read,
        buffer: &mut [u8],
        ready_capacity: usize,
        stream: OutputStream,
    ) -> std::io::Result<usize> {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.halted {
            return Ok(0);
        }
        let ceiling = u128::from(self.limit) + 1;
        let remaining = ceiling.saturating_sub(state.used);
        if remaining == 0 {
            state.exceeded = true;
            state.halted = true;
            return Ok(0);
        }
        let request = buffer
            .len()
            .min(ready_capacity)
            .min(usize::try_from(remaining).unwrap_or(usize::MAX));
        if request == 0 {
            return Ok(0);
        }
        let read = reader.read(&mut buffer[..request])?;
        state.used += read as u128;
        retain(
            &mut state,
            stream,
            self.max_stdout_bytes,
            self.max_stderr_bytes,
            &buffer[..read],
        );
        if state.used > u128::from(self.limit) {
            state.exceeded = true;
            state.halted = true;
        }
        Ok(read)
    }

    /// Returns the configured limit and observed count after the sentinel.
    pub(in crate::command_runner) fn exceeded(&self) -> Option<(u64, u128)> {
        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.exceeded.then_some((self.limit, state.used))
    }

    /// Returns whether the budget has stopped further stream reads.
    pub(in crate::command_runner) fn is_halted(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .halted
    }

    /// Records that the selected stream reached EOF.
    /// # Parameters
    pub(in crate::command_runner) fn mark_complete(&self, stream: OutputStream) {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match stream {
            OutputStream::Stdout => state.stdout_complete = true,
            OutputStream::Stderr => state.stderr_complete = true,
        }
    }

    /// Clones a point-in-time snapshot; a live reader may still be appending.
    pub(in crate::command_runner) fn captured_output(&self) -> CommandCapturedOutput {
        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        CommandCapturedOutput::new(
            (state.stdout.clone(), state.stdout_truncated, state.stdout_complete),
            (state.stderr.clone(), state.stderr_truncated, state.stderr_complete),
        )
    }

    /// Combines joined stream results with snapshots for streams not joined.
    ///
    /// For a stream whose retained bytes were transferred after its reader was
    /// joined, the corresponding joined result is authoritative, including
    /// its bytes, truncation flag, and EOF-derived completeness flag. If no
    /// joined result is supplied for such a stream, this returns no bytes and
    /// marks it incomplete. Streams not transferred use a mutex-protected
    /// point-in-time snapshot; their reader may still append bytes, so the
    /// snapshot may be incomplete. Truncation describes bytes omitted by a
    /// per-stream retention cap; completeness describes reaching EOF, and
    /// neither is implied by the other.
    /// # Parameters
    pub(in crate::command_runner) fn captured_output_with_joined(
        &self,
        joined: Option<&CommandCapturedOutput>,
    ) -> CommandCapturedOutput {
        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (stdout_bytes, stdout_truncated, stdout_complete) = if state.stdout_transferred {
            joined.map_or_else(
                || (Vec::new(), state.stdout_truncated, false),
                |output| {
                    (
                        output.stdout().to_vec(),
                        output.stdout_truncated(),
                        output.stdout_complete(),
                    )
                },
            )
        } else {
            (state.stdout.clone(), state.stdout_truncated, state.stdout_complete)
        };
        let (stderr_bytes, stderr_truncated, stderr_complete) = if state.stderr_transferred {
            joined.map_or_else(
                || (Vec::new(), state.stderr_truncated, false),
                |output| {
                    (
                        output.stderr().to_vec(),
                        output.stderr_truncated(),
                        output.stderr_complete(),
                    )
                },
            )
        } else {
            (state.stderr.clone(), state.stderr_truncated, state.stderr_complete)
        };
        CommandCapturedOutput::new(
            (stdout_bytes, stdout_truncated, stdout_complete),
            (stderr_bytes, stderr_truncated, stderr_complete),
        )
    }

    /// Clones one stream's current snapshot while its reader may still run.
    /// The returned bytes and flags describe the state while the mutex was
    /// held; a live reader may append more bytes or later reach EOF. A
    /// truncated snapshot may still be incomplete, and a snapshot is not a
    /// joined reader result.
    /// # Parameters
    pub(in crate::command_runner) fn captured_stream(&self, stream: OutputStream) -> CapturedOutput {
        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match stream {
            OutputStream::Stdout => CapturedOutput {
                bytes: state.stdout.clone(),
                truncated: state.stdout_truncated,
                complete: state.stdout_complete,
            },
            OutputStream::Stderr => CapturedOutput {
                bytes: state.stderr.clone(),
                truncated: state.stderr_truncated,
                complete: state.stderr_complete,
            },
        }
    }

    /// Moves one stream's retained bytes after its reader has been joined.
    ///
    /// The transfer flag makes a second take explicit: it returns empty bytes
    /// with the original metadata and cannot present an empty buffer as the
    /// original complete output.
    /// # Parameters
    pub(in crate::command_runner) fn take_captured_stream(&self, stream: OutputStream) -> CapturedOutput {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match stream {
            OutputStream::Stdout => {
                let truncated = state.stdout_truncated;
                let complete = state.stdout_complete;
                if state.stdout_transferred {
                    return CapturedOutput {
                        bytes: Vec::new(),
                        truncated,
                        complete,
                    };
                }
                state.stdout_transferred = true;
                CapturedOutput {
                    bytes: std::mem::take(&mut state.stdout),
                    truncated,
                    complete,
                }
            }
            OutputStream::Stderr => {
                let truncated = state.stderr_truncated;
                let complete = state.stderr_complete;
                if state.stderr_transferred {
                    return CapturedOutput {
                        bytes: Vec::new(),
                        truncated,
                        complete,
                    };
                }
                state.stderr_transferred = true;
                CapturedOutput {
                    bytes: std::mem::take(&mut state.stderr),
                    truncated,
                    complete,
                }
            }
        }
    }

    /// Returns stream flags without retaining or cloning its byte buffer.
    /// # Parameters
    pub(in crate::command_runner) fn captured_stream_metadata(&self, stream: OutputStream) -> CapturedOutput {
        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (truncated, complete) = match stream {
            OutputStream::Stdout => (state.stdout_truncated, state.stdout_complete),
            OutputStream::Stderr => (state.stderr_truncated, state.stderr_complete),
        };
        CapturedOutput {
            bytes: Vec::new(),
            truncated,
            complete,
        }
    }
}

/// Retains only the configured prefix and records omitted bytes as truncation.
/// # Parameters
///
/// * `state` - Mutable retained-byte and truncation state.
/// * `stream` - Output stream whose state is updated or returned.
/// * `max_stdout` - Optional maximum retained stdout bytes.
/// * `max_stderr` - Optional maximum retained stderr bytes.
/// * `chunk` - Bytes read from one output stream.
fn retain(
    state: &mut TotalOutputBudgetState,
    stream: OutputStream,
    max_stdout: Option<usize>,
    max_stderr: Option<usize>,
    chunk: &[u8],
) {
    let TotalOutputBudgetState {
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        ..
    } = state;
    let (bytes, truncated, maximum) = match stream {
        OutputStream::Stdout => (stdout, stdout_truncated, max_stdout),
        OutputStream::Stderr => (stderr, stderr_truncated, max_stderr),
    };
    match maximum {
        Some(maximum) => {
            let remaining = maximum.saturating_sub(bytes.len());
            bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
            if chunk.len() > remaining {
                *truncated = true;
            }
        }
        None => bytes.extend_from_slice(chunk),
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::io::Cursor;
    use std::sync::Arc;
    use std::thread;

    use super::super::io_cancellation::IoCancellation;
    use super::super::output_capture_error::OutputCaptureError;
    use super::super::output_collector::join_output_reader;
    use super::super::output_reader::OutputReader;
    use super::CapturedOutput;
    use super::TotalOutputBudget;
    use crate::CommandCapturedOutput;
    use crate::OutputStream;

    #[test]
    fn test_total_output_zero_limit_uses_one_byte_sentinel() {
        let budget = TotalOutputBudget::new(0, None, None);
        let mut reader = Cursor::new(b"x");
        let mut buffer = [0; 8];
        assert_eq!(
            budget
                .read_ready(&mut reader, &mut buffer, 8, OutputStream::Stdout)
                .expect("read succeeds"),
            1
        );
        assert_eq!(budget.exceeded(), Some((0, 1)));
    }

    #[test]
    fn test_total_output_u64_max_limit_uses_wider_accounting() {
        let budget = TotalOutputBudget::new(u64::MAX, None, None);
        let mut reader = Cursor::new(b"small");
        let mut buffer = [0; 8];
        assert_eq!(
            budget
                .read_ready(&mut reader, &mut buffer, 8, OutputStream::Stdout)
                .expect("read succeeds"),
            5
        );
        assert_eq!(budget.exceeded(), None);
    }

    #[test]
    fn test_total_output_take_moves_buffer_once_and_preserves_metadata() {
        let budget = TotalOutputBudget::new(8, None, None);
        let mut reader = Cursor::new(b"payload");
        let mut buffer = [0; 8];
        budget
            .read_ready(&mut reader, &mut buffer, 8, OutputStream::Stdout)
            .expect("read succeeds");
        let expected_ptr = budget
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .stdout
            .as_ptr();

        let first = budget.take_captured_stream(OutputStream::Stdout);
        let second = budget.take_captured_stream(OutputStream::Stdout);

        assert_eq!(first.bytes.as_ptr(), expected_ptr);
        assert_eq!(first.bytes, b"payload");
        assert!(!first.complete);
        assert!(second.bytes.is_empty());
        assert!(!second.complete);
        assert!(budget.captured_output().stdout().is_empty());

        let joined = CommandCapturedOutput::new((b"joined".to_vec(), false, true), (b"err".to_vec(), false, true));
        let merged = budget.captured_output_with_joined(Some(&joined));
        assert_eq!(merged.stdout(), b"joined");
        assert!(merged.stdout_complete());
        assert!(merged.stderr().is_empty());
        assert!(!merged.stderr_complete());
        let missing_joined = budget.captured_output_with_joined(None);
        assert!(!missing_joined.stdout_complete());
    }

    #[test]
    fn test_total_output_error_join_moves_buffer_without_copy() {
        let budget = Arc::new(TotalOutputBudget::new(32, None, None));
        let mut reader = Cursor::new(b"payload");
        let mut buffer = [0; 8];
        budget
            .read_ready(&mut reader, &mut buffer, 8, OutputStream::Stdout)
            .expect("read succeeds");
        let expected_ptr = budget
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .stdout
            .as_ptr();

        let (cancellation, _token) = IoCancellation::pair().expect("cancellation pair initializes");
        let reader = OutputReader::new_with_budget(
            thread::spawn(|| {
                Err(OutputCaptureError::Read {
                    source: io::Error::other("injected read failure"),
                    output: CapturedOutput::default(),
                })
            }),
            cancellation,
            budget,
            OutputStream::Stdout,
        );

        let error = join_output_reader(reader).expect_err("reader failure is preserved");
        let OutputCaptureError::Read { output, .. } = error else {
            panic!("expected reader error");
        };

        assert_eq!(output.bytes.as_ptr(), expected_ptr);
        assert_eq!(output.bytes, b"payload");
    }
}
