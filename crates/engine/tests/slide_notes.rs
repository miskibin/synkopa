//! Slide notes (FL Studio's): a slide note starts no sound; it glides the notes of its channel
//! that are sounding by the distance from the highest of them to its key, over its length.

use std::path::PathBuf;
use std::sync::Arc;

use wave_engine::poly::{lab, params, SynthInst};
use wave_engine::synth::Dest;
use wave_engine::*;

const SR: u32 = 48000;

/// One second of a 440 Hz sine at 0.5: a sampler channel's sample (root C4 = 60 plays 440 Hz).
fn sine_wav(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wave-slide-{}-{}", std::process::id(), name));
    std::fs::create_dir_all(&d).unwrap();
    let path = d.join("sine.wav");
    let spec = hound::WavSpec { channels: 1, sample_rate: SR, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(&path, spec).unwrap();
    for i in 0..SR {
        w.write_sample((0.5 * (std::f64::consts::TAU * 440.0 * i as f64 / SR as f64).sin()) as f32).unwrap();
    }
    w.finalize().unwrap();
    path
}

/// (step, length in steps, key, slide)
type N = (f32, f32, u8, bool);

/// An engine at 120 BPM (a step is 1/8 s) with a sampler channel playing `notes` from a
/// two-bar pattern clip, and a second sampler channel playing `other` alongside.
fn engine(name: &str, notes: &[N], other: &[N]) -> Arc<Engine> {
    let mut p = Project::empty();
    p.bpm = 120.0;
    p.inserts[0].fx.clear();
    let e = Engine::new(p, Arc::new(NoPlugins));
    let (srcs, ..) = e.import_audio(&[sine_wav(name)], None).unwrap();
    let mut q = e.project();
    let (a, b) = (q.alloc_id(), q.alloc_id());
    let chan = |id, name: &str| {
        Channel::new(id, name, "#4f8ef7", ChannelSrc::Sample { source: srcs[0], sampler: SamplerState::default(), slices: vec![] })
    };
    let note = |ch: Id, &(s, l, k, slide): &N| Note { slide, ..Note::new(ch, s, l, Some(k)) };
    let mut all: Vec<Note> = notes.iter().map(|n| note(a, n)).collect();
    all.extend(other.iter().map(|n| note(b, n)));
    let pattern = Pattern {
        id: 1,
        name: "P".into(),
        color: "#4f8ef7".into(),
        len: 8.0,
        min_len: 8.0,
        notes: all,
        swing: 0.0,
        channel_loops: std::collections::BTreeMap::new(),
    };
    let clip = Clip {
        warp: Vec::new(),
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
    e.apply(
        &[
            Op::PutChannel { channel: chan(a, "A"), index: None },
            Op::PutChannel { channel: chan(b, "B"), index: None },
            Op::PutPattern { pattern },
            Op::PutClips { clips: vec![clip] },
        ],
        false,
        None,
    )
    .unwrap();
    e
}

fn render(e: &Engine, beats: f64) -> Vec<f32> {
    e.render(0.0, beats, 0.0, SR).unwrap().as_chunks::<2>().0.iter().map(|f| f[0]).collect()
}

/// Frequency of `x` between two times (seconds), from the period of its autocorrelation peak.
fn hz(x: &[f32], a: f64, b: f64) -> f64 {
    let x = &x[(a * SR as f64) as usize..(b * SR as f64) as usize];
    let ac = |lag: usize| x.iter().zip(&x[lag..]).map(|(p, q)| (p * q) as f64).sum::<f64>() / (x.len() - lag) as f64;
    // 60 Hz .. 2 kHz; the first lag that is a local maximum above half the energy
    let e0 = ac(0);
    let (lo, hi) = (SR as usize / 2000, SR as usize / 60);
    let lag = (lo..hi).find(|&l| ac(l) > 0.5 * e0 && ac(l) >= ac(l - 1) && ac(l) >= ac(l + 1)).expect("no period");
    // parabolic interpolation around the peak
    let (y0, y1, y2) = (ac(lag - 1), ac(lag), ac(lag + 1));
    let d = 0.5 * (y0 - y2) / (y0 - 2.0 * y1 + y2);
    SR as f64 / (lag as f64 + d)
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0f32, |m, v| m.max(v.abs()))
}

fn near(got: f64, want: f64) -> bool {
    (got / want).log2().abs() * 1200.0 < 15.0
}

#[test]
fn a_slide_note_glides_the_sounding_note_to_its_key_over_its_length() {
    // C4 from 0 s; a slide to C5 at 0.25 s lasting 0.25 s
    let out = render(&engine("up", &[(0.0, 16.0, 60, false), (2.0, 2.0, 72, true)], &[]), 2.0);
    assert!(near(hz(&out, 0.05, 0.24), 440.0), "{}", hz(&out, 0.05, 0.24));
    // half way through the glide: half way in pitch (linear in semitones)
    let mid = hz(&out, 0.37, 0.38);
    assert!(near(mid, 440.0 * 2f64.powf(0.5)), "{mid}");
    assert!(near(hz(&out, 0.52, 0.66), 880.0), "{}", hz(&out, 0.52, 0.66));
}

#[test]
fn a_slide_note_alone_is_silent_and_one_after_the_note_ended_does_nothing() {
    let out = render(&engine("alone", &[(0.0, 4.0, 72, true)], &[]), 1.0);
    assert_eq!(peak(&out), 0.0);
    // the note ends at 0.25 s; a slide at 0.375 s finds nothing to bend, and starts no note
    let out = render(&engine("late", &[(0.0, 2.0, 60, false), (3.0, 2.0, 72, true)], &[]), 1.0);
    assert!(peak(&out[(0.3 * SR as f64) as usize..]) < 1e-3);
}

#[test]
fn a_chord_slides_together_by_the_distance_from_its_top_note() {
    // C4 + G4 (top); a slide to A4 lifts both by two semitones: D4 + A4
    let out = render(&engine("chord", &[(0.0, 16.0, 67, false), (2.0, 0.0001, 69, true)], &[]), 1.0);
    assert!(near(hz(&out, 0.3, 0.5), 440.0 * 2f64.powf(9.0 / 12.0)), "{}", hz(&out, 0.3, 0.5));
    let out = render(&engine("chord2", &[(0.0, 16.0, 60, false), (0.0, 16.0, 67, false), (2.0, 0.0001, 69, true)], &[]), 1.0);
    // the sum of D4 and A4 (a fifth) repeats at D3's period
    assert!(near(hz(&out, 0.3, 0.5), 440.0 * 2f64.powf(2.0 / 12.0) / 2.0), "{}", hz(&out, 0.3, 0.5));
}

#[test]
fn a_slide_on_one_channel_leaves_the_other_alone() {
    let out = render(&engine("other", &[(2.0, 0.0001, 72, true)], &[(0.0, 16.0, 60, false)]), 1.0);
    assert!(near(hz(&out, 0.3, 0.5), 440.0), "{}", hz(&out, 0.3, 0.5));
}

#[test]
fn a_slide_note_starting_with_its_note_bends_it() {
    // same step: the note starts, then the slide moves it (order inside the pattern does not matter)
    for notes in [[(0.0, 16.0, 60, false), (0.0, 2.0, 67, true)], [(0.0, 2.0, 67, true), (0.0, 16.0, 60, false)]] {
        let out = render(&engine("same", &notes, &[]), 1.0);
        assert!(near(hz(&out, 0.3, 0.5), 440.0 * 2f64.powf(7.0 / 12.0)), "{}", hz(&out, 0.3, 0.5));
    }
}

#[test]
fn the_slide_flag_round_trips_and_is_left_out_when_off() {
    let n = Note { slide: true, ..Note::new(1, 0.0, 2.0, Some(60)) };
    let j = serde_json::to_value(&n).unwrap();
    assert_eq!(j["slide"], true);
    assert_eq!(serde_json::from_value::<Note>(j).unwrap(), n);
    let j = serde_json::to_value(Note::new(1, 0.0, 2.0, Some(60))).unwrap();
    assert!(j.get("slide").is_none());
    assert!(!serde_json::from_value::<Note>(serde_json::json!({"ch": 1, "step": 0.0, "len": 1.0})).unwrap().slide);
}

#[test]
fn a_synth_slides_its_sounding_note() {
    let sr = SR as f32;
    let mut s = SynthInst::new(1, params::defaults(), sr);
    s.note(0, (1.0 * sr) as u32, 57, 0.8, Dest::Click);
    s.slide((0.25 * sr) as u32, 69, (0.25 * sr) as u32);
    let (l, _) = lab::run(&mut s, 1.0, sr);
    assert!(near(hz(&l, 0.1, 0.24), 220.0), "{}", hz(&l, 0.1, 0.24));
    assert!(near(hz(&l, 0.55, 0.9), 440.0), "{}", hz(&l, 0.55, 0.9));
    // nothing sounding: a slide is ignored and starts nothing
    let mut s = SynthInst::new(1, params::defaults(), sr);
    s.slide(0, 69, 100);
    let (l, _) = lab::run(&mut s, 0.3, sr);
    assert_eq!(peak(&l), 0.0);
}

#[test]
fn two_slides_in_one_block_end_on_the_later_ones_key_whatever_their_order_in_the_pattern() {
    // C4, then a slide up to C5 and, 120 samples later (the same audio block), one back to C4,
    // stored later-first: the later slide must win
    let out = render(&engine("pair", &[(0.0, 16.0, 60, false), (2.02, 0.5, 60, true), (2.0, 0.02, 72, true)], &[]), 2.0);
    assert!(near(hz(&out, 0.45, 0.6), 440.0), "{}", hz(&out, 0.45, 0.6));
}
