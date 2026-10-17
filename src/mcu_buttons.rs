//! The MCU buttons this program can press, and the names they go by in a mapping
//! file: `mcu:<name>`.
//!
//! In MCU a button and its LED share a note - the DAW lights the button by
//! sending that note, and it expects the same note back when the button is
//! pressed. The numbers here are therefore measured rather than chosen: the
//! transport ones are the decoder's own constants ([`crate::mcu`], the half that
//! reads the DAW), and the capture in `tests/fixtures/mcu-reaper.txt` is what
//! showed them in the first place.

/// Play. The DAW lights it while it plays.
pub const PLAY: u8 = crate::mcu::PLAY_LED;
/// Stop. Lit right after the transport stopped.
pub const STOP: u8 = crate::mcu::STOP_LED;
/// Record.
pub const RECORD: u8 = crate::mcu::RECORD_LED;
/// The loop, MCU's own "Cycle".
pub const LOOP: u8 = crate::mcu::LOOP_LED;
/// The metronome, MCU's "Click".
pub const CLICK: u8 = crate::mcu::CLICK_LED;

/// Bank and channel (track) navigation. Not in our capture, so these are MCU's
/// published numbers rather than measured ones - and a mapping file can name an
/// explicit note (`mcu:46`) if a DAW turns out to disagree.
pub const BANK_LEFT: u8 = 46;
pub const BANK_RIGHT: u8 = 47;
pub const TRACK_LEFT: u8 = 48;
pub const TRACK_RIGHT: u8 = 49;

/// The prefix a mapping value must carry. Required rather than optional: MMC's
/// commands go by the same names - "play", "stop" - and mean something else, so
/// the spelled-out form is the only unambiguous one.
const PREFIX: &str = "mcu:";

/// The names a mapping file can use, with the spellings that read naturally next
/// to our own button names.
const NAMES: &[(&str, u8)] = &[
    ("play", PLAY),
    ("stop", STOP),
    ("record", RECORD),
    ("loop", LOOP),
    ("cycle", LOOP),
    ("click", CLICK),
    ("metro", CLICK),
    ("metronome", CLICK),
    ("bank_left", BANK_LEFT),
    ("bank_right", BANK_RIGHT),
    ("bankleft", BANK_LEFT),
    ("bankright", BANK_RIGHT),
    ("track_left", TRACK_LEFT),
    ("track_right", TRACK_RIGHT),
    ("trackleft", TRACK_LEFT),
    ("trackright", TRACK_RIGHT),
    ("track_prev", TRACK_LEFT),
    ("track_next", TRACK_RIGHT),
];

/// The note a mapping value names, or an explicit number, or `None` when it
/// names nothing - which the parser then says out loud instead of guessing at.
pub fn mcu_button_parse(value: &str) -> Option<u8> {
    let text = value.trim();
    if text.len() < PREFIX.len() || !text[..PREFIX.len()].eq_ignore_ascii_case(PREFIX) {
        return None;
    }
    let text = text[PREFIX.len()..].trim();
    if let Some((_, note)) = NAMES.iter().find(|(name, _)| name.eq_ignore_ascii_case(text)) {
        return Some(*note);
    }
    // A number, for whatever MCU has that this table does not name.
    text.parse::<i32>()
        .ok()
        .filter(|note| (0..=127).contains(note))
        .map(|note| note as u8)
}

/// The name of a note, for the listing the loader prints when asked.
pub fn mcu_button_name(note: i32) -> &'static str {
    for (name, candidate) in NAMES {
        if i32::from(*candidate) == note {
            return name;
        }
    }
    "<unknown note>"
}

/// The bytes one button edge is made of, in the shape the capture shows the DAW
/// using for its own buttons: `90 <note> 7F` pressed, `80 <note> 40` released.
pub fn mcu_button_message(note: u8, press: bool) -> [u8; 3] {
    if press {
        [0x90, note & 0x7f, 0x7f]
    } else {
        [0x80, note & 0x7f, 0x40]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_transport_notes_are_the_ones_the_daw_uses() {
        // The DAW's own bytes, from the capture: the transport window has play lit
        // and then stop lit, and record switched off next to them.
        let capture = include_str!("../tests/fixtures/mcu-reaper.txt");
        assert!(
            capture.contains(&format!("90 {:02X} 7F", PLAY)),
            "the note we send for play is the one the DAW lights"
        );
        assert!(
            capture.contains(&format!("80 {:02X} 40", STOP)),
            "and stop"
        );
        assert!(
            capture.contains(&format!("80 {:02X} 40", RECORD)),
            "and record"
        );
        // Loop and click are the decoder's, which is the half that reads them.
        assert_eq!(LOOP, 86);
        assert_eq!(CLICK, 89);
    }

    #[test]
    fn parses_names_and_numbers_and_refuses_the_rest() {
        assert_eq!(mcu_button_parse("mcu:play"), Some(PLAY));
        assert_eq!(mcu_button_parse("MCU:Play"), Some(PLAY));
        assert_eq!(mcu_button_parse("mcu: metro "), Some(CLICK));
        assert_eq!(mcu_button_parse("mcu:track_next"), Some(TRACK_RIGHT));
        assert_eq!(mcu_button_parse("mcu:94"), Some(94));
        // Without the prefix this is not an MCU value at all: those names are
        // MMC's, and MMC means something else by them.
        assert_eq!(mcu_button_parse("play"), None);
        assert_eq!(mcu_button_parse("mcu:"), None);
        assert_eq!(mcu_button_parse("mcu:nowhere"), None);
        assert_eq!(mcu_button_parse("mcu:200"), None);
        // And naming a note back.
        assert_eq!(mcu_button_name(i32::from(PLAY)), "play");
        assert_eq!(mcu_button_name(999), "<unknown note>");
    }

    #[test]
    fn an_edge_is_the_shape_the_capture_shows() {
        assert_eq!(mcu_button_message(PLAY, true), [0x90, 94, 0x7f]);
        assert_eq!(mcu_button_message(PLAY, false), [0x80, 94, 0x40]);
    }
}
