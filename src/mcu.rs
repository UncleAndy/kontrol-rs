//! The DAW half of `doc/display-interface.md`: what Reaper's *Mackie Control
//! Universal* control surface sends, decoded into the `Studio` the pages read.
//!
//! Every number below was measured from a capture of the surface's own output
//! (`doc/Captured_Codes.md`, the log in `kk-mcu-20261017-032907`); the two
//! places where a number is still a guess say so. Nothing here ever sends a
//! byte back, so listening cannot disturb the DAW.

use crate::studio::{Digits, BANK_CHANNELS, DISPLAY_DIGITS, Studio, Transport};
use crate::timecode::TimeLayout;

/// The channel strip LEDs, one block of eight notes per row. Measured:
/// selecting the first three tracks lit notes 24, 25 and 26 in turn, and
/// muting the second track lit note 17 - so the four rows are arm, solo, mute
/// and select, in that order, exactly as MCU numbers them.
const ARM_NOTES: u8 = 0;
const SOLO_NOTES: u8 = 8;
const MUTE_NOTES: u8 = 16;
const SELECT_NOTES: u8 = 24;

/// Transport LEDs. 93 lit right after the transport stopped and 94 while it
/// ran, so those two are measured. 95 is the record LED next to them and is
/// the one guess here: nothing in the capture engaged record.
const STOP_LED: u8 = 93;
const PLAY_LED: u8 = 94;
const RECORD_LED: u8 = 95;

/// The loop and click LEDs: both lit in the step where the metronome and the
/// loop were toggled. 86 (0x56) is the loop LED in MCU's own LED map - the map
/// that predicted 93 and 94 correctly - which leaves 89 as the click.
const LOOP_LED: u8 = 86;
const CLICK_LED: u8 = 89;

/// What every MCU SysEx starts with, and the command byte of an LCD write.
const SYSEX_HEADER: [u8; 5] = [0xf0, 0x00, 0x00, 0x66, 0x14];
const LCD_WRITE: u8 = 0x12;

/// Characters in one LCD name field, which is also the step between offsets: a
/// field starts at 0, 7, 14 ... 49 and holds seven characters.
pub const NAME_FIELD: usize = 7;

/// One message, as much of it as the screen has a use for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    /// A channel LED, or one of the transport ones: lit means note on with a
    /// velocity, dark means note off or note on with velocity zero - both
    /// shapes were seen in the capture.
    Led { note: u8, lit: bool },
    /// Fader positions arrive here, not as controllers: one message per
    /// channel, 14 bits, the ninth being the master.
    Fader { channel: u8, value: i16 },
    /// A meter level, one per channel, and by far the most traffic.
    Level { channel: u8, value: u8 },
    /// Seven characters of a track name at `offset`.
    Name { offset: u8, text: String },
    /// An LCD write that is not a name field: Reaper writes its own status
    /// line, a whole 55 characters wide, through the same command.
    Status { text: String },
    /// One digit of the DAW's own display, over controllers 64..75. The digit
    /// number counts from the right: zero is controller 64, the rightmost
    /// character. The value is the character with bit 6 as the dot, and zero
    /// means that digit is dark.
    Display { digit: u8, value: u8 },
    /// Something the screen has no use for yet: the V-Pot ring modes on
    /// 0x30..0x37, the 0x20 command, and the notes that only blink.
    Other,
}

/// MIDI arrives as a stream of bytes, so the framing has to be kept somewhere.
pub struct Parser {
    running: u8,
    data: [u8; 2],
    have: usize,
    sysex: Vec<u8>,
    in_sysex: bool,
}

impl Parser {
    pub fn new() -> Parser {
        Parser {
            running: 0,
            data: [0; 2],
            have: 0,
            sysex: Vec::new(),
            in_sysex: false,
        }
    }

