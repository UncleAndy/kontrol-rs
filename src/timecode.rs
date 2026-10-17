//! The DAW's time display, read as a time.
//!
//! The panel is given the DAW's own twelve seven-segment digits and nothing
//! else, so what those digits mean has to be measured rather than assumed. The
//! protocol says only that they are a time display and that bit 6 of a value is
//! its dot; Reaper sends no dots at all and, in its `H:M:S:F` format, not even a
//! separator between hours, minutes and seconds - which is why the row reads as
//! one run of digits until the layout is known.
//!
//! What was measured on a real run, with the transport stopped and the format set
//! to `H:M:S:F`. The rows are copied from the console echo of
//! `komplement -X <port>`, values exactly as the DAW wrote them, digit 0 being
//! the rightmost:
//!
//! ```text
//! 64:48 65:48 66:32 67:52 68:50 69:51 70:48 71:48 72:48 73:0 74:49 75:48
//! 64:48 65:48 66:32 67:48 68:48 69:49 70:48 71:48 72:48 73:0 74:49 75:48
//! ```
//!
//! Both were checked against Reaper's own transport: the first is `00:03:24:00`
//! and the second `00:01:00:00`, the position the run was stopped on. Leading
//! zeros are not shown at all, so what goes on the panel is `3:24:00` and
//! `1:00:00`: a field in front that is zero is left out, and the first field that
//! is shown loses its own leading zero. A panel too narrow for the whole time gets
//! the seconds, with the minutes in front of them when there are any (`3:24`),
//! because that is the part which says where the DAW is right now.
//!
//! The row itself is `hhmmss:ff`: the frames are the two rightmost digits, the
//! DAW's separator - a real space, `0x20`, not a dark digit - sits immediately
//! left of them, and the fields left of it are two digits each, seconds first,
//! then minutes, then as many hours as there is room for. Digit 73 is dark in
//! both rows, which is how the DAW turns a leading zero off.
//!
//! Nothing here is guessed: a row without that separator is handed on exactly as
//! the DAW wrote it, so a format we have not measured can never be dressed up as
//! one we have.

use crate::studio::{Digits, DISPLAY_DIGITS, TIMECODE_DIGITS};

/// What the DAW writes where it wants a separator in this format. It is a real
/// space, not a dark digit: a dark digit is what it turns a leading zero off
/// with, and the two mean different things.
const SEPARATOR: u8 = b' ';

/// The digit the DAW writes for a digit it has turned off.
const DARK: u8 = 0;

/// How the DAW's display row is to be read.
///
/// `AsWritten` is the default: the digits are drawn as they arrived, holes and
/// all. A named layout exists because the DAW leaves out the punctuation that
/// would make the row readable - and a layout is only ever added here after it
/// has been measured on a real run, never inferred.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TimeLayout {
    #[default]
    AsWritten,
    HoursMinutesSecondsFrames,
}

impl TimeLayout {
    /// A layout by the name the DAW offers it under, as it would be typed:
    /// `h:m:s:f` (Reaper's name for the format, colons and all), `HMSF`, or
    /// `written` for the row as it arrived. Case, colons, spaces, dashes and
    /// underscores are all ignored.
    pub fn from_name(name: &str) -> Option<TimeLayout> {
        let squeezed: String = name
            .chars()
            .filter(|character| !matches!(character, ':' | ' ' | '_' | '-'))
            .flat_map(char::to_lowercase)
            .collect();
        match squeezed.as_str() {
            "written" | "asis" => Some(TimeLayout::AsWritten),
            "hmsf" | "hoursminutessecondsframes" => Some(TimeLayout::HoursMinutesSecondsFrames),
            _ => None,
        }
    }

    /// The name to print: the DAW's own, so `-T` can be given the same word.
    pub fn name(&self) -> &'static str {
        match self {
            TimeLayout::AsWritten => "written",
            TimeLayout::HoursMinutesSecondsFrames => "h:m:s:f",
        }
    }

    /// The time this row means, or the row itself when this layout does not fit
    /// it.
    pub fn position(&self, digits: &Digits) -> String {
        match self {
            TimeLayout::AsWritten => digits.timecode(),
            TimeLayout::HoursMinutesSecondsFrames => fields(digits)
                .map(|fields| fields.written())
                .unwrap_or_else(|| digits.timecode()),
        }
    }

    /// The position for a panel that has room only for what matters most, or
    /// empty when this layout has nothing shorter to say - in which case the
    /// caller clips the full text as it always did.
    pub fn short_position(&self, digits: &Digits) -> String {
        match self {
            TimeLayout::AsWritten => String::new(),
            TimeLayout::HoursMinutesSecondsFrames => {
                fields(digits).map(|fields| fields.short()).unwrap_or_default()
            }
        }
    }
}

