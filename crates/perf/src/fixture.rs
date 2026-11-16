//! Deterministic synthetic projects of several sizes, with real WAV files, for benchmarking.
//!
//! `build` writes the audio into `<dir>/audio` and returns the [`Project`]; `generate`
//! also saves it as a `.wave` file (relative audio paths, like a real song) plus a plain
//! `project.json` that the UI mock backend can load.

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use wave_engine::catalog::{new_fx, params};
use wave_engine::sources::write_wav;
use wave_engine::*;

pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.max(1) * 0x9E37_79B9_7F4A_7C15)
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    /// 0..1
    pub fn f(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.f()
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
}

#[derive(Clone, Debug)]
pub struct Spec {
    pub name: &'static str,
    pub tracks: usize,
    pub audio_clips: usize,
    pub pat_clips: usize,
    pub patterns: usize,
    pub notes: usize,
    pub sources: usize,
    pub src_secs: (f64, f64),
    pub beats: f64,
    pub bpm: f64,
    pub automations: usize,
    pub auto_points: usize,
    pub comps: usize,
    pub tuned_clips: usize,
    /// Tracks with the heavy vocal chain (denoise, tune, voice, ...).
    pub vocal_tracks: usize,
}

pub fn specs() -> Vec<Spec> {
    vec![
        Spec {
            name: "small",
            tracks: 8,
            audio_clips: 60,
            pat_clips: 12,
            patterns: 6,
            notes: 64,
            sources: 8,
            src_secs: (4.0, 12.0),
            beats: 256.0,
            bpm: 128.0,
            automations: 4,
            auto_points: 40,
            comps: 1,
            tuned_clips: 1,
            vocal_tracks: 1,
        },
        Spec {
            name: "medium",
            tracks: 32,
            audio_clips: 420,
            pat_clips: 60,
            patterns: 14,
            notes: 160,
            sources: 20,
            src_secs: (5.0, 18.0),
            beats: 384.0,
            bpm: 90.0,
            automations: 16,
            auto_points: 100,
            comps: 2,
            tuned_clips: 2,
            vocal_tracks: 3,
        },
        // 512 beats at 52 bpm is a 9.8 minute song (the arrangement limit, TOTAL_BEATS, is 2048)
        Spec {
            name: "large",
            tracks: 64,
            audio_clips: 1200,
            pat_clips: 160,
            patterns: 24,
            notes: 256,
            sources: 40,
            src_secs: (6.0, 24.0),
            beats: 512.0,
            bpm: 52.0,
            automations: 40,
            auto_points: 200,
            comps: 3,
            tuned_clips: 4,
            vocal_tracks: 6,
        },
        Spec {
            name: "huge",
            tracks: 128,
            audio_clips: 3000,
            pat_clips: 400,
            patterns: 48,
            notes: 400,
            sources: 80,
            src_secs: (6.0, 24.0),
            beats: 512.0,
            bpm: 52.0,
            automations: 100,
            auto_points: 300,
            comps: 4,
            tuned_clips: 8,
            vocal_tracks: 12,
        },
    ]
}

pub fn spec(name: &str) -> Option<Spec> {
    if name == "song" {
        // `build` makes this one with `build_song`; the counts here only describe it
        return Some(Spec {
            name: "song",
            tracks: SONG_TRACKS,
            audio_clips: 24,
            pat_clips: 420,
            patterns: 82,
            notes: 0,
            sources: 40,
            src_secs: (0.2, 8.0),
            beats: 512.0,
            bpm: 128.0,
            automations: 8,
            auto_points: 64,
            comps: 0,
            tuned_clips: 0,
            vocal_tracks: 0,
        });
    }
    specs().into_iter().find(|s| s.name == name)
}

const SONG_TRACKS: usize = 40;

