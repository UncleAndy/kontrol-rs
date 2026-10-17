//! The medium font: seven columns by nine rows, the size between the small 5x7
//! `font` and that font drawn at double scale.
//!
//! The layout differs from `font` on purpose: a column of this font is nine
//! pixels tall, which does not fit in a byte, so a glyph is stored row-major -
//! one byte per row, bit 0 the leftmost column. Capitals occupy the whole cell,
//! so three lines of them fit a 128x32 screen with room between the lines.
//!
//! Text is upper-case only (lower-case is folded up), like the small font, and a
//! character with no glyph draws as `?`.
//!
//! The table below is generated: the glyphs are drawn as nine rows of `#` and `.`
//! in `tools/font-medium-glyphs.txt`, and `tools/font-medium-table.awk` turns
//! them into the Rust table. Edit the art (`awk -f tools/font-medium-table.awk
//! tools/font-medium-glyphs.txt` prints a sheet of the whole font as it will look
//! on the panel) and paste the new table in, rather than editing bit patterns by
//! hand.

/// Columns per glyph.
pub const GLYPH_WIDTH: usize = 7;

/// Rows per glyph, and bytes per glyph.
pub const GLYPH_HEIGHT: usize = 9;

/// Columns left empty after each glyph.
pub const GLYPH_SPACING: usize = 1;

