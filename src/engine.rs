//! The decision logic of `komplement`: what a HID report means.
//!
//! The C tool does this inline in its main loop, which makes it untestable
//! without the keyboard. Here it lives in one place and talks to the outside
//! world through [`Sink`], so the tests can feed it synthetic reports and check
//! exactly which keys, MMC messages and LED updates come out.

use crate::buttons::{DIAL_CCW_INDEX, DIAL_CW_INDEX, TOGGLE_BUTTON_TOTAL};
use crate::leds::{LED_BRIGHT, LED_OFF, LED_ON};
use crate::mapping::{Layer, MappingKey, MappingTable};

/// Every bit the payload carries; the LED report is a fixed 22 bytes and must
/// not grow, so this stays a separate constant.
pub const MAX_BUTTONS: usize = 40;

/// Where the engine's decisions go.
pub trait Sink {
    /// A keystroke: uinput press (`press = true`) or release.
    fn keystroke(&mut self, code: u16, press: bool);
    /// An MMC message; the original only sends it on press.
    fn mmc(&mut self, command: u8);
    /// One LED state (indexes 0..20).
    fn led(&mut self, index: i32, state: u8);
    /// The 21 LED states are complete and should be pushed to the device.
    fn leds_sync(&mut self);
    /// True when the device did not accept an LED report; the engine stops
    /// lighting up when that happens (the C tool returns -1 there).
    fn failed(&self) -> bool {
        false
    }
}

/// Extracts the button bits: bits 0..31 from the bytes after the report ID,
/// bits 32..39 from the fifth byte. Bytes past the report's length are zero.
pub fn button_mask(packet: &[u8]) -> u64 {
    let mut mask = 0u64;
    for i in 1..=5usize {
        if i < packet.len() {
            mask |= u64::from(packet[i]) << ((i - 1) * 8);
        }
    }
    mask
}

/// The byte the 4D dial's position lives in.
pub const DIAL_POSITION_BYTE: usize = 28;

pub struct Engine {
    table: MappingTable,
    held_layer: [Option<Layer>; MAX_BUTTONS],
    dial_position: Option<u8>,
    shift_was_pressed: bool,
    /// Buttons the program acts on itself, with nothing in the mapping for them.
    /// In the merged mode those are the two page buttons: they turn the panel's
    /// page, and with SHIFT they change the glyph size. They are lit like mapped
    /// buttons, because on this keyboard an unlit button means "nothing here" -
    /// and these do something, even though no key comes out of them.
    own_buttons: [bool; MAX_BUTTONS],
}

impl Engine {
    pub fn new(table: MappingTable) -> Self {
        Engine {
            table,
            held_layer: [None; MAX_BUTTONS],
            dial_position: None,
            shift_was_pressed: false,
            own_buttons: [false; MAX_BUTTONS],
        }
    }

    /// Says that the program acts on this button itself, so its LED is lit even
    /// though the mapping has nothing for it. Both layers: a button the program
    /// handles is handled with SHIFT down as well.
    pub fn acts_on(&mut self, button: usize) {
        if button < MAX_BUTTONS {
            self.own_buttons[button] = true;
        }
    }

    /// Whether a button is lit in the picture for a layer: because something is
    /// mapped to it there, or because the program acts on it itself.
    fn lit(&self, button: usize, layer: Layer) -> bool {
        self.own_buttons[button] || self.table.is_mapped(button as i32, layer)
    }

    pub fn table(&self) -> &MappingTable {
        &self.table
    }

    /// Lets go of everything that is being held and forgets it, each button in
    /// the layer it was pressed in.
    ///
    /// Two callers: SHIFT released before the other buttons - their release would
    /// otherwise be looked up in the wrong layer - and a keyboard that goes away
    /// mid-press, where a key left held down in another program outlives the
    /// unplug. The layer is remembered rather than assumed for exactly that
    /// second case: what is held when the keyboard is pulled out can have been
    /// pressed under either of them.
    pub fn release_all(&mut self, sink: &mut impl Sink) {
        for index in 1..MAX_BUTTONS {
            if let Some(layer) = self.held_layer[index] {
                let mapping = self.table.get(index as i32, layer);
                emit(&mapping, sink, false);
            }
        }
        self.held_layer = [None; MAX_BUTTONS];
    }

