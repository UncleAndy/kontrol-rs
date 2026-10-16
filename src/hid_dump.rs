//! `-d`: print every HID report that differs from the previous one, with the
//! fields the protocol is already known to use decoded. Mirrors `hid_dump.c`.
//!
//! It exists to locate controls that are not handled yet (rotary rotation and
//! touch, 4D directions, ...) in a capture, without a hex editor.

use crate::buttons::get_button_name;
use crate::buttons::{HID_DUMP_PACKET_SZ, TOGGLE_BUTTON_TOTAL};

/// Button bits of a report: bits 0..31 from the four bytes after the report ID,
/// bits 32..39 from the fifth byte. Bytes past the report's length are zero.
pub fn button_mask(packet: &[u8]) -> u64 {
    let mut mask = 0u64;
    for i in 1..=5usize {
        if i < packet.len() {
            mask |= u64::from(packet[i]) << ((i - 1) * 8);
        }
    }
    mask
}

pub struct Dump {
    previous: [u8; HID_DUMP_PACKET_SZ],
    previous_length: usize,
    have_previous: bool,
    count: u64,
}

impl Default for Dump {
    fn default() -> Self {
        Self::new()
    }
}

impl Dump {
    pub fn new() -> Self {
        Dump {
            previous: [0; HID_DUMP_PACKET_SZ],
            previous_length: 0,
            have_previous: false,
            count: 0,
        }
    }

    /// Prints the report when it differs from the previous one. Returns true
    /// when something was printed.
    pub fn packet(&mut self, packet: &[u8], stamp_ms: i64) -> bool {
        let length = packet.len().min(HID_DUMP_PACKET_SZ);
        let packet = &packet[..length];

        let differs = !self.have_previous
            || length != self.previous_length
            || self.previous[..length] != *packet;
        if !differs {
            return false;
        }

        self.count += 1;

        let bytes: Vec<String> = packet.iter().map(|byte| format!("{byte:02x}")).collect();
        println!(
            "[dump #{} @{}ms] len={} : {}",
            self.count,
            stamp_ms,
            length,
            bytes.join(" ")
        );

        if self.have_previous {
            let mut changes: Vec<String> = Vec::new();
            if length != self.previous_length {
                changes.push(format!("len {}->{}", self.previous_length, length));
            }
            for i in 0..length.min(self.previous_length) {
                if self.previous[i] != packet[i] {
                    changes.push(format!("[{}] {:02x}->{:02x}", i, self.previous[i], packet[i]));
                }
            }
            println!("[dump #{}] changed: {}", self.count, changes.join(" "));
        }

        let mask = button_mask(packet);
        if length > 28 {
            println!(
                "[dump #{}] mask=0x{:010x} (bit0=SHIFT) dial[28]=0x{:02x}",
                self.count, mask, packet[28]
            );
        } else {
            println!("[dump #{}] mask=0x{:010x} (bit0=SHIFT)", self.count, mask);
        }

        let mut names: Vec<&str> = Vec::new();
        for bit in 0..TOGGLE_BUTTON_TOTAL {
            if mask & (1u64 << bit) != 0 {
                names.push(get_button_name(bit as i32));
            }
        }
        if names.is_empty() {
            names.push("none");
        }
        println!("[dump #{}] buttons: {}", self.count, names.join(" "));

        use std::io::Write;
        let _ = std::io::stdout().flush();

        self.previous[..length].copy_from_slice(packet);
        self.previous_length = length;
        self.have_previous = true;

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(payload: &[u8]) -> Vec<u8> {
        let mut packet = vec![0u8; 30];
        packet[..payload.len()].copy_from_slice(payload);
        packet
    }

    #[test]
    fn mask_reads_the_fifth_byte() {
        // Report ID, then one byte per group of eight buttons.
        let packet = report(&[1, 0x00, 0x00, 0x00, 0x00, 0x02]);
        assert_eq!(button_mask(&packet), 1 << 33);

        let packet = report(&[1, 0x00, 0x00, 0x20, 0x00, 0x00]);
        assert_eq!(button_mask(&packet), 1 << 21);

        let packet = report(&[1, 0x01, 0x00, 0x00, 0x00, 0x00]);
        assert_eq!(button_mask(&packet), 1);
    }

    #[test]
    fn short_reports_are_not_read_past_their_end() {
        let packet = vec![1u8, 0x01];
        assert_eq!(button_mask(&packet), 1);
        assert_eq!(button_mask(&[]), 0);
    }

    #[test]
    fn duplicates_are_silent() {
        let mut dump = Dump::new();
        let packet = report(&[1, 0x01, 0x00, 0x00, 0x00, 0x00]);
        assert!(dump.packet(&packet, 1000));
        assert!(!dump.packet(&packet, 2000));
        let mut changed = packet.clone();
        changed[4] = 0x20;
        assert!(dump.packet(&changed, 3000));
    }
}
