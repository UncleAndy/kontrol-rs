//! The eight rotary knobs, as the HID report carries them while the display is
//! taken over - which is the state the panel itself needs, and the state in
//! which the knobs put nothing on the MIDI port at all.
//!
//! Measured, one knob at a time, up and down, with `komplement -x -d`
//! (`doc/Captured_Codes.md`): the position of knob N sits at bytes
//! `FIRST_KNOB_BYTE + KNOB_STRIDE * N` - knob 1 at 6-7, knob 8 at 20-21 - it is
//! little-endian, it steps by [`DEFAULT_STEP`] per detent, it is *absolute*
//! rather than a delta, and it wraps inside [`DEFAULT_MODULUS`]: turning knob 3
//! up, `03e7` (999) came round to `0008` (8) - one detent, not a thousand. The
//! touch bit that belongs to knob N is button bit `25 + N`: `packet[4]` bits 1..7
//! for knobs 1..7 and `packet[5]` bit 0 for knob 8, i.e. the `Rotary1..Rotary8`
//! the mapping knows - the same bits that made the old keystroke mapping fire on
//! every turn.

/// The first knob's low byte in the report.
pub const FIRST_KNOB_BYTE: usize = 6;
/// One little-endian position per knob.
pub const KNOB_STRIDE: usize = 2;
/// How many knobs the A-series reports.
pub const KNOBS: usize = 8;
/// Detents move the position by this much.
pub const DEFAULT_STEP: u16 = 8;
/// Positions live in `0..DEFAULT_MODULUS` and wrap around.
pub const DEFAULT_MODULUS: u16 = 1000;

/// The knobs' positions, one report at a time.
#[derive(Debug)]
pub struct Knobs {
    positions: [Option<u16>; KNOBS],
    step: u16,
    modulus: u16,
}

impl Knobs {
    /// `step` and `modulus` come from the settings; zero means "the measured
    /// default" rather than a division by nothing.
    pub fn new(step: u16, modulus: u16) -> Self {
        Knobs {
            positions: [None; KNOBS],
            step: if step == 0 { DEFAULT_STEP } else { step },
            modulus: if modulus == 0 { DEFAULT_MODULUS } else { modulus },
        }
    }

    /// Turns one report into detents moved, one entry per knob, `0` for a knob
    /// that stood still. The first report only records where the knobs are: an
    /// absolute position says nothing about movement, and reading a jump out of
    /// it would move a parameter the moment the program starts.
    pub fn read(&mut self, packet: &[u8]) -> [i32; KNOBS] {
        let mut moved = [0i32; KNOBS];
        for knob in 0..KNOBS {
            let low = FIRST_KNOB_BYTE + KNOB_STRIDE * knob;
            let Some(bytes) = packet.get(low..low + KNOB_STRIDE) else {
                continue;
            };
            let position = u16::from_le_bytes([bytes[0], bytes[1]]);
            if let Some(previous) = self.positions[knob] {
                if previous != position {
                    moved[knob] = detents(previous, position, self.step, self.modulus);
                }
            }
            self.positions[knob] = Some(position);
        }
        moved
    }
}

/// How many detents separate two positions, the short way round the modulus: a
/// knob that has just come round the end has moved one step, not a thousand.
fn detents(from: u16, to: u16, step: u16, modulus: u16) -> i32 {
    let step = i32::from(step);
    let modulus = i32::from(modulus);
    let mut delta = (i32::from(to) - i32::from(from)).rem_euclid(modulus);
    if delta > modulus / 2 {
        delta -= modulus;
    }
    // A position that did not land exactly on a step still moved a detent.
    (f64::from(delta) / f64::from(step)).round() as i32
}

