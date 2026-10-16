//! A 5x7 font for the keyboard's display.
//!
//! The layout matches the frame exactly: one byte per column, bit 0 the top row,
//! so a glyph is five column bytes with the rows in bits 0..6. Glyphs are
//! upper-case only; lower-case input is folded, anything unknown becomes `?`.
//!
//! The shapes are the well-known public-domain 5x7 patterns.

use std::ops::Range;

/// Columns per glyph, without the spacing column.
pub const GLYPH_WIDTH: usize = 5;
/// Rows per glyph; the eighth row of a page stays free as a gap.
pub const GLYPH_HEIGHT: usize = 7;
/// Empty columns between two glyphs.
pub const GLYPH_SPACING: usize = 1;

/// The five column bytes of one character.
pub fn glyph(character: char) -> [u8; GLYPH_WIDTH] {
    match character.to_ascii_uppercase() {
        ' ' => [0x00, 0x00, 0x00, 0x00, 0x00],
        '!' => [0x00, 0x00, 0x5f, 0x00, 0x00],
        '"' => [0x00, 0x07, 0x00, 0x07, 0x00],
        '#' => [0x14, 0x7f, 0x14, 0x7f, 0x14],
        '$' => [0x24, 0x2a, 0x7f, 0x2a, 0x12],
        '%' => [0x62, 0x64, 0x08, 0x13, 0x23],
        '&' => [0x36, 0x49, 0x55, 0x22, 0x50],
        '\'' => [0x00, 0x05, 0x03, 0x00, 0x00],
        '(' => [0x00, 0x1c, 0x22, 0x41, 0x00],
        ')' => [0x00, 0x41, 0x22, 0x1c, 0x00],
        '*' => [0x14, 0x08, 0x3e, 0x08, 0x14],
        '+' => [0x08, 0x08, 0x3e, 0x08, 0x08],
        ',' => [0x00, 0x50, 0x30, 0x00, 0x00],
        '-' => [0x08, 0x08, 0x08, 0x08, 0x08],
        '.' => [0x00, 0x60, 0x60, 0x00, 0x00],
        '/' => [0x20, 0x10, 0x08, 0x04, 0x02],
        '0' => [0x3e, 0x51, 0x49, 0x45, 0x3e],
        '1' => [0x00, 0x42, 0x7f, 0x40, 0x00],
        '2' => [0x42, 0x61, 0x51, 0x49, 0x46],
        '3' => [0x21, 0x41, 0x45, 0x4b, 0x31],
        '4' => [0x18, 0x14, 0x12, 0x7f, 0x10],
        '5' => [0x27, 0x45, 0x45, 0x45, 0x39],
        '6' => [0x3c, 0x4a, 0x49, 0x49, 0x30],
        '7' => [0x01, 0x71, 0x09, 0x05, 0x03],
        '8' => [0x36, 0x49, 0x49, 0x49, 0x36],
        '9' => [0x06, 0x49, 0x49, 0x29, 0x1e],
        ':' => [0x00, 0x36, 0x36, 0x00, 0x00],
        ';' => [0x00, 0x56, 0x36, 0x00, 0x00],
        '<' => [0x08, 0x14, 0x22, 0x41, 0x00],
        '=' => [0x14, 0x14, 0x14, 0x14, 0x14],
        '>' => [0x00, 0x41, 0x22, 0x14, 0x08],
        '?' => [0x02, 0x01, 0x51, 0x09, 0x06],
        '@' => [0x32, 0x49, 0x79, 0x41, 0x3e],
        'A' => [0x7e, 0x11, 0x11, 0x11, 0x7e],
        'B' => [0x7f, 0x49, 0x49, 0x49, 0x36],
        'C' => [0x3e, 0x41, 0x41, 0x41, 0x22],
        'D' => [0x7f, 0x41, 0x41, 0x22, 0x1c],
        'E' => [0x7f, 0x49, 0x49, 0x49, 0x41],
        'F' => [0x7f, 0x09, 0x09, 0x09, 0x01],
        'G' => [0x3e, 0x41, 0x49, 0x49, 0x7a],
        'H' => [0x7f, 0x08, 0x08, 0x08, 0x7f],
        'I' => [0x00, 0x41, 0x7f, 0x41, 0x00],
        'J' => [0x20, 0x40, 0x41, 0x3f, 0x01],
        'K' => [0x7f, 0x08, 0x14, 0x22, 0x41],
        'L' => [0x7f, 0x40, 0x40, 0x40, 0x40],
        'M' => [0x7f, 0x02, 0x0c, 0x02, 0x7f],
        'N' => [0x7f, 0x04, 0x08, 0x10, 0x7f],
        'O' => [0x3e, 0x41, 0x41, 0x41, 0x3e],
        'P' => [0x7f, 0x09, 0x09, 0x09, 0x06],
        'Q' => [0x3e, 0x41, 0x51, 0x21, 0x5e],
        'R' => [0x7f, 0x09, 0x19, 0x29, 0x46],
        'S' => [0x46, 0x49, 0x49, 0x49, 0x31],
        'T' => [0x01, 0x01, 0x7f, 0x01, 0x01],
        'U' => [0x3f, 0x40, 0x40, 0x40, 0x3f],
        'V' => [0x1f, 0x20, 0x40, 0x20, 0x1f],
        'W' => [0x3f, 0x40, 0x38, 0x40, 0x3f],
        'X' => [0x63, 0x14, 0x08, 0x14, 0x63],
        'Y' => [0x07, 0x08, 0x70, 0x08, 0x07],
        'Z' => [0x61, 0x51, 0x49, 0x45, 0x43],
        '[' => [0x00, 0x7f, 0x41, 0x41, 0x00],
        '\\' => [0x02, 0x04, 0x08, 0x10, 0x20],
        ']' => [0x00, 0x41, 0x41, 0x7f, 0x00],
        '^' => [0x04, 0x02, 0x01, 0x02, 0x04],
        '_' => [0x40, 0x40, 0x40, 0x40, 0x40],
        '`' => [0x00, 0x01, 0x02, 0x04, 0x00],
        '{' => [0x00, 0x08, 0x36, 0x41, 0x00],
        '|' => [0x00, 0x00, 0x7f, 0x00, 0x00],
        '}' => [0x00, 0x41, 0x36, 0x08, 0x00],
        '~' => [0x08, 0x08, 0x2a, 0x1c, 0x08],
        _ => [0x02, 0x01, 0x51, 0x09, 0x06],
    }
}

