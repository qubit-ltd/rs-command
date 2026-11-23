// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Preparation and ownership for files transferred to managed commands.
mod cleanup;
mod identity;
pub(crate) mod input;
mod managed_preparation_failure;
pub(crate) mod prepared_files;
pub(crate) use identity::resolve_io_path;
