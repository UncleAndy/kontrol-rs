//! The decoder against the DAW's own bytes.
//!
//! The fixture is a slice of the capture described in `doc/Captured_Codes.md`,
//! converted to raw MIDI by `tools/mcu-slice.awk`: four windows of Reaper's
//! Mackie Control Universal output, in the order the DAW sent them. Testing on
//! the real stream is what keeps the decoder honest - its numbers were measured
//! the same way, and the fixture travels with the code.

use std::fs;

use kontrol_rs::mcu::{Decoder, Message, NAME_FIELD};
use kontrol_rs::studio::{Transport, DISPLAY_DIGITS, TIMECODE_DIGITS};

/// A status line is longer than one name field; that is what tells them apart.
const NAME_FIELD_TOO_LONG: usize = NAME_FIELD;

/// Every message of the fixture, as bytes.
fn fixture_messages() -> Vec<Vec<u8>> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mcu-reaper.txt");
    let text = fs::read_to_string(path).unwrap_or_else(|err| panic!("{path}: {err}"));
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            line.split_whitespace()
                .map(|word| {
                    u8::from_str_radix(word, 16).unwrap_or_else(|_| panic!("not hex: {word:?}"))
                })
                .collect()
        })
        .collect()
}

/// How many messages of the fixture start with each status family.
fn status_families(messages: &[Vec<u8>]) -> [usize; 6] {
    let mut families = [0usize; 6];
    for message in messages {
        let status = message[0] & 0xf0;
        let slot = match status {
            0x80 | 0x90 => 0,
            0xb0 => 1,
            0xd0 => 2,
            0xe0 => 3,
            0xf0 => 4,
            other => panic!("unexpected status {other:#x}"),
        };
        families[slot] += 1;
    }
    families
}

