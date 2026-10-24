//! Native debug builds can print decoded tokens despite FullParams print flags.
//! Discard all native messages; engine errors are reported separately, without text.
use std::ffi::{c_char, c_void};
use std::sync::Once;
use whisper_rs::whisper_rs_sys::{ggml_log_level, ggml_log_set, whisper_log_set};

pub(crate) fn quiet_logging() {
    static INIT: Once = Once::new();
    unsafe extern "C" fn discard(_: ggml_log_level, _: *const c_char, _: *mut c_void) {}
    INIT.call_once(|| {
        // SAFETY: static callbacks never unwind, access no pointers or shared
        // state, and remain valid for the process lifetime. Install before use.
        unsafe {
            whisper_log_set(Some(discard), std::ptr::null_mut());
            ggml_log_set(Some(discard), std::ptr::null_mut());
        }
    });
}
