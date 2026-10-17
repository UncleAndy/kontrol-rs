//! The tool's own settings file: what the command line did not say.
//!
//! Not to be confused with [`crate::config`], which parses the *mapping* file
//! (`Play=Space`). This module reads the settings of the program itself.
//!
//! One `key = value` per line, `#` starts a comment, values may be quoted so a
//! path can hold a space. Every setting is optional, and *what is not said here*
//! is left to the command line, then to discovery, then to the built-in default
//! - in that order, which is the order the owner asked for.
//!
//! Two rules follow from being a service rather than a one-shot command:
//!
//! * a **missing** file is not an error (a fresh install runs on the defaults),
//!   but an unreadable one *is* reported: a service that silently ignores the
//!   settings it was told to use is worse than one that refuses to start;
//! * an unknown key, or a value that does not parse, is reported and skipped
//!   rather than swallowed - a typo must not look like a working config.

use std::path::{Path, PathBuf};

/// Where the settings live when nobody says otherwise.
pub const DIRECTORY: &str = "komplement";
pub const FILE_NAME: &str = "config";

/// The settings of the program itself. `None` means "not said here".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    /// USB vendor id of the keyboard, in hex (`17cc`).
    pub vid: Option<u16>,
    /// USB product id of the keyboard, in hex (`1740` for an A49).
    pub pid: Option<u16>,
    /// The uinput device the keystrokes are injected through.
    pub uinput: Option<String>,
    /// The mapping file, in the sense of [`crate::config`].
    pub mapping: Option<String>,
    /// Where the DAW's control surface writes: an ALSA address (`14:0`) or a
    /// piece of its name (`Midi Through`). What discovery falls back to when it
    /// cannot decide, and what `-X` uses when its own argument finds nothing.
    pub mcu: Option<String>,
    /// Which knob does what: `knob1 = volume` moves the current track's level
    /// over MCU, `knob1 = none` leaves the knobs read but silent.
    pub knob1: Option<String>,
    /// How much of the full travel one detent of the first knob is worth, as a
    /// fraction: `knob1_step = 0.05` is twenty detents across the whole travel.
    pub knob1_step: Option<f32>,
    /// The same with SHIFT held: `knob1_step_shift = 0.005` is two hundred
    /// detents across the whole travel, for setting a level to the decibel.
    pub knob1_step_shift: Option<f32>,
}

impl Settings {
    /// `$XDG_CONFIG_HOME/komplement/config`, or `~/.config/komplement/config`,
    /// or `None` when there is no home to put it in.
    pub fn path() -> Option<PathBuf> {
        let base = match std::env::var_os("XDG_CONFIG_HOME") {
            Some(directory) if !directory.is_empty() => PathBuf::from(directory),
            _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
        };
        Some(base.join(DIRECTORY).join(FILE_NAME))
    }

    /// Whether the first knob drives the current track's level.
    ///
    /// On unless the settings say otherwise: a knob that does nothing is the
    /// surprise, not one that moves a fader - and `knob1 = none` is how a user
    /// says "read them, but say nothing".
    pub fn knob1_volume(&self) -> bool {
        match self.knob1.as_deref() {
            Some(value) => !matches!(
                value.trim().to_lowercase().as_str(),
                "none" | "off" | "false" | "no"
            ),
            None => true,
        }
    }

    /// How much of the full travel one detent of the first knob is worth.
    ///
    /// Anything a hand-written file can say is checked here rather than trusted:
    /// a fraction of nothing, a negative number, a fraction above the whole
    /// travel, or a number that is not a number at all falls back to the
    /// measured default instead of moving a fader by an infinity.
    pub fn knob1_step(&self) -> f32 {
        match self.knob1_step {
            Some(step) if step.is_finite() && step > 0.0 && step <= 1.0 => step,
            _ => crate::knobs::DEFAULT_VOLUME_STEP,
        }
    }

    /// The same with SHIFT held on the keyboard. Checked the same way, and the
    /// default is finer than the plain step: with SHIFT the knob becomes a fine
    /// adjustment rather than doing nothing.
    pub fn knob1_step_shift(&self) -> f32 {
        match self.knob1_step_shift {
            Some(step) if step.is_finite() && step > 0.0 && step <= 1.0 => step,
            _ => crate::knobs::DEFAULT_SHIFT_STEP,
        }
    }

