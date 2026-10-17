//! The state the screen shows and the pages it can show it on: the model half of
//! `doc/display-interface.md`.
//!
//! Nothing here knows where a value came from. The sources (MCU over MIDI, the
//! message FIFO) fill the state in; the pages read it and the size decides how
//! much of a page fits. Keeping the three apart is what lets the screen be built
//! and judged before either source exists.

/// Glyph sizes the screen can draw in. The cycle is what SHIFT plus a page
/// button steps through, and the size alone decides how many lines fit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    /// Four lines of the 5x7 font, twenty-one characters each.
    Small,
    /// Three lines of the 7x9 font, sixteen characters each.
    Medium,
    /// Two lines at double scale, ten characters each.
    Large,
}

impl Size {
    /// The next size in the cycle.
    pub fn next(self) -> Size {
        match self {
            Size::Small => Size::Medium,
            Size::Medium => Size::Large,
            Size::Large => Size::Small,
        }
    }

    /// The size a `-S` name asks for after its colon, if it is one of them.
    pub fn from_name(name: &str) -> Option<Size> {
        match name {
            "small" | "s" => Some(Size::Small),
            "medium" | "med" | "m" => Some(Size::Medium),
            "large" | "big" | "l" => Some(Size::Large),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Size::Small => "small",
            Size::Medium => "medium",
            Size::Large => "large",
        }
    }

    /// Characters that fit on one line at this size.
    pub fn line_characters(self) -> usize {
        match self {
            Size::Small => crate::font::characters_per_line(crate::screen::WIDTH),
            Size::Medium => crate::font_medium::characters_per_line(crate::screen::WIDTH),
            Size::Large => {
                crate::font::scaled_characters_per_line(crate::screen::WIDTH, LARGE_SCALE)
            }
        }
    }

    /// How many lines fit at this size.
    pub fn line_count(self) -> usize {
        match self {
            Size::Small => 4,
            Size::Medium => 3,
            Size::Large => 2,
        }
    }

    /// The pixel row line `index` starts on, or `None` when the size has no such
    /// line.
    pub fn line_row(self, index: usize) -> Option<usize> {
        let rows = self.line_rows();
        rows.get(index).copied()
    }

    pub fn line_rows(self) -> &'static [usize] {
        match self {
            Size::Small => &[0, 8, 16, 24],
            Size::Medium => &[0, 11, 22],
            Size::Large => &[0, 16],
        }
    }

    /// Pixel columns one character takes, with its spacing.
    pub fn character_advance(self) -> usize {
        match self {
            Size::Small => crate::font::GLYPH_WIDTH + crate::font::GLYPH_SPACING,
            Size::Medium => crate::font_medium::GLYPH_WIDTH + crate::font_medium::GLYPH_SPACING,
            Size::Large => {
                (crate::font::GLYPH_WIDTH + crate::font::GLYPH_SPACING) * LARGE_SCALE
            }
        }
    }

    /// Pixel columns a string of `characters` characters occupies.
    pub fn text_width(self, characters: usize) -> usize {
        if characters == 0 {
            0
        } else {
            characters * self.character_advance() - self.character_advance()
                + self.glyph_width()
        }
    }

    /// Pixel columns one glyph body takes.
    pub fn glyph_width(self) -> usize {
        match self {
            Size::Small => crate::font::GLYPH_WIDTH,
            Size::Medium => crate::font_medium::GLYPH_WIDTH,
            Size::Large => crate::font::GLYPH_WIDTH * LARGE_SCALE,
        }
    }

    /// Pixel rows one glyph body takes.
    pub fn glyph_height(self) -> usize {
        match self {
            Size::Small => crate::font::GLYPH_HEIGHT,
            Size::Medium => crate::font_medium::GLYPH_HEIGHT,
            Size::Large => crate::font::GLYPH_HEIGHT * LARGE_SCALE,
        }
    }
}

/// Scale of the large size.
pub const LARGE_SCALE: usize = 2;

/// What the transport is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    Stopped,
    Playing,
    Recording,
}

