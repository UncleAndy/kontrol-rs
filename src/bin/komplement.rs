//! `komplement` - turns the non-MIDI controls of a Komplete Kontrol A-series
//! keyboard into keystrokes (and MMC messages), so they become usable in your
//! software of choice. Rust port of `komplement.c`.

use std::process::exit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use kontrol_rs::alsa_seq::MmcPort;
use kontrol_rs::config::config_read;
use kontrol_rs::defs::{
    DEFAULT_PID, DEFAULT_UINPUT_PATH, DEFAULT_VID, KOMPLEMENT_NAME, KOMPLEMENT_VERSION,
    READ_BUFFER_SZ, RISK_DISCLAIMER,
};
use kontrol_rs::engine::{Engine, Sink};
use kontrol_rs::hid::Hid;
use kontrol_rs::hid_dump::Dump;
use kontrol_rs::leds::Leds;
use kontrol_rs::mapping::MappingTable;
use kontrol_rs::uinput::Keyboard;
use kontrol_rs::GetOpt;

/// Set from the signal handler; the main loop stops when it becomes true.
static ABORTED: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_interrupt(_signal: libc::c_int) {
    ABORTED.store(true, Ordering::SeqCst);
}

fn print_usage(argv0: &str) {
    // Written flush left on purpose: a line continuation (`\` at the end of a
    // source line) would strip the leading spaces that indent the options.
    println!(
        "{KOMPLEMENT_NAME} v{KOMPLEMENT_VERSION} by @hvangalen@mastodon.nl

This utility converts the non-MIDI keys on the Komplete Kontrol A-series keyboards
to keyboard presses, so they will become usable in your software of choice.

Usage: {argv0} <options>

Options:
 -o /path/to/uinput   The path to uinput device file ({DEFAULT_UINPUT_PATH}).
 -m /path/to/mapping  The path to mapping file (required to be useful).
 -a                   Do not create ALSA MIDI output port for MMC messages.
 -n                   Do not animate the buttons when starting/stopping.

 -q                   Be less verbose.

 -d                   Dump HID packets as they change (for mapping
                      the buttons and rotaries that are not handled yet).

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
                      navmid, navbig, glyphs, text60.

Advanced options:
 -p <productId>       USB product ID (in case you want to try other hardware).
 -v <vendorId>        USB vendor ID (in case you want to try other hardware).

{RISK_DISCLAIMER}"
    );
}

/// Where the engine's decisions are actually carried out.
struct Outputs<'a> {
    hid: &'a Hid,
    leds: &'a mut Leds,
    /// `None` until the virtual keyboard exists.
    keyboard: Option<&'a mut Keyboard>,
    mmc: Option<&'a MmcPort>,
    /// The text of the first LED report the device refused, if any. The C tool
    /// only knows *that* something failed; keeping the reason is what tells
    /// "opened but not writable" apart from "could not be opened at all".
    failed: Option<String>,
}

impl Sink for Outputs<'_> {
    fn keystroke(&mut self, code: u16, press: bool) {
        let Some(keyboard) = self.keyboard.as_deref_mut() else {
            return;
        };
        let result = if press {
            keyboard.press(code)
        } else {
            keyboard.release(code)
        };
        if let Err(err) = result {
            eprintln!("Failed to send key {code}: {err}");
        }
    }

    fn mmc(&mut self, command: u8) {
        // MMC is a one-shot message, only sent when it is pressed.
        if let Some(port) = self.mmc {
            if let Err(err) = port.send_mmc(command, 0) {
                eprintln!("Failed to send MMC: {err}");
            }
        }
    }

    fn led(&mut self, index: i32, state: u8) {
        self.leds.update(index, state);
    }

    fn leds_sync(&mut self) {
        if let Err(err) = self.leds.sync(self.hid) {
            if self.failed.is_none() {
                self.failed = Some(err);
            }
        }
    }

    fn failed(&self) -> bool {
        self.failed.is_some()
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let argv0 = arguments
        .first()
        .cloned()
        .unwrap_or_else(|| KOMPLEMENT_NAME.to_string());

    let mut vid = DEFAULT_VID;
    let mut pid = DEFAULT_PID;
    let mut animate = true;
    let mut quiet = false;
    let mut midi_controller = true;
    let mut dump_packets = false;
    let mut uinput_path = DEFAULT_UINPUT_PATH.to_string();
    let mut mapping_path: Option<String> = None;
    let mut list_devices = false;
    let mut screen_probe = false;
    let mut screen_state: Option<String> = None;
    let mut parsed_options = 0usize;

    let mut options = GetOpt::new(
        arguments.iter().map(String::as_str).collect(),
        "v:p:m:o:qnhdalS:",
    );
    while let Some((option, value)) = options.next_option() {
        parsed_options += 1;
        match option {
            'h' => {
                print_usage(&argv0);
                return;
            }
            'v' => vid = i32::from_str_radix(value.trim_start_matches("0x"), 16).unwrap_or(vid),
            'p' => pid = i32::from_str_radix(value.trim_start_matches("0x"), 16).unwrap_or(pid),
            'm' => mapping_path = Some(value),
            'o' => uinput_path = value,
            'q' => quiet = true,
            'd' => dump_packets = true,
            'n' => animate = false,
            'a' => midi_controller = false,
            'l' => list_devices = true,
            's' => screen_probe = true,
            'S' => screen_state = Some(value),
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

    let Some(mapping_path) = mapping_path else {
        print_usage(&argv0);
        println!("ERROR: The -m <mapping> option is required.");
        exit(1);
    };

    // Install the interrupt handlers before touching the device.
    unsafe {
        libc::signal(libc::SIGINT, handle_interrupt as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, handle_interrupt as *const () as libc::sighandler_t);
        libc::signal(libc::SIGQUIT, handle_interrupt as *const () as libc::sighandler_t);
    }

    // Read the mapping file: this doubles as the "is the file usable?" check.
    let mut table = MappingTable::new();
    if let Err(err) = config_read(&mapping_path, !quiet, &mut table) {
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

    let mut leds = Leds::new();
    leds.init();
    if animate {
        leds.animate_on(&hid);
    }

    let mut engine = Engine::new(table);
    let mut outputs = Outputs {
        hid: &hid,
        leds: &mut leds,
        keyboard: None,
        mmc: None,
        failed: None,
    };

    // Lights the LEDs and, in doing so, checks that the device answers.
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
    outputs.mmc = mmc.as_ref();

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
    outputs.keyboard = Some(&mut keyboard);

    let mut dump = Dump::new();
    let started = Instant::now();
    let mut buffer = [0u8; READ_BUFFER_SZ];
    let mut read_errors = 0;

    while !ABORTED.load(Ordering::SeqCst) {
        let read = match hid.read_timeout(&mut buffer, 2000) {
            Ok(read) => read,
            Err(reason) => {
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
        }

        engine.process(packet, &mut outputs);
    }

    // Clean up.
    if animate {
        leds.animate_off(&hid);
    } else {
        leds.off(&hid);
    }

    exit(return_code);
}
