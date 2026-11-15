//! `worker:schedule`: a plugin's non-real-time work (loading an impulse response, a
//! sample, a model) handed from `run()` to a host thread and its answer handed back.
//!
//! ```text
//!   audio thread                         worker thread (one per instance)
//!   run() -> schedule_work(bytes) ──ring──▶ work(bytes, respond)
//!   after run(): work_response(bytes) ◀──ring── respond(bytes)
//!                end_run()
//! ```
//!
//! Both rings are lock-free SPSC byte rings of length-prefixed messages: the audio
//! thread never allocates, locks or waits. The worker thread polls (2 ms) rather than
//! being woken, so `schedule_work` makes no system call.

use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::sys::{LV2_Handle, LV2_Worker_Interface, LV2_Worker_Status, WORKER_ERR_NO_SPACE, WORKER_ERR_UNKNOWN, WORKER_SUCCESS};

/// Bytes each direction can hold in flight.
pub const RING_BYTES: usize = 1 << 16;
/// The largest single message (a plugin sending more gets `WORKER_ERR_NO_SPACE`).
pub const MAX_MESSAGE: usize = RING_BYTES / 2;
const POLL: Duration = Duration::from_millis(2);

/// Jobs every worker thread has run (for tests and diagnostics).
static JOBS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How many `work()` calls the worker threads have made in this process.
pub fn jobs_run() -> usize {
    JOBS.load(Ordering::Relaxed)
}

/// Queue one message. `false` when the ring has no room for it (nothing is written).
pub fn push_msg(p: &mut rtrb::Producer<u8>, data: &[u8]) -> bool {
    let Ok(len) = u32::try_from(data.len()) else { return false };
    let total = 4 + data.len();
    let Ok(mut chunk) = p.write_chunk(total) else { return false };
    let (a, b) = chunk.as_mut_slices();
    let mut bytes = len.to_le_bytes().into_iter().chain(data.iter().copied());
    for slot in a.iter_mut().chain(b.iter_mut()) {
        *slot = bytes.next().unwrap_or(0);
    }
    chunk.commit_all();
    true
}

/// What [`pop_msg`] found.
#[derive(Debug, PartialEq, Eq)]
pub enum Popped {
    Empty,
    /// A message of this many bytes is now at the start of the buffer.
    Message(usize),
    /// A message larger than the buffer was skipped.
    Dropped,
}

/// Take the next message into `buf`. A message is committed by the producer in one
/// piece, so a visible header means its body is there too.
pub fn pop_msg(c: &mut rtrb::Consumer<u8>, buf: &mut [u8]) -> Popped {
    if c.slots() < 4 {
        return Popped::Empty;
    }
    let mut header = [0u8; 4];
    match c.read_chunk(4) {
        Ok(chunk) => {
            let (a, b) = chunk.as_slices();
            for (dst, src) in header.iter_mut().zip(a.iter().chain(b.iter())) {
                *dst = *src;
            }
            chunk.commit_all();
        }
        Err(_) => return Popped::Empty,
    }
    let len = u32::from_le_bytes(header) as usize;
    let Ok(chunk) = c.read_chunk(len) else {
        // cannot happen with `push_msg` as the only producer; drop what is there
        let n = c.slots();
        if let Ok(rest) = c.read_chunk(n) {
            rest.commit_all();
        }
        return Popped::Dropped;
    };
    if len > buf.len() {
        chunk.commit_all();
        return Popped::Dropped;
    }
    let (a, b) = chunk.as_slices();
    buf[..a.len()].copy_from_slice(a);
    buf[a.len()..a.len() + b.len()].copy_from_slice(b);
    chunk.commit_all();
    Popped::Message(len)
}

/// The `LV2_Worker_Schedule` handle: the request ring's producer. Only the thread that
/// runs the instance touches it (the audio thread, or the creating thread before the
/// instance is handed over).
pub struct Schedule {
    requests: UnsafeCell<rtrb::Producer<u8>>,
}

// SAFETY: see the type's documentation; the instance is used by one thread at a time.
unsafe impl Send for Schedule {}
unsafe impl Sync for Schedule {}

/// `LV2_Worker_Schedule::schedule_work`.
pub unsafe extern "C" fn schedule_work(handle: *mut c_void, size: u32, data: *const c_void) -> LV2_Worker_Status {
    if handle.is_null() || (data.is_null() && size > 0) || size as usize > MAX_MESSAGE {
        return WORKER_ERR_NO_SPACE;
    }
    // SAFETY: `handle` is the `Schedule` this host put in the feature, alive as long as
    // the instance; only the thread running the instance calls this.
    let schedule = unsafe { &*handle.cast::<Schedule>() };
    let bytes = if size == 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size as usize) } };
    // SAFETY: as above, no other thread holds the producer.
    if push_msg(unsafe { &mut *schedule.requests.get() }, bytes) {
        WORKER_SUCCESS
    } else {
        WORKER_ERR_NO_SPACE
    }
}

