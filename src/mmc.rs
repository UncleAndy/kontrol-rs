//! MMC commands that can be assigned in a mapping file, mirroring `mmc_stuff.c`.
//!
//! They are not keystrokes: they go out over the ALSA sequencer port as MIDI
//! Machine Control SysEx, so a DAW that listens to MMC reacts to them.

use crate::defs::INVALID_KEY_OR_BUTTON;

pub const MMC_STOP: u8 = 0x01;
pub const MMC_PLAY: u8 = 0x02;
pub const MMC_FORWARD: u8 = 0x04;
pub const MMC_REWIND: u8 = 0x05;
pub const MMC_RECORD_STROBE: u8 = 0x06;
pub const MMC_RECORD_EXIT: u8 = 0x07;
pub const MMC_RECORD_PAUSE: u8 = 0x08;

/// Indexed by command value; holes are commands without a name.
static MMC_NAMES: [Option<&str>; 9] = [
    None,
    Some("MMC_Stop"),
    Some("MMC_Play"),
    None,
    Some("MMC_Forward"),
    Some("MMC_Rewind"),
    Some("MMC_Record_Strobe"),
    Some("MMC_Record_Exit"),
    Some("MMC_Record_Pause"),
];

/// Prefix match, as in the C tool (`strncasecmp` with the length of `name`).
pub fn mmc_key_parse(name: &str) -> Option<u8> {
    for (command, entry) in MMC_NAMES.iter().enumerate() {
        if let Some(entry) = entry {
            if entry.len() >= name.len() && entry[..name.len()].eq_ignore_ascii_case(name) {
                return Some(command as u8);
            }
        }
    }
    None
}

pub fn mmc_key_name(command: i32) -> &'static str {
    if command > 0 && (command as usize) < MMC_NAMES.len() {
        if let Some(name) = MMC_NAMES[command as usize] {
            return name;
        }
    }
    INVALID_KEY_OR_BUTTON
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_names() {
        assert_eq!(mmc_key_parse("MMC_Play"), Some(MMC_PLAY));
        assert_eq!(mmc_key_parse("mmc_rewind"), Some(MMC_REWIND));
        assert_eq!(mmc_key_parse("MMC_Nope"), None);
        assert_eq!(mmc_key_name(i32::from(MMC_STOP)), "MMC_Stop");
        assert_eq!(mmc_key_name(0), INVALID_KEY_OR_BUTTON);
        assert_eq!(mmc_key_name(3), INVALID_KEY_OR_BUTTON);
    }
}