/// A produced song at the scale of FL Studio 20's demo "WiseLabs - Nahash" (89 channels,
/// 82 patterns, ~5,500 notes, ~440 playlist items, ~120 mixer effects, native instruments
/// only), so a WAVE-vs-FL comparison opens comparable work: 14 synth channels (the built-in
/// synth, like the demo's Harmor/Sytrus parts), 75 sampler channels (drum and FX one-shots),
/// 40 mixer inserts with 2-4 effects each, 4 minutes at 128 BPM.
pub fn build_song(dir: &Path) -> Project {
    let mut rng = Rng::new(4242);
    let audio_dir = dir.join("audio");
    std::fs::create_dir_all(&audio_dir).unwrap();
    let mut p = Project::routed();
    p.bpm = 128.0;
    p.tracks.clear();
    p.inserts.truncate(1);
    // no starter kit: only the song's own channels, patterns and sounds
    p.channels.clear();
    p.patterns.clear();
    p.sources.clear();
    p.clips.clear();
    let spb = 60.0 / p.bpm;
    let beats = 512.0;

    // ---- sources: 36 one-shots (drums, hits, risers) and 4 loops for audio clips
    for i in 0..40 {
        let (st, secs) = if i < 36 { (2, rng.range(0.2, 1.6)) } else { (1, 8.0) };
        let sr = if i % 4 == 3 { 44100 } else { 48000 };
        let path = audio_dir.join(format!("song{i:03}.wav"));
        if !path.exists() {
            write_wav(&path, &synth(st, secs, sr, 900 + i as u64), sr).unwrap();
        }
        let id = p.alloc_id();
        p.sources.push(Source {
            id,
            name: if i < 36 { format!("Hit {i}") } else { format!("Loop {i}") },
            color: color(i),
            path: path.display().to_string(),
            dur: secs,
            sr,
            ch: 2,
            peak_db: -6.0,
        });
    }

    // ---- mixer: 40 inserts, the master with EQ, comp and limiter
    for fx in [FxKind::Comp, FxKind::Eq] {
        let id = p.alloc_id();
        p.inserts[0].fx.insert(0, new_fx(id, fx, p.bpm));
    }
    const CHAINS: [&[FxKind]; 6] = [
        &[FxKind::Eq, FxKind::Comp],
        &[FxKind::Eq, FxKind::Sat, FxKind::Comp],
        &[FxKind::Eq, FxKind::Delay, FxKind::Reverb],
        &[FxKind::Eq, FxKind::Comp, FxKind::Limit],
        &[FxKind::Eq, FxKind::Chorus, FxKind::Reverb],
        &[FxKind::Eq, FxKind::Comp, FxKind::Sat, FxKind::Delay],
    ];
    let mut inserts = vec![];
    for t in 0..SONG_TRACKS {
        let ins_id = p.alloc_id();
        let fx = CHAINS[t % CHAINS.len()].iter().map(|k| new_fx(p.alloc_id(), *k, p.bpm)).collect();
        p.inserts.push(Insert {
            id: ins_id,
            name: format!("Bus {t}"),
            color: color(t),
            vol: VOL_UNITY,
            pan: ((t % 7) as f32 - 3.0) / 8.0,
            width: 1.0,
            muted: false,
            solo: false,
            fx,
            sends: vec![],
            master: true,
        });
        inserts.push(ins_id);
        let tid = p.alloc_id();
        p.tracks.push(Track {
            id: tid,
            name: format!("Track {t}"),
            color: color(t),
            muted: false,
            solo: false,
            arm: false,
            vol: VOL_UNITY,
            h: 56.0,
            insert: ins_id,
            icon: None,
        });
    }

    // ---- channels: 14 synths, 75 samplers, each routed to an insert like a produced song
    let cat = wave_engine::poly::catalog();
    let pick = |c: &str| cat.presets.iter().filter(|x| x.category == c).map(|x| x.name).collect::<Vec<_>>();
    let mut synth_presets: Vec<&str> = ["Bas", "Pad", "Lead", "Pluck", "Klawisze"].iter().flat_map(|c| pick(c)).collect();
    if synth_presets.is_empty() {
        synth_presets = cat.presets.iter().map(|x| x.name).collect();
    }
    let mut synths = vec![];
    for k in 0..14 {
        let id = p.alloc_id();
        let name = synth_presets[(k * 7) % synth_presets.len()];
        p.channels.push(Channel {
            id,
            name: name.to_string(),
            color: color(k),
            src: ChannelSrc::Synth { synth: wave_engine::poly::preset_state(name).unwrap_or_default() },
            vol: VOL_UNITY * 0.8,
            pan: 0.0,
            mute: false,
            solo: false,
            insert: Some(inserts[k % SONG_TRACKS]),
            root: SAMPLE_ROOT,
        });
        synths.push(id);
    }
    let mut samplers = vec![];
    for k in 0..75 {
        let id = p.alloc_id();
        let sid = p.sources[k % 36].id;
        p.channels.push(Channel {
            id,
            name: format!("Hit {k}"),
            color: color(k + 3),
            src: ChannelSrc::Sample { source: sid, sampler: Default::default() },
            vol: VOL_UNITY,
            pan: 0.0,
            mute: false,
            solo: false,
            insert: Some(inserts[14 + k % (SONG_TRACKS - 14)]),
            root: SAMPLE_ROOT,
        });
        samplers.push(id);
    }

    // ---- 82 patterns: 52 drum/FX patterns (steps), 30 synth parts (chords, bass, melodies)
    let mut drum_pats = vec![];
    let mut synth_pats = vec![];
    for i in 0..82 {
        let id = p.alloc_id();
        let mut notes = vec![];
        if i < 52 {
            // 4 bars of 16ths: 4-6 instruments, ~45 hits
            let n_ch = 4 + rng.below(3);
            let base = rng.below(samplers.len());
            for c in 0..n_ch {
                let ch = samplers[(base + c * 11) % samplers.len()];
                let every = [4, 8, 2, 16, 4, 8][c % 6];
                let mut s = (c % 2) * (every / 2).max(1);
                while s < 64 {
                    let mut n = Note::trigger(ch, s as f32, None);
                    n.v = 0.6 + 0.4 * rng.f() as f32;
                    notes.push(n);
                    s += every;
                }
            }
            drum_pats.push(id);
        } else {
            // 8 bars: chords (3 voices), bass or a melody, ~95 notes
            let ch = synths[(i - 52) % synths.len()];
            let root = 36 + rng.below(24) as u8;
            let chords = i % 3 == 0;
            let mut s = 0.0;
            while s < 128.0 {
                let len = if chords { 8.0 } else { [1.0, 2.0, 2.0, 4.0][rng.below(4)] };
                let deg = [0u8, 3, 5, 7, 10, 12][rng.below(6)];
                if chords {
                    for v in [0u8, 3, 7] {
                        notes.push(Note::new(ch, s, len, Some(root + deg + v)));
                    }
                } else {
                    notes.push(Note::new(ch, s, len, Some(root + deg)));
                }
                s += len;
            }
            synth_pats.push(id);
        }
        notes.sort_by(|a, b| a.step.total_cmp(&b.step));
        let len = if i < 52 { 16.0 } else { 32.0 };
        p.patterns.push(Pattern { id, name: format!("Pattern {i}"), color: color(i), len, min_len: len, notes, swing: 0.0 });
    }

    // ---- playlist: ~420 pattern clips in sections, 24 audio loop clips
    let cid = |p: &mut Project| p.alloc_id();
    let sections = [(0.0, 64.0, 6), (64.0, 192.0, 18), (192.0, 256.0, 8), (256.0, 448.0, 22), (448.0, 512.0, 6)];
    let mut placed = 0;
    for (a, b, layers) in sections {
        for l in 0..layers {
            let (pat, plen) =
                if l % 3 == 2 { (synth_pats[rng.below(synth_pats.len())], 32.0) } else { (drum_pats[rng.below(drum_pats.len())], 16.0) };
            let track = (l * 3 + placed) % SONG_TRACKS;
            let mut s = a;
            while s + plen <= b && placed < 420 {
                let id = cid(&mut p);
                p.clips.push(new_clip(id, track as u32, s, plen, ClipKind::Pat, pat));
                s += plen;
                placed += 1;
            }
        }
    }
    for i in 0..24 {
        let src = &p.sources[36 + i % 4];
        let len = src.dur / spb;
        let start = 64.0 + (i as f64 * 16.0) % (beats - 64.0 - len);
        let sid = src.id;
        let id = cid(&mut p);
        p.clips.push(new_clip(id, (SONG_TRACKS - 1 - i % 4) as u32, start, len, ClipKind::Audio, sid));
    }

    // ---- automation: filter sweeps and fades
    for i in 0..8 {
        let id = p.alloc_id();
        let ins = &p.inserts[1 + (i * 5) % SONG_TRACKS];
        let target = AutoTarget::InsertVol { id: ins.id };
        let points: Vec<AutoPoint> = (0..64)
            .map(|k| AutoPoint::at(64.0 * k as f64 / 63.0, (0.5 + 0.4 * (k as f32 * 0.2 + i as f32).sin()).clamp(0.0, 1.0)))
            .collect();
        p.automations.push(Automation { id, name: format!("Sweep {i}"), color: color(i), target, points });
        let c = cid(&mut p);
        p.clips.push(new_clip(c, ((i * 5) % SONG_TRACKS) as u32, 64.0 * i as f64, 64.0, ClipKind::Auto, id));
    }
    p.loop_range = Some(LoopRange { a: 256.0, b: 320.0 });
    p
}

