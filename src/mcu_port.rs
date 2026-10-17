//! The DAW's MIDI port, opened for listening: where `src/mcu.rs` gets its bytes.
//!
//! A DAW's *Mackie Control Universal* output is an ALSA **sequencer** port (not
//! rawmidi hardware), so it is read as sequencer events and turned back into the
//! raw MIDI bytes the decoder takes. The port is found by name or address,
//! because that is the one thing the user has to say: a capture showed Reaper
//! writing into `Midi Through`, but the number of that client changes between
//! sessions.
//!
//! Nothing is ever written back to the DAW, so listening cannot disturb it.

use std::ffi::CString;

use crate::mcu::Decoder;

// Re-exported, because `Port` has them as public fields: whoever reads a port
// list has to be able to look at what ALSA said about a port.
pub use alsa::seq::{PortCap, PortType};

use alsa::seq::{
    Addr, ClientIter, Event, EventType, EvCtrl, EvNote, MidiEvent, PortIter, PortSubscribe, Seq,
};

/// The name our own client carries, so the user can see who is listening.
pub const CLIENT_NAME: &str = "KOMPLEMENTARY MCU IN";

/// ALSA's own announce port: `System:Announce`, client 0 port 1. Subscribing to
/// it is the only way to be *told* that a port or a client appeared or went
/// away - and a subscription to a client that died is dead and silent, so
/// without this a restarted DAW would never be noticed.
pub const ANNOUNCE_CLIENT: i32 = 0;
pub const ANNOUNCE_PORT: i32 = 1;

/// Room for a short MIDI message, plus what the encoder may want to keep: these
/// are three-byte messages, and the buffer is only ever sized, never filled.
pub const MIDI_MESSAGE_MAX: u32 = 64;

/// How much room this client asks the sequencer for, in the units ALSA uses for
/// a pool (its own figures are small, and its defaults are sized for a trickle).
/// Best effort: the number is a margin, not the cure - draining the queue is.
const CLIENT_POOL_SIZE: u32 = 2000;

/// What one read did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Read {
    /// How many MIDI messages went into the decoder.
    pub messages: usize,
    /// Whether the sequencer reported a full queue, which means some values
    /// never arrived: the DAW sent faster than this process took it.
    pub overflowed: bool,
    /// How many port/client announcements came with this read. They are not
    /// MIDI (see [`is_announcement`]) and mean the world changed underneath:
    /// a DAW that restarted, or a port that went away.
    pub announcements: usize,
}

/// One port ALSA knows about, flattened into what a person has to choose from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Port {
    pub client: i32,
    pub port: i32,
    /// The client's name, e.g. `Midi Through`.
    pub client_name: String,
    /// The port's own name, e.g. `Midi Through Port-0`.
    pub port_name: String,
    /// What ALSA says this port can do, measured rather than assumed: whether it
    /// can be read, written to, or subscribed to as a source or a destination.
    pub capability: PortCap,
    /// What ALSA says this port *is*: hardware, software, application, a
    /// synthesizer, a plain port. A control surface's port is the DAW's own
    /// application port - but a DAW may also write into a hardware port (a
    /// `Midi Through` loopback, measured on the owner's machine), so this is for
    /// the listing to show, not a reason to drop a candidate by itself.
    pub kind: PortType,
}

impl Port {
    /// What the user types to name this port, and what an error lists.
    pub fn address(&self) -> String {
        format!("{}:{}", self.client, self.port)
    }

    /// `14:0  Midi Through / Midi Through Port-0`.
    pub fn describe(&self) -> String {
        format!("{}  {} / {}", self.address(), self.client_name, self.port_name)
    }

