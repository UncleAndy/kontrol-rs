//! The display of the A-series keyboards, as described in
//! `doc/kontrol_usb_re.cpp` (and guessed at in `doc/mini-screen.txt`).
//!
//! The frame is 128x32 pixels, one bit each, and the numbers fit exactly: four
//! pages of 128 bytes = 512 bytes, which is what the C example's two reports
//! carry (`e0 00 00 <page> 00 80 00 02 00` + 256 bytes, with `<page>` 0 and 2).
//! Inside a page, byte N is column N and its bit 0 is the top row of those eight
//! rows, so a report covers two pages = sixteen rows.
//!
//! Consequences that earlier probes got wrong, and that matter:
//!
//! * the panel is **inverted**: a set bit is a DARK pixel, a clear bit a lit one.
//!   Measured on hardware: all-zero bytes light the whole screen, all-`0xff`
//!   bytes make it black, and `0xff` in the left columns leaves the left half
//!   black. Frames in this module use the natural convention (a set bit is a lit
//!   pixel) and the inversion happens when the report is built, so that `on()`
//!   really means "everything lit";
//! * only offsets 0 and 2 are meaningful; other values are not page numbers and
//!   may put the controller into a service state (the "screen stays white
//!   whatever I write" symptom);
//! * the first 128 bytes of a report are a *page*, i.e. a horizontal band, not
//!   the left half; halves of the *width* are ranges of columns inside every
//!   page.
//!
//! `-S <state>` shows one named state and waits, so a state can be looked at
//! (or photographed) and repeated at will; `-s` walks a few coarse states and
//! asks about them.

use crate::hid::Hid;
use crate::hid_dump::Dump;

/// Screen width in pixels, i.e. columns per page.
pub const WIDTH: usize = 128;
/// A byte covers 8 rows; four pages fill the screen.
pub const PAGES: usize = 4;
/// Bytes per page, one per column.
pub const PAGE_BYTES: usize = WIDTH;
/// A complete frame: four pages, 512 bytes.
pub const FRAME_BYTES: usize = PAGES * PAGE_BYTES;
/// Payload of one report: two pages, i.e. sixteen rows.
pub const REPORT_PAYLOAD: usize = 2 * PAGE_BYTES;
/// The two offsets a report may carry: pages 0-1 and pages 2-3.
pub const REPORT_OFFSETS: [u8; 2] = [0, 2];

/// Report ID and the header bytes the device expects before the pixels. Byte 3
/// is replaced by the offset (first page) the report carries.
pub const REPORT_HEADER: [u8; 9] = [0xe0, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x02, 0x00];

/// The device's "disable local controls", the first thing in the C example.
pub const DISABLE_LOCAL_CONTROLS: [u8; 3] = [0xa0, 0x03, 0x04];
/// What `komplement` normally sends at start: back to "MIDI mode".
pub const MIDI_MODE: [u8; 3] = [0xa0, 0x07, 0x00];

pub type Frame = [u8; FRAME_BYTES];

/// The states `-S` knows, in the order the help lists them. The four `page-`
/// states are the real pages and take a size after a colon; the four `nav`
/// states are the earlier drafts of the same screens.
pub const STATE_NAMES: [&str; 21] = [
    "on",
    "off",
    "top",
    "bottom",
    "left",
    "right",
    "vstripes",
    "hstripes",
    "border",
    "message",
    "waiting",
    "nav",
    "navmed",
    "navmid",
    "navbig",
    "page-transport",
    "page-track",
    "page-tracks",
    "page-general",
    "glyphs",
    "text60",
];

pub fn off() -> Frame {
    [0x00; FRAME_BYTES]
}

pub fn on() -> Frame {
    [0xff; FRAME_BYTES]
}

/// One report carrying the two pages that start at `first_page`. The panel is
/// inverted, so the bits are complemented here: a lit pixel in a frame becomes a
/// clear bit on the wire.
pub fn report(first_page: usize, frame: &Frame) -> Vec<u8> {
    let start = first_page * PAGE_BYTES;

    let mut report = REPORT_HEADER.to_vec();
    report[3] = first_page as u8;
    report.extend(frame[start..start + REPORT_PAYLOAD].iter().map(|byte| !byte));
    report
}

/// The two reports that put `frame` on the screen: pages 0-1, then pages 2-3.
pub fn frame_reports(frame: &Frame) -> [Vec<u8>; 2] {
    [report(0, frame), report(2, frame)]
}

/// Every column of every page gets `mask`: `0xff` lights a column through all
/// eight rows of its page.
fn columns(mask: u8) -> Frame {
    [mask; FRAME_BYTES]
}

