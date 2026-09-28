use std::{
    ffi::{CStr, c_char, c_void},
    ptr::NonNull,
};

unsafe extern "C" {
    pub(super) fn aow_macos_log_create(category: *const c_char) -> *mut c_void;
    pub(super) fn aow_macos_log_release(log: *mut c_void);
    pub(super) fn aow_macos_log_write(log: *mut c_void, level: u8, message: *const c_char);
}

#[derive(Clone, Copy)]
#[repr(u8)]
pub(super) enum LogType {
    Default = 0,
    Debug = 1,
    Error = 2,
    Fault = 3,
}

pub(super) struct Handle(NonNull<c_void>);
// os_log objects support concurrent logging; the object is immutable here and
// released only when the final Arc is dropped, after all borrowed writers.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { aow_macos_log_release(self.0.as_ptr()) };
    }
}

pub(super) fn create_log(category: &CStr) -> Handle {
    // Apple's API always returns a valid object and copies category/subsystem.
    let handle = unsafe { aow_macos_log_create(category.as_ptr()) };
    Handle(NonNull::new(handle).expect("os_log_create returned null"))
}

pub(crate) fn write_log(handle: &Handle, level: LogType, message: &CStr) {
    unsafe { aow_macos_log_write(handle.0.as_ptr(), level as u8, message.as_ptr()) };
}