    /// Feeds one byte and returns the message that byte completed, if any.
    pub fn push(&mut self, byte: u8) -> Option<Message> {
        if self.in_sysex {
            match byte {
                0xf7 => {
                    self.in_sysex = false;
                    let sysex = std::mem::take(&mut self.sysex);
                    return Some(decode_sysex(&sysex));
                }
                // A realtime byte can sit anywhere, even inside a SysEx.
                b if b >= 0xf8 => {}
                b => self.sysex.push(b),
            }
            return None;
        }

        match byte {
            0xf0 => {
                self.in_sysex = true;
                self.sysex.clear();
                self.sysex.push(0xf0);
            }
            0xf8..=0xff => {}
            status if status >= 0x80 => {
                self.running = status;
                self.have = 0;
            }
            data => {
                // A data byte with no status before it is nothing we can use.
                if self.running == 0 {
                    return None;
                }
                self.data[self.have] = data;
                self.have += 1;
                if self.have == expected_data_bytes(self.running) {
                    self.have = 0; // running status: the status byte stays
                    return Some(decode(self.running, self.data));
                }
            }
        }
        None
    }
}

impl Default for Parser {
    fn default() -> Self {
        Parser::new()
    }
}

/// How many data bytes follow a status byte. All the messages a control
/// surface uses take two, bar the one-byte program change and aftertouch.
fn expected_data_bytes(status: u8) -> usize {
    match status & 0xf0 {
        0xc0 | 0xd0 => 1,
        _ => 2,
    }
}

fn decode(status: u8, data: [u8; 2]) -> Message {
    let channel = status & 0x0f;
    match status & 0xf0 {
        0x80 => Message::Led {
            note: data[0],
            lit: false,
        },
        0x90 => Message::Led {
            note: data[0],
            lit: data[1] != 0,
        },
        0xd0 => Message::Level {
            channel,
            value: data[0],
        },
        0xe0 => Message::Fader {
            channel,
            value: ((data[1] as i16) << 7 | data[0] as i16) - 8192,
        },
        // The twelve-digit display comes over the controllers numbered from 64,
        // which is the rightmost digit. Nothing else lives in the controllers
        // this decoder reads, so the rest is still unread.
        0xb0 if (64..64 + DISPLAY_DIGITS as u8).contains(&data[0]) => Message::Display {
            digit: data[0] - 64,
            value: data[1],
        },
        _ => Message::Other,
    }
}

fn decode_sysex(bytes: &[u8]) -> Message {
    if bytes.len() < SYSEX_HEADER.len() + 2 || bytes[..SYSEX_HEADER.len()] != SYSEX_HEADER {
        return Message::Other;
    }
    if bytes[SYSEX_HEADER.len()] != LCD_WRITE {
        return Message::Other;
    }

    let offset = bytes[SYSEX_HEADER.len() + 1];
    let text: String = bytes[SYSEX_HEADER.len() + 2..]
        .iter()
        .take_while(|byte| **byte < 0x80)
        .map(|byte| *byte as char)
        .collect();

    // A name field is exactly seven characters at a multiple of seven; a
    // status line is longer and starts at zero, so the length tells them apart.
    if text.chars().count() == NAME_FIELD && offset % NAME_FIELD as u8 == 0 {
        Message::Name {
            offset,
            text: text.trim_end().to_string(),
        }
    } else {
        Message::Status {
            text: text.trim_end().to_string(),
        }
    }
}

/// Where a 14-bit fader value sits, 0.0 at the bottom and 1.0 at the top.
fn fader_fraction(value: i16) -> f32 {
    (value as f32 + 8192.0) / 16384.0
}

