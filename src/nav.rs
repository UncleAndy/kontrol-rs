//! The navigation screen: what the keyboard shows while you work in the DAW.
//!
//! This module owns the *layout* only - it turns a [`NavState`] into a frame. The
//! state will be filled from a MIDI or OSC feed from the DAW; `sample()` is the
//! fixed state used by `-S nav` to judge the layout on the real panel.
//!
//! Four lines, 21 characters each at the built-in 5x7 font, plus a bar drawn in
//! pixels:
//!
//! ```text
//! 2 BASS DI            K3
//! VOL ################ -6.1 DB
//! 2:14.3  120BPM  PLAY
//! M S ARM  GRID 1/16
//! ```

use crate::font;
use crate::screen::{self, Frame, WIDTH};

/// What the transport is doing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transport {
    Stopped,
    Playing,
    Recording,
}

impl Transport {
    fn word(self) -> &'static str {
        match self {
            Transport::Stopped => "STOP",
            Transport::Playing => "PLAY",
            Transport::Recording => "REC",
        }
    }
}

/// Everything the screen shows. Strings are already formatted for display: the
/// feed from the DAW decides the wording, the layout only decides the placing.
#[derive(Clone, Debug, PartialEq)]
pub struct NavState {
    /// Name of the selected track, with its number, e.g. `2 BASS DI`.
    pub track: String,
    /// Which knob is being touched, 1..=16; `None` while nothing is touched.
    pub knob: Option<u8>,
    /// Name of the parameter on the second line, e.g. `VOL`, `PAN`.
    pub parameter: String,
    /// Its value as text, e.g. `-6.1 DB`.
    pub value: String,
    /// Where the value sits in its range, 0.0..=1.0, for the bar.
    pub value_fraction: f32,
    /// Play position, e.g. `2:14.3` (bar.beat.ticks).
    pub position: String,
    /// Tempo, e.g. `120`.
    pub tempo: String,
    pub transport: Transport,
    pub mute: bool,
    pub solo: bool,
    pub armed: bool,
    /// Grid division as text, e.g. `1/16`.
    pub grid: String,
}

impl NavState {
    /// The state `-S nav` draws: something with every field filled, so the
    /// layout can be judged at a glance.
    pub fn sample() -> Self {
        NavState {
            track: "2 BASS DI".to_string(),
            knob: Some(3),
            parameter: "VOL".to_string(),
            value: "-6.1 DB".to_string(),
            value_fraction: 0.62,
            position: "2:14.3".to_string(),
            tempo: "120".to_string(),
            transport: Transport::Playing,
            mute: false,
            solo: true,
            armed: true,
            grid: "1/16".to_string(),
        }
    }
}

/// Characters that fit on one line.
pub const LINE_CHARACTERS: usize = font::characters_per_line(WIDTH);

/// The pixel rows the four lines sit on.
pub const LINES: [usize; 4] = [0, 8, 16, 24];

/// Glyph scale of the big layout: double size, so ten characters per line.
pub const BIG_SCALE: usize = 2;

/// Characters that fit on a big line.
pub const BIG_LINE_CHARACTERS: usize = font::scaled_characters_per_line(WIDTH, BIG_SCALE);

/// The pixel rows of the big layout: two lines of fourteen rows, four rows apart.
pub const BIG_LINES: [usize; 2] = [0, 16];

/// The pixel rows of the middle layout: a small line, a big value line, a small
/// line - seven, fourteen and seven rows, so all three fit on the screen.
pub const MID_LINES: [usize; 3] = [0, 8, 24];

/// Characters the small side lines of the middle layout use, leaving room for the
/// right-aligned part of the same line.
const MID_SIDE_CHARACTERS: usize = 15;

