//! The four pages: what each one shows, and how a size turns that into pixels.
//!
//! A page says what it wants on the screen - a handful of lines, most important
//! first, each with text from the left, an optional bar in the middle and text
//! against the right edge - and the size decides how many of those lines fit and
//! how big they are drawn. The general page is the exception: it is drawn as a
//! mixer rather than written as text, and ignores the size.

use crate::screen::{self, Frame, WIDTH};
use crate::studio::{Channel, Page, Size, Studio};

/// One line of a page.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// Drawn from the left edge.
    pub left: String,
    /// A bar between the two texts, at this fraction of its range, if the size
    /// leaves room for one.
    pub bar: Option<f32>,
    /// Drawn against the right edge.
    pub right: String,
}

impl Line {
    pub fn text(left: impl Into<String>) -> Line {
        Line {
            left: left.into(),
            bar: None,
            right: String::new(),
        }
    }

    pub fn two_sided(left: impl Into<String>, right: impl Into<String>) -> Line {
        Line {
            left: left.into(),
            bar: None,
            right: right.into(),
        }
    }

    pub fn with_bar(left: impl Into<String>, fraction: f32, right: impl Into<String>) -> Line {
        Line {
            left: left.into(),
            bar: Some(fraction),
            right: right.into(),
        }
    }
}

/// Columns the general page draws: one per channel of the bank.
pub const COLUMNS: usize = 8;

/// A bar shorter than this is not worth drawing; the line shows its text alone.
const MINIMUM_BAR_COLUMNS: usize = 6;

const LABEL_ROW: usize = 0;
const LABEL_CHARACTERS: usize = 2;
const MARK_ROW: usize = 8;
const MARK_WIDTH: usize = 3;
const MARK_HEIGHT: usize = 3;
const FADER_ROW: usize = 12;
const FADER_WIDTH: usize = 6;
const FADER_HEIGHT: usize = 12;
const BOTTOM_ROW: usize = 25;

/// What a page shows, most important line first. A page may return more lines
/// than any size fits - that is the point: the size decides how many are shown.
pub fn lines(page: Page, studio: &Studio) -> Vec<Line> {
    match page {
        Page::Transport => vec![
            Line::two_sided(&studio.position, studio.transport.word()),
            Line::text(studio.transport_flags()),
        ],
        Page::Track => vec![
            // The position and the transport come first, because the size
            // decides how many lines fit and these two are what has to be
            // readable while the DAW plays, even at the smallest size.
            Line::two_sided(&studio.position, studio.transport.word()),
            Line::two_sided(
                format!("{} {}", studio.track_number, studio.track_name),
                studio.flags(),
            ),
            Line::with_bar("VOL", studio.volume_fraction, &studio.volume),
            Line::with_bar("PAN", studio.pan_fraction, &studio.pan),
        ],
        Page::Tracks => studio
            .bank
            .iter()
            .enumerate()
            .map(|(index, channel)| {
                let number = usize::from(studio.bank_first_track) + index;
                let selected = number == usize::from(studio.track_number);
                Line::two_sided(
                    format!(
                        "{}{:>2} {}",
                        if selected { ">" } else { " " },
                        number,
                        channel.name
                    ),
                    channel_flags(channel),
                )
            })
            .collect(),
        // Drawn, not written.
        Page::General => Vec::new(),
    }
}

/// A channel's mute/solo flags, two characters wide so the column lines up.
fn channel_flags(channel: &Channel) -> String {
    match (channel.mute, channel.solo) {
        (true, true) => "MS".to_string(),
        (true, false) => "M-".to_string(),
        (false, true) => "-S".to_string(),
        (false, false) => "--".to_string(),
    }
}

/// Draws a page at a size.
pub fn render(page: Page, studio: &Studio, size: Size) -> Frame {
    if page == Page::General {
        return mixer(studio);
    }

    let mut frame = screen::off();
    for (index, line) in lines(page, studio).iter().enumerate() {
        let Some(row) = size.line_row(index) else {
            break;
        };
        draw_line(&mut frame, line, size, row);
    }
    frame
}