/// Bytes in, `Studio` out.
pub struct Decoder {
    parser: Parser,
    studio: Studio,
    /// The record-arm LEDs of the bank. `Channel` carries only the two flags
    /// the pages read today, so arm state is kept here and mirrored onto the
    /// selected track, where a page shows it.
    arms: [bool; BANK_CHANNELS],
    /// Meter levels, as they last arrived. Nothing reads them yet; the fourth
    /// page draws fader positions, and this is where a level display would
    /// take them from.
    levels: [u8; BANK_CHANNELS],
    /// Which strip the DAW has selected, once it has said.
    selected: Option<usize>,
    /// The DAW's own display row, kept as digits so one update cannot disturb
    /// the rest of it.
    digits: Digits,
    /// How those digits are to be read as a time. `AsWritten` unless the user
    /// says which format the DAW was told to send, because the row does not
    /// carry that anywhere - see `crate::timecode`.
    layout: TimeLayout,
}

impl Decoder {
    pub fn new() -> Decoder {
        Decoder::with_layout(TimeLayout::AsWritten)
    }

    /// A decoder that reads the display row in the given format.
    pub fn with_layout(layout: TimeLayout) -> Decoder {
        Decoder {
            parser: Parser::new(),
            studio: Studio::empty(),
            arms: [false; BANK_CHANNELS],
            levels: [0; BANK_CHANNELS],
            selected: None,
            digits: Digits::default(),
            layout,
        }
    }

    /// Feeds one byte of the DAW's stream. The message that byte completed
    /// comes back, which is what the tests read.
    pub fn push(&mut self, byte: u8) -> Option<Message> {
        let message = self.parser.push(byte)?;
        self.apply(&message);
        Some(message)
    }

    /// The state the pages read.
    pub fn studio(&self) -> &Studio {
        &self.studio
    }

    /// The meter levels of the bank, one per channel.
    pub fn levels(&self) -> &[u8; BANK_CHANNELS] {
        &self.levels
    }

    /// The record-arm state of the bank's channels.
    pub fn arms(&self) -> &[bool; BANK_CHANNELS] {
        &self.arms
    }

    fn apply(&mut self, message: &Message) {
        match message {
            Message::Led { note, lit } => self.apply_led(*note, *lit),
            Message::Fader { channel, value } => {
                let index = *channel as usize;
                if index < BANK_CHANNELS {
                    self.studio.bank[index].fader = fader_fraction(*value);
                    self.mirror();
                }
            }
            Message::Level { channel, value } => {
                let index = *channel as usize;
                if index < BANK_CHANNELS {
                    self.levels[index] = *value;
                }
            }
            // The display is written one digit at a time, and the two texts the
            // pages read are derived from the row, so they travel together.
            Message::Display { digit, value } => {
                if self.digits.set(*digit as usize, *value) {
                    self.studio.display = self.digits.text();
                    self.studio.position = self.layout.position(&self.digits);
                    self.studio.position_short = self.layout.short_position(&self.digits);
                }
            }
            Message::Name { offset, text } => {
                let index = *offset as usize / NAME_FIELD;
                if index < BANK_CHANNELS {
                    self.studio.bank[index].name = text.clone();
                    self.mirror();
                }
            }
            Message::Status { .. } | Message::Other => {}
        }
    }

    fn apply_led(&mut self, note: u8, lit: bool) {
        match note {
            n if n < SOLO_NOTES => {
                let index = (n - ARM_NOTES) as usize;
                self.arms[index] = lit;
            }
            n if n < MUTE_NOTES => {
                let index = (n - SOLO_NOTES) as usize;
                self.studio.bank[index].solo = lit;
            }
            n if n < SELECT_NOTES => {
                let index = (n - MUTE_NOTES) as usize;
                self.studio.bank[index].mute = lit;
            }
            n if n < SELECT_NOTES + BANK_CHANNELS as u8 => {
                let index = (n - SELECT_NOTES) as usize;
                if lit {
                    self.selected = Some(index);
                } else if self.selected == Some(index) {
                    self.selected = None;
                }
            }
            STOP_LED => {
                if lit {
                    self.studio.transport = Transport::Stopped;
                }
            }
            PLAY_LED => {
                if lit {
                    self.studio.transport = Transport::Playing;
                }
            }
            RECORD_LED => {
                if lit {
                    self.studio.transport = Transport::Recording;
                }
            }
            LOOP_LED => self.studio.looping = lit,
            CLICK_LED => self.studio.click = lit,
            // Whatever only blinks, and the LEDs of a second unit.
            _ => {}
        }
        self.mirror();
    }