pub const GLYPHS: [(char, [u8; GLYPH_HEIGHT]); 48] = [
    ('A', [0b0011100, 0b1000001, 0b1000001, 0b1000001, 0b1111111, 0b1000001, 0b1000001, 0b1000001, 0b1000001]),
    ('B', [0b0111111, 0b1000001, 0b1000001, 0b1000001, 0b0111111, 0b1000001, 0b1000001, 0b1000001, 0b0111111]),
    ('C', [0b0011110, 0b0100001, 0b0000001, 0b0000001, 0b0000001, 0b0000001, 0b0000001, 0b0100001, 0b0011110]),
    ('D', [0b0011111, 0b0100001, 0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b0100001, 0b0011111]),
    ('E', [0b1111111, 0b0000001, 0b0000001, 0b0000001, 0b0011111, 0b0000001, 0b0000001, 0b0000001, 0b1111111]),
    ('F', [0b1111111, 0b0000001, 0b0000001, 0b0000001, 0b0011111, 0b0000001, 0b0000001, 0b0000001, 0b0000001]),
    ('G', [0b0011110, 0b0100001, 0b0000001, 0b0000001, 0b1111001, 0b1000001, 0b1000001, 0b0100001, 0b0011110]),
    ('H', [0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b1111111, 0b1000001, 0b1000001, 0b1000001, 0b1000001]),
    ('I', [0b1111111, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b1111111]),
    ('J', [0b1111000, 0b0100000, 0b0100000, 0b0100000, 0b0100000, 0b0100000, 0b0100000, 0b0100001, 0b0011110]),
    ('K', [0b0100001, 0b0010001, 0b0001001, 0b0000101, 0b0000011, 0b0000101, 0b0001001, 0b0010001, 0b0100001]),
    ('L', [0b0000001, 0b0000001, 0b0000001, 0b0000001, 0b0000001, 0b0000001, 0b0000001, 0b0000001, 0b1111111]),
    ('M', [0b1000001, 0b1100011, 0b1010101, 0b1010101, 0b1001001, 0b1000001, 0b1000001, 0b1000001, 0b1000001]),
    ('N', [0b1000001, 0b1000011, 0b1000101, 0b1001001, 0b1010001, 0b1100001, 0b1000001, 0b1000001, 0b1000001]),
    ('O', [0b0011110, 0b0100001, 0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b0100001, 0b0011110]),
    ('P', [0b0111111, 0b1000001, 0b1000001, 0b1000001, 0b0111111, 0b0000001, 0b0000001, 0b0000001, 0b0000001]),
    ('Q', [0b0011110, 0b0100001, 0b1000001, 0b1000001, 0b1000001, 0b1010001, 0b1100001, 0b0100001, 0b0111110]),
    ('R', [0b0111111, 0b1000001, 0b1000001, 0b1000001, 0b0111111, 0b0001001, 0b0010001, 0b0100001, 0b1000001]),
    ('S', [0b0111110, 0b1000001, 0b0000001, 0b0000001, 0b0111110, 0b1000000, 0b0100000, 0b1000001, 0b0111110]),
    ('T', [0b1111111, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000]),
    ('U', [0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b0111110]),
    ('V', [0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b0100010, 0b0100010, 0b0010100, 0b0010100, 0b0001000]),
    ('W', [0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b1001001, 0b1001001, 0b1010101, 0b1100011, 0b1100011]),
    ('X', [0b1000001, 0b1000001, 0b0100010, 0b0010100, 0b0001000, 0b0010100, 0b0100010, 0b1000001, 0b1000001]),
    ('Y', [0b1000001, 0b1000001, 0b0100010, 0b0010100, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000]),
    ('Z', [0b1111111, 0b0100000, 0b0010000, 0b0001000, 0b0000100, 0b0000010, 0b0000001, 0b0000001, 0b1111111]),
    ('0', [0b0011110, 0b0100001, 0b1000001, 0b1000001, 0b1011001, 0b1011001, 0b1000001, 0b0100001, 0b0011110]),
    ('1', [0b0001000, 0b0001100, 0b0001010, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0111110]),
    ('2', [0b0111110, 0b1000001, 0b1000000, 0b0100000, 0b0010000, 0b0001000, 0b0000100, 0b0000010, 0b1111111]),
    ('3', [0b0111110, 0b1000001, 0b1000000, 0b1000000, 0b0111110, 0b1000000, 0b1000000, 0b1000001, 0b0111110]),
    ('4', [0b0100000, 0b0110000, 0b0101000, 0b0100100, 0b0100010, 0b0100001, 0b1111111, 0b0100000, 0b0100000]),
    ('5', [0b1111111, 0b0000001, 0b0000001, 0b0000001, 0b0111110, 0b1000000, 0b1000000, 0b1000001, 0b0111110]),
    ('6', [0b0111100, 0b0000010, 0b0000001, 0b0000001, 0b0111111, 0b1000001, 0b1000001, 0b1000001, 0b0111110]),
    ('7', [0b1111111, 0b0100000, 0b0010000, 0b0010000, 0b0001000, 0b0001000, 0b0000100, 0b0000100, 0b0000100]),
    ('8', [0b0111110, 0b1000001, 0b1000001, 0b1000001, 0b0111110, 0b1000001, 0b1000001, 0b1000001, 0b0111110]),
    ('9', [0b0111110, 0b1000001, 0b1000001, 0b1000001, 0b1111110, 0b1000000, 0b1000000, 0b0100000, 0b0011110]),
    ('-', [0b0000000, 0b0000000, 0b0000000, 0b0000000, 0b0111110, 0b0000000, 0b0000000, 0b0000000, 0b0000000]),
    ('.', [0b0000000, 0b0000000, 0b0000000, 0b0000000, 0b0000000, 0b0000000, 0b0001100, 0b0001100, 0b0001100]),
    (':', [0b0000000, 0b0001100, 0b0001100, 0b0000000, 0b0000000, 0b0000000, 0b0001100, 0b0001100, 0b0000000]),
    ('/', [0b1000000, 0b0100000, 0b0010000, 0b0010000, 0b0001000, 0b0001000, 0b0000100, 0b0000010, 0b0000001]),
    ('_', [0b0000000, 0b0000000, 0b0000000, 0b0000000, 0b0000000, 0b0000000, 0b0000000, 0b0000000, 0b1111111]),
    ('%', [0b1000011, 0b0100011, 0b0010000, 0b0001000, 0b0000100, 0b0000010, 0b1100001, 0b1100000, 0b0000000]),
    ('+', [0b0000000, 0b0001000, 0b0001000, 0b0001000, 0b1111111, 0b0001000, 0b0001000, 0b0001000, 0b0000000]),
    ('(', [0b0010000, 0b0001000, 0b0000100, 0b0000100, 0b0000100, 0b0000100, 0b0000100, 0b0001000, 0b0010000]),
    (')', [0b0000100, 0b0001000, 0b0010000, 0b0010000, 0b0010000, 0b0010000, 0b0010000, 0b0001000, 0b0000100]),
    ('!', [0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0000000, 0b0001000]),
    ('?', [0b0111110, 0b1000001, 0b1000000, 0b0100000, 0b0010000, 0b0001000, 0b0000000, 0b0001000, 0b0001000]),
    (' ', [0b0000000, 0b0000000, 0b0000000, 0b0000000, 0b0000000, 0b0000000, 0b0000000, 0b0000000, 0b0000000]),
];