    /// Lights the LEDs the way the C tool does after start-up: SHIFT and the
    /// octave buttons always, plus everything mapped without SHIFT. LEDs come
    /// up one by one with a small delay (that is what `animate` does) and the
    /// loop stops early when the device does not answer.
    pub fn light_up_initial(&self, sink: &mut impl Sink, animate: bool) {
        for i in 0..TOGGLE_BUTTON_TOTAL {
            let state = if i == 0 || i == 19 || i == 20 || self.lit(i, Layer::Normal) {
                LED_ON
            } else {
                LED_OFF
            };
            sink.led(i as i32, state);
            sink.leds_sync();

            if sink.failed() {
                return;
            }

            if animate {
                std::thread::sleep(std::time::Duration::from_micros(5000));
            }
        }
    }

    /// Handles one HID report: the state machine of the C tool's main loop.
    pub fn process(&mut self, packet: &[u8], sink: &mut impl Sink) {
        let key_value = button_mask(packet);
        let shift_is_pressed = key_value & 1 != 0;

        if shift_is_pressed != self.shift_was_pressed {
            if shift_is_pressed {
                self.light_up_shifted(sink);
            } else {
                self.light_up_normal(sink);
            }
            self.shift_was_pressed = shift_is_pressed;

            // When SHIFT is released before the other buttons, their release
            // would be looked up in the wrong layer, so everything that is held
            // is released here and forgotten.
            if !shift_is_pressed {
                self.release_all(sink);
            }
        }

        // The 4D dial: one step per change of its position byte, wrapping
        // around 0..15. It has no button of its own but does have a SHIFT layer.
        if packet.len() > DIAL_POSITION_BYTE {
            let new_position = packet[DIAL_POSITION_BYTE];
            let change = match self.dial_position {
                None => {
                    self.dial_position = Some(new_position);
                    0
                }
                Some(previous) => {
                    if new_position == 0x0f && previous == 0x00 {
                        -1
                    } else if new_position == 0x00 && previous == 0x0f {
                        1
                    } else {
                        i32::from(new_position) - i32::from(previous)
                    }
                }
            };

            if change != 0 {
                let index = if change > 0 {
                    DIAL_CW_INDEX
                } else {
                    DIAL_CCW_INDEX
                };
                let layer = if shift_is_pressed {
                    Layer::Shifted
                } else {
                    Layer::Normal
                };
                let mapping = self.table.get(index as i32, layer);

                // A single keypress/release per step.
                emit(&mapping, sink, true);
                emit(&mapping, sink, false);

                self.dial_position = Some(new_position);
            }
        }

        // Any button whose state changed. SHIFT is bit 0 and handled above.
        let layer = if shift_is_pressed {
            Layer::Shifted
        } else {
            Layer::Normal
        };
        for button_number in 1..MAX_BUTTONS {
            let pressed = key_value & (1u64 << button_number) != 0;
            if pressed == self.held_layer[button_number].is_some() {
                continue;
            }

            let mapping = self.table.get(button_number as i32, layer);
            emit(&mapping, sink, pressed);
            self.held_layer[button_number] = if pressed { Some(layer) } else { None };
        }
    }

    fn light_up_shifted(&self, sink: &mut impl Sink) {
        for i in 0..TOGGLE_BUTTON_TOTAL {
            let state = if i == 0 {
                LED_BRIGHT
            } else if self.lit(i, Layer::Shifted) {
                LED_ON
            } else {
                LED_OFF
            };
            sink.led(i as i32, state);
        }
        sink.leds_sync();
    }

    fn light_up_normal(&self, sink: &mut impl Sink) {
        for i in 0..TOGGLE_BUTTON_TOTAL {
            let state = if i == 0 || i == 19 || i == 20 || self.lit(i, Layer::Normal) {
                LED_ON
            } else {
                LED_OFF
            };
            sink.led(i as i32, state);
        }
        sink.leds_sync();
    }
}

