//! The UI thread's event loop, as plugin windows and plugins need it.
//!
//! On Windows nothing here is needed: Win32 queues belong to the thread and Tauri's loop
//! dispatches every window of it. Linux has no such global loop. X11 windows need someone to
//! read their connection, and a VST3 plugin asks the host for `Linux::IRunLoop` to watch its
//! file descriptors and run its timers; JUCE-built plugins stop their own message thread once
//! they get one (Nota's "black window, silent plugin" bug was a run loop nobody pumped).
//!
//! So the application *installs* the loop it really runs, on its UI thread, once
//! ([`install`]): in Synkopa that is GTK's glib main context. Code that needs it asks
//! [`current`], which answers only on that thread, and only if one is installed. A process
//! without one (the `wave` CLI rendering headless, a test) then offers plugins no run loop
//! at all, which is the safe answer: they keep their own message threads.
//!
//! [`PollLoop`] is a small loop of its own for tests and the editor probe: timers
//! everywhere, file descriptors on Unix (`poll`).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// A registration: what [`EventLoop::remove`] takes back. Never reused by one loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceKey(pub u64);

/// A main-thread event loop plugin code can hang work on.
///
/// Callbacks run on the loop's thread, from the top of the loop. A callback may remove its
/// own source (or any other) while it runs; the loop must not drop that callback until it
/// has returned.
pub trait EventLoop {
    /// Call `callback` whenever `fd` is readable (or hung up / in error). Unix only:
    /// elsewhere `None`.
    fn watch_fd(&self, fd: i32, callback: Box<dyn FnMut()>) -> Option<SourceKey>;
    /// Call `callback` every `period` (at least 1 ms).
    fn add_timer(&self, period: Duration, callback: Box<dyn FnMut()>) -> SourceKey;
    /// Stop a source. Unknown or already removed keys are ignored.
    fn remove(&self, key: SourceKey);
}

thread_local! {
    static INSTALLED: RefCell<Option<Rc<dyn EventLoop>>> = const { RefCell::new(None) };
}

/// Make `event_loop` this thread's loop (the UI thread's). Replaces an earlier one.
pub fn install(event_loop: Rc<dyn EventLoop>) {
    INSTALLED.with(|slot| *slot.borrow_mut() = Some(event_loop));
}

/// Forget this thread's loop (tests).
pub fn uninstall() {
    INSTALLED.with(|slot| *slot.borrow_mut() = None);
}

/// The loop installed on this thread, if any.
pub fn current() -> Option<Rc<dyn EventLoop>> {
    INSTALLED.with(|slot| slot.borrow().clone())
}

/// Whether this thread has a loop.
pub fn is_installed() -> bool {
    INSTALLED.with(|slot| slot.borrow().is_some())
}

type Callback = Rc<RefCell<Box<dyn FnMut()>>>;

struct Timer {
    period: Duration,
    due: Instant,
    callback: Callback,
}

#[derive(Default)]
struct PollState {
    next: u64,
    timers: BTreeMap<u64, Timer>,
    fds: BTreeMap<u64, (i32, Callback)>,
}

/// A self-contained loop: [`run_for`](Self::run_for) / [`run_once`](Self::run_once) turn it.
/// For tests and diagnostics, where no GUI toolkit runs a loop.
#[derive(Default)]
pub struct PollLoop {
    state: RefCell<PollState>,
}

impl PollLoop {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// Sources registered and not removed.
    pub fn len(&self) -> usize {
        let s = self.state.borrow();
        s.timers.len() + s.fds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Run every due timer and every ready descriptor once, waiting at most `wait` for
    /// something to become due. Returns how many callbacks ran.
    pub fn run_once(&self, wait: Duration) -> usize {
        let now = Instant::now();
        let next_due = self.state.borrow().timers.values().map(|t| t.due).min();
        let wait = next_due.map_or(wait, |due| wait.min(due.saturating_duration_since(now)));
        let mut ran = 0;
        for callback in self.ready_fds(wait) {
            (callback.borrow_mut())();
            ran += 1;
        }
        if ran == 0 && !self.has_fds() {
            std::thread::sleep(wait);
        }
        let now = Instant::now();
        let due: Vec<(u64, Callback)> = {
            let mut s = self.state.borrow_mut();
            s.timers
                .iter_mut()
                .filter(|(_, t)| t.due <= now)
                .map(|(key, t)| {
                    t.due = now + t.period;
                    (*key, Rc::clone(&t.callback))
                })
                .collect()
        };
        for (key, callback) in due {
            // removed by an earlier callback of this round: not run
            if self.state.borrow().timers.contains_key(&key) {
                (callback.borrow_mut())();
                ran += 1;
            }
        }
        ran
    }

    /// Turn the loop until `duration` has passed.
    pub fn run_for(&self, duration: Duration) -> usize {
        let end = Instant::now() + duration;
        let mut ran = 0;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return ran;
            }
            ran += self.run_once(left.min(Duration::from_millis(5)));
        }
    }

    fn has_fds(&self) -> bool {
        !self.state.borrow().fds.is_empty()
    }

    #[cfg(unix)]
    fn ready_fds(&self, wait: Duration) -> Vec<Callback> {
        let (keys, mut polled): (Vec<u64>, Vec<libc::pollfd>) = {
            let s = self.state.borrow();
            s.fds.iter().map(|(key, (fd, _))| (*key, libc::pollfd { fd: *fd, events: libc::POLLIN, revents: 0 })).unzip()
        };
        if polled.is_empty() {
            return Vec::new();
        }
        let timeout = i32::try_from(wait.as_millis()).unwrap_or(i32::MAX);
        // SAFETY: `polled` is a live array of `polled.len()` pollfd structures.
        let n = unsafe { libc::poll(polled.as_mut_ptr(), polled.len() as libc::nfds_t, timeout) };
        if n <= 0 {
            return Vec::new();
        }
        let s = self.state.borrow();
        keys.iter()
            .zip(&polled)
            .filter(|(_, p)| p.revents != 0)
            .filter_map(|(key, _)| s.fds.get(key).map(|(_, callback)| Rc::clone(callback)))
            .collect()
    }

    #[cfg(not(unix))]
    fn ready_fds(&self, _wait: Duration) -> Vec<Callback> {
        Vec::new()
    }
}