    /// The same, with what ALSA says the port is, for a list a person has to
    /// choose from: `(readable, hardware)`.
    pub fn describe_kind(&self) -> String {
        let mut parts = Vec::new();
        if self.capability.contains(PortCap::READ) {
            parts.push("readable");
        }
        if self.capability.contains(PortCap::WRITE) {
            parts.push("writable");
        }
        if self.capability.contains(PortCap::SUBS_READ) {
            parts.push("subscribable");
        }
        if self.kind.contains(PortType::HARDWARE) {
            parts.push("hardware");
        }
        if self.kind.contains(PortType::SOFTWARE) {
            parts.push("software");
        }
        if self.kind.contains(PortType::APPLICATION) {
            parts.push("application");
        }
        if self.kind.contains(PortType::SYNTHESIZER) {
            parts.push("synthesizer");
        }
        if self.kind.contains(PortType::PORT) {
            parts.push("port");
        }
        parts.join(", ")
    }

    /// Whether a DAW *could* write into this port, which is the only thing that
    /// makes it able to be the source a control surface sends from. Measured
    /// from ALSA's own capabilities, so a port that is only a destination (a
    /// capture tool's port, another program's input) is never a candidate.
    pub fn can_be_a_source(&self) -> bool {
        self.capability.contains(PortCap::READ) || self.capability.contains(PortCap::SUBS_READ)
    }

    /// Whether a DAW *could* write into this port - the other half of being a
    /// surface, and the half that decides whether anything in front of the DAW
    /// publishes us as something the DAW can read.
    ///
    /// `a2jmidid` creates a JACK *output* (which is what a DAW's MIDI input list
    /// is made of) only for an ALSA port that carries `SUBS_READ`, and a JACK
    /// *input* for one that carries `SUBS_WRITE`. A port that is a source alone
    /// is therefore published in the one direction no DAW reads, and the surface
    /// is invisible: measured on a machine where Reaper ran on PipeWire and
    /// every port it could see was a bidirectional one.
    pub fn can_be_a_destination(&self) -> bool {
        self.capability.contains(PortCap::WRITE) || self.capability.contains(PortCap::SUBS_WRITE)
    }

    /// True when the wanted text names this port: an exact address, or a piece
    /// of either name.
    fn matches(&self, wanted: &str) -> bool {
        let wanted = wanted.trim();
        if wanted.is_empty() {
            return false;
        }
        if wanted == self.address() {
            return true;
        }
        let wanted = wanted.to_lowercase();
        self.client_name.to_lowercase().contains(&wanted)
            || self.port_name.to_lowercase().contains(&wanted)
    }
}

/// Picks the port to listen to.
///
/// `N:M` is taken as an address and used as it is; anything else is matched
/// against the client and port names, and an exact name wins over a partial
/// one. A miss is an error that lists everything there was, because that list
/// is what the user needs in order to type the right thing.
pub fn choose(wanted: &str, ports: &[Port]) -> Result<Port, String> {
    let wanted = wanted.trim();
    if wanted.is_empty() {
        return Err(describe_ports("no port was named", ports));
    }

    if let Some((client, port)) = parse_address(wanted) {
        if let Some(found) = ports
            .iter()
            .find(|candidate| candidate.client == client && candidate.port == port)
        {
            return Ok(found.clone());
        }
        return Err(describe_ports(
            &format!("nothing is listening on {wanted}"),
            ports,
        ));
    }

    let lower = wanted.to_lowercase();
    let exact = ports.iter().find(|candidate| {
        candidate.client_name.to_lowercase() == lower || candidate.port_name.to_lowercase() == lower
    });
    if let Some(found) = exact {
        return Ok(found.clone());
    }

    ports
        .iter()
        .find(|candidate| candidate.matches(wanted))
        .cloned()
        .ok_or_else(|| describe_ports(&format!("no port matches {wanted:?}"), ports))
}

fn parse_address(wanted: &str) -> Option<(i32, i32)> {
    let (client, port) = wanted.split_once(':')?;
    let client = client.trim().parse().ok()?;
    let port = port.trim().parse().ok()?;
    Some((client, port))
}

