//! The LED report of the A-series protocol and its animations.
//!
//! The report is exactly `1 + TOTAL_HID_BUTTONS` bytes: a report ID followed by
//! one state byte per button. That length is a protocol constant - the buttons
//! above index 20 have no LED slot at all, so this is never widened (mirrors
//! `button_leds.c`).

use std::thread::sleep;
use std::time::Duration;

use crate::hid::Hid;

/// How many LEDs the report has room for (bits 0..20 of the button mask).
pub const TOTAL_HID_BUTTONS: usize = 21;

pub const LED_BRIGHT: u8 = 0x7e;
pub const LED_ON: u8 = 0x7c;
pub const LED_OFF: u8 = 0x00;

/// Delay between the animation steps (the C tool uses 30000 µs).
const ANIMATE_DELAY: Duration = Duration::from_millis(30);
/// Delay per LED when the LEDs are switched off one by one.
const ANIMATE_OFF_DELAY: Duration = Duration::from_millis(5);

/// The wipe sequence: columns of LED indexes, -1 marks a pause.
const ANIMATE_COLUMNS: [[i8; 5]; 11] = [
    [0, 3, 6, 9, 19],
    [1, 4, 7, 10, 20],
    [2, 5, 8, 11, -1],
    [-1, -1, -1, -1, -1],
    [12, 13, 14, -1, -1],
    [12, 13, 15, -1, -1],
    [-1, -1, -1, -1, -1],
    [-1, -1, -1, -1, -1],
    [16, -1, -1, -1, -1],
    [17, -1, -1, -1, -1],
    [18, -1, -1, -1, -1],
];

pub struct Leds {
    buffer: [u8; 1 + TOTAL_HID_BUTTONS],
}

impl Default for Leds {
    fn default() -> Self {
        Self::new()
    }
}

impl Leds {
    pub fn new() -> Self {
        Leds {
            buffer: [0; 1 + TOTAL_HID_BUTTONS],
        }
    }

    /// Sets the report ID and clears every LED.
    pub fn init(&mut self) {
        self.buffer[0] = 0x80;
        self.clear();
    }

    pub fn clear(&mut self) {
        self.buffer[1..].fill(LED_OFF);
    }

    /// Queues one LED state; `sync()` actually sends it.
    pub fn update(&mut self, index: i32, state: u8) {
        if index >= 0 && (index as usize) < TOTAL_HID_BUTTONS {
            self.buffer[index as usize + 1] = state;
        }
    }

    /// Sends the LED report. Returns false when the device did not accept it
    /// (the C tool uses this to detect that the device cannot be opened).
    pub fn sync(&self, hid: &Hid) -> bool {
        hid.write_raw(&self.buffer) >= 0
    }

    /// The wipe from right to left, then everything off - what the tool plays
    /// on start-up unless `-n` is given.
    pub fn animate_on(&mut self, hid: &Hid) {
        for i in (0..ANIMATE_COLUMNS.len()).rev() {
            for j in 0..5 {
                let index = ANIMATE_COLUMNS[i][j];
                if index >= 0 {
                    self.update(i32::from(index), LED_ON);
                }
                if i < ANIMATE_COLUMNS.len() - 1 {
                    let previous = ANIMATE_COLUMNS[i + 1][j];
                    if previous >= 0 {
                        self.update(i32::from(previous), LED_OFF);
                    }
                }
            }
            self.sync(hid);
            sleep(ANIMATE_DELAY);
        }

        self.off(hid);
    }

    /// Switches every LED off, one at a time (used on shutdown).
    pub fn animate_off(&mut self, hid: &Hid) {
        for i in 0..TOTAL_HID_BUTTONS {
            self.update(i as i32, LED_OFF);
            self.sync(hid);
            sleep(ANIMATE_OFF_DELAY);
        }
    }

    pub fn off(&mut self, hid: &Hid) {
        self.clear();
        self.sync(hid);
    }
}