    /// Reads the settings, if the file is there.
    ///
    /// Returns the settings and whatever should be *said* about them: the
    /// reasons a line was skipped, and an unreadable file. A file that is
    /// simply not there is silence, because that is what a fresh install is.
    pub fn load(path: &Path) -> (Settings, Vec<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => Settings::parse(&text),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                (Settings::default(), Vec::new())
            }
            Err(err) => (Settings::default(), vec![format!("{}: {err}", path.display())]),
        }
    }

    /// Reads the settings, and says so when the file the user *named* is not
    /// there: an explicit path that does not exist is a mistake, not a default.
    pub fn load_named(path: &Path) -> Result<(Settings, Vec<String>), String> {
        if !path.exists() {
            return Err(format!("{}: no such file", path.display()));
        }
        Ok(Settings::load(path))
    }

    /// The parser, on its own so the rules above are testable without a file.
    pub fn parse(text: &str) -> (Settings, Vec<String>) {
        let mut settings = Settings::default();
        let mut notes = Vec::new();

        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let Some((key, value)) = split_setting(line) else {
                notes.push(format!(
                    "line {}: {line:?} is neither a setting nor a comment",
                    number + 1
                ));
                continue;
            };
            let value = unquote(value);
            if value.is_empty() {
                notes.push(format!("line {}: {key:?} has no value", number + 1));
                continue;
            }

            match key.to_lowercase().as_str() {
                "vid" => match parse_id(value) {
                    Some(id) => settings.vid = Some(id),
                    None => notes.push(not_a_number(&key, value)),
                },
                "pid" => match parse_id(value) {
                    Some(id) => settings.pid = Some(id),
                    None => notes.push(not_a_number(&key, value)),
                },
                "uinput" => settings.uinput = Some(value.to_string()),
                "mapping" => settings.mapping = Some(value.to_string()),
                "mcu" => settings.mcu = Some(value.to_string()),
                "knob1" => settings.knob1 = Some(value.to_string()),
                "knob1_step" => match value.trim().parse::<f32>() {
                    Ok(step) => settings.knob1_step = Some(step),
                    Err(_) => notes.push(not_a_number(&key, value)),
                },
                "knob1_step_shift" => match value.trim().parse::<f32>() {
                    Ok(step) => settings.knob1_step_shift = Some(step),
                    Err(_) => notes.push(not_a_number(&key, value)),
                },
                other => notes.push(format!("{other:?} is not a setting this tool knows; ignored")),
            }
        }

        (settings, notes)
    }
}

/// `key = value` or `key value`; the key is the first word.
fn split_setting(line: &str) -> Option<(&str, &str)> {
    let (key, value) = match line.split_once('=') {
        Some((key, value)) => (key, value),
        None => {
            let mut words = line.splitn(2, char::is_whitespace);
            (words.next()?, words.next()?)
        }
    };
    let key = key.trim();
    let value = value.trim();
    if key.is_empty() {
        return None;
    }
    Some((key, value))
}

/// One layer of quotes, so `mapping = "/path with space/reaper.map"` works.
fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
            return &value[1..value.len() - 1];
        }
    }
    value
}

/// The USB ids are written the way the command line writes them: hex, with the
/// `0x` optional. Decimal is *not* accepted: `-p 1740` has always meant
/// `0x1740`, and a file that read it as a decimal would be a trap.
fn parse_id(value: &str) -> Option<u16> {
    u16::from_str_radix(
        value.trim_start_matches("0x").trim_start_matches("0X"),
        16,
    )
    .ok()
}