fn describe_ports(what: &str, ports: &[Port]) -> String {
    let mut message = format!("{what}; ALSA offers:\n");
    if ports.is_empty() {
        message.push_str("  (nothing at all)\n");
    }
    for port in ports {
        let kind = port.describe_kind();
        if kind.is_empty() {
            message.push_str(&format!("  {}\n", port.describe()));
        } else {
            message.push_str(&format!("  {}  ({kind})\n", port.describe()));
        }
    }
    message.push_str(
        "Name the port to listen to with -X, either as an address (14:0) or as a\n\
         piece of its name (Midi Through). It is the port the DAW's control surface\n\
         writes to, not the keyboard's own MIDI port.",
    );
    message
}

/// Every port ALSA knows about, in the order ALSA lists them.
pub fn ports() -> Result<Vec<Port>, String> {
    let seq = Seq::open(None, None, false).map_err(|err| err.to_string())?;
    ports_with(&seq)
}

/// The same list, through a sequencer handle the caller already holds: looking
/// for a port again (a DAW restart) must not mean opening a second sequencer.
pub fn ports_with(seq: &Seq) -> Result<Vec<Port>, String> {
    let mut found = Vec::new();
    for client in ClientIter::new(seq) {
        let client_name = client.get_name().unwrap_or("").trim().to_string();
        for port in PortIter::new(seq, client.get_client()) {
            found.push(Port {
                client: port.get_client(),
                port: port.get_port(),
                client_name: client_name.clone(),
                port_name: port.get_name().unwrap_or("").trim().to_string(),
                capability: port.get_capability(),
                kind: port.get_type(),
            });
        }
    }
    Ok(found)
}

/// Our own input port, subscribed to the DAW's.
pub struct McuInput {
    seq: Seq,
    port: i32,
    source: Port,
    /// Why port announcements are unavailable, when they are: the listener still
    /// works, but a DAW restart would only be noticed at the end of the run.
    announce_error: Option<String>,
}

impl McuInput {
    /// Finds `wanted` among everything ALSA knows about and subscribes to it.
    pub fn open(wanted: &str) -> Result<McuInput, String> {
        let chosen = choose(wanted, &ports()?)?;
        McuInput::open_on(&chosen)
    }

    /// Subscribes to a port that [`crate::presence`] has already picked - the
    /// way in for the caller that searched instead of being told.
    pub fn open_chosen(chosen: &Port) -> Result<McuInput, String> {
        McuInput::open_on(chosen)
    }

