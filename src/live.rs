//! The live view: what the DAW says, drawn on the panel right now.
//!
//! This is the decoder made visible, not the daemon. One page at one size,
//! redrawn whenever the picture changes, and nothing else: paging by the two
//! buttons, messages and the redraw limit belong to the daemon (steps 3 and 4 of
//! `doc/display-interface.md`). It exists so that the decoder can be judged on
//! the keyboard with a real DAW attached, which is the only thing that can prove
//! it right.
//!
//! Everything here has been checked without the DAW: what can be tested is the
//! picture rule (`Screen`) and the names (`page_and_size`), and the stream
//! itself is tested against the capture in `tests/mcu_capture.rs`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::engine;
use crate::hid::Hid;
use crate::mcu::Decoder;
use crate::mcu_port::McuInput;
use crate::pages;
use crate::screen::{self, Frame};
use crate::studio::{Page, Size};
use crate::timecode::TimeLayout;

/// Set from the signal handler; the loop stops when it becomes true.
static STOP: AtomicBool = AtomicBool::new(false);

/// How many full-queue complaints in a row, each with nothing behind it, mean
/// nothing is being delivered rather than a burst being lost. At twenty
/// milliseconds a piece this is about five seconds.
const SILENT_OVERFLOWS_BEFORE_GIVING_UP: usize = 250;

extern "C" fn handle_interrupt(_signal: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

/// True once Ctrl+C (or SIGTERM) has been seen.
pub fn stop_requested() -> bool {
    STOP.load(Ordering::SeqCst)
}

/// Takes the interrupt the way this loop needs it: **without** `SA_RESTART`, so
/// that a blocking wait for the DAW comes back with an error instead of being
/// restarted. That is what lets Ctrl+C end the run promptly even when the DAW
/// has gone quiet - and the DAW *does* go quiet, the capture showed eight
/// seconds of nothing while it was idle.
pub fn watch_for_interrupts() {
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handle_interrupt as *const () as libc::sighandler_t;
        action.sa_flags = 0;
        libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut());
        libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut());
    }
}

/// The panel together with the picture it was last given.
///
/// A frame the panel already shows does not need sending again - the rule the
/// daemon will follow too, so the two can be compared.
#[derive(Default)]
pub struct Screen {
    last: Option<Frame>,
}

impl Screen {
    pub fn new() -> Screen {
        Screen { last: None }
    }

    /// The two reports that put `frame` on the panel, or `None` when that is the
    /// picture it is already showing.
    pub fn reports(&mut self, frame: &Frame) -> Option<[Vec<u8>; 2]> {
        if self.last.as_ref() == Some(frame) {
            return None;
        }
        self.last = Some(*frame);
        Some(screen::frame_reports(frame))
    }
}

/// The panel's rate limit: at most twenty frames a second, as
/// `doc/display-interface.md` sets out, and a waiting room the caller sleeps in.
///
/// Waiting happens *after* the DAW's queue has been drained into the state, which
/// is the whole point: a fader that moves fast then costs one frame, not twenty,
/// and the queue cannot run over while the keyboard is being written to.
#[derive(Default)]
pub struct Pace {
    last: Option<Instant>,
}

impl Pace {
    /// The shortest gap between two frames on the panel.
    pub const MIN_GAP: Duration = Duration::from_millis(50);

    /// How long to wait before drawing, and a note that the frame went out.
    pub fn rest(&mut self, now: Instant) -> Duration {
        let rest = self
            .last
            .map(|last| Pace::MIN_GAP.saturating_sub(now.saturating_duration_since(last)))
            .unwrap_or(Duration::ZERO);
        self.last = Some(now + rest);
        rest
    }
}

/// How long the loop waits for the keyboard before it looks at the DAW again.
/// Nothing is lost by waiting: the DAW's queue is drained in full each time round,
/// and no message is older than this.
const KEYBOARD_WAIT_MILLIS: i32 = 20;

/// The buttons that turn the page, as `engine::button_mask` numbers them: the two
/// page buttons are bits 14 and 15 of the mask (measured: bit 14 is Page Left and
/// bit 15 Page Right in the third report byte), and SHIFT is bit 0.
const PAGE_LEFT: u64 = 1 << 14;
const PAGE_RIGHT: u64 = 1 << 15;
const SHIFT: u64 = 1;