unsafe extern "C" fn respond(handle: *mut c_void, size: u32, data: *const c_void) -> LV2_Worker_Status {
    if handle.is_null() || (data.is_null() && size > 0) || size as usize > MAX_MESSAGE {
        return WORKER_ERR_NO_SPACE;
    }
    // SAFETY: `handle` is the worker thread's own response producer, passed into `work`
    // for the duration of the call.
    let responses = unsafe { &mut *handle.cast::<rtrb::Producer<u8>>() };
    let bytes = if size == 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size as usize) } };
    if push_msg(responses, bytes) {
        WORKER_SUCCESS
    } else {
        WORKER_ERR_NO_SPACE
    }
}

/// A raw pointer that may cross to the worker thread.
#[derive(Clone, Copy)]
struct SendPtr<T>(*const T);
// SAFETY: the plugin instance and its worker interface are valid until the worker
// thread is joined (in `Worker::drop`, before the instance is freed); LV2 allows `work`
// to run concurrently with `run`.
unsafe impl<T> Send for SendPtr<T> {}

/// One instance's worker: its thread, and the audio-thread ends of the rings.
///
/// Made in two steps because each side needs the other: the `worker:schedule` feature
/// (with [`Worker::schedule_handle`]) is passed to `instantiate`, and the thread can only
/// start ([`Worker::start`]) once the instance handle exists.
pub struct Worker {
    schedule: Box<Schedule>,
    responses: rtrb::Consumer<u8>,
    scratch: Vec<u8>,
    iface: *const LV2_Worker_Interface,
    /// The worker thread's ring ends, until it starts.
    pending: Option<(rtrb::Consumer<u8>, rtrb::Producer<u8>)>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

// SAFETY: `iface` is plugin data valid for the instance's life; the rest is Send.
unsafe impl Send for Worker {}

impl Worker {
    /// The rings and the schedule handle; no thread yet.
    pub fn new() -> Self {
        let (req_tx, req_rx) = rtrb::RingBuffer::<u8>::new(RING_BYTES);
        let (res_tx, res_rx) = rtrb::RingBuffer::<u8>::new(RING_BYTES);
        Self {
            schedule: Box::new(Schedule { requests: UnsafeCell::new(req_tx) }),
            responses: res_rx,
            scratch: vec![0u8; MAX_MESSAGE],
            iface: std::ptr::null(),
            pending: Some((req_rx, res_tx)),
            stop: Arc::new(AtomicBool::new(false)),
            thread: None,
        }
    }

    /// The handle for the `worker:schedule` feature (stable for this worker's life).
    pub fn schedule_handle(&self) -> *mut c_void {
        std::ptr::addr_of!(*self.schedule).cast_mut().cast()
    }

    /// Start the worker thread for the plugin instance `handle` with its `iface`.
    ///
    /// # Safety
    /// `handle` and `iface` must stay valid until this `Worker` is dropped.
    pub unsafe fn start(&mut self, handle: LV2_Handle, iface: *const LV2_Worker_Interface, name: &str) -> std::io::Result<()> {
        let Some((mut req_rx, mut res_tx)) = self.pending.take() else { return Ok(()) };
        let stop = Arc::clone(&self.stop);
        let (h, i) = (SendPtr(handle.cast_const()), SendPtr(iface));
        let thread = std::thread::Builder::new().name(format!("lv2-worker {name}")).spawn(move || {
            let (h, i) = (h, i);
            let mut buf = vec![0u8; MAX_MESSAGE];
            // SAFETY: valid until joined (see `start`'s contract).
            let Some(work) = (unsafe { (*i.0).work }) else { return };
            while !stop.load(Ordering::Acquire) {
                match pop_msg(&mut req_rx, &mut buf) {
                    Popped::Message(n) => {
                        let res = std::ptr::addr_of_mut!(res_tx).cast::<c_void>();
                        // SAFETY: the instance is alive; `buf[..n]` and the producer
                        // outlive the call.
                        let status = unsafe { work(h.0.cast_mut(), respond, res, n as u32, buf.as_ptr().cast()) };
                        JOBS.fetch_add(1, Ordering::Relaxed);
                        if status != WORKER_SUCCESS {
                            log::debug!("LV2 worker: work() returned {}", status_name(status));
                        }
                    }
                    Popped::Dropped => log::warn!("LV2 worker: dropped an oversized request"),
                    Popped::Empty => std::thread::park_timeout(POLL),
                }
            }
        })?;
        self.iface = iface;
        self.thread = Some(thread);
        Ok(())
    }

