/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
use process_wrap::std::ChildWrapper;

/// Child process wrapper managed by this runner.
pub(crate) type ManagedChildProcess = Box<dyn ChildWrapper>;