    fn open_on(chosen: &Port) -> Result<McuInput, String> {
        // Non-blocking, because the reader is not allowed to wait: the caller has
        // a keyboard to look at in between the DAW's messages, and a stopped DAW
        // sends nothing at all. `read_into` takes the whole queue either way.
        let seq = Seq::open(None, None, true).map_err(|err| err.to_string())?;
        seq.set_client_name(&CString::new(CLIENT_NAME).map_err(|err| err.to_string())?)
            .map_err(|err| err.to_string())?;

        // Room for a burst. A DAW does not trickle: it dumps its whole surface
        // state the moment it starts, sends nine pitch bends together every time
        // a fader moves, and floods channel aftertouch while playing. The
        // defaults are sized for a trickle, and a queue that runs over is the
        // `ENOSPC` the first hardware run ended with. Best effort on purpose:
        // these calls fail on nothing we care about, and draining the queue is
        // what really keeps up.
        let _ = seq.set_client_pool_input(CLIENT_POOL_SIZE);
        {
            // The buffer belongs to the client, not to this handle, so setting
            // it through a temporary one is enough.
            let input = seq.input();
            let _ = input.set_input_buffer_size(CLIENT_POOL_SIZE);
        }

        let name = CString::new(format!("{CLIENT_NAME} input")).map_err(|err| err.to_string())?;
        let port = seq
            .create_simple_port(
                &name,
                // Written to by us and subscribed as a source - and *readable*
                // as well, which is not vanity. A bridge in front of a DAW
                // decides what the DAW can read from an ALSA port by that
                // port's capabilities: `a2jmidid` publishes a JACK *output*
                // (which is what a DAW's MIDI input list is made of) only for a
                // port that carries `SUBS_READ`, and a source alone is
                // published as a JACK *input*, which no DAW can read as a
                // surface. Real surfaces are bidirectional for the same reason.
                PortCap::WRITE | PortCap::SUBS_WRITE | PortCap::READ | PortCap::SUBS_READ,
                // `PORT` as well as `APPLICATION`, and that is a measurement
                // rather than a preference: a DAW's MIDI device list holds the
                // ports a bridge in front of it marks as physical, and
                // `a2jmidid` marks a port that way only for the types
                // `HARDWARE|PORT|SPECIFIC` - every port Reaper could see on the
                // machine this was measured on had that mark, and the one port
                // that did not (this one, declared `APPLICATION` alone) was the
                // one it left out. A surface is a MIDI port to everything that
                // looks at it, so it says so.
                PortType::MIDI_GENERIC | PortType::APPLICATION | PortType::PORT,
            )
            .map_err(|err| err.to_string())?;

        // The DAW's port first: without it there is nothing to listen to.
        let mut input = McuInput {
            seq,
            port,
            source: chosen.clone(),
            announce_error: None,
        };
        input
            .connect(Addr {
                client: chosen.client,
                port: chosen.port,
            })
            .map_err(|err| format!("could not subscribe to {}: {err}", chosen.describe()))?;

        // …and then ALSA's announce port, so that a *restart* of the DAW can be
        // noticed while the run goes on. Optional on purpose: losing it costs
        // promptness, not the listener, so it is reported rather than fatal.
        if let Err(reason) = input.connect(Addr {
            client: ANNOUNCE_CLIENT,
            port: ANNOUNCE_PORT,
        }) {
            input.announce_error = Some(reason);
        }

        Ok(input)
    }

    /// Subscribes our port to `sender`, so that its events are delivered here.
    fn connect(&self, sender: Addr) -> Result<(), String> {
        let subscription = PortSubscribe::empty().map_err(|err| err.to_string())?;
        subscription.set_sender(sender);
        subscription.set_dest(Addr {
            client: self.seq.client_id().map_err(|err| err.to_string())?,
            port: self.port,
        });
        self.seq
            .subscribe_port(&subscription)
            .map_err(|err| err.to_string())
    }

    /// Undoes [`McuInput::connect`], best effort: when the other client died the
    /// kernel has dropped the subscription already, and an error here says
    /// nothing about anything we care about.
    fn disconnect(&self, sender: Addr) {
        if let Ok(client) = self.seq.client_id() {
            let _ = self
                .seq
                .unsubscribe_port(sender, Addr { client, port: self.port });
        }
    }

    /// Our own port number, so it can be found in a list of connections.
    pub fn port(&self) -> i32 {
        self.port
    }

    /// Sends one short MIDI message to whoever is subscribed to our port - the
    /// DAW's Mackie Control *input*, when it has been pointed at us. A surface
    /// has to answer as well as listen, which is why the port is created with
    /// `WRITE`, and this is the same mechanism the MMC messages already use.
    pub fn send(&self, message: &[u8]) -> Result<(), String> {
        let mut encoder =
            MidiEvent::new(MIDI_MESSAGE_MAX).map_err(|err| format!("no MIDI encoder: {err}"))?;
        let (used, event) = encoder
            .encode(message)
            .map_err(|err| format!("{message:02x?} could not be encoded: {err}"))?;
        if used != message.len() {
            return Err(format!(
                "only {used} of {} bytes are a MIDI message",
                message.len()
            ));
        }
        let Some(mut event) = event else {
            return Err(format!("{message:02x?} is not a MIDI message"));
        };
        event.set_source(self.port);
        event.set_subs();
        event.set_direct();
        self.seq
            .event_output_direct(&mut event)
            .map(|_| ())
            .map_err(|err| format!("sending {message:02x?} failed: {err}"))
    }

