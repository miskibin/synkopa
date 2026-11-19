//! `Steinberg::Linux::IRunLoop`: the host's event loop, lent to a plugin.
//!
//! Linux has no global UI event loop, so VST3 makes the host provide one: through the
//! editor's `IPlugFrame` and through the host context a plugin is initialised with, so it
//! has one even without an editor. A plugin registers file descriptors to be told about
//! (`IEventHandler::onFDIsSet`, e.g. its X11 connection or a wake-up pipe) and timers
//! (`ITimerHandler::onTimer`, e.g. repaints). JUCE- and DPF-built plugins depend on it:
//! JUCE stops its own message thread once it gets one, so a loop that is offered and then
//! never turned leaves the plugin frozen and silent (the bug Nota had on Linux).
//!
//! [`RunLoop`] maps the registrations onto the event loop the application installed on its
//! UI thread ([`wave_plugin_windows::event_loop`]; glib in Synkopa). Registering anywhere
//! else (another thread, or a process with no loop) is refused, and the host context only
//! *offers* the interface at all where a loop is installed. Every registration is tied to
//! the plugin instance and removed when it is torn down, whatever the plugin forgot, so the
//! loop can never call into an unloaded library.

use std::time::Duration;

use parking_lot::Mutex;
use vst3::ComRef;
use vst3::Steinberg::Linux::{FileDescriptor, IEventHandler, IEventHandlerTrait, ITimerHandler, ITimerHandlerTrait, TimerInterval};
use vst3::Steinberg::{kInvalidArgument, kResultFalse, kResultOk, tresult};
use wave_plugin_windows::event_loop::{self, SourceKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Fd,
    Timer,
}

struct Entry {
    kind: Kind,
    /// The handler's interface pointer (its identity for unregistering).
    handler: usize,
    key: SourceKey,
}

/// One plugin instance's registrations.
#[derive(Default)]
pub(crate) struct RunLoop {
    entries: Mutex<Vec<Entry>>,
}

/// Whether plugins created on this thread now can be offered a run loop.
pub(crate) fn available() -> bool {
    event_loop::is_installed()
}

impl RunLoop {
    pub(crate) fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self::default())
    }

    /// Live registrations.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.lock().len()
    }

    /// # Safety
    /// `handler` is null or a live `IEventHandler`.
    pub(crate) unsafe fn register_fd(&self, handler: *mut IEventHandler, fd: FileDescriptor) -> tresult {
        let Some(handler_ref) = ComRef::from_raw(handler) else {
            return kInvalidArgument;
        };
        if fd < 0 {
            return kInvalidArgument;
        }
        let Some(lp) = event_loop::current() else {
            log::warn!("a VST3 plugin asked for its file descriptor to be watched off the UI thread; refused");
            return kResultFalse;
        };
        let owned = handler_ref.to_com_ptr();
        let callback = Box::new(move || {
            // SAFETY: the handler is kept alive by `owned` until this source is removed.
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { owned.onFDIsSet(fd) })).is_err() {
                log::error!("a VST3 plugin panicked handling its file descriptor {fd}");
            }
        });
        log::debug!("VST3 plugin watches file descriptor {fd}");
        match lp.watch_fd(fd, callback) {
            Some(key) => {
                self.entries.lock().push(Entry { kind: Kind::Fd, handler: handler as usize, key });
                kResultOk
            }
            None => kResultFalse,
        }
    }

    /// # Safety
    /// `handler` is null or a live `ITimerHandler`.
    pub(crate) unsafe fn register_timer(&self, handler: *mut ITimerHandler, milliseconds: TimerInterval) -> tresult {
        let Some(handler_ref) = ComRef::from_raw(handler) else {
            return kInvalidArgument;
        };
        let Some(lp) = event_loop::current() else {
            log::warn!("a VST3 plugin asked for a timer off the UI thread; refused");
            return kResultFalse;
        };
        let owned = handler_ref.to_com_ptr();
        let callback = Box::new(move || {
            // SAFETY: as in `register_fd`.
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { owned.onTimer() })).is_err() {
                log::error!("a VST3 plugin panicked in its timer");
            }
        });
        log::debug!("VST3 plugin timer every {milliseconds} ms");
        let key = lp.add_timer(Duration::from_millis(milliseconds.clamp(1, 60_000)), callback);
        self.entries.lock().push(Entry { kind: Kind::Timer, handler: handler as usize, key });
        kResultOk
    }

    /// Remove every registration of `handler` of this kind.
    pub(crate) fn unregister(&self, kind_is_fd: bool, handler: usize) -> tresult {
        let kind = if kind_is_fd { Kind::Fd } else { Kind::Timer };
        let gone: Vec<SourceKey> = {
            let mut entries = self.entries.lock();
            let mut keys = Vec::new();
            entries.retain(|e| {
                let hit = e.kind == kind && e.handler == handler;
                if hit {
                    keys.push(e.key);
                }
                !hit
            });
            keys
        };
        if gone.is_empty() {
            return kInvalidArgument;
        }
        self.remove(gone);
        kResultOk
    }

    /// Remove everything (the instance is being torn down). UI thread.
    pub(crate) fn clear(&self) {
        let gone: Vec<SourceKey> = self.entries.lock().drain(..).map(|e| e.key).collect();
        if !gone.is_empty() {
            log::debug!("a VST3 plugin left {} run-loop registration(s); removed", gone.len());
            self.remove(gone);
        }
    }

    fn remove(&self, keys: Vec<SourceKey>) {
        match event_loop::current() {
            Some(lp) => keys.into_iter().for_each(|k| lp.remove(k)),
            None => log::error!("{} plugin run-loop registration(s) removed off the UI thread: left in place", keys.len()),
        }
    }
}

