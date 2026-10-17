//! The panel in a thread of its own, so that a screen which fails cannot take the
//! keyboard with it.
//!
//! The owner asked for this in as many words: the screen half must never be able
//! to bring the keystrokes down, and when the screen thread dies it has to be
//! started again. Rendering is the only part of the live view that computes on a
//! whole frame, so rendering is the part that lives here; the reading loop hands
//! over a job and picks the reports up again, and waits for neither.
//!
//! Two rules make that isolation real rather than nominal:
//!
//! * **Newest wins.** Only the job that is waiting matters - a picture that has
//!   already been replaced is not worth drawing - and the slot holds exactly one.
//!   A panel that is busy, or gone, therefore cannot hold reading up: the rule the
//!   whole service follows is *keys first, panel second*.
//! * **A report is a whole frame.** The two reports of a frame travel as one
//!   message, so a message that cannot be handed over loses a frame instead of
//!   half of one. Half a frame is the one thing worse than a stale one: the
//!   panel's own cache would count it as drawn and never send it again.
//!
//! What the panel is showing is remembered *inside* the thread (`Screen`), which
//! is why a restarted thread starts by drawing whatever comes next in full.

use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::pages;
use crate::screen::{self, Frame};
use crate::studio::{Page, Size, Studio};

/// The two reports that make up one frame.
pub type Reports = [Vec<u8>; 2];

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
    pub fn reports(&mut self, frame: &Frame) -> Option<Reports> {
        if self.last.as_ref() == Some(frame) {
            return None;
        }
        self.last = Some(*frame);
        Some(screen::frame_reports(frame))
    }

    /// Forget what the panel is showing, so the next frame is written in full.
    /// One thing needs this: a panel that was unplugged and plugged back in shows
    /// nothing knowable, and a frame it already had means one that is never sent -
    /// the panel would stay dark while the state moved on.
    pub fn forget(&mut self) {
        self.last = None;
    }
}

/// The panel's rate limit: at most twenty frames a second, as
/// `doc/display-interface.md` sets out, and a waiting room the caller sleeps in.
///
/// Waiting happens *after* the state has been built, which is the whole point: a
/// fader that moves fast then costs one frame, not twenty, and the DAW's queue
/// cannot run over while the keyboard is being written to. It happens on the
/// panel's own thread now, where sleeping costs the reading side nothing at all.
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

/// What to hand the panel: the state to draw, and nothing the panel could write
/// back into. `redraw` says the panel's memory of the picture cannot be trusted -
/// it was unplugged, or this is the first frame after the thread was started
/// again - so the frame has to go out in full even if it equals the last one.
#[derive(Clone)]
pub struct Job {
    pub page: Page,
    pub size: Size,
    pub studio: Studio,
    /// Whether the panel has no DAW to listen to: the two words it shows instead.
    pub waiting: bool,
    pub redraw: bool,
}

/// The panel's doorbell: the newest job, and whether the panel has been stopped.
///
/// A condition variable rather than a channel, because a channel's receiving end
/// cannot be handed to a second thread - and starting a thread again is the whole
/// point of this module.
#[derive(Default)]
struct DoorState {
    job: Option<Job>,
    stopped: bool,
}

struct Door {
    state: Mutex<DoorState>,
    bell: Condvar,
}

impl Door {
    /// Leaves the newest job and rings. It never waits, and it never fails: a
    /// lock poisoned by a thread that panicked while holding it is taken back
    /// rather than given up on, because the alternative is a panel that is dead
    /// for good.
    fn ring(&self, job: Job) {
        let mut state = self.state.lock().unwrap_or_else(|held| held.into_inner());
        state.job = Some(job);
        drop(state);
        self.bell.notify_one();
    }

    /// Waits for a job, or for the panel to be stopped. `None` means stopped:
    /// that is the only way out of the loop on the other side.
    fn wait(&self) -> Option<Job> {
        let mut state = self.state.lock().unwrap_or_else(|held| held.into_inner());
        loop {
            if state.stopped {
                return None;
            }
            if let Some(job) = state.job.take() {
                return Some(job);
            }
            state = self
                .bell
                .wait(state)
                .unwrap_or_else(|held| held.into_inner());
        }
    }

    /// Asks the thread to end, and takes back the answer "a thread that panicked
    /// while holding this lock" by force of `into_inner` above.
    fn stop(&self) {
        let mut state = self.state.lock().unwrap_or_else(|held| held.into_inner());
        state.stopped = true;
        drop(state);
        self.bell.notify_all();
    }
}

/// The rendering thread, and the two ways in and out of it.
pub struct Panel {
    /// The doorbell the thread listens at, and the way to ring it again after a
    /// restart, which is why this is shared rather than owned by the thread.
    door: Arc<Door>,
    /// Handed to every thread, so a restarted one does not lose the way back.
    answers: SyncSender<Reports>,
    /// Reports for the reading loop to write to the device, which only it touches.
    reports: Receiver<Reports>,
    /// The thread, or `None` after it was stopped. Anything but a running thread
    /// counts as finished, because that is what the reading loop has to react to.
    handle: Option<JoinHandle<()>>,
    /// How often the thread had to be started again.
    pub restarts: usize,
}

impl Panel {
    /// Starts the thread, which draws a job at a time and lives until the panel
    /// is stopped.
    pub fn start() -> Panel {
        let door = Arc::new(Door {
            state: Mutex::new(DoorState::default()),
            bell: Condvar::new(),
        });
        // Generous: a full channel means the reading side is stuck, not that the
        // panel is behind, and frames are dropped whole rather than queued for
        // ever.
        let (answers, reports) = sync_channel::<Reports>(8);
        let handle = spawn(Arc::clone(&door), answers.clone());
        Panel {
            door,
            answers,
            reports,
            handle: Some(handle),
            restarts: 0,
        }
    }

