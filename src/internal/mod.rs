// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private command construction and run-option representations.

pub(crate) mod command_argument;
pub(crate) mod command_run_options_parts;
pub(crate) mod command_stdin;
pub(crate) mod command_stdin_mode;
pub(crate) mod diagnostic_source;

/// Invokes one task waker without allowing its panic to escape notification.
///
/// The callback runs after callers release their state locks. A panic payload
/// is dropped under a second unwind boundary because user supplied payloads
/// may panic from `Drop` as well.
///
/// # Parameters
/// * `waker`: The task notification callback to invoke.
pub(crate) fn wake_waker(waker: std::task::Waker) {
    let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake())) else {
        return;
    };
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(payload))) {
        std::mem::forget(payload);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    struct PanicOnDrop;
    impl Drop for PanicOnDrop {
        fn drop(&mut self) {
            panic!("controlled panic payload drop");
        }
    }

    struct PanicPayloadWake;
    impl std::task::Wake for PanicPayloadWake {
        fn wake(self: Arc<Self>) {
            std::panic::panic_any(PanicOnDrop);
        }
    }

    #[test]
    fn test_wake_waker_contains_panicking_payload_destructor() {
        super::wake_waker(std::task::Waker::from(Arc::new(PanicPayloadWake)));
    }
}