/// The glyph for `character`, upper-cased first. Characters with no glyph draw
/// as `?`, and a glyph for `?` is expected to exist.
pub fn glyph(character: char) -> [u8; GLYPH_HEIGHT] {
    let wanted = character.to_ascii_uppercase();
    let mut fallback = [0u8; GLYPH_HEIGHT];

    for (candidate, rows) in GLYPHS {
        if candidate == wanted {
            return rows;
        }
        if candidate == '?' {
            fallback = rows;
        }
    }

    fallback
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_seven_columns_and_never_uses_the_eighth_bit() {
        for (character, rows) in GLYPHS {
            for (row, bits) in rows.iter().enumerate() {
                assert_eq!(
                    bits & 0b1000_0000,
                    0,
                    "{character} row {row} sets a column that does not exist"
                );
            }
        }
    }

    #[test]
    fn the_alphabet_digits_and_question_mark_are_all_present() {
        for character in ('A'..='Z').chain('0'..='9').chain(['?', '-', '.', ':', '/', '_', '%', '+', '(', ')', '!']) {
            let rows = glyph(character);
            assert!(
                rows.iter().any(|bits| *bits != 0),
                "{character} has no glyph"
            );
        }
    }

    #[test]
    fn no_two_letters_or_digits_are_the_same_glyph() {
        // A copy-paste slip in the table shows up as two characters drawing the
        // same shape.
        for (index, (character, rows)) in GLYPHS.iter().enumerate() {
            if !character.is_ascii_alphanumeric() {
                continue;
            }
            for (other, other_rows) in GLYPHS.iter().skip(index + 1) {
                assert!(
                    rows != other_rows,
                    "{character} and {other} are the same glyph"
                );
            }
        }
    }

    #[test]
    fn only_the_space_is_blank() {
        for (character, rows) in GLYPHS {
            if character == ' ' {
                assert_eq!(rows, [0u8; GLYPH_HEIGHT]);
                continue;
            }
            assert!(
                rows.iter().any(|bits| *bits != 0),
                "{character} is blank"
            );
        }
    }

    #[test]
    fn lowercase_folds_to_uppercase() {
        assert_eq!(glyph('a'), glyph('A'));
        assert_eq!(glyph('z'), glyph('Z'));
        assert_eq!(glyph('5'), glyph('5'));
    }

    #[test]
    fn unknown_characters_draw_as_question_mark() {
        // Cyrillic has no glyph in this font, and neither has a stray symbol.
        assert_eq!(glyph('✔'), glyph('?'));
        assert_eq!(glyph('~'), glyph('?'));
    }

    #[test]
    fn a_line_holds_sixteen_characters() {
        assert_eq!(characters_per_line(128), 16);
        assert_eq!(text_width(16), 127);
        assert_eq!(text_width(0), 0);
    }
}
