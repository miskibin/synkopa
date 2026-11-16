//! Worker threads for the audio callback: one block's independent jobs (synth instances,
//! insert chains) spread over several cores, the way FL Studio's "multithreaded generator /
//! mixer processing" and Ableton's multicore support do.
//!
//! Real-time rules: a dispatch allocates nothing, takes no lock and makes no blocking call.
//! The calling thread (the audio callback) takes jobs from the same atomic counter as the
//! workers, so if no worker wakes up in time it simply does all the work itself: never slower
//! than single-threaded except for a few atomics. It waits only for jobs a worker has already
//! started. Idle workers spin for a short while (the next stage of the same block usually
//! follows within microseconds), then park; a dispatch unparks only the parked ones.
//!
//! The threads are spawned by [`RtPool::new`] (off the audio thread). Dropping the pool tells
//! them to exit and does not join them, so it is safe wherever the owner is dropped.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::SeqCst};
use std::sync::Arc;
use std::thread::Thread;
use std::time::{Duration, Instant};

/// How long an idle worker spins before it parks.
const SPIN: Duration = Duration::from_micros(150);

type Job = *const (dyn Fn(usize) + Sync);

struct Inner {
    /// Bumped by every dispatch (and at shutdown): what wakes a worker.
    epoch: AtomicU64,
    /// The job may be read (set after `job`/`total` are written, cleared before they change).
    open: AtomicBool,
    /// Workers between "I may look at the job" and "I am done with it".
    active: AtomicUsize,
    next: AtomicUsize,
    done: AtomicUsize,
    total: AtomicUsize,
    job: UnsafeCell<Option<Job>>,
    quit: AtomicBool,
    /// Per worker: it is parked (or about to park) and needs an unpark.
    sleeping: Vec<AtomicBool>,
    threads: Vec<std::sync::OnceLock<Thread>>,
}

// `job` is written only by the dispatching thread while `open` is false and no worker is
// `active`; workers read it only while `active` with `open` true (see `run`).
unsafe impl Sync for Inner {}
unsafe impl Send for Inner {}

pub struct RtPool {
    inner: Arc<Inner>,
}

impl RtPool {
    /// A pool with `workers` threads besides the caller (0 = everything on the caller).
    /// Spawns and allocates: call off the audio thread.
    pub fn new(workers: usize) -> Self {
        let inner = Arc::new(Inner {
            epoch: AtomicU64::new(0),
            open: AtomicBool::new(false),
            active: AtomicUsize::new(0),
            next: AtomicUsize::new(0),
            done: AtomicUsize::new(0),
            total: AtomicUsize::new(0),
            job: UnsafeCell::new(None),
            quit: AtomicBool::new(false),
            sleeping: (0..workers).map(|_| AtomicBool::new(false)).collect(),
            threads: (0..workers).map(|_| std::sync::OnceLock::new()).collect(),
        });
        for w in 0..workers {
            let inner2 = inner.clone();
            let h = std::thread::Builder::new()
                .name(format!("wave-audio-{w}"))
                .stack_size(1 << 20)
                .spawn(move || worker(&inner2, w))
                .expect("spawn audio worker");
            let _ = inner.threads[w].set(h.thread().clone());
        }
        RtPool { inner }
    }

    /// Threads besides the caller.
    pub fn workers(&self) -> usize {
        self.inner.sleeping.len()
    }

