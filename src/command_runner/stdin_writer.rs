/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
use std::{
    io,
    thread,
};

/// Stdin writer thread result type.
pub(crate) type StdinWriter = Option<thread::JoinHandle<io::Result<()>>>;