/// Pixel columns a string of `characters` characters occupies.
pub const fn text_width(characters: usize) -> usize {
    if characters == 0 {
        0
    } else {
        characters * (GLYPH_WIDTH + GLYPH_SPACING) - GLYPH_SPACING
    }
}

/// How many characters fit on a line of `pixels` columns.
pub const fn characters_per_line(pixels: usize) -> usize {
    (pixels + GLYPH_SPACING) / (GLYPH_WIDTH + GLYPH_SPACING)
}

/// Pixel columns a string occupies when drawn at `scale`.
pub const fn scaled_text_width(characters: usize, scale: usize) -> usize {
    if characters == 0 {
        0
    } else {
        (characters * (GLYPH_WIDTH + GLYPH_SPACING) - GLYPH_SPACING) * scale
    }
}

/// How many characters fit on a line of `pixels` columns at `scale`; at least
/// one, so a caller never divides the line away entirely.
pub const fn scaled_characters_per_line(pixels: usize, scale: usize) -> usize {
    if scale == 0 {
        return 0;
    }

    let per_character = (GLYPH_WIDTH + GLYPH_SPACING) * scale;
    let fits = (pixels + GLYPH_SPACING * scale) / per_character;
    if fits == 0 {
        1
    } else {
        fits
    }
}

/// Row positions that keep a glyph inside one page: a page is eight rows, a
/// glyph seven, so the glyph may start in the page's first row only.
pub fn text_rows() -> Vec<usize> {
    (0..4).map(|page| page * 8).collect::<Vec<usize>>()
}

/// The pixel rows a line of text at `row` covers.
pub fn line_rows(row: usize) -> Range<usize> {
    row..row + GLYPH_HEIGHT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_stay_inside_seven_rows() {
        for code in 32u8..127 {
            let character = code as char;
            for column in glyph(character) {
                assert_eq!(
                    column & 0x80,
                    0,
                    "glyph {character:?} has a bit in the ninth row"
                );
            }
        }
    }

    #[test]
    fn space_is_empty_and_letters_are_not() {
        assert_eq!(glyph(' '), [0x00; GLYPH_WIDTH]);
        for character in "AZaz09".chars() {
            assert!(
                glyph(character).iter().any(|column| *column != 0),
                "glyph {character:?} is empty"
            );
        }
        assert!(glyph('\u{1f600}').iter().any(|column| *column != 0));
    }

    #[test]
    fn lower_case_is_the_upper_case_shape() {
        for character in "abcdefghijklmnopqrstuvwxyz".chars() {
            assert_eq!(
                glyph(character),
                glyph(character.to_ascii_uppercase()),
                "glyph {character:?}"
            );
        }
    }

    #[test]
    fn a_is_five_columns_and_seven_rows_wide() {
        // The shape the display will show, as rows of text.
        let a = glyph('A');
        assert_eq!(a, [0x7e, 0x11, 0x11, 0x11, 0x7e]);

        let mut rows = Vec::new();
        for row in 0..GLYPH_HEIGHT {
            let line: String = a
                .iter()
                .map(|column| if column & (1 << row) != 0 { '#' } else { '.' })
                .collect();
            rows.push(line);
        }
        assert_eq!(
            rows,
            vec![
                ".###.", "#...#", "#...#", "#...#", "#####", "#...#", "#...#"
            ]
        );
    }

    #[test]
    fn a_line_holds_twenty_one_characters_of_one_hundred_and_twenty_eight_pixels() {
        assert_eq!(characters_per_line(128), 21);
        assert_eq!(text_width(21), 125);
        assert_eq!(text_width(0), 0);
    }

    #[test]
    fn text_rows_keep_glyphs_inside_their_pages() {
        assert_eq!(text_rows(), vec![0, 8, 16, 24]);
        assert_eq!(line_rows(8), 8..15);
    }
}
