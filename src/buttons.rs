//! The button table of the A-series protocol, mirroring `button_names.c`.
//!
//! Bit N of the HID report's button mask is the button with index N; indexes
//! 40/41 are virtual ("buttons" that stand for the 4D dial turning, which is
//! handled outside the button loop).

use crate::defs::INVALID_KEY_OR_BUTTON;

/// Buttons that the HID report carries (bits 0..39).
pub const TOGGLE_BUTTON_TOTAL: usize = 40;
/// Virtual index for a clockwise turn of the 4D dial.
pub const DIAL_CW_INDEX: usize = 40;
/// Virtual index for a counter-clockwise turn of the 4D dial.
pub const DIAL_CCW_INDEX: usize = 41;
/// The buttons plus the two virtual dial indexes.
pub const REAL_BUTTON_TOTAL: usize = TOGGLE_BUTTON_TOTAL + 2;

/// The largest report we read/dump (see `hid_dump`).
pub const HID_DUMP_PACKET_SZ: usize = 64;

pub static BUTTON_NAMES: [&str; REAL_BUTTON_TOTAL] = [
    "Shift",
    "Scale",
    "Arp",
    "Undo",
    "Quantize",
    "Ideas",
    "Loop",
    "Metro",
    "Tempo",
    "Play",
    "Record",
    "Stop",
    "Preset Up",
    "Preset Down",
    // Measured on an A49 (doc/Captured_Codes.md): bits 14 and 15 are the two
    // buttons beside the display, which step through its pages. The inherited
    // table called them Mute and Solo, but this keyboard has no such buttons;
    // the old names still resolve, see BUTTON_ALIASES.
    "Page Left",  // bit 14, buf[2] 0x40
    "Page Right", // bit 15, buf[2] 0x80
    "Browser",
    "Plug-In",
    "Track",
    "Octave Down",
    "Octave Up",
    "4D Up",     // bit 21, buf[3] 0x20
    "4D Left",   // bit 22, buf[3] 0x40  } measured on an A49, see
    "4D Right",  // bit 23, buf[3] 0x80  } doc/Captured_Codes.md
    "4D Down",   // bit 24, buf[4] 0x01
    "Rotary1",
    "Rotary2",
    "Rotary3",
    "Rotary4",
    "Rotary5",
    "Rotary6",
    "Rotary7",
    "Rotary8",
    "4D Button",
    "Button34",
    "Button35",
    "Button36",
    "Button37",
    "Button38",
    "Button39",
    // Handled outside the button loop.
    "4D CW",  // clockwise rotation of the 4D dial
    "4D CCW", // counter-clockwise rotation of the 4D dial
];

/// Names that mapping files of the original tool use for buttons that were
/// renamed once the hardware was measured. Both spellings resolve, so an old
/// mapping file keeps working.
pub static BUTTON_ALIASES: [(&str, usize); 2] = [("Mute", 14), ("Solo", 15)];

pub fn get_button_name(index: i32) -> &'static str {
    if index >= 0 && (index as usize) < REAL_BUTTON_TOTAL {
        BUTTON_NAMES[index as usize]
    } else {
        INVALID_KEY_OR_BUTTON
    }
}

/// Resolves a button name (or the alternative `ButtonN` spelling) to its index.
///
/// Like `get_button_index()` in the C tool this compares only as many
/// characters as the given name has, so a name that is a *prefix* of a table
/// entry also matches. That is deliberate: mapping files of the original tool
/// rely on it, and the shipped files use full names anyway.
pub fn get_button_index(name: &str) -> Option<usize> {
    for (index, entry) in BUTTON_NAMES.iter().enumerate() {
        if starts_with_ignore_case(entry, name) {
            return Some(index);
        }
        let alternate = format!("Button{index}");
        if starts_with_ignore_case(&alternate, name) {
            return Some(index);
        }
    }

    // Old spellings come last: a name that the table itself matches must never
    // be taken over by an alias.
    for (old, index) in BUTTON_ALIASES {
        if starts_with_ignore_case(old, name) {
            return Some(index);
        }
    }

    None
}

/// `entry` starts with `name`, ignoring case and using `name`'s length, which
/// is what `strncasecmp(name, entry, strlen(name))` does in C.
fn starts_with_ignore_case(entry: &str, name: &str) -> bool {
    entry.len() >= name.len() && entry[..name.len()].eq_ignore_ascii_case(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_looked_up() {
        assert_eq!(get_button_index("Shift"), Some(0));
        assert_eq!(get_button_index("shift"), Some(0));
        assert_eq!(get_button_index("4D CCW"), Some(41));
        assert_eq!(get_button_index("Button34"), Some(34));
        assert_eq!(get_button_index("Rotary8"), Some(32));
        assert_eq!(get_button_index("nonsense"), None);
    }

    #[test]
    fn index_to_name_round_trips() {
        for index in 0..REAL_BUTTON_TOTAL {
            let name = get_button_name(index as i32);
            assert_eq!(get_button_index(name), Some(index));
        }
        assert_eq!(get_button_name(-1), INVALID_KEY_OR_BUTTON);
        assert_eq!(get_button_name(42), INVALID_KEY_OR_BUTTON);
    }

    #[test]
    fn the_two_buttons_beside_the_display_are_named_for_what_they_do() {
        // Measured on an A49: bit 14 is the left button, bit 15 the right one.
        assert_eq!(get_button_name(14), "Page Left");
        assert_eq!(get_button_name(15), "Page Right");
        assert_eq!(get_button_index("Page Left"), Some(14));
        assert_eq!(get_button_index("page right"), Some(15));
    }

    #[test]
    fn the_names_the_inherited_table_used_still_resolve() {
        // Mapping files of the original tool say Mute and Solo; they must keep
        // working even though the table now names those bits differently.
        assert_eq!(get_button_index("Mute"), Some(14));
        assert_eq!(get_button_index("Solo"), Some(15));
        assert_eq!(get_button_index("Mut"), Some(14));

        // ... and the aliases must not shadow a real name: the alias is needed
        // precisely because no table entry answers to it any more.
        for (old, _) in BUTTON_ALIASES {
            for entry in BUTTON_NAMES {
                assert!(
                    !starts_with_ignore_case(entry, old),
                    "the table now has `{entry}`, which answers to the alias `{old}`"
                );
            }
        }
    }
}
