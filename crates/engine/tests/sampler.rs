//! The sampler channel and the standard library: what the settings do to the sound and how
//! library samples are stored in a project.

use std::path::PathBuf;
use std::sync::Arc;

use wave_engine::*;

const SR: u32 = 48000;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wave-smp-{}-{}", std::process::id(), name));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// One second, 440 Hz: loud (0.8) for the first half, quiet (0.1) for the second.
fn two_level_wav(name: &str) -> PathBuf {
    // one file per call: tests run in parallel and several share a `name`, so one could read the
    // file while another was still writing it ("no suitable format reader found")
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = tmp(name).join(format!("two-level-{}.wav", N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let spec = hound::WavSpec { channels: 1, sample_rate: SR, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(&path, spec).unwrap();
    for i in 0..SR {
        let amp = if i < SR / 2 { 0.8 } else { 0.1 };
        w.write_sample((amp * (std::f64::consts::TAU * 440.0 * i as f64 / SR as f64).sin()) as f32).unwrap();
    }
    w.finalize().unwrap();
    path
}

/// An engine at 120 BPM whose only channel plays the test file with `set` applied; `notes`
/// are (step, length in steps, key) in a two-bar pattern on track 0. No master limiter.
fn engine(name: &str, set: impl Fn(&mut SamplerState), notes: &[(f32, f32, Option<u8>)]) -> (Arc<Engine>, Id) {
    let mut p = Project::empty();
    p.bpm = 120.0;
    p.inserts[0].fx.clear();
    let e = Engine::new(p, Arc::new(NoPlugins));
    let (srcs, _) = e.import_audio(&[two_level_wav(name)], None).unwrap();
    let mut q = e.project();
    let ch = q.alloc_id();
    let mut sampler = SamplerState::default();
    set(&mut sampler);
    let channel = Channel::new(ch, "Test", "#4f8ef7", ChannelSrc::Sample { source: srcs[0], sampler, slices: vec![] });
    let pattern = Pattern {
        id: 1,
        name: "P".into(),
        color: "#4f8ef7".into(),
        len: 8.0,
        min_len: 8.0,
        notes: notes.iter().map(|&(s, l, k)| Note::new(ch, s, l, k)).collect(),
        swing: 0.0,
    };
    let clip = Clip {
        id: 2,
        track: 0,
        start: 0.0,
        len: 8.0,
        offset: 0.0,
        kind: ClipKind::Pat,
        source: 1,
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
    };
    e.apply(&[Op::PutChannel { channel, index: None }, Op::PutPattern { pattern }, Op::PutClips { clips: vec![clip] }], false, None)
        .unwrap();
    (e, ch)
}

/// Left channel of the first `beats` beats.
fn render(e: &Engine, beats: f64) -> Vec<f32> {
    e.render(0.0, beats, 0.0, SR).unwrap().as_chunks::<2>().0.iter().map(|f| f[0]).collect()
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0f32, |m, v| m.max(v.abs()))
}

/// Peak of `x` between two times in seconds.
fn peak_in(x: &[f32], a: f64, b: f64) -> f32 {
    peak(&x[(a * SR as f64) as usize..((b * SR as f64) as usize).min(x.len())])
}

fn one(set: impl Fn(&mut SamplerState), len_steps: f32) -> Vec<f32> {
    let (e, _) = engine("one", set, &[(0.0, len_steps, None)]);
    render(&e, 4.0)
}

#[test]
fn a_sized_note_stops_at_its_end_or_releases_from_it_and_matches_pattern_preview() {
    for env in [false, true] {
        for steps in [1.0, 3.0] {
            let (e, _) = engine(
                "timed",
                |s| {
                    s.env = env;
                    s.release = 200.0;
                },
                &[(0.0, steps, None)],
            );
            let out = render(&e, 4.0);
            let end = (steps * SR as f32 / 8.0) as usize; // 120 BPM: one step = 125 ms
            assert!((peak(&out[end - 1000..end - 100]) - 0.8).abs() < 0.01);
            if env {
                // FL's envelope: the note-off starts a 200 ms release, half way down after 100 ms
                let mid = peak(&out[end + 4700..end + 4900]);
                assert!((mid - 0.4).abs() < 0.03, "half way through the release: {mid}");
                assert_eq!(peak(&out[end + 9700..]), 0.0, "silent once the release is over");
            } else {
                assert!(peak(&out[end - 24..end]) < 0.3, "declick before the note ends");
                assert_eq!(peak(&out[end..]), 0.0, "the sampler must be silent at the note end");
            }

            let mut rt = e.create_rt(SR as f32, 256);
            e.pattern_play(Some(1), 0.0, None).unwrap();
            let (mut left, mut right) = ([0.0; 256], [0.0; 256]);
            let live: Vec<_> = (0..(SR as usize / 256))
                .flat_map(|_| {
                    rt.process(&mut left, &mut right, None);
                    left
                })
                .collect();
            let diff = live.iter().zip(&out).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
            assert!(diff < 1e-5, "pattern preview differs from export by {diff}");
        }
    }

    // A long note plays an untrimmed sample as recorded, ending when the source runs out.
    let whole = one(|_| {}, 16.0);
    assert!(peak_in(&whole, 0.0, 0.002) > 0.5);
    assert!((peak_in(&whole, 0.55, 0.95) - 0.1).abs() < 0.01);
    assert_eq!(peak_in(&whole, 1.0, 2.0), 0.0);
}

#[test]
fn legacy_sized_notes_ring_out_or_release_with_the_envelope() {
    for env in [false, true] {
        let (e, _) = engine(
            "legacy",
            |s| {
                s.env = env;
                s.release = 200.0;
            },
            &[(0.0, 1.0, None)],
        );
        let mut pattern = e.project().patterns[0].clone();
        pattern.notes[0].trig = None;
        e.apply(&[Op::PutPattern { pattern }], false, None).unwrap();
        let out = render(&e, 4.0);
        if env {
            let after = peak_in(&out, 0.175, 0.195);
            assert!(after > 0.05 && after < 0.7, "a 200 ms release is still fading 50 ms in: {after}");
            assert_eq!(peak_in(&out, 0.326, 2.0), 0.0);
        } else {
            assert!((peak_in(&out, 0.55, 0.95) - 0.1).abs() < 0.01);
        }
    }
}

#[test]
fn an_enveloped_step_rings_out_and_round_trips_without_changing_short_held_notes() {
    let (e, ch) = engine("trigger", |s| s.env = true, &[(0.0, 1.0, None)]);
    let mut pattern = e.project().patterns[0].clone();
    pattern.notes[0].trig = None; // a file made before event kinds existed
    e.apply(&[Op::PutPattern { pattern: pattern.clone() }], false, None).unwrap();
    let path = tmp("trigger").join("legacy.wave");
    e.save(&path).unwrap();
    assert!(e.open(&path).unwrap().is_empty());
    assert_eq!(e.project().patterns[0].notes[0].trig, None);
    assert_eq!(peak_in(&render(&e, 4.0), 0.2, 0.99), 0.0);

    pattern.notes[0] = Note::trigger(ch, 0.0, None);
    e.apply(&[Op::PutPattern { pattern }], false, None).unwrap();
    let trigger = render(&e, 4.0);
    assert!(peak_in(&trigger, 0.9, 0.99) > 0.09);
    assert_eq!(peak_in(&trigger, 1.01, 2.0), 0.0);
    assert!(e.undo());
    assert_eq!(peak_in(&render(&e, 4.0), 0.2, 0.99), 0.0);
    assert!(e.redo());
    let path = tmp("trigger").join("explicit.wave");
    e.save(&path).unwrap();
    assert!(e.open(&path).unwrap().is_empty());
    assert_eq!(e.project().patterns[0].notes[0].trig, Some(true));
    assert_eq!(render(&e, 4.0), trigger);
}

#[test]
fn sample_fades_follow_the_audible_edges_and_match_the_live_engine() {
    for reverse in [false, true] {
        for trigger in [false, true] {
            let (e, ch) = engine(
                "fades",
                |s| {
                    s.start = 0.25;
                    s.end = 0.75;
                    s.reverse = reverse;
                    s.env = true;
                    s.fade_in_ms = 100.0;
                    s.fade_out_ms = 200.0;
                },
                &[(0.0, 8.0, None)],
            );
            if trigger {
                let mut pattern = e.project().patterns[0].clone();
                pattern.notes = vec![Note::trigger(ch, 0.0, None)];
                e.apply(&[Op::PutPattern { pattern }], false, None).unwrap();
            }
            let out = render(&e, 4.0);
            let (start, end) = if reverse { (0.1, 0.8) } else { (0.8, 0.1) };
            assert!(peak_in(&out, 0.04, 0.05) < start * 0.51);
            assert!(peak_in(&out, 0.11, 0.15) > start * 0.99);
            assert!(peak_in(&out, 0.37, 0.40) < end * 0.66);
            assert!(peak_in(&out, 0.49, 0.5) < end * 0.051);
            assert_eq!(peak_in(&out, 0.51, 2.0), 0.0);

            let mut rt = e.create_rt(SR as f32, 256);
            e.set_transport(Some(false), Some(false), None, None);
            e.play(0.0);
            let (mut left, mut right) = ([0.0; 256], [0.0; 256]);
            let live: Vec<_> = (0..(SR as usize / 256))
                .flat_map(|_| {
                    rt.process(&mut left, &mut right, None);
                    left
                })
                .collect();
            let diff = live.iter().zip(&out).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
            assert!(diff < 1e-5, "live and offline fades differ by {diff}");
        }
    }
}

#[test]
fn start_and_end_pick_the_part_that_plays() {
    // the second half only: quiet from the first sample
    let tail = one(|s| s.start = 0.5, 16.0);
    assert!((peak_in(&tail, 0.01, 0.45) - 0.1).abs() < 0.01);
    assert_eq!(peak_in(&tail, 0.51, 2.0), 0.0);
    // the first quarter only
    let head = one(|s| s.end = 0.25, 16.0);
    assert!((peak_in(&head, 0.0, 0.24) - 0.8).abs() < 0.01);
    assert_eq!(peak_in(&head, 0.2501, 2.0), 0.0);
    // a cut inside the waveform is faded, not clicked: the last half millisecond is quiet
    assert!(peak_in(&head, 0.2496, 0.25) < 0.3);
}

#[test]
fn reverse_plays_the_part_backwards() {
    let out = one(|s| s.reverse = true, 16.0);
    assert!((peak_in(&out, 0.05, 0.45) - 0.1).abs() < 0.01, "the quiet end comes first");
    assert!((peak_in(&out, 0.55, 0.95) - 0.8).abs() < 0.01);
    assert_eq!(peak_in(&out, 1.01, 2.0), 0.0);
    // reversed and trimmed: the loud first half, backwards
    let half = one(
        |s| {
            s.reverse = true;
            s.end = 0.5;
        },
        16.0,
    );
    assert!((peak_in(&half, 0.05, 0.45) - 0.8).abs() < 0.01);
    assert_eq!(peak_in(&half, 0.51, 2.0), 0.0);
}

#[test]
fn the_envelope_shapes_the_level() {
    // attack: silent at first, full after it
    let slow = one(
        |s| {
            s.env = true;
            s.attack = 200.0;
        },
        16.0,
    );
    assert!(peak_in(&slow, 0.0, 0.01) < 0.05);
    let mid = peak_in(&slow, 0.099, 0.102);
    assert!((mid - 0.4).abs() < 0.03, "half way up the attack: {mid}");
    assert!((peak_in(&slow, 0.21, 0.45) - 0.8).abs() < 0.01);
    // decay to a sustain level
    let held = one(
        |s| {
            s.env = true;
            s.decay = 100.0;
            s.sustain = 0.5;
        },
        16.0,
    );
    assert!((peak_in(&held, 0.11, 0.45) - 0.4).abs() < 0.01);
    // sustain 0: the decay is the whole sound, however long the file
    let pluck = one(
        |s| {
            s.env = true;
            s.decay = 100.0;
            s.sustain = 0.0;
        },
        16.0,
    );
    assert!(peak_in(&pluck, 0.0, 0.01) > 0.7);
    assert_eq!(peak_in(&pluck, 0.101, 2.0), 0.0);
}

#[test]
fn with_the_envelope_off_the_sample_plays_as_recorded() {
    let shaped = |s: &mut SamplerState| {
        s.attack = 200.0;
        s.decay = 100.0;
        s.sustain = 0.0;
    };
    // the stored shape is kept but not heard
    let off = one(shaped, 16.0);
    assert!(peak_in(&off, 0.0, 0.002) > 0.5);
    assert!((peak_in(&off, 0.55, 0.95) - 0.1).abs() < 0.01);
    assert_eq!(off, one(|_| {}, 16.0));
}

#[test]
fn delay_and_hold_place_the_attack_and_the_decay() {
    let out = one(
        |s| {
            s.env = true;
            s.delay = 100.0;
            s.hold = 100.0;
            s.decay = 100.0;
            s.sustain = 0.0;
        },
        16.0,
    );
    assert_eq!(peak_in(&out, 0.0, 0.099), 0.0, "silent through the delay");
    assert!((peak_in(&out, 0.101, 0.199) - 0.8).abs() < 0.01, "full through the hold");
    let mid = peak_in(&out, 0.249, 0.251);
    assert!((mid - 0.4).abs() < 0.03, "half way down the decay: {mid}");
    assert_eq!(peak_in(&out, 0.301, 2.0), 0.0);
}

#[test]
fn tension_bends_each_segment_without_moving_its_ends() {
    let attack_mid = |k: f32| {
        let out = one(
            |s| {
                s.env = true;
                s.attack = 200.0;
                s.attack_tension = k;
            },
            16.0,
        );
        assert!((peak_in(&out, 0.21, 0.45) - 0.8).abs() < 0.01, "the attack still ends at full");
        peak_in(&out, 0.099, 0.102) / 0.8
    };
    let (down, straight, up) = (attack_mid(-1.0), attack_mid(0.0), attack_mid(1.0));
    assert!((straight - 0.5).abs() < 0.04);
    // bent fully up the attack is at 8/9 half way; bent down at 1/9
    assert!((up - 8.0 / 9.0).abs() < 0.04, "{up}");
    assert!((down - 1.0 / 9.0).abs() < 0.04, "{down}");

    // release: up keeps the level high for longer, down falls away fast
    let release_mid = |k: f32| {
        let (e, _) = engine(
            "release-bend",
            |s| {
                s.env = true;
                s.release = 200.0;
                s.release_tension = k;
            },
            &[(0.0, 1.0, None)],
        );
        let out = render(&e, 4.0);
        assert_eq!(peak_in(&out, 0.326, 2.0), 0.0, "the release still ends on time");
        peak_in(&out, 0.2240, 0.2260) / 0.8
    };
    assert!(release_mid(1.0) > 0.85 && release_mid(-1.0) < 0.15);
}

#[test]
fn a_note_off_during_the_attack_releases_from_where_it_was() {
    // a one-step (125 ms) note with a 500 ms attack: released at a quarter of full, no jump up
    let (e, _) = engine(
        "early-off",
        |s| {
            s.env = true;
            s.attack = 500.0;
            s.release = 100.0;
        },
        &[(0.0, 1.0, None)],
    );
    let out = render(&e, 4.0);
    let at_off = peak_in(&out, 0.120, 0.125);
    assert!((at_off - 0.2).abs() < 0.02, "{at_off}");
    assert!(peak_in(&out, 0.125, 0.3) <= at_off + 0.005, "the release never rises");
    assert_eq!(peak_in(&out, 0.226, 2.0), 0.0);
}

#[test]
fn files_from_before_the_envelope_switch_keep_their_sound() {
    let doc = |sampler: serde_json::Value| {
        let mut v = serde_json::to_value(Project::empty()).unwrap();
        v["sources"] = serde_json::json!([]);
        v["channels"] = serde_json::json!([{ "id": 900, "name": "S", "color": "#fff", "kind": "sample", "source": 3, "sampler": sampler }]);
        let p = Project::from_json(v).unwrap();
        let ChannelSrc::Sample { sampler, .. } = p.channel(900).unwrap().src else { panic!() };
        sampler
    };
    // gated, or one-shot with a shape it always played: the envelope is on
    assert!(doc(serde_json::json!({ "gate": true })).env);
    assert!(doc(serde_json::json!({ "gate": false, "attack": 30.0 })).env);
    assert!(doc(serde_json::json!({ "decay": 100.0, "sustain": 0.2 })).env);
    // an untouched one-shot: off, as before; a new file's switch is read as written
    assert!(!doc(serde_json::json!({ "gate": false, "decay": 100.0 })).env);
    assert!(!doc(serde_json::json!({})).env);
    assert!(!doc(serde_json::json!({ "env": false, "attack": 30.0 })).env);
}

#[test]
fn transpose_and_fine_tune_repitch_on_top_of_the_note() {
    let zero_crossings = |x: &[f32]| x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() as f64;
    let hz = |set: fn(&mut SamplerState), key: Option<u8>| {
        let (e, _) = engine("tune", set, &[(0.0, 16.0, key)]);
        let out = render(&e, 1.0);
        zero_crossings(&out[SR as usize / 100..SR as usize / 5]) / (0.2 - 0.01)
    };
    assert!((hz(|_| {}, None) - 440.0).abs() < 6.0);
    assert!((hz(|s| s.semi = 12, None) - 880.0).abs() < 8.0);
    assert!((hz(|s| s.semi = -12, None) - 220.0).abs() < 6.0);
    // +100 cents is a semitone; on top of a note a semitone down it cancels
    assert!((hz(|s| s.fine = 100.0, Some(59)) - 440.0).abs() < 6.0);
    // an octave up is also twice as fast: the file is over in half the time
    let (e, _) = engine("tune", |s| s.semi = 12, &[(0.0, 16.0, None)]);
    assert_eq!(peak_in(&render(&e, 4.0), 0.51, 2.0), 0.0);
}

#[test]
fn normalise_brings_the_peak_up() {
    assert!((peak(&one(|_| {}, 16.0)) - 0.8).abs() < 0.01);
    let want = 10f32.powf(-0.3 / 20.0);
    let got = peak(&one(|s| s.norm = true, 1.0));
    assert!((got - want).abs() < 0.01, "normalised peak {got}, expected {want}");
}

#[test]
fn a_cut_group_silences_its_sounding_notes() {
    // two notes of the same channel a beat apart; the file lasts two beats
    let notes = [(0.0, 16.0, None), (4.0, 16.0, None)];
    let free = render(&engine("cut", |_| {}, &notes).0, 4.0);
    // without a group they overlap: the first note's quiet half under the second's loud half
    assert!(peak_in(&free, 0.52, 0.95) > 0.8);
    let cut = render(&engine("cut", |s| s.cut = 1, &notes).0, 4.0);
    let (a, b) = (peak_in(&cut, 0.52, 0.95), peak_in(&cut, 1.02, 1.45));
    assert!((a - 0.8).abs() < 0.01 && (b - 0.1).abs() < 0.01, "only the second note sounds: {a}, {b}");
    // the cut is at the new note, not earlier: the first note is whole until then
    assert!((peak_in(&cut, 0.40, 0.495) - 0.8).abs() < 0.01);

    let (e, ch) = engine(
        "trigger-cut",
        |s| {
            s.env = true;
            s.cut = 1;
        },
        &notes,
    );
    let mut pattern = e.project().patterns[0].clone();
    pattern.notes = vec![Note::trigger(ch, 0.0, None), Note::trigger(ch, 4.0, None)];
    e.apply(&[Op::PutPattern { pattern }], false, None).unwrap();
    assert_eq!(render(&e, 4.0), cut);

    // across channels: a second channel in the group cuts the first; another group does not
    let across = |group: u8| {
        let (e, ch) = engine("cut2", |s| s.cut = 1, &[(0.0, 16.0, None)]);
        let mut p = e.project();
        let src = p.channel(ch).unwrap().sample_source().unwrap();
        let other = p.alloc_id();
        // the other channel plays only the quiet second half, so the two can be told apart
        let sampler = SamplerState { cut: group, start: 0.5, ..Default::default() };
        let mut pat = p.patterns[0].clone();
        pat.notes.push(Note::new(other, 4.0, 16.0, None));
        e.apply(
            &[
                Op::PutChannel {
                    channel: Channel::new(other, "Other", "#fff", ChannelSrc::Sample { source: src, sampler, slices: vec![] }),
                    index: None,
                },
                Op::PutPattern { pattern: pat },
            ],
            false,
            None,
        )
        .unwrap();
        peak_in(&render(&e, 4.0), 0.55, 0.95)
    };
    assert!((across(1) - 0.1).abs() < 0.02, "same group: the first channel is cut");
    assert!(across(2) > 0.1 + 0.05, "another group: both sound");
}

#[test]
fn settings_out_of_range_are_refused() {
    let (e, ch) = engine("check", |_| {}, &[]);
    let base = e.project().channel(ch).unwrap().clone();
    let with = |f: &dyn Fn(&mut SamplerState)| {
        let mut c = base.clone();
        if let ChannelSrc::Sample { sampler, .. } = &mut c.src {
            f(sampler);
        }
        e.apply(&[Op::PutChannel { channel: c, index: None }], false, None)
    };
    assert!(with(&|s| s.start = 0.6).is_ok());
    assert!(with(&|s| (s.start, s.end) = (0.6, 0.6)).is_err(), "an empty part");
    assert!(with(&|s| s.end = 1.5).is_err());
    assert!(with(&|s| s.start = f32::NAN).is_err());
    assert!(with(&|s| s.semi = 25).is_err());
    assert!(with(&|s| s.fine = 101.0).is_err());
    assert!(with(&|s| s.attack = -1.0).is_err());
    assert!(with(&|s| s.sustain = 1.1).is_err());
    assert!(with(&|s| s.release = 0.0).is_err());
    assert!(with(&|s| s.delay = 5001.0).is_err());
    assert!(with(&|s| s.hold = -1.0).is_err());
    assert!(with(&|s| s.release_tension = 1.5).is_err());
    assert!(with(&|s| s.attack_tension = f32::NAN).is_err());
    assert!(with(&|s| s.fade_in_ms = -1.0).is_err());
    assert!(with(&|s| s.fade_out_ms = 5001.0).is_err());
    assert!(with(&|s| s.cut = 9).is_err());
    assert!(with(&|s| s.cut = 8).is_ok());
    // a refused edit changes nothing, and an accepted one is one undo step
    let ChannelSrc::Sample { sampler, .. } = e.project().channel(ch).unwrap().src else { panic!() };
    assert_eq!((sampler.start, sampler.cut), (0.0, 8));
    e.undo();
    let ChannelSrc::Sample { sampler, .. } = e.project().channel(ch).unwrap().src else { panic!() };
    assert_eq!((sampler.start, sampler.cut), (0.6, 0));
}

#[test]
fn settings_survive_a_save_and_files_without_them_get_the_defaults() {
    let (e, ch) = engine(
        "save",
        |s| {
            s.start = 0.25;
            s.reverse = true;
            s.semi = -5;
            s.env = true;
            s.delay = 5.0;
            s.hold = 20.0;
            s.decay = 80.0;
            s.decay_tension = 0.5;
            s.sustain = 0.4;
            s.cut = 3;
        },
        &[(0.0, 2.0, None)],
    );
    let path = tmp("save").join("song.wave");
    e.save(&path).unwrap();
    let before = render(&e, 2.0);
    let f = Engine::new(Project::empty(), Arc::new(NoPlugins));
    assert!(f.open(&path).unwrap().is_empty());
    assert_eq!(f.project().channel(ch).unwrap().src, e.project().channel(ch).unwrap().src);
    assert!(render(&f, 2.0) == before);
    // a channel written before the sampler had settings
    let old: Channel =
        serde_json::from_value(serde_json::json!({ "id": 7, "name": "Stab", "color": "#fff", "kind": "sample", "source": 3 })).unwrap();
    assert_eq!(old.src, ChannelSrc::Sample { source: 3, sampler: SamplerState::default(), slices: vec![] });
}

#[test]
fn library_samples_are_stored_by_a_portable_path_and_shared() {
    let e = Engine::new(Project::empty(), Arc::new(NoPlugins));
    let n = e.project().sources.len();
    // the browser hands over the file where the library is installed on this computer
    let dir = factory::install().unwrap();
    let file = dir.join("triangle.flac");
    assert!(file.is_file());
    let (a, clips) = e.import_audio(std::slice::from_ref(&file), Some((2, 4.0))).unwrap();
    let p = e.project();
    let src = p.sources.iter().find(|s| s.id == a[0]).unwrap();
    assert_eq!(src.path, "lib:Standard/triangle.flac");
    assert_eq!(src.name, "Triangle");
    assert_eq!(clips.len(), 1);
    // again (a sampler channel, another clip): the same source, not a copy
    let (b, _) = e.import_audio(&[file], None).unwrap();
    assert_eq!(b, a);
    assert_eq!(e.project().sources.len(), n + 1);
    // the saved file names no folder of this computer, and opens with its audio
    let path = tmp("lib").join("song.wave");
    e.save(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("lib:Standard/triangle.flac"));
    assert!(!text.contains(&factory::library_dir().display().to_string()));
    let f = Engine::new(Project::empty(), Arc::new(NoPlugins));
    assert!(f.open(&path).unwrap().is_empty());
    assert!(peak(&f.render(4.0, 6.0, 0.0, SR).unwrap()) > 0.05);
    // a sample of a kit that is not installed is reported, not fatal
    let broken = text.replace("lib:Standard/triangle.flac", "lib:No Such Kit/triangle.flac");
    let path2 = tmp("lib").join("broken.wave");
    std::fs::write(&path2, broken).unwrap();
    let warnings = f.open(&path2).unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("not installed"), "{}", warnings[0]);
}

#[test]
fn a_new_project_does_not_inherit_audio_from_the_previous_one() {
    // ids start over: the audio of source N of the old project must not play as source N of the new
    let (e, _) = engine("fresh", |_| {}, &[(0.0, 1.0, None)]);
    e.new_project(Project::empty()).expect("no take is active");
    let p = e.project();
    assert!(p.sources.iter().all(|s| s.path.starts_with("lib:")));
    let kick = p.channels[0].id;
    let pattern = Pattern {
        id: 1,
        name: "P".into(),
        color: "#fff".into(),
        len: 4.0,
        min_len: 4.0,
        notes: vec![Note::new(kick, 0.0, 1.0, None)],
        swing: 0.0,
    };
    let clip = Clip {
        id: 2,
        track: 0,
        start: 0.0,
        len: 4.0,
        offset: 0.0,
        kind: ClipKind::Pat,
        source: 1,
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
    };
    e.apply(&[Op::PutPattern { pattern }, Op::PutClips { clips: vec![clip] }], false, None).unwrap();
    let out = e.render(0.0, 4.0, 0.0, SR).unwrap();
    // the kick is 0.38 s long; the test file would ring for a second
    assert!(peak(&out) > 0.2);
    assert_eq!(peak(&out[2 * SR as usize * 6 / 10..]), 0.0);
}

#[test]
fn the_demo_project_is_self_contained_and_every_section_sounds() {
    let p = Project::demo();
    assert_eq!(p.bpm, wave_engine::demo::DEMO_BPM);
    assert_eq!(p.key, Some(3));
    assert_eq!(p.scale, 2);
    assert!(p.sources.iter().all(|s| factory::embedded(&s.path).is_some()));
    for c in &p.clips {
        match c.kind {
            ClipKind::Pat => assert!(p.patterns.iter().any(|x| x.id == c.source)),
            ClipKind::Audio => assert!(p.sources.iter().any(|x| x.id == c.source)),
            ClipKind::Auto => assert!(p.automations.iter().any(|x| x.id == c.source)),
            ClipKind::Comp => panic!("the demo has no comps"),
        }
        assert!((c.track as usize) < p.tracks.len());
        if let Some(ins) = c.insert {
            assert!(p.insert(ins).is_some());
        }
    }
    for pt in &p.patterns {
        assert!(p.clips.iter().any(|c| c.kind == ClipKind::Pat && c.source == pt.id), "pattern {} is not on the playlist", pt.name);
        assert!(pt.notes.iter().all(|n| p.channel(n.ch).is_some()));
    }
    for c in &p.channels {
        assert!(p.patterns.iter().any(|pt| pt.notes.iter().any(|n| n.ch == c.id)), "channel {} plays nothing", c.name);
        if let Some(ins) = c.insert {
            assert!(p.insert(ins).is_some());
        }
    }
    for t in &p.tracks {
        assert!(p.insert(t.insert).is_some());
    }
    for m in &p.inserts {
        wave_engine::routing::validate(&p, m).unwrap_or_else(|e| panic!("insert {}: {e}", m.name));
    }

    let unique = |mut ids: Vec<Id>, what: &str| {
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "{what} ids repeat");
        ids.last().copied().unwrap_or(0)
    };
    unique(p.clips.iter().map(|c| c.id).collect(), "clip");
    unique(p.patterns.iter().map(|x| x.id).collect(), "pattern");
    unique(p.inserts.iter().flat_map(|m| m.fx.iter().map(|f| f.id)).collect(), "effect");
    let top = unique(
        p.channels.iter().map(|x| x.id).chain(p.sources.iter().map(|x| x.id)).chain(p.automations.iter().map(|x| x.id)).collect(),
        "channel, source and automation",
    );
    assert!(p.next_id > top);
    assert_eq!(p.song_end(), 160.0);

    let mut again = p.clone();
    again.migrate();
    assert_eq!(again, p);

    // round-trip save & open without warnings
    let dir = tmp("demo-roundtrip");
    let file = dir.join("demo.wave");
    let e = Engine::new(p.clone(), Arc::new(NoPlugins));
    e.save(&file).unwrap();
    let reopened = Engine::new(Project::empty(), Arc::new(NoPlugins));
    assert!(reopened.open(&file).unwrap().is_empty());
    assert_eq!(reopened.project(), p);
    let _ = std::fs::remove_dir_all(&dir);

    let out = e.render(0.0, 160.0, 2.0, SR).unwrap();
    let frames_per_beat = 60.0 / wave_engine::demo::DEMO_BPM * SR as f64;
    let rms = |a: f64, b: f64| {
        let s = &out[(a * frames_per_beat) as usize * 2..(b * frames_per_beat) as usize * 2];
        (s.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / s.len() as f64).sqrt()
    };
    for (name, a, b) in [
        ("intro", 0.0, 16.0),
        ("act 1 hook", 16.0, 48.0),
        ("verse", 48.0, 80.0),
        ("bridge", 80.0, 94.0),
        ("act 2 drop", 96.0, 128.0),
        ("climax", 128.0, 144.0),
        ("outro", 144.0, 156.0),
    ] {
        let v = rms(a, b);
        assert!(v > 0.02, "{name} is silent ({v})");
    }
    // pre-drop vacuum cuts cleanly to silence right before Act II
    assert!(rms(95.25, 95.85) < 1e-4, "switch vacuum leaked: {}", rms(95.25, 95.85));
    let pk = peak(&out);
    assert!(pk <= 1.0, "the demo clips: {pk}");
    assert!(peak(&out[out.len() - SR as usize / 2..]) < 0.02, "outro tail did not fade out");
}