    /// Runs `f(i)` for every `i < total`, spread over the workers and this thread; returns
    /// when every call has returned. Real-time safe (see the module docs). Not reentrant.
    pub fn run(&self, total: usize, f: &(dyn Fn(usize) + Sync)) {
        let s = &*self.inner;
        if total == 0 {
            return;
        }
        if total == 1 || s.sleeping.is_empty() {
            for i in 0..total {
                f(i);
            }
            return;
        }
        // the lifetime is erased: the job is unreachable again before this returns
        let job: Job = unsafe { std::mem::transmute::<&(dyn Fn(usize) + Sync), &'static (dyn Fn(usize) + Sync)>(f) };
        // SAFETY: `open` is false and `active` is 0 here (the previous `run` ended that way)
        unsafe { *s.job.get() = Some(job) };
        s.total.store(total, SeqCst);
        s.next.store(0, SeqCst);
        s.done.store(0, SeqCst);
        s.open.store(true, SeqCst);
        s.epoch.fetch_add(1, SeqCst);
        // wake only as many parked workers as there are jobs beyond this thread's first
        let mut want = total - 1;
        for (w, sl) in s.sleeping.iter().enumerate() {
            if want == 0 {
                break;
            }
            if sl.load(SeqCst) {
                if let Some(t) = s.threads[w].get() {
                    t.unpark();
                }
            }
            want -= 1;
        }
        loop {
            let i = s.next.fetch_add(1, SeqCst);
            if i >= total {
                break;
            }
            f(i);
            s.done.fetch_add(1, SeqCst);
        }
        // jobs a worker already took are running: wait for them (bounded by one job)
        while s.done.load(SeqCst) < total {
            std::hint::spin_loop();
        }
        s.open.store(false, SeqCst);
        while s.active.load(SeqCst) != 0 {
            std::hint::spin_loop();
        }
        unsafe { *s.job.get() = None };
    }

    /// `f(i, &mut items[i])` for every item, in parallel (each item to exactly one call).
    pub fn for_each_mut<T: Send>(&self, items: &mut [T], f: &(dyn Fn(usize, &mut T) + Sync)) {
        struct P<T>(*mut T);
        unsafe impl<T: Send> Sync for P<T> {}
        let base = P(items.as_mut_ptr());
        let len = items.len();
        let base = &base;
        // SAFETY: `run` hands every index below `len` to exactly one call, so the `&mut`s
        // never alias, and `items` stays borrowed until `run` returns
        self.run(len, &move |i| f(i, unsafe { &mut *base.0.add(i) }));
    }
}

impl Drop for RtPool {
    fn drop(&mut self) {
        let s = &*self.inner;
        s.quit.store(true, SeqCst);
        s.epoch.fetch_add(1, SeqCst);
        for t in s.threads.iter().filter_map(|t| t.get()) {
            t.unpark();
        }
    }
}

fn worker(s: &Inner, w: usize) {
    boost_priority();
    // the epoch the pool was created with: a dispatch (or the drop) that happened before this
    // thread got going is seen at once instead of being waited for
    let mut seen = 0;
    loop {
        // wait for the next dispatch: spin, then park
        let t0 = Instant::now();
        let mut spins = 0u32;
        while s.epoch.load(SeqCst) == seen {
            std::hint::spin_loop();
            spins += 1;
            if spins.is_multiple_of(64) && t0.elapsed() > SPIN {
                s.sleeping[w].store(true, SeqCst);
                if s.epoch.load(SeqCst) == seen {
                    std::thread::park();
                }
                s.sleeping[w].store(false, SeqCst);
            }
        }
        seen = s.epoch.load(SeqCst);
        if s.quit.load(SeqCst) {
            return;
        }
        s.active.fetch_add(1, SeqCst);
        if s.open.load(SeqCst) {
            // SAFETY: `open` with this thread counted in `active`: the dispatcher keeps the
            // job alive and unchanged until `active` drops back to 0
            if let Some(job) = unsafe { *s.job.get() } {
                let total = s.total.load(SeqCst);
                let _no_denormals = crate::processor::NoDenormals::new();
                loop {
                    let i = s.next.fetch_add(1, SeqCst);
                    if i >= total {
                        break;
                    }
                    unsafe { (*job)(i) };
                    s.done.fetch_add(1, SeqCst);
                }
            }
        }
        s.active.fetch_sub(1, SeqCst);
    }
}

/// Audio workers run at the audio thread's priority: a worker preempted mid-job makes the
/// callback wait for it.
fn boost_priority() {
    #[cfg(windows)]
    unsafe {
        // THREAD_PRIORITY_TIME_CRITICAL, as cpal sets for the WASAPI/ASIO callback thread
        extern "system" {
            fn GetCurrentThread() -> isize;
            fn SetThreadPriority(h: isize, p: i32) -> i32;
        }
        SetThreadPriority(GetCurrentThread(), 15);
    }
    // ALSA/JACK (rtkit) and CoreAudio callbacks run real-time: the workers ask for the same
    // class, or a worker preempted mid-job makes the callback wait past its deadline. Without
    // permission (no rtkit, no rtprio limit) they stay at normal priority.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    // the handle only matters for an explicit demotion: the worker keeps the class until it exits
    let _ = audio_thread_priority::promote_current_thread_to_real_time(512, 48_000);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    #[test]
    fn every_index_runs_exactly_once() {
        let pool = RtPool::new(3);
        for total in [0usize, 1, 2, 3, 7, 64, 500] {
            let hits: Vec<AtomicU32> = (0..total).map(|_| AtomicU32::new(0)).collect();
            pool.run(total, &|i| {
                hits[i].fetch_add(1, SeqCst);
            });
            assert!(hits.iter().all(|h| h.load(SeqCst) == 1), "total {total}");
        }
    }

    #[test]
    fn many_dispatches_in_a_row_stay_consistent() {
        // workers alternate between spinning and parking: no lost wake-up, no stale job
        let pool = RtPool::new(4);
        let mut v = vec![0u64; 37];
        for round in 0..20_000u64 {
            pool.for_each_mut(&mut v, &|i, x| *x += i as u64 + round);
            if round % 5000 == 0 {
                std::thread::sleep(Duration::from_millis(2)); // let them park
            }
        }
        for (i, x) in v.iter().enumerate() {
            assert_eq!(*x, 20_000 * i as u64 + (0..20_000u64).sum::<u64>());
        }
    }

    #[test]
    fn work_is_shared_between_threads() {
        let pool = RtPool::new(3);
        let ids = std::sync::Mutex::new(std::collections::HashSet::new());
        for _ in 0..50 {
            pool.run(16, &|_| {
                ids.lock().unwrap().insert(std::thread::current().id());
                let t = Instant::now();
                while t.elapsed() < Duration::from_micros(200) {}
            });
        }
        assert!(ids.lock().unwrap().len() >= 2, "only the caller worked");
    }

    #[test]
    fn no_workers_runs_on_the_caller() {
        let pool = RtPool::new(0);
        let me = std::thread::current().id();
        pool.run(5, &|_| assert_eq!(std::thread::current().id(), me));
    }

    #[test]
    fn dropping_the_pool_ends_its_threads() {
        let pool = RtPool::new(2);
        let weak = Arc::downgrade(&pool.inner);
        drop(pool);
        let t = Instant::now();
        while weak.strong_count() > 0 && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(weak.strong_count(), 0);
    }
}