    /// The port we listen to, as it was found.
    pub fn source(&self) -> &Port {
        &self.source
    }

    /// Why port announcements are unavailable, when they are: everything else
    /// works, but a DAW restart is then only noticed at the end of the run.
    pub fn announce_error(&self) -> Option<&str> {
        self.announce_error.as_deref()
    }

    /// Whether the port we listen to is still there.
    ///
    /// A subscription to a client that went away is dead *and silent*, which is
    /// indistinguishable from a DAW that has nothing to say - so this is the
    /// check that tells those two apart.
    pub fn still_there(&self) -> Result<bool, String> {
        Ok(ports_with(&self.seq)?.iter().any(|port| {
            port.client == self.source.client && port.port == self.source.port
        }))
    }

    /// Looks for our port again by its name and listens to it where it is now.
    ///
    /// A restarted DAW comes back with a new client number; its *name* is the one
    /// thing that stays. The port's own name is tried first, then the client's,
    /// and the first match wins - the rule the rest of this file already uses.
    /// The dead subscription is dropped (best effort) and a new one made, so
    /// events flow again without the caller restarting anything.
    pub fn refind(&mut self) -> Result<Port, String> {
        let ports = ports_with(&self.seq)?;
        let found = choose(&self.source.port_name, &ports)
            .or_else(|_| choose(&self.source.client_name, &ports))?;

        if found.client == self.source.client && found.port == self.source.port {
            // It never went away; only the address was announced again.
            return Ok(found);
        }

        self.disconnect(Addr {
            client: self.source.client,
            port: self.source.port,
        });
        self.connect(Addr {
            client: found.client,
            port: found.port,
        })?;
        self.source = found.clone();
        Ok(found)
    }

    /// Reads the next message and everything queued behind it into `decoder`.
    ///
    /// The first read blocks until the DAW says something; it comes back with an
    /// error when a signal interrupts it, which is how the live view notices
    /// Ctrl+C. Everything already waiting is taken before returning, because a
    /// DAW sends in bursts: drawing once per message is what runs this buffer
    /// over, and the caller only wants to draw once per burst anyway.
    ///
    /// Takes everything the sequencer has waiting for us and comes straight back:
    /// it never waits for the DAW.
    ///
    /// Not waiting is the point. A DAW that is stopped sends *nothing at all* (the
    /// capture measured eight silent seconds), so a reader that waited would hide
    /// the keyboard's own buttons for as long as the DAW had nothing to say. The
    /// whole queue is still taken in one go before anything is drawn, which is
    /// what keeps `ENOSPC` away.
    ///
    /// A full queue is reported, not fatal: the sequencer says `ENOSPC`, some
    /// values did not arrive, and the state is still the DAW's - only a few of
    /// its numbers are older than they could have been.
    pub fn read_into(&mut self, decoder: &mut Decoder) -> Result<Read, String> {
        // The crate keeps the input side in its own handle: a `Seq` lends out
        // one `Input` at a time, and it is the `Input` that reads events.
        let mut input = self.seq.input();
        let mut read = Read {
            messages: 0,
            overflowed: false,
            announcements: 0,
        };

        loop {
            match input.event_input() {
                Ok(event) => {
                    if is_announcement(&event) {
                        // The sequencer's own bookkeeping, not the DAW's music:
                        // a client id fed to the decoder would be a note number.
                        read.announcements += 1;
                    } else if let Some(bytes) = message_bytes(&event) {
                        for byte in bytes {
                            decoder.push(byte);
                        }
                        read.messages += 1;
                    }
                }
                // Nothing left: this is how a non-blocking drain ends, not an
                // error.
                Err(err) if err.errno() == libc::EAGAIN => return Ok(read),
                Err(err) if err.errno() == libc::ENOSPC => {
                    read.overflowed = true;
                    if self.pending(&mut input)? == 0 {
                        return Ok(read);
                    }
                }
                Err(err) => return Err(err.to_string()),
            }
        }
    }