/// The in-browser mock (`ui/src/api/mock.ts`) must start new and demo projects like the engine
/// and list the same standard library, so it reads them from a file generated here. After a
/// change to the demo, the starter kit or the library, regenerate it:
/// `WAVE_UPDATE_FIXTURES=1 cargo test -p wave-engine --test sampler mock_fixture`.
#[test]
fn the_mock_fixture_matches_the_engine() {
    let files: Vec<serde_json::Value> = factory::files()
        .iter()
        .map(|f| serde_json::json!({ "file": f.file, "name": f.name, "group": f.group, "root": f.root, "pitched": f.pitched, "dur": f.dur, "ch": f.ch, "bytes": f.bytes.len(), "color": factory::color(f) }))
        .collect();
    let unlisted: Vec<_> = factory::files().iter().filter(|f| !f.is_listed()).map(|f| f.file).collect();
    let want = serde_json::json!({ "kit": factory::KIT, "files": files, "unlisted": unlisted, "empty": Project::empty(), "demo": Project::demo() });
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui/src/api/factory.json");
    if std::env::var_os("WAVE_UPDATE_FIXTURES").is_some() {
        std::fs::write(path, serde_json::to_string(&want).unwrap() + "\n").unwrap();
    }
    let have: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).expect("ui/src/api/factory.json")).unwrap();
    // compare through JSON text: floats written and read back are not bit-identical to f32
    let norm = |v: &serde_json::Value| serde_json::from_str::<serde_json::Value>(&serde_json::to_string(v).unwrap()).unwrap();
    assert!(norm(&have) == norm(&want), "ui/src/api/factory.json is out of date: see this test's comment");
}

