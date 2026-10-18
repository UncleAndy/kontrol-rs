//! The DAW half of `doc/display-interface.md`: what Reaper's *Mackie Control
//! Universal* control surface sends, decoded into the `Studio` the pages read.
//!
//! Every number below was measured from a capture of the surface's own output
//! (`doc/Captured_Codes.md`, the log in `kk-mcu-20261017-032907`); the two
//! places where a number is still a guess say so. Nothing here ever sends a
//! byte back, so listening cannot disturb the DAW.

use crate::studio::{Digits, ASSIGNMENT_DIGITS, BANK_CHANNELS, DISPLAY_DIGITS, Studio, Transport};
use crate::knobs::{pan_from_dot, MASTER_CHANNEL, PAN_CC_FIRST, PAN_RING_CC_FIRST};
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
pub const STOP_LED: u8 = 93;
pub const PLAY_LED: u8 = 94;
pub const RECORD_LED: u8 = 95;

/// The loop and click LEDs: both lit in the step where the metronome and the
/// loop were toggled. 86 (0x56) is the loop LED in MCU's own LED map - the map
/// that predicted 93 and 94 correctly - which leaves 89 as the click.
pub const LOOP_LED: u8 = 86;
pub const CLICK_LED: u8 = 89;

/// What every MCU SysEx starts with, and the command byte of an LCD write.
const SYSEX_HEADER: [u8; 5] = [0xf0, 0x00, 0x00, 0x66, 0x14];
const LCD_WRITE: u8 = 0x12;

/// Device Query: the host asking the surface who it is.
const DEVICE_QUERY: u8 = 0x00;
/// Host Connection Query, our answer to one: serial and a challenge.
const HOST_QUERY: u8 = 0x01;
/// Host Connection Reply: the host's answer to our challenge.
const HOST_REPLY: u8 = 0x02;
/// Host Connection Confirmation, which is what we send back.
const HOST_CONFIRM: u8 = 0x03;

/// The seven ASCII bytes of the serial number the protocol has a surface
/// announce. Ours, not Mackie's: nothing depends on its value.
const SERIAL: [u8; 7] = *b"KMPL-01";

/// The four bytes of the challenge we send.
///
/// Fixed on purpose. A host computes its response from these bytes, and the same
/// bytes are what a response could be checked against, so a fixed challenge
/// costs nothing - and it makes the exchange reproducible when something is
/// wrong.
const CHALLENGE: [u8; 4] = [0x4b, 0x4f, 0x4d, 0x50];

/// The response a host is expected to send for a challenge, from the algorithm
/// in the protocol reference:
///
/// ```text
/// r[0] = 0x7F & ( c[0] + ( c[1] ^ 0x0A ) - c[3] )
/// r[1] = 0x7F & ( ( c[2] >> 4 ) ^ ( c[0] + c[3] ) )
/// r[2] = 0x7F & ( c[3] - ( c[2] << 2 ) ^ ( c[0] | c[1] ) )
/// r[3] = 0x7F & ( c[1] - c[2] + ( 0xF0 ^ ( c[3] << 4 ) ) )
/// ```
pub fn response_for(challenge: [u8; 4]) -> [u8; 4] {
    let c = challenge.map(i32::from);
    [
        (0x7F & (c[0] + (c[1] ^ 0x0A) - c[3])) as u8,
        (0x7F & ((c[2] >> 4) ^ (c[0] + c[3]))) as u8,
        (0x7F & ((c[3] - (c[2] << 2)) ^ (c[0] | c[1]))) as u8,
        (0x7F & (c[1] - c[2] + (0xF0 ^ (c[3] << 4)))) as u8,
    ]
}

/// Host Connection Query: our serial and our challenge, in the order the
/// protocol puts them.
fn host_connection_query() -> Vec<u8> {
    let mut reply = Vec::with_capacity(SYSEX_HEADER.len() + 12);
    reply.extend_from_slice(&SYSEX_HEADER);
    reply.push(HOST_QUERY);
    reply.extend_from_slice(&SERIAL);
    reply.extend_from_slice(&CHALLENGE);
    reply.push(0xf7);
    reply
}

