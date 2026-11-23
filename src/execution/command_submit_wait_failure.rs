// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0 (the "License");
//    you may not use this file except in compliance with the License.
//    You may obtain a copy of the License at
//
//        http://www.apache.org/licenses/LICENSE-2.0
//
//    Unless required by applicable law or agreed to in writing, software
//    distributed under the License is distributed on an "AS IS" BASIS,
//    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//    See the License for the specific language governing permissions and
//    limitations under the License.
// =============================================================================

use super::RetryTimerFailure;

/// Failure while waiting to retry a managed command submission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandSubmitWaitFailure {
    /// The executor has reached its configured limit for concurrent waiters.
    WaiterLimit,
    /// The retry timer could not be scheduled.
    Timer(#[doc = "The timer startup, capacity, or connectivity failure."] RetryTimerFailure),
}

impl From<RetryTimerFailure> for CommandSubmitWaitFailure {
    /// Wraps a retry timer scheduling failure.
    fn from(failure: RetryTimerFailure) -> Self {
        Self::Timer(failure)
    }
}