    /// The pages show one track's name, fader and flags; this puts the selected
    /// strip's values there, after anything that could have changed them.
    fn mirror(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        let channel = self.studio.bank[index].clone();
        self.studio.track_number = self.studio.bank_first_track + index as u16;
        self.studio.track_name = channel.name;
        self.studio.volume_fraction = channel.fader;
        self.studio.mute = channel.mute;
        self.studio.solo = channel.solo;
        self.studio.armed = self.arms[index];
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Decoder::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(decoder: &mut Decoder, bytes: &[u8]) -> Vec<Message> {
        bytes.iter().filter_map(|byte| decoder.push(*byte)).collect()
    }

    /// An LCD write, framed the way the capture has it.
    fn lcd(offset: u8, text: &[u8]) -> Vec<u8> {
        let mut sysex = SYSEX_HEADER.to_vec();
        sysex.push(LCD_WRITE);
        sysex.push(offset);
        sysex.extend_from_slice(text);
        sysex.push(0xf7);
        sysex
    }

    #[test]
    fn a_note_lights_a_led_and_note_off_and_velocity_zero_darken_it() {
        let mut decoder = Decoder::new();
        // Mute of the second strip is note 17, measured.
        let messages = feed(&mut decoder, &[0x90, 17, 127]);
        assert_eq!(messages, vec![Message::Led { note: 17, lit: true }]);
        assert!(decoder.studio().bank[1].mute);

        feed(&mut decoder, &[0x90, 17, 0]);
        assert!(!decoder.studio().bank[1].mute, "velocity zero must darken");

        feed(&mut decoder, &[0x90, 17, 127]);
        feed(&mut decoder, &[0x80, 17, 64]);
        assert!(!decoder.studio().bank[1].mute, "note off must darken");
    }

    #[test]
    fn the_four_led_rows_are_arm_solo_mute_and_select() {
        let mut decoder = Decoder::new();
        feed(&mut decoder, &[0x90, 3, 127]);
        assert!(decoder.arms()[3]);

        feed(&mut decoder, &[0x90, 10, 127]);
        assert!(decoder.studio().bank[2].solo);

        feed(&mut decoder, &[0x90, 16, 127]);
        assert!(decoder.studio().bank[0].mute);

        feed(&mut decoder, &[0x90, 27, 127]);
        assert_eq!(decoder.studio().track_number, 4, "select of strip four");
        assert!(decoder.studio().armed, "arm of that strip follows the select");
    }

    #[test]
    fn a_fader_message_becomes_a_position() {
        let mut decoder = Decoder::new();
        feed(&mut decoder, &[0xe0, 0x00, 0x40]); // centre
        assert!((decoder.studio().bank[0].fader - 0.5).abs() < 0.0001);

        feed(&mut decoder, &[0xe3, 0x00, 0x00]); // bottom, fourth strip
        assert_eq!(decoder.studio().bank[3].fader, 0.0);

        feed(&mut decoder, &[0xe3, 0x7f, 0x7f]); // top
        assert!((decoder.studio().bank[3].fader - 1.0).abs() < 0.0001);

        // The ninth channel is the master, which no page shows.
        feed(&mut decoder, &[0xe8, 0x7f, 0x7f]);
        assert_eq!(decoder.studio().bank.len(), BANK_CHANNELS);
    }

    #[test]
    fn a_name_field_lands_on_its_channel_and_a_status_line_does_not() {
        let mut decoder = Decoder::new();
        feed(&mut decoder, &lcd(21, b"2 Drum "));
        assert_eq!(decoder.studio().bank[3].name, "2 Drum");

        // Clearing a field is a write of seven spaces.
        feed(&mut decoder, &lcd(21, b"       "));
        assert_eq!(decoder.studio().bank[3].name, "");

        // Reaper's own status line uses the same command at offset zero with a
        // longer payload, and must not be read as the first channel's name.
        let mut decoder = Decoder::new();
        feed(&mut decoder, &lcd(0, b"REAPER! Initializing... Please wait..."));
        assert_eq!(decoder.studio().bank[0].name, "");
    }

    #[test]
    fn the_transport_leds_give_the_transport_state() {
        let mut decoder = Decoder::new();
        feed(&mut decoder, &[0x90, 94, 127]);
        assert_eq!(decoder.studio().transport, Transport::Playing);
        feed(&mut decoder, &[0x90, 93, 127]);
        assert_eq!(decoder.studio().transport, Transport::Stopped);
        feed(&mut decoder, &[0x90, 86, 127]);
        assert!(decoder.studio().looping);
        feed(&mut decoder, &[0x90, 89, 127]);
        assert!(decoder.studio().click);
    }

    #[test]
    fn the_meter_levels_are_kept_per_channel() {
        let mut decoder = Decoder::new();
        feed(&mut decoder, &[0xd2, 15, 0xd5, 96]);
        assert_eq!(decoder.levels()[2], 15);
        assert_eq!(decoder.levels()[5], 96);
    }

    #[test]
    fn framing_survives_running_status_and_a_sysex_split_in_two() {
        let mut decoder = Decoder::new();
        // One status byte, two controllers: running status.
        let messages = feed(&mut decoder, &[0xb0, 64, 48, 65, 49]);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0], Message::Display { digit: 0, value: 48 });

