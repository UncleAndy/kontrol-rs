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
use std::thread;
use std::time::{Duration, Instant};

use crate::hid::Hid;
use crate::mcu::Decoder;
use crate::mcu_port::McuInput;
use crate::pages;
use crate::screen::{self, Frame};
use crate::studio::{Digits, Page, Size, Studio};
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

/// What the DAW says, echoed to the console, so that the panel's own display can
/// be measured from a paste rather than from a photograph of the keyboard.
///
/// It speaks on change only, and no more than four times a second: a display
/// that is moving is worth reading while it moves, an idle one is worth silence.
/// The values are printed as the DAW wrote them, because a dot, a dark digit and
/// a separator differ by the number alone - the rendering in between loses that
/// difference.
#[derive(Default)]
pub struct Echo {
    last: Option<String>,
    at: Option<Instant>,
}

impl Echo {
    pub const MIN_GAP: Duration = Duration::from_millis(250);

    /// The line to print, if this is a moment worth printing at.
    pub fn line(&mut self, studio: &Studio, digits: &Digits, now: Instant) -> Option<String> {
        let values = digits.raw_text();
        if self.last.as_deref() == Some(values.as_str()) {
            return None;
        }
        if let Some(at) = self.at {
            if now.saturating_duration_since(at) < Echo::MIN_GAP {
                return None;
            }
        }
        self.last = Some(values.clone());
        self.at = Some(now);
        Some(format!(
            "DAW display {:?}; position {:?}; transport {}; values (digit 0 is the rightmost) {}",
            digits.text(),
            studio.position,
            studio.transport.word(),
            values
        ))
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
            "Drawing page {:?} at {:?} size, reading the time display as {:?}. Nothing is sent to the DAW; Ctrl+C gives the display back.",
            page.name(),
            size.name(),
            layout.name()
        );
    }

    watch_for_interrupts();

    let mut decoder = Decoder::with_layout(layout);
    let mut screen = Screen::new();
    let mut pace = Pace::default();
    let mut echo = Echo::default();
    let mut messages = 0usize;
    let mut overflowed = 0usize;
    let mut silent_overflows = 0usize;

    while !stop_requested() {
        match input.read_into(&mut decoder) {
            Ok(read) => {
                messages += read.messages;
                if read.overflowed {
                    overflowed += 1;
                    if read.messages == 0 {
                        // The complaint arrived without a single message behind
                        // it. Kept up for a few seconds, that is not a burst
                        // being lost, it is nothing being delivered at all, and
                        // the port is what to look at - so do not spin here.
                        silent_overflows += 1;
                        if silent_overflows > SILENT_OVERFLOWS_BEFORE_GIVING_UP {
                            return Err(format!(
                                "the sequencer reported a full queue {silent_overflows} times without \
                                 delivering one message: nothing is reaching this port. Check that \
                                 the DAW's Control/OSC/Web surface writes to it ({}).",
                                input.source().describe()
                            ));
                        }
                        thread::sleep(Duration::from_millis(20));
                    }
                } else {
                    silent_overflows = 0;
                }
            }
            Err(message) => {
                // An interrupted wait is how the run ends, not a failure.
                if stop_requested() {
                    break;
                }
                return Err(format!("while listening to the DAW: {message}"));
            }
        }

        if !quiet {
            if let Some(line) = echo.line(decoder.studio(), decoder.digits(), Instant::now()) {
                println!("{line}");
            }
        }

        let frame = pages::render(page, decoder.studio(), size);
        if let Some(reports) = screen.reports(&frame) {
            // The DAW's queue is empty at this point, so waiting a few
            // milliseconds costs nothing but frames.
            let rest = pace.rest(Instant::now());
            if !rest.is_zero() {
                thread::sleep(rest);
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
    fn the_echo_speaks_on_change_and_not_more_than_four_times_a_second() {
        let studio = crate::studio::Studio::sample();
        let mut digits = Digits::default();
        let mut echo = Echo::default();
        let t0 = Instant::now();

        let line = echo
            .line(&studio, &digits, t0)
            .expect("the first row is worth saying");
        assert!(
            line.contains("DAW display \""),
            "the line carries the row as the panel shows it: {line}"
        );
        assert!(
            line.contains("0:0 "),
            "and the values as the DAW wrote them: {line}"
        );

        assert!(
            echo.line(&studio, &digits, t0 + Duration::from_secs(1)).is_none(),
            "the same row said twice is nothing new"
        );

        digits.set(0, b'7'); // the rightmost digit becomes a seven
        assert!(
            echo.line(&studio, &digits, t0 + Duration::from_millis(100)).is_none(),
            "a change inside the gap waits rather than flooding the console"
        );
        assert!(
            echo.line(&studio, &digits, t0 + Duration::from_millis(400)).is_some(),
            "once the gap has passed the change is said"
        );
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
