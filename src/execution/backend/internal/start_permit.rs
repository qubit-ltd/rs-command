// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Represents the start permit state or contract used by managed command
/// execution.
#[derive(Debug)]
pub(crate) struct StartPermit(#[doc = "Marker proving start authorization won the race."] pub(crate) ());
