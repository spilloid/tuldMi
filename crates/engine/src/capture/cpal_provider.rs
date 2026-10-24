//! cpal-backed provider (WASAPI on Windows). System audio = input stream on a render device
//! (WASAPI loopback). Each stream lives on its own thread that owns the `cpal::Stream` (which is
//! not `Send` on every platform) and parks until stopped. Device ids are stable across runs
//! where cpal provides them; otherwise the device name.

use super::*;

pub struct CpalProvider {
    // Implementation-defined.
}

impl CpalProvider {
    pub fn new() -> Self {
        todo!()
    }
}

impl Default for CpalProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioCaptureProvider for CpalProvider {
    fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        todo!()
    }
    fn open(&self, source: Source, device_id: Option<&str>, tx: Sender<CaptureMsg>) -> Result<Box<dyn CaptureStream>> {
        let _ = (source, device_id, tx);
        todo!()
    }
    fn default_device_id(&self, source: Source) -> Option<String> {
        let _ = source;
        todo!()
    }
}
