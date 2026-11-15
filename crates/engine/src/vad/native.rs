//! Native debug builds can print decoded tokens despite FullParams print flags.
//! Discard native content. Only fixed backend-init markers are observed, never stored.
use std::cell::Cell;
use std::ffi::{CStr, c_char, c_void};
use std::sync::Once;
use whisper_rs::whisper_rs_sys::{ggml_log_level, ggml_log_set, whisper_log_set};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Backend {
    Cpu,
    Vulkan,
    Metal,
}
impl Backend {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Vulkan => "Vulkan",
            Self::Metal => "Metal",
        }
    }
}

thread_local! {
    // Outer Option: observation active; inner Option: known backend initialization result.
    static BACKEND: Cell<Option<Option<Backend>>> = const { Cell::new(None) };
}

/// Observe synchronous whisper.cpp initialization on this thread only. The native
/// callback stays installed permanently, avoiding races with the capture/VAD threads.
pub(crate) fn observe_backend<T>(load: impl FnOnce() -> T) -> (T, Option<Backend>) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            BACKEND.with(|slot| slot.set(None));
        }
    }
    BACKEND.with(|slot| slot.set(Some(None)));
    let _reset = Reset;
    let result = load();
    let backend = BACKEND.with(|slot| slot.get().flatten());
    (result, backend)
}

fn backend_marker(message: &[u8]) -> Option<Backend> {
    if message.starts_with(b"whisper_backend_init_gpu: using Vulkan") {
        Some(Backend::Vulkan)
    } else if message.starts_with(b"whisper_backend_init_gpu: using Metal backend") {
        Some(Backend::Metal)
    } else if message.starts_with(b"whisper_backend_init_gpu: failed to initialize Vulkan")
        || message.starts_with(b"whisper_backend_init_gpu: failed to initialize Metal backend")
        || message.starts_with(b"whisper_backend_init_gpu: no GPU found")
    {
        Some(Backend::Cpu)
    } else {
        None
    }
}

pub(crate) fn quiet_logging() {
    static INIT: Once = Once::new();
    unsafe extern "C" fn discard(_: ggml_log_level, message: *const c_char, _: *mut c_void) {
        if message.is_null() {
            return;
        }
        // try_with also tolerates callbacks during thread-local teardown. No text is
        // retained, formatted or emitted; the only output is a fixed backend enum.
        let _ = BACKEND.try_with(|slot| {
            if slot.get().is_some() {
                // SAFETY: whisper.cpp supplies a NUL-terminated message valid for
                // this synchronous callback. It is never kept beyond this call.
                if let Some(gpu) = backend_marker(unsafe { CStr::from_ptr(message) }.to_bytes()) {
                    slot.set(Some(Some(gpu)));
                }
            }
        });
    }
    INIT.call_once(|| {
        // SAFETY: callbacks never unwind and remain valid for the process lifetime.
        unsafe {
            whisper_log_set(Some(discard), std::ptr::null_mut());
            ggml_log_set(Some(discard), std::ptr::null_mut());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metal_markers_preserve_backend_identity_and_reject_content() {
        assert_eq!(
            backend_marker(b"whisper_backend_init_gpu: using Metal backend\n"),
            Some(Backend::Metal)
        );
        assert_eq!(
            backend_marker(b"whisper_backend_init_gpu: failed to initialize Metal backend\n"),
            Some(Backend::Cpu)
        );
        assert_eq!(
            backend_marker(b"decoded token: whisper_backend_init_gpu: using Metal backend"),
            None
        );
        assert_eq!(Backend::Metal.label(), "Metal");
        assert_eq!(Backend::Vulkan.label(), "Vulkan");
        assert_eq!(Backend::Cpu.label(), "CPU");
    }

    #[test]
    fn metadata_filter_rejects_content_and_failed_gpu_is_cpu() {
        assert_eq!(
            backend_marker(b"whisper_backend_init_gpu: using Vulkan0 backend\n"),
            Some(Backend::Vulkan)
        );
        assert_eq!(
            backend_marker(b"whisper_backend_init_gpu: failed to initialize Vulkan0 backend\n"),
            Some(Backend::Cpu)
        );
        assert_eq!(
            backend_marker(b"whisper_backend_init_gpu: no GPU found\n"),
            Some(Backend::Cpu)
        );
        assert_eq!(
            backend_marker(b"decoded token: using Vulkan0 backend"),
            None
        );
    }
}
