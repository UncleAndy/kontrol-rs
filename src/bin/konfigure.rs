//! `konfigure` - generates the SysEx that configures the buttons and knobs of a
//! Komplete Kontrol A-series keyboard. Rust port of `konfigure.c`.

use std::io::Write;
use std::process::exit;

use alsa::rawmidi::Rawmidi;
use alsa::Direction;

use kontrol_rs::defs::{KOMPLEMENT_NAME, KONFIGURE_NAME, KONFIGURE_VERSION, RISK_DISCLAIMER};
use kontrol_rs::preset::{build_sysex, parse_file, NAME_SZ};
use kontrol_rs::GetOpt;

fn print_usage(argv0: &str) {
    // Flush left on purpose: line continuations would strip the indentation.
    println!(
        "{KONFIGURE_NAME} v{KONFIGURE_VERSION} by @hvangalen@mastodon.nl

This utility generates SysEx to configure Komplete Kontrol A-series keyboards.

Usage: {argv0} <options> -p path/to/preset.pst

Options:
 -p <path>    Path to preset definition.
 -o <path>    Write SysEx to this file in stead of standard out.
 -m <port>    Write SysEx directly to this ALSA MIDI port.
 -q           Be less verbose.

{RISK_DISCLAIMER}"
    );
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let argv0 = arguments
        .first()
        .cloned()
        .unwrap_or_else(|| KOMPLEMENT_NAME.to_string());

    let mut verbose = true;
    let mut preset_path: Option<String> = None;
    let mut output_path: Option<String> = None;
    let mut midi_port: Option<String> = None;

    let mut options = GetOpt::new(
        arguments.iter().map(String::as_str).collect(),
        "p:o:m:hq",
    );
    while let Some((option, value)) = options.next_option() {
        match option {
            'h' => {
                print_usage(&argv0);
                return;
            }
            'p' => preset_path = Some(value),
            'o' => output_path = Some(value),
            'm' => midi_port = Some(value),
            'q' => verbose = false,
            other => {
                eprintln!("Unknown option: -{other}");
                print_usage(&argv0);
                exit(1);
            }
        }
    }

    let Some(preset_path) = preset_path else {
        print_usage(&argv0);
        println!("ERROR: The -p <preset> option is required.");
        exit(1);
    };

    let (preset, _assigned) = match parse_file(&preset_path, verbose) {
        Ok(result) => result,
        Err(message) => {
            println!("{message}");
            println!("ERROR: The preset file could not be parsed.");
            exit(2);
        }
    };

    if verbose {
        // One line per assignment, in the order they are written.
        for page in 0..4 {
            for button in 0..8 {
                let index = page * 8 + button;
                let entry = &preset.buttons[index];
                println!(
                    "Button {}: {} type={} chan={:2x} {:02x} {:02x} {:02x} {:02x}",
                    index,
                    preset.button_label(index),
                    entry.kind,
                    entry.channel,
                    entry.data[0],
                    entry.data[1],
                    entry.data[2],
                    entry.data[3]
                );
            }
        }
    }

    let sysex = build_sysex(&preset);
    debug_assert_eq!(sysex.len(), 13 + NAME_SZ + 32 * 6 + 32 * NAME_SZ + 1);

    // Output: a file, an ALSA MIDI port, or standard out.
    if let Some(port_name) = midi_port {
        let port = match Rawmidi::new(&port_name, Direction::Playback, false) {
            Ok(port) => port,
            Err(err) => {
                println!("ERROR: The MIDI port {port_name} could not be opened: {err}");
                exit(10);
            }
        };

        if let Err(err) = port.write(&sysex) {
            println!("Failed to send MIDI to device. {err}");
            exit(11);
        }
        return;
    }

    if let Some(path) = output_path {
        match std::fs::File::create(&path) {
            Ok(mut file) => {
                if let Err(err) = file.write_all(&sysex) {
                    println!("Failed to write data to file. {err}");
                    exit(3);
                }
            }
            Err(err) => {
                println!("The output file `{path}` could not be opened: {err}");
                exit(3);
            }
        }
        return;
    }

    let mut stdout = std::io::stdout();
    if let Err(err) = stdout.write_all(&sysex) {
        println!("Failed to write data to standard out. {err}");
        exit(3);
    }
    let _ = stdout.flush();
}
