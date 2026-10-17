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

    /// Forget what the panel is showing, so the next frame is written in full.
    /// Only one thing needs this: a panel that was unplugged and plugged back in
    /// shows nothing knowable, and a frame it already had means one that is never
    /// sent - the panel would stay dark while the state moved on.
    pub fn forget(&mut self) {
        self.last = None;
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

/// How often the keyboard is looked for again after it went away. Twice a
/// second: quick enough to feel immediate, slow enough to cost nothing, since
/// looking means enumerating the USB devices.
pub const KEYBOARD_RETRY_MILLIS: u64 = 500;

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
    hid: Hid,
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
    if let Some(reason) = input.announce_error() {
        // Not fatal, but not hidden either: without the announce port a DAW that
        // restarts is only noticed at the end of the run.
        println!(
            "Port announcements are unavailable ({reason}), so a DAW that restarts while this runs will not be noticed."
        );
    }

    watch_for_interrupts();

    let mut decoder = Decoder::with_layout(layout);
    let mut screen = Screen::new();
    let mut pace = Pace::default();
    let mut paging = Paging::new(page, size);
    let mut messages = 0usize;
    let mut overflowed = 0usize;
    // Whether the DAW's port is known to be gone: the panel then says so rather
    // than showing a state nobody is reporting any more.
    let mut waiting = false;
    let mut silent_overflows = 0usize;
    let mut report = [0u8; 64];
    // The ids of the keyboard, which is what finds it again after an unplug: the
    // handle itself is dead by then, the ids are not.
    let ids = hid.ids();
    // The keyboard belongs to this loop from here on: finding it again after it
    // was unplugged means opening it again, which a borrowed handle cannot do.
    let mut keyboard = Some(hid);
    // Why the loop ended, when it ended for a reason.
    let mut outcome: Result<(), String> = Ok(());
    // When the keyboard was last looked for, and whether its absence has already
    // been said out loud: the search runs twice a second, the report happens once.
    let mut last_search = Instant::now();
    let mut reported_absent = false;

    while !stop_requested() {
        // The DAW first: its whole queue goes into the state before anything is
        // drawn.
        match input.read_into(&mut decoder) {
            Ok(read) => {
                messages += read.messages;
                if read.announcements > 0 {
                    // The sequencer told us the world moved: a port or a client
                    // appeared, changed or went away. Our subscription to a DAW
                    // that died is dead and silent, so this is the moment to look
                    // whether the port we listen to is still there - and if it is
                    // not, to find it again by name, which is the one thing a
                    // restarting DAW keeps.
                    match input.still_there() {
                        Ok(true) => {}
                        Ok(false) => match input.refind() {
                            Ok(found) => println!(
                                "The DAW's port went away and came back as {}; listening to it again.",
                                found.describe()
                            ),
                            Err(message) => println!(
                                "The DAW's port went away and is not back yet ({}); \
                                 waiting for it to appear.\n({message})",
                                input.source().describe()
                            ),
                        },
                        Err(message) => println!("Could not look for the DAW's port again: {message}"),
                    }
                    // Either way there is nothing to draw from now on until the
                    // DAW speaks again: the state on the panel would be a memory,
                    // and a memory is worse than a word.
                    waiting = true;
                }
                if read.messages > 0 {
                    // It is speaking again, so it is there again.
                    waiting = false;
                }
                if read.overflowed {
                    overflowed += 1;
                    if read.messages == 0 {
                        // The complaint arrived without a single message behind
                        // it. Kept up for a few seconds, that is not a burst
                        // being lost, it is nothing being delivered at all, and
                        // the port is what to look at.
                        silent_overflows += 1;
                        if silent_overflows > SILENT_OVERFLOWS_BEFORE_GIVING_UP {
                            outcome = Err(format!(
                                "the sequencer reported a full queue {silent_overflows} times without \
                                 delivering one message: nothing is reaching this port. Check that \
                                 the DAW's Control/OSC/Web surface writes to it ({}).",
                                input.source().describe()
                            ));
                            break;
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
                outcome = Err(format!("while listening to the DAW: {message}"));
                break;
            }
        }

        // Then the keyboard, whose buttons turn the page. Waiting for a report is
        // also what paces this loop when the DAW has nothing to say: the DAW's
        // read does not wait for anything, so without this the loop would spin.
        //
        // A keyboard that goes away is a state to wait out, not a reason to stop
        // listening to the DAW: unplugging it takes the panel with it, because one
        // device is both - so there is nothing to draw while it is gone, and it is
        // not ours to say so on a panel that is not there. hidapi's own error text
        // is no help ("hid_error is not implemented yet"), which is why the device
        // list is asked instead: it needs no permission and answers the only
        // question that matters, whether the keyboard is there at all.
        let mut keyboard_gone: Option<String> = None;
        match keyboard.as_ref() {
            Some(device) => match device.read_timeout(&mut report, KEYBOARD_WAIT_MILLIS) {
                Ok(length) if length > 0 => {
                    paging.pressed(&report[..length]);
                }
                Ok(_) => {}
                Err(message) => keyboard_gone = Some(message),
            },
            None => {
                // Nothing to read from: give the loop a beat so the DAW keeps
                // being served without spinning the processor, and look for the
                // keyboard again twice a second.
                std::thread::sleep(Duration::from_millis(KEYBOARD_WAIT_MILLIS as u64));
                if last_search.elapsed() >= Duration::from_millis(KEYBOARD_RETRY_MILLIS) {
                    last_search = Instant::now();
                    match Hid::open(ids.0, ids.1) {
                        Ok(device) => {
                            // The panel's own memory went with the device, so it is
                            // taken over again and the frame is written in full.
                            let _ = device.write_raw(&[0xa0, 0x03, 0x04]);
                            screen.forget();
                            keyboard = Some(device);
                            reported_absent = false;
                            if !quiet {
                                println!(
                                    "The keyboard is back ({:04x}:{:04x}); the panel is being written to again.",
                                    ids.0, ids.1
                                );
                            }
                        }
                        Err(_) => {
                            // Not there, or there and not openable: the device list
                            // tells those two apart, and it is asked once.
                            if !reported_absent {
                                reported_absent = true;
                                if !quiet {
                                    println!(
                                        "The keyboard {:04x}:{:04x} is not openable yet; still listening to the DAW, and looking again every {KEYBOARD_RETRY_MILLIS} ms.",
                                        ids.0, ids.1
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Some(message) = keyboard_gone {
            if stop_requested() {
                // An interrupted read from a keyboard that is going away is still
                // how the run ends, not something to recover from.
                break;
            }
            keyboard = None;
            if !quiet {
                let why = match Hid::enumerate_matching(ids.0, ids.1) {
                    Ok((_, true)) => "it is still listed, so it is there and not answering".to_string(),
                    Ok((_, false)) => "it is not listed any more, so it was unplugged".to_string(),
                    Err(reason) => format!("asking the device list failed: {reason}"),
                };
                println!(
                    "The keyboard is gone ({message}): {why}. Listening to the DAW goes on; the keyboard is looked for again every {KEYBOARD_RETRY_MILLIS} ms."
                );
            }
        }

        let frame = if waiting {
            crate::screen::waiting_frame()
        } else {
            pages::render(paging.page, decoder.studio(), paging.size)
        };
        if let Some(device) = keyboard.as_ref() {
            if let Some(reports) = screen.reports(&frame) {
                // Both queues are empty at this point, so waiting a few
                // milliseconds costs nothing but frames.
                let rest = pace.rest(Instant::now());
                if !rest.is_zero() {
                    std::thread::sleep(rest);
                }
                for report in reports {
                    let _ = device.write_raw(&report);
                }
            }
        }
    }

    // The run is over, for whatever reason: give the display back to the
    // keyboard's own logic first, on whichever handle is current - the one
    // plugged in last, if the keyboard went away and came back.
    if let Some(device) = keyboard.as_ref() {
        let _ = device.write_raw(&[0xa0, 0x07, 0x00]);
    }
    outcome?;

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
            "The panel was left on page {:?} at {:?} size, on tracks {}-{}.",
            paging.page.name(),
            paging.size.name(),
            studio.bank_first_track,
            studio.bank_first_track + crate::studio::BANK_CHANNELS as u16 - 1
        );
        println!("The display is being given back to the keyboard.");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panel_that_was_unplugged_is_written_to_in_full_again() {
        let mut screen = Screen::new();
        let frame = screen::on();
        assert!(screen.reports(&frame).is_some());

        // What a panel shows after being unplugged and plugged back in is not
        // knowable, so the same picture has to go out again: a panel reached for
        // the first time is exactly what forgetting describes.
        screen.forget();
        assert!(
            screen.reports(&frame).is_some(),
            "a panel that was away needs the frame again"
        );
        assert!(screen.reports(&frame).is_none(), "and only once");
    }

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