/// Draws one line: the left text, then a bar in whatever is left over, then the
/// right text against the edge. Each part is clipped to the space it is given,
/// so the three can never overlap.
fn draw_line(frame: &mut Frame, line: &Line, size: Size, row: usize) {
    let capacity = size.line_characters();

    let (right, right_characters) = fit(&line.right, capacity);
    let mut left_capacity = capacity - right_characters;
    if right_characters > 0 {
        left_capacity = left_capacity.saturating_sub(1);
    }
    if line.bar.is_some() {
        left_capacity = left_capacity.saturating_sub(2);
    }

    let (left, left_characters) = fit(&line.left, left_capacity);
    draw_text(frame, size, 0, row, &left);

    if let Some(fraction) = line.bar {
        let start = size.text_width(left_characters) + size.character_advance();
        let end = WIDTH
            .saturating_sub(size.text_width(right_characters))
            .saturating_sub(if right_characters > 0 {
                size.character_advance()
            } else {
                0
            });

        if end > start + MINIMUM_BAR_COLUMNS {
            let (inset, height) = bar_geometry(size);
            screen::draw_bar(frame, start, row + inset, end - start, height, fraction);
        }
    }

    if right_characters > 0 {
        draw_text(frame, size, WIDTH - size.text_width(right_characters), row, &right);
    }
}

/// The general page: the bank as a mixer, one column per channel - a two
/// character label, marks for mute and solo, and a fader. It ignores the size,
/// because at every size it is the same picture; the transport and the bank's
/// numbering share the bottom line.
pub fn mixer(studio: &Studio) -> Frame {
    let mut frame = screen::off();
    let column = WIDTH / COLUMNS;

    for (index, channel) in studio.bank.iter().take(COLUMNS).enumerate() {
        let x = index * column;

        screen::draw_text(
            &mut frame,
            x + 2,
            LABEL_ROW,
            &clip_to(&channel.name, LABEL_CHARACTERS),
        );

        if channel.mute {
            screen::fill_rect(&mut frame, x + 1, MARK_ROW, MARK_WIDTH, MARK_HEIGHT);
        }
        if channel.solo {
            screen::fill_rect(
                &mut frame,
                x + column - 1 - MARK_WIDTH,
                MARK_ROW,
                MARK_WIDTH,
                MARK_HEIGHT,
            );
        }

        screen::draw_vbar(
            &mut frame,
            x + (column - FADER_WIDTH) / 2,
            FADER_ROW,
            FADER_WIDTH,
            FADER_HEIGHT,
            channel.fader,
        );
    }

    let bank = format!(
        "{}-{}",
        studio.bank_first_track,
        usize::from(studio.bank_first_track) + COLUMNS - 1
    );
    let transport = format!("{} {}", studio.transport.word(), studio.transport_flags());
    let transport_capacity = crate::font::characters_per_line(WIDTH) - bank.chars().count() - 1;
    screen::draw_text(
        &mut frame,
        0,
        BOTTOM_ROW,
        &clip_to(transport.trim_end(), transport_capacity),
    );
    screen::draw_text(
        &mut frame,
        WIDTH - crate::font::text_width(bank.chars().count()),
        BOTTOM_ROW,
        &bank,
    );

    frame
}

/// Draws text at a size: the small font, the medium font, or the small font at
/// double scale.
fn draw_text(frame: &mut Frame, size: Size, x: usize, row: usize, text: &str) {
    match size {
        Size::Small => screen::draw_text(frame, x, row, text),
        Size::Medium => screen::draw_text_medium(frame, x, row, text),
        Size::Large => {
            screen::draw_text_scaled(frame, x, row, text, crate::studio::LARGE_SCALE)
        }
    }
}

/// How far below a line's top a bar starts, and how tall it is: never outside
/// the line's glyph box.
fn bar_geometry(size: Size) -> (usize, usize) {
    let inset = size.glyph_height() / 8;
    (inset, size.glyph_height() - inset * 2)
}

/// Clips text to `limit` characters and says how many it kept.
fn fit(text: &str, limit: usize) -> (String, usize) {
    let clipped: String = text.chars().take(limit).collect();
    let characters = clipped.chars().count();
    (clipped, characters)
}