impl Transport {
    pub fn word(self) -> &'static str {
        match self {
            Transport::Stopped => "STOP",
            Transport::Playing => "PLAY",
            Transport::Recording => "REC",
        }
    }
}

/// One channel of the mixer bank: the eight tracks MCU sends names, fader
/// positions and mute/solo states for at a time.
#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    pub name: String,
    /// Where the fader sits, 0.0 (bottom) to 1.0 (top).
    pub fader: f32,
    pub mute: bool,
    pub solo: bool,
}

impl Channel {
    fn new(name: &str, fader: f32, mute: bool, solo: bool) -> Channel {
        Channel {
            name: name.to_string(),
            fader,
            mute,
            solo,
        }
    }
}

/// The pages, in the order the two buttons beside the display walk them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// Position, transport state and its flags.
    Transport,
    /// The selected track: name, volume, pan, mute/solo/arm.
    Track,
    /// The bank's tracks as text, most important first.
    Tracks,
    /// The bank as a mixer: one column per channel.
    General,
}

impl Page {
    /// Every page, in ring order.
    pub const ALL: [Page; 4] = [Page::Transport, Page::Track, Page::Tracks, Page::General];

    pub fn next(self) -> Page {
        match self {
            Page::Transport => Page::Track,
            Page::Track => Page::Tracks,
            Page::Tracks => Page::General,
            Page::General => Page::Transport,
        }
    }

    pub fn previous(self) -> Page {
        match self {
            Page::Transport => Page::General,
            Page::Track => Page::Transport,
            Page::Tracks => Page::Track,
            Page::General => Page::Tracks,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Page::Transport => "transport",
            Page::Track => "track",
            Page::Tracks => "tracks",
            Page::General => "general",
        }
    }

    /// The page a `-S` state name asks for, if it is one of them. Both `track`
    /// and `page-track` name the same page.
    pub fn from_name(name: &str) -> Option<Page> {
        let name = name.strip_prefix("page-").unwrap_or(name);
        Page::ALL.into_iter().find(|page| page.name() == name)
    }
}

/// Everything the pages read. Fields are filled by whichever source knows them,
/// so a source that is not there leaves the rest of the screen intact.
#[derive(Clone, Debug, PartialEq)]
pub struct Studio {
    pub track_number: u16,
    pub track_name: String,
    /// The volume as text, as the DAW spells it.
    pub volume: String,
    /// Where the volume sits in its range, for the bar.
    pub volume_fraction: f32,
    pub pan: String,
    pub pan_fraction: f32,
    pub mute: bool,
    pub solo: bool,
    pub armed: bool,
    pub position: String,
    /// The same position in the form a narrow panel can carry: the part of it
    /// that says where the DAW is right now. Empty when the stream's format has
    /// nothing shorter to say, and then the full text is what gets clipped.
    pub position_short: String,
    /// The DAW's whole twelve-digit row as text, holes and all: what its own
    /// display shows, next to the timecode `position` holds on its own.
    pub display: String,
    pub transport: Transport,
    pub looping: bool,
    pub click: bool,
    /// The bank MCU is showing: eight channels.
    pub bank: Vec<Channel>,
    /// The number of the bank's first channel, so the screen can say "1-8".
    pub bank_first_track: u16,
}

/// Channels in one MCU bank, which is how many the bank view holds.
pub const BANK_CHANNELS: usize = 8;

/// Digits in the DAW's own display, the row of seven-segment characters it
/// writes over controllers 64..75.
pub const DISPLAY_DIGITS: usize = 12;

/// How many of those digits are the timecode: the ten rightmost ones. The two
/// left of them are the assignment digits, which the protocol numbers with the
/// same row.
pub const TIMECODE_DIGITS: usize = 10;