impl EventLoop for PollLoop {
    fn watch_fd(&self, fd: i32, callback: Box<dyn FnMut()>) -> Option<SourceKey> {
        if !cfg!(unix) {
            return None;
        }
        let mut s = self.state.borrow_mut();
        s.next += 1;
        let key = s.next;
        s.fds.insert(key, (fd, Rc::new(RefCell::new(callback))));
        Some(SourceKey(key))
    }

    fn add_timer(&self, period: Duration, callback: Box<dyn FnMut()>) -> SourceKey {
        let period = period.max(Duration::from_millis(1));
        let mut s = self.state.borrow_mut();
        s.next += 1;
        let key = s.next;
        s.timers.insert(key, Timer { period, due: Instant::now() + period, callback: Rc::new(RefCell::new(callback)) });
        SourceKey(key)
    }

    fn remove(&self, key: SourceKey) {
        // the callback itself may be running: the Rc taken for the call keeps it alive
        let removed = {
            let mut s = self.state.borrow_mut();
            (s.timers.remove(&key.0).map(|t| t.callback), s.fds.remove(&key.0).map(|(_, c)| c))
        };
        drop(removed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn timers_fire_repeatedly_until_removed_even_from_inside_themselves() {
        let lp = PollLoop::new();
        let count = Rc::new(Cell::new(0));
        let key = Rc::new(Cell::new(None));
        let (c, k, l) = (Rc::clone(&count), Rc::clone(&key), Rc::clone(&lp));
        let id = lp.add_timer(
            Duration::from_millis(1),
            Box::new(move || {
                c.set(c.get() + 1);
                if c.get() == 3 {
                    l.remove(k.get().unwrap());
                }
            }),
        );
        key.set(Some(id));
        lp.run_for(Duration::from_millis(60));
        assert_eq!(count.get(), 3, "stopped by its own removal");
        assert!(lp.is_empty());
        lp.remove(id);
    }

    #[test]
    fn a_timer_removed_by_an_earlier_one_in_the_same_round_does_not_run() {
        let lp = PollLoop::new();
        let ran = Rc::new(Cell::new(0));
        let victim = Rc::new(Cell::new(None));
        let (v, l) = (Rc::clone(&victim), Rc::clone(&lp));
        lp.add_timer(Duration::from_millis(1), Box::new(move || l.remove(v.get().unwrap())));
        let r = Rc::clone(&ran);
        victim.set(Some(lp.add_timer(Duration::from_millis(1), Box::new(move || r.set(r.get() + 1)))));
        std::thread::sleep(Duration::from_millis(3));
        lp.run_once(Duration::ZERO);
        assert_eq!(ran.get(), 0);
    }

    #[test]
    fn the_installed_loop_belongs_to_its_thread() {
        let lp = PollLoop::new();
        install(lp.clone());
        assert!(is_installed());
        assert!(!std::thread::spawn(is_installed).join().unwrap(), "another thread sees no loop");
        uninstall();
        assert!(current().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn a_readable_descriptor_calls_its_watcher() {
        let lp = PollLoop::new();
        let mut fds = [0; 2];
        // SAFETY: a fresh pipe; both ends are closed below.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let hits = Rc::new(Cell::new(0));
        let h = Rc::clone(&hits);
        let reader = fds[0];
        let key = lp
            .watch_fd(
                reader,
                Box::new(move || {
                    let mut b = [0_u8; 8];
                    // SAFETY: reading into a live local buffer.
                    unsafe { libc::read(reader, b.as_mut_ptr().cast(), b.len()) };
                    h.set(h.get() + 1);
                }),
            )
            .unwrap();
        lp.run_once(Duration::from_millis(5));
        assert_eq!(hits.get(), 0, "nothing written yet");
        // SAFETY: writing one byte from a live buffer.
        unsafe { libc::write(fds[1], b"x".as_ptr().cast(), 1) };
        lp.run_once(Duration::from_millis(50));
        assert_eq!(hits.get(), 1);
        lp.remove(key);
        // SAFETY: closing the pipe we opened.
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
    }
}