/// The left or right half of the *width*: a range of columns inside every page.
pub fn half_width(left: bool) -> Frame {
    let mut frame = off();
    let range = if left { 0..WIDTH / 2 } else { WIDTH / 2..WIDTH };

    for page in 0..PAGES {
        for column in range.clone() {
            frame[page * PAGE_BYTES + column] = 0xff;
        }
    }

    frame
}

/// The upper or lower half of the *height*: whole pages, i.e. horizontal bands.
pub fn half_height(top: bool) -> Frame {
    let mut frame = off();
    let range = if top { 0..2 } else { 2..PAGES };

    for page in range {
        frame[page * PAGE_BYTES..(page + 1) * PAGE_BYTES].fill(0xff);
    }

    frame
}

/// Wide vertical stripes, four columns on and four off, through all pages.
pub fn vertical_stripes() -> Frame {
    let mut frame = off();

    for page in 0..PAGES {
        for column in 0..WIDTH {
            frame[page * PAGE_BYTES + column] = if (column / 4) % 2 == 0 { 0xff } else { 0x00 };
        }
    }

    frame
}

/// Horizontal stripes: the lower four rows of every eight-row page.
pub fn horizontal_stripes() -> Frame {
    columns(0x0f)
}

/// A one-pixel rectangle around the whole screen: it shows the exact bounds and
/// which way round the screen is.
pub fn border() -> Frame {
    let mut frame = off();

    // Top edge: the top row of the first page. Bottom edge: the bottom row of the
    // last page. Bit 0 of a byte is the top row of its page, bit 7 the last.
    for column in 0..WIDTH {
        frame[column] |= 0x01;
        frame[3 * PAGE_BYTES + column] |= 0x80;
    }

    // Left and right edges: full columns in every page.
    for page in 0..PAGES {
        frame[page * PAGE_BYTES] = 0xff;
        frame[page * PAGE_BYTES + WIDTH - 1] = 0xff;
    }

    frame
}

/// Draws text into the frame: one byte per column, bit 0 the top row, so `row`
/// is a pixel row. Glyphs that run past the right edge are cut; text never wraps.
pub fn draw_text(frame: &mut Frame, x: usize, row: usize, text: &str) {
    draw_text_scaled(frame, x, row, text, 1);
}

/// The same, with every glyph pixel drawn `scale` times as wide and as tall.
/// The bits are placed row by row (not inside one page) so a glyph may cross a
/// page boundary - which the double-size lines of the navigation screen do.
pub fn draw_text_scaled(frame: &mut Frame, x: usize, row: usize, text: &str, scale: usize) {
    if scale == 0 || row >= PAGES * 8 {
        return;
    }

    let glyph_height = crate::font::GLYPH_HEIGHT;
    let mut column = x;

    for character in text.chars() {
        for glyph_column in crate::font::glyph(character) {
            for sx in 0..scale {
                let target = column + sx;
                if target >= WIDTH {
                    continue;
                }

                for r in 0..glyph_height {
                    if glyph_column & (1 << r) == 0 {
                        continue;
                    }

                    for sy in 0..scale {
                        let y = row + r * scale + sy;
                        if y < PAGES * 8 {
                            frame[(y / 8) * PAGE_BYTES + target] |= 1 << (y % 8);
                        }
                    }
                }
            }
            column += scale;
        }
        column += crate::font::GLYPH_SPACING * scale;
    }
}

/// Draws a bar: an outline with a filled part, in pixels. `fraction` is where the
/// value sits in its range, so a bar of a parameter at 0.62 is filled a little
/// over half way. The bar may cross a page boundary - a line of the medium or
/// large size starts at a row that is not a multiple of eight.
pub fn draw_bar(frame: &mut Frame, x: usize, row: usize, width: usize, height: usize, fraction: f32) {
    if width == 0 || height == 0 || row >= PAGES * 8 || x >= WIDTH {
        return;
    }

    let fraction = fraction.clamp(0.0, 1.0);
    let filled = ((width as f32 - 2.0) * fraction).round() as usize;

    for column in 0..width {
        let target = x + column;
        if target >= WIDTH {
            break;
        }

        for offset in 0..height {
            let y = row + offset;
            if y >= PAGES * 8 {
                break;
            }

            let edge = offset == 0 || offset == height - 1 || column == 0 || column == width - 1;
            let inside = column >= 1 && column <= filled;
            if edge || inside {
                frame[(y / 8) * PAGE_BYTES + target] |= 1 << (y % 8);
            }
        }
    }
}