    /// How many events are waiting, ours and the sequencer's.
    fn pending(&self, input: &mut alsa::seq::Input) -> Result<u32, String> {
        input
            .event_input_pending(true)
            .map_err(|err| err.to_string())
    }
}

/// Whether this event is the sequencer's own bookkeeping rather than MIDI.
///
/// A subscriber of ALSA's announce port is told when ports and clients appear,
/// change and go away; those events carry an *address*, not a note, so they must
/// never reach the decoder: a client id read as a key number would light up a
/// mute LED. They are counted instead, because they mean the world moved - which
/// is how a restarted DAW is noticed at all (the subscription to it died with it).
fn is_announcement(event: &Event) -> bool {
    matches!(
        event.get_type(),
        EventType::PortStart
            | EventType::PortExit
            | EventType::PortChange
            | EventType::ClientStart
            | EventType::ClientExit
            | EventType::ClientChange
            | EventType::PortSubscribed
            | EventType::PortUnsubscribed
    )
}

/// One sequencer event as raw MIDI, or `None` when it is not MIDI we read.
fn message_bytes(event: &Event) -> Option<Vec<u8>> {
    match event.get_type() {
        EventType::Noteon => {
            let note: EvNote = event.get_data()?;
            Some(vec![0x90 | note.channel, note.note, note.velocity])
        }
        EventType::Noteoff => {
            let note: EvNote = event.get_data()?;
            Some(vec![0x80 | note.channel, note.note, note.off_velocity])
        }
        EventType::Keypress => {
            let note: EvNote = event.get_data()?;
            Some(vec![0xa0 | note.channel, note.note, note.velocity])
        }
        EventType::Controller => {
            let control: EvCtrl = event.get_data()?;
            Some(vec![
                0xb0 | control.channel,
                control.param as u8,
                (control.value & 0x7f) as u8,
            ])
        }
        EventType::Chanpress => {
            let control: EvCtrl = event.get_data()?;
            Some(vec![0xd0 | control.channel, (control.value & 0x7f) as u8])
        }
        EventType::Pitchbend => {
            let control: EvCtrl = event.get_data()?;
            let raw = (control.value + 8192).clamp(0, 16383) as u16;
            Some(vec![
                0xe0 | control.channel,
                (raw & 0x7f) as u8,
                (raw >> 7) as u8,
            ])
        }
        EventType::Sysex => {
            let bytes = event.get_ext()?;
            Some(frame_sysex(bytes))
        }
        _ => None,
    }
}

/// ALSA hands a SysEx out with or without its framing bytes, depending on who
/// sent it; the decoder wants `F0 ... F7` either way.
fn frame_sysex(bytes: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(bytes.len() + 2);
    if bytes.first() != Some(&0xf0) {
        framed.push(0xf0);
    }
    framed.extend_from_slice(bytes);
    if framed.last() != Some(&0xf7) {
        framed.push(0xf7);
    }
    framed
}

#[cfg(test)]
mod tests {
    use super::*;
    use alsa::seq::Connect;

    fn port(client: i32, port: i32, client_name: &str, port_name: &str) -> Port {
        Port {
            client,
            port,
            client_name: client_name.to_string(),
            port_name: port_name.to_string(),
            // What a real one looks like: something a DAW can write into.
            capability: PortCap::READ | PortCap::SUBS_READ,
            kind: PortType::MIDI_GENERIC | PortType::HARDWARE,
        }
    }

    /// What `aconnect -l` showed on the machine the capture was taken on.
    fn machine() -> Vec<Port> {
        vec![
            port(0, 0, "System", "Timer"),
            port(14, 0, "Midi Through", "Midi Through Port-0"),
            port(16, 0, "Steinberg UR22C", "Steinberg UR22C MIDI 1"),
            port(20, 0, "KOMPLETE KONTROL A49", "KOMPLETE KONTROL A49 MIDI 1"),
            port(144, 0, "PipeWire-System", "input"),
        ]
    }

