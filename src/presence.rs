//! Finding the things the tool needs: which keyboard, which port.
//!
//! Neither is stable enough to hard-code. A HID path ends in the USB bus and
//! port it happens to hang on (`...:0003:0002:00`), and an ALSA client number
//! changes with every session - the capture that this port was built from saw
//! Reaper on `14:0`, which means nothing tomorrow.
//!
//! So both are found, in the order the owner asked for: what was named on the
//! command line, then what the settings file says, then the only thing that can
//! be meant - and where that last step would be a guess, the guess is *said*,
//! because a service that quietly picks the wrong port is the worst of the
//! three. Nothing here touches hardware: both functions work on a list, which
//! is what makes the rules testable without a keyboard on the desk.

use crate::hid::DeviceSummary;
use crate::mcu_port::{Port, CLIENT_NAME};

/// The product ids this tool has evidence for - its own error message names
/// them (`1730 = A25, 1740 = A49`), and the C tool's default is the first of
/// them. A newer model is one line away in the settings file, which is why an
/// unsuccessful search lists what it did see instead of just failing.
pub const KNOWN_PRODUCT_IDS: [u16; 2] = [0x1730, 0x1740];

/// The word that means "not named on the command line" for `-X`, so the search
/// can be asked for on purpose rather than by passing an empty argument.
pub const AUTO: &str = "auto";

/// Where a choice came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Named on the command line.
    Named,
    /// Named in the settings file.
    Configured,
    /// The only thing that could be meant: no guessing involved.
    OnlyCandidate,
    /// The first of several. Whoever asked has to say so - this is a guess.
    FirstOfSeveral,
}