/// Draws a fader: an outline with the level filled from the bottom, so a channel
/// at 0.7 stands seven tenths of the way up its travel. Like `draw_bar` it may
/// cross a page boundary. The horizontal flavour is for parameters that read
/// left to right, this one for levels.
pub fn draw_vbar(frame: &mut Frame, x: usize, row: usize, width: usize, height: usize, fraction: f32) {
    if width == 0 || height == 0 || row >= PAGES * 8 || x >= WIDTH {
        return;
    }

    let fraction = fraction.clamp(0.0, 1.0);
    let filled = ((height as f32 - 2.0) * fraction).round() as usize;

    for column in 0..width {
        let target = x + column;
        if target >= WIDTH {
            break;
        }

        for offset in 0..height {
            let y = row + offset;
            if y >= PAGES * 8 {
                break;
            }

            // 0 is the bottom row of the bar, 1 the one above it, and so on.
            let from_bottom = height - 1 - offset;
            let edge = offset == 0 || offset == height - 1 || column == 0 || column == width - 1;
            let inside =
                column >= 1 && column <= width - 2 && from_bottom >= 1 && from_bottom <= filled;
            if edge || inside {
                frame[(y / 8) * PAGE_BYTES + target] |= 1 << (y % 8);
            }
        }
    }
}

/// Fills a plain block of pixels: the mute and solo marks of the mixer page. It
/// may cross a page boundary as well.
pub fn fill_rect(frame: &mut Frame, x: usize, row: usize, width: usize, height: usize) {
    for offset in 0..height {
        let y = row + offset;
        if y >= PAGES * 8 {
            break;
        }

        for column in 0..width {
            let target = x + column;
            if target >= WIDTH {
                break;
            }
            frame[(y / 8) * PAGE_BYTES + target] |= 1 << (y % 8);
        }
    }
}

/// Draws text with the medium 7x9 font. Its glyphs are stored row by row, so they
/// are laid out row by row here too - and a glyph may therefore cross a page
/// boundary, which no glyph of the small font does.
pub fn draw_text_medium(frame: &mut Frame, x: usize, row: usize, text: &str) {
    let mut column = x;

    for character in text.chars() {
        for (offset, row_bits) in crate::font_medium::glyph(character).iter().enumerate() {
            let y = row + offset;
            if y >= PAGES * 8 {
                break;
            }

            for c in 0..crate::font_medium::GLYPH_WIDTH {
                if row_bits & (1 << c) == 0 {
                    continue;
                }
                let target = column + c;
                if target < WIDTH {
                    frame[(y / 8) * PAGE_BYTES + target] |= 1 << (y % 8);
                }
            }
        }

        column += crate::font_medium::GLYPH_WIDTH + crate::font_medium::GLYPH_SPACING;
    }
}

/// How many glyphs the sheet puts on one line.
pub const SHEET_COLUMNS: usize = 16;

/// The pixel row the sheet's lines start on: nine rows of glyphs, two empty.
pub const SHEET_LINE_PITCH: usize = 11;

/// Lines the sheet draws the font on, and what the panel holds.
pub const SHEET_LINES: usize = 3;

/// Glyphs to a line: sixteen fit with the usual one-pixel gap, and that is what
/// the font had until it grew a forty-ninth glyph - the star the tracks page
/// marks its selection with. The sheet packs them in rather than dropping the
/// last one, which is the one worth looking at.
pub fn sheet_columns() -> usize {
    SHEET_COLUMNS.max(crate::font_medium::GLYPHS.len().div_ceil(SHEET_LINES))
}

/// The gap between glyphs on the sheet: the screen's width shared out.
fn sheet_pitch() -> usize {
    WIDTH / sheet_columns()
}

/// Every glyph of the medium font, three lines of nine rows, which is what the
/// screen holds. Drawn to be looked at, not to be informative.
pub fn glyph_sheet() -> Frame {
    let mut frame = off();
    let columns = sheet_columns();
    let pitch = sheet_pitch();

    for (index, (character, _)) in crate::font_medium::GLYPHS.iter().enumerate() {
        let slot = index % columns;
        let line = index / columns;
        draw_text_medium(
            &mut frame,
            slot * pitch,
            line * SHEET_LINE_PITCH,
            &character.to_string(),
        );
    }

    frame
}

/// What the panel shows while there is no DAW to listen to: two words in the
/// medium font, readable from across the desk. ASCII, because that is all the
/// fonts carry - the docs call this state «Ждём DAW».
pub fn waiting_frame() -> Frame {
    let mut frame = off();
    for (row, text) in [(7usize, "WAITING"), (18, "FOR DAW")] {
        let width = crate::font_medium::text_width(text.chars().count());
        let x = WIDTH.saturating_sub(width) / 2;
        draw_text_medium(&mut frame, x, row, text);
    }
    frame
}

/// Two lines of text on a dark screen: the state that shows whether our own
/// rendering arrives at all.
pub fn message_frame() -> Frame {
    let mut frame = off();
    draw_text(&mut frame, 0, 0, "KOMPLEMENT");
    draw_text(&mut frame, 0, 8, "REAPER NAV");
    draw_text(&mut frame, 0, 16, "128X32 1 BIT");
    draw_text(&mut frame, 0, 24, "VOL -3.2 DB");
    frame
}

