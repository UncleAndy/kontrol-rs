//! `komplement` - turns the non-MIDI controls of a Komplete Kontrol A-series
//! keyboard into keystrokes (and MMC messages), so they become usable in your
//! software of choice. Rust port of `komplement.c`.

use std::path::Path;
use std::process::exit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use kontrol_rs::alsa_seq::MmcPort;
use kontrol_rs::config::config_read;
use kontrol_rs::defs::{
    DEFAULT_PID, DEFAULT_UINPUT_PATH, DEFAULT_VID, KOMPLEMENT_NAME, KOMPLEMENT_VERSION,
    READ_BUFFER_SZ, RISK_DISCLAIMER,
};
use kontrol_rs::engine::Engine;
use kontrol_rs::hid::Hid;
use kontrol_rs::hid_dump::Dump;
use kontrol_rs::leds::Leds;
use kontrol_rs::mapping::MappingTable;
use kontrol_rs::outputs::{Keys, Outputs};
use kontrol_rs::settings::Settings;
use kontrol_rs::studio::{Page, Size};
use kontrol_rs::uinput::Keyboard;
use kontrol_rs::GetOpt;

/// Set from the signal handler; the main loop stops when it becomes true.
static ABORTED: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_interrupt(_signal: libc::c_int) {
    ABORTED.store(true, Ordering::SeqCst);
}

/// The options the parser accepts, in `getopt` notation. The usage text and the
/// parser must agree; a test below checks that they do.
const SHORT_OPTIONS: &str = "hv:p:m:o:c:qnsxdhalS:X:T:";

/// Whether this run has to name a mapping file.
///
/// Only the keystroke half reads one, and a packet dump (`-d`) is not that half:
/// it reads the keyboard and prints what it reports, which is a complete job -
/// and it is the very tool used to find out what a control sends. Demanding a
/// mapping there turns the command that discovers new controls into one that
/// cannot be run until they are already known.
fn mapping_needed(dump_packets: bool) -> bool {
    !dump_packets
}

fn print_usage(argv0: &str) {
    println!("{}", usage_text(argv0));
}

