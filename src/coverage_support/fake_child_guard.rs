/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
/// Guard restoring synthetic-child state when dropped.
pub(super) struct FakeChildGuard {
    /// Previously configured state for this thread.
    pub(super) previous: bool,
}

impl Drop for FakeChildGuard {
    /// Restores the previous synthetic-child state.
    fn drop(&mut self) {
        super::FAKE_CHILDREN_ENABLED.set(self.previous);
    }
}