/// Host Connection Confirmation: our serial, and nothing else.
fn host_connection_confirmation() -> Vec<u8> {
    let mut reply = Vec::with_capacity(SYSEX_HEADER.len() + 9);
    reply.extend_from_slice(&SYSEX_HEADER);
    reply.push(HOST_CONFIRM);
    reply.extend_from_slice(&SERIAL);
    reply.push(0xf7);
    reply
}

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
    /// A meter, on the strip's channel pressure. The strip is the *high* nibble
    /// of the data byte and the level the low one: `0xD0 0x35` is strip 3 at
    /// level 5, not channel 0 at level 0x35.
    Level { strip: u8, level: u8 },
    /// The V-Pot's ring: where a strip's pan is, as the DAW sees it. The only
    /// pan feedback there is - see `knobs::pan_from_dot`.
    Ring { strip: u8, dot: u8 },
    /// A V-Pot being moved: the strip's pan, on the strip's own controller
    /// number. Unlike the fader this is not a channel of its own - the strips
    /// are told apart by the CC number, and they all arrive on channel one.
    Pan { strip: u8, value: u8 },
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
    /// The host asking who we are: the port answers it with a Host Connection
    /// Query, serial and challenge both.
    DeviceQuery,
    /// The host's answer to our challenge. The bytes are not kept: see the arm
    /// in the decoder for why the confirmation goes out regardless.
    HostReply,
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
            strip: data[0] >> 4,
            level: data[0] & 0x0f,
        },
        0xe0 => Message::Fader {
            channel,
            value: ((data[1] as i16) << 7 | data[0] as i16) - 8192,
        },
        // The twelve-digit display comes over the controllers numbered from 64,
        // which is the rightmost digit. Nothing else lives in the controllers
        // this decoder reads, so the rest is still unread.
        0xb0 if (PAN_RING_CC_FIRST..PAN_RING_CC_FIRST + BANK_CHANNELS as u8).contains(&data[0]) => {
            Message::Ring {
                strip: data[0] - PAN_RING_CC_FIRST,
                // Not a plain position: bits 0-3 hold the dot, bits 4-5 the ring
                // mode (Reaper only ever used mode 0, a single dot), bit 6 the
                // LED under the encoder. The panel draws a dot, so the rest is
                // masked off rather than read.
                dot: data[1] & 0x0f,
            }
        },
        0xb0 if (PAN_CC_FIRST..PAN_CC_FIRST + BANK_CHANNELS as u8).contains(&data[0]) => {
            Message::Pan {
                strip: data[0] - PAN_CC_FIRST,
                value: data[1],
            }
        }
        0xb0 if (64..64 + DISPLAY_DIGITS as u8).contains(&data[0]) => Message::Display {
            digit: data[0] - 64,
            value: data[1],
        },
        _ => Message::Other,
    }
}