fn not_a_number(key: &str, value: &str) -> String {
    format!("{key:?}: {value:?} is not a hex number; using the default")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_knob_moves_the_level_unless_the_settings_say_no() {
        // On by default: a knob that does nothing is the surprise, not one that
        // moves a fader.
        assert!(Settings::default().knob1_volume());
        assert!(Settings::parse("knob1 = volume\n").0.knob1_volume());
        assert!(!Settings::parse("knob1 = none\n").0.knob1_volume());
        assert!(!Settings::parse("knob1 = OFF\n").0.knob1_volume());
        // The step is a setting too, and anything unusable falls back rather
        // than being obeyed: no zero, no negatives, nothing above the travel.
        assert!((Settings::parse("knob1_step = 0.05\n").0.knob1_step() - 0.05).abs() < 1e-6);
        assert_eq!(Settings::default().knob1_step(), crate::knobs::DEFAULT_VOLUME_STEP);
        assert_eq!(
            Settings::parse("knob1_step = 0\n").0.knob1_step(),
            crate::knobs::DEFAULT_VOLUME_STEP
        );
        assert_eq!(
            Settings::parse("knob1_step = 7\n").0.knob1_step(),
            crate::knobs::DEFAULT_VOLUME_STEP
        );
        // And the SHIFT step, which defaults to the fine hand rather than to
        // doing nothing at all.
        assert_eq!(
            Settings::default().knob1_step_shift(),
            crate::knobs::DEFAULT_SHIFT_STEP
        );
        assert!((Settings::parse("knob1_step_shift = 0.001\n").0.knob1_step_shift() - 0.001).abs() < 1e-7);
        assert_eq!(
            Settings::parse("knob1_step_shift = -1\n").0.knob1_step_shift(),
            crate::knobs::DEFAULT_SHIFT_STEP
        );
        assert_eq!(
            crate::knobs::volume_step(true, 0.02, 0.005),
            0.005,
            "SHIFT is the fine hand"
        );
        assert_eq!(crate::knobs::volume_step(false, 0.02, 0.005), 0.02);
    }

    #[test]
    fn every_setting_is_read() {
        let (settings, notes) = Settings::parse(
            "# komplement\n\
             vid = 17cc\n\
             pid = 0x1740\n\
             uinput = /dev/uinput\n\
             mapping = mappings/reaper.map\n\
             mcu = Midi Through\n",
        );

        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(
            settings,
            Settings {
                vid: Some(0x17cc),
                pid: Some(0x1740),
                uinput: Some("/dev/uinput".to_string()),
                mapping: Some("mappings/reaper.map".to_string()),
                mcu: Some("Midi Through".to_string()),
                knob1: None,
                knob1_step: None,
                knob1_step_shift: None,
            }
        );
    }

    #[test]
    fn comments_blanks_and_colons_instead_of_equals_are_all_fine() {
        let (settings, notes) = Settings::parse("\n   # a comment, indented\n\npid 1740\nmcu = 14:0\n");

        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(settings.pid, Some(0x1740));
        // A colon is a port address, not a setting separator.
        assert_eq!(settings.mcu, Some("14:0".to_string()));
    }

    #[test]
    fn a_path_with_a_space_survives_quotes() {
        let (settings, notes) = Settings::parse("mapping = \"/home/andy/my mapping/reaper.map\"\n");
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(
            settings.mapping,
            Some("/home/andy/my mapping/reaper.map".to_string())
        );
    }

    #[test]
    fn what_cannot_be_used_is_said_and_skipped() {
        let (settings, notes) = Settings::parse(
            "vid = zz\n\
             pid =\n\
             nonsense = 1\n\
             bareword\n",
        );

        // The defaults stay in place and every reason is reported.
        assert_eq!(settings, Settings::default());
        assert_eq!(notes.len(), 4, "{notes:?}");
        assert!(notes[0].contains("vid"));
        assert!(notes[1].contains("no value"));
        assert!(notes[2].contains("nonsense"));
        assert!(notes[3].contains("comment"));
    }

    #[test]
    fn a_missing_file_is_not_a_problem_but_an_unreadable_one_is() {
        let missing = Path::new("/nonexistent/komplement/config");
        let (settings, notes) = Settings::load(missing);
        assert_eq!(settings, Settings::default());
        assert!(notes.is_empty(), "silence is right for the default path");

        // A path the user *named* and that is not there is a mistake, though.
        let message = Settings::load_named(missing).unwrap_err();
        assert!(message.contains("no such file"), "{message}");
    }

    #[test]
    fn the_search_path_follows_the_xdg_rules() {
        // The path is built from the environment, so this test only checks the
        // shape it must have on any machine that has a home at all.
        if let Some(path) = Settings::path() {
            assert!(path.ends_with("komplement/config"), "{}", path.display());
        }
    }
}