/// The sample editor's keyboard on an audio clip (FL's audio clip channel): a key plays the
/// clip's sample transposed from the clip's root key, plus its pitch knob; the root is saved
/// only when it is not the default and an out-of-range one is refused.
#[test]
fn an_audio_clip_plays_from_its_keyboard_relative_to_its_root() {
    let mut p = Project::empty();
    p.bpm = 120.0;
    p.inserts[0].fx.clear();
    let e = Engine::new(p, Arc::new(NoPlugins));
    let (srcs, _) = e.import_audio(&[two_level_wav("cliproot")], None).unwrap();
    let clip = |root: u8, pitch: f32| Clip {
        id: 7,
        track: 0,
        start: 0.0,
        len: 2.0,
        offset: 0.0,
        kind: ClipKind::Audio,
        source: srcs[0],
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
        pitch,
        insert: None,
        root,
    };
    let zero_crossings = |x: &[f32]| x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() as f64;
    let hz = |root: u8, pitch: f32, key: u8| {
        e.apply(&[Op::PutClips { clips: vec![clip(root, pitch)] }], false, None).unwrap();
        let mut rt = e.create_rt(SR as f32, 256);
        e.preview_clip_key(7, key, 1.0).unwrap();
        let (mut left, mut right) = ([0.0; 256], [0.0; 256]);
        let out: Vec<_> = (0..(SR as usize / 5 / 256))
            .flat_map(|_| {
                rt.process(&mut left, &mut right, None);
                left
            })
            .collect();
        zero_crossings(&out[SR as usize / 100..]) / (out.len() as f64 / SR as f64 - 0.01)
    };
    assert!((hz(60, 0.0, 60) - 440.0).abs() < 8.0, "the root key plays the sample as recorded");
    assert!((hz(60, 0.0, 72) - 880.0).abs() < 12.0, "an octave above the root plays twice as fast");
    assert!((hz(69, 0.0, 57) - 220.0).abs() < 6.0, "a moved root moves every key");
    assert!((hz(60, -12.0, 72) - 440.0).abs() < 8.0, "the pitch knob adds to the key");

    let json = serde_json::to_value(clip(60, 0.0)).unwrap();
    assert!(json.get("root").is_none(), "the default root is not written");
    let back: Clip = serde_json::from_value(json).unwrap();
    assert_eq!(back.root, CLIP_ROOT, "a clip saved without a root gets C5");
    assert_eq!(serde_json::to_value(clip(57, 0.0)).unwrap()["root"], 57);
    assert!(e.apply(&[Op::PutClips { clips: vec![clip(128, 0.0)] }], false, None).is_err());
    assert!(e.preview_clip_key(999, 60, 1.0).is_err());
}