/// The four fields of the row, as text.
struct Fields {
    hours: String,
    minutes: String,
    seconds: String,
    frames: String,
}

impl Fields {
    /// The time as it is worth showing. Leading zeros are not shown at all: a
    /// field in front that is zero is left out, and the first field that is shown
    /// loses its own leading zero, so `00:03:24:00` reads `3:24:00` and
    /// `00:00:24:07` reads `24:07`. The last field is kept even when it is zero,
    /// so a position of zero still reads as something rather than as nothing.
    fn written(&self) -> String {
        let fields = [
            self.hours.as_str(),
            self.minutes.as_str(),
            self.seconds.as_str(),
            self.frames.as_str(),
        ];
        let first = fields
            .iter()
            .position(|field| field.bytes().any(|byte| byte != b'0'))
            .unwrap_or(fields.len() - 1);

        let mut text = String::new();
        for (index, field) in fields.iter().enumerate().skip(first) {
            if index > first {
                text.push(':');
            }
            let digits = if index == first {
                field.trim_start_matches('0')
            } else {
                field
            };
            if digits.is_empty() {
                text.push('0');
            } else {
                text.push_str(digits);
            }
        }
        text
    }

    /// What is worth keeping when there is no room for the whole time: the
    /// seconds, which are what says where the DAW is right now, with the minutes
    /// in front of them when there are any. Leading zeros go here too, so the
    /// minutes of `00:01:00:00` read `1:00` and a position under a minute reads
    /// its seconds alone.
    fn short(&self) -> String {
        let fields = [self.minutes.as_str(), self.seconds.as_str()];
        let first = fields
            .iter()
            .position(|field| field.bytes().any(|byte| byte != b'0'))
            .unwrap_or(fields.len() - 1);

        let mut text = String::new();
        for (index, field) in fields.iter().enumerate().skip(first) {
            if index > first {
                text.push(':');
            }
            let digits = if index == first {
                field.trim_start_matches('0')
            } else {
                field
            };
            if digits.is_empty() {
                text.push('0');
            } else {
                text.push_str(digits);
            }
        }
        text
    }
}

/// Reads the row as its four fields, or `None` when it does not have the shape
/// the measurement established.
fn fields(digits: &Digits) -> Option<Fields> {
    let raw = digits.raw();

    // The timecode is digits, dark digits and one separator. Anything else in it
    // means this is some other format - Reaper's bars and beats row has no space
    // in it at all - and it is none of this function's business.
    let timecode = &raw[..TIMECODE_DIGITS];
    let shaped = timecode
        .iter()
        .all(|value| *value == DARK || *value == SEPARATOR || is_digit(*value));
    if !shaped {
        return None;
    }

    // The separator sits immediately left of the frames, and there has to be
    // room left of it for hours.
    let separator = timecode.iter().rposition(|value| *value == SEPARATOR)?;
    if separator < 2 || separator + 5 >= TIMECODE_DIGITS {
        return None;
    }

    let frames = two_digits(raw, separator - 2)?;
    let seconds = two_digits(raw, separator + 1)?;
    let minutes = two_digits(raw, separator + 3)?;
    // Everything still further left is the hours, with the DAW's dark leading
    // digits dropped so the field can be as wide as it needs to be.
    let hours: String = raw[separator + 5..TIMECODE_DIGITS]
        .iter()
        .rev()
        .filter(|value| **value != DARK)
        .map(|value| character(*value))
        .collect();

    Some(Fields {
        hours: if hours.is_empty() {
            "0".to_string()
        } else {
            hours
        },
        minutes,
        seconds,
        frames,
    })
}

/// A two-digit field whose rightmost digit is at `right`, read left to right.
///
/// A dark digit inside a field is written as a zero: the DAW turns a leading zero
/// off and keeps the column, and its own transport shows the padded form
/// (`00:01:00:00`, never `0:1:0:0`).
fn two_digits(raw: &[u8; DISPLAY_DIGITS], right: usize) -> Option<String> {
    let left = raw.get(right + 1)?;
    Some(format!(
        "{}{}",
        padded(*left),
        padded(*raw.get(right)?)
    ))
}

fn padded(value: u8) -> char {
    if value == DARK {
        '0'
    } else {
        character(value)
    }
}

/// The character of a value: bit 6 is the dot and is not part of it.
fn character(value: u8) -> char {
    (value & 0x3f) as char
}

