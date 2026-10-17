//! Where the engine's decisions are carried out.
//!
//! Two things live here, and they are the two halves of the same idea:
//!
//! * [`Outputs`] is the `Sink` the engine writes into - keystrokes to the virtual
//!   keyboard, MMC to the ALSA port, the LED picture to the HID device.
//! * [`Keys`] is that sink *plus* the engine and the state around it, ready to be
//!   driven by whatever reads the keyboard: the plain keystroke mode in
//!   `komplement`, and the merged mode in `live`, which is one program with the
//!   panel and the keystrokes on one HID handle.
//!
//! The device itself is deliberately not a field of [`Keys`]: it is borrowed for
//! one turn at a time, because the reading loop replaces the handle when the
//! keyboard is unplugged and plugged back in.

use crate::alsa_seq::MmcPort;
use crate::engine::{Engine, Sink};
use crate::hid::Hid;
use crate::leds::Leds;
use crate::uinput::Keyboard;

/// Where the engine's decisions are actually carried out.
pub struct Outputs<'a> {
    hid: &'a Hid,
    leds: &'a mut Leds,
    /// `None` until the virtual keyboard exists.
    keyboard: Option<&'a mut Keyboard>,
    mmc: Option<&'a MmcPort>,
    /// The text of the first LED report the device refused, if any. The C tool
    /// only knows *that* something failed; keeping the reason is what tells
    /// "opened but not writable" apart from "could not be opened at all".
    pub failed: Option<String>,
}

impl<'a> Outputs<'a> {
    /// The sink for one turn: everything it writes to, and the device it writes
    /// through.
    pub fn for_turn(
        hid: &'a Hid,
        leds: &'a mut Leds,
        keyboard: Option<&'a mut Keyboard>,
        mmc: Option<&'a MmcPort>,
    ) -> Outputs<'a> {
        Outputs {
            hid,
            leds,
            keyboard,
            mmc,
            failed: None,
        }
    }
}

impl Sink for Outputs<'_> {
    fn keystroke(&mut self, code: u16, press: bool) {
        let Some(keyboard) = self.keyboard.as_deref_mut() else {
            return;
        };
        let result = if press {
            keyboard.press(code)
        } else {
            keyboard.release(code)
        };
        if let Err(err) = result {
            eprintln!("Failed to send key {code}: {err}");
        }
    }

    fn mmc(&mut self, command: u8) {
        // MMC is a one-shot message, only sent when it is pressed.
        if let Some(port) = self.mmc {
            if let Err(err) = port.send_mmc(command, 0) {
                eprintln!("Failed to send MMC: {err}");
            }
        }
    }

    fn led(&mut self, index: i32, state: u8) {
        self.leds.update(index, state);
    }

    fn leds_sync(&mut self) {
        if let Err(err) = self.leds.sync(self.hid) {
            if self.failed.is_none() {
                self.failed = Some(err);
            }
        }
    }

    fn failed(&self) -> bool {
        self.failed.is_some()
    }
}

/// The keystroke half of the program: the engine, and everything it needs except
/// the device.
///
/// The caller's loop keeps ownership of the HID handle and lends it per turn,
/// which is what makes this work in a loop that may have to open the device
/// again. `animate` is the C tool's start-up flicker, done once on whichever
/// device is there.
pub struct Keys<'a> {
    engine: Engine,
    leds: &'a mut Leds,
    keyboard: &'a mut Keyboard,
    mmc: Option<&'a MmcPort>,
    animate: bool,
    /// Whether the start-up LED picture has been written to a device yet. After
    /// an unplug it has to be written again, on the device that is back.
    lit: bool,
}

impl<'a> Keys<'a> {
    pub fn new(
        engine: Engine,
        leds: &'a mut Leds,
        keyboard: &'a mut Keyboard,
        mmc: Option<&'a MmcPort>,
        animate: bool,
    ) -> Keys<'a> {
        Keys {
            engine,
            leds,
            keyboard,
            mmc,
            animate,
            lit: false,
        }
    }

    /// Lights the LEDs the way the C tool does after start-up, and reports the
    /// reason when the device refused. Called once on start-up and once more
    /// every time the keyboard comes back, because its LEDs went with it.
    pub fn light_up(&mut self, hid: &Hid) -> Option<String> {
        let animate = self.animate && !self.lit;
        let mut sink = Outputs::for_turn(hid, &mut *self.leds, Some(&mut *self.keyboard), self.mmc);
        self.engine.light_up_initial(&mut sink, animate);
        self.lit = true;
        sink.failed
    }

    /// One keyboard report, turned into whatever is mapped to it. The reason an
    /// LED report was refused is returned, and only once per report: the caller
    /// says it out loud the first time and keeps quiet afterwards.
    pub fn report(&mut self, packet: &[u8], hid: &Hid) -> Option<String> {
        let mut sink = Outputs::for_turn(hid, &mut *self.leds, Some(&mut *self.keyboard), self.mmc);
        self.engine.process(packet, &mut sink);
        sink.failed
    }

    /// Lets go of everything held. Called when the keyboard goes away: no release
    /// will arrive from a keyboard that is gone, and a key left down in another
    /// program outlives the unplug.
    pub fn release_all(&mut self, hid: &Hid) {
        let mut sink = Outputs::for_turn(hid, &mut *self.leds, Some(&mut *self.keyboard), self.mmc);
        self.engine.release_all(&mut sink);
    }

    /// The LEDs out on the way out, the way the plain mode does it.
    pub fn farewell(&mut self, hid: &Hid) {
        if self.animate {
            self.leds.animate_off(hid);
        } else {
            self.leds.off(hid);
        }
    }
}
