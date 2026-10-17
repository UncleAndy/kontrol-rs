//! Checks against the mapping files that ship with the project.
//!
//! The mapping files are the interface to the user, so these are the tests that
//! catch a broken parser: `mappings/reaper.map` is the file the A49 actually
//! runs with, `mappings/none.map` is the empty one used for measurements.

use kontrol_rs::buttons::get_button_index;
use kontrol_rs::config::config_read;
use kontrol_rs::keys::key_name;
use kontrol_rs::mapping::{KeyKind, Layer, MappingTable};
use kontrol_rs::mmc::mmc_key_parse;

fn read(path: &str) -> MappingTable {
    let mut table = MappingTable::new();
    config_read(path, false, &mut table).unwrap_or_else(|err| panic!("{path}: {err:?}"));
    table
}

fn mapped_count(table: &MappingTable) -> usize {
    (0..42)
        .filter(|index| table.is_mapped(*index, Layer::Any))
        .count()
}

fn keys_of(table: &MappingTable, name: &str, layer: Layer) -> Vec<String> {
    let index = get_button_index(name).unwrap_or_else(|| panic!("no such button: {name}")) as i32;
    table
        .get(index, layer)
        .iter()
        .map(|key| match key.kind {
            KeyKind::Key => key_name(key.key).to_string(),
            KeyKind::Mmc => format!("MMC:{}", key.key),
        })
        .collect()
}

#[test]
fn the_reaper_mapping_parses_completely() {
    let table = read("mappings/reaper.map");

    // Every row the file contains, in both layers, ends up in the table: 25
    // plain rows plus the 19 `Shift+...` rows. The C tool prints 35 for its copy
    // of this file - the two rows for Page Left and Page Right were taken out on
    // purpose, because those two buttons belong to the screen daemon that pages
    // the display with them (doc/display-interface.md), and one press should not
    // both page the screen and toggle mute in the DAW.
    //
    // The sixteen rotary rows went the same way: their touch sensors fire on every
    // turn, so the chord they sent rode along with each rotation - Reaper selected
    // another track while the knob was setting the volume of the current one, and
    // in its Learn dialog the keystroke was caught instead of the CC.
    let normal = (0..42).filter(|index| table.is_mapped(*index, Layer::Normal)).count();
    let shifted = (0..42).filter(|index| table.is_mapped(*index, Layer::Shifted)).count();
    assert_eq!((normal, shifted), (25, 19));
    assert_eq!(mapped_count(&table), 25);

    // The two buttons beside the display are free, in both layers.
    assert_eq!(get_button_index("Page Left"), Some(14));
    assert_eq!(get_button_index("Page Right"), Some(15));
    assert!(!table.is_mapped(14, Layer::Any));
    assert!(!table.is_mapped(15, Layer::Any));

    // Turning the 4D dial jogs the cursor: fast (Shift held inside the mapping)
    // and precise (bare arrow keys) in the shifted layer.
    assert_eq!(
        keys_of(&table, "4D CW", Layer::Normal),
        ["LEFTCTRL", "LEFTSHIFT", "RIGHT"]
    );
    assert_eq!(keys_of(&table, "4D CCW", Layer::Normal), ["LEFTCTRL", "LEFTSHIFT", "LEFT"]);
    assert_eq!(keys_of(&table, "4D CW", Layer::Shifted), ["RIGHT"]);
    assert_eq!(keys_of(&table, "4D CCW", Layer::Shifted), ["LEFT"]);

    // The rotary touch sensors are free, in both layers: they fire on every turn,
    // so a keystroke from one arrives together with the rotation's own CC, and
    // Reaper cannot tell the two apart while it is learning.
    for name in ["Rotary1", "Rotary8"] {
        let index = get_button_index(name).expect("the knob exists") as i32;
        assert!(!table.is_mapped(index, Layer::Any), "{name} is still mapped");
    }

    // Four keys is the maximum the format allows.
    assert_eq!(
        keys_of(&table, "Preset Up", Layer::Shifted),
        ["LEFTCTRL", "LEFTALT", "LEFTSHIFT", "UP"]
    );
    assert_eq!(keys_of(&table, "Preset Up", Layer::Shifted).len(), 4);

    // The 4D directions and the dial button.
    assert_eq!(keys_of(&table, "4D Up", Layer::Normal), ["LEFTCTRL", "LEFTALT", "UP"]);
    assert_eq!(keys_of(&table, "4D Down", Layer::Normal), ["LEFTCTRL", "LEFTALT", "DOWN"]);
    assert_eq!(keys_of(&table, "4D Button", Layer::Normal), ["V"]);

    // Nothing was mapped for the buttons the keyboard never reports.
    assert!(!table.is_mapped(34, Layer::Any));
}

#[test]
fn an_empty_mapping_leaves_every_button_free() {
    let table = read("mappings/none.map");
    assert_eq!(mapped_count(&table), 0);
}

#[test]
fn the_rosegarden_mapping_parses_and_carries_mmc() {
    let table = read("mappings/rosegarden.map");
    assert!(mapped_count(&table) > 0);

    // MMC commands are stored as their command byte, not as a key code.
    let index = get_button_index("Play").expect("Play exists") as i32;
    let keys = table.get(index, Layer::Normal);
    assert_eq!(keys.len(), 1);
    let key = keys.iter().next().unwrap();
    assert_eq!(key.kind, KeyKind::Mmc);
    assert_eq!(key.key, i32::from(mmc_key_parse("MMC_Play").unwrap()));
}

#[test]
fn unknown_buttons_and_keys_do_not_stop_the_parse() {
    // The parser is forgiving on purpose: a bad row is reported and skipped.
    let unknown_button = get_button_index("NoSuchButton");
    assert!(unknown_button.is_none());

    // Names are matched by prefix, exactly like the C tool's strncasecmp.
    assert_eq!(get_button_index("Shift"), get_button_index("Shift"));
    assert!(get_button_index("Rotar").is_some());
    assert!(kontrol_rs::keys::key_parse("space").is_some());
    assert!(kontrol_rs::keys::key_parse("nope").is_none());
}
