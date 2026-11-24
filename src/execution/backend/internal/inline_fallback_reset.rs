// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

thread_local! {
    static ALLOW_INLINE_REAPER_FALLBACK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Restores a worker's prior permission to perform inline reaper cleanup.
pub(crate) struct InlineFallbackReset(#[doc = "Whether inline fallback was enabled before entering this scope."] bool);

impl Drop for InlineFallbackReset {
    /// Restores the thread-local mode even when cleanup unwinds.
    fn drop(&mut self) {
        ALLOW_INLINE_REAPER_FALLBACK.with(|enabled| enabled.set(self.0));
    }
}

/// Runs recovery cleanup with inline fallback enabled for the current thread.
///
/// The prior thread-local setting is restored during normal return or unwind.
///
/// # Type Parameters
/// * `T`: Value returned by the cleanup callback.
///
/// # Parameters
/// * `callback`: Performs one recovery cleanup attempt.
pub(crate) fn with_inline_reaper_fallback<T>(callback: impl FnOnce() -> T) -> T {
    ALLOW_INLINE_REAPER_FALLBACK.with(|enabled| {
        let previous = enabled.replace(true);
        let _reset = InlineFallbackReset(previous);
        callback()
    })
}

/// Reports whether inline fallback is enabled on the current thread.
pub(crate) fn inline_reaper_fallback_allowed() -> bool {
    ALLOW_INLINE_REAPER_FALLBACK.with(std::cell::Cell::get)
}