/// Which of the enumerated HID devices is the keyboard.
///
/// `named` is `-v`/`-p` (either of them means the pair is pinned), `configured`
/// is the settings file, and `vendor_id` is the family to look for when
/// neither says anything.
pub fn choose_keyboard(
    devices: &[DeviceSummary],
    named: Option<(u16, u16)>,
    configured: Option<(u16, u16)>,
    vendor_id: u16,
) -> Result<(DeviceSummary, Source, Vec<String>), String> {
    if let Some((vendor, product)) = named.or(configured) {
        let source = if named.is_some() {
            Source::Named
        } else {
            Source::Configured
        };
        return find(devices, vendor, product)
            .map(|device| (device, source, Vec::new()))
            .ok_or_else(|| not_attached(vendor, product, devices));
    }

    // Nothing was named. One keyboard shows up as several HID interfaces with
    // the same ids, so the candidates are deduplicated by ids: several
    // *interfaces* are not several *keyboards*, and hidapi picks the interface
    // when it opens by ids anyway.
    let mut families: Vec<(u16, u16)> = Vec::new();
    for device in devices {
        if device.vendor_id == vendor_id && KNOWN_PRODUCT_IDS.contains(&device.product_id) {
            if !families.contains(&(device.vendor_id, device.product_id)) {
                families.push((device.vendor_id, device.product_id));
            }
        }
    }

    let mut notes = Vec::new();
    let (source, (vendor, product)) = match families.len() {
        0 => return Err(none_of_ours(vendor_id, devices)),
        1 => (Source::OnlyCandidate, families[0]),
        _ => {
            notes.push(format!(
                "more than one keyboard of this family is attached ({}); using the first, \
                 {} - set vid/pid in the settings file to be sure",
                families
                    .iter()
                    .map(|(vendor, product)| format!("{vendor:04x}:{product:04x}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                pair(families[0])
            ));
            (Source::FirstOfSeveral, families[0])
        }
    };

    let device = find(devices, vendor, product).expect("the family came from this list");
    notes.push(format!(
        "using {} ({}){}",
        pair((vendor, product)),
        describe(&device),
        if source == Source::OnlyCandidate {
            ", the only keyboard of this family the HID layer sees"
        } else {
            ""
        }
    ));

    Ok((device, source, notes))
}

/// Which ALSA port the DAW's control surface writes to.
///
/// The order is the owner's: the name `-X` was given, then discovery, then the
/// settings file. Discovery only ever accepts a *single* plausible port; a
/// working machine's port list holds ALSA's own `System`, the keyboard's own
/// MIDI ports and usually `Midi Through`, and choosing between several of those
/// by position would be a coin toss. What counts as plausible is measured
/// rather than assumed - the port has to be one a DAW could *write* into
/// (`Port::can_be_a_source`, from ALSA's own capabilities), so a capture tool's
/// port or another program's input is never offered, and the list the user has
/// to choose from says what each port is. When it cannot decide, the message
/// names the line that pins the answer instead.
pub fn choose_daw_port(
    ports: &[Port],
    named: Option<&str>,
    configured: Option<&str>,
    keyboard: Option<&str>,
) -> Result<(Port, Source, Vec<String>), String> {
    let mut notes = Vec::new();

    if let Some(asked) = named.map(str::trim).filter(|asked| !asked.is_empty()) {
        if !asked.eq_ignore_ascii_case(AUTO) {
            match crate::mcu_port::choose(asked, ports) {
                Ok(port) => return Ok((port, Source::Named, notes)),
                Err(_) => notes.push(format!(
                    "no port matches {asked:?} (neither an address nor a piece of a name); \
                     looking for one instead"
                )),
            }
        }
    }

    let candidates = plausible(ports, keyboard);
    if candidates.len() == 1 {
        let port = candidates[0].clone();
        notes.push(format!(
            "using {}, the only port that can be the DAW's control surface",
            port.describe()
        ));
        return Ok((port, Source::OnlyCandidate, notes));
    }

    if let Some(wanted) = configured.map(str::trim).filter(|wanted| !wanted.is_empty()) {
        match crate::mcu_port::choose(wanted, ports) {
            Ok(port) => return Ok((port, Source::Configured, notes)),
            Err(_) => notes.push(format!(
                "the settings file names {wanted:?} as the DAW's port, and no port matches it"
            )),
        }
    }

    if candidates.is_empty() {
        return Err(nothing_to_take(ports, keyboard));
    }

    Err(cannot_tell(&candidates, ports, keyboard, named, configured))
}

/// The ports that could be a DAW's control surface: not ours, not ALSA's own
/// `System` (the announce and timer clients), not the keyboard's own MIDI
/// ports - the surface's port is the one the DAW *writes* into, and the
/// keyboard's ports carry its own notes - and, measured from ALSA rather than
/// guessed, only ports something could actually write into. A capture tool's
/// port or another program's input can never be a surface's source, so offering
/// them would only make the list harder to read.
fn plausible(ports: &[Port], keyboard: Option<&str>) -> Vec<Port> {
    let own = CLIENT_NAME.to_lowercase();
    ports
        .iter()
        .filter(|port| {
            let client = port.client_name.to_lowercase();
            let name = port.port_name.to_lowercase();
            if client.contains(&own) || client.starts_with("system") || !port.can_be_a_source() {
                return false;
            }
            match keyboard.map(str::to_lowercase) {
                Some(keyboard) => !client.contains(&keyboard) && !name.contains(&keyboard),
                None => true,
            }
        })
        .cloned()
        .collect()
}

fn find(devices: &[DeviceSummary], vendor: u16, product: u16) -> Option<DeviceSummary> {
    devices
        .iter()
        .find(|device| device.vendor_id == vendor && device.product_id == product)
        .cloned()
}

fn pair((vendor, product): (u16, u16)) -> String {
    format!("{vendor:04x}:{product:04x}")
}

fn describe(device: &DeviceSummary) -> String {
    match &device.product {
        Some(product) => product.clone(),
        None => "(no name)".to_string(),
    }
}

fn not_attached(vendor: u16, product: u16, devices: &[DeviceSummary]) -> String {
    let mut message = format!(
        "{} is not attached: the HID layer does not see it.\n",
        pair((vendor, product))
    );
    let mut seen: Vec<String> = devices
        .iter()
        .filter(|device| device.vendor_id == vendor)
        .map(|device| format!("{} {}", pair((device.vendor_id, device.product_id)), describe(device)))
        .collect();
    seen.dedup();
    message += "Devices with this vendor id that it does see:\n";
    if seen.is_empty() {
        message += "  (none at all - check the cable and the port)\n";
    } else {
        for line in seen {
            message += &format!("  {line}\n");
        }
    }
    message + "Also check that the keyboard is in MIDI mode."
}

fn none_of_ours(vendor_id: u16, devices: &[DeviceSummary]) -> String {
    let mut message = format!(
        "no keyboard of this family is attached: none of the devices the HID layer sees has \
         vendor id {vendor_id:04x} together with one of the known product ids ({}).\n",
        KNOWN_PRODUCT_IDS
            .iter()
            .map(|product| format!("{product:04x}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut seen: Vec<String> = devices
        .iter()
        .filter(|device| device.vendor_id == vendor_id)
        .map(|device| format!("{} {}", pair((device.vendor_id, device.product_id)), describe(device)))
        .collect();
    seen.dedup();
    if seen.is_empty() {
        message += &format!("Nothing with vendor id {vendor_id:04x} is attached at all.\n");
    } else {
        message += "What it does see from this vendor:\n";
        for line in seen {
            message += &format!("  {line}\n");
        }
    }
    message + "A newer model works once its product id is set in the settings file (pid = ...)."
}

fn nothing_to_take(ports: &[Port], keyboard: Option<&str>) -> String {
    let mut message = String::from(
        "nothing in ALSA's port list can be the DAW's control surface: our own port, ALSA's own \
         clients and the keyboard's own MIDI ports are never candidates.\n",
    );
    if let Some(keyboard) = keyboard {
        message += &format!("({keyboard:?} is skipped: that is the keyboard's own MIDI port.)\n");
    }
    message += "Everything ALSA offers:\n";
    if ports.is_empty() {
        message += "  (nothing at all - is the ALSA sequencer running?)\n";
    }
    for port in ports {
        message += &format!("{}\n", listed(port));
    }
    message + "Is the DAW running, and does its control surface write to a port at all?\n\
               Pin the answer with one line in the settings file (komplement/config):\n  mcu = <name or address>"
}

/// One line of a list a person has to choose from: the port, and what ALSA says
/// it is, when it says anything at all.
fn listed(port: &Port) -> String {
    let kind = port.describe_kind();
    if kind.is_empty() {
        format!("  {}", port.describe())
    } else {
        format!("  {}  ({kind})", port.describe())
    }
}

fn cannot_tell(
    candidates: &[Port],
    ports: &[Port],
    keyboard: Option<&str>,
    named: Option<&str>,
    configured: Option<&str>,
) -> String {
    let mut message = String::from(
        "could not tell which port the DAW's control surface writes to: more than one port \
         could be it.\n",
    );
    if !candidates.is_empty() {
        message += "The candidates are:\n";
        for port in candidates {
            message += &format!("{}\n", listed(port));
        }
    }
    message += "Everything ALSA offers:\n";
    if ports.is_empty() {
        message += "  (nothing at all)\n";
    }
    for port in ports {
        message += &format!("{}\n", listed(port));
    }
    if let Some(named) = named.map(str::trim).filter(|named| !named.is_empty()) {
        message += &format!("-X was given {named:?}, which found nothing.\n");
    }
    if let Some(configured) = configured.map(str::trim).filter(|configured| !configured.is_empty()) {
        message += &format!("The settings file names {configured:?}, which found nothing.\n");
    }
    if let Some(keyboard) = keyboard {
        message += &format!("({keyboard:?} is skipped: that is the keyboard's own MIDI port.)\n");
    }
    message + "Pin the answer with one line in the settings file (komplement/config):\n  mcu = <name or address>\n\
               It is the port the surface *writes* to, not the keyboard's own MIDI port."
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcu_port::{PortCap, PortType};

    fn hid(vendor: u16, product: u16, product_name: &str, path: &str) -> DeviceSummary {
        DeviceSummary {
            vendor_id: vendor,
            product_id: product,
            product: Some(product_name.to_string()),
            manufacturer: Some("Native Instruments".to_string()),
            serial: None,
            interface: 0,
            path: path.to_string(),
        }
    }

    /// A port like one a DAW or a hardware interface offers: readable, and so
    /// able to be a source.
    fn port(client: i32, port: i32, client_name: &str, port_name: &str) -> Port {
        Port {
            client,
            port,
            client_name: client_name.to_string(),
            port_name: port_name.to_string(),
            capability: PortCap::READ | PortCap::SUBS_READ,
            kind: PortType::MIDI_GENERIC | PortType::HARDWARE,
        }
    }

    /// A port that is only a destination - what a capture tool's port or another
    /// program's input looks like. It can never be a surface's source.
    fn sink(client: i32, port: i32, client_name: &str, port_name: &str) -> Port {
        Port {
            client,
            port,
            client_name: client_name.to_string(),
            port_name: port_name.to_string(),
            capability: PortCap::WRITE | PortCap::SUBS_WRITE,
            kind: PortType::APPLICATION | PortType::SOFTWARE,
        }
    }

    fn a49() -> DeviceSummary {
        hid(0x17cc, 0x1740, "Komplete Kontrol A49", "/dev/hidraw3")
    }

    #[test]
    fn a_named_pair_is_used_as_it_is() {
        let devices = [hid(0x046d, 0xc52b, "USB Receiver", "/dev/hidraw0"), a49()];

        let (found, source, notes) =
            choose_keyboard(&devices, Some((0x17cc, 0x1740)), None, 0x17cc).unwrap();

        assert_eq!(found.product_id, 0x1740);
        assert_eq!(source, Source::Named);
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn the_settings_file_comes_after_the_command_line() {
        let devices = [a49(), hid(0x17cc, 0x1730, "Komplete Kontrol A25", "/dev/hidraw4")];

        let (found, source, _) =
            choose_keyboard(&devices, None, Some((0x17cc, 0x1730)), 0x17cc).unwrap();
        assert_eq!(found.product_id, 0x1730);
        assert_eq!(source, Source::Configured);

        // …and the command line still wins over it.
        let (found, source, _) = choose_keyboard(
            &devices,
            Some((0x17cc, 0x1740)),
            Some((0x17cc, 0x1730)),
            0x17cc,
        )
        .unwrap();
        assert_eq!(found.product_id, 0x1740);
        assert_eq!(source, Source::Named);
    }

    #[test]
    fn an_unnamed_keyboard_is_found_by_its_family() {
        let devices = [hid(0x046d, 0xc52b, "USB Receiver", "/dev/hidraw0"), a49()];

        let (found, source, notes) = choose_keyboard(&devices, None, None, 0x17cc).unwrap();

        assert_eq!(found.product_id, 0x1740);
        assert_eq!(source, Source::OnlyCandidate);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("Komplete Kontrol A49"), "{notes:?}");
        assert!(notes[0].contains("only keyboard"), "{notes:?}");
    }

    #[test]
    fn two_interfaces_of_one_keyboard_are_not_two_keyboards() {
        let devices = [
            hid(0x17cc, 0x1740, "Komplete Kontrol A49", "/dev/hidraw3"),
            hid(0x17cc, 0x1740, "Komplete Kontrol A49", "/dev/hidraw4"),
        ];

        let (_, source, notes) = choose_keyboard(&devices, None, None, 0x17cc).unwrap();

        assert_eq!(source, Source::OnlyCandidate, "{notes:?}");
    }

    #[test]
    fn the_first_of_several_is_taken_but_said_out_loud() {
        let devices = [a49(), hid(0x17cc, 0x1730, "Komplete Kontrol A25", "/dev/hidraw4")];

        let (found, source, notes) = choose_keyboard(&devices, None, None, 0x17cc).unwrap();

        assert_eq!(found.product_id, 0x1740);
        assert_eq!(source, Source::FirstOfSeveral);
        assert!(
            notes.iter().any(|note| note.contains("17cc:1730") && note.contains("17cc:1740")),
            "the note has to name both candidates: {notes:?}"
        );
    }

    #[test]
    fn nothing_to_find_says_what_was_seen_and_what_to_do() {
        // A device of the vendor but of an unknown product: the message has to
        // name it, because that is what the settings file needs.
        let devices = [hid(0x17cc, 0x9999, "Komplete Kontrol A61", "/dev/hidraw5")];

        let message = choose_keyboard(&devices, None, None, 0x17cc).unwrap_err();

        assert!(message.contains("17cc:9999"), "{message}");
        assert!(message.contains("Komplete Kontrol A61"), "{message}");
        assert!(message.contains("pid = ..."), "{message}");

        // Nothing of the vendor at all.
        let message = choose_keyboard(&[hid(0x046d, 0xc52b, "USB Receiver", "/dev/hidraw0")], None, None, 0x17cc)
            .unwrap_err();
        assert!(message.contains("Nothing with vendor id 17cc is attached"), "{message}");

        // A pinned pair that is not there.
        let message = choose_keyboard(&[a49()], Some((0x17cc, 0x1730)), None, 0x17cc).unwrap_err();
        assert!(message.contains("17cc:1730 is not attached"), "{message}");
        assert!(message.contains("17cc:1740"), "the list shows what there is: {message}");
    }

    #[test]
    fn a_named_port_is_used_as_it_is() {
        let ports = [
            port(14, 0, "Midi Through", "Midi Through Port-0"),
            port(20, 0, "Komplete Kontrol A49", "Komplete Kontrol A49 MIDI"),
            port(128, 0, CLIENT_NAME, "surface"),
        ];

        let (found, source, notes) =
            choose_daw_port(&ports, Some("14:0"), None, Some("Komplete Kontrol A49")).unwrap();

        assert_eq!(found.address(), "14:0");
        assert_eq!(source, Source::Named);
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn a_name_that_finds_nothing_falls_through_with_a_note() {
        let ports = [
            port(14, 0, "Midi Through", "Midi Through Port-0"),
            port(20, 0, "Komplete Kontrol A49", "Komplete Kontrol A49 MIDI"),
            port(128, 0, CLIENT_NAME, "surface"),
        ];

        let (found, source, notes) =
            choose_daw_port(&ports, Some("Reaper"), None, Some("Komplete Kontrol A49")).unwrap();

        // The only port that can be the surface, but the miss is not hidden.
        assert_eq!(found.client_name, "Midi Through");
        assert_eq!(source, Source::OnlyCandidate);
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes[0].contains("Reaper"), "{notes:?}");
    }

    #[test]
    fn the_word_auto_asks_for_the_search() {
        let ports = [port(14, 0, "Midi Through", "Midi Through Port-0")];

        let (found, source, _) = choose_daw_port(&ports, Some("auto"), None, None).unwrap();

        assert_eq!(found.address(), "14:0");
        assert_eq!(source, Source::OnlyCandidate);
    }

    #[test]
    fn the_keyboard_and_our_own_port_are_never_candidates() {
        let ports = [
            port(20, 0, "Komplete Kontrol A49", "Komplete Kontrol A49 MIDI"),
            port(128, 0, CLIENT_NAME, "surface"),
            port(0, 0, "System", "Timer"),
        ];

        let message = choose_daw_port(&ports, None, None, Some("Komplete Kontrol A49")).unwrap_err();

        let (message, list) = message
            .split_once("Everything ALSA offers:")
            .expect("the message lists everything ALSA offers");
        assert!(message.contains("nothing in ALSA's port list"), "{message}");
        assert!(
            !message.contains("Komplete Kontrol A49 MIDI"),
            "no candidate may be offered here: {message}"
        );
        assert!(list.contains("Komplete Kontrol A49 MIDI"), "the full list is still shown: {list}");
        assert!(list.contains(CLIENT_NAME), "ours is shown as well: {list}");
        assert!(list.contains("mcu = <name or address>"), "the pin hint stays: {list}");
    }

    #[test]
    fn a_port_that_cannot_be_a_source_is_never_a_candidate() {
        // The owner's machine, minus the keyboard: a `Midi Through` loopback, the
        // two inputs of a USB interface, a running capture tool and PipeWire's own
        // ports - the last three of which nothing can be written into by a DAW.
        let ports = [
            port(14, 0, "Midi Through", "Midi Through Port-0"),
            port(16, 0, "Steinberg UR22C", "Steinberg UR22C MIDI 1"),
            port(16, 1, "Steinberg UR22C", "Steinberg UR22C MIDI 2"),
            sink(128, 0, "aseqdump", "aseqdump"),
            sink(144, 0, "PipeWire-System", "input"),
            sink(145, 0, "PipeWire-RT-Event", "input"),
            port(20, 0, "KOMPLETE KONTROL A49", "KOMPLETE KONTROL A49 MIDI 1"),
        ];

        let message =
            choose_daw_port(&ports, Some("auto"), None, Some("KOMPLETE KONTROL A49")).unwrap_err();
        let (candidates, everything) = message
            .split_once("Everything ALSA offers:")
            .expect("the message lists everything ALSA offers");

        // Three ports a DAW could write into, and the keyboard is still not one.
        assert!(candidates.contains("Midi Through"), "{candidates}");
        assert!(candidates.contains("Steinberg UR22C MIDI 1"), "{candidates}");
        assert!(candidates.contains("Steinberg UR22C MIDI 2"), "{candidates}");
        assert!(
            !candidates.contains("aseqdump"),
            "a destination is no candidate: {candidates}"
        );
        assert!(
            !candidates.contains("PipeWire"),
            "nor is another program's input: {candidates}"
        );
        assert!(!candidates.contains("KOMPLETE KONTROL"), "{candidates}");

        // …while the full list still shows everything, and says what it is.
        assert!(everything.contains("aseqdump"), "{everything}");
        assert!(
            everything.contains("(writable, software, application)"),
            "the list has to say what a port can do: {everything}"
        );
        assert!(everything.contains("(readable, subscribable, hardware)"), "{everything}");

        // With nothing but destinations there is nothing to take at all, and the
        // message says that rather than "more than one".
        let only_sinks = [sink(128, 0, "aseqdump", "aseqdump")];
        let message = choose_daw_port(&only_sinks, None, None, None).unwrap_err();
        assert!(message.contains("nothing in ALSA's port list"), "{message}");
    }

    #[test]
    fn nothing_to_take_at_all_says_so_and_not_more_than_one_could_be() {
        let message =
            choose_daw_port(&[], None, None, Some("Komplete Kontrol A49")).unwrap_err();

        assert!(message.contains("nothing in ALSA's port list"), "{message}");
        assert!(message.contains("nothing at all"), "{message}");
        assert!(!message.contains("more than one port"), "{message}");
    }

    #[test]
    fn the_settings_file_decides_when_the_search_cannot() {
        let ports = [
            port(14, 0, "Midi Through", "Midi Through Port-0"),
            port(16, 0, "Loopback A", "Loopback A Port-0"),
        ];

        let (found, source, _) =
            choose_daw_port(&ports, None, Some("Loopback A"), None).unwrap();

        assert_eq!(found.address(), "16:0");
        assert_eq!(source, Source::Configured);
    }

    #[test]
    fn a_configured_port_that_is_missing_is_said_before_giving_up() {
        let ports = [
            port(14, 0, "Midi Through", "Midi Through Port-0"),
            port(16, 0, "Loopback A", "Loopback A Port-0"),
        ];

        let message = choose_daw_port(&ports, None, Some("Reaper"), None).unwrap_err();

        assert!(message.contains("names \"Reaper\""), "{message}");
        assert!(message.contains("which found nothing"), "{message}");
    }
}
