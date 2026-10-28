// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
use std::sync::Mutex;

use super::captured_output::CapturedOutput;
use crate::CommandCapturedOutput;
use crate::OutputStream;

/// Per-run aggregate stdout/stderr hard limit.
#[derive(Debug)]
pub(in crate::command_runner) struct TotalOutputBudget {
    limit: u64,
    state: Mutex<State>,
    max_stdout_bytes: Option<usize>,
    max_stderr_bytes: Option<usize>,
}

#[derive(Debug)]
struct State {
    used: u128,
    exceeded: bool,
    halted: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
    stdout_complete: bool,
    stderr_complete: bool,
}

impl TotalOutputBudget {
    pub(in crate::command_runner) fn new(
        limit: u64,
        max_stdout_bytes: Option<usize>,
        max_stderr_bytes: Option<usize>,
    ) -> Self {
        Self {
            limit,
            max_stdout_bytes,
            max_stderr_bytes,
            state: Mutex::new(State {
                used: 0,
                exceeded: false,
                halted: false,
                stdout: Vec::new(),
                stderr: Vec::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                stdout_complete: false,
                stderr_complete: false,
            }),
        }
    }

    /// Reads only bytes already reported ready. The caller must establish
    /// readiness first.
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

    pub(in crate::command_runner) fn exceeded(&self) -> Option<(u64, u128)> {
        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.exceeded.then_some((self.limit, state.used))
    }

    pub(in crate::command_runner) fn is_halted(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .halted
    }

    pub(in crate::command_runner) fn mark_complete(&self, stream: OutputStream) {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match stream {
            OutputStream::Stdout => state.stdout_complete = true,
            OutputStream::Stderr => state.stderr_complete = true,
        }
    }

    pub(in crate::command_runner) fn captured_output(&self) -> CommandCapturedOutput {
        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        CommandCapturedOutput::new(
            (state.stdout.clone(), state.stdout_truncated, state.stdout_complete),
            (state.stderr.clone(), state.stderr_truncated, state.stderr_complete),
        )
    }

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
}

fn retain(state: &mut State, stream: OutputStream, max_stdout: Option<usize>, max_stderr: Option<usize>, chunk: &[u8]) {
    let State {
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
    use std::io::Cursor;

    use super::TotalOutputBudget;
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
}
