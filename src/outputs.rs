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
    /// Where a mapped MCU button goes. A queue rather than a port, because the
    /// MCU port belongs to the caller and this sink only lives for one turn:
    /// whoever drives the engine takes the queue out afterwards and sends it.
    mcu: Option<&'a mut Vec<[u8; 3]>>,
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
        mcu: Option<&'a mut Vec<[u8; 3]>>,
    ) -> Outputs<'a> {
        Outputs {
            hid,
            leds,
            keyboard,
            mmc,
            mcu,
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

    fn mcu(&mut self, note: u8, press: bool) {
        // Only the wait: sending is the caller's, because the port outlives the
        // turn and this sink does not.
        if let Some(queue) = self.mcu.as_deref_mut() {
            queue.push(crate::mcu_buttons::mcu_button_message(note, press));
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
    /// What the mapping asked the DAW's surface to do, waiting to be sent on the
    /// MCU port - which the caller owns, so the waiting happens here.
    mcu: Vec<[u8; 3]>,
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
            mcu: Vec::new(),
            animate,
            lit: false,
        }
    }

    /// Takes whatever the mapping asked the DAW's surface to do. Sent by the
    /// caller, on the port the caller owns; empty when nothing was mapped that
    /// way, which is the usual case.
    pub fn take_mcu(&mut self) -> Vec<[u8; 3]> {
        std::mem::take(&mut self.mcu)
    }

    /// Says that the program acts on these buttons itself - the page buttons in
    /// the merged mode - so they are lit even though the mapping has nothing for
    /// them. Called before the first `light_up`.
    pub fn acts_on(&mut self, buttons: &[usize]) {
        for button in buttons {
            self.engine.acts_on(*button);
        }
    }

    /// Lights the LEDs the way the C tool does after start-up, and reports the
    /// reason when the device refused. Called once on start-up and once more
    /// every time the keyboard comes back, because its LEDs went with it.
    pub fn light_up(&mut self, hid: &Hid) -> Option<String> {
        let animate = self.animate && !self.lit;
        let mut sink = Outputs::for_turn(
            hid,
            &mut *self.leds,
            Some(&mut *self.keyboard),
            self.mmc,
            Some(&mut self.mcu),
        );
        self.engine.light_up_initial(&mut sink, animate);
        self.lit = true;
        sink.failed
    }

    /// One keyboard report, turned into whatever is mapped to it. The reason an
    /// LED report was refused is returned, and only once per report: the caller
    /// says it out loud the first time and keeps quiet afterwards.
    pub fn report(&mut self, packet: &[u8], hid: &Hid) -> Option<String> {
        let mut sink = Outputs::for_turn(
            hid,
            &mut *self.leds,
            Some(&mut *self.keyboard),
            self.mmc,
            Some(&mut self.mcu),
        );
        self.engine.process(packet, &mut sink);
        sink.failed
    }

    /// Lets go of everything held. Called when the keyboard goes away: no release
    /// will arrive from a keyboard that is gone, and a key left down in another
    /// program outlives the unplug.
    pub fn release_all(&mut self, hid: &Hid) {
        let mut sink = Outputs::for_turn(
            hid,
            &mut *self.leds,
            Some(&mut *self.keyboard),
            self.mmc,
            Some(&mut self.mcu),
        );
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
