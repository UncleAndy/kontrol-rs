//! The virtual keyboard that the keystrokes are injected into.
//!
//! Mirrors `uinput_open()` / `key_press()` / `key_release()` of `uinput_stuff.c`:
//! a `BUS_USB` device named `Kontroller` that advertises every key the mapping
//! files can name, so a mapping never fails because a key bit was not requested.
//! Each press/release is followed by a `SYN_REPORT`, exactly like the C tool
//! (the `evdev` crate appends the `SYN_REPORT` itself when emitting).

use std::io;

use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, KeyCode};

use crate::keys::KEY_TABLE;

pub const DEVICE_NAME: &str = "Kontroller";
pub const VENDOR_ID: u16 = 0x1234;
pub const PRODUCT_ID: u16 = 0x5678;

pub struct Keyboard {
    device: VirtualDevice,
}

impl Keyboard {
    pub fn open() -> io::Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for (_, code) in KEY_TABLE {
            keys.insert(KeyCode::new(*code));
        }

        let device = VirtualDevice::builder()?
            .name(DEVICE_NAME)
            .with_keys(&keys)?
            .build()?;

        Ok(Keyboard { device })
    }

    pub fn press(&mut self, code: u16) -> io::Result<()> {
        self.emit(code, 1)
    }

    pub fn release(&mut self, code: u16) -> io::Result<()> {
        self.emit(code, 0)
    }

    fn emit(&mut self, code: u16, value: i32) -> io::Result<()> {
        self.device
            .emit(&[InputEvent::new(EventType::KEY.0, code, value)])
    }
}
