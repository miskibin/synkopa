//! Recording against a beat: takes line up with what the performer heard (device round
//! trip + plugin delay compensation), loop passes keep their boundaries, punch-in on the fly.

use std::path::Path;
use std::sync::Arc;

use wave_engine::catalog::new_fx;
use wave_engine::sources::Audio;
use wave_engine::*;

const SR: u32 = 48000;
const BLOCK: usize = 256;

/// No effects anywhere, track 0 armed, no count-in, recording on.
fn armed() -> Project {
    let mut p = Project::routed();
    for m in p.inserts.iter_mut() {
        m.fx.clear();
    }
    for t in p.tracks.iter_mut() {
        t.arm = false;
    }
    p.tracks[0].arm = true;
    p
}

/// An engine whose project is saved in its own folder, so takes land there.
fn engine(p: Project, tag: &str) -> Arc<Engine> {
    let dir = std::env::temp_dir().join(format!("wave-rec-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let e = Engine::new(p, Arc::new(NoPlugins));
    e.save(&dir.join("song.wave")).unwrap();
    e
}

fn frames(beats: f64, bpm: f64) -> usize {
    (beats * 60.0 / bpm * SR as f64).round() as usize
}

/// Feeds `total` frames of input, `input(f)` for absolute input frame `f` (from `*at`).
fn feed(rt: &mut Rt, at: &mut usize, total: usize, input: &dyn Fn(usize) -> f32) {
    let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
    let end = *at + total;
    while *at < end {
        let n = BLOCK.min(end - *at);
        let inp: Vec<f32> = (0..n).map(|i| input(*at + i)).collect();
        rt.process(&mut l[..n], &mut r[..n], Some((&inp, &inp)));
        *at += n;
    }
}

fn take_audio(p: &Project, source: Id) -> Vec<f32> {
    let s = p.sources.iter().find(|s| s.id == source).expect("take source");
    let a = Audio::load(Path::new(&s.path)).expect("take file");
    a.content().chunks(2).map(|f| f[0]).collect()
}

#[test]
fn a_take_lines_up_with_the_beat_the_singer_heard() {
    // a pitch corrector on another track delays all playback (PDC); the device adds its own
    // round trip. A sound made exactly on beat 5 must land on beat 5 of the take.
    let mut p = armed();
    let other = p.tracks[1].insert;
    p.inserts.iter_mut().find(|m| m.id == other).unwrap().fx.push(new_fx(7000, FxKind::Tune, p.bpm));
    let bpm = p.bpm;
    let pdc = wave_engine::vocal::tune_latency(SR as f32);
    assert_eq!(wave_engine::engine::playback_latency(&p, SR as f32), pdc);
    let e = engine(p, "align");
    let mut rt = e.create_rt(SR as f32, BLOCK);
    let io = 1100usize;
    e.shared().set_io_latency(io as u32);
    e.set_transport(None, Some(false), Some(true), None);
    e.play(4.0);
    // the singer hears beat 5 `pdc + output` frames after the engine plays it, and the
    // mic sound reaches the engine `input` frames later: `io + pdc` after beat 5 in all
    let hit = frames(1.0, bpm) + io + pdc;
    let mut at = 0;
    feed(&mut rt, &mut at, frames(3.0, bpm), &|f| if f == hit { 0.9 } else { 0.0 });
    let _ = e.stop();
    let p = e.project();
    let clip = p.clips.iter().find(|c| c.kind == ClipKind::Audio).expect("a take");
    assert!((clip.start - 4.0).abs() < 1e-9, "take starts at {}", clip.start);
    let audio = take_audio(&p, clip.source);
    let onset = audio.iter().position(|x| x.abs() > 0.5).expect("the hit was recorded");
    assert_eq!(onset, frames(1.0, bpm), "hit lands {} frames off beat 5", onset as i64 - frames(1.0, bpm) as i64);
}

#[test]
fn loop_passes_keep_their_boundaries_with_latency() {
    let mut p = armed();
    p.loop_range = Some(LoopRange { a: 8.0, b: 12.0 });
    let bpm = p.bpm;
    let e = engine(p, "loop");
    let mut rt = e.create_rt(SR as f32, BLOCK);
    let io = 777usize;
    e.shared().set_io_latency(io as u32);
    e.set_transport(None, Some(false), Some(true), None);
    e.play(8.0);
    let pass = frames(4.0, bpm);
    // what the singer does during heard pass k reaches the engine `io` frames late
    let level = move |f: usize| if f < io { 0.0 } else { 0.1 * (((f - io) / pass) + 1) as f32 };
    let mut at = 0;
    feed(&mut rt, &mut at, pass * 3 + 2000, &level);
    let _ = e.stop();
    let p = e.project();
    let comp = &p.comps[0];
    assert_eq!(comp.takes.len(), 3, "the short tail is not a take");
    for (k, t) in comp.takes.iter().enumerate() {
        assert!((t.len - 4.0).abs() < 0.01, "take {k} is {} beats", t.len);
        let a = take_audio(&p, t.source);
        let want = 0.1 * (k + 1) as f32;
        // (the audio thread wraps on the frame at or just after the loop end, so passes
        // drift by ≤ 1 frame each against this ideal input: allow a few at the edges; a
        // misplaced boundary would be off by the whole latency, 777 frames)
        let bad = (4..a.len() - 4).find(|&i| (a[i] - want).abs() > 1e-4);
        assert_eq!(bad.map(|i| (i, a[i], a.len())), None, "take {k} holds audio of another pass (want {want})");
    }
}

#[test]
fn arming_while_playing_punches_in_at_the_playhead() {
    let p = armed();
    let e = Engine::new(p, Arc::new(NoPlugins));
    let mut rt = e.create_rt(SR as f32, BLOCK);
    e.play(2.0);
    let mut at = 0;
    feed(&mut rt, &mut at, BLOCK * 300, &|_| 0.3);
    let punch = e.shared().beat();
    e.set_transport(None, None, Some(true), None);
    feed(&mut rt, &mut at, SR as usize, &|_| 0.3);
    // punch out keeps the transport running and lands the take
    e.set_transport(None, None, Some(false), None);
    assert!(e.state().transport.playing);
    let p = e.project();
    let clips: Vec<_> = p.clips.iter().filter(|c| c.kind == ClipKind::Audio).collect();
    assert_eq!(clips.len(), 1, "punch-in recorded nothing");
    assert!((clips[0].start - punch).abs() < 1e-9, "take starts at {} instead of {punch}", clips[0].start);
    let secs = clips[0].len * 60.0 / p.bpm;
    assert!((secs - 1.0).abs() < 0.02, "take is {secs} s");
    let _ = e.stop();
}

/// `armed()` with nothing armed and a 4-beat pattern clip on each track in `busy`, from `at`.
fn unarmed(busy: &[u32], at: f64) -> Project {
    let mut p = armed();
    p.tracks[0].arm = false;
    p.patterns = vec![Pattern { id: 9000, name: "P".into(), color: "#4f8ef7".into(), len: 4.0, min_len: 4.0, notes: vec![], swing: 0.0 }];
    for (k, &t) in busy.iter().enumerate() {
        p.clips.push(Clip {
            id: 9001 + k as Id,
            track: t,
            start: at,
            len: 4.0,
            offset: 0.0,
            kind: ClipKind::Pat,
            source: 9000,
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
        });
    }
    p
}

/// Records one second of input from beat 0 and stops.
fn record_second(e: &Engine) {
    let mut rt = e.create_rt(SR as f32, BLOCK);
    e.set_transport(None, Some(false), Some(true), None);
    e.play(0.0);
    let mut at = 0;
    feed(&mut rt, &mut at, SR as usize, &|_| 0.3);
    let _ = e.stop();
}

fn take_track(p: &Project) -> Option<u32> {
    p.clips.iter().find(|c| c.kind == ClipKind::Audio).map(|c| c.track)
}

#[test]
fn with_nothing_armed_a_take_goes_to_the_first_free_track() {
    // track 0 holds a clip where the take is: the take goes to track 1
    let e = engine(unarmed(&[0], 0.0), "free");
    record_second(&e);
    assert_eq!(take_track(&e.project()), Some(1));
}

#[test]
fn a_take_stays_on_the_track_it_was_recorded_on() {
    // track 0 is busy from beat 32 on: no room "for good" from beat 0, so the take records on
    // track 1, and lands there (where it was drawn while recording), though it would have fit
    let e = engine(unarmed(&[0], 32.0), "later");
    record_second(&e);
    assert_eq!(take_track(&e.project()), Some(1));
}

#[test]
fn with_every_track_busy_a_take_gets_a_new_track_and_undo_removes_both() {
    let n = armed().tracks.len() as u32;
    let all: Vec<u32> = (0..n).collect();
    let e = engine(unarmed(&all, 0.0), "full");
    record_second(&e);
    let p = e.project();
    assert_eq!(p.tracks.len() as u32, n + 1, "no track was added");
    assert_eq!(take_track(&p), Some(n));
    assert!(e.undo());
    let p = e.project();
    assert_eq!(p.tracks.len() as u32, n, "undoing the take leaves its empty track behind");
    assert_eq!(take_track(&p), None);
}

#[test]
fn a_track_added_for_a_take_that_recorded_nothing_goes_away() {
    let n = armed().tracks.len() as u32;
    let all: Vec<u32> = (0..n).collect();
    let e = engine(unarmed(&all, 0.0), "none");
    let _rt = e.create_rt(SR as f32, BLOCK);
    e.set_transport(None, Some(false), Some(true), None);
    e.play(0.0);
    assert_eq!(e.project().tracks.len() as u32, n + 1, "the track to record on exists while recording");
    let _ = e.stop();
    assert_eq!(e.project().tracks.len() as u32, n);
}

#[test]
fn arming_while_playing_with_nothing_armed_punches_in_on_a_free_track() {
    let e = engine(unarmed(&[0], 0.0), "punch");
    let mut rt = e.create_rt(SR as f32, BLOCK);
    e.play(0.0);
    let mut at = 0;
    feed(&mut rt, &mut at, BLOCK * 20, &|_| 0.3);
    e.set_transport(None, None, Some(true), None);
    feed(&mut rt, &mut at, SR as usize, &|_| 0.3);
    let _ = e.stop();
    assert_eq!(take_track(&e.project()), Some(1));
}

/// Like `feed`, but the device layer reports `io` as the measured round trip of every
/// callback (as `wave-audio-io` does once real input flows), before the block is processed.
fn feed_measured(e: &Engine, rt: &mut Rt, at: &mut usize, total: usize, io: u32, input: &dyn Fn(usize) -> f32) {
    let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
    let end = *at + total;
    while *at < end {
        let n = BLOCK.min(end - *at);
        let inp: Vec<f32> = (0..n).map(|i| input(*at + i)).collect();
        e.shared().add_take_latency(io);
        rt.process(&mut l[..n], &mut r[..n], Some((&inp, &inp)));
        *at += n;
    }
}

fn first_take_onset(e: &Engine) -> (f64, usize) {
    let p = e.project();
    let clip = p.clips.iter().find(|c| c.kind == ClipKind::Audio).expect("a take");
    let audio = take_audio(&p, clip.source);
    (clip.start, audio.iter().position(|x| x.abs() > 0.5).expect("the hit was recorded"))
}

const MS1: i64 = SR as i64 / 1000;

/// The estimate at the start of a take is stale (`at_start`: the input was just opened, so
/// nothing was measured yet) while the device measures `io` during the take: the take lines
/// up with what was measured during it, with and without count-in.
fn record_with_stale_start(tag: &str, at_start: u32, count_in: bool) {
    let p = armed();
    let bpm = p.bpm;
    let e = engine(p, tag);
    let mut rt = e.create_rt(SR as f32, BLOCK);
    let io = 1700u32;
    e.shared().set_io_latency(at_start);
    e.set_transport(None, Some(count_in), Some(true), None);
    e.play(4.0);
    let lead = if count_in { frames(4.0, bpm) } else { 0 };
    // a hit sung exactly when beat 5 is heard reaches the engine `io` frames later
    let hit = lead + frames(1.0, bpm) + io as usize;
    let mut at = 0;
    feed_measured(&e, &mut rt, &mut at, lead + frames(3.0, bpm), io, &|f| if f == hit { 0.9 } else { 0.0 });
    let _ = e.stop();
    let (start, onset) = first_take_onset(&e);
    assert!((start - 4.0).abs() < 1e-9, "take starts at {start}");
    let off = onset as i64 - frames(1.0, bpm) as i64;
    assert!(off.abs() <= MS1, "{tag}: hit lands {off} frames off beat 5");
}

#[test]
fn a_take_started_before_the_round_trip_was_measured_still_lines_up() {
    record_with_stale_start("stale0", 0, false); // fresh stream: nothing published yet
    record_with_stale_start("stale512", 512, false); // one buffer guessed for the input
    record_with_stale_start("staleci", 0, true); // count-in: measured after the count
}

#[test]
fn a_running_input_lines_up_as_before() {
    record_with_stale_start("settled", 1700, false);
    record_with_stale_start("settledci", 1700, true);
}

#[test]
fn punch_in_with_an_input_opened_at_the_punch_lines_up() {
    // monitoring off: the input opens only when record is armed during playback, so the
    // estimate at the punch is the output-only guess; the take uses what was measured after
    let p = armed();
    let bpm = p.bpm;
    let e = engine(p, "punchio");
    let mut rt = e.create_rt(SR as f32, BLOCK);
    let io = 1300u32;
    e.shared().set_io_latency(600);
    e.play(2.0);
    let mut at = 0;
    feed(&mut rt, &mut at, frames(1.0, bpm), &|_| 0.0);
    e.set_transport(None, None, Some(true), None);
    let punch_at = at;
    let punch = e.shared().beat();
    // a hit sung when beat (punch + 2) is heard
    let hit = punch_at + frames(2.0, bpm) + io as usize;
    feed_measured(&e, &mut rt, &mut at, frames(3.0, bpm), io, &|f| if f == hit { 0.9 } else { 0.0 });
    e.set_transport(None, None, Some(false), None);
    let (start, onset) = first_take_onset(&e);
    assert!((start - punch).abs() < 1e-9, "take starts at {start} instead of {punch}");
    let want = ((punch + 2.0 - start) * 60.0 / bpm * SR as f64).round() as i64;
    let off = onset as i64 - want;
    assert!(off.abs() <= MS1, "punch-in hit lands {off} frames off");
}

#[test]
fn stopping_on_the_wrap_keeps_the_delayed_last_sample_of_each_pass() {
    let mut p = armed();
    p.bpm = 120.0;
    p.loop_range = Some(LoopRange { a: 8.0, b: 12.0 });
    let e = engine(p, "loop-stop");
    let mut rt = e.create_rt(SR as f32, BLOCK);
    let io = 2048;
    e.shared().set_io_latency(io as u32);
    e.set_transport(None, Some(false), Some(true), None);
    e.play(9.0); // the second pass must also capture beats 8..9
    let partial = frames(3.0, 120.0);
    let full = frames(4.0, 120.0);
    let input = |f| match f {
        f if f == io => 0.5,
        f if f == io + partial - 1 => 0.6,
        f if f == io + partial => 0.7,
        f if f == io + partial + full - 1 => 0.8,
        f if f >= io + partial + full => 0.9,
        _ => 0.0,
    };
    let mut at = 0;
    feed(&mut rt, &mut at, partial + full, &input);
    std::thread::scope(|scope| {
        let stop = scope.spawn(|| {
            let _ = e.stop();
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while e.state().transport.playing {
            assert!(std::time::Instant::now() < deadline, "Stop was not sent");
            std::thread::yield_now();
        }
        feed(&mut rt, &mut at, io + BLOCK, &input);
        stop.join().unwrap();
    });
    let p = e.project();
    let takes = &p.comps[0].takes;
    assert_eq!(takes.len(), 2);
    assert_eq!(takes[0].len, 3.0);
    assert_eq!(takes[1].len, 4.0);
    for (take, expected) in takes.iter().zip([(partial, 0.5, 0.6), (full, 0.7, 0.8)]) {
        let audio = take_audio(&p, take.source);
        assert_eq!((audio.len(), audio[0], *audio.last().unwrap()), expected);
        assert!(audio[1..audio.len() - 1].iter().all(|&sample| sample == 0.0));
    }
}

#[test]
fn the_recording_filter_leaves_audio_out_and_keeps_one_kind_on() {
    let e = engine(unarmed(&[], 0.0), "filter");
    // notes only: the input's audio is not taken
    let t = e.set_rec_filter(Some(false), None);
    assert!(!t.rec_audio && t.rec_notes);
    record_second(&e);
    assert_eq!(take_track(&e.project()), None, "audio was recorded with Audio unticked");
    // unticking the last kind leaves it on
    let t = e.set_rec_filter(None, Some(false));
    assert!(!t.rec_audio && t.rec_notes, "the filter must keep one kind on");
    // audio back on: it records again
    e.set_rec_filter(Some(true), None);
    record_second(&e);
    assert_eq!(take_track(&e.project()), Some(0));
}

#[test]
fn a_new_project_is_refused_while_a_take_is_active() {
    // Ctrl+N during a recording take must be refused (the app keeps the crash-recovery copy
    // of the still-unsaved project and reports the refusal), not silently ignored.
    let mut p = armed();
    p.bpm = 97.0;
    let e = engine(p, "newproj");
    let mut rt = e.create_rt(SR as f32, BLOCK);
    e.set_transport(None, Some(false), Some(true), None);
    e.play(0.0);
    let mut at = 0;
    feed(&mut rt, &mut at, BLOCK * 4, &|_| 0.3);
    assert!(e.new_project(Project::empty()).is_err(), "a take is active: the reset must be refused");
    assert_eq!(e.project().bpm, 97.0, "the refused reset must leave the project alone");
    let _ = e.stop();
    e.new_project(Project::empty()).expect("the take is over: the reset must work");
    assert!(e.project().clips.is_empty() && e.project().bpm != 97.0, "the new project replaced the old one");
}

#[test]
fn a_notes_only_take_refuses_new_project_too() {
    // `play` starts a MIDI take with Notes on and Audio off: it lives outside the audio
    // take, and a reset would let it finish into the new project (its old track index and
    // channel id) — the same refusal must cover it, and the reset must work after Stop.
    let mut p = armed();
    p.bpm = 97.0;
    let e = engine(p, "newproj-notes");
    let mut rt = e.create_rt(SR as f32, BLOCK);
    e.set_transport(None, Some(false), Some(true), None); // record on
    e.set_rec_filter(Some(false), Some(true)); // notes only: no audio take, `c.take` stays None
    e.play(0.0);
    let mut at = 0;
    feed(&mut rt, &mut at, BLOCK * 4, &|_| 0.0);
    let refused = e.new_project(Project::empty()).unwrap_err();
    assert!(refused.to_string().contains("recording"), "{refused}");
    assert_eq!(e.project().bpm, 97.0, "the refused reset must leave the project alone");
    let _ = e.stop();
    e.new_project(Project::empty()).expect("the take is over: the reset must work");
    assert!(e.project().bpm != 97.0, "the new project replaced the old one");
}