    /// Hands the newest picture over without ever waiting: a panel that is busy -
    /// or gone - must not hold the reading side up.
    pub fn draw(&self, job: Job) {
        self.door.ring(job);
    }

    /// The reports of one finished frame, if the panel has produced any.
    pub fn reports(&self) -> Option<Reports> {
        self.reports.try_recv().ok()
    }

    /// Whether the rendering thread is not running any more, which in this
    /// program means it panicked: nothing else stops it while the reading loop is
    /// running. A panel that was stopped counts as finished too.
    pub fn is_finished(&self) -> bool {
        self.handle
            .as_ref()
            .map_or(true, |handle| handle.is_finished())
    }

    /// Starts the thread again after it ended, and says how often that has
    /// happened altogether. The reading loop calls this and sends its next job
    /// with `redraw`, because a fresh thread knows nothing about the picture the
    /// panel is showing.
    pub fn restart(&mut self) -> usize {
        self.restarts += 1;
        // A panel that is started again is running again, whatever it was told
        // before: `stop` is for shutting down, not for staying down.
        {
            let mut state = self.door.state.lock().unwrap_or_else(|held| held.into_inner());
            state.stopped = false;
        }
        let fresh = spawn(Arc::clone(&self.door), self.answers.clone());
        if let Some(old) = self.handle.replace(fresh) {
            let _ = old.join();
        }
        self.restarts
    }

    /// Ends the thread and waits for it - an orderly stop, for the service.
    pub fn stop(&mut self) {
        self.door.stop();
        if let Some(old) = self.handle.take() {
            let _ = old.join();
        }
    }
}

/// One thread: it waits at the door for the newest job, draws it, and hands the
/// reports over. It has no way to end except being stopped - which is what
/// `Panel::stop` does, and what a panic does not need to do.
fn spawn(door: Arc<Door>, answers: SyncSender<Reports>) -> JoinHandle<()> {
    thread::spawn(move || {
        // The panel's memory of its own picture. A restarted thread has none,
        // which is exactly what a restarted screen needs.
        let mut screen = Screen::new();
        let mut pace = Pace::default();

        while let Some(job) = door.wait() {
            if job.redraw {
                screen.forget();
            }
            let frame = if job.waiting {
                screen::waiting_frame()
            } else {
                pages::render(job.page, &job.studio, job.size)
            };

            if let Some(reports) = screen.reports(&frame) {
                let rest = pace.rest(Instant::now());
                if !rest.is_zero() {
                    thread::sleep(rest);
                }
                let _ = answers.try_send(reports);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A state to draw: the same one every page is rendered from in the tests of
    /// `pages`, so nothing here needs a DAW.
    fn job(studio: Studio) -> Job {
        Job {
            page: Page::Track,
            size: Size::Medium,
            studio,
            waiting: false,
            redraw: false,
        }
    }

    /// Waits for the panel to hand a frame over, up to a moment.
    fn reports(panel: &Panel) -> Option<Reports> {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Some(reports) = panel.reports() {
                return Some(reports);
            }
            thread::sleep(Duration::from_millis(5));
        }
        None
    }

    #[test]
    fn a_job_becomes_the_two_reports_of_a_frame() {
        let panel = Panel::start();
        panel.draw(job(Studio::empty()));

        let reports = reports(&panel).expect("the panel draws what it is given");
        assert_eq!(reports.len(), 2, "a frame goes out as two reports");
        assert!(!reports[0].is_empty(), "and they carry the picture");
    }

    #[test]
    fn the_same_picture_is_not_drawn_twice() {
        let panel = Panel::start();
        panel.draw(job(Studio::empty()));
        panel.draw(job(Studio::empty()));
        assert!(reports(&panel).is_some(), "the first frame goes out");

        // The same picture again is nothing to draw, and the rate limit means
        // waiting longer than the gap is enough to be sure.
        panel.draw(job(Studio::empty()));
        thread::sleep(Pace::MIN_GAP * 3);
        assert!(
            reports(&panel).is_none(),
            "an unchanged picture must not be sent again"
        );

        // A redraw is for a panel whose picture cannot be trusted: it goes out
        // even though nothing changed.
        panel.draw(Job {
            redraw: true,
            ..job(Studio::empty())
        });
        assert!(
            reports(&panel).is_some(),
            "a panel that was away needs the frame again"
        );
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
    fn a_panel_that_stopped_is_noticed_and_started_again() {
        let mut panel = Panel::start();
        panel.draw(job(Studio::empty()));
        assert!(reports(&panel).is_some());

        panel.stop();
        assert!(panel.is_finished(), "a stopped panel is a finished thread");

        assert_eq!(panel.restart(), 1, "the first restart counts as one");
        panel.draw(job(Studio::empty()));
        assert!(
            reports(&panel).is_some(),
            "a restarted panel draws in full, because it remembers nothing"
        );
    }

    #[test]
    fn a_busy_panel_does_not_hold_the_caller_up() {
        let panel = Panel::start();
        // Far more pictures than the panel can draw in the time: every call has
        // to return at once, and only the newest one is worth drawing.
        let started = Instant::now();
        for _ in 0..200 {
            panel.draw(job(Studio::empty()));
        }
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "drawing must never wait for the panel: {:?}",
            started.elapsed()
        );
    }
}