/// The frame of a named state, or `None` when the name is unknown. `text60` is
/// not a frame: it clears the pixels and sends the text reports.
///
/// A page may name a size after a colon - `page-track:large` - because the size
/// is what the SHIFT key steps through on the panel.
pub fn named_frame(name: &str) -> Option<Frame> {
    let (base, size) = match name.split_once(':') {
        Some((base, size)) => (base, crate::studio::Size::from_name(size)?),
        None => (name, crate::studio::Size::Medium),
    };

    if let Some(page) = crate::studio::Page::from_name(base) {
        return Some(crate::pages::render(
            page,
            &crate::studio::Studio::sample(),
            size,
        ));
    }

    match base {
        "on" => Some(on()),
        "off" => Some(off()),
        "top" => Some(half_height(true)),
        "bottom" => Some(half_height(false)),
        "left" => Some(half_width(true)),
        "right" => Some(half_width(false)),
        "vstripes" => Some(vertical_stripes()),
        "hstripes" => Some(horizontal_stripes()),
        "border" => Some(border()),
        "message" => Some(message_frame()),
        "waiting" => Some(waiting_frame()),
        "nav" => Some(crate::nav::render(&crate::nav::NavState::sample())),
        "navmed" => Some(crate::nav::render_medium(&crate::nav::NavState::sample())),
        "navmid" => Some(crate::nav::render_mid(&crate::nav::NavState::sample())),
        "navbig" => Some(crate::nav::render_big(&crate::nav::NavState::sample())),
        "glyphs" => Some(glyph_sheet()),
        _ => None,
    }
}

pub fn state_help() -> String {
    format!(
        "States: {}.\n\
        message draws a fixed four-line text. nav, navmed, navmid and navbig are the\n\
        earlier drafts of the four-line, three-line 7x9, mixed and two-line layouts.\n\
        page-transport, page-track, page-tracks and page-general are the real pages,\n\
        drawn from a sample state; each takes a size after a colon, page-track:large,\n\
        one of small, medium or large (medium by default). glyphs shows the whole 7x9\n\
        font, text60 clears the screen and sends the text reports guessed at in\n\
        doc/mini-screen.txt - measured: it does nothing. waiting is what the panel says when the DAW's port is gone (WAITING / FOR DAW on the panel).",
        STATE_NAMES.join(", ")
    )
}

/// The text report guessed at in `doc/mini-screen.txt`: id `0x60`, a mode byte,
/// the line index, an alignment byte, then ASCII padded with zeroes. This is the
/// one part of the documentation that is explicitly only "approximately" known.
pub fn text_report(line: u8, text: &str, width: usize) -> Vec<u8> {
    let mut report = vec![0u8; 4 + width];
    report[0] = 0x60;
    report[1] = 0x01;
    report[2] = line;
    report[3] = 0x00;

    for (i, byte) in text.bytes().take(width).enumerate() {
        report[4 + i] = byte;
    }

    report
}

fn pause(millis: u64) {
    std::thread::sleep(std::time::Duration::from_millis(millis));
}

/// Writes one frame: exactly two reports, the only two offsets that mean
/// anything.
pub fn show_frame(hid: &Hid, what: &str, frame: &Frame) -> usize {
    let mut accepted = 0;
    for report in frame_reports(frame) {
        match hid.write_raw(&report) {
            Ok(written) => {
                if written != report.len() {
                    println!("   {what}: wrote {written} of {} byte(s)", report.len());
                }
                accepted += 1;
            }
            Err(reason) => println!("   {what} FAILED: {reason}"),
        }
    }
    accepted
}

/// Reads for `millis`, printing reports that differ from the previous one.
fn read_for(hid: &Hid, millis: i32, dump: &mut Dump) -> usize {
    let mut buffer = [0u8; 64];
    let mut arrived = 0usize;
    let mut elapsed = 0;

    while elapsed < millis {
        match hid.read_timeout(&mut buffer, 100) {
            Ok(0) => {}
            Ok(length) => {
                arrived += 1;
                dump.packet(&buffer[..length], i64::from(elapsed));
            }
            Err(reason) => {
                println!("   reading FAILED: {reason}");
                break;
            }
        }
        elapsed += 100;
    }

    arrived
}

/// Prints a question and waits for the answer. Without a terminal to read from
/// (output redirected, no input) it just carries on, so the probe still runs.
fn ask(question: &str) -> String {
    use std::io::Write;

    print!("\n? {question}\n  > ");
    let _ = std::io::stdout().flush();

    let mut answer = String::new();
    match std::io::stdin().read_line(&mut answer) {
        Ok(0) | Err(_) => {
            println!("(no input available, continuing)");
            String::new()
        }
        Ok(_) => answer.trim().to_string(),
    }
}

