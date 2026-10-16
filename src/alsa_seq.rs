//! ALSA sequencer client with one MIDI output port, used for MMC messages.
//! Mirrors `alsa.c`: the port is visible to other applications (a DAW can
//! subscribe to "KOMPLEMENTARY: MIDI OUT").

use std::ffi::CString;

use alsa::seq::{Event, EventType, PortCap, PortType, Seq};

use crate::defs::{ALSA_CLIENT_NAME, ALSA_PORT_NAME};

pub struct MmcPort {
    seq: Seq,
    port: i32,
}

impl MmcPort {
    /// Creates the client and the output port. The error string is already
    /// user-facing.
    pub fn open(client_name: Option<&str>) -> Result<Self, String> {
        let seq = Seq::open(None, None, false).map_err(|err| err.to_string())?;

        let name = client_name.unwrap_or(ALSA_CLIENT_NAME);
        let name = CString::new(name).map_err(|err| err.to_string())?;
        seq.set_client_name(&name).map_err(|err| err.to_string())?;

        let port_name = CString::new(format!("{ALSA_CLIENT_NAME} {ALSA_PORT_NAME}"))
            .map_err(|err| err.to_string())?;
        let port = seq
            .create_simple_port(
                &port_name,
                PortCap::READ | PortCap::SUBS_READ,
                PortType::APPLICATION,
            )
            .map_err(|err| err.to_string())?;

        Ok(MmcPort { seq, port })
    }

    /// Sends `F0 7F 7F 06 <command> F7` (device ID 0x7f = all devices).
    pub fn send_mmc(&self, command: u8, _channel: u8) -> Result<(), String> {
        let message = [0xf0, 0x7f, 0x7f, 0x06, command, 0xf7];

        let mut event = Event::new_ext(EventType::Sysex, &message[..]);
        event.set_source(self.port);
        event.set_subs();
        event.set_direct();

        self.seq
            .event_output_direct(&mut event)
            .map(|_| ())
            .map_err(|err| err.to_string())
    }
}