fn usage_text(argv0: &str) -> String {
    // Written flush left on purpose: a line continuation (`\` at the end of a
    // source line) would strip the leading spaces that indent the options.
    format!(
        "{KOMPLEMENT_NAME} v{KOMPLEMENT_VERSION} by @hvangalen@mastodon.nl

This utility converts the non-MIDI keys on the Komplete Kontrol A-series keyboards
to keyboard presses, so they will become usable in your software of choice.

Usage: {argv0} <options>

Options:
 -h                   Print this help and exit.

 -o /path/to/uinput   The path to uinput device file ({DEFAULT_UINPUT_PATH}).
 -m /path/to/mapping  The path to mapping file (required to be useful).
                      In the -X mode a mapping makes it the merged mode: the
                      panel and the keystrokes run together in one program, on
                      one HID device.
 -c /path/to/settings The settings file to read. Without -c,
                      $XDG_CONFIG_HOME/komplement/config (usually
                      ~/.config/komplement/config) is read if it is there; an
                      absent default file is normal, but a path given with -c
                      must exist. The file holds one `key = value` per line:
                      vid, pid, uinput, mapping and mcu (the port the DAW's
                      surface writes to). What it says is used where the command
                      line said nothing.
 -a                   Do not create ALSA MIDI output port for MMC messages.
 -n                   Do not animate the buttons when starting/stopping.

 -q                   Be less verbose.

 -d                   Dump HID packets as they change (for mapping
                      the buttons and rotaries that are not handled yet).
                      Needs no -m: such a run only reads the keyboard.

 -x                   Take the display over for the run (`a0 03 04`, what the
                      screen daemon will do) and give it back at the end. Use it
                      with -d to see what a control reports while the display is
                      ours rather than Native Instruments'.

 -l                   List the HID devices the system exposes and exit
                      (works without the keyboard attached).

 -s                   Probe the keyboard's display: shows coarse states (whole
                      screen, halves, stripes, each with its opposite) and asks
                      yes/no questions about them, so the layout is measured
                      instead of guessed. The keyboard's own display logic is off
                      while the probe runs and is restored at its end.

 -S <state>           Show one named state on the display and wait, so it can be
                      looked at or photographed: on, off, top, bottom, left,
                      right, vstripes, hstripes, border, message, nav, navmed,
                      navmid, navbig, glyphs, text60, waiting, and the four live pages
                      page-transport, page-track, page-tracks, page-general -
                      each of those with :small, :medium or :large after it.

 -X <port>            Draw what a DAW reports over Mackie Control Universal,
                      live. <port> names the port its control surface writes to,
                      as an address (14:0) or a piece of its name (\"Midi Through\",
                      quote it); -X auto looks for it instead, and so does a name
                      that matches nothing. The search takes the only port that
                      can be the surface (never ours, never the keyboard's own
                      MIDI port); when several could be it, the settings file
                      decides (mcu = ...), and failing that the list of ports is
                      printed to choose from. The display is taken over for the
                      run and given back
                      on the way out; nothing is ever sent to the DAW. With a
                      mapping (-m) the keystrokes run at the same time: both
                      halves in one program on one HID device, keys first, and
                      whichever half fails leaves the other running. With
                      -S page-track:large the page and the glyph size are chosen
                      (default: page-track at medium). The two page buttons turn
                      the page while it runs, and with SHIFT held down they change
                      the glyph size instead. Ctrl+C stops it. See
                      doc/display-interface.md.

 -T <format>          How to read the DAW's time display in -X. The row carries
                      no punctuation Reaper actually sends, so the format has to
                      be named the way the DAW names it: h:m:s:f for that format
                      (measured) or written for the row exactly as it arrived
                      (the default). See doc/display-interface.md.

Advanced options:
 -p <productId>       USB product ID (in case you want to try other hardware).
 -v <vendorId>        USB vendor ID (in case you want to try other hardware).

Without -v/-p, and with nothing in the settings file, the keyboard is found by
itself: the only device of the family this tool knows (the A-series ids above),
or the first one when several are attached - which is then said out loud,
because taking the first of several is a guess. Whatever it picks is reported,
so a model this list does not know yet is one `pid = ...` line away.

{RISK_DISCLAIMER}"
    )
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let argv0 = arguments
        .first()
        .cloned()
        .unwrap_or_else(|| KOMPLEMENT_NAME.to_string());

    let mut vid = DEFAULT_VID;
    let mut pid = DEFAULT_PID;
    let mut vid_given = false;
    let mut pid_given = false;
    let mut animate = true;
    let mut quiet = false;
    let mut midi_controller = true;
    let mut dump_packets = false;
    let mut screen_takeover = false;
    let mut uinput_path = DEFAULT_UINPUT_PATH.to_string();
    let mut uinput_given = false;
    let mut mapping_path: Option<String> = None;
    let mut settings_path: Option<String> = None;
    let mut list_devices = false;
    let mut screen_probe = false;
    let mut screen_state: Option<String> = None;
    let mut mcu_input: Option<String> = None;
    let mut time_layout = kontrol_rs::timecode::TimeLayout::AsWritten;
    let mut parsed_options = 0usize;

    let mut options = GetOpt::new(
        arguments.iter().map(String::as_str).collect(),
        SHORT_OPTIONS,
    );
    while let Some((option, value)) = options.next_option() {
        parsed_options += 1;
        match option {
            'h' => {
                print_usage(&argv0);
                return;
            }
            'v' => {
                vid = i32::from_str_radix(value.trim_start_matches("0x"), 16).unwrap_or(vid);
                vid_given = true;
            }
            'p' => {
                pid = i32::from_str_radix(value.trim_start_matches("0x"), 16).unwrap_or(pid);
                pid_given = true;
            }
            'm' => mapping_path = Some(value),
            'o' => {
                uinput_path = value;
                uinput_given = true;
            }
            'c' => settings_path = Some(value),
            'q' => quiet = true,
            'd' => dump_packets = true,
            'x' => screen_takeover = true,
            'n' => animate = false,
            'a' => midi_controller = false,
            'l' => list_devices = true,
            's' => screen_probe = true,
            'S' => screen_state = Some(value),
            'X' => mcu_input = Some(value),
            'T' => {
                match kontrol_rs::timecode::TimeLayout::from_name(&value) {
                    Some(layout) => time_layout = layout,
                    None => {
                        eprintln!("Unknown time display format: {value}");
                        eprintln!("Use -T h:m:s:f for Reaper's hour format, or -T written for the row as it arrives.");
                        exit(1);
                    }
                }
            }
            other => {
                eprintln!("Unknown option: -{other}");
                print_usage(&argv0);
                exit(1);
            }
        }
    }

    if parsed_options == 0 {
        print_usage(&argv0);
        exit(1);
    }

    // The settings file: what the command line did not say. A path named with
    // -c has to be there; the default path being absent is a fresh install.
    let (settings, settings_notes) = match settings_path.as_deref() {
        Some(path) => match Settings::load_named(Path::new(path)) {
            Ok(read) => read,
            Err(message) => {
                eprintln!("The settings file {message}");
                exit(1);
            }
        },
        None => match Settings::path() {
            Some(path) => Settings::load(&path),
            None => (Settings::default(), Vec::new()),
        },
    };
    for note in &settings_notes {
        eprintln!("Settings: {note}");
    }

    // The settings fill in what the command line left alone - never the other
    // way round, so a flag always beats the file.
    if !vid_given {
        if let Some(id) = settings.vid {
            vid = i32::from(id);
        }
    }
    if !pid_given {
        if let Some(id) = settings.pid {
            pid = i32::from(id);
        }
    }
    if !uinput_given {
        if let Some(path) = &settings.uinput {
            uinput_path = path.clone();
        }
    }
    if mapping_path.is_none() {
        mapping_path = settings.mapping.clone();
    }

    let mut return_code = 0;

    // Listing devices is a stand-alone mode: no mapping file, no keyboard.
    if list_devices {
        match kontrol_rs::hid::list_devices() {
            Ok(devices) => print!(
                "{}",
                kontrol_rs::hid::format_device_list(&devices, vid as u16, pid as u16)
            ),
            Err(reason) => println!("Could not list HID devices: {reason}"),
        }
        return;
    }

    // Which keyboard: the command line, then the settings file, then the only
    // device of the family this tool knows - and when that last step is a guess
    // between several, the note says so, because a choice nobody can see is a
    // choice nobody can correct. The ids it settles on are what every open below
    // uses, so nothing else has to know how they were found.
    let named_pair = (vid_given || pid_given).then(|| (vid as u16, pid as u16));
    let configured_pair =
        (settings.vid.is_some() || settings.pid.is_some()).then(|| (vid as u16, pid as u16));
    let mut keyboard: Option<kontrol_rs::hid::DeviceSummary> = None;
    match kontrol_rs::hid::list_devices() {
        Ok(devices) => match kontrol_rs::presence::choose_keyboard(
            &devices,
            named_pair,
            configured_pair,
            DEFAULT_VID as u16,
        ) {
            Ok((device, _source, notes)) => {
                vid = i32::from(device.vendor_id);
                pid = i32::from(device.product_id);
                for note in &notes {
                    eprintln!("Keyboard: {note}");
                }
                keyboard = Some(device);
            }
            Err(message) => {
                eprintln!("{message}");
                exit(1);
            }
        },
        // Enumeration itself failed: the open below will report it too.
        Err(reason) => eprintln!("Could not list HID devices: {reason}"),
    }
    let keyboard_name = keyboard
        .as_ref()
        .and_then(|device| device.product.clone());

    // `-X`: draw what a DAW reports over MCU, live, on the panel. Stands alone
    // like the other display modes - no mapping file, no keystrokes - and takes
    // the display over for the run. This is the decoder seen on hardware; the
    // daemon (step 4 of doc/display-interface.md) is what keeps it.
    if let Some(port_argument) = mcu_input {
        let (page, size) = kontrol_rs::live::page_and_size(
            screen_state.as_deref().unwrap_or(""),
            Page::Track,
            Size::Medium,
        );
        // The port is settled before the keyboard is even opened, so a name that
        // finds nothing cannot disturb the panel, and so the search happens
        // where its notes and its error are still readable.
        let chosen = match kontrol_rs::mcu_port::ports().and_then(|ports| {
            kontrol_rs::presence::choose_daw_port(
                &ports,
                Some(&port_argument),
                settings.mcu.as_deref(),
                keyboard_name.as_deref(),
            )
        }) {
            Ok((port, _source, notes)) => {
                for note in &notes {
                    eprintln!("DAW port: {note}");
                }
                port
            }
            Err(message) => {
                println!("{message}");
                exit(1);
            }
        };

        // The merged mode. With a mapping file this is one program with both
        // halves - the panel *and* the keystrokes, on this single HID handle,
        // which is the merge the whole daemon plan leads to. Without `-m` it
        // stays what it has been: the decoder's view on the panel.
        let mut table = MappingTable::new();
        let mut mapping_given = false;
        if let Some(path) = mapping_path.as_deref() {
            match config_read(path, !quiet, &mut table) {
                Ok(()) => mapping_given = true,
                Err(err) => {
                    match err {
                        kontrol_rs::config::ConfigError::NotFound => println!(
                            "The file `{path}` could not be found in the current path, nor in the mappings folder."
                        ),
                        kontrol_rs::config::ConfigError::Io(message) => {
                            println!("The file `{path}` could not be opened: {message}")
                        }
                    }
                    println!("The mapping file could not be read.");
                    exit(2);
                }
            }
        }

        match Hid::open(vid as u16, pid as u16) {
            Ok(hid) => {
                let input = match kontrol_rs::mcu_port::McuInput::open_chosen(&chosen) {
                    Ok(input) => input,
                    Err(message) => {
                        println!("{message}");
                        exit(1);
                    }
                };

                // The keystroke half, when there is a mapping: the virtual
                // keyboard, the LED picture and the MMC port. A virtual keyboard
                // that cannot be opened is *not* fatal here - the panel is what
                // this mode is for, and it goes on working without one - which is
                // the other half of "keys first, panel second": whatever is
                // missing, the rest keeps running.
                let mut virtual_keyboard = match Keyboard::open() {
                    Ok(keyboard) => Some(keyboard),
                    Err(err) => {
                        println!("uinput open: {err}");
                        println!(
                            "Keystrokes cannot be sent, so only the panel runs. `{}` has to be read *and* written (see `ls -l /dev/uinput`).",
                            kontrol_rs::uinput::UINPUT_PATH
                        );
                        None
                    }
                };
                let mmc = if midi_controller {
                    match kontrol_rs::alsa_seq::MmcPort::open(None) {
                        Ok(port) => Some(port),
                        Err(message) => {
                            println!("Error opening the ALSA port for MMC. {message}");
                            None
                        }
                    }
                } else {
                    None
                };
                let mut leds = Leds::new();
                leds.init();
                let keys = match (mapping_given, virtual_keyboard.as_mut()) {
                    (true, Some(keyboard)) => Some(Keys::new(
                        Engine::new(table),
                        &mut leds,
                        keyboard,
                        mmc.as_ref(),
                        animate,
                    )),
                    _ => None,
                };
                if mapping_given && keys.is_none() && !quiet {
                    println!("The mapping was read, but there is no virtual keyboard to send it to.");
                }

                // MIDI mode first, then the panel: the takeover on its own left
                // the keyboard out of MIDI mode for the whole run and took the
                // knobs' CC with it.
                for report in kontrol_rs::screen::TAKEOVER {
                    let _ = hid.write_raw(report);
                }
                // The handle is handed over: the loop may have to open the
                // keyboard again after it is unplugged, and it gives the display
                // back to the keyboard itself on the way out, whichever handle
                // that is by then.
                let outcome =
                    kontrol_rs::live::run(hid, input, page, size, quiet, time_layout, keys);
                if let Err(message) = outcome {
                    println!("{message}");
                    exit(1);
                }
            }
            Err(reason) => {
                println!("Opening the HID device {vid:04x}:{pid:04x} failed: {reason}");
                println!("Use -l to see which devices are there (-p 1740 for the A49).");
                exit(1);
            }
        }
        return;
    }

    // One named state on the display, to look at or photograph.
    if let Some(name) = screen_state {
        match Hid::open(vid as u16, pid as u16) {
            Ok(hid) => {
                if let Err(message) = kontrol_rs::screen::run_state(&hid, &name) {
                    println!("{message}");
                    println!("{}", kontrol_rs::screen::state_help());
                    exit(1);
                }
            }
            Err(reason) => {
                println!("Opening the HID device {vid:04x}:{pid:04x} failed: {reason}");
                println!("Use -l to see which devices are there (-p 1740 for the A49).");
                exit(1);
            }
        }
        return;
    }

    // The screen probe stands alone as well, but it does need the keyboard: it
    // takes the display over and puts it back at the end.
    if screen_probe {
        match Hid::open(vid as u16, pid as u16) {
            Ok(hid) => kontrol_rs::screen::run_probe(&hid),
            Err(reason) => {
                println!("Opening the HID device {vid:04x}:{pid:04x} failed: {reason}");
                println!("Use -l to see which devices are there (-p 1740 for the A49).");
                exit(1);
            }
        }
        return;
    }

    // A mapping is what the plain mode is *for* - keystrokes - so it is required
    // unless the run is a packet dump: reading the keyboard and printing what it
    // reports is a complete job without one, and it is the tool used to find out
    // what a control sends in the first place.
    let mapping_path = match mapping_path {
        Some(path) => Some(path),
        None if !mapping_needed(dump_packets) => None,
        None => {
            print_usage(&argv0);
            println!("ERROR: The -m <mapping> option is required.");
            exit(1);
        }
    };

    // Install the interrupt handlers before touching the device.
    unsafe {
        libc::signal(libc::SIGINT, handle_interrupt as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, handle_interrupt as *const () as libc::sighandler_t);
        libc::signal(libc::SIGQUIT, handle_interrupt as *const () as libc::sighandler_t);
    }

    // Read the mapping file: this doubles as the "is the file usable?" check.
    let mut table = MappingTable::new();
    if let Some(mapping_path) = mapping_path.as_deref() {
        if let Err(err) = config_read(mapping_path, !quiet, &mut table) {
            match err {
                kontrol_rs::config::ConfigError::NotFound => println!(
                    "The file `{mapping_path}` could not be found in the current path, nor in the mappings folder."
                ),
                kontrol_rs::config::ConfigError::Io(message) => {
                    println!("The file `{mapping_path}` could not be opened: {message}")
                }
            }
            println!("The mapping file could not be read.");
            exit(2);
        }
    }

    // Open the HID device. The reason comes from hidapi/libusb, so a failure
    // (no such device / permission / device busy) is readable here instead of
    // being guessed at.
    let hid = match Hid::open(vid as u16, pid as u16) {
        Ok(hid) => hid,
        Err(reason) => {
            println!("Opening the HID device {vid:04x}:{pid:04x} failed: {reason}");

            // Enumeration needs no permission, so this says whether the
            // keyboard is there at all or is merely not openable.
            match Hid::enumerate_matching(vid as u16, pid as u16) {
                Ok((lines, _)) if lines.is_empty() => println!(
                    "The HID layer does not see a single device with vendor id {vid:04x}: check the\n\
cable and the port, and that the keyboard is in MIDI mode."
                ),
                Ok((lines, present)) => {
                    println!("Devices with vendor id {vid:04x} that the HID layer sees:");
                    for line in lines {
                        println!("  {line}");
                    }
                    if present {
                        println!(
                            "The keyboard is present but could not be opened: it is most likely still\n\
held by another process (a running komplement/konfigure) or by the kernel driver."
                        );
                    } else {
                        println!(
                            "None of them has product id {pid:04x}: pass the right -p productId\n\
(1730 = A25, 1740 = A49)."
                        );
                    }
                }
                Err(err) => println!("(could not list HID devices: {err})"),
            }

            println!(
                "Also check that the current user is allowed to read the device node under\n\
/dev/bus/usb (group and mode)."
            );
            exit(0);
        }
    };

    // This resets the HID device back to "MIDI Mode".
    let _ = hid.write_raw(&[0xa0, 0x07, 0x00]);

    // `-x`: take the display over, the way the screen daemon will. Controls can
    // report differently depending on who owns the display, so this is what
    // makes that measurable.
    if screen_takeover {
        let _ = hid.write_raw(&[0xa0, 0x03, 0x04]);
    }

    let mut leds = Leds::new();
    leds.init();
    if animate {
        leds.animate_on(&hid);
    }

    let mut engine = Engine::new(table);

    // Lights the LEDs and, in doing so, checks that the device answers. The
    // keystroke sink is built per turn below, because it borrows the handle the
    // loop reads from and that handle is replaced when the keyboard comes back
    // after an unplug.
    {
        let mut outputs = Outputs::for_turn(&hid, &mut leds, None, None);
        engine.light_up_initial(&mut outputs, animate);
        if let Some(reason) = outputs.failed.clone() {
            println!(
                "The HID device {vid:04x}:{pid:04x} was opened, but writing the LED state to it failed: {reason}"
            );
            println!(
                "Reading works while writing does not; check whether something else holds the device\n\
(another komplement/konfigure instance, or the kernel driver)."
            );
            exit(return_code);
        }
    }

    // ALSA output port for MMC messages.
    let mmc = if midi_controller {
        match MmcPort::open(None) {
            Ok(port) => {
                if !quiet {
                    println!("ALSA output port created.");
                }
                Some(port)
            }
            Err(message) => {
                println!("Error opening ALSA port. {message}");
                return_code = 10;
                leds.off(&hid);
                exit(return_code);
            }
        }
    } else {
        None
    };

    // The virtual keyboard.
    if uinput_path != DEFAULT_UINPUT_PATH {
        println!(
            "Note: -o {uinput_path} is ignored; this port always creates the virtual keyboard at {}.",
            kontrol_rs::uinput::UINPUT_PATH
        );
    }
    let mut keyboard = match Keyboard::open() {
        Ok(keyboard) => keyboard,
        Err(err) => {
            println!("uinput open: {err}");
            println!(
                "Could not open {} (this port always uses that path; -o is accepted for\n\
compatibility only). Can it be read *and written* by the current user?",
                kontrol_rs::uinput::UINPUT_PATH
            );
            if err.kind() == std::io::ErrorKind::PermissionDenied {
                println!(
                    "Permission denied: check `ls -l /dev/uinput` (expect crw-rw---- root audio,\n\
with the current user in that group) and `getfacl /dev/uinput`. An ACL entry overrides the\n\
group rights - `user:andy:-w-`, which an earlier `setfacl -m u:andy:w` leaves behind, grants\n\
write but not read and makes the read-write open fail. Remove the ACLs with\n\
`sudo setfacl -b /dev/uinput`, and if you ever add one back, grant `rw`, not `w`."
                );
            }
            leds.off(&hid);
            exit(-2);
        }
    };
    let mut dump = Dump::new();
    // The knobs say nothing on the MIDI port while the display is taken over;
    // their movement is in the report instead, read by the same bytes whatever
    // mode this run is in.
    let mut knobs = kontrol_rs::knobs::Knobs::new(
        kontrol_rs::knobs::DEFAULT_STEP,
        kontrol_rs::knobs::DEFAULT_MODULUS,
    );
    let started = Instant::now();
    let mut buffer = [0u8; READ_BUFFER_SZ];
    let mut read_errors = 0;
    // The keyboard can be unplugged while this runs, and that is a state to wait
    // out rather than the end: the handle dies with the device, the USB ids do
    // not, so the ids are kept and the device is opened again when it is back.
    // A key left held down in another program would outlive the unplug, so
    // everything held is let go of first - that is what `release_all` is for.
    let mut device = Some(hid);
    let mut last_search = Instant::now();
    let mut reported_absent = false;

    while !ABORTED.load(Ordering::SeqCst) {
        let Some(current) = device.as_ref() else {
            std::thread::sleep(std::time::Duration::from_millis(
                kontrol_rs::live::KEYBOARD_RETRY_MILLIS,
            ));
            if last_search.elapsed()
                >= std::time::Duration::from_millis(kontrol_rs::live::KEYBOARD_RETRY_MILLIS)
            {
                last_search = Instant::now();
                match Hid::open(vid as u16, pid as u16) {
                    Ok(opened) => {
                        // Its LEDs went with it, so they are lit the way start-up
                        // does, and the display is taken over again.
                        {
                            let mut fresh =
                                Outputs::for_turn(&opened, &mut leds, Some(&mut keyboard), mmc.as_ref());
                            engine.light_up_initial(&mut fresh, false);
                        }
                        // A keyboard that comes back is in whatever mode it
                        // likes: MIDI mode is asserted again (the knobs' CC), and
                        // the display is taken over again only if this run took
                        // it over in the first place.
                        let _ = opened.write_raw(&kontrol_rs::screen::MIDI_MODE);
                        if screen_takeover {
                            let _ = opened.write_raw(&kontrol_rs::screen::DISABLE_LOCAL_CONTROLS);
                        }
                        if !quiet {
                            println!(
                                "The keyboard is back ({vid:04x}:{pid:04x}); reading it again."
                            );
                        }
                        device = Some(opened);
                        reported_absent = false;
                    }
                    Err(reason) => {
                        if !reported_absent {
                            reported_absent = true;
                            if !quiet {
                                println!(
                                    "Waiting for the keyboard ({reason}); the virtual keyboard stays as it is."
                                );
                            }
                        }
                    }
                }
            }
            continue;
        };

        // Built per turn: it borrows the current handle, and that handle is a
        // different one after the keyboard was unplugged and came back.
        let mut outputs = Outputs::for_turn(current, &mut leds, Some(&mut keyboard), mmc.as_ref());

        let read = match current.read_timeout(&mut buffer, 2000) {
            Ok(read) => read,
            Err(reason) => {
                // Whether it is gone is asked of the device list, not of hidapi:
                // its own text for this ("hid_error is not implemented yet") says
                // nothing, while the list needs no permission and answers.
                let gone = matches!(
                    Hid::enumerate_matching(vid as u16, pid as u16),
                    Ok((_, false))
                );
                if gone {
                    engine.release_all(&mut outputs);
                    device = None;
                    reported_absent = false;
                    if !quiet {
                        println!(
                            "The keyboard {vid:04x}:{pid:04x} went away: everything held was released, and it is looked for again every {} ms.",
                            kontrol_rs::live::KEYBOARD_RETRY_MILLIS
                        );
                    }
                    continue;
                }
                println!("Error reading HID device: {reason}");
                read_errors += 1;
                if read_errors > 10 {
                    println!("Too many read errors, aborting.");
                    break;
                }
                continue;
            }
        };
        if read == 0 {
            continue;
        }

        let read = read as usize;
        let packet = &buffer[..read];
        read_errors = 0;

        if dump_packets {
            let stamp_ms = started.elapsed().as_millis() as i64;
            dump.packet(packet, stamp_ms);
            kontrol_rs::knobs::announce(&knobs.read(packet));
        }

        engine.process(packet, &mut outputs);
    }

    // Clean up, on whichever handle is current: the keyboard may have been
    // unplugged and come back while this ran, and the handle it was opened with
    // is dead by then.
    if let Some(current) = device.as_ref() {
        if animate {
            leds.animate_off(current);
        } else {
            leds.off(current);
        }

        // Give the display back to the keyboard's own logic.
        if screen_takeover {
            let _ = current.write_raw(&[0xa0, 0x07, 0x00]);
        }
    }

    exit(return_code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_packet_dump_is_not_asked_for_a_mapping() {
        // The run that finds out what a control sends must not require the list
        // of what the controls do first.
        assert!(
            !mapping_needed(true),
            "-d reads the keyboard and prints what it says; no mapping is involved"
        );
        assert!(
            mapping_needed(false),
            "without -d there is nothing to send but keystrokes, which come from the mapping"
        );
    }

    /// The usage text and the parser are two lists of the same options, and an
    /// option that is documented but rejected - or the other way round - is a
    /// trap that only shows up on the hardware, one capture later. This walks
    /// the usage text and asks the parser about every option it mentions.
    #[test]
    fn every_documented_option_is_accepted_by_the_parser() {
        let usage = usage_text("komplement");
        let mut seen = 0usize;

        for line in usage.lines() {
            let Some(rest) = line.strip_prefix(" -") else {
                continue;
            };
            let letter = rest.chars().next().expect("an option letter after ` -`");

            assert!(
                SHORT_OPTIONS.contains(letter),
                "the usage documents -{letter}, which the parser rejects"
            );

            let arguments = ["komplement".to_string(), format!("-{letter}")];
            let mut options = GetOpt::new(
                arguments.iter().map(String::as_str).collect(),
                SHORT_OPTIONS,
            );
            assert_eq!(
                options.next_option().map(|(option, _)| option),
                Some(letter),
                "-{letter} is documented but the parser does not return it"
            );

            seen += 1;
        }

        assert!(
            seen >= 12,
            "only {seen} options were found in the usage text; the scan is wrong"
        );

        // And the other way round: an option the parser takes but the usage does
        // not mention is just as hidden as the reverse.
        for letter in SHORT_OPTIONS.chars().filter(|c| *c != ':') {
            assert!(
                usage.contains(&format!(" -{letter}")),
                "the parser accepts -{letter}, which the usage does not document"
            );
        }
    }
}