/// Waits for Enter, or for `millis` when there is no terminal to wait on, so the
/// state stays on the screen long enough to look at it.
fn wait_for_enter(millis: u64) {
    use std::io::Write;

    print!("\n  Press Enter when you have seen it (or wait {} s): ", millis / 1000);
    let _ = std::io::stdout().flush();

    let mut answer = String::new();
    match std::io::stdin().read_line(&mut answer) {
        Ok(0) | Err(_) => pause(millis),
        Ok(_) => {}
    }
}

/// Sends the text-report guess on a cleared screen.
fn show_text(hid: &Hid) {
    show_frame(hid, "off", &off());
    pause(300);
    for (line, text) in ["komplement probe", "second line", "third line"]
        .iter()
        .enumerate()
    {
        match hid.write_raw(&text_report(line as u8, text, 24)) {
            Ok(written) => println!("   text 0x60 line {line}: wrote {written} byte(s)"),
            Err(reason) => println!("   text 0x60 line {line} FAILED: {reason}"),
        }
    }
}

/// `-S <state>`: shows one state, waits until it has been seen, and puts the
/// device back into MIDI mode. Repeatable, so a state can be photographed.
pub fn run_state(hid: &Hid, name: &str) -> Result<(), String> {
    println!("Disabling local controls (a0 03 04).");
    let _ = hid.write_raw(&DISABLE_LOCAL_CONTROLS);
    pause(300);

    if name == "text60" {
        println!("State `text60`: pixels cleared, then the 0x60 text reports.");
        show_text(hid);
    } else {
        let frame = named_frame(name).ok_or_else(|| format!("Unknown state `{name}`."))?;
        let accepted = show_frame(hid, &format!("state `{name}`"), &frame);
        println!("   {accepted} of 2 report(s) accepted");
    }

    wait_for_enter(10_000);

    println!("Back to MIDI mode (a0 07 00).");
    let _ = hid.write_raw(&MIDI_MODE);

    Ok(())
}

/// Shows one state, asks one question, then shows its opposite, so "did it
/// change?" is answerable at a glance on a small screen.
fn pair(hid: &Hid, first: (&str, Frame, &str), second: (&str, Frame, &str), hold_ms: u64) {
    show_frame(hid, first.0, &first.1);
    pause(hold_ms);
    ask(first.2);

    show_frame(hid, second.0, &second.1);
    pause(hold_ms);
    ask(second.2);
    println!();
}

