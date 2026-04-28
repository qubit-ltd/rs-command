/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
use std::{
    io,
    thread,
};

/// Stdin writer thread result type.
pub(crate) type StdinWriter = Option<thread::JoinHandle<io::Result<()>>>;