fn decode_sysex(bytes: &[u8]) -> Message {
    if bytes.len() < SYSEX_HEADER.len() + 1 || bytes[..SYSEX_HEADER.len()] != SYSEX_HEADER {
        return Message::Other;
    }
    match bytes[SYSEX_HEADER.len()] {
        // The host asking who we are, and the host answering our challenge: the
        // only two commands a surface ever answers on its own, and the two that
        // carry no payload of their own at all.
        DEVICE_QUERY => return Message::DeviceQuery,
        HOST_REPLY => return Message::HostReply,
        // An LCD write carries an offset byte after the command, so it has to
        // be one longer than the two above before its body makes sense.
        LCD_WRITE if bytes.len() >= SYSEX_HEADER.len() + 2 => {}
        _ => return Message::Other,
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
    /// A handshake reply waiting to be sent. The parser cannot send anything by
    /// itself, so a Device Query leaves its answer here for the port to take.
    handshake: Option<Vec<u8>>,
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
            handshake: None,
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

    /// Take a handshake reply the parser produced, if one is waiting.
    ///
    /// The parser has no output of its own: a Device Query has to be answered
    /// through the port that does the sending, so the bytes are parked here for
    /// `McuInput::read_into` to pick up - and taken once, so an answer is never
    /// sent twice.
    pub fn take_handshake_reply(&mut self) -> Option<Vec<u8>> {
        self.handshake.take()
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
                } else if index == MASTER_CHANNEL as usize {
                    // The ninth fader: the master, which the strips leave over.
                    self.studio.master_fader = fader_fraction(*value);
                }
            }
            Message::Ring { strip, dot } => {
                let index = *strip as usize;
                // Only until the DAW says something exact: the ring is a twelfth
                // of the travel wide, so it seeds where the pan is and the
                // controller value corrects it. A ring that arrives again must
                // not pull a pan the tool has since set back to the coarse dot.
                if index < BANK_CHANNELS && !self.studio.bank[index].pan_known {
                    self.studio.bank[index].pan = pan_from_dot(*dot);
                    self.studio.bank[index].pan_known = true;
                    self.mirror();
                }
            }
            Message::DeviceQuery => {
                // The host asked who we are: answer with the serial and the
                // challenge, which is what a real surface does at power-on as
                // well. The port sends it - see `take_handshake_reply`.
                self.handshake = Some(host_connection_query());
            }
            Message::HostReply => {
                // The host answered the challenge. `response_for` knows what the
                // answer should have been, and the answer is deliberately not
                // enforced: the point of this is to be let in, and a mismatch is
                // far more likely to mean the two sides disagree about the
                // arithmetic than a host that ought to be turned away.
                self.handshake = Some(host_connection_confirmation());
            }
            Message::Pan { strip, value } => {
                let index = *strip as usize;
                if index < BANK_CHANNELS {
                    self.studio.bank[index].pan = f32::from(*value) / 127.0;
                    self.studio.bank[index].pan_known = true;
                    self.mirror();
                }
            }
            Message::Level { strip, level } => {
                let index = *strip as usize;
                if index < BANK_CHANNELS {
                    self.levels[index] = *level;
                }
            }
            // The display is written one digit at a time, and the two texts the
            // pages read are derived from the row, so they travel together.
            Message::Display { digit, value } => {
                if self.digits.set(*digit as usize, *value) {
                    self.studio.display = self.digits.text();
                    self.studio.position = self.layout.position(&self.digits);
                    self.studio.position_short = self.layout.short_position(&self.digits);
                    self.studio.assignment = self.digits.assignment();
                    self.take_the_bank_from_the_assignment();
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

    /// The bank's first track, taken from the assignment display.
    ///
    /// The two digits on the left of the DAW's row are where the surface is, not
    /// what is selected: the capture's window in which the select LEDs move
    /// between two tracks of one bank carries **no write to them at all**, and the
    /// number of the selected track would have had to change there. Measured on
    /// hardware as well: the surface sitting on tracks 9..16 read `09 000010 00`.
    /// That makes them the only absolute track number the stream carries, and the
    /// reason the tracks page can number past the eighth track at all.
    ///
    /// Anything that is not a two-digit number - a blank pair, letters from the
    /// protocol's own character table - leaves the bank where it was.
    fn take_the_bank_from_the_assignment(&mut self) {
        let assignment = self.studio.assignment.trim();
        if assignment.len() != ASSIGNMENT_DIGITS
            || !assignment.bytes().all(|byte| byte.is_ascii_digit())
        {
            return;
        }
        let Ok(number) = assignment.parse::<u16>() else {
            return;
        };
        if number >= 1 && number != self.studio.bank_first_track {
            self.studio.bank_first_track = number;
            self.mirror();
        }
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
    fn the_ring_says_where_a_pan_stands_until_the_pan_itself_arrives() {
        let mut decoder = Decoder::new();
        // A centred track: dot six of twelve, and that is the DAW's own word.
        feed(&mut decoder, &[0xb0, 0x30, 6]);
        assert!((decoder.studio().bank[0].pan - 0.5).abs() < 0.01, "the middle dot");
        assert!(decoder.studio().bank[0].pan_known);
        // Each strip has a controller number of its own.
        feed(&mut decoder, &[0xb0, 0x31, 11]);
        assert!(decoder.studio().bank[1].pan > 0.99, "the widest dot is hard right");
        feed(&mut decoder, &[0xb0, 0x32, 0]);
        assert!(decoder.studio().bank[2].pan < 0.01, "and dot zero is hard left");
        // The exact value wins, and a ring arriving afterwards must not pull the
        // pan back to what the coarse dot can say.
        feed(&mut decoder, &[0xb0, 16, 100]);
        let exact = decoder.studio().bank[0].pan;
        assert!(exact > 0.7, "the controller value is the truth");
        feed(&mut decoder, &[0xb0, 0x30, 6]);
        assert_eq!(decoder.studio().bank[0].pan, exact, "the coarse dot keeps quiet");
    }

    #[test]
    fn the_v_pots_and_the_master_fader_are_read_back() {
        let mut decoder = Decoder::new();
        // A V-Pot of the first strip, in the middle and then hard left.
        feed(&mut decoder, &[0xb0, 16, 64]);
        assert!(
            (decoder.studio().bank[0].pan - 0.5).abs() < 0.01,
            "the V-Pot's middle is 64, and reads back as the middle"
        );
        feed(&mut decoder, &[0xb0, 16, 0]);
        assert!(decoder.studio().bank[0].pan < 0.01, "hard left");
        // The second strip has a controller number of its own, and the first
        // one must not move with it.
        feed(&mut decoder, &[0xb0, 17, 127]);
        assert!(decoder.studio().bank[1].pan > 0.99, "hard right");
        assert!(decoder.studio().bank[0].pan < 0.01, "and the first one stays");
        // The master: the ninth fader, which the eight strips leave over.
        let before = decoder.studio().master_fader;
        feed(&mut decoder, &[0xe8, 0x00, 0x40]);
        assert!(
            (decoder.studio().master_fader - 0.5).abs() < 0.01,
            "half-way up the master"
        );
        assert_ne!(before, decoder.studio().master_fader);
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
    fn the_assignment_digits_say_which_bank_the_surface_is_on() {
        let mut decoder = Decoder::new();
        // Measured on hardware: with the surface showing tracks 9..16 the two
        // leftmost characters read "09". Controller 75 is the leftmost digit and
        // 74 the one after it, so the pair arrives right to left like the rest.
        feed(&mut decoder, &[0xb0, 75, 48, 74, 57]);
        assert_eq!(decoder.studio().assignment, "09");
        assert!(decoder.studio().display.starts_with("09"));
        assert_eq!(
            decoder.studio().bank_first_track,
            9,
            "the two digits are the bank's first track, not the selection"
        );

        // The eighth strip of that bank is track sixteen, not eight.
        feed(&mut decoder, &[0x90, 31, 127]);
        assert_eq!(decoder.studio().track_number, 16);

        // And back to the first bank: the numbers follow it down again.
        feed(&mut decoder, &[0xb0, 75, 48, 74, 49]);
        assert_eq!(decoder.studio().bank_first_track, 1);
        assert_eq!(decoder.studio().track_number, 8);
    }

    #[test]
    fn a_pair_that_is_not_a_number_leaves_the_bank_alone() {
        let mut decoder = Decoder::new();
        feed(&mut decoder, &[0xb0, 75, 48, 74, 57]);
        assert_eq!(decoder.studio().bank_first_track, 9);

        // Zeros, a blank pair and letters from the protocol's own table: none of
        // them is a bank.
        for pair in [[48u8, 48u8], [0u8, 0u8], [2u8, 20u8]] {
            feed(&mut decoder, &[0xb0, 75, pair[0], 74, pair[1]]);
            assert_eq!(
                decoder.studio().bank_first_track,
                9,
                "a pair of {pair:?} is not a bank number"
            );
        }
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
    fn a_device_query_is_answered_with_the_serial_and_a_challenge() {
        let reply = host_connection_query();
        assert_eq!(reply.len(), SYSEX_HEADER.len() + 13);
        assert_eq!(reply[..SYSEX_HEADER.len()], SYSEX_HEADER);
        assert_eq!(reply[SYSEX_HEADER.len()], HOST_QUERY);
        assert_eq!(
            &reply[SYSEX_HEADER.len() + 1..SYSEX_HEADER.len() + 8],
            &SERIAL
        );
        assert_eq!(
            &reply[SYSEX_HEADER.len() + 8..SYSEX_HEADER.len() + 12],
            &CHALLENGE
        );
        assert_eq!(*reply.last().unwrap(), 0xf7);

        let confirm = host_connection_confirmation();
        assert_eq!(confirm[SYSEX_HEADER.len()], HOST_CONFIRM);
        assert_eq!(
            &confirm[SYSEX_HEADER.len() + 1..SYSEX_HEADER.len() + 8],
            &SERIAL
        );
        assert_eq!(*confirm.last().unwrap(), 0xf7);
    }

    #[test]
    fn the_response_algorithm_matches_the_reference() {
        // Worked out from the pseudo code in the reference for this very
        // challenge, so a slip in the arithmetic cannot pass unnoticed.
        assert_eq!(response_for(CHALLENGE), [0x40, 0x1f, 0x53, 0x72]);
    }

    #[test]
    fn a_device_query_through_the_decoder_leaves_one_reply_to_send() {
        let mut decoder = Decoder::new();
        let query = [0xf0, 0x00, 0x00, 0x66, 0x14, 0x00, 0xf7];
        feed(&mut decoder, &query);
        let reply = decoder.take_handshake_reply().expect("the query went unanswered");
        assert_eq!(reply[SYSEX_HEADER.len()], HOST_QUERY);
        assert!(
            decoder.take_handshake_reply().is_none(),
            "the same answer was left waiting twice"
        );

        // And the host's reply to the challenge is confirmed.
        feed(
            &mut decoder,
            &[0xf0, 0x00, 0x00, 0x66, 0x14, 0x02, 0x40, 0x1f, 0x53, 0x72, 0xf7],
        );
        let confirm = decoder
            .take_handshake_reply()
            .expect("the host's reply went unconfirmed");
        assert_eq!(confirm[SYSEX_HEADER.len()], HOST_CONFIRM);
    }

    #[test]
    fn the_meter_levels_are_kept_per_channel() {
        let mut decoder = Decoder::new();
        // The strip is the high nibble of the data byte and the level the low
        // one. This test used to feed 0xd2 and 0xd5 - the strip in the status
        // byte - with a level of 96, which a nibble cannot hold. That was the
        // bug the protocol reference caught.
        feed(&mut decoder, &[0xd0, 0x2f, 0xd0, 0x56]);
        assert_eq!(decoder.levels()[2], 15);
        assert_eq!(decoder.levels()[5], 6);
    }

    #[test]
    fn a_meter_sweep_addresses_every_strip_instead_of_one() {
        // What Reaper actually sends, all through the capture: D0 00, D0 10,
        // D0 20 ... D0 70. Read as a status-byte channel that is one strip with a
        // rising level; read properly it is eight strips, each silent, which is
        // what a meter bank looks like when nothing is playing.
        let mut decoder = Decoder::new();
        for strip in 0..8u8 {
            feed(&mut decoder, &[0xd0, strip << 4]);
        }
        for strip in 0..BANK_CHANNELS {
            assert_eq!(decoder.levels()[strip], 0);
        }
        // And the low nibble is a level, not part of the strip.
        feed(&mut decoder, &[0xd0, 0x35]);
        assert_eq!(decoder.levels()[3], 5);
        assert_eq!(decoder.levels()[0], 0);
    }

    #[test]
    fn a_ring_byte_carries_the_mode_and_the_led_as_well_as_the_dot() {
        // 0x46: dot 6, mode 0, the LED under the encoder lit. Only the dot is
        // drawn, so only the dot is kept - but the rest must not leak into it.
        match decode(0xb0, [0x30, 0x46]) {
            Message::Ring { strip, dot } => {
                assert_eq!(strip, 0);
                assert_eq!(dot, 6);
            }
            _ => panic!("expected a ring"),
        }
        match decode(0xb0, [0x37, 0x3f]) {
            Message::Ring { strip, dot } => {
                assert_eq!(strip, 7);
                assert_eq!(dot, 15);
            }
            _ => panic!("expected a ring"),
        }
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
        // A blinking LED and a controller nothing reads. (The ring is read now:
        // the capture showed its values running five to eleven, which a mode of
        // four values cannot be - it is the dot the pan stands on.)
        feed(&mut decoder, &[0x90, 76, 127]);
        feed(&mut decoder, &[0xb0, 80, 3]);
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