        // An LCD write fed byte by byte, the way the daemon will see it.
        let sysex = lcd(7, b"0 Lead ");
        let mut split = Decoder::new();
        let mut completed = Vec::new();
        for byte in sysex {
            if let Some(message) = split.push(byte) {
                completed.push(message);
            }
        }
        assert_eq!(completed.len(), 1);
        assert_eq!(split.studio().bank[1].name, "0 Lead");
    }

    #[test]
    fn anything_the_screen_does_not_read_changes_nothing() {
        let mut decoder = Decoder::new();
        let before = decoder.studio().clone();
        // A blinking LED and a V-Pot ring mode: neither is read yet, and both
        // must leave the state alone.
        feed(&mut decoder, &[0x90, 76, 127]);
        feed(&mut decoder, &[0xb0, 48, 3]);
        assert_eq!(decoder.studio(), &before);
    }

    #[test]
    fn the_displays_digits_land_from_the_right_and_only_where_they_are_written() {
        let mut decoder = Decoder::new();
        // The capture's own group write: the two assignment digits first, then
        // the ten of the timecode, with zeros where a digit is dark.
        feed(&mut decoder, &[0xb0, 75, 48, 74, 49]);
        feed(
            &mut decoder,
            &[
                0xb0, 64, 48, 65, 48, 66, 48, 67, 0, 68, 0, 69, 49, 70, 0, 71, 49, 72, 49, 73, 0,
            ],
        );

        // Read as the panel reads it: left to right, rightmost digit last.
        assert_eq!(decoder.studio().position, " 11 1  000");
        assert_eq!(decoder.studio().display, "01 11 1  000");

        // A jump of the playhead moves only the field it belongs to.
        feed(&mut decoder, &[0xb0, 71, 53, 72, 50]);
        assert_eq!(decoder.studio().position, " 25 1  000");
    }

    #[test]
    fn a_dot_rides_in_bit_six_of_a_digit() {
        let mut decoder = Decoder::new();
        feed(&mut decoder, &[0xb0, 64, 0x30, 65, 0x70]);
        // Digit one is `0` with the dot, digit zero is plain `0`.
        assert_eq!(decoder.studio().position, "        0.0");
    }
}
