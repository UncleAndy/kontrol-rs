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
    println!(
        "{KOMPLEMENT_NAME} v{KOMPLEMENT_VERSION} by @hvangalen@mastodon.nl\n\n\
This utility converts the non-MIDI keys on the Komplete Kontrol A-series keyboards\n\
to keyboard presses, so they will become usable in your software of choice.\n\n\
Usage: {argv0} <options>\n\n\
Options:\n\
 -o /path/to/uinput   The path to uinput device file ({DEFAULT_UINPUT_PATH}).\n\
 -m /path/to/mapping  The path to mapping file (required to be useful).\n\
 -a                   Do not create ALSA MIDI output port for MMC messages.\n\
 -n                   Do not animate the buttons when starting/stopping.\n\n\
 -q                   Be less verbose.\n\n\
 -d                   Dump HID packets as they change (for mapping\n\
                      the buttons and rotaries that are not handled yet).\n\n\
Advanced options:\n\
 -p <productId>       USB product ID (in case you want to try other hardware).\n\
 -v <vendorId>        USB vendor ID (in case you want to try other hardware).\n\n\
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
    /// Set when the device did not accept an LED report, which is how the C
    /// tool notices that it cannot really talk to the keyboard.
    failed: bool,
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
        if !self.leds.sync(self.hid) {
            self.failed = true;
        }
    }

    fn failed(&self) -> bool {
        self.failed
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
    let mut parsed_options = 0usize;

    let mut options = GetOpt::new(
        arguments.iter().map(String::as_str).collect(),
        "v:p:m:o:qnhda",
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

    // Open the HID device.
    let Some(hid) = Hid::open(vid as u16, pid as u16) else {
        println!(
            "Opening the HID device {:04x}:{:04x} failed.\n\
Did you pass the right vendorId and productId? Does the current user have permissions?",
            vid, pid
        );
        exit(0);
    };

    // This resets the HID device back to "MIDI Mode".
    hid.write_raw(&[0xa0, 0x07, 0x00]);

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
        failed: false,
    };

    // Lights the LEDs and, in doing so, checks that the device answers.
    engine.light_up_initial(&mut outputs, animate);
    if outputs.failed {
        println!(
            "Opening the HID device {:04x}:{:04x} failed.\n\
Did you pass the right vendorId and productId? Does the current user have permissions?",
            vid, pid
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
    let mut keyboard = match Keyboard::open() {
        Ok(keyboard) => keyboard,
        Err(err) => {
            println!("uinput open: {err}");
            println!(
                "Opening the device file at {uinput_path} failed.\nCan it be read by the current user?"
            );
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
        let read = hid.read_timeout(&mut buffer, 2000);
        if read < 0 {
            println!("Error reading HID device.");
            read_errors += 1;
            if read_errors > 10 {
                println!("Too many read errors, aborting.");
                break;
            }
            continue;
        } else if read == 0 {
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