/// The DAW's row of seven-segment digits, as it was written.
///
/// Two measured rules live here. The row is numbered **from the right**: the
/// protocol's controller 64 is the rightmost digit, so a fixed index means a
/// fixed place on the panel however the DAW updates it, one digit at a time.
/// And the value written is the character with **bit 6 (0x40) as the dot**, so
/// `0x30` is `0`, `0x70` is `0.` and a zero means that digit is dark - which is
/// why a display read from the capture can have holes in it where the DAW turned
/// a leading zero off.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Digits([u8; DISPLAY_DIGITS]);

impl Digits {
    /// Writes one digit, counting from the right as the protocol does.
    /// Returns whether this changed anything, so a caller can redraw only then.
    pub fn set(&mut self, index: usize, value: u8) -> bool {
        if index >= DISPLAY_DIGITS || self.0[index] == value {
            return false;
        }
        self.0[index] = value;
        true
    }

    /// The whole row as the panel shows it, left to right.
    pub fn text(&self) -> String {
        self.render(0..DISPLAY_DIGITS)
    }

    /// The values exactly as the DAW wrote them, digit zero first. What a
    /// measurement needs: the dot bit, a dark digit and a separator are only
    /// told apart by the value itself.
    pub fn raw(&self) -> &[u8; DISPLAY_DIGITS] {
        &self.0
    }

    /// The timecode part of it: the ten rightmost digits.
    pub fn timecode(&self) -> String {
        self.render(0..TIMECODE_DIGITS)
    }

    /// Renders a run of digits the way the panel reads it - rightmost digit
    /// last - so the caller can ask for the whole row or just the timecode.
    fn render(&self, digits: std::ops::Range<usize>) -> String {
        let mut text = String::with_capacity(digits.len() * 2);
        for index in digits.rev() {
            let value = self.0[index];
            // Bit 6 is the dot, not part of the character.
            let character = (value & 0x3f) as char;
            if character == '\0' || character == ' ' {
                text.push(' ');
            } else {
                text.push(character);
            }
            if value & 0x40 != 0 {
                text.push('.');
            }
        }
        text
    }
}


impl Studio {
    /// An empty state: what the screen shows before a source has said anything.
    /// The bank is there from the start, so a name that arrives later has a
    /// place to land and the mixer page always has its eight columns.
    pub fn empty() -> Studio {
        Studio {
            track_number: 1,
            track_name: String::new(),
            volume: String::new(),
            volume_fraction: 0.0,
            pan: String::new(),
            pan_fraction: 0.5,
            mute: false,
            solo: false,
            armed: false,
            position: String::new(),
            position_short: String::new(),
            display: String::new(),
            transport: Transport::Stopped,
            looping: false,
            click: false,
            bank_first_track: 1,
            bank: (0..BANK_CHANNELS)
                .map(|_| Channel::new("", 0.0, false, false))
                .collect(),
        }
    }

    /// A filled-in state for looking at the pages while no source is connected.
    /// Every field the pages use is set to something, so a layout mistake is
    /// visible on the panel rather than hidden behind a dash.
    pub fn sample() -> Studio {
        Studio {
            track_number: 2,
            track_name: "BASS DI".to_string(),
            volume: "-6.1 DB".to_string(),
            volume_fraction: 0.62,
            pan: "C".to_string(),
            pan_fraction: 0.5,
            mute: false,
            solo: true,
            armed: true,
            position: "3:24:00".to_string(),
            position_short: "3:24".to_string(),
            display: "01 000324 00".to_string(),
            transport: Transport::Playing,
            looping: true,
            click: false,
            bank_first_track: 1,
            bank: vec![
                Channel::new("DRUMS", 0.78, false, false),
                Channel::new("BASS DI", 0.62, false, true),
                Channel::new("GTR L", 0.55, false, false),
                Channel::new("GTR R", 0.51, false, false),
                Channel::new("KEYS", 0.44, true, false),
                Channel::new("PAD", 0.31, false, false),
                Channel::new("VOX", 0.85, false, false),
                Channel::new("FX", 0.18, true, true),
            ],
        }
    }

    /// The flags of a channel as one short string, for the text pages.
    pub fn flags(&self) -> String {
        format!(
            "{} {} {}",
            if self.mute { "M" } else { "-" },
            if self.solo { "S" } else { "-" },
            if self.armed { "ARM" } else { "---" }
        )
    }