/// Style of source `i`: 0 = voice-like, 1 = pad, 2 = drum loop.
pub fn style(i: usize) -> usize {
    i % 3
}

fn synth(style: usize, secs: f64, sr: u32, seed: u64) -> Vec<f32> {
    let mut rng = Rng::new(seed);
    let n = (secs * sr as f64) as usize;
    let mut out = vec![0f32; n * 2];
    let srf = sr as f32;
    match style {
        0 => {
            // a sung phrase: notes glide, vibrato, formants, breath noise, silences between phrases
            let mut i = 0;
            let mut phase = 0f32;
            while i < n {
                let len = (rng.range(0.25, 0.9) as f32 * srf) as usize;
                let f0 = 130.0 * 2f32.powf(rng.below(14) as f32 / 12.0);
                let rest = rng.f() < 0.18;
                for k in 0..len.min(n - i) {
                    let t = k as f32 / srf;
                    let env = if rest { 0.0 } else { (t * 40.0).min(1.0) * (((len - k) as f32 / srf) * 30.0).min(1.0) };
                    let vib = 1.0 + 0.012 * (TAU * 5.5 * t).sin() * (t * 3.0).min(1.0);
                    phase += TAU * f0 * vib / srf;
                    if phase > TAU {
                        phase -= TAU;
                    }
                    let mut s = 0.0;
                    for h in 1..=14 {
                        let hf = f0 * h as f32;
                        let form = (-((hf - 700.0) / 500.0).powi(2)).exp() + 0.6 * (-((hf - 2400.0) / 700.0).powi(2)).exp();
                        s += (phase * h as f32).sin() * form / h as f32;
                    }
                    let noise = (rng.f() as f32 - 0.5) * 0.02;
                    let v = (s * 0.35 + noise) * env * 0.8 + (rng.f() as f32 - 0.5) * 0.004;
                    out[(i + k) * 2] = v;
                    out[(i + k) * 2 + 1] = v * 0.97;
                }
                i += len;
            }
        }
        1 => {
            let notes = [0.0, 3.0, 7.0, 10.0];
            for (c, o) in notes.iter().enumerate() {
                let f = 110.0 * 2f32.powf((*o + (seed % 5) as f32) / 12.0);
                for i in 0..n {
                    let t = i as f32 / srf;
                    let saw = ((t * f * (1.0 + c as f32 * 0.003)) % 1.0) * 2.0 - 1.0;
                    let env = (t * 2.0).min(1.0) * ((secs as f32 - t) * 2.0).clamp(0.0, 1.0);
                    let v = saw * 0.12 * env;
                    out[i * 2] += v * (1.0 - c as f32 * 0.2);
                    out[i * 2 + 1] += v * (0.6 + c as f32 * 0.2);
                }
            }
        }
        _ => {
            let beat = (srf * 60.0 / 120.0) as usize;
            for b in 0..(n / beat + 1) {
                let at = b * beat;
                for k in 0..(beat / 2).min(n.saturating_sub(at)) {
                    let t = k as f32 / srf;
                    let kick = (TAU * (60.0 + 90.0 * (-t * 30.0).exp()) * t).sin() * (-t * 9.0).exp();
                    let hat = if b % 2 == 1 { (rng.f() as f32 - 0.5) * (-t * 40.0).exp() * 0.5 } else { 0.0 };
                    let v = kick * 0.7 + hat;
                    out[(at + k) * 2] += v;
                    out[(at + k) * 2 + 1] += v;
                }
            }
        }
    }
    out
}