#[test]
fn the_decoder_reads_reapers_own_stream() {
    let messages = fixture_messages();
    let mut decoder = Decoder::new();

    let mut leds = 0usize;
    let mut faders = 0usize;
    let mut levels = 0usize;
    let mut name_writes = 0usize;
    let mut statuses = 0usize;
    let mut digits = 0usize;
    let mut others = 0usize;

    let mut was_playing = false;
    let mut then_stopped = false;
    let mut track_two_muted = false;
    let mut track_two_unmuted = false;
    let mut selected_two = false;
    let mut fader_first = None;
    let mut fader_last = 0.0f32;

    for message in &messages {
        for byte in message {
            let Some(decoded) = decoder.push(*byte) else {
                continue;
            };
            match decoded {
                Message::Led { .. } => leds += 1,
                Message::Fader { .. } => faders += 1,
                Message::Level { .. } => levels += 1,
                // V-Pots and their rings: controller messages, and they count
                // with the rest of them.
                Message::Pan { .. } => others += 1,
                Message::Ring { .. } => others += 1,
                Message::Name { .. } => name_writes += 1,
                Message::Status { ref text } => {
                    // The status line is Reaper's own, and must never be read
                    // as a track name.
                    assert!(
                        text.chars().count() > NAME_FIELD_TOO_LONG,
                        "a name field was taken for a status line: {text:?}"
                    );
                    statuses += 1;
                }
                Message::Display { .. } => digits += 1,
                Message::Other => others += 1,
            }
        }

        let studio = decoder.studio();
        match studio.transport {
            Transport::Playing => was_playing = true,
            Transport::Stopped => {
                if was_playing {
                    then_stopped = true;
                }
            }
            Transport::Recording => {}
        }
        if studio.bank[1].mute {
            track_two_muted = true;
        } else if track_two_muted {
            track_two_unmuted = true;
        }
        if studio.track_number == 2 {
            selected_two = true;
        }
        if fader_first.is_none() && studio.bank[3].fader > 0.0 {
            fader_first = Some(studio.bank[3].fader);
        } else if fader_first.is_some() {
            fader_last = studio.bank[3].fader;
        }
    }

    // Nothing was lost and nothing was invented: every message of the fixture
    // came back as exactly one message, in the family its first byte names.
    let families = status_families(&messages);
    assert_eq!(leds, families[0], "notes");
    assert_eq!(levels, families[2], "aftertouch");
    assert_eq!(faders, families[3], "pitch bend");
    assert_eq!(
        name_writes + statuses + others + digits,
        families[1] + families[4],
        "controllers and SysEx"
    );
    assert!(name_writes >= 7, "the fixture writes seven name fields: {name_writes}");
    assert!(statuses >= 1, "Reaper's status line is in the fixture: {statuses}");

    let studio = decoder.studio();

    // The names, as the DAW spelled them: one field per channel, seven
    // characters each, at offsets 7, 14 ... 49.
    let names: Vec<&str> = studio.bank.iter().map(|channel| channel.name.as_str()).collect();
    assert_eq!(
        &names[1..],
        &["0 Lead", "1 Back", "2 Drum", "3 Bass", "4 Guit", "5 Keyb", "6 Perc"]
    );

    // The transport ran and then stopped inside the first window.
    assert!(was_playing, "no play LED was seen");
    assert!(then_stopped, "the stop LED after the play LED was not seen");
    assert_eq!(studio.transport, Transport::Stopped);

    // Mute of the second strip is note 17: it went on, and later off again.
    assert!(track_two_muted, "note 17 never lit");
    assert!(track_two_unmuted, "note 17 never went dark again");
    assert!(!studio.bank[1].mute);

    // Selecting a strip fills the track fields from that strip.
    assert!(selected_two, "the select LED of the second strip was not seen");
    assert_eq!(studio.track_number, 2);
    assert_eq!(studio.track_name, studio.bank[1].name);
    assert_eq!(studio.volume_fraction, studio.bank[1].fader);

    // One fader was moved through the fourth window, upwards.
    let first = fader_first.expect("no fader position arrived");
    assert!(
        fader_last > first + 0.1,
        "the fader did not move: {first} then {fader_last}"
    );

    // Meters arrive as aftertouch, one per channel, and are kept.
    assert!(decoder.levels().iter().any(|level| *level > 0), "no meter levels");

    // The DAW's own display arrived as digits, and the twelve of them make the
    // row the panel now shows: ten of the timecode and two of the assignment
    // digits the protocol puts left of it.
    assert!(digits > 100, "the display digits are in the fixture: {digits}");
    assert_eq!(
        studio.display.chars().filter(|c| *c != '.').count(),
        DISPLAY_DIGITS,
        "the row is twelve digits, with a dot among them where the DAW set one: {:?}",
        studio.display
    );
    assert_eq!(
        studio.position.chars().filter(|c| *c != '.').count(),
        TIMECODE_DIGITS,
        "the timecode is the ten rightmost of them: {:?}",
        studio.position
    );
    assert!(
        studio.display.starts_with("01"),
        "the assignment digits of the capture read 01: {:?}",
        studio.display
    );
    // Pinned to what the fixture actually says, so a change in the decode shows
    // up here instead of quietly moving a digit on the panel: bar 25, beat 1,
    // tick 000, with the digits the DAW turned off left as holes.
    assert_eq!(studio.display, "01 25 1  000");
    assert_eq!(studio.position, " 25 1  000");

    println!(
        "fixture: {} messages (led {leds}, level {levels}, fader {faders}, name {name_writes}, status {statuses}, digit {digits}, other {others})\n\
         names: {names:?}\n\
         display: {:?}, timecode: {:?}\n\
         transport: {:?}, looping {}, click {}\n\
         track: {} {:?}, fader {:?}\n\
         levels: {:?}",
        messages.len(),
        studio.display,
        studio.position,
        studio.transport,
        studio.looping,
        studio.click,
        studio.track_number,
        studio.track_name,
        studio.volume_fraction,
        decoder.levels(),
    );
}
