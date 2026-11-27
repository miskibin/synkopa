//! Music Box: the modeled tine instrument as a Channel Rack channel (model, ops, rendering).

use std::sync::Arc;

use wave_engine::poly::lab;
use wave_engine::*;

const SR: u32 = 48000;

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

const CH: Id = 3000;

fn project(notes: Vec<Note>, music_box: MusicBoxState) -> Project {
    let mut p = Project::empty();
    p.inserts[0].fx.clear();
    p.channels.push(Channel {
        name: "Music Box".into(),
        src: ChannelSrc::MusicBox { music_box },
        root: 60,
        ..Channel::new(CH, "", "#cf8e52", ChannelSrc::MusicBox { music_box: Default::default() })
    });
    p.patterns = vec![Pattern {
        id: 1,
        name: "P".into(),
        color: "#4f8ef7".into(),
        len: 4.0,
        min_len: 4.0,
        bar: 4.0,
        notes,
        swing: 0.0,
        channel_loops: std::collections::BTreeMap::new(),
    }];
    p.clips = vec![Clip {
        id: 1,
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
        follow_tempo: false,
        pitch: 0.0,
        insert: None,
        root: 60,
    }];
    p
}

fn render_to(p: Project, to_beat: f64) -> Vec<f32> {
    let e = Engine::new(p, Arc::new(NoPlugins));
    e.render(0.0, to_beat, 0.0, SR).unwrap().as_chunks::<2>().0.iter().map(|f| f[0]).collect()
}

fn render(p: Project) -> Vec<f32> {
    render_to(p, 4.0)
}

fn note(step: f32, len: f32, key: u8, v: f32) -> Note {
    Note { v, ..Note::new(CH, step, len, Some(key)) }
}

/// Dominant frequency of `x` by zero crossings (the fundamental dominates the amplitude).
fn freq(x: &[f32]) -> f64 {
    let n = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    n as f64 * SR as f64 / x.len() as f64
}

/// One-pole highpass at roughly `hz`, for measuring the overtone stack.
fn highpass(x: &[f32], hz: f64) -> Vec<f32> {
    let a = (-std::f64::consts::TAU * hz / SR as f64).exp();
    let mut prev_in = 0.0f64;
    let mut prev_out = 0.0f64;
    x.iter()
        .map(|&s| {
            let s = s as f64;
            let y = a * (prev_out + s - prev_in);
            prev_in = s;
            prev_out = y;
            y as f32
        })
        .collect()
}

#[test]
fn plays_the_note_pitch_across_the_keyboard() {
    // measure after the attack: the first ~150 ms carries the bright overtone stack
    let x = render(project(vec![note(0.0, 8.0, 60, 0.9)], MusicBoxState::default()));
    let f = freq(&x[10000..40000]);
    assert!((f - 261.63).abs() < 5.0, "C4 expected, got {f:.1} Hz");
    let x = render(project(vec![note(0.0, 8.0, 84, 0.9)], MusicBoxState::default()));
    let f = freq(&x[10000..40000]);
    assert!((f - 1046.5).abs() < 16.0, "C6 expected, got {f:.1} Hz");
}

#[test]
fn a_hard_pluck_is_brighter_than_a_soft_one() {
    let level = |v: f32| {
        let x = render(project(vec![note(0.0, 4.0, 60, v)], MusicBoxState::default()));
        rms(&highpass(&x[1000..40000], 1200.0))
    };
    // the overtone stack speaks far more on a hard pluck
    assert!(level(1.0) > level(0.15) * 1.8, "soft {} vs hard {}", level(0.15), level(1.0));
    // and the whole note is louder
    let loud = rms(&render(project(vec![note(0.0, 4.0, 60, 1.0)], MusicBoxState::default()))[1000..40000]);
    let quiet = rms(&render(project(vec![note(0.0, 4.0, 60, 0.2)], MusicBoxState::default()))[1000..40000]);
    assert!(loud > quiet * 1.5, "loud {loud:.4} vs quiet {quiet:.4}");
}

