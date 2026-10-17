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

use kontrol_rs::knobs::fader_message;
use kontrol_rs::mcu::Decoder;
use kontrol_rs::mcu_port::{ports, McuInput, CLIENT_NAME};

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

    // The read does not wait, so it happens on its own thread - which waits, with
    // a deadline: a port that never delivers must fail, not hang.
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut decoder = Decoder::new();
        let mut delivered = 0;
        while delivered < 3 {
            match input.read_into(&mut decoder) {
                Ok(read) => {
                    delivered += read.messages;
                    if read.messages == 0 {
                        // Nothing waiting is the normal answer from a
                        // non-blocking read; waiting is the reader's job.
                        thread::sleep(Duration::from_millis(5));
                    }
                }
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

/// A DAW that restarts is a *new* client with a new number, and the subscription
/// to the old one dies with it - silently. What has to hold is that the listening
/// side can find its port again by name and hear the new client.
///
/// Like the test above, this needs a sequencer and no hardware, and does nothing
/// on a machine without `/dev/snd/seq`.
#[test]
fn a_writer_that_comes_back_under_the_same_name_is_listened_to_again() {
    if !sequencer_available() {
        println!(
            "No /dev/snd/seq on this machine, so there is no sequencer to test against.\n\
             This test needs ALSA and does nothing without it - run `cargo test` where the\n\
             keyboard and the live view live, and it checks that a restarted DAW is followed."
        );
        return;
    }

    const NAME: &str = "kontrol-rs follow writer";

    /// One writer client with one port of the given name, the way a DAW's
    /// control surface looks from here.
    fn writer_of(name: &str) -> (Seq, i32) {
        let writer = Seq::open(None, None, false).expect("a sequencer to write from");
        writer
            .set_client_name(&CString::new("kontrol-rs follow test").expect("a name"))
            .expect("naming the client");
        let port = writer
            .create_simple_port(
                &CString::new(name).expect("a name"),
                PortCap::READ | PortCap::SUBS_READ,
                PortType::APPLICATION,
            )
            .expect("creating the writer port");
        (writer, port)
    }

    let (first, _first_port) = writer_of(NAME);
    let mut input = McuInput::open(NAME).expect("subscribing to the writer port");
    let first_client = input.source().client;

    // The DAW quits: its client goes away, taking the subscription with it.
    drop(first);

    // ALSA removes the client asynchronously, so give it a moment rather than
    // asserting on the instant. Waiting for a *state*, not for a fixed time.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while input.still_there().expect("listing the ports") {
        if std::time::Instant::now() > deadline {
            panic!("the writer port was still listed five seconds after its client closed");
        }
        thread::sleep(Duration::from_millis(20));
    }

    // The DAW is started again: same name, and - the point of the test - it may
    // well get a different client number, which is why the name is what is
    // searched for.
    let (second, second_port) = writer_of(NAME);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let found = loop {
        match input.refind() {
            Ok(found) => break found,
            Err(message) => {
                if std::time::Instant::now() > deadline {
                    panic!("the writer never came back to the port list: {message}");
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
    };

    assert_eq!(found.port_name, NAME);
    assert_eq!(
        found.client,
        second.client_id().expect("the new writer's client number"),
        "the port that was found has to belong to the client that came back, not to the one \
         that quit (it was {first_client})"
    );

    // …and the events of the new writer actually arrive, which is what the
    // re-subscription is for: finding the port is not hearing it.
    let mut decoder = Decoder::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut delivered = 0;
    while delivered < 1 {
        if std::time::Instant::now() > deadline {
            panic!("the new writer's message never reached the decoder");
        }
        delivered += input
            .read_into(&mut decoder)
            .expect("reading from the port")
            .messages;
        send(&second, second_port, &[0x90, 17, 127]);
        thread::sleep(Duration::from_millis(20));
    }

    assert!(decoder.studio().bank[1].mute, "the mute LED of the second strip");
}

/// What a surface says *back*: `send` on our own port reaches whoever subscribed
/// to it - which is what a DAW's Mackie Control *input* does when it is pointed
/// at us, and the way the first knob moves the current track's level.
#[test]
fn what_our_port_sends_reaches_the_daw() {
    if !sequencer_available() {
        println!(
            "No /dev/snd/seq on this machine, so there is no sequencer to test against.\n\
             This test needs ALSA and does nothing without it - run `cargo test` where the\n\
             keyboard and the live view live, and it checks the whole sending path there."
        );
        return;
    }

    // A stand-in for the DAW's own output, so that our port has something to
    // listen to and the pair is in the state it is in for real.
    let writer = Seq::open(None, None, false).expect("a sequencer to write from");
    writer
        .set_client_name(&CString::new("kontrol-rs test").expect("a name"))
        .expect("naming the client");
    writer
        .create_simple_port(
            &CString::new(WRITER).expect("a name"),
            PortCap::READ | PortCap::SUBS_READ,
            PortType::MIDI_GENERIC | PortType::APPLICATION,
        )
        .expect("the stand-in DAW's port");

    let surface = McuInput::open(WRITER).expect("the surface, subscribing to the stand-in DAW");
    // Our own port has to be readable *and* writable: a bridge in front of a DAW
    // decides what the DAW can read from an ALSA port by its capabilities, and
    // publishes a JACK output (a DAW's MIDI input) only for a readable port.
    // Without that half the surface is invisible to Reaper, which is a fault
    // that only shows up on a machine with a bridge in the way.
    let ours = ports()
        .expect("a list of ports")
        .into_iter()
        .find(|port| port.describe().contains(CLIENT_NAME))
        .expect("our own port, in ALSA's list");
    assert!(
        ours.can_be_a_source() && ours.can_be_a_destination(),
        "the surface's port must be readable as well as writable: {}",
        ours.describe()
    );
    // The DAW's input, standing in for Reaper's: a listener on the surface's own
    // port, which is where a level goes when the knob is turned.
    let mut daw = McuInput::open(CLIENT_NAME).expect("the DAW, subscribing to the surface");

    let wanted = fader_message(0, 0.5);
    surface.send(&wanted).expect("sending a fader position");

    let mut decoder = Decoder::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut messages = 0;
    while std::time::Instant::now() < deadline && messages == 0 {
        messages += daw
            .read_into(&mut decoder)
            .expect("reading on the DAW's side")
            .messages;
        if messages == 0 {
            thread::sleep(Duration::from_millis(5));
        }
    }

    assert_eq!(messages, 1, "one fader move should arrive, and nothing else");
    let fader = decoder.studio().bank[0].fader;
    assert!(
        (fader - 0.5).abs() < 0.001,
        "the level went out as {wanted:02x?} and arrived as {fader}"
    );
}