/// Says which knobs moved, and how far. One line per report that moved
/// something, and nothing at all otherwise - a quiet keyboard stays quiet.
pub fn announce(moved: &[i32; KNOBS]) {
    let lines: Vec<String> = moved
        .iter()
        .enumerate()
        .filter(|(_, detents)| **detents != 0)
        .map(|(knob, detents)| {
            format!(
                "Knob {}: {}{detents}",
                knob + 1,
                if *detents > 0 { "+" } else { "" }
            )
        })
        .collect();
    if !lines.is_empty() {
        println!("[knobs] {}", lines.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A packet with the eight positions given, and nothing else that matters.
    fn packet(positions: [u16; KNOBS]) -> Vec<u8> {
        let mut bytes = vec![0u8; 30];
        for (knob, position) in positions.iter().enumerate() {
            let low = FIRST_KNOB_BYTE + KNOB_STRIDE * knob;
            bytes[low..low + KNOB_STRIDE].copy_from_slice(&position.to_le_bytes());
        }
        bytes
    }

    fn standing() -> [u16; KNOBS] {
        [132, 991, 999, 216, 999, 999, 999, 999]
    }

    #[test]
    fn the_first_report_says_where_the_knobs_are_and_nothing_else() {
        let mut knobs = Knobs::new(DEFAULT_STEP, DEFAULT_MODULUS);
        assert_eq!(
            knobs.read(&packet(standing())),
            [0; KNOBS],
            "an absolute position is not a movement"
        );
        assert_eq!(knobs.read(&packet(standing())), [0; KNOBS]);
    }

    #[test]
    fn a_detent_up_and_down_is_one_each_way() {
        let mut knobs = Knobs::new(DEFAULT_STEP, DEFAULT_MODULUS);
        knobs.read(&packet(standing()));

        // Measured on knob 1: 84 00 -> 8c 00 -> 94 00 (0x84=132, 0x8c=140).
        let mut moved = [0i32; KNOBS];
        moved[0] = 1;
        assert_eq!(knobs.read(&packet([140, 991, 999, 216, 999, 999, 999, 999])), moved);
        assert_eq!(knobs.read(&packet([148, 991, 999, 216, 999, 999, 999, 999])), moved);

        moved[0] = -1;
        assert_eq!(knobs.read(&packet([140, 991, 999, 216, 999, 999, 999, 999])), moved);
    }

    #[test]
    fn coming_round_the_end_is_one_detent_not_a_thousand() {
        // The capture, verbatim, two reports of knob 3 being turned up:
        //   [dump #36] 01 00 00 00 08 00 84 00 df 03 e7 03 ...
        //   [dump #37] 01 00 00 00 08 00 84 00 df 03 08 00 ...
        let before = [
            0x01, 0x00, 0x00, 0x00, 0x08, 0x00, 0x84, 0x00, 0xdf, 0x03, 0xe7, 0x03, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x01, 0x24,
        ];
        let after = [
            0x01, 0x00, 0x00, 0x00, 0x08, 0x00, 0x84, 0x00, 0xdf, 0x03, 0x08, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x01, 0x24,
        ];

        let mut knobs = Knobs::new(DEFAULT_STEP, DEFAULT_MODULUS);
        assert_eq!(knobs.read(&before), [0; KNOBS]);

        let mut expected = [0i32; KNOBS];
        expected[2] = 1;
        assert_eq!(
            knobs.read(&after),
            expected,
            "999 stepping up by 8 came round to 8: one detent"
        );
    }

    #[test]
    fn each_knob_has_its_own_pair_of_bytes() {
        let mut knobs = Knobs::new(DEFAULT_STEP, DEFAULT_MODULUS);
        knobs.read(&packet([0; KNOBS]));

        for knob in 0..KNOBS {
            let mut up = [0u16; KNOBS];
            up[knob] = DEFAULT_STEP;

            let mut expected = [0i32; KNOBS];
            expected[knob] = 1;
            assert_eq!(
                knobs.read(&packet(up)),
                expected,
                "knob {} lives at bytes {} and {}",
                knob + 1,
                FIRST_KNOB_BYTE + KNOB_STRIDE * knob,
                FIRST_KNOB_BYTE + KNOB_STRIDE * knob + 1
            );

            // Put it back so the next knob starts from a known position.
            knobs.read(&packet([0; KNOBS]));
        }
    }

    #[test]
    fn a_report_that_changes_something_else_moves_no_knob() {
        let mut knobs = Knobs::new(DEFAULT_STEP, DEFAULT_MODULUS);
        let mut first = packet(standing());
        knobs.read(&first);

        // The touch bit and a button go, the positions stay.
        first[4] = 0x02;
        first[2] = 0x40;
        assert_eq!(knobs.read(&first), [0; KNOBS]);
    }

    #[test]
    fn the_modulus_and_the_step_are_settings() {
        // A keyboard whose positions count by 4 and wrap at 128 is one setting
        // away, which is the whole reason these are not literals in the code.
        let mut knobs = Knobs::new(4, 128);
        knobs.read(&packet([124, 0, 0, 0, 0, 0, 0, 0]));

        let mut expected = [0i32; KNOBS];
        expected[0] = 1;
        assert_eq!(knobs.read(&packet([0, 0, 0, 0, 0, 0, 0, 0])), expected);
    }
}