#[test]
fn the_overtone_dies_before_the_fundamental() {
    // a long pattern and a long gate, so the note decays naturally inside it; 24 beats = 12 s
    let mut p = project(vec![note(0.0, 64.0, 60, 0.9)], MusicBoxState::default());
    p.patterns[0].len = 24.0;
    p.patterns[0].min_len = 24.0;
    p.clips[0].len = 24.0;
    let x = render_to(p, 24.0);
    let high = |from: usize, to: usize| rms(&highpass(&x[from..to], 1200.0));
    let low = |from: usize, to: usize| rms(&lowpass(&x[from..to], 800.0));
    // the sparkle is an attack feature; the fundamental keeps ringing seconds later
    assert!(high(1000, 5000) > high(100_000, 200_000) * 6.0, "overtones must die first");
    assert!(
        low(100_000, 200_000) > 4e-4,
        "the fundamental must keep ringing (got {} vs attack {})",
        low(100_000, 200_000),
        low(1000, 5000)
    );
}

/// Crude one-pole lowpass at roughly `hz`, for measuring the fundamental.
fn lowpass(x: &[f32], hz: f64) -> Vec<f32> {
    let a = 1.0 - (-std::f64::consts::TAU * hz / SR as f64).exp();
    let mut y = 0.0;
    x.iter()
        .map(|&s| {
            y += a * (s as f64 - y);
            y as f32
        })
        .collect()
}

#[test]
fn a_note_rings_for_its_length_then_fades_over_the_release() {
    // a one-step note (5625 frames), then the release fade
    let short = render(project(vec![note(0.0, 1.0, 60, 0.9)], MusicBoxState { release: 40.0, ..Default::default() }));
    let long = render(project(vec![note(0.0, 1.0, 60, 0.9)], MusicBoxState { release: 2000.0, ..Default::default() }));
    assert!(rms(&short[1000..5000]) > 0.02, "the note sounds while held");
    // 100 ms after the key is let go: gone with a 40 ms release, still ringing with 2 s
    let after = 5625 + 4800..5625 + 9600;
    assert!(rms(&short[after.clone()]) < 5e-4);
    assert!(rms(&long[after]) > 0.004, "a 2 s release must leave the note ringing");
    // and a release never ends in a step (no click): the house spike/step detector,
    // which passes a bright but continuous attack
    assert!(lab::click_flags(&short, SR as f32, 3.0).is_empty(), "the release clicks");
}

#[test]
fn settings_are_validated_saved_and_undone() {
    let mut p = project(vec![], MusicBoxState::default());
    let ch = p.channels.iter().find(|c| c.id == CH).unwrap().clone();
    let bad = |ring, release| Channel {
        src: ChannelSrc::MusicBox { music_box: MusicBoxState { ring, release, ..Default::default() } },
        ..ch.clone()
    };
    assert!(ops::apply(&mut p, &Op::PutChannel { channel: bad(999.0, 400.0), index: None }).is_err());
    assert!(ops::apply(&mut p, &Op::PutChannel { channel: bad(100.0, 9999.0), index: None }).is_err());

    let e = Engine::new(p, Arc::new(NoPlugins));
    let good =
        Channel { src: ChannelSrc::MusicBox { music_box: MusicBoxState { ring: 180.0, brightness: 60.0, ..Default::default() } }, ..ch };
    e.apply(&[Op::PutChannel { channel: good.clone(), index: None }], false, None).unwrap();
    assert_eq!(e.project().channel(CH).unwrap().src, good.src);

    // JSON: {"kind":"musicbox","musicBox":{...}}, and a bare {"kind":"musicbox"} loads with the defaults
    let v = serde_json::to_value(e.project().channel(CH).unwrap()).unwrap();
    assert_eq!(v["kind"], "musicbox");
    assert_eq!(v["musicBox"]["ring"], 180.0);
    let mut bare = v.clone();
    bare.as_object_mut().unwrap().remove("musicBox");
    let c: Channel = serde_json::from_value(bare).unwrap();
    assert_eq!(c.src, ChannelSrc::MusicBox { music_box: MusicBoxState::default() });

    e.undo();
    assert_eq!(e.project().channel(CH).unwrap().src, ChannelSrc::MusicBox { music_box: MusicBoxState::default() });
}

