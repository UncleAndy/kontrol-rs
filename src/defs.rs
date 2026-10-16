//! Names, versions and paths, mirroring `defs.h` / `version.h` of the C tool.

/// Also used as the ALSA sequencer client/port name.
pub const KOMPLEMENT_NAME: &str = "Komplement";
pub const KONFIGURE_NAME: &str = "Konfigure";

pub const KOMPLEMENT_VERSION: &str = "0.1alpha8";
pub const KONFIGURE_VERSION: &str = "0.1alpha6";

pub const INVALID_KEY_OR_BUTTON: &str = "**Invalid**";

pub const ALSA_CLIENT_NAME: &str = "KOMPLEMENTARY";
pub const ALSA_PORT_NAME: &str = "MIDI OUT";

pub const DEFAULT_UINPUT_PATH: &str = "/dev/uinput";

pub const DEFAULT_VID: i32 = 0x17cc;
pub const DEFAULT_PID: i32 = 0x1730;

/// How many bytes are read from the HID device at a time (the A-series report
/// is 30 bytes, a larger buffer costs nothing and lets `-d` show everything).
pub const READ_BUFFER_SZ: usize = 64;

pub const RISK_DISCLAIMER: &str = "** DISCLAIMER: YOU USE THIS AT YOUR OWN RISK **\n\
I am not responsible if this utility screws things up. I did my best to test this\n\
thoroughly, but everything makes mistakes sometimes.\n\n";

/// Where mapping files are looked up when the path as given does not exist.
/// `None` in a plain `cargo build`; the flake/nix build sets it to the installed
/// `share/kontrol-rs/mappings` directory.
pub const MAPPINGS_PATH: Option<&str> = option_env!("KONTROL_MAPPINGS_PATH");

/// Same for the preset files of `konfigure`.
pub const PRESETS_PATH: Option<&str> = option_env!("KONTROL_PRESETS_PATH");
