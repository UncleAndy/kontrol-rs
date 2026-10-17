//! The listening side against a real ALSA sequencer, in one process: a writer
//! port stands in for a DAW and the decoder has to see what is sent to it.
//!
//! This is the one part of the live view that the capture cannot check, so it is
//! checked against ALSA itself. It needs the sequencer (`snd_seq`, which every
//! Linux with ALSA has) and no hardware at all: the writer is found by name,
//! exactly the way a DAW's port is found by name.

use std::ffi::CString;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use alsa::seq::{MidiEvent, PortCap, PortType, Seq};

use kontrol_rs::mcu::Decoder;
use kontrol_rs::mcu_port::{ports, McuInput};

/// The name of the port this test writes from.
const WRITER: &str = "kontrol-rs test writer";

/// Whether this machine has the ALSA sequencer at all. A container without
/// `/dev/snd/seq` cannot run this test, and saying so is better than a failure
/// that blames the code: the machine that runs the live view has a sequencer,
/// and there the test does its work.
fn sequencer_available() -> bool {
    std::path::Path::new("/dev/snd/seq").exists()
}

/// Sends raw MIDI into the sequencer, the way a DAW's surface does.
fn send(seq: &Seq, port: i32, bytes: &[u8]) {
    let mut encoder = MidiEvent::new(64).expect("a MIDI encoder");
    let (_, event) = encoder.encode(bytes).expect("encoding a message");
    let mut event = event.expect("the bytes made a message");
    event.set_source(port);
    event.set_subs();
    event.set_direct();
    seq.event_output_direct(&mut event).expect("sending");
}

#[test]
fn what_a_writer_port_sends_reaches_the_decoder() {
    if !sequencer_available() {
        println!(
            "No /dev/snd/seq on this machine, so there is no sequencer to test against.\n\
             This test needs ALSA and does nothing without it - run `cargo test` where the\n\
             keyboard and the live view live, and it checks the whole listening path there."
        );
        return;
    }

    let writer = Seq::open(None, None, false).expect("a sequencer to write from");
    writer
        .set_client_name(&CString::new("kontrol-rs test").expect("a name"))
        .expect("naming the client");
    let port = writer
        .create_simple_port(
            &CString::new(WRITER).expect("a name"),
            PortCap::READ | PortCap::SUBS_READ,
            PortType::APPLICATION,
        )
        .expect("creating the writer port");

    // The writer is a port like any other, so the live view has to be able to
    // find it by name - which is how a DAW's port gets found.
    let listed = ports()
        .expect("listing the ports")
        .iter()
        .any(|candidate| candidate.port_name == WRITER);
    assert!(listed, "the writer port was not listed by ALSA");

    let mut input = McuInput::open(WRITER).expect("subscribing to the writer port");

    // Reading blocks, so it happens on its own thread and the test waits for an
    // answer with a deadline: a port that never delivers must fail, not hang.
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut decoder = Decoder::new();
        let mut delivered = 0;
        while delivered < 3 {
            match input.read_into(&mut decoder) {
                Ok(read) => delivered += read.messages,
                Err(message) => {
                    let _ = sender.send(Err(message));
                    return;
                }
            }
        }
        let _ = sender.send(Ok(decoder.studio().clone()));
    });

    // The three shapes the capture measured, sent as bytes: a mute LED, a fader
    // at the top of the fourth strip, and one seven-character name field.
    send(&writer, port, &[0x90, 17, 127]);
    send(&writer, port, &[0xe3, 0x7f, 0x7f]);
    let mut lcd = vec![0xf0, 0x00, 0x00, 0x66, 0x14, 0x12, 0x07];
    lcd.extend_from_slice(b"0 Lead ");
    lcd.push(0xf7);
    send(&writer, port, &lcd);

    let state = receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("the port delivered nothing within ten seconds")
        .expect("reading from the port failed");

    assert!(state.bank[1].mute, "the mute LED of the second strip");
    assert!(
        (state.bank[3].fader - 1.0).abs() < 0.0001,
        "the fader of the fourth strip: {}",
        state.bank[3].fader
    );
    assert_eq!(state.bank[1].name, "0 Lead", "the name field at offset seven");
}
