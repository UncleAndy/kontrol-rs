//! `kontrol-rs` - the Rust port of the `komplementary-kontrol` tools.
//!
//! Two binaries share this library:
//!
//! * [`komplement`](../komplement/index.html) - resident daemon that turns the
//!   non-MIDI controls of a Komplete Kontrol A-series keyboard into keystrokes
//!   (and MMC messages) for the DAW of your choice;
//! * [`konfigure`](../konfigure/index.html) - offline generator for the SysEx
//!   that configures the keyboard's knobs and buttons.
//!
//! The modules mirror the C sources one to one, so the two implementations can
//! be compared side by side:
//!
//! | Rust | C |
//! |---|---|
//! | [`buttons`] | `button_names.c` |
//! | [`keys`] | `uinput_stuff.c` (key table) |
//! | [`mmc`] | `mmc_stuff.c` |
//! | [`mapping`] | `mapping.c` |
//! | [`config`] | `config.c` |
//! | [`hid`] | `hid.c` |
//! | [`hid_dump`] | `hid_dump.c` |
//! | [`leds`] | `button_leds.c` |
//! | [`uinput`] | `uinput_stuff.c` (device) |
//! | [`alsa_seq`] | `alsa.c` |
//! | [`preset`] | `konfigure_parser.c` + the SysEx layout |
//!
//! New in the port, with no C counterpart:
//!
//! | Rust | what it is |
//! |---|---|
//! | [`settings`] | the tool's own settings file (`vid`, `pid`, `mcu`, ...) |
//! | [`presence`] | finding the keyboard and the DAW's port |
//! | [`mcu`], [`mcu_port`], [`studio`], [`pages`], [`timecode`] | the panel: the DAW's Mackie Control output, drawn on the keyboard's screen |
//! | [`live`] | the panel and the keyboard run together (`-X`) |

pub mod alsa_seq;
pub mod buttons;
pub mod config;
pub mod defs;
pub mod engine;
pub mod font;
pub mod font_medium;
pub mod hid;
pub mod hid_dump;
pub mod keys;
pub mod knobs;
pub mod leds;
pub mod live;
pub mod mapping;
pub mod mcu;
pub mod mcu_port;
pub mod mmc;
pub mod nav;
pub mod outputs;
pub mod pages;
pub mod panel;
pub mod preset;
pub mod presence;
pub mod screen;
pub mod settings;
pub mod studio;
pub mod timecode;
pub mod uinput;

/// Parses the options of `komplement` in the same way `getopt()` does in the C
/// tool: a value may follow its option directly (`-p1740`) or as the next
/// argument (`-p 1740`).
pub struct GetOpt<'a> {
    args: Vec<&'a str>,
    index: usize,
    short: String,
}

impl<'a> GetOpt<'a> {
    pub fn new(args: Vec<&'a str>, short_options: &str) -> Self {
        GetOpt {
            args,
            index: 1, // skip argv[0]
            short: short_options.to_string(),
        }
    }

    /// Returns `(option, value)`; `value` is empty for options that take none.
    /// `None` means the arguments are exhausted.
    pub fn next_option(&mut self) -> Option<(char, String)> {
        let argument = *self.args.get(self.index)?;
        self.index += 1;

        if !argument.starts_with('-') || argument == "-" {
            return Some(('?', argument.to_string()));
        }

        let mut chars = argument[1..].chars();
        let option = chars.next()?;
        let rest: String = chars.collect();

        if !self.short.contains(option) {
            return Some(('?', option.to_string()));
        }

        // Does this option want a value?
        let wants_value = self
            .short
            .find(option)
            .map(|position| self.short[position + 1..].starts_with(':'))
            .unwrap_or(false);

        if !wants_value {
            return Some((option, String::new()));
        }

        if !rest.is_empty() {
            return Some((option, rest));
        }

        let value = self.args.get(self.index).map(|value| value.to_string());
        self.index += 1;
        Some((option, value.unwrap_or_default()))
    }
}