#[test]
fn a_chord_is_the_plain_sum_of_its_notes() {
    let keys: Vec<u8> = (58..66).collect();
    let chord = render(project(keys.iter().map(|&k| note(0.0, 8.0, k, 1.0)).collect(), MusicBoxState::default()));
    let mut sum = vec![0f32; chord.len()];
    for &k in &keys {
        for (s, x) in sum.iter_mut().zip(render(project(vec![note(0.0, 8.0, k, 1.0)], MusicBoxState::default()))) {
            *s += x;
        }
    }
    let err = chord.iter().zip(&sum).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
    // f32 addition order differs between the chord and the summed singles
    assert!(err < 1e-3, "chord differs from the sum of its notes by {err}");
}

#[test]
fn a_loud_note_leaves_headroom_for_chords() {
    let x = render(project(vec![note(0.0, 8.0, 60, 1.0)], MusicBoxState::default()));
    let pk = x.iter().fold(0f32, |a, b| a.max(b.abs()));
    let db = 20.0 * pk.log10();
    assert!(db < -6.0 && db > -12.0, "loud note peak {db:.1} dBFS");
    // and it is deterministic: the same project renders sample-identically
    let again = render(project(vec![note(0.0, 8.0, 60, 1.0)], MusicBoxState::default()));
    assert_eq!(x, again);
}

#[test]
fn overlapping_notes_ringing_into_each_other_do_not_click() {
    // 20 notes ring into each other (retrigger damping, overlapping releases); the pool has
    // room for them all (96 voices), so any click would be our release or damping math
    let mut notes = vec![];
    for n in 0..20u32 {
        let step = n as f32 * 0.15;
        notes.push(note(step, 2.0, 60 + (n % 12) as u8, 0.9));
    }
    let x = render(project(notes, MusicBoxState::default()));
    assert!(lab::click_flags(&x, SR as f32, 3.0).is_empty(), "overlapping notes click");
    assert!(rms(&x) > 0.01);
}

#[test]
fn a_slide_note_glides_the_tine_to_its_new_key() {
    // C4 rings from 0; a slide note to C5 starts at step 4 and glides over its 4 steps
    // (each step is 0.117 s at 128 BPM): 0.469 s .. 0.938 s
    let mut p =
        project(vec![note(0.0, 32.0, 60, 0.9), Note { slide: true, ..Note::new(CH, 4.0, 4.0, Some(72)) }], MusicBoxState::default());
    p.patterns[0].len = 24.0;
    p.patterns[0].min_len = 24.0;
    p.clips[0].len = 24.0;
    let x = render_to(p, 24.0);
    let sr = SR as f64;
    // measure after the attack: the early overtone stack inflates zero crossings
    let before = freq(&x[(0.28 * sr) as usize..(0.44 * sr) as usize]);
    assert!((before - 261.63).abs() < 15.0, "before the slide: {before:.1} Hz");
    // half way through the glide the tine sits between its old and new pitch
    let mid = freq(&x[(0.66 * sr) as usize..(0.74 * sr) as usize]);
    assert!(mid > 300.0 && mid < 460.0, "mid-glide should sit between C4 and C5: {mid:.1} Hz");
    let after = freq(&x[(1.10 * sr) as usize..(1.40 * sr) as usize]);
    assert!((after - 523.25).abs() < 14.0, "after the glide: {after:.1} Hz");
}

#[test]
fn the_body_resonance_stays_bounded_even_on_its_own_pitch() {
    // F#3 = 185 Hz sits exactly on the first body mode: a sustained note there must pass the
    // resonator at its own amplitude (a bare (1 − r) feed amplified it ~20x before the gain)
    let loud = project(vec![note(0.0, 64.0, 54, 1.0)], MusicBoxState { body: 100.0, ..Default::default() });
    let pk = render(loud).iter().fold(0f32, |a, b| a.max(b.abs()));
    assert!(pk < 0.8, "F#3 with Body 100 % peaks at {pk:.2}");
    // E4 = 329.6 Hz sits on the second mode (330 Hz): bounded too
    let loud2 = project(vec![note(0.0, 64.0, 64, 1.0)], MusicBoxState { body: 100.0, ..Default::default() });
    let pk2 = render(loud2).iter().fold(0f32, |a, b| a.max(b.abs()));
    assert!(pk2 < 0.8, "E4 with Body 100 % peaks at {pk2:.2}");
    // and Body 0 stays the dry tine, also bounded
    let dry = render(project(vec![note(0.0, 64.0, 54, 1.0)], MusicBoxState { body: 0.0, ..Default::default() }));
    let dpk = dry.iter().fold(0f32, |a, b| a.max(b.abs()));
    assert!(dpk < 0.8, "dry tine peaks at {dpk:.2}");
}

