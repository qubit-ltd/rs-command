//! File ownership is kept in `ResourceRef`s until the last helper drops its handle.
//! Cleanup confirmation is published by the lifecycle adapter after those handles drain.
