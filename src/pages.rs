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
    /// A shorter form of `left`, for a size that leaves no room for the whole of
    /// it. `None` means the text is simply clipped at its end, as it always was.
    pub short: Option<String>,
}

impl Line {
    pub fn text(left: impl Into<String>) -> Line {
        Line {
            left: left.into(),
            bar: None,
            right: String::new(),
            short: None,
        }
    }

    pub fn two_sided(left: impl Into<String>, right: impl Into<String>) -> Line {
        Line {
            left: left.into(),
            bar: None,
            right: right.into(),
            short: None,
        }
    }

    pub fn with_bar(left: impl Into<String>, fraction: f32, right: impl Into<String>) -> Line {
        Line {
            left: left.into(),
            bar: Some(fraction),
            right: right.into(),
            short: None,
        }
    }

    /// What to draw instead of `left` when the size cannot show all of it.
    ///
    /// It exists for values whose *end* is the part that matters - a time is read
    /// for the seconds, not for the hours - because the alternative is the first
    /// few characters and a reader who cannot tell where the DAW is. An empty
    /// text means there is nothing shorter to say, and the full text is clipped
    /// as before.
    pub fn shortened(mut self, text: impl Into<String>) -> Line {
        let text = text.into();
        self.short = if text.is_empty() { None } else { Some(text) };
        self
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

/// The fader runs from below the marks to the last row of the screen. The mixer
/// used to end in a line of transport and bank text under the bars; the owner
/// asked for that row to go and for the bars to take its rows, so the height is
/// whatever is left below the marks rather than a number of its own.
const FADER_HEIGHT: usize = crate::screen::PAGES * 8 - FADER_ROW;

/// The row the mixer's bottom line used to sit on, kept for the test that it is
/// the faders' row now.
#[cfg(test)]
const TRANSPORT_ROW: usize = 25;

/// How thick the line under the current track's number is. One pixel: two would
/// run into the mute mark, which sits on the row below the number.
const UNDERLINE_HEIGHT: usize = 1;

/// What a page shows, most important line first. A page may return more lines
/// than any size fits - that is the point: the size decides how many are shown.
pub fn lines(page: Page, studio: &Studio) -> Vec<Line> {
    match page {
        Page::Transport => vec![
            Line::two_sided(&studio.position, studio.transport.word())
                .shortened(&studio.position_short),
            Line::text(studio.transport_flags()),
        ],
        Page::Track => vec![
            // The position and the transport come first, because the size
            // decides how many lines fit and these two are what has to be
            // readable while the DAW plays, even at the smallest size.
            Line::two_sided(&studio.position, studio.transport.word())
                .shortened(&studio.position_short),
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
                        if selected { "*" } else { " " },
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

/// The lines a page shows at a size.
///
/// Only the tracks page has use for this: the bank holds eight tracks and a size
/// has room for four, three or two rows, so the list is a *window* that follows
/// the selection and keeps it in the middle, held inside the bank at either end
/// (there is nothing to centre against there). Hardware showed the alternative:
/// the list always started at the bank's first track, so selecting the fifth or
/// sixth pushed the mark off the bottom of the panel.
pub fn lines_at(page: Page, studio: &Studio, size: Size) -> Vec<Line> {
    let all = lines(page, studio);
    if page != Page::Tracks {
        return all;
    }
    let rows = size.line_rows().len();
    let first = window_start(selected_in_bank(studio), all.len(), rows);
    all.into_iter().skip(first).take(rows).collect()
}

/// Where the selected track sits in the bank, when it is in this bank at all.
fn selected_in_bank(studio: &Studio) -> Option<usize> {
    let first = usize::from(studio.bank_first_track);
    let selected = usize::from(studio.track_number).checked_sub(first)?;
    (selected < studio.bank.len()).then_some(selected)
}

/// The first track of a window of `rows` that holds the selected one in the
/// middle, or as close to an end of the bank as the bank allows.
fn window_start(selected: Option<usize>, total: usize, rows: usize) -> usize {
    if rows == 0 || total <= rows {
        return 0;
    }
    let middle = rows - 1 - (rows - 1) / 2;
    match selected {
        Some(selected) => selected.saturating_sub(middle).min(total - rows),
        // The selected track is in another bank: show this bank from its start.
        None => 0,
    }
}

/// What a mixer column says at the top: the channel's number, not its name.
///
/// A column is sixteen pixels wide, which is two characters in the small font -
/// two characters of a name read as a number on a strip where every other number
/// means something, and that is exactly what happened: the owner's first track is
/// called "7 Strings", so the leftmost column said `7` next to the level of the
/// first track, and the mixer looked as though it numbered the channels
/// `7-0-1-2-3-4-5-6`. The number lines up with the faders and with the bank's
/// range on the bottom line; the names are on the tracks page, in full.
fn mixer_label(studio: &Studio, index: usize) -> String {
    format!("{:>2}", usize::from(studio.bank_first_track) + index)
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

/// The margin in front of a track's number on the tracks page: one character,
/// which holds the star when the track is the current one.
const MARGIN_CHARACTERS: usize = 1;

/// Characters a track's number takes on that page, right-aligned in the field
/// after the margin: `format!("{:>2}", number)`.
const NUMBER_CHARACTERS: usize = 2;

/// How thick the line under a number is: one pixel, or two where the glyphs are
/// drawn at double scale and a single pixel would disappear.
fn underline_thickness(size: Size) -> usize {
    if size == Size::Large {
        2
    } else {
        1
    }
}

/// Draws a page at a size.
pub fn render(page: Page, studio: &Studio, size: Size) -> Frame {
    if page == Page::General {
        return mixer(studio);
    }

    let mut frame = screen::off();
    for (index, line) in lines_at(page, studio, size).iter().enumerate() {
        let Some(row) = size.line_row(index) else {
            break;
        };
        draw_line(&mut frame, line, size, row);

        // The current track's number carries a line under it here as well, the
        // way the mixer draws one: the star in the margin says which row, and
        // the line says it again on the number itself, which is what the owner
        // asked for. It is drawn from the number's own field, so it sits under
        // the digits and not under the name.
        if page == Page::Tracks && line.left.starts_with('*') {
            screen::fill_rect(
                &mut frame,
                size.text_width(MARGIN_CHARACTERS),
                row + size.glyph_height(),
                size.text_width(MARGIN_CHARACTERS + NUMBER_CHARACTERS)
                    - size.text_width(MARGIN_CHARACTERS),
                underline_thickness(size),
            );
        }
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

    let (left, left_characters) = fitted_left(line, left_capacity);
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
        let number = usize::from(studio.bank_first_track) + index;

        screen::draw_text(
            &mut frame,
            x + 2,
            LABEL_ROW,
            &clip_to(&mixer_label(studio, index), LABEL_CHARACTERS),
        );

        // The current track's number is underlined. This page says what each
        // track is *doing* - its level, its mute and solo - and nothing on it
        // says which track is *selected*; the number is the only thing here that
        // names a column, so the line goes under it. It follows the bank like the
        // numbers and the faders do: with the bank on 9..16 it is track nine or
        // later that is underlined, never the ninth column.
        if number == usize::from(studio.track_number) {
            screen::fill_rect(
                &mut frame,
                x + 2,
                LABEL_ROW + crate::font::GLYPH_HEIGHT,
                crate::font::text_width(LABEL_CHARACTERS),
                UNDERLINE_HEIGHT,
            );
        }

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

/// The left text that fits, and how many characters it took.
///
/// A line may carry a shorter form for the case where the whole of it does not
/// fit; that form is used instead of clipping, and is itself clipped if even it
/// is too long. A line without one is clipped at its end exactly as before.
fn fitted_left(line: &Line, limit: usize) -> (String, usize) {
    if line.left.chars().count() <= limit {
        return fit(&line.left, limit);
    }
    match &line.short {
        Some(short) => fit(short, limit),
        None => fit(&line.left, limit),
    }
}

fn clip_to(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::{PAGE_BYTES, PAGES};
    use crate::studio::Transport;

    #[test]
    fn the_track_page_leads_with_the_position_and_the_transport() {
        // What the owner asked to see while the DAW plays: where it is and
        // whether it is moving, before anything else on the page.
        let studio = Studio::sample();
        let lines = lines(Page::Track, &studio);
        assert_eq!(lines[0].left, studio.position);
        assert_eq!(lines[0].right, studio.transport.word());
        assert!(
            studio.position.chars().count() + studio.transport.word().chars().count() + 1
                <= Size::Medium.line_characters(),
            "position and transport have to fit one line at the medium size"
        );
    }

    #[test]
    fn a_narrow_size_shows_the_end_of_the_position_rather_than_its_start() {
        // At the large size the whole time does not fit beside the transport
        // word. What is drawn then is the short form - the seconds, with the
        // minutes in front - and never the first few characters, which would drop
        // the only part that says where the DAW is.
        let mut page_lines = lines(Page::Track, &Studio::sample());
        let line = page_lines.remove(0);
        let capacity = Size::Large
            .line_characters()
            .saturating_sub(line.right.chars().count())
            .saturating_sub(1);

        assert!(
            line.left.chars().count() > capacity,
            "the whole time must not fit at the large size, or this proves nothing"
        );
        assert_eq!(fitted_left(&line, capacity).0, "3:24");
    }

    #[test]
    fn a_line_without_a_short_form_is_clipped_at_its_end_as_before() {
        let line = Line::two_sided("A NAME TOO LONG TO DRAW", "MS");
        assert_eq!(fitted_left(&line, 4).0, "A NA");
    }

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
    fn the_tracks_page_keeps_the_selected_track_in_the_middle_of_its_window() {
        let mut studio = Studio::sample();
        let total = studio.bank.len();
        for size in [Size::Small, Size::Medium, Size::Large] {
            let rows = size.line_rows().len();
            for number in 1..=8u16 {
                studio.track_number = number;
                let window = lines_at(Page::Tracks, &studio, size);
                assert_eq!(window.len(), rows, "a full window at every size");

                let marked = window
                    .iter()
                    .position(|line| line.left.starts_with('*'))
                    .unwrap_or_else(|| {
                        panic!("track {number} is selected and the window does not show it")
                    });
                let inside = usize::from(number) - usize::from(studio.bank_first_track);
                let start = inside - marked;
                // Either the window is held against an end of the bank, or the
                // selected track is one of the two middle rows.
                let middle = marked == (rows - 1) / 2 || marked == rows - 1 - (rows - 1) / 2;
                assert!(
                    start == 0 || start + rows == total || middle,
                    "track {number}: row {marked} of {rows}, window from {start}, neither middle nor at an end"
                );
            }
        }

        // The ends of the bank: the window stops there instead of running past.
        studio.track_number = 1;
        assert!(
            lines_at(Page::Tracks, &studio, Size::Medium)[0]
                .left
                .starts_with('*'),
            "the first track is the first line when it is selected"
        );
        studio.track_number = 8;
        let window = lines_at(Page::Tracks, &studio, Size::Medium);
        assert!(
            window[2].left.starts_with('*'),
            "and the last track is the last line"
        );
    }

    #[test]
    fn a_bank_the_selection_is_not_in_shows_from_its_first_track() {
        let mut studio = Studio::sample();
        studio.track_number = 13; // another bank entirely
        let window = lines_at(Page::Tracks, &studio, Size::Small);
        assert_eq!(window.len(), 4);
        assert!(window[0].left.contains(" 1 "), "the bank still shows from its start");
        assert!(!window.iter().any(|line| line.left.starts_with('*')));
    }

    #[test]
    fn the_tracks_page_numbers_from_where_the_bank_starts() {
        let mut studio = Studio::sample();
        // What the DAW's assignment display said on hardware: the surface is on
        // tracks 9..16, and the twelfth track is the selected one.
        studio.bank_first_track = 9;
        studio.track_number = 12;

        let window = lines_at(Page::Tracks, &studio, Size::Small);
        assert!(
            window.iter().any(|line| line.left.starts_with("*12 ")),
            "the selected track is numbered from the bank, not from one: {:?}",
            window.iter().map(|line| line.left.as_str()).collect::<Vec<_>>()
        );
        assert!(
            window[0].left.contains("10 "),
            "the first line of the window is track ten: {:?}",
            window[0].left
        );
    }

    /// The marker is a star, and a star both fonts have: `>` was the marker
    /// before, and the 7x9 font the medium size draws with has no `>` - so the
    /// panel showed the `?` a font falls back to, next to the selected track.
    #[test]
    fn the_tracks_page_marks_the_selected_track_with_a_star() {
        let studio = Studio::sample();
        let marked: Vec<String> = lines(Page::Tracks, &studio)
            .iter()
            .map(|line| line.left.clone())
            .collect();
        assert!(
            marked[1].starts_with('*'),
            "the selected track is not marked: {:?}",
            marked[1]
        );
        assert!(!marked[0].starts_with('*'));
        assert_eq!(marked[1], "* 2 BASS DI");
    }

    /// The mixer numbers its columns the way the faders are wired, so the
    /// leftmost number belongs to the leftmost level indicator.
    #[test]
    fn the_mixer_numbers_its_columns_like_the_faders() {
        let mut studio = Studio::sample();
        studio.bank_first_track = 9;
        assert_eq!(mixer_label(&studio, 0), " 9");
        assert_eq!(mixer_label(&studio, 7), "16");
        studio.bank_first_track = 1;
        assert_eq!(mixer_label(&studio, 0), " 1");
    }

    /// The mixer underlines the current track's number, and the line follows the
    /// bank exactly as the numbers and the faders do: with the bank on 9..16 it is
    /// track nine and later that carries it, never the ninth column.
    #[test]
    fn the_mixer_underlines_the_current_tracks_number() {
        let column = WIDTH / COLUMNS;
        let underline_row = LABEL_ROW + crate::font::GLYPH_HEIGHT;
        let under = |frame: &Frame, index: usize| {
            let x = index * column + 2;
            (x..x + crate::font::text_width(LABEL_CHARACTERS))
                .all(|x| frame[(underline_row / 8) * PAGE_BYTES + x] & (1 << (underline_row % 8)) != 0)
        };

        let mut studio = Studio::sample();
        studio.bank_first_track = 1;
        studio.track_number = 3;
        let frame = render(Page::General, &studio, Size::Medium);
        assert!(
            under(&frame, 2),
            "the current track's number has no line under it"
        );
        assert!(!under(&frame, 1), "a number that is not current has one");

        studio.bank_first_track = 9;
        studio.track_number = 12;
        let frame = render(Page::General, &studio, Size::Medium);
        assert!(
            under(&frame, 3),
            "track twelve is the third of the bank nine to sixteen"
        );
        assert!(
            !under(&frame, 0),
            "the line is under track twelve, not under the ninth column"
        );

        // A selection in another bank leaves every number alone.
        studio.track_number = 2;
        let frame = render(Page::General, &studio, Size::Medium);
        assert!((0..COLUMNS).all(|index| !under(&frame, index)));
    }

    /// The tracks page underlines the current track's number too: the star in the
    /// margin says which row, and the line says it again on the number, at every
    /// size and under the digits rather than under the name.
    #[test]
    fn the_tracks_page_underlines_the_current_tracks_number() {
        let mut studio = Studio::sample();
        studio.track_number = 3;

        for size in [Size::Small, Size::Medium, Size::Large] {
            let frame = render(Page::Tracks, &studio, size);
            let shown = lines_at(Page::Tracks, &studio, size);
            let marked = shown
                .iter()
                .position(|line| line.left.starts_with('*'))
                .unwrap_or_else(|| panic!("the current track is not shown at {size:?}"));
            let row = size.line_row(marked).unwrap() + size.glyph_height();
            let lit = |x: usize| frame[(row / 8) * PAGE_BYTES + x] & (1 << (row % 8)) != 0;

            assert!(
                (size.text_width(1)..size.text_width(3)).all(lit),
                "the number has no line under it at {size:?}"
            );
            assert!(
                !lit(size.text_width(3)),
                "the line runs past the number at {size:?}"
            );
        }
    }

    /// A font with no glyph for a character draws `?` instead, and that is how
    /// the selected-track marker came to look like a question mark at medium
    /// size. Everything the pages write *themselves* has to be drawable in both
    /// fonts - the DAW's own track names are the one thing we cannot promise.
    #[test]
    fn every_character_the_pages_write_themselves_is_in_both_fonts() {
        let written = [
            "* - . : / 0123456789",
            "VOL PAN",
            "M S ARM - - --- MS M- -S --",
            "PLAY STOP REC PAUSE LOOP CLICK",
            "WAITING FOR DAW",
        ];
        for text in written {
            for character in text.chars() {
                assert_ne!(
                    crate::font::glyph(character),
                    crate::font::glyph('?'),
                    "{character:?} has no glyph in the 5x7 font"
                );
                assert_ne!(
                    crate::font_medium::glyph(character),
                    crate::font_medium::glyph('?'),
                    "{character:?} has no glyph in the 7x9 font"
                );
            }
        }
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

        let fader_pages = (
            FADER_ROW / 8,
            ((FADER_ROW + FADER_HEIGHT) / 8 + 1).min(PAGES),
        );
        let low_pixels = lit_pixels(&low, fader_pages.0, fader_pages.1);
        let full_pixels = lit_pixels(&full, fader_pages.0, fader_pages.1);
        assert!(
            full_pixels > low_pixels,
            "a full fader ({full_pixels}) is not bigger than an empty one ({low_pixels})"
        );
    }

    /// The faders take the rows the mixer's bottom line used to have, and there is
    /// no line under them: the owner asked for the row of transport and bank text
    /// to go and for the bars to grow into it.
    #[test]
    fn the_general_page_gives_the_faders_the_bottom_rows() {
        let mut studio = Studio::sample();
        studio.bank[0].fader = 1.0;
        studio.bank[0].mute = false;
        studio.bank[0].solo = false;
        let frame = mixer(&studio);

        let column = WIDTH / COLUMNS;
        let lit = |x: usize, y: usize| frame[(y / 8) * PAGE_BYTES + x] & (1 << (y % 8)) != 0;
        let fader = |index: usize| {
            let x = index * column + (column - FADER_WIDTH) / 2;
            x..x + FADER_WIDTH
        };

        // A full fader reaches the last row of the screen: there is no bottom edge
        // for it to stop a row short of.
        for y in FADER_ROW..PAGES * 8 {
            assert!(
                fader(0).all(|x| lit(x, y)),
                "the first column's fader leaves row {y} empty"
            );
        }

        // And nothing but faders is written down there any more.
        for y in TRANSPORT_ROW..PAGES * 8 {
            for x in 0..WIDTH {
                if lit(x, y) {
                    assert!(
                        (0..COLUMNS).any(|index| fader(index).contains(&x)),
                        "row {y} has ink at {x}, which no fader covers"
                    );
                }
            }
        }
    }

    #[test]
    fn the_general_page_survives_a_bank_that_is_not_full() {
        let studio = Studio {
            bank: vec![Channel {
                name: "ONE".to_string(),
                fader: 0.5,
                pan: 0.5,
                pan_known: true,
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