    #[test]
    fn an_address_names_a_port_exactly() {
        let chosen = choose("20:0", &machine()).unwrap();
        assert_eq!(chosen.client_name, "KOMPLETE KONTROL A49");

        let chosen = choose(" 14:0 ", &machine()).unwrap();
        assert_eq!(chosen.port_name, "Midi Through Port-0");
    }

    #[test]
    fn a_piece_of_a_name_is_enough_and_case_does_not_matter() {
        let chosen = choose("midi through", &machine()).unwrap();
        assert_eq!(chosen.address(), "14:0");

        let chosen = choose("UR22C", &machine()).unwrap();
        assert_eq!(chosen.address(), "16:0");
    }

    #[test]
    fn an_exact_name_beats_a_partial_one() {
        let ports = vec![
            port(20, 0, "keysomething", "port one"),
            port(24, 0, "keys", "keys"),
        ];
        let chosen = choose("keys", &ports).unwrap();
        assert_eq!(chosen.client, 24);

        // Two ports that both contain the name are a tie, and the one ALSA
        // lists first wins; nothing about a name makes one of them better.
        let ports = vec![
            port(20, 0, "keys A", "port one"),
            port(24, 0, "keys B", "port two"),
        ];
        assert_eq!(choose("keys", &ports).unwrap().client, 20);
    }

    #[test]
    fn a_miss_lists_what_there_was() {
        let message = choose("nonsense", &machine()).unwrap_err();
        assert!(message.contains("no port matches"), "{message}");
        for expected in ["14:0", "Midi Through Port-0", "Steinberg UR22C"] {
            assert!(message.contains(expected), "{message} did not list {expected}");
        }

        let message = choose("99:9", &machine()).unwrap_err();
        assert!(message.contains("nothing is listening on 99:9"), "{message}");

        let message = choose("", &machine()).unwrap_err();
        assert!(message.contains("no port was named"), "{message}");
    }

    #[test]
    fn a_sysex_is_framed_whichever_way_alsa_hands_it_over() {
        let with_framing = vec![0xf0, 0x00, 0x66, 0xf7];
        assert_eq!(frame_sysex(&with_framing), with_framing);

        let without = vec![0x00, 0x66, 0x14, 0x12];
        assert_eq!(frame_sysex(&without), vec![0xf0, 0x00, 0x66, 0x14, 0x12, 0xf7]);
    }

    #[test]
    fn an_announcement_is_never_mistaken_for_music() {
        // Every type ALSA's announce port sends, as the sequencer builds them:
        // an address, not a note. None of them may become bytes.
        let address = Addr { client: 20, port: 0 };
        let connect = Connect {
            sender: address,
            dest: Addr { client: 128, port: 0 },
        };
        for kind in [
            EventType::ClientStart,
            EventType::ClientExit,
            EventType::ClientChange,
            EventType::PortStart,
            EventType::PortExit,
            EventType::PortChange,
            EventType::PortSubscribed,
            EventType::PortUnsubscribed,
        ] {
            // Announcements carry an address; the two subscription events carry
            // that address twice, as a connect.
            let event = if kind == EventType::PortSubscribed || kind == EventType::PortUnsubscribed {
                Event::new(kind, &connect)
            } else {
                Event::new(kind, &address)
            };
            assert!(is_announcement(&event), "{kind:?} has to be an announcement");
            assert!(
                message_bytes(&event).is_none(),
                "{kind:?} must not reach the decoder"
            );
        }

        // …while the DAW's own traffic is still music.
        let note = Event::new(
            EventType::Noteon,
            &EvNote {
                channel: 0,
                note: 17,
                velocity: 127,
                off_velocity: 0,
                duration: 0,
            },
        );
        assert!(!is_announcement(&note));
        assert_eq!(message_bytes(&note), Some(vec![0x90, 17, 127]));
    }
}