/// Cuts text to one line of `limit` characters, so a long name cannot run past
/// the edge or push the right-aligned parts out of the layout.
fn clip_to(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn clip(text: &str) -> String {
    clip_to(text, LINE_CHARACTERS)
}

/// Draws `text` so that it ends at the right edge.
fn draw_right(frame: &mut Frame, row: usize, text: &str) {
    let text = clip(text);
    let width = font::text_width(text.chars().count());
    screen::draw_text(frame, WIDTH - width, row, &text);
}

/// Turns the state into a frame: three text lines, one line with the bar.
pub fn render(state: &NavState) -> Frame {
    let mut frame = screen::off();

    // Line 1: the track, and which knob is live on the right.
    screen::draw_text(&mut frame, 0, LINES[0], &clip(&state.track));
    if let Some(knob) = state.knob {
        draw_right(&mut frame, LINES[0], &format!("K{knob}"));
    }

    // Line 2: parameter on the left, value on the right, bar between them.
    screen::draw_text(&mut frame, 0, LINES[1], &clip(&state.parameter));
    let value = clip(&state.value);
    let value_width = font::text_width(value.chars().count());
    draw_right(&mut frame, LINES[1], &value);

    let bar_x = font::text_width(state.parameter.chars().count().min(LINE_CHARACTERS)) + 2;
    let bar_width = (WIDTH - value_width - 3).saturating_sub(bar_x);
    if bar_width >= 8 {
        screen::draw_bar(&mut frame, bar_x, LINES[1], bar_width, 7, state.value_fraction);
    }

    // Line 3: position, tempo, transport.
    screen::draw_text(
        &mut frame,
        0,
        LINES[2],
        &clip(&format!("{}  {}BPM", state.position, state.tempo)),
    );
    draw_right(&mut frame, LINES[2], state.transport.word());

    // Line 4: the flags of the track and the grid.
    let mut flags = String::new();
    flags += if state.mute { "M" } else { "-" };
    flags += " ";
    flags += if state.solo { "S" } else { "-" };
    flags += " ";
    flags += if state.armed { "ARM" } else { "---" };
    screen::draw_text(&mut frame, 0, LINES[3], &clip(&flags));
    draw_right(&mut frame, LINES[3], &format!("GRID {}", clip(&state.grid)));

    frame
}

/// The big layout: the same state at double glyph size, which fits two lines
/// instead of four - the track and the live value, i.e. what you look at while
/// navigating. The knob marker stays small in the top-right corner; position,
/// tempo, transport and the flags do not fit at this size, so they live in the
/// four-line layout only.
///
/// ```text
/// 2 BASS DI          K3
///
/// -6.1 DB
/// ```
pub fn render_big(state: &NavState) -> Frame {
    let mut frame = screen::off();

    screen::draw_text_scaled(
        &mut frame,
        0,
        BIG_LINES[0],
        &clip_to(&state.track, BIG_LINE_CHARACTERS),
        BIG_SCALE,
    );

    let value = clip_to(&state.value, BIG_LINE_CHARACTERS);
    screen::draw_text_scaled(&mut frame, 0, BIG_LINES[1], &value, BIG_SCALE);

    if let Some(knob) = state.knob {
        draw_right(&mut frame, BIG_LINES[0], &format!("K{knob}"));
    }

    frame
}

/// The middle layout: three lines, the value double size between two small ones -
/// the compromise between the four-line and the two-line screens. The parameter
/// name sits in the top-right corner next to the track, so the big line can be
/// the value alone; position, tempo and transport share the bottom line with the
/// knob marker. The mute/solo/armed flags do not fit here.
///
/// ```text
/// 2 BASS DI        VOL
///
/// -6.1 DB
///
/// 2:14.3 120BPM PLAY    K3
/// ```
pub fn render_mid(state: &NavState) -> Frame {
    let mut frame = screen::off();

    // Line 1: the track, and the parameter the value belongs to.
    screen::draw_text(
        &mut frame,
        0,
        MID_LINES[0],
        &clip_to(&state.track, MID_SIDE_CHARACTERS),
    );
    draw_right(&mut frame, MID_LINES[0], &clip_to(&state.parameter, LINE_CHARACTERS));

    // Line 2: the value at double size - the thing you look at while turning.
    screen::draw_text_scaled(
        &mut frame,
        0,
        MID_LINES[1],
        &clip_to(&state.value, BIG_LINE_CHARACTERS),
        BIG_SCALE,
    );

    // Line 3: position, tempo, transport, and the live knob.
    let info = format!(
        "{} {}BPM {}",
        state.position,
        state.tempo,
        state.transport.word()
    );
    screen::draw_text(
        &mut frame,
        0,
        MID_LINES[2],
        &clip_to(&info, MID_SIDE_CHARACTERS),
    );
    if let Some(knob) = state.knob {
        draw_right(&mut frame, MID_LINES[2], &format!("K{knob}"));
    }

    frame
}

/// The pixel rows of the uniform medium layout: three lines of nine rows, two
/// rows apart, which is what the 7x9 font is sized for.
pub const MEDIUM_LINES: [usize; 3] = [0, 11, 22];

/// Characters that fit on a medium line.
pub const MEDIUM_LINE_CHARACTERS: usize = crate::font_medium::characters_per_line(WIDTH);

/// Characters the bottom line of the uniform medium layout uses, leaving room for
/// the knob marker beside it.
const MEDIUM_INFO_CHARACTERS: usize = 13;

/// Draws `text` in the medium font so that it ends at the right edge.
fn draw_right_medium(frame: &mut Frame, row: usize, text: &str) {
    let text = clip_to(text, MEDIUM_LINE_CHARACTERS);
    let width = crate::font_medium::text_width(text.chars().count());
    screen::draw_text_medium(frame, WIDTH - width, row, &text);
}

/// The uniform medium layout: three lines, all of them in the 7x9 font - the
/// track, then parameter and value, then position, tempo and transport with the
/// knob marker. It carries the same information as the four-line `render`, at one
/// size throughout, which is what the medium font exists for.
///
/// ```text
/// 2 BASS DI
/// VOL -6.1 DB
/// 2:14.3 120BPM PLAY   K3
/// ```
pub fn render_medium(state: &NavState) -> Frame {
    let mut frame = screen::off();

    screen::draw_text_medium(
        &mut frame,
        0,
        MEDIUM_LINES[0],
        &clip_to(&state.track, MEDIUM_LINE_CHARACTERS),
    );

    let value = format!("{} {}", state.parameter, state.value);
    screen::draw_text_medium(
        &mut frame,
        0,
        MEDIUM_LINES[1],
        &clip_to(&value, MEDIUM_LINE_CHARACTERS),
    );

    let info = format!(
        "{} {}BPM {}",
        state.position,
        state.tempo,
        state.transport.word()
    );
    screen::draw_text_medium(
        &mut frame,
        0,
        MEDIUM_LINES[2],
        &clip_to(&info, MEDIUM_INFO_CHARACTERS),
    );

    if let Some(knob) = state.knob {
        draw_right_medium(&mut frame, MEDIUM_LINES[2], &format!("K{knob}"));
    }

    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::{PAGE_BYTES, PAGES};

    /// Columns with at least one lit pixel in the given line.
    fn lit_columns(frame: &Frame, row: usize) -> usize {
        let page = row / 8;
        frame[page * PAGE_BYTES..(page + 1) * PAGE_BYTES]
            .iter()
            .filter(|byte| **byte != 0x00)
            .count()
    }

    /// Lit pixels in the given line.
    fn lit_pixels(frame: &Frame, row: usize) -> u32 {
        let page = row / 8;
        frame[page * PAGE_BYTES..(page + 1) * PAGE_BYTES]
            .iter()
            .map(|byte| byte.count_ones())
            .sum()
    }

    #[test]
    fn a_render_fills_every_line() {
        let frame = render(&NavState::sample());
        for row in LINES {
            assert!(lit_columns(&frame, row) > 0, "line at row {row} is empty");
        }
        assert_eq!(LINES.len(), PAGES);
    }

    #[test]
    fn a_long_track_name_is_cut_to_one_line() {
        let long = NavState {
            track: "SEVENTEEN CHARACTERS AND MUCH MORE THAN THAT".to_string(),
            ..NavState::sample()
        };
        let cut = NavState {
            track: long.track.chars().take(LINE_CHARACTERS).collect(),
            ..NavState::sample()
        };

        assert_eq!(render(&long), render(&cut));
    }

    #[test]
    fn a_higher_value_fills_more_of_the_bar() {
        let low = NavState {
            value_fraction: 0.1,
            ..NavState::sample()
        };
        let high = NavState {
            value_fraction: 0.9,
            ..NavState::sample()
        };

        let low_bar = render(&low);
        let high_bar = render(&high);
        // The bar's outline is the same in both, so only the number of lit
        // pixels separates them.
        assert!(
            lit_pixels(&high_bar, LINES[1]) > lit_pixels(&low_bar, LINES[1]) + 8,
            "the bar did not grow"
        );
    }

    #[test]
    fn flags_and_transport_change_the_frame() {
        let playing = render(&NavState::sample());
        let stopped = render(&NavState {
            transport: Transport::Stopped,
            ..NavState::sample()
        });
        let muted = render(&NavState {
            mute: true,
            solo: false,
            armed: false,
            ..NavState::sample()
        });

        assert_ne!(playing, stopped);
        assert_ne!(playing, muted);
    }

    #[test]
    fn the_knob_marker_only_shows_when_a_knob_is_touched() {
        let touched = render(&NavState::sample());
        let untouched = render(&NavState {
            knob: None,
            ..NavState::sample()
        });
        assert_ne!(touched, untouched);
    }

    #[test]
    fn the_big_layout_draws_two_lines_at_double_size() {
        let frame = render_big(&NavState::sample());
        assert_eq!(BIG_LINE_CHARACTERS, 10);

        // Line 1 sits at rows 0..13, so it crosses from page 0 into page 1.
        assert!(lit_pixels(&frame, 0) > 0);
        assert!(frame[PAGE_BYTES..2 * PAGE_BYTES]
            .iter()
            .any(|byte| *byte != 0x00));

        // Line 2 sits at rows 16..29, so pages 2 and 3 both carry pixels.
        assert!(frame[2 * PAGE_BYTES..3 * PAGE_BYTES]
            .iter()
            .any(|byte| *byte != 0x00));
        assert!(frame[3 * PAGE_BYTES..].iter().any(|byte| *byte != 0x00));

        // Rows 14 and 15 are the gap between the lines and must stay empty.
        for column in 0..WIDTH {
            assert_eq!(
                frame[PAGE_BYTES + column] & 0b1100_0000,
                0,
                "column {column} bleeds into the gap"
            );
        }
    }

    #[test]
    fn the_big_layout_fits_exactly_ten_characters() {
        let fits = NavState {
            track: "TEN CHARS!".to_string(),
            ..NavState::sample()
        };
        let cut = NavState {
            track: "TEN CHARS! AND MORE".to_string(),
            ..NavState::sample()
        };

        assert_eq!(render_big(&fits), render_big(&cut));
        assert_ne!(render_big(&fits), render_big(&NavState::sample()));
    }

    #[test]
    fn the_big_layout_shows_the_knob_marker() {
        assert_ne!(
            render_big(&NavState::sample()),
            render_big(&NavState {
                knob: None,
                ..NavState::sample()
            })
        );
    }

    #[test]
    fn the_middle_layout_draws_three_lines_with_the_value_in_the_middle() {
        let frame = render_mid(&NavState::sample());

        // Line 1 is small, in page 0.
        assert!(lit_pixels(&frame, 0) > 0);
        // Line 2 is double size at rows 8..21, so it covers pages 1 and 2.
        assert!(frame[PAGE_BYTES..2 * PAGE_BYTES]
            .iter()
            .any(|byte| *byte != 0x00));
        assert!(frame[2 * PAGE_BYTES..3 * PAGE_BYTES]
            .iter()
            .any(|byte| *byte != 0x00));
        // Line 3 is small again, in page 3, and keeps row 31 free.
        assert!(frame[3 * PAGE_BYTES..].iter().any(|byte| *byte != 0x00));
        for column in 0..WIDTH {
            assert_eq!(frame[3 * PAGE_BYTES + column] & 0x80, 0, "row 31 is used");
        }
    }

    #[test]
    fn the_middle_layout_clips_both_sides_of_a_line() {
        let fits = NavState {
            track: "FIFTEEN CHARS!".to_string(),
            ..NavState::sample()
        };
        let cut = NavState {
            track: "FIFTEEN CHARS! AND MORE".to_string(),
            ..NavState::sample()
        };
        assert_eq!(render_mid(&fits), render_mid(&cut));

        // The parameter and the value of the sample state are short enough to be
        // shown whole, so only the knob marker can change the frame.
        assert_ne!(
            render_mid(&NavState::sample()),
            render_mid(&NavState {
                knob: None,
                ..NavState::sample()
            })
        );
    }

    #[test]
    fn the_middle_layout_shows_position_tempo_and_transport() {
        let playing = render_mid(&NavState::sample());
        let stopped = render_mid(&NavState {
            transport: Transport::Stopped,
            position: "12:0.0".to_string(),
            ..NavState::sample()
        });
        assert_ne!(playing, stopped);
    }

    #[test]
    fn the_uniform_medium_layout_draws_three_lines_of_the_medium_font() {
        let frame = render_medium(&NavState::sample());

        // Sixteen characters per line, three lines of nine rows.
        assert_eq!(MEDIUM_LINE_CHARACTERS, 16);

        // Line 1 in page 0, line 2 across pages 1 and 2 (rows 11..19), line 3
        // across pages 2 and 3 (rows 22..30).
        assert!(frame[..PAGE_BYTES].iter().any(|byte| *byte != 0x00));
        assert!(frame[PAGE_BYTES..2 * PAGE_BYTES].iter().any(|byte| *byte != 0x00));
        assert!(frame[2 * PAGE_BYTES..3 * PAGE_BYTES].iter().any(|byte| *byte != 0x00));
        assert!(frame[3 * PAGE_BYTES..].iter().any(|byte| *byte != 0x00));

        // Row 31 stays free, and so do the two rows between the lines.
        for column in 0..WIDTH {
            assert_eq!(frame[3 * PAGE_BYTES + column] & 0x80, 0, "row 31 is used");
        }
    }

    #[test]
    fn the_uniform_medium_layout_clips_a_long_track_name() {
        let fits = NavState {
            track: "SIXTEEN CHARS!!".to_string(),
            ..NavState::sample()
        };
        let cut = NavState {
            track: "SIXTEEN CHARS!! AND MORE".to_string(),
            ..NavState::sample()
        };
        assert_eq!(render_medium(&fits), render_medium(&cut));

        assert_ne!(
            render_medium(&NavState::sample()),
            render_medium(&NavState {
                knob: None,
                ..NavState::sample()
            })
        );
    }
}
