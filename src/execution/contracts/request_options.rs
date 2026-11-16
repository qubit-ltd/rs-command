// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use crate::CommandResourceRunOptions;
use crate::CommandRunOptions;

/// Either cloneable collection settings or uniquely owned resource settings.
#[derive(Debug)]
pub enum RequestOptions {
    /// Describes the standard case handled by managed command execution.
    Standard(#[doc = "Cloneable command options retained by the scheduled request."] CommandRunOptions),
    /// Describes the resources case handled by managed command execution.
    Resources(#[doc = "Uniquely owned input and output resources retained by the request."] CommandResourceRunOptions),
}
