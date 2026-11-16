//! Session bridge contract between blocking process pipes and caller endpoints.
/// Blocking byte bridge used by process workers.
pub trait SessionBridge: Send + Sync {
    fn read_stdin(&self, buffer: &mut [u8]) -> std::io::Result<usize>;
    fn write_stdout(&self, bytes: &[u8]) -> std::io::Result<()>;
    fn write_stderr(&self, bytes: &[u8]) -> std::io::Result<()>;
    fn close_stdin(&self);
}