#[test]
fn body_zero_is_the_dry_reference_and_the_modes_add_energy_in_their_bands() {
    // crude bandpass around the 185 Hz mode: lowpass 300 minus lowpass 120
    let band = |x: &[f32]| {
        let hi = rms(&lowpass(&lowpass(x, 300.0), 300.0));
        let lo = rms(&lowpass(&lowpass(x, 120.0), 120.0));
        (hi * hi - lo * lo).max(0.0).sqrt()
    };
    let on = render_to(project(vec![note(0.0, 64.0, 54, 1.0)], MusicBoxState { body: 100.0, ..Default::default() }), 24.0);
    let off = render_to(project(vec![note(0.0, 64.0, 54, 1.0)], MusicBoxState { body: 0.0, ..Default::default() }), 24.0);
    let window = 2000..200_000;
    let on_band = band(&on[window.clone()]);
    let off_band = band(&off[window.clone()]);
    // Body 0 is the dry reference; the modes must add real energy in their own band
    assert!(on_band > off_band * 1.15, "185 Hz band: body {on_band:.5} vs dry {off_band:.5}");
    // the body's own added signal (on − off) must sit in the mode bands, not above them: the
    // resonators ring at 185/330 Hz, so anything the difference carries over 1.5 kHz is the
    // resonator leaking broadband input through
    let diff: Vec<f32> = on.iter().zip(&off).map(|(a, b)| a - b).collect();
    let low_part = rms(&lowpass(&lowpass(&diff, 500.0), 500.0));
    let high_part = rms(&highpass(&diff, 1500.0));
    assert!(high_part < low_part * 0.15, "the body leaks highs: {high_part:.5} vs mode-band {low_part:.5}");
}

#[test]
fn a_zero_velocity_note_is_silent() {
    let x = render(project(vec![note(0.0, 8.0, 60, 0.0)], MusicBoxState::default()));
    assert!(x.iter().all(|v| v.abs() < 1e-9), "a zero-velocity note made sound");
}

#[test]
fn a_manual_knob_edit_survives_an_automation_without_clips() {
    // an envelope that no clip drives owns nothing: after editing ring to 250 the render must
    // decay like ring 250 (the old carry reinstated the stale runtime value on every rebuild)
    let base = |ring: f32| {
        let mut p = project(vec![note(0.0, 64.0, 60, 0.9)], MusicBoxState { ring, ..Default::default() });
        p.patterns[0].len = 16.0;
        p.patterns[0].min_len = 16.0;
        p.clips[0].len = 16.0;
        p
    };
    let auto = Automation {
        id: 500,
        name: "R".into(),
        color: "#fff".into(),
        target: AutoTarget::MusicBox { chan_id: CH, param: "ring".into() },
        points: vec![AutoPoint::at(0.0, 0.5)],
    };
    let e = Engine::new(base(100.0), Arc::new(NoPlugins));
    e.apply(&[Op::PutAutomation { automation: auto }], false, None).unwrap();
    let ch = e.project().channel(CH).unwrap().clone();
    let edited = Channel { src: ChannelSrc::MusicBox { music_box: MusicBoxState { ring: 250.0, ..Default::default() } }, ..ch };
    e.apply(&[Op::PutChannel { channel: edited, index: None }], false, None).unwrap();
    let edited_rms = rms(&render_to(e.project().clone(), 24.0)[150_000..250_000]);

    // control: the same undriven automation without the edit decays like ring 100
    let control = render_to(base(100.0), 24.0);
    let control_rms = rms(&control[150_000..250_000]);
    // and plain ring 250 without any automation decays the same as the edited one
    let plain = render_to(base(250.0), 24.0);
    let plain_rms = rms(&plain[150_000..250_000]);

    assert!(edited_rms > control_rms * 2.0, "the edit was ignored: {edited_rms:.5} vs control {control_rms:.5}");
    assert!((edited_rms - plain_rms).abs() < 1e-4, "the edit changed the sound: {edited_rms:.5} vs plain {plain_rms:.5}");
}
