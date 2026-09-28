/// Keeps cancellation from leaving an unobserved agent or shell running.
pub(super) struct ProcessGroup(pub(super) u32);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-(self.0 as i32), libc::SIGKILL);
        }
    }
}
