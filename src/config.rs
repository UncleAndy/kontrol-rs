//! Parser for mapping files, mirroring `config.c` of the C tool.
//!
//! The file format is a line per control:
//!
//! ```text
//! # comment
//! Play=Space
//! Shift+Play=MMC_Play
//! 4D CW=LeftCtrl,LeftShift,Right
//! ```
//!
//! The parser is deliberately as forgiving (and as quirky) as the original one:
//! button names are matched by prefix, a `Shift+` prefix selects the second
//! layer, unknown keys are reported and skipped, and a missing trailing newline
//! still completes the last row.

use std::path::PathBuf;

use crate::buttons::{get_button_index, get_button_name};
use crate::defs::MAPPINGS_PATH;
use crate::keys::{key_name, key_parse};
use crate::mapping::{Layer, MappedKey, MappingKey, MappingTable};
use crate::mcu_buttons::{mcu_button_name, mcu_button_parse};
use crate::mmc::{mmc_key_name, mmc_key_parse};

#[derive(Debug)]
pub enum ConfigError {
    /// Neither the given path nor the installed mappings directory has it.
    NotFound,
    /// The file exists but could not be read.
    Io(String),
}

/// Finds a mapping file: the path as given first, then the installed
/// `share/.../mappings` directory (the equivalent of the C tool's
/// `MAPPINGS_PATH` fallback).
pub fn resolve_mapping_path(filename: &str) -> Option<PathBuf> {
    let direct = PathBuf::from(filename);
    if direct.is_file() {
        return Some(direct);
    }

    if let Some(dir) = MAPPINGS_PATH {
        let candidate = PathBuf::from(dir).join(filename);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

/// Reads a mapping file into `table`. `verbose` prints one line per assigned
/// row, which is also how the mapping files are checked before use.
pub fn config_read(filename: &str, verbose: bool, table: &mut MappingTable) -> Result<(), ConfigError> {
    let path = resolve_mapping_path(filename).ok_or(ConfigError::NotFound)?;
    let data = std::fs::read(&path).map_err(|err| ConfigError::Io(err.to_string()))?;

    let mut button_index: i32 = -1;
    let mut shifted = false;
    let mut comment = false;
    let mut line_counter = 1usize;
    let mut buffer: Vec<u8> = Vec::with_capacity(32);
    let mut mapping = MappingKey::new();

    let mut position = 0usize;
    loop {
        // At the end of the file the original pretends a newline followed when
        // it was in the middle of a row, so a file without a trailing newline
        // still gets its last mapping.
        let byte: Option<u8> = if position < data.len() {
            let byte = data[position];
            position += 1;
            Some(byte)
        } else if button_index > -1 {
            Some(b'\n')
        } else {
            None
        };

        let Some(byte) = byte else { break };

        if button_index == -1 {
            // Still reading the name of the control.
            if byte == b'\n' {
                line_counter += 1;
                comment = false;
                shifted = false;
                buffer.clear();
                continue;
            } else if !comment && buffer.is_empty() && byte == b' ' {
                println!("Bad formatting on line {line_counter}");
                comment = true;
                continue;
            } else if buffer.is_empty() && byte == b'#' {
                comment = true;
                continue;
            }

            if !comment {
                if byte == b'=' {
                    button_index = get_button_index(&as_text(&buffer))
                        .map(|index| index as i32)
                        .unwrap_or(-1);
                    buffer.clear();
                    continue;
                } else if byte == b'+' {
                    // "Shift+Something" is a mapping for the SHIFT layer.
                    if is_prefix_of("Shift", &buffer) {
                        shifted = true;
                        buffer.clear();
                        continue;
                    }
                }
            }

            buffer.push(byte);
            continue;
        }

        // The name is known, so this is the list of keys.
        if byte == b'+' || byte == b',' || byte == b'\n' {
            let name = as_text(&buffer);
            // Three kinds, tried in this order: a key of the virtual keyboard, an
            // MMC command, an MCU button. `mcu:` is what keeps the last two apart
            // - they share names like "play" and "stop" and mean different things,
            // so the MCU one is always spelled out.
            match (
                key_parse(&name),
                mmc_key_parse(&name),
                mcu_button_parse(&name),
            ) {
                (None, None, None) => {
                    println!("Failed to parse key `{name}` at line {line_counter}");
                }
                (None, Some(command), _) => {
                    push(&mut mapping, MappedKey::mmc(command), &name, line_counter);
                }
                (None, None, Some(note)) => {
                    push(&mut mapping, MappedKey::mcu(note), &name, line_counter);
                }
                (Some(code), _, _) => {
                    push(&mut mapping, MappedKey::key(code), &name, line_counter);
                }
            }

            if byte == b'\n' {
                let layer = if shifted { Layer::Shifted } else { Layer::Normal };
                table.set(button_index, layer, mapping);

                if verbose {
                    let prefix = if shifted { "Shift+" } else { "" };
                    let keys: Vec<&str> = mapping
                        .iter()
                        .map(|key| match key.kind {
                            crate::mapping::KeyKind::Key => key_name(key.key),
                            crate::mapping::KeyKind::Mmc => mmc_key_name(key.key),
                            crate::mapping::KeyKind::Mcu => mcu_button_name(key.key),
                        })
                        .collect();
                    println!(
                        "Button `{}{}` mapped to {}",
                        prefix,
                        get_button_name(button_index),
                        keys.join("+")
                    );
                }

                line_counter += 1;
                mapping = MappingKey::new();
                button_index = -1;
                shifted = false;
                comment = false;
            }

            buffer.clear();
            continue;
        }

        buffer.push(byte);
    }

    Ok(())
}

/// Adds a key, reporting the (safety) limit of the original's fixed array
/// instead of overflowing it.
fn push(mapping: &mut MappingKey, key: MappedKey, name: &str, line_counter: usize) {
    if !mapping.push(key) {
        println!("Line {line_counter}: more than 4 keys, ignoring `{name}`");
    }
}

fn as_text(buffer: &[u8]) -> String {
    String::from_utf8_lossy(buffer).into_owned()
}

/// True when `buffer` is a prefix of `text` (what `strncasecmp` with the
/// buffer's length does in the C tool).
fn is_prefix_of(text: &str, buffer: &[u8]) -> bool {
    text.len() >= buffer.len() && text.as_bytes()[..buffer.len()].eq_ignore_ascii_case(buffer)
}