#[test]
fn a_file_loaded_for_a_sampler_is_not_an_undo_step_of_its_own() {
    let e = Engine::new(Project::empty(), Arc::new(NoPlugins));
    let clips_before = e.project().clips.len();
    // loaded for a sampler channel or a replaced sample: nothing to place, nothing to undo
    let (srcs, clips) = e.import_audio_with(&[two_level_wav("noundo-a")], None, false).unwrap();
    assert!(clips.is_empty());
    assert!(!e.undo(), "a file loaded for another edit must not add an undo step");
    assert!(e.project().sources.iter().any(|s| s.id == srcs[0]), "the source stays for the edit that uses it");
    // placed on a track it is one step, undone together with its clip
    let (_, clips) = e.import_audio(&[two_level_wav("noundo-b")], Some((0, 0.0))).unwrap();
    assert_eq!(clips.len(), 1);
    assert_eq!(e.project().clips.len(), clips_before + 1);
    assert!(e.undo());
    assert_eq!(e.project().clips.len(), clips_before);
    assert!(!e.undo(), "one import, one step");
    // into the audio list on its own (the Browser's Import) it is still a step, as before
    let n = e.project().sources.len();
    e.import_audio(&[two_level_wav("noundo-c")], None).unwrap();
    assert_eq!(e.project().sources.len(), n + 1);
    assert!(e.undo());
    assert_eq!(e.project().sources.len(), n);
    // and a file loaded for another edit drops redo, so redo cannot bring back a project without it
    assert!(e.redo());
    assert!(e.undo());
    e.import_audio_with(&[two_level_wav("noundo-d")], None, false).unwrap();
    assert!(!e.redo(), "redo must be cleared when a new source came in");
}