/// `-s`: a few coarse states, each with a yes/no question, so the screen - not
/// the documentation - says what is what. Only whole screens and halves, because
/// single pixels cannot be judged on a screen this size.
pub fn run_probe(hid: &Hid) {
    let mut dump = Dump::new();

    println!("Display probe: coarse states only, each one followed by its opposite.");
    println!("Answer the questions in the terminal and send me the whole output.");
    println!("For a closer look at one state use -S <state>; states: {}", STATE_NAMES.join(", "));
    println!();

    ask("Before anything: what does the keyboard show on its own right now, and what colours are the lit pixels?");

    println!("1. Disabling local controls (a0 03 04): the keyboard's own display logic");
    println!("   stops handling the screen from here on.");
    let _ = hid.write_raw(&DISABLE_LOCAL_CONTROLS);
    pause(500);
    ask("Did the screen change when that happened (e.g. its own display went away)?");
    println!();

    let hold = 2500;

    println!("2. Everything lit, then everything dark.");
    pair(
        hid,
        ("on", on(), "Is the WHOLE screen bright now?"),
        ("off", off(), "And now is the WHOLE screen dark/empty?"),
        hold,
    );

    println!("3. Upper half of the height, then lower half.");
    pair(
        hid,
        (
            "top",
            half_height(true),
            "Is the bright part the UPPER half?",
        ),
        (
            "bottom",
            half_height(false),
            "And now the LOWER half - did the bright part move down?",
        ),
        hold,
    );

    println!("4. Left half of the width, then right half (ranges of columns inside every");
    println!("   page - this is what earlier probes got wrong).");
    pair(
        hid,
        ("left", half_width(true), "Is the bright part the LEFT half?"),
        (
            "right",
            half_width(false),
            "And now the RIGHT half? Or is it a band across the screen instead?",
        ),
        hold,
    );

    println!("5. Stripes: four columns on, four off, then rows: four rows on, four off.");
    pair(
        hid,
        (
            "vstripes",
            vertical_stripes(),
            "Wide VERTICAL stripes across the screen?",
        ),
        (
            "hstripes",
            horizontal_stripes(),
            "And this one: HORIZONTAL stripes?",
        ),
        hold,
    );

    println!("6. Text mode as guessed in doc/mini-screen.txt (report 0x60), on a cleared");
    println!("   screen.");
    show_frame(hid, "off", &off());
    pause(1000);
    ask("Is the screen empty now (before the text)? yes/no");
    show_text(hid);
    pause(1500);
    ask("Did ANYTHING change - text, even unreadable? Or exactly as before?");
    println!();

    println!("7. Press a few keys and turn a knob now (4 s): do they still arrive?");
    let arrived = read_for(hid, 4000, &mut dump);
    println!("   {arrived} report(s) arrived.");
    println!();

    println!("8. Back to MIDI mode (a0 07 00).");
    let _ = hid.write_raw(&MIDI_MODE);
    println!();
    println!("Done. Send me this output; the yes/no answers pin down the layout.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_waiting_screen_says_so_in_two_words_and_nothing_else() {
        let frame = waiting_frame();
        assert_ne!(frame, off(), "a dark screen says nothing");

        // Two words, one above the other, and nothing else on the panel: a lit
        // pixel is a lit pixel, so the words are not asserted letter by letter -
        // what matters is that a person sees a message where a page of DAW state
        // would otherwise be, and that the rows around it stay dark.
        let lit_rows = |first: usize, last: usize| {
            let mut count = 0;
            for row in first..last {
                let band = row / 8;
                let bit = 1u8 << (row % 8);
                for column in 0..WIDTH {
                    if frame[band * PAGE_BYTES + column] & bit != 0 {
                        count += 1;
                    }
                }
            }
            count
        };

        let height = crate::font_medium::GLYPH_HEIGHT;
        assert!(lit_rows(7, 7 + height) > 20, "the first word has to be lit");
        assert!(lit_rows(18, 18 + height) > 20, "the second word has to be lit");
        assert_eq!(lit_rows(0, 7), 0, "nothing above the first word");
        assert_eq!(lit_rows(7 + height, 18), 0, "the gap between the words stays dark");
        assert_eq!(lit_rows(18 + height, PAGES * 8), 0, "nothing below the second word");

        // Centred, so it reads the same from either side of the desk.
        let first_lit = (0..WIDTH)
            .find(|column| {
                (0..18 + height)
                    .any(|row| frame[(row / 8) * PAGE_BYTES + column] & (1 << (row % 8)) != 0)
            })
            .expect("something is lit");
        assert!(
            first_lit > 8 && first_lit < WIDTH / 3,
            "the words are centred, not flush left: the first lit column is {first_lit}"
        );

        // …and it is not the same picture as the probe's message screen, which
        // is the point of having a state of its own.
        assert_ne!(frame, message_frame());
    }

    #[test]
    fn a_report_carries_the_header_the_pages_and_the_last_page_of_each_half() {
        let mut frame = off();
        frame[0] = 0xf1;
        frame[PAGE_BYTES - 1] = 0xf2;
        frame[PAGE_BYTES] = 0xf3;
        frame[2 * PAGE_BYTES - 1] = 0xf4;
        frame[2 * PAGE_BYTES] = 0xf5;

        let upper = report(0, &frame);
        assert_eq!(upper.len(), REPORT_HEADER.len() + REPORT_PAYLOAD);
        assert_eq!(&upper[..9], &[0xe0, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x02, 0x00]);
        // The payload is inverted on the way out, so every frame byte arrives
        // complemented.
        assert_eq!(upper[9], !0xf1);
        assert_eq!(upper[9 + PAGE_BYTES - 1], !0xf2);
        assert_eq!(upper[9 + PAGE_BYTES], !0xf3);
        assert_eq!(upper[9 + 2 * PAGE_BYTES - 1], !0xf4);
        // The third page belongs to the second report.
        assert!(!upper.contains(&!0xf5));

        let lower = report(2, &frame);
        assert_eq!(lower[3], 0x02);
        assert_eq!(lower[9], !0xf5);
    }

    #[test]
    fn the_panel_is_inverted_on_the_wire() {
        // A lit pixel in a frame travels as a clear bit: measured on hardware,
        // all-zero bytes light the whole screen and 0xff makes it black.
        let lit = report(0, &on());
        assert!(lit[9..].iter().all(|byte| *byte == 0x00));

        let dark = report(0, &off());
        assert!(dark[9..].iter().all(|byte| *byte == 0xff));

        let mut frame = off();
        frame[3] = 0x80;
        assert_eq!(report(0, &frame)[9 + 3], 0x7f);
    }

    #[test]
    fn text_lands_in_the_page_of_its_row_and_stays_inside_the_width() {
        let mut frame = off();
        draw_text(&mut frame, 0, 8, "A");

        // Row 8 is the first row of page 1: five columns there, nothing else.
        let glyph = crate::font::glyph('A');
        for column in 0..crate::font::GLYPH_WIDTH {
            assert_eq!(frame[PAGE_BYTES + column], glyph[column], "column {column}");
        }
        assert!(frame[..PAGE_BYTES].iter().all(|byte| *byte == 0x00));
        assert!(frame[PAGE_BYTES + 6..].iter().all(|byte| *byte == 0x00));
        assert!(frame[2 * PAGE_BYTES..].iter().all(|byte| *byte == 0x00));
    }

    #[test]
    fn text_past_the_right_edge_is_cut_and_never_touches_another_page() {
        let mut frame = off();
        draw_text(&mut frame, WIDTH - 2, 16, "ABCDEFGH");
        assert!(frame[2 * PAGE_BYTES..3 * PAGE_BYTES]
            .iter()
            .any(|byte| *byte != 0x00));
        assert!(frame[3 * PAGE_BYTES..].iter().all(|byte| *byte == 0x00));

        // A row past the screen draws nothing at all.
        let mut frame = off();
        draw_text(&mut frame, 0, 32, "A");
        assert!(frame.iter().all(|byte| *byte == 0x00));
    }

    #[test]
    fn a_bar_is_an_outline_with_a_filled_part() {
        // Height 7 at row 8: bits 0..6 of the page's bytes.
        const FULL: u8 = 0b0111_1111;
        const EDGES_ONLY: u8 = 0b0100_0001;

        let mut outline = off();
        draw_bar(&mut outline, 10, 8, 40, 7, 0.0);
        assert_eq!(outline[PAGE_BYTES + 10], FULL, "left edge");
        assert_eq!(outline[PAGE_BYTES + 11], EDGES_ONLY, "interior, empty");
        assert_eq!(outline[PAGE_BYTES + 49], FULL, "right edge");
        assert!(outline[..PAGE_BYTES].iter().all(|byte| *byte == 0x00));
        assert!(outline[PAGE_BYTES + 50..].iter().all(|byte| *byte == 0x00));

        let mut full = off();
        draw_bar(&mut full, 10, 8, 40, 7, 1.0);
        assert!(full[PAGE_BYTES + 10..PAGE_BYTES + 50]
            .iter()
            .all(|byte| *byte == FULL));

        let mut half = off();
        draw_bar(&mut half, 10, 8, 40, 7, 0.5);
        // Nineteen of the thirty-eight interior columns are filled.
        assert_eq!(half[PAGE_BYTES + 29], FULL);
        assert_eq!(half[PAGE_BYTES + 30], EDGES_ONLY);
    }

    #[test]
    fn medium_text_lands_on_the_row_it_is_asked_for() {
        let mut frame = off();
        draw_text_medium(&mut frame, 0, 11, "A");

        // Rows 11..19 are pages 1 and 2; nothing else is touched.
        assert!(frame[..PAGE_BYTES].iter().all(|byte| *byte == 0x00));
        assert!(frame[PAGE_BYTES..2 * PAGE_BYTES]
            .iter()
            .any(|byte| *byte != 0x00));
        assert!(frame[2 * PAGE_BYTES..3 * PAGE_BYTES]
            .iter()
            .any(|byte| *byte != 0x00));
        assert!(frame[3 * PAGE_BYTES..].iter().all(|byte| *byte == 0x00));
    }

    #[test]
    fn medium_text_is_cut_at_the_right_edge() {
        let mut frame = off();
        draw_text_medium(&mut frame, 0, 0, &"A".repeat(16));
        let full = frame;

        let mut frame = off();
        draw_text_medium(&mut frame, 0, 0, &"A".repeat(20));
        assert_eq!(full, frame);
    }

    #[test]
    fn medium_text_starting_below_the_screen_changes_nothing() {
        let mut frame = off();
        draw_text_medium(&mut frame, 0, 32, "A");
        assert_eq!(frame, off());
    }

    #[test]
    fn the_glyph_sheet_shows_every_glyph_on_three_lines() {
        assert!(
            crate::font_medium::GLYPHS.len() <= sheet_columns() * SHEET_LINES,
            "the sheet has to hold the whole font in the three lines the screen has"
        );

        let frame = glyph_sheet();
        for line in 0..3 {
            let page = line * SHEET_LINE_PITCH / 8;
            assert!(
                frame[page * PAGE_BYTES..(page + 1) * PAGE_BYTES]
                    .iter()
                    .any(|byte| *byte != 0x00),
                "sheet line {line} is empty"
            );
        }
    }

    #[test]
    fn a_bar_stays_inside_the_width_and_the_page() {
        let mut frame = off();
        draw_bar(&mut frame, WIDTH - 5, 16, 40, 7, 0.5);
        assert!(frame[2 * PAGE_BYTES..3 * PAGE_BYTES]
            .iter()
            .any(|byte| *byte != 0x00));
        assert!(frame[3 * PAGE_BYTES..].iter().all(|byte| *byte == 0x00));

        let mut frame = off();
        draw_bar(&mut frame, 0, 32, 40, 7, 0.5);
        assert!(frame.iter().all(|byte| *byte == 0x00));

        let mut frame = off();
        draw_bar(&mut frame, 0, 8, 0, 7, 0.5);
        assert!(frame.iter().all(|byte| *byte == 0x00));
    }

    #[test]
    fn a_message_carries_text_in_all_four_pages() {
        let frame = message_frame();
        for page in 0..PAGES {
            assert!(
                frame[page * PAGE_BYTES..(page + 1) * PAGE_BYTES]
                    .iter()
                    .any(|byte| *byte != 0x00),
                "page {page} is empty"
            );
        }
    }

    #[test]
    fn only_two_offsets_are_ever_used() {
        let reports = frame_reports(&on());
        let offsets: Vec<u8> = reports.iter().map(|report| report[3]).collect();
        assert_eq!(offsets, REPORT_OFFSETS.to_vec());
    }

    #[test]
    fn the_two_halves_of_the_width_are_opposite_and_keep_pages_apart() {
        let left = half_width(true);
        let right = half_width(false);

        for page in 0..PAGES {
            let start = page * PAGE_BYTES;
            assert!(left[start..start + WIDTH / 2].iter().all(|byte| *byte == 0xff));
            assert!(left[start + WIDTH / 2..start + WIDTH].iter().all(|byte| *byte == 0x00));
            assert!(right[start..start + WIDTH / 2].iter().all(|byte| *byte == 0x00));
            assert!(right[start + WIDTH / 2..start + WIDTH].iter().all(|byte| *byte == 0xff));
        }
    }

    #[test]
    fn the_two_halves_of_the_height_are_opposite_pages() {
        let top = half_height(true);
        let bottom = half_height(false);

        assert!(top[..2 * PAGE_BYTES].iter().all(|byte| *byte == 0xff));
        assert!(top[2 * PAGE_BYTES..].iter().all(|byte| *byte == 0x00));
        assert!(bottom[..2 * PAGE_BYTES].iter().all(|byte| *byte == 0x00));
        assert!(bottom[2 * PAGE_BYTES..].iter().all(|byte| *byte == 0xff));
    }

    #[test]
    fn stripes_alternate_in_fours() {
        let frame = vertical_stripes();
        for column in 0..WIDTH {
            let expected = if (column / 4) % 2 == 0 { 0xff } else { 0x00 };
            assert_eq!(frame[column], expected, "column {column}");
            assert_eq!(frame[PAGE_BYTES + column], expected);
        }
        assert_eq!(frame[0], 0xff);
        assert_eq!(frame[4], 0x00);
        assert_eq!(frame[8], 0xff);

        assert!(horizontal_stripes().iter().all(|byte| *byte == 0x0f));
    }

    #[test]
    fn the_border_touches_all_four_edges() {
        let frame = border();

        // Top edge: the topmost bit of the first page in every column.
        assert!(frame[..WIDTH].iter().all(|byte| byte & 0x01 != 0));
        // Bottom edge: the lowest bit of the last page in every column.
        assert!(frame[3 * PAGE_BYTES..].iter().all(|byte| byte & 0x80 != 0));
        // Left and right edges: full columns in every page.
        for page in 0..PAGES {
            assert_eq!(frame[page * PAGE_BYTES], 0xff);
            assert_eq!(frame[page * PAGE_BYTES + WIDTH - 1], 0xff);
        }

        // Nothing else: the pages in between carry no top or bottom row, which
        // is what separates this from four separate lines.
        for page in 1..3 {
            let start = page * PAGE_BYTES;
            assert!(frame[start + 1..start + WIDTH - 1]
                .iter()
                .all(|byte| *byte == 0x00));
        }
    }

    #[test]
    fn named_states_are_known_or_refused() {
        for name in STATE_NAMES {
            if name == "text60" {
                assert!(named_frame(name).is_none());
            } else {
                assert!(named_frame(name).is_some(), "state {name}");
            }
        }
        assert!(named_frame("nonsense").is_none());
    }

    #[test]
    fn a_text_report_carries_id_line_and_ascii() {
        let report = text_report(2, "AB", 8);
        assert_eq!(report.len(), 12);
        assert_eq!(report[0], 0x60);
        assert_eq!(report[1], 0x01);
        assert_eq!(report[2], 2);
        assert_eq!(report[3], 0x00);
        assert_eq!(&report[4..6], b"AB");
        assert!(report[6..].iter().all(|byte| *byte == 0x00));
    }

    #[test]
    fn text_longer_than_the_width_is_cut_not_overflowing() {
        let report = text_report(0, "abcdefgh", 4);
        assert_eq!(report.len(), 8);
        assert_eq!(&report[4..], b"abcd");
    }
}
