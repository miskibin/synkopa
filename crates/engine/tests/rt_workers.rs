//! Multi-core playback: a stream whose synth instances and insert chains run on worker
//! threads sounds exactly like a single-threaded one, and neither the audio thread nor the
//! workers allocate while it plays.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::Arc;

use wave_engine::catalog::new_fx;
use wave_engine::*;

// ---- allocation counter: the test thread (the "callback") and the pool's `wave-audio-*` threads
static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicU64 = AtomicU64::new(0);
thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    /// 0 = not looked up yet, 1 = an audio worker, 2 = another thread.
    static ROLE: Cell<u8> = const { Cell::new(0) };
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

fn note_alloc() {
    if !COUNTING.load(SeqCst) {
        return;
    }
    let _ = ROLE.try_with(|role| {
        if role.get() == 0 && !BUSY.with(|b| b.replace(true)) {
            // looking the name up may allocate itself: BUSY keeps that out of the count
            let audio = std::thread::current().name().is_some_and(|n| n.starts_with("wave-audio"));
            role.set(if audio { 1 } else { 2 });
            BUSY.with(|b| b.set(false));
        }
        if role.get() == 1 || ARMED.with(|a| a.get()) {
            ALLOCS.fetch_add(1, SeqCst);
        }
    });
}

struct Counting;
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        note_alloc();
        System.alloc(l)
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        note_alloc();
        System.alloc_zeroed(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        note_alloc();
        System.realloc(p, l, n)
    }
}
#[global_allocator]
static A: Counting = Counting;

const SR: f32 = 48000.0;
const BLOCK: usize = 256;

/// 8 synth channels on 8 inserts with effect chains, plus the master chain.
fn project() -> Project {
    let mut p = Project::routed();
    let presets: Vec<&str> = poly::catalog().presets.iter().map(|x| x.name).collect();
    let chains: [&[FxKind]; 4] =
        [&[FxKind::Eq, FxKind::Comp], &[FxKind::Sat, FxKind::Delay], &[FxKind::Chorus, FxKind::Reverb], &[FxKind::Eq, FxKind::Limit]];
    while p.inserts.len() < 9 {
        let id = p.alloc_id();
        p.inserts.push(Insert {
            id,
            name: format!("Bus {id}"),
            color: "#4f8ef7".into(),
            vol: VOL_UNITY,
            pan: 0.0,
            width: 1.0,
            muted: false,
            solo: false,
            fx: vec![],
            sends: vec![],
            master: true,
        });
    }
    for k in 1..9 {
        let fx: Vec<Fx> = chains[k % 4].iter().map(|kind| new_fx(p.alloc_id(), *kind, p.bpm)).collect();
        p.inserts[k].fx = fx;
    }
    let mut notes = vec![];
    for k in 0..8 {
        let id = p.alloc_id();
        let name = presets[(k * 5) % presets.len()];
        p.channels.push(Channel {
            id,
            name: name.into(),
            color: "#9b7cf8".into(),
            src: ChannelSrc::Synth { synth: poly::preset_state(name).unwrap() },
            vol: VOL_UNITY,
            pan: (k as f32 - 3.5) / 4.0,
            mute: false,
            solo: false,
            insert: Some(p.inserts[1 + k].id),
            root: 60,
        });
        for s in 0..8 {
            notes.push(Note::new(id, (s * 4) as f32, 3.0, Some(48 + ((k * 3 + s * 5) % 24) as u8)));
        }
    }
    let pid = p.alloc_id();
    p.patterns = vec![Pattern { id: pid, name: "P".into(), color: "#4f8ef7".into(), len: 8.0, min_len: 8.0, notes, swing: 0.0 }];
    let cid = p.alloc_id();
    p.clips = vec![Clip {
        id: cid,
        track: 0,
        start: 0.0,
        len: 8.0,
        offset: 0.0,
        kind: ClipKind::Pat,
        source: pid,
        muted: false,
        gain: 0.0,
        fade_in: 0.0,
        fade_out: 0.0,
        pan: 0.0,
        norm: false,
        rev: false,
        tune: None,
        stretch: 1.0,
        resample: false,
        pitch: 0.0,
        insert: None,
        root: 60,
    }];
    p
}

/// Plays the project from the start for `blocks` blocks on a stream with `workers` threads.
fn play(workers: usize, blocks: usize, count: bool) -> (Vec<f32>, u64, usize) {
    let e = Engine::new(project(), Arc::new(NoPlugins));
    e.set_render_workers(workers);
    let mut rt = e.create_rt(SR, BLOCK);
    let pool_threads = rt.workers();
    e.play(0.0);
    let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
    // the first blocks take in the graph and the instances (and may warm up lazily)
    for _ in 0..8 {
        rt.process(&mut l, &mut r, None);
    }
    let mut out = Vec::with_capacity(blocks * BLOCK * 2);
    let mut allocs = 0;
    for _ in 0..blocks {
        if count {
            ALLOCS.store(0, SeqCst);
            ARMED.with(|a| a.set(true));
            COUNTING.store(true, SeqCst);
        }
        rt.process(&mut l, &mut r, None);
        if count {
            COUNTING.store(false, SeqCst);
            ARMED.with(|a| a.set(false));
            allocs += ALLOCS.load(SeqCst);
        }
        out.extend_from_slice(&l);
        out.extend_from_slice(&r);
    }
    let _ = e.stop();
    (out, allocs, pool_threads)
}

#[test]
fn workers_sound_exactly_like_one_thread() {
    let blocks = (SR as usize * 4) / BLOCK; // 4 s: notes start, sustain, release, effect tails
    let (one, _, t1) = play(1, blocks, false);
    let (many, _, t4) = play(4, blocks, false);
    assert_eq!(t1, 0);
    assert_eq!(t4, 3);
    assert!(one.iter().any(|x| x.abs() > 1e-3), "the project is silent");
    let diff = one.iter().zip(&many).filter(|(a, b)| a.to_bits() != b.to_bits()).count();
    assert_eq!(diff, 0, "{diff} samples differ between 1 and 4 threads");
}

#[test]
fn workers_do_not_allocate_while_playing() {
    let (_, allocs, threads) = play(4, 400, true);
    assert_eq!(threads, 3);
    assert_eq!(allocs, 0, "the audio thread or a worker allocated");
}