fn is_digit(value: u8) -> bool {
    matches!(value & 0x3f, 0x30..=0x39)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row as the console echo printed it: the values in order, digit 0 first,
    /// which is the rightmost digit of the panel.
    fn row(values: [u8; DISPLAY_DIGITS]) -> Digits {
        let mut digits = Digits::default();
        for (index, value) in values.iter().enumerate() {
            digits.set(index, *value);
        }
        digits
    }

    fn hmsf(digits: &Digits) -> String {
        TimeLayout::HoursMinutesSecondsFrames.position(digits)
    }

    fn short(digits: &Digits) -> String {
        TimeLayout::HoursMinutesSecondsFrames.short_position(digits)
    }

    #[test]
    fn the_two_measured_rows_read_as_they_did_in_reaper() {
        // Stopped at 00:03:24:00 on Reaper's own transport. Leading zeros are not
        // shown, so the hours fall away and the minutes lose their zero.
        let first = row([48, 48, 32, 52, 50, 51, 48, 48, 48, 0, 49, 48]);
        assert_eq!(hmsf(&first), "3:24:00");
        assert_eq!(
            short(&first),
            "3:24",
            "and what a narrow panel shows: the seconds, with the minutes in front"
        );
        assert_eq!(
            TimeLayout::AsWritten.position(&first),
            " 000324 00",
            "while the row as written is still available untouched"
        );

        // Stopped at 00:01:00:00, the position the run ended on.
        let second = row([48, 48, 32, 48, 48, 49, 48, 48, 48, 0, 49, 48]);
        assert_eq!(hmsf(&second), "1:00:00");
        assert_eq!(short(&second), "1:00");
    }

    #[test]
    fn a_position_under_a_minute_shows_its_seconds_alone() {
        // 00:00:24:07: hours and minutes are zero and go, leading zeros and all.
        let digits = row([55, 48, 32, 52, 50, 48, 48, 48, 48, 0, 49, 48]);
        assert_eq!(hmsf(&digits), "24:07");
        assert_eq!(short(&digits), "24");
    }

    #[test]
    fn a_position_of_zero_still_reads_as_something() {
        // The DAW writing 00:00:00:00: every field is zero, and the last one is
        // kept, so the panel says `0` rather than nothing.
        let digits = row([48, 48, 32, 48, 48, 48, 48, 48, 48, 0, 49, 48]);
        assert_eq!(hmsf(&digits), "0");
        assert_eq!(short(&digits), "0");

        // A row nothing has been written to yet has no separator at all, so it is
        // not this shape and is handed on as it is - holes and all.
        let untouched = row([0; DISPLAY_DIGITS]);
        assert_eq!(hmsf(&untouched), "          ");
    }

    #[test]
    fn every_field_keeps_its_column_even_where_the_daw_turned_a_digit_off() {
        // 00:03:24:0seven: the frames' tens digit is dark, and the field still
        // reads as two digits, the way the DAW's own transport writes it.
        let digits = row([55, 0, 32, 52, 50, 51, 48, 48, 48, 0, 49, 48]);
        assert_eq!(hmsf(&digits), "3:24:07");
    }

    #[test]
    fn hours_wider_than_two_digits_are_not_cut_short() {
        // 12:34:56:29, with the dark slot still left of the hours.
        let digits = row([57, 50, 32, 54, 53, 52, 51, 50, 49, 0, 49, 48]);
        assert_eq!(hmsf(&digits), "12:34:56:29");
    }

    #[test]
    fn a_row_from_another_format_is_handed_on_exactly_as_written() {
        // The bars and beats row out of the capture: `01 25 1  000`. Its blanks
        // are dark digits, not separators, so it must not be dressed up as a
        // time - the digits go on the panel as they arrived.
        let digits = row([48, 48, 48, 0, 0, 49, 0, 53, 50, 0, 49, 48]);
        assert_eq!(
            TimeLayout::AsWritten.position(&digits),
            " 25 1  000",
            "the timecode as the panel reads it"
        );
        assert_eq!(
            hmsf(&digits),
            " 25 1  000",
            "and the hours layout leaves a row it cannot read alone"
        );
    }

    #[test]
    fn the_layouts_are_named_the_way_the_daw_names_them() {
        assert_eq!(TimeLayout::from_name("h:m:s:f"), Some(TimeLayout::HoursMinutesSecondsFrames));
        assert_eq!(TimeLayout::from_name("HMSF"), Some(TimeLayout::HoursMinutesSecondsFrames));
        assert_eq!(TimeLayout::from_name("written"), Some(TimeLayout::AsWritten));
        assert_eq!(TimeLayout::from_name("h:m:s:f").unwrap().name(), "h:m:s:f");
        assert_eq!(TimeLayout::from_name("bars"), None);
        assert_eq!(TimeLayout::default(), TimeLayout::AsWritten);
    }
}