    /// After `run()`: deliver the worker's answers, then `end_run`. Audio thread; no
    /// allocation, no lock. Nothing to do before [`Worker::start`].
    ///
    /// # Safety
    /// `handle` is the instance this worker belongs to, not running concurrently.
    pub unsafe fn after_run(&mut self, handle: LV2_Handle) {
        if self.iface.is_null() {
            return;
        }
        // SAFETY: `iface` is valid for the instance's life.
        let iface = unsafe { &*self.iface };
        loop {
            match pop_msg(&mut self.responses, &mut self.scratch) {
                Popped::Message(n) => {
                    if let Some(f) = iface.work_response {
                        // SAFETY: the instance is alive and not running; the bytes outlive the call.
                        unsafe { f(handle, n as u32, self.scratch.as_ptr().cast()) };
                    }
                }
                Popped::Dropped => {}
                Popped::Empty => break,
            }
        }
        if let Some(f) = iface.end_run {
            // SAFETY: as above.
            unsafe { f(handle) };
        }
    }
}

impl Default for Worker {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            let _ = t.join();
        }
    }
}

/// A status a plugin's `work` returned that is not success (for messages).
pub fn status_name(s: LV2_Worker_Status) -> &'static str {
    match s {
        WORKER_SUCCESS => "success",
        WORKER_ERR_NO_SPACE => "no space",
        WORKER_ERR_UNKNOWN => "unknown error",
        _ => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn messages_keep_their_boundaries_across_the_ring_wrap() {
        let (mut p, mut c) = rtrb::RingBuffer::<u8>::new(32);
        let mut buf = [0u8; 16];
        for round in 0..20u8 {
            assert!(push_msg(&mut p, &[round, round + 1, round + 2]));
            assert!(push_msg(&mut p, &[]));
            assert_eq!(pop_msg(&mut c, &mut buf), Popped::Message(3));
            assert_eq!(&buf[..3], &[round, round + 1, round + 2]);
            assert_eq!(pop_msg(&mut c, &mut buf), Popped::Message(0));
            assert_eq!(pop_msg(&mut c, &mut buf), Popped::Empty);
        }
        assert!(!push_msg(&mut p, &[0; 40]), "larger than the ring");
        assert!(push_msg(&mut p, &[7; 20]));
        assert_eq!(pop_msg(&mut c, &mut buf), Popped::Dropped, "larger than the reader's buffer");
        assert_eq!(pop_msg(&mut c, &mut buf), Popped::Empty);
    }

    // A fake plugin: work() answers with the request reversed; work_response() records it.
    static ANSWERS: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
    static END_RUNS: Mutex<usize> = Mutex::new(0);

    unsafe extern "C" fn work(
        _h: LV2_Handle,
        respond: crate::sys::LV2_Worker_Respond_Function,
        rh: *mut c_void,
        size: u32,
        data: *const c_void,
    ) -> LV2_Worker_Status {
        let mut v = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size as usize) }.to_vec();
        v.reverse();
        unsafe { respond(rh, v.len() as u32, v.as_ptr().cast()) }
    }
    unsafe extern "C" fn work_response(_h: LV2_Handle, size: u32, data: *const c_void) -> LV2_Worker_Status {
        ANSWERS.lock().unwrap().push(unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size as usize) }.to_vec());
        WORKER_SUCCESS
    }
    unsafe extern "C" fn end_run(_h: LV2_Handle) -> LV2_Worker_Status {
        *END_RUNS.lock().unwrap() += 1;
        WORKER_SUCCESS
    }
    static IFACE: LV2_Worker_Interface =
        LV2_Worker_Interface { work: Some(work), work_response: Some(work_response), end_run: Some(end_run) };

    #[test]
    fn a_scheduled_job_runs_on_the_worker_and_its_answer_returns_after_run() {
        // SAFETY: the fake interface is static; the fake plugin has no instance data.
        let mut w = Worker::new();
        let h = w.schedule_handle();
        // before the thread starts, after_run has nothing to deliver
        unsafe { w.after_run(std::ptr::null_mut()) };
        unsafe { w.start(std::ptr::null_mut(), &IFACE, "test").unwrap() };
        // SAFETY: called from this thread only, like `run()` would.
        unsafe {
            assert_eq!(schedule_work(h, 3, [1u8, 2, 3].as_ptr().cast()), WORKER_SUCCESS);
            assert_eq!(schedule_work(h, (MAX_MESSAGE + 1) as u32, [0u8].as_ptr().cast()), WORKER_ERR_NO_SPACE);
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while ANSWERS.lock().unwrap().is_empty() && std::time::Instant::now() < deadline {
            // SAFETY: the fake instance is never "running" concurrently.
            unsafe { w.after_run(std::ptr::null_mut()) };
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(ANSWERS.lock().unwrap().as_slice(), &[vec![3u8, 2, 1]]);
        assert!(*END_RUNS.lock().unwrap() >= 1);
        drop(w); // joins the thread
        assert_eq!(status_name(WORKER_ERR_NO_SPACE), "no space");
    }
}
