//! Preset files of `konfigure` and the SysEx they produce.
//! Mirrors `konfigure_parser.c` and the SysEx layout of `konfigure.c`.

use std::path::PathBuf;

use crate::defs::{KOMPLEMENT_NAME, PRESETS_PATH};

/// Length of a name / button label on the device.
pub const NAME_SZ: usize = 10;
/// Pages (4) times buttons per page (8).
pub const MAX_PRESET_BUTTONS: usize = 4 * 8;

pub const TYPE_CC: u8 = 0x00;
pub const TYPE_PGM: u8 = 0x01;

/// The fixed SysEx header of a button assignment packet.
pub const LEAD_IN: [u8; 13] = [
    0xf0, 0x00, 0x21, 0x09, 0x30, 0x17, 0x4d, 0x43, 0x01, 0x00, 0x01, 0x0a, 0x04,
];
pub const SYSEX_END: u8 = 0xf7;

/// What the device accepts for `presets/basic_cc.pst`, and what the tests use
/// as the expected length.
pub const BUTTON_SYSEX_SZ: usize = 536;

#[derive(Clone, Copy)]
pub struct PresetButton {
    pub assigned: bool,
    /// Space padded, exactly as stored in the SysEx.
    pub label: [u8; NAME_SZ],
    pub kind: u8,
    pub channel: u8,
    pub data: [u8; 4],
}

impl Default for PresetButton {
    fn default() -> Self {
        PresetButton {
            assigned: false,
            label: [b' '; NAME_SZ],
            kind: 0,
            channel: 0,
            data: [0; 4],
        }
    }
}

pub struct Preset {
    pub label: [u8; NAME_SZ],
    pub buttons: [PresetButton; MAX_PRESET_BUTTONS],
}

impl Default for Preset {
    fn default() -> Self {
        Preset {
            // The original puts in a default name so it can be used when the
            // file has no `name=` line.
            label: *b"Unnamed   ",
            buttons: [PresetButton::default(); MAX_PRESET_BUTTONS],
        }
    }
}

impl Preset {
    pub fn label_text(&self) -> String {
        String::from_utf8_lossy(&self.label).into_owned()
    }

    pub fn button_label(&self, index: usize) -> String {
        String::from_utf8_lossy(&self.buttons[index].label)
            .trim_end()
            .to_string()
    }
}

