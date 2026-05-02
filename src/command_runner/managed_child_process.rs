/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
use process_wrap::std::ChildWrapper;

/// Child process wrapper managed by this runner.
pub(crate) type ManagedChildProcess = Box<dyn ChildWrapper>;