/// A sample dropped on the Channel Rack (loaded without a step, then the channel): one Ctrl+Z
/// takes both back out, so the file does not stay in the project's audio list for good.
#[test]
fn undoing_a_sampler_made_from_a_file_also_removes_the_file() {
    let e = Engine::new(Project::empty(), Arc::new(NoPlugins));
    let sources = e.project().sources.len();
    let channels = e.project().channels.len();
    let (srcs, _) = e.import_audio_with(&[two_level_wav("fold-a")], None, false).unwrap();
    let id = e.project().next_id + 1;
    let ch = Channel::sampler(id, "Drop", "#69c3d6", srcs[0]);
    e.apply(&[Op::PutChannel { channel: ch, index: None }], false, None).unwrap();
    assert_eq!(e.project().channels.len(), channels + 1);
    assert!(e.undo());
    assert_eq!(e.project().channels.len(), channels);
    assert_eq!(e.project().sources.len(), sources, "the dropped file goes with its channel");
    assert!(!e.undo(), "one drop, one step");
    // redo brings both back
    assert!(e.redo());
    assert_eq!(e.project().sources.len(), sources + 1);
    assert_eq!(e.project().channels.len(), channels + 1);
}

/// A file loaded (no step of its own) in the middle of a knob drag: Ctrl+Z after the drag goes
/// back to before the drag, not to the knob values the import happened to see.
#[test]
fn a_file_loaded_during_a_gesture_undoes_to_the_gestures_start() {
    let e = Engine::new(Project::empty(), Arc::new(NoPlugins));
    let start = e.project();
    let m = start.inserts[0].clone();
    e.begin_gesture();
    e.apply(&[Op::PutInsert { insert: Insert { vol: 0.5, ..m.clone() } }], true, None).unwrap();
    e.import_audio_with(&[two_level_wav("gesture-import")], None, false).unwrap();
    e.apply(&[Op::PutInsert { insert: Insert { vol: 0.25, ..m.clone() } }], true, None).unwrap();
    e.end_gesture();
    assert!(e.undo());
    assert_eq!(e.project().inserts[0].vol, m.vol, "undo returns to the value before the drag");
    assert_eq!(e.project().sources, start.sources, "and the file loaded during it goes too");
}