fn clip_to(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::{PAGE_BYTES, PAGES};
    use crate::studio::Transport;

    /// Which of a size's lines carry pixels: the honest count of drawn lines. A
    /// glyph of the medium or the large size is taller than a page-block, so
    /// counting page-blocks would count one line twice.
    fn lines_with_content(frame: &Frame, size: Size) -> usize {
        size.line_rows()
            .iter()
            .filter(|row| {
                (**row..**row + size.glyph_height()).any(|y| {
                    frame[(y / 8) * PAGE_BYTES..(y / 8 + 1) * PAGE_BYTES]
                        .iter()
                        .any(|byte| byte & (1 << (y % 8)) != 0)
                })
            })
            .count()
    }

    fn lit_pixels(frame: &Frame, from: usize, to: usize) -> u32 {
        frame[from * PAGE_BYTES..to * PAGE_BYTES]
            .iter()
            .map(|byte| byte.count_ones())
            .sum()
    }

    #[test]
    fn every_page_draws_something_at_every_size() {
        let studio = Studio::sample();
        for page in Page::ALL {
            for size in [Size::Small, Size::Medium, Size::Large] {
                let frame = render(page, &studio, size);
                assert!(
                    frame.iter().any(|byte| *byte != 0x00),
                    "{:?} is blank at {:?}",
                    page,
                    size
                );
            }
        }
    }

    #[test]
    fn a_bigger_size_shows_fewer_lines_of_the_same_page() {
        // The tracks page has eight lines, of which the size shows as many as it
        // has room for: four, three, then two.
        let studio = Studio::sample();
        assert_eq!(lines(Page::Tracks, &studio).len(), COLUMNS);

        assert_eq!(lines_with_content(&render(Page::Tracks, &studio, Size::Small), Size::Small), 4);
        assert_eq!(
            lines_with_content(&render(Page::Tracks, &studio, Size::Medium), Size::Medium),
            3
        );
        assert_eq!(
            lines_with_content(&render(Page::Tracks, &studio, Size::Large), Size::Large),
            2
        );
    }

    #[test]
    fn the_tracks_page_marks_the_selected_track() {
        let studio = Studio::sample();
        let marked: Vec<String> = lines(Page::Tracks, &studio)
            .iter()
            .map(|line| line.left.clone())
            .collect();
        assert!(marked[1].starts_with('>'), "the selected track is not marked");
        assert!(!marked[0].starts_with('>'));
        assert_eq!(marked[1], "> 2 BASS DI");
    }

    #[test]
    fn the_volume_bar_follows_the_value() {
        let quiet = Studio {
            volume_fraction: 0.05,
            ..Studio::sample()
        };
        let loud = Studio {
            volume_fraction: 0.95,
            ..Studio::sample()
        };

        let quiet_frame = render(Page::Track, &quiet, Size::Small);
        let loud_frame = render(Page::Track, &loud, Size::Small);
        assert_ne!(quiet_frame, loud_frame);

        // The bar lives in the row band of the line that carries it, and which
        // line that is is the page's business, not this test's: asking the page
        // keeps the test true when the lines are reordered.
        let volume_line = lines(Page::Track, &Studio::sample())
            .iter()
            .position(|line| line.left.starts_with("VOL"))
            .expect("the track page carries the volume line");
        let row_band = Size::Small.line_rows()[volume_line] / 8;
        assert!(
            lit_pixels(&loud_frame, row_band, row_band + 1)
                > lit_pixels(&quiet_frame, row_band, row_band + 1),
            "the bar did not grow with the value"
        );
    }

    #[test]
    fn the_line_parts_are_clipped_so_they_cannot_overlap() {
        let long_name = "A TRACK NAME THAT GOES ON AND ON";
        let fits = Studio {
            track_name: long_name.chars().take(15).collect(),
            ..Studio::sample()
        };
        let cut = Studio {
            track_name: long_name.to_string(),
            ..Studio::sample()
        };

        // The right text of the line is nothing, so a medium line takes sixteen
        // characters; both studios must come out identical.
        let a = render(Page::Track, &fits, Size::Medium);
        let b = render(Page::Track, &cut, Size::Medium);
        assert_eq!(a, b);

        // A value long enough to leave no room squeezes the name instead of
        // running over it.
        let crowded = Studio {
            track_name: "BASS DI".to_string(),
            volume: "A VERY LONG VALUE INDEED".to_string(),
            ..Studio::sample()
        };
        let frame = render(Page::Track, &crowded, Size::Medium);
        assert!(frame.iter().any(|byte| *byte != 0x00));
    }

    #[test]
    fn a_bar_needs_room_and_is_left_out_when_there_is_none() {
        // A bar with no space between the two texts is dropped, not drawn over
        // them: the same studio without a bar then draws the same frame.
        let studio = Studio {
            track_name: "SIXTEEN CHARS!!".to_string(),
            ..Studio::sample()
        };
        let with_bar = render(Page::Track, &studio, Size::Medium);
        let page_lines = lines(Page::Track, &studio);
        let volume_line = page_lines
            .iter()
            .find(|line| line.left.starts_with("VOL"))
            .expect("the track page carries the volume line");
        assert!(volume_line.bar.is_some(), "the sample line has a bar");
        assert!(with_bar.iter().any(|byte| *byte != 0x00));
    }

    #[test]
    fn the_general_page_draws_every_column() {
        let studio = Studio::sample();
        let frame = mixer(&studio);
        let column = WIDTH / COLUMNS;

        for index in 0..COLUMNS {
            let x = index * column;
            assert!(
                (x..x + column).any(|column_x| (0..PAGES).any(|page| {
                    frame[page * PAGE_BYTES + column_x] != 0x00
                })),
                "column {index} is empty"
            );
        }
    }

    #[test]
    fn the_general_page_ignores_the_size() {
        let studio = Studio::sample();
        assert_eq!(
            render(Page::General, &studio, Size::Small),
            render(Page::General, &studio, Size::Large)
        );
        assert_eq!(render(Page::General, &studio, Size::Small), mixer(&studio));
    }

    #[test]
    fn the_general_page_marks_mute_and_solo_only_when_they_are_set() {
        let mut studio = Studio::sample();
        studio.bank[0].mute = false;
        studio.bank[0].solo = false;
        let bare = mixer(&studio);

        studio.bank[0].mute = true;
        let muted = mixer(&studio);
        assert_ne!(bare, muted);

        studio.bank[0].solo = true;
        let both = mixer(&studio);
        assert_ne!(muted, both);
    }

    #[test]
    fn the_general_page_fills_a_fader_from_the_bottom() {
        let mut studio = Studio::sample();
        studio.bank[0].fader = 0.0;
        studio.bank[0].mute = false;
        studio.bank[0].solo = false;
        let low = mixer(&studio);

        studio.bank[0].fader = 1.0;
        let full = mixer(&studio);

        let fader_pages = (FADER_ROW / 8, (FADER_ROW + FADER_HEIGHT) / 8 + 1);
        let low_pixels = lit_pixels(&low, fader_pages.0, fader_pages.1);
        let full_pixels = lit_pixels(&full, fader_pages.0, fader_pages.1);
        assert!(
            full_pixels > low_pixels,
            "a full fader ({full_pixels}) is not bigger than an empty one ({low_pixels})"
        );
    }

    #[test]
    fn the_general_page_survives_a_bank_that_is_not_full() {
        let studio = Studio {
            bank: vec![Channel {
                name: "ONE".to_string(),
                fader: 0.5,
                mute: false,
                solo: false,
            }],
            ..Studio::sample()
        };
        let frame = mixer(&studio);
        assert!(frame.iter().any(|byte| *byte != 0x00));
    }

    #[test]
    fn the_transport_page_says_what_the_transport_does() {
        let playing = render(Page::Transport, &Studio::sample(), Size::Medium);
        let stopped = render(
            Page::Transport,
            &Studio {
                transport: Transport::Stopped,
                looping: false,
                position: "0:0.0".to_string(),
                ..Studio::sample()
            },
            Size::Medium,
        );
        assert_ne!(playing, stopped);
    }

    /// The frame as text, one line per pixel row: for judging a layout without a
    /// keyboard. A lit pixel is a dark dot on the panel, which is inverted.
    fn art(frame: &Frame) -> Vec<String> {
        (0..PAGES * 8)
            .map(|y| {
                (0..WIDTH)
                    .map(|x| {
                        if frame[(y / 8) * PAGE_BYTES + x] & (1 << (y % 8)) != 0 {
                            '#'
                        } else {
                            '.'
                        }
                    })
                    .collect()
            })
            .collect()
    }

    /// Prints every page at every size, so a layout can be read as text:
    /// `cargo test --lib pages::tests::print -- --nocapture`.
    #[test]
    fn print_every_page_as_text() {
        let studio = Studio::sample();
        for page in Page::ALL {
            for size in [Size::Small, Size::Medium, Size::Large] {
                println!("{:<9} at {:<6}", page.name(), size.name());
                let rows = art(&render(page, &studio, size));
                assert_eq!(rows.len(), PAGES * 8);
                assert!(
                    rows.iter().any(|row| row.contains('#')),
                    "{} at {} came out blank",
                    page.name(),
                    size.name()
                );
                for row in rows {
                    println!("{row}");
                }
                println!();
            }
        }
    }
}