/// Sends every key of a mapping, in the order the mapping file lists them.
fn emit(mapping: &MappingKey, sink: &mut impl Sink, press: bool) {
    for key in mapping.iter() {
        match key.kind {
            crate::mapping::KeyKind::Key => sink.keystroke(key.key as u16, press),
            crate::mapping::KeyKind::Mmc => {
                if press {
                    sink.mmc(key.key as u8);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapping::{MappedKey, MappingKey};

    #[derive(Debug, PartialEq, Eq)]
    enum Call {
        Key(u16, bool),
        Mmc(u8),
        Led(i32, u8),
        Sync,
    }

    #[derive(Default)]
    struct Recorder {
        calls: Vec<Call>,
    }

    impl Sink for Recorder {
        fn keystroke(&mut self, code: u16, press: bool) {
            self.calls.push(Call::Key(code, press));
        }
        fn mmc(&mut self, command: u8) {
            self.calls.push(Call::Mmc(command));
        }
        fn led(&mut self, index: i32, state: u8) {
            self.calls.push(Call::Led(index, state));
        }
        fn leds_sync(&mut self) {
            self.calls.push(Call::Sync);
        }
    }

    /// A 30 byte report with the given button bytes.
    fn report(bytes: &[u8]) -> Vec<u8> {
        let mut packet = vec![0u8; 30];
        for (index, byte) in bytes.iter().enumerate() {
            packet[1 + index] = *byte;
        }
        packet
    }

    fn table_with(rows: &[(usize, Layer, &[MappedKey])]) -> MappingTable {
        let mut table = MappingTable::new();
        for (index, layer, keys) in rows {
            let mut mapping = MappingKey::new();
            for key in keys.iter() {
                mapping.push(*key);
            }
            table.set(*index as i32, *layer, mapping);
        }
        table
    }

    /// Pressing and releasing a mapped button produces press then release.
    #[test]
    fn a_button_press_and_release_sends_the_mapped_keys() {
        let table = table_with(&[(9, Layer::Normal, &[MappedKey::key(57)])]); // Play -> Space
        let mut engine = Engine::new(table);
        let mut sink = Recorder::default();

        engine.process(&report(&[0x00, 0x02, 0x00, 0x00, 0x00]), &mut sink);
        engine.process(&report(&[0x00, 0x00, 0x00, 0x00, 0x00]), &mut sink);

        assert_eq!(
            sink.calls,
            vec![Call::Key(57, true), Call::Key(57, false)]
        );
    }

    /// A button the program acts on itself is lit without a mapping: on this
    /// keyboard an unlit button reads as "nothing here", and the page buttons do
    /// something in the merged mode even though the mapping has nothing for them.
    #[test]
    fn a_button_the_program_acts_on_is_lit_without_a_mapping() {
        let table = table_with(&[(9, Layer::Normal, &[MappedKey::key(57)])]); // Play -> Space
        let mut engine = Engine::new(table);
        engine.acts_on(14);
        let mut sink = Recorder::default();

        engine.light_up_initial(&mut sink, false);

        assert!(
            sink.calls.contains(&Call::Led(14, LED_ON)),
            "a button the program acts on has to be lit: {:?}",
            sink.calls
        );
        assert!(
            sink.calls.contains(&Call::Led(15, LED_OFF)),
            "and one it does not stays dark: {:?}",
            sink.calls
        );

        // With SHIFT down too: a button the program handles is handled then as
        // well (it changes the glyph size).
        sink.calls.clear();
        engine.process(&report(&[0x01]), &mut sink);
        assert!(
            sink.calls.contains(&Call::Led(14, LED_ON)),
            "the shifted picture lights it too: {:?}",
            sink.calls
        );
    }

    /// Everything held is let go of when the keyboard goes away: a key left
    /// pressed in another program would outlive the unplug, so the release has to
    /// come from the tool itself.
    #[test]
    fn everything_held_is_let_go_of_when_the_keyboard_goes_away() {
        let table = table_with(&[(9, Layer::Normal, &[MappedKey::key(57)])]); // Play -> Space
        let mut engine = Engine::new(table);
        let mut sink = Recorder::default();

        engine.process(&report(&[0x00, 0x02, 0x00, 0x00, 0x00]), &mut sink);
        assert_eq!(sink.calls, vec![Call::Key(57, true)]);

        // No further report ever arrives: the keyboard is unplugged with the
        // button still down.
        sink.calls.clear();
        engine.release_all(&mut sink);
        assert_eq!(sink.calls, vec![Call::Key(57, false)]);

        // And it is forgotten: the next report must not release it twice.
        sink.calls.clear();
        engine.release_all(&mut sink);
        assert!(sink.calls.is_empty(), "{:?}", sink.calls);
    }

    /// An unchanged report sends nothing; a repeated one does not repeat.
    #[test]
    fn repeated_reports_are_ignored() {
        let table = table_with(&[(9, Layer::Normal, &[MappedKey::key(57)])]);
        let mut engine = Engine::new(table);
        let mut sink = Recorder::default();

        let pressed = report(&[0x00, 0x02, 0x00, 0x00, 0x00]);
        engine.process(&pressed, &mut sink);
        engine.process(&pressed, &mut sink);

        assert_eq!(sink.calls, vec![Call::Key(57, true)]);
    }

    /// SHIFT selects the second layer.
    #[test]
    fn shift_selects_the_shifted_layer() {
        let table = table_with(&[
            (9, Layer::Normal, &[MappedKey::key(57)]),  // Space
            (9, Layer::Shifted, &[MappedKey::key(28)]), // Enter
        ]);
        let mut engine = Engine::new(table);
        let mut sink = Recorder::default();

        engine.process(&report(&[0x01, 0x02, 0x00, 0x00, 0x00]), &mut sink);
        assert!(sink.calls.contains(&Call::Key(28, true)));

        sink.calls.clear();
        // Releasing SHIFT first releases the held button in the shifted layer,
        // and the still-held button is then a fresh press in the normal layer.
        // The C tool does the same: its release lookup would otherwise use the
        // layer that SHIFT just left.
        engine.process(&report(&[0x00, 0x02, 0x00, 0x00, 0x00]), &mut sink);
        assert!(sink.calls.contains(&Call::Key(28, false)));
        assert!(sink.calls.contains(&Call::Key(57, true)));

        // ... and the following release of the button itself is then normal.
        sink.calls.clear();
        engine.process(&report(&[0x00, 0x00, 0x00, 0x00, 0x00]), &mut sink);
        assert!(sink.calls.contains(&Call::Key(57, false)));
        assert!(!sink.calls.contains(&Call::Key(28, false)));
    }

    /// The 4D dial: one step per change, with the wrap around 0/15.
    #[test]
    fn the_dial_steps_and_wraps() {
        let table = table_with(&[
            (DIAL_CW_INDEX, Layer::Normal, &[MappedKey::key(106)]), // Right
            (DIAL_CCW_INDEX, Layer::Normal, &[MappedKey::key(105)]), // Left
        ]);
        let mut engine = Engine::new(table);
        let mut sink = Recorder::default();

        let mut packet = report(&[0x00, 0x00, 0x00, 0x00, 0x00]);
        packet[DIAL_POSITION_BYTE] = 0x0f;
        engine.process(&packet, &mut sink);
        assert!(sink.calls.is_empty(), "the first report only sets the position");

        // Forwards: 15 -> 0 is one click clockwise.
        packet[DIAL_POSITION_BYTE] = 0x00;
        engine.process(&packet, &mut sink);
        assert_eq!(sink.calls, vec![Call::Key(106, true), Call::Key(106, false)]);

        // Backwards: 0 -> 15 is one click counter clockwise.
        sink.calls.clear();
        packet[DIAL_POSITION_BYTE] = 0x0f;
        engine.process(&packet, &mut sink);
        assert_eq!(sink.calls, vec![Call::Key(105, true), Call::Key(105, false)]);

        // A jump forwards past the wrap (15 -> 1) computes a negative step, so
        // it counts as counter clockwise - the plain `new - previous` the C
        // tool uses, not a shortest-path comparison.
        sink.calls.clear();
        packet[DIAL_POSITION_BYTE] = 0x01;
        engine.process(&packet, &mut sink);
        assert_eq!(sink.calls, vec![Call::Key(105, true), Call::Key(105, false)]);
    }

    /// A row with an MMC command sends it once, on press.
    #[test]
    fn mmc_is_sent_once_per_press() {
        let table = table_with(&[(9, Layer::Normal, &[MappedKey::mmc(0x02)])]);
        let mut engine = Engine::new(table);
        let mut sink = Recorder::default();

        engine.process(&report(&[0x00, 0x02, 0x00, 0x00, 0x00]), &mut sink);
        engine.process(&report(&[0x00, 0x00, 0x00, 0x00, 0x00]), &mut sink);

        assert_eq!(sink.calls, vec![Call::Mmc(0x02)]);
    }

    /// SHIFT lights up the shifted entries, releasing it restores the normal ones.
    #[test]
    fn shift_repaints_the_leds() {
        let table = table_with(&[(9, Layer::Shifted, &[MappedKey::key(28)])]);
        let mut engine = Engine::new(table);
        let mut sink = Recorder::default();

        engine.process(&report(&[0x01, 0x00, 0x00, 0x00, 0x00]), &mut sink);
        // SHIFT itself is bright, the mapped button is on.
        assert!(sink.calls.contains(&Call::Led(0, LED_BRIGHT)));
        assert!(sink.calls.contains(&Call::Led(9, LED_ON)));
        assert!(sink.calls.contains(&Call::Led(8, LED_OFF)));
    }
}