impl Drop for RunLoop {
    fn drop(&mut self) {
        self.clear();
    }
}

/// Forward `IRunLoop` to a [`RunLoop`] (for the frame and the host context).
macro_rules! forward_run_loop {
    ($ty:ty, $field:ident) => {
        impl vst3::Steinberg::Linux::IRunLoopTrait for $ty {
            unsafe fn registerEventHandler(
                &self,
                handler: *mut vst3::Steinberg::Linux::IEventHandler,
                fd: vst3::Steinberg::Linux::FileDescriptor,
            ) -> vst3::Steinberg::tresult {
                self.$field.register_fd(handler, fd)
            }
            unsafe fn unregisterEventHandler(&self, handler: *mut vst3::Steinberg::Linux::IEventHandler) -> vst3::Steinberg::tresult {
                self.$field.unregister(true, handler as usize)
            }
            unsafe fn registerTimer(
                &self,
                handler: *mut vst3::Steinberg::Linux::ITimerHandler,
                milliseconds: vst3::Steinberg::Linux::TimerInterval,
            ) -> vst3::Steinberg::tresult {
                self.$field.register_timer(handler, milliseconds)
            }
            unsafe fn unregisterTimer(&self, handler: *mut vst3::Steinberg::Linux::ITimerHandler) -> vst3::Steinberg::tresult {
                self.$field.unregister(false, handler as usize)
            }
        }
    };
}
pub(crate) use forward_run_loop;

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use vst3::{Class, ComWrapper};
    use wave_plugin_windows::event_loop::PollLoop;

    struct Ticker {
        fired: Arc<AtomicUsize>,
    }
    impl Class for Ticker {
        type Interfaces = (ITimerHandler, IEventHandler);
    }
    impl ITimerHandlerTrait for Ticker {
        unsafe fn onTimer(&self) {
            self.fired.fetch_add(1, Ordering::Relaxed);
        }
    }
    impl IEventHandlerTrait for Ticker {
        unsafe fn onFDIsSet(&self, _fd: FileDescriptor) {
            self.fired.fetch_add(100, Ordering::Relaxed);
        }
    }

    fn ticker() -> (ComWrapper<Ticker>, Arc<AtomicUsize>) {
        let fired = Arc::new(AtomicUsize::new(0));
        (ComWrapper::new(Ticker { fired: Arc::clone(&fired) }), fired)
    }

    #[test]
    fn without_an_installed_loop_registrations_are_refused() {
        event_loop::uninstall();
        assert!(!available());
        let rl = RunLoop::new();
        let (t, _) = ticker();
        let h = t.as_com_ref::<ITimerHandler>().unwrap().as_ptr();
        // SAFETY: a live handler.
        assert_eq!(unsafe { rl.register_timer(h, 10) }, kResultFalse);
        assert_eq!(unsafe { rl.register_timer(std::ptr::null_mut(), 10) }, kInvalidArgument);
        assert_eq!(rl.len(), 0);
    }

    #[test]
    fn a_timer_runs_on_the_loop_until_unregistered_and_teardown_removes_leftovers() {
        let lp = PollLoop::new();
        event_loop::install(Rc::clone(&lp) as Rc<dyn event_loop::EventLoop>);
        assert!(available());
        let rl = RunLoop::new();
        let (t, fired) = ticker();
        let h = t.as_com_ref::<ITimerHandler>().unwrap().as_ptr();
        // SAFETY: live handlers throughout.
        unsafe {
            assert_eq!(rl.register_timer(h, 2), kResultOk);
        }
        lp.run_for(Duration::from_millis(40));
        let n = fired.load(Ordering::Relaxed);
        assert!(n >= 3, "the plugin's timer fired {n} times");
        assert_eq!(rl.unregister(false, h as usize), kResultOk);
        assert_eq!(rl.unregister(false, h as usize), kInvalidArgument, "already gone");
        lp.run_for(Duration::from_millis(10));
        assert_eq!(fired.load(Ordering::Relaxed), n, "no call after unregistering");
        assert!(lp.is_empty());

        // a plugin that forgets to unregister: the instance's teardown removes it
        unsafe {
            assert_eq!(rl.register_timer(h, 2), kResultOk);
        }
        assert_eq!(lp.len(), 1);
        drop(rl);
        assert!(lp.is_empty(), "nothing can call into an unloaded plugin");
        event_loop::uninstall();
    }

    // libc is a Linux dev-dependency: the run loop is Linux's (VST3 on macOS uses the system loop)
    #[cfg(target_os = "linux")]
    #[test]
    fn a_registered_descriptor_reaches_the_plugin() {
        let lp = PollLoop::new();
        event_loop::install(Rc::clone(&lp) as Rc<dyn event_loop::EventLoop>);
        let rl = RunLoop::new();
        let (t, fired) = ticker();
        let h = t.as_com_ref::<IEventHandler>().unwrap().as_ptr();
        let mut fds = [0; 2];
        // SAFETY: a fresh pipe, closed below; live handler.
        unsafe {
            assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
            assert_eq!(rl.register_fd(h, fds[0]), kResultOk);
            assert_eq!(rl.register_fd(h, -1), kInvalidArgument);
            libc::write(fds[1], b"x".as_ptr().cast(), 1);
        }
        lp.run_once(Duration::from_millis(50));
        assert!(fired.load(Ordering::Relaxed) >= 100);
        assert_eq!(rl.unregister(true, h as usize), kResultOk);
        assert!(lp.is_empty());
        // SAFETY: closing our pipe.
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
        event_loop::uninstall();
    }
}