/// Which page at which size the panel is showing, and what its buttons do.
///
/// The page buttons turn the page and SHIFT with them changes the size, which is
/// how the small and the large layout are reached while the DAW plays. A *press*
/// turns a page, never a hold. The buttons do not repeat themselves (measured:
/// one packet per press, one per release), but the edge is what is counted all
/// the same: a report arriving twice in a row cannot turn two pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paging {
    pub page: Page,
    pub size: Size,
    held: u64,
}

impl Paging {
    pub fn new(page: Page, size: Size) -> Paging {
        Paging {
            page,
            size,
            held: 0,
        }
    }

    /// Reads one report from the keyboard and acts on the buttons it carries.
    /// Returns whether the picture has to be redrawn.
    pub fn pressed(&mut self, report: &[u8]) -> bool {
        let now = engine::button_mask(report);
        let pressed = now & !self.held;
        self.held = now;
        let shift = now & SHIFT != 0;

        let mut changed = false;
        for (button, forwards) in [(PAGE_LEFT, false), (PAGE_RIGHT, true)] {
            if pressed & button == 0 {
                continue;
            }
            if shift {
                self.size = if forwards {
                    self.size.next()
                } else {
                    self.size.previous()
                };
            } else {
                self.page = if forwards {
                    self.page.next()
                } else {
                    self.page.previous()
                };
            }
            changed = true;
        }
        changed
    }
}

/// The page and size a name like `page-track:large` asks for. A name without a
/// colon keeps `default_page`; one without a size keeps `default_size`. This is
/// the same convention the named states use, so `-S` can serve both.
pub fn page_and_size(state: &str, default_page: Page, default_size: Size) -> (Page, Size) {
    let (page_name, size_name) = match state.split_once(':') {
        Some((page, size)) => (page, Some(size)),
        None => (state, None),
    };

    let page = Page::from_name(page_name).unwrap_or(default_page);
    let size = size_name.and_then(Size::from_name).unwrap_or(default_size);
    (page, size)
}