fn color(i: usize) -> String {
    ["#4f8ef7", "#5aa9e6", "#69c3d6", "#47b8a8", "#5fbf8a", "#7c8cf8", "#e0a44a", "#9a8cf0", "#d9678f", "#8b93a1"][i % 10].into()
}

const LIGHT: [&[FxKind]; 4] = [
    &[FxKind::Eq, FxKind::Comp],
    &[FxKind::Eq, FxKind::Comp, FxKind::Sat],
    &[FxKind::Eq, FxKind::Comp, FxKind::Delay, FxKind::Reverb],
    &[FxKind::Eq, FxKind::Deess, FxKind::Comp, FxKind::Limit],
];
const VOCAL: &[FxKind] =
    &[FxKind::Denoise, FxKind::Tune, FxKind::Voice, FxKind::Eq, FxKind::Deess, FxKind::Comp, FxKind::Fet, FxKind::Sat, FxKind::Reverb];

fn new_clip(id: Id, track: u32, start: f64, len: f64, kind: ClipKind, source: Id) -> Clip {
    Clip {
        id,
        track,
        start,
        len,
        offset: 0.0,
        kind,
        source,
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
    }
}

/// Builds the project and writes its audio under `<dir>/audio`.
pub fn build(spec: &Spec, dir: &Path) -> Project {
    if spec.name == "song" {
        return build_song(dir);
    }
    let mut rng = Rng::new(7 + spec.tracks as u64);
    let audio_dir = dir.join("audio");
    std::fs::create_dir_all(&audio_dir).unwrap();
    let mut p = Project::routed();
    p.bpm = spec.bpm;
    p.tracks.clear();
    p.inserts.truncate(1); // master
    let spb = 60.0 / spec.bpm;

    // ---- sources
    let mut src_style = vec![];
    for i in 0..spec.sources {
        let st = style(i);
        let secs = rng.range(spec.src_secs.0, spec.src_secs.1);
        let sr = if i % 4 == 3 { 44100 } else { 48000 };
        let path = audio_dir.join(format!("src{i:03}.wav"));
        if !path.exists() {
            write_wav(&path, &synth(st, secs, sr, 100 + i as u64), sr).unwrap();
        }
        let id = p.alloc_id();
        p.sources.push(Source {
            id,
            name: format!("{} {i}", ["Vocal", "Pad", "Drums"][st]),
            color: color(i),
            path: path.display().to_string(),
            dur: secs,
            sr,
            ch: 2,
            peak_db: -6.0,
        });
        src_style.push(st);
    }
    let vocals: Vec<usize> = (0..spec.sources).filter(|i| style(*i) == 0).collect();
    let drums: Vec<usize> = (0..spec.sources).filter(|i| style(*i) == 2).collect();

    // ---- sampler channels
    for (k, &di) in drums.iter().take(3).enumerate() {
        let id = p.alloc_id();
        let sid = p.sources[di].id;
        p.channels.push(Channel {
            id,
            name: format!("Sampler {k}"),
            color: color(k + 3),
            src: ChannelSrc::Sample { source: sid, sampler: Default::default() },
            vol: VOL_UNITY,
            pan: 0.0,
            mute: false,
            solo: false,
            insert: None,
            root: SAMPLE_ROOT,
        });
    }

    // ---- mixer: master + one insert per track
    for fx in [FxKind::Comp, FxKind::Eq] {
        let id = p.alloc_id();
        p.inserts[0].fx.insert(0, new_fx(id, fx, spec.bpm)); // before the master limiter
    }
    for t in 0..spec.tracks {
        let ins_id = p.alloc_id();
        let tid = p.alloc_id();
        let vocal = t < spec.vocal_tracks;
        let kinds: &[FxKind] = if vocal { VOCAL } else { LIGHT[t % 4] };
        let mut fx = vec![];
        for k in kinds {
            let id = p.alloc_id();
            fx.push(new_fx(id, *k, spec.bpm));
        }
        p.inserts.push(Insert {
            id: ins_id,
            name: format!("Ins {t}"),
            color: color(t),
            vol: VOL_UNITY,
            pan: ((t % 7) as f32 - 3.0) / 6.0,
            width: 1.0,
            muted: false,
            solo: false,
            fx,
            sends: vec![],
            master: true,
        });
        p.tracks.push(Track {
            id: tid,
            name: if vocal { format!("Vocal {t}") } else { format!("Track {t}") },
            color: color(t),
            muted: false,
            solo: false,
            arm: false,
            vol: VOL_UNITY * 0.9,
            h: 56.0,
            insert: ins_id,
            icon: None,
        });
    }

    // ---- patterns
    let chans: Vec<Id> = p.channels.iter().map(|c| c.id).collect();
    let pitched: Vec<Id> = p.channels.iter().map(|c| c.id).collect();
    let mut pats = vec![];
    for i in 0..spec.patterns {
        let id = p.alloc_id();
        let len = if i % 3 == 0 { 8.0 } else { 16.0 };
        let steps = (len * 4.0) as usize;
        let mut notes = vec![];
        for _ in 0..spec.notes {
            let ch = chans[rng.below(chans.len())];
            let pitched_ch = pitched.contains(&ch);
            notes.push(Note::new(
                ch,
                rng.below(steps) as f32,
                [1.0, 1.0, 2.0, 4.0][rng.below(4)],
                if pitched_ch { Some(36 + rng.below(36) as u8) } else { None },
            ));
        }
        notes.sort_by(|a, b| a.step.total_cmp(&b.step));
        p.patterns.push(Pattern { id, name: format!("Pattern {i}"), color: color(i), len, min_len: len, notes, swing: 0.0 });
        pats.push(id);
    }
    // one stress pattern for the piano roll
    {
        let id = p.alloc_id();
        let ch = pitched.first().copied().unwrap_or(chans[0]);
        let mut notes: Vec<Note> = (0..2000)
            .map(|_| Note::new(ch, rng.below(1024) as f32, [1.0, 2.0, 4.0][rng.below(3)], Some(24 + rng.below(60) as u8)))
            .collect();
        notes.sort_by(|a, b| a.step.total_cmp(&b.step));
        p.patterns.push(Pattern {
            id,
            name: "Piano roll stress (2000 notes)".into(),
            color: color(9),
            len: 256.0,
            min_len: 256.0,
            notes,
            swing: 0.0,
        });
    }

    // ---- clips
    let cid = |p: &mut Project| p.alloc_id();
    for i in 0..spec.audio_clips {
        let track = if i < spec.tuned_clips { i % spec.vocal_tracks.max(1) } else { rng.below(spec.tracks) };
        let si = if track < spec.vocal_tracks { vocals[rng.below(vocals.len())] } else { rng.below(spec.sources) };
        let s = &p.sources[si];
        let len = (s.dur / spb).min(spec.beats / 4.0).max(0.5);
        let start = if i < spec.tuned_clips { 4.0 + 20.0 * i as f64 } else { rng.range(0.0, (spec.beats - len).max(0.0)) };
        let src = s.id;
        let id = cid(&mut p);
        let mut c = new_clip(id, track as u32, (start * 4.0).round() / 4.0, len, ClipKind::Audio, src);
        c.gain = rng.range(-6.0, 2.0) as f32;
        if i % 5 == 0 {
            c.fade_in = 0.02;
            c.fade_out = 0.05;
        }
        if i % 23 == 0 {
            c.norm = true;
        }
        if i % 41 == 0 {
            c.rev = true;
        }
        if i < spec.tuned_clips && style(si) == 0 {
            c.tune = Some(ClipTune {
                notes: (0..3)
                    .map(|k| NoteEdit {
                        start: 0.4 + k as f64 * 0.7,
                        end: 1.0 + k as f64 * 0.7,
                        shift: 40.0 * (k as f32 - 1.0),
                        drift: 0.5,
                        formant: 0.0,
                        gain: 0.0,
                        speed: None,
                    })
                    .collect(),
                pencil: vec![],
                reference: None,
            });
        }
        p.clips.push(c);
    }
    for _ in 0..spec.pat_clips {
        let track = rng.below(spec.tracks.max(1));
        let pat = pats[rng.below(pats.len())];
        let plen = p.patterns.iter().find(|x| x.id == pat).unwrap().len;
        let start = (rng.range(0.0, spec.beats - plen) / 4.0).floor() * 4.0;
        let id = cid(&mut p);
        p.clips.push(new_clip(id, track as u32, start, plen, ClipKind::Pat, pat));
    }

    // ---- automation
    let mut autos = vec![];
    for i in 0..spec.automations {
        let id = p.alloc_id();
        let ins = &p.inserts[1 + rng.below(spec.tracks)];
        let target = if i % 2 == 0 {
            AutoTarget::InsertVol { id: ins.id }
        } else {
            let f = &ins.fx[rng.below(ins.fx.len())];
            match params(f.kind).first() {
                Some(d) => AutoTarget::Fx { fx_id: f.id, param: d.id.to_string() },
                None => AutoTarget::InsertPan { id: ins.id },
            }
        };
        let span = 64.0;
        let points: Vec<AutoPoint> = (0..spec.auto_points)
            .map(|k| {
                AutoPoint::at(
                    span * k as f64 / (spec.auto_points - 1).max(1) as f64,
                    (0.5 + 0.4 * (k as f32 * 0.37 + i as f32).sin()).clamp(0.0, 1.0),
                )
            })
            .collect();
        p.automations.push(Automation { id, name: format!("Auto {i}"), color: color(i), target, points });
        autos.push((id, span));
    }
    for (k, (aid, span)) in autos.iter().enumerate() {
        let id = cid(&mut p);
        let track = (k * 3) % spec.tracks;
        p.clips.push(new_clip(id, track as u32, (k % 8) as f64 * 64.0, *span, ClipKind::Auto, *aid));
    }

    // ---- comps (takes of a vocal source, comp clip on a vocal track)
    for k in 0..spec.comps {
        let id = p.alloc_id();
        let mut takes = vec![];
        for t in 0..4 {
            let tid = p.alloc_id();
            let si = vocals[(k * 4 + t) % vocals.len()];
            let s = &p.sources[si];
            takes.push(Take {
                id: tid,
                name: format!("Take {}", t + 1),
                source: s.id,
                start: 0.0,
                len: s.dur / spb,
                offset: 0.0,
                color: None,
            });
        }
        let end = takes.iter().map(|t| t.len).fold(f64::MAX, f64::min);
        let mut comp = Comp { id, name: format!("Comp {k}"), takes, regions: vec![], xfade: DEFAULT_XFADE };
        let seg = end / 6.0;
        for r in 0..6 {
            let take = comp.takes[r % 4].id;
            comp.write(take, r as f64 * seg, (r + 1) as f64 * seg);
        }
        let cl = cid(&mut p);
        let track = (k % spec.vocal_tracks.max(1)) as u32;
        p.clips.push(new_clip(cl, track, 300.0 + k as f64 * 40.0, end.min(32.0), ClipKind::Comp, id));
        p.comps.push(comp);
    }

    p.loop_range = Some(LoopRange { a: 16.0, b: 48.0 });
    p
}

pub struct Generated {
    pub dir: PathBuf,
    pub wave: PathBuf,
    pub project: Project,
}

/// Builds, saves `<dir>/<name>.wave` and writes `<dir>/project.json` (UI fixture).
pub fn generate(spec: &Spec, root: &Path) -> Generated {
    let dir = root.join(spec.name);
    let project = build(spec, &dir);
    let e = Engine::new(project.clone(), Arc::new(NoPlugins));
    let wave = dir.join(format!("{}.wave", spec.name));
    e.save(&wave).expect("save fixture");
    let ui = serde_json::to_vec(&project).unwrap();
    std::fs::write(dir.join("project.json"), ui).unwrap();
    Generated { dir, wave, project }
}
