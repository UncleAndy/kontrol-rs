//! Thin wrapper around hidapi, mirroring `hid.c` of the C tool.
//!
//! The device is opened through libusb (the same backend the C tool links), the
//! 22-byte LED report is written with its report ID as the first byte, and reads
//! use a timeout so the main loop stays interruptible.

use hidapi::{HidApi, HidDevice};

pub struct Hid {
    /// Kept alive: `HidDevice` borrows from the context internally.
    _api: HidApi,
    device: HidDevice,
}

impl Hid {
    /// Opens the device with the given USB ids. `None` when it is not there or
    /// the user has no permission for it.
    pub fn open(vid: u16, pid: u16) -> Option<Self> {
        let api = HidApi::new().ok()?;
        let device = api.open(vid, pid).ok()?;
        // The main loop wants blocking reads with a timeout, not polled reads.
        let _ = device.set_blocking_mode(true);
        Some(Hid {
            _api: api,
            device,
        })
    }

    /// Returns the number of bytes written, or -1.
    pub fn write_raw(&self, buffer: &[u8]) -> isize {
        match self.device.write(buffer) {
            Ok(written) => written as isize,
            Err(_) => -1,
        }
    }

    /// Returns the number of bytes read (0 on timeout), or -1 on error.
    pub fn read_timeout(&self, buffer: &mut [u8], millis: i32) -> isize {
        match self.device.read_timeout(buffer, millis) {
            Ok(read) => read as isize,
            Err(_) => -1,
        }
    }
}