/// Listens to the DAW's MCU port and draws one page until interrupted.
///
/// The port is opened by the caller rather than here, so that a name that
/// matches nothing is reported before the display is taken over: a typo should
/// not make the panel flicker.
pub fn run(
    hid: &Hid,
    mut input: McuInput,
    page: Page,
    size: Size,
    quiet: bool,
    layout: TimeLayout,
) -> Result<(), String> {
    if !quiet {
        println!(
            "Listening for Mackie Control Universal messages from {}; our own port is {} ({}).",
            input.source().describe(),
            input.port(),
            crate::mcu_port::CLIENT_NAME
        );
        println!(
            "Drawing page {:?} at {:?} size, reading the time display as {:?}. The two page buttons turn the page, with SHIFT they change the size. Nothing is sent to the DAW; Ctrl+C gives the display back.",
            page.name(),
            size.name(),
            layout.name()
        );
    }

    watch_for_interrupts();

    let mut decoder = Decoder::with_layout(layout);
    let mut screen = Screen::new();
    let mut pace = Pace::default();
    let mut paging = Paging::new(page, size);
    let mut messages = 0usize;
    let mut overflowed = 0usize;
    let mut silent_overflows = 0usize;
    let mut report = [0u8; 64];

    while !stop_requested() {
        // The DAW first: its whole queue goes into the state before anything is
        // drawn.
        match input.read_into(&mut decoder) {
            Ok(read) => {
                messages += read.messages;
                if read.overflowed {
                    overflowed += 1;
                    if read.messages == 0 {
                        // The complaint arrived without a single message behind
                        // it. Kept up for a few seconds, that is not a burst
                        // being lost, it is nothing being delivered at all, and
                        // the port is what to look at.
                        silent_overflows += 1;
                        if silent_overflows > SILENT_OVERFLOWS_BEFORE_GIVING_UP {
                            return Err(format!(
                                "the sequencer reported a full queue {silent_overflows} times without \
                                 delivering one message: nothing is reaching this port. Check that \
                                 the DAW's Control/OSC/Web surface writes to it ({}).",
                                input.source().describe()
                            ));
                        }
                    }
                } else {
                    silent_overflows = 0;
                }
            }
            Err(message) => {
                // An interrupted read is how the run ends, not a failure.
                if stop_requested() {
                    break;
                }
                return Err(format!("while listening to the DAW: {message}"));
            }
        }

        // Then the keyboard, whose buttons turn the page. Waiting for a report is
        // also what paces this loop when the DAW has nothing to say: the DAW's
        // read does not wait for anything, so without this the loop would spin.
        match hid.read_timeout(&mut report, KEYBOARD_WAIT_MILLIS) {
            Ok(length) if length > 0 => {
                paging.pressed(&report[..length]);
            }
            Ok(_) => {}
            Err(message) => {
                if !stop_requested() {
                    return Err(format!("while reading the keyboard: {message}"));
                }
            }
        }

        let frame = pages::render(paging.page, decoder.studio(), paging.size);
        if let Some(reports) = screen.reports(&frame) {
            // Both queues are empty at this point, so waiting a few milliseconds
            // costs nothing but frames.
            let rest = pace.rest(Instant::now());
            if !rest.is_zero() {
                std::thread::sleep(rest);
            }
            for report in reports {
                let _ = hid.write_raw(&report);
            }
        }
    }

    if !quiet {
        let studio = decoder.studio();
        println!(
            "Stopped after {messages} messages{}.",
            if overflowed > 0 {
                format!(" ({overflowed} reads came with a full queue, so a few values are missing)")
            } else {
                String::new()
            }
        );
        println!(
            "Last state: transport {}, position {:?}, display {:?}, track {} {:?}, first channel {:?}.",
            studio.transport.word(),
            studio.position,
            studio.display,
            studio.track_number,
            studio.track_name,
            studio.bank.first().map(|channel| channel.name.as_str()).unwrap_or("")
        );
        println!(
            "The panel was left on page {:?} at {:?} size.",
            paging.page.name(),
            paging.size.name()
        );
        println!("The display is being given back to the keyboard.");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_is_sent_once_and_only_again_when_it_changes() {
        let mut screen = Screen::new();
        let first = screen::on();

        let reports = screen.reports(&first).expect("the first frame is always sent");
        assert_eq!(reports.len(), 2, "a frame goes out as two reports");

        assert!(
            screen.reports(&first).is_none(),
            "an unchanged picture must not be sent again"
        );

        let mut second = first;
        second[0] ^= 0x01;
        assert!(screen.reports(&second).is_some(), "a changed picture must be sent");

        // And going back to a picture already shown is a change again, because
        // only the *last* frame is remembered.
        assert!(screen.reports(&first).is_some());
    }

    #[test]
    fn frames_never_go_out_faster_than_twenty_a_second() {
        let t0 = Instant::now();
        let mut pace = Pace::default();

        assert_eq!(
            pace.rest(t0),
            Duration::ZERO,
            "the first frame goes out at once"
        );
        assert_eq!(
            pace.rest(t0 + Duration::from_millis(10)),
            Duration::from_millis(40),
            "ten milliseconds after a frame, the next one waits the rest of the gap"
        );
        assert_eq!(
            pace.rest(t0 + Duration::from_millis(60)),
            Duration::from_millis(40),
            "the last frame went out at t0 + 50, so this one waits until t0 + 100"
        );
        assert_eq!(
            pace.rest(t0 + Duration::from_millis(1000)),
            Duration::ZERO,
            "after an idle second there is nothing to wait for"
        );
    }

    #[test]
    fn the_page_and_size_of_a_name_are_read_the_way_the_states_write_them() {
        assert_eq!(
            page_and_size("page-track:large", Page::Transport, Size::Small),
            (Page::Track, Size::Large)
        );
        assert_eq!(
            page_and_size("page-general", Page::Transport, Size::Medium),
            (Page::General, Size::Medium)
        );
        assert_eq!(
            page_and_size("tracks:s", Page::Transport, Size::Medium),
            (Page::Tracks, Size::Small)
        );
        // Nothing useful in the name leaves both defaults alone.
        assert_eq!(
            page_and_size("", Page::Track, Size::Medium),
            (Page::Track, Size::Medium)
        );
        assert_eq!(
            page_and_size("nonsense:nonsense", Page::Track, Size::Medium),
            (Page::Track, Size::Medium)
        );
    }

    #[test]
    fn the_page_buttons_turn_the_page_and_shift_changes_the_size() {
        // The page buttons are the top two bits of the mask's second byte - the
        // key 14 and key 15 the button table names - and SHIFT is bit 0.
        assert_eq!(engine::button_mask(&report(&[0x00, 0x40])), PAGE_LEFT);
        assert_eq!(engine::button_mask(&report(&[0x00, 0x80])), PAGE_RIGHT);
        assert_eq!(engine::button_mask(&report(&[0x01])), SHIFT);

        let mut paging = Paging::new(Page::Transport, Size::Medium);

        // A press, and then the report that says the key was let go: the keyboard
        // repeats a button that stays down, and only the press counts.
        fn press(paging: &mut Paging, keys: &[u8]) -> bool {
            let acted = paging.pressed(&report(keys));
            paging.pressed(&report(&[0x00]));
            acted
        }

        // Page Right: the next page, in the order the pages are listed.
        assert!(press(&mut paging, &[0x00, 0x80]));
        assert_eq!(paging.page, Page::Track);
        assert_eq!(
            paging.size,
            Size::Medium,
            "the size is not the page buttons' business"
        );

        // Page Left: back, and past the first page it wraps around.
        assert!(press(&mut paging, &[0x00, 0x40]));
        assert_eq!(paging.page, Page::Transport);
        assert!(press(&mut paging, &[0x00, 0x40]));
        assert_eq!(paging.page, Page::General, "the pages wrap around");

        // SHIFT is held down, and with it the same buttons change the size.
        assert!(press(&mut paging, &[0x01, 0x80]));
        assert_eq!(paging.size, Size::Large);
        assert_eq!(
            paging.page,
            Page::General,
            "SHIFT changes the size, not the page"
        );
        assert!(press(&mut paging, &[0x01, 0x40]));
        assert_eq!(paging.size, Size::Medium);

        // A button that stays down is one press, however many reports the
        // keyboard sends: four pages would race past on one finger.
        let down = report(&[0x00, 0x80]);
        assert!(paging.pressed(&down), "the press itself turns the page");
        assert_eq!(paging.page, Page::Transport);
        assert!(
            !paging.pressed(&down),
            "the keyboard repeating it is not another press"
        );
        assert!(!paging.pressed(&down));
        assert_eq!(paging.page, Page::Transport);
        // And letting go arms it again.
        paging.pressed(&report(&[0x00]));
        assert!(press(&mut paging, &[0x00, 0x80]));
        assert_eq!(paging.page, Page::Track);
    }

    #[test]
    fn a_report_without_the_page_buttons_changes_nothing() {
        let mut paging = Paging::new(Page::Tracks, Size::Small);
        // A report from some other button entirely, and the report the keyboard
        // sends when everything is let go.
        assert!(!paging.pressed(&report(&[0x00, 0x20])));
        assert!(!paging.pressed(&report(&[0x00])));
        assert_eq!(paging, Paging::new(Page::Tracks, Size::Small));
    }

    /// A 30-byte report as the keyboard sends it, with `bytes` starting at the
    /// first byte of the button mask - the layout `engine::button_mask` reads.
    fn report(bytes: &[u8]) -> Vec<u8> {
        let mut packet = vec![0u8; 30];
        for (index, byte) in bytes.iter().enumerate() {
            packet[1 + index] = *byte;
        }
        packet
    }

    #[test]
    fn every_page_draws_something_a_panel_can_show() {
        // The empty state is what the DAW's first message lands on, so no page
        // may be blank there: a blank one would look like a dead screen.
        let studio = crate::studio::Studio::empty();
        for page in Page::ALL {
            for size in [Size::Small, Size::Medium, Size::Large] {
                let frame = pages::render(page, &studio, size);
                assert_eq!(frame.len(), screen::FRAME_BYTES);
            }
        }
    }
}