fn resolve_preset_path(filename: &str) -> Option<PathBuf> {
    let direct = PathBuf::from(filename);
    if direct.is_file() {
        return Some(direct);
    }

    if let Some(dir) = PRESETS_PATH {
        let candidate = PathBuf::from(dir).join(filename);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

/// Reads a preset file. Returns the preset and how many buttons were assigned.
pub fn parse_file(path: &str, verbose: bool) -> Result<(Preset, usize), String> {
    let resolved =
        resolve_preset_path(path).ok_or_else(|| format!("The file `{path}` could not be found."))?;

    if verbose {
        println!("Parsing `{}`...", resolved.display());
    }

    let content =
        std::fs::read(&resolved).map_err(|err| format!("The file `{path}` could not be opened. Check the permissions. ({err})"))?;

    let mut preset = Preset::default();
    let mut assigned_total = 0usize;

    for (index, line) in content.split(|byte| *byte == b'\n').enumerate() {
        let line_number = index + 1;

        if starts_with_ignore_case(line, b"name=") {
            let name = trim_end(&line[5..]);
            let length = name.len().min(NAME_SZ);
            preset.label[..length].copy_from_slice(&name[..length]);
            if verbose {
                println!("Name `{}`", preset.label_text());
            }
            continue;
        }

        if !starts_with_ignore_case(line, b"button") {
            continue;
        }

        let rest = &line[6..];
        let (Some(equals), Some(slash)) = (find(rest, b'='), find(rest, b'/')) else {
            continue;
        };

        let page = atoi_like(&line[6..]);
        let button = atoi_like(&slash[1..]);
        let index = page * 8 + button;

        if index > 31 {
            println!("Line #{line_number}: The button {page} / {button} is out of range.");
            continue;
        }
        if index < 0 {
            continue;
        }
        let index = index as usize;

        let righthand = trim_end(&equals[1..]);
        if verbose {
            println!(
                "Page {page}, button {button}, index={index}, `{}`",
                String::from_utf8_lossy(righthand)
            );
        }

        if righthand.is_empty() {
            if verbose {
                println!("Button{page}/{button} is not assigned.");
            }
            preset.buttons[index].assigned = false;
            continue;
        }

        if preset.buttons[index].assigned {
            println!("Line #{line_number}: Warning, the button {page} / {button} is being re-assigned.");
        }

        match parse_button_line(&mut preset.buttons[index], righthand, verbose) {
            Ok(()) => {
                preset.buttons[index].assigned = true;
                assigned_total += 1;
            }
            Err(message) => println!("Line #{line_number}: {message}"),
        }
    }

    Ok((preset, assigned_total))
}

/// Parses the part after the `=` sign: `label,type,channel,d0,d1,d2,d3`.
fn parse_button_line(button: &mut PresetButton, line: &[u8], verbose: bool) -> Result<(), String> {
    *button = PresetButton::default();

    if line.is_empty() {
        return Ok(());
    }

    let mut phase = 0usize;
    let mut start = 0usize;
    let mut position = 0usize;

    while position <= line.len() {
        let separator = position == line.len() || line[position] == b',';
        let mut got_token = false;

        if separator {
            let token = &line[start..position];
            let value = atoi_like(token);

            if phase > 0 && value > 127 {
                return Err("A value is out of range.".to_string());
            }

            match phase {
                0 => {
                    let length = token.len().min(NAME_SZ);
                    button.label[..length].copy_from_slice(&token[..length]);
                    got_token = true;
                }
                1 => {
                    button.kind = value as u8;
                    got_token = true;
                }
                2 => {
                    button.channel = value as u8;
                    got_token = true;
                }
                3..=6 => {
                    button.data[phase - 3] = value as u8;
                    got_token = true;
                }
                _ => {}
            }
        }

        position += 1;

        if got_token {
            start = position;
            // The original wraps around, so a ninth field would overwrite the
            // label again; kept for faithfulness.
            phase = if phase == 6 { 0 } else { phase + 1 };
        }
    }

    button.assigned = true;

    if verbose {
        println!(
            "Button LABEL={}, TYPE={}, CHANNEL={}, DATA={:02x} {:02x} {:02x} {:02x}",
            button_label_text(button),
            button.kind,
            button.channel,
            button.data[0],
            button.data[1],
            button.data[2],
            button.data[3]
        );
    }

    Ok(())
}

fn button_label_text(button: &PresetButton) -> String {
    String::from_utf8_lossy(&button.label).trim_end().to_string()
}

/// The 536 byte SysEx: header, template name, 32 button assignments, 32 labels,
/// end-of-exclusive.
///
/// The labels are written in the original's unusual order (button index
/// `button + page * 4`), which is *not* the order the assignments use - the
/// device shows them correctly only this way.
pub fn build_sysex(preset: &Preset) -> Vec<u8> {
    let mut out = Vec::with_capacity(BUTTON_SYSEX_SZ);

    out.extend_from_slice(&LEAD_IN);
    out.extend_from_slice(&preset.label);

    for page in 0..4 {
        for button in 0..8 {
            let button = &preset.buttons[page * 8 + button];
            out.push(button.kind);
            out.push(button.channel);
            out.extend_from_slice(&button.data);
        }
    }

    for button in 0..8 {
        for page in 0..4 {
            out.extend_from_slice(&preset.buttons[button + page * 4].label);
        }
    }

    out.push(SYSEX_END);
    out
}

fn starts_with_ignore_case(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len() && haystack[..needle.len()].eq_ignore_ascii_case(needle)
}

fn find(haystack: &[u8], needle: u8) -> Option<&[u8]> {
    haystack
        .iter()
        .position(|byte| *byte == needle)
        .map(|index| &haystack[index..])
}

/// Strips newlines, carriage returns and spaces from the end (the original's
/// `__cleanup_text`).
fn trim_end(text: &[u8]) -> &[u8] {
    let mut end = text.len();
    while end > 0 && matches!(text[end - 1], b'\n' | b'\r' | b' ') {
        end -= 1;
    }
    &text[..end]
}

/// `atoi()` semantics: optional spaces and sign, then digits.
fn atoi_like(text: &[u8]) -> i32 {
    let mut index = 0usize;
    while index < text.len() && (text[index] as char).is_ascii_whitespace() {
        index += 1;
    }

    let mut negative = false;
    if index < text.len() && (text[index] == b'-' || text[index] == b'+') {
        negative = text[index] == b'-';
        index += 1;
    }

    let mut value: i32 = 0;
    while index < text.len() && text[index].is_ascii_digit() {
        value = value.saturating_mul(10).saturating_add(i32::from(text[index] - b'0'));
        index += 1;
    }

    if negative {
        -value
    } else {
        value
    }
}

/// The ALSA client name used by the sequencer port of `komplement`.
pub const _CLIENT_NAME_HINT: &str = KOMPLEMENT_NAME;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_preset_generates_536_bytes() {
        let (preset, assigned) = parse_file("../komplementary-kontrol/presets/basic_cc.pst", false)
            .or_else(|_| parse_file("presets/basic_cc.pst", false))
            .expect("basic_cc.pst should be found");
        assert_eq!(assigned, 32);
        assert_eq!(preset.label_text(), "Basic CC  ");
        let sysex = build_sysex(&preset);
        assert_eq!(sysex.len(), BUTTON_SYSEX_SZ);
        assert_eq!(sysex[0], 0xf0);
        assert_eq!(sysex[LEAD_IN.len()..LEAD_IN.len() + NAME_SZ], preset.label);
        assert_eq!(*sysex.last().unwrap(), SYSEX_END);
        // First assignment of page 0: label Vol(CC7), type CC, channel 0, CC 7.
        let first = LEAD_IN.len() + NAME_SZ;
        assert_eq!(sysex[first], TYPE_CC);
        assert_eq!(sysex[first + 1], 0);
        assert_eq!(sysex[first + 2], 7);

        // The labels are written in the original's `button + page * 4` order,
        // not in the order of the assignments.
        let labels = first + 32 * 6;
        let label = |index: usize| {
            String::from_utf8_lossy(&sysex[labels + index * NAME_SZ..labels + (index + 1) * NAME_SZ])
                .trim_end()
                .to_string()
        };
        assert_eq!(label(0), "Vol(CC7)"); // position 0 -> buttons[0]
        assert_eq!(label(1), "CC22"); // position 1 -> buttons[4] (page 0, button 4)
        assert_eq!(label(4), "Pan(CC10)"); // position 4 -> buttons[1]
    }

    #[test]
    fn atoi_matches_the_c_behaviour() {
        assert_eq!(atoi_like(b"12"), 12);
        assert_eq!(atoi_like(b" 7"), 7);
        assert_eq!(atoi_like(b"7abc"), 7);
        assert_eq!(atoi_like(b"abc"), 0);
        assert_eq!(atoi_like(b""), 0);
    }

    #[test]
    fn unassigned_buttons_are_spaces() {
        let preset = Preset::default();
        let sysex = build_sysex(&preset);
        assert_eq!(sysex.len(), BUTTON_SYSEX_SZ);
        // Everything after the header and name is zero bytes or spaces.
        assert!(sysex[LEAD_IN.len() + NAME_SZ..LEAD_IN.len() + NAME_SZ + 192]
            .iter()
            .all(|byte| *byte == 0));
        assert!(sysex[LEAD_IN.len() + NAME_SZ + 192..sysex.len() - 1]
            .iter()
            .all(|byte| *byte == b' '));
    }

    #[test]
    fn out_of_range_values_are_rejected() {
        let mut button = PresetButton::default();
        assert!(parse_button_line(&mut button, b"X,0,0,200,0,0,127", false).is_err());
        assert!(parse_button_line(&mut button, b"X,0,0,7,0,0,127", false).is_ok());
        assert_eq!(button.data[0], 7);
    }
}