/// The test channel cut into two slices at the middle of the file (loud half, quiet half).
fn sliced(name: &str, notes: &[(f32, f32, Option<u8>)]) -> Vec<f32> {
    let (e, ch) = engine(name, |_| {}, notes);
    let mut c = e.project().channel(ch).unwrap().clone();
    if let ChannelSrc::Sample { slices, .. } = &mut c.src {
        *slices = vec![0.0, 0.5];
    }
    e.apply(&[Op::PutChannel { channel: c, index: None }], false, None).unwrap();
    render(&e, 4.0)
}

#[test]
fn slice_mode_plays_one_slice_per_key_at_the_files_pitch() {
    // the root key plays slice 1 (the loud half) and stops at the next marker
    let x = sliced("slice-root", &[(0.0, 8.0, Some(60))]);
    assert!(peak_in(&x, 0.05, 0.45) > 0.7, "slice 1 is the loud half");
    assert!(peak_in(&x, 0.52, 1.0) < 1e-3, "and ends where slice 2 starts");
    // root + 1 plays slice 2 (the quiet half), not the loud one pitched up a semitone
    let y = sliced("slice-two", &[(0.0, 8.0, Some(61))]);
    assert!((peak_in(&y, 0.05, 0.45) - 0.1).abs() < 0.02, "slice 2: {}", peak_in(&y, 0.05, 0.45));
    assert!(peak_in(&y, 0.52, 1.0) < 1e-3, "slice 2 ends with the file");
    // its pitch is the file's: 440 Hz has ~ 220 zero crossings in 0.25 s
    let seg = &y[(0.1 * SR as f64) as usize..(0.35 * SR as f64) as usize];
    let zc = seg.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
    assert!((210..=230).contains(&zc), "{zc} zero crossings");
    // a key past the last slice plays nothing
    let z = sliced("slice-none", &[(0.0, 8.0, Some(62))]);
    assert!(peak(&z) < 1e-6);
}

#[test]
fn a_new_slice_cuts_the_one_sounding() {
    // slice 1 at 0, slice 2 an eighth note later: the loud half stops when the quiet one starts
    let x = sliced("slice-cut", &[(0.0, 8.0, Some(60)), (2.0, 8.0, Some(61))]);
    assert!(peak_in(&x, 0.05, 0.24) > 0.7);
    assert!(peak_in(&x, 0.28, 0.6) < 0.15, "after the cut only slice 2 sounds: {}", peak_in(&x, 0.28, 0.6));
}