    /// The transport flags as one short string, empty when nothing is set.
    pub fn transport_flags(&self) -> String {
        let mut flags: Vec<&str> = Vec::new();
        if self.looping {
            flags.push("LOOP");
        }
        if self.click {
            flags.push("CLICK");
        }
        flags.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_size_cycle_walks_all_three_and_returns() {
        assert_eq!(Size::Small.next(), Size::Medium);
        assert_eq!(Size::Medium.next(), Size::Large);
        assert_eq!(Size::Large.next(), Size::Small);
    }

    #[test]
    fn a_smaller_size_fits_more_lines_and_a_bigger_one_fits_more_pixels() {
        assert_eq!(Size::Small.line_count(), 4);
        assert_eq!(Size::Medium.line_count(), 3);
        assert_eq!(Size::Large.line_count(), 2);

        assert!(Size::Small.line_characters() > Size::Medium.line_characters());
        assert!(Size::Medium.line_characters() > Size::Large.line_characters());

        // A line always fits inside the screen, at every size.
        for size in [Size::Small, Size::Medium, Size::Large] {
            let capacity = size.line_characters();
            assert!(
                size.text_width(capacity) <= crate::screen::WIDTH,
                "{} characters do not fit at {:?}",
                capacity,
                size
            );
            assert!(
                size.line_rows()[size.line_count() - 1] + size.glyph_height()
                    <= crate::screen::PAGES * 8,
                "the last line of {:?} runs off the screen",
                size
            );
        }
    }

    #[test]
    fn every_line_row_exists_and_the_sizes_do_not_agree_on_them() {
        assert_eq!(Size::Small.line_rows(), &[0, 8, 16, 24]);
        assert_eq!(Size::Medium.line_rows(), &[0, 11, 22]);
        assert_eq!(Size::Large.line_rows(), &[0, 16]);

        for size in [Size::Small, Size::Medium, Size::Large] {
            assert!(size.line_row(size.line_count() - 1).is_some());
            assert_eq!(size.line_row(size.line_count()), None);
        }
    }

    #[test]
    fn the_pages_form_a_ring() {
        for (index, page) in Page::ALL.iter().enumerate() {
            assert_eq!(page.next(), Page::ALL[(index + 1) % Page::ALL.len()]);
            assert_eq!(page.previous(), Page::ALL[(index + Page::ALL.len() - 1) % Page::ALL.len()]);
            assert_eq!(Page::from_name(page.name()), Some(*page));
            assert_eq!(Page::from_name(&format!("page-{}", page.name())), Some(*page));
        }
        assert_eq!(Page::from_name("nonsense"), None);
        assert_eq!(Page::from_name("page-nonsense"), None);
    }

    #[test]
    fn the_sample_state_uses_every_field_the_pages_read() {
        let studio = Studio::sample();
        assert!(!studio.track_name.is_empty());
        assert!(!studio.volume.is_empty());
        assert!(!studio.position.is_empty());
        assert!(!studio.bank.is_empty());
        assert!(studio.volume_fraction > 0.0 && studio.volume_fraction < 1.0);
        // At least one flag set, so the flags column is visible in a sample.
        assert!(studio.mute || studio.solo || studio.armed);
        assert!(studio.bank.iter().any(|channel| channel.mute));
        assert!(studio.bank.iter().any(|channel| channel.solo));
    }

    #[test]
    fn the_flag_strings_say_what_is_set() {
        let studio = Studio::sample();
        assert_eq!(studio.flags(), "- S ARM");

        let quiet = Studio {
            mute: true,
            solo: false,
            armed: false,
            looping: false,
            click: false,
            ..Studio::sample()
        };
        assert_eq!(quiet.flags(), "M - ---");
        assert_eq!(quiet.transport_flags(), "");

        let both = Studio {
            looping: true,
            click: true,
            ..Studio::sample()
        };
        assert_eq!(both.transport_flags(), "LOOP CLICK");
    }
}
