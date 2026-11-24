//! Chords of audio: what a chord model hears, decoded into chords on the project's beats, and
//! the block chords "Send to piano roll" writes.
//!
//! A [`ChordModel`] (the downloadable ChordMini network, `crates/chords`) turns a source into
//! [`ChordFrames`]: per ~93 ms frame, a probability for every chord of [`QUALITIES`] on every
//! root, plus "no chord". The frames are cached per source; everything after that is cheap and
//! runs per request:
//!
//! * [`decode`] reads the frames a clip plays, folds the vocabulary to the requested
//!   [`Detail`], and finds the most likely chord sequence (Viterbi with a cost for every change, so
//!   one odd frame does not split a chord), then moves every change to the nearest project beat
//!   (chords change on beats) and drops chords shorter than half a beat;
//! * [`voice`] stacks each chord as a pianist would play it from a lead sheet: the root in the
//!   bass, the chord tones close together above middle C, each chord in the inversion nearest
//!   the one before (smooth voice leading instead of parallel root-position blocks).

use serde::{Deserialize, Serialize};

/// Chord qualities, in [`ChordFrames`] order. Models with a larger vocabulary fold theirs into
/// these (6ths into triads, minor-major 7ths into minor, half-diminished into m7, …).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Quality {
    #[serde(rename = "maj")]
    Maj,
    #[serde(rename = "min")]
    Min,
    #[serde(rename = "7")]
    Dom7,
    #[serde(rename = "maj7")]
    Maj7,
    #[serde(rename = "m7")]
    Min7,
    #[serde(rename = "sus2")]
    Sus2,
    #[serde(rename = "sus4")]
    Sus4,
    #[serde(rename = "dim")]
    Dim,
}

pub const QUALITIES: [Quality; 8] =
    [Quality::Maj, Quality::Min, Quality::Dom7, Quality::Maj7, Quality::Min7, Quality::Sus2, Quality::Sus4, Quality::Dim];
/// Classes per frame: 12 roots × [`QUALITIES`], then "no chord".
pub const CLASSES: usize = 12 * QUALITIES.len() + 1;
/// The "no chord" class.
pub const NO_CHORD: usize = CLASSES - 1;

impl Quality {
    /// Semitones above the root.
    pub fn intervals(self) -> &'static [u8] {
        match self {
            Quality::Maj => &[0, 4, 7],
            Quality::Min => &[0, 3, 7],
            Quality::Dom7 => &[0, 4, 7, 10],
            Quality::Maj7 => &[0, 4, 7, 11],
            Quality::Min7 => &[0, 3, 7, 10],
            Quality::Sus2 => &[0, 2, 7],
            Quality::Sus4 => &[0, 5, 7],
            Quality::Dim => &[0, 3, 6],
        }
    }
    /// The chord symbol's suffix: "", "m", "7", "maj7", "m7", "sus2", "sus4", "dim".
    pub fn suffix(self) -> &'static str {
        match self {
            Quality::Maj => "",
            Quality::Min => "m",
            Quality::Dom7 => "7",
            Quality::Maj7 => "maj7",
            Quality::Min7 => "m7",
            Quality::Sus2 => "sus2",
            Quality::Sus4 => "sus4",
            Quality::Dim => "dim",
        }
    }
    /// What the chord is at [`Detail::Simple`]: major or minor.
    pub fn simple(self) -> Quality {
        match self {
            Quality::Maj | Quality::Dom7 | Quality::Maj7 | Quality::Sus2 | Quality::Sus4 => Quality::Maj,
            Quality::Min | Quality::Min7 | Quality::Dim => Quality::Min,
        }
    }
    fn index(self) -> usize {
        QUALITIES.iter().position(|q| *q == self).unwrap_or(0)
    }
}

/// Class index of a chord.
pub fn class(root: u8, q: Quality) -> usize {
    (root as usize % 12) * QUALITIES.len() + q.index()
}

/// How much of the vocabulary to name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Detail {
    /// Every quality of [`QUALITIES`].
    #[default]
    Full,
    /// Major and minor triads only (sevenths and suspensions folded in): the reliable
    /// harmonic outline, and the easier one to play over.
    Simple,
}

/// What a chord model heard: frame `i` is centred at `i / rate` seconds of the source.
pub struct ChordFrames {
    pub rate: f64,
    /// Per frame, probabilities over [`CLASSES`] (summing to 1).
    pub probs: Vec<[f32; CLASSES]>,
}

/// A chord recognizer (the app loads one when its model is installed).
pub trait ChordModel: Send + Sync {
    fn name(&self) -> &'static str;
    /// Chord probabilities of mono audio at `sr`. Slow (about a second a minute): never on the
    /// audio thread.
    fn analyze(&self, mono: &[f32], sr: u32) -> Result<ChordFrames, String>;
}

/// One detected chord, on the project's timeline.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChordSegment {
    /// Project beats.
    pub start: f64,
    pub end: f64,
    /// Pitch class, 0 = C.
    pub root: u8,
    pub quality: Quality,
    /// "Bbmaj7", "F#m".
    pub name: String,
    /// 0..1: how sure the model was over the chord's frames.
    pub confidence: f32,
}

/// `chords_analyze`: the chords of an audio clip.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChordAnalysis {
    pub clip_id: u64,
    /// The clip's extent, project beats.
    pub start: f64,
    pub end: f64,
    pub detail: Detail,
    /// Transposition applied for the clip's own pitch shift, semitones.
    pub transpose: i32,
    /// Spelled with flats (the clip is in a flat key).
    pub flats: bool,
    pub segments: Vec<ChordSegment>,
    /// The model that heard them.
    pub model: String,
}

const SHARPS: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const FLATS: [&str; 12] = ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"];

/// A chord symbol: "C", "Ebm7", "F#sus4".
pub fn chord_name(root: u8, q: Quality, flats: bool) -> String {
    let names = if flats { &FLATS } else { &SHARPS };
    format!("{}{}", names[root as usize % 12], q.suffix())
}

/// Whether a key is written with flats (F, Bb, Eb, Ab, Db, Gb major; their relative minors).
pub fn key_uses_flats(root: u8, minor: bool) -> bool {
    let major_root = if minor { (root + 3) % 12 } else { root % 12 };
    matches!(major_root, 5 | 10 | 3 | 8 | 1 | 6)
}

/// Cost (in log probability) of changing chord between two frames. The model's frames are
/// ~93 ms; at this cost a new chord must out-score the old one by about a factor of three
/// for a quarter of a second before it takes over.
const SWITCH_COST: f32 = 3.0;

/// The chords in source seconds `a..b` of `frames`, at `detail`, as (start s, end s, class,
/// confidence). Neighbouring frames of the same chord are one segment.
pub fn decode_seconds(frames: &ChordFrames, a: f64, b: f64, detail: Detail) -> Vec<(f64, f64, usize, f32)> {
    let rate = frames.rate.max(1e-9);
    let i0 = ((a * rate).round().max(0.0) as usize).min(frames.probs.len());
    let i1 = ((b * rate).round().max(0.0) as usize).clamp(i0, frames.probs.len());
    let span = &frames.probs[i0..i1];
    if span.is_empty() {
        return vec![];
    }
    // fold the vocabulary to the detail
    let fold = |c: usize| -> usize {
        if c == NO_CHORD || detail == Detail::Full {
            return c;
        }
        let (root, q) = (c / QUALITIES.len(), QUALITIES[c % QUALITIES.len()]);
        class(root as u8, q.simple())
    };
    let folded: Vec<[f32; CLASSES]> = span
        .iter()
        .map(|p| {
            let mut f = [0f32; CLASSES];
            for (c, v) in p.iter().enumerate() {
                f[fold(c)] += v;
            }
            f
        })
        .collect();
    let live: Vec<usize> = (0..CLASSES).filter(|&c| fold(c) == c).collect();
    // Viterbi: stay free, switch at SWITCH_COST
    let n = folded.len();
    let lp = |f: &[f32; CLASSES], c: usize| f[c].max(1e-9).ln();
    let mut score: Vec<f32> = live.iter().map(|&c| lp(&folded[0], c)).collect();
    let mut back: Vec<Vec<u16>> = Vec::with_capacity(n);
    back.push((0..live.len() as u16).collect());
    for f in &folded[1..] {
        let (best_k, best) = score.iter().enumerate().fold((0, f32::MIN), |m, (k, &s)| if s > m.1 { (k, s) } else { m });
        let mut next = Vec::with_capacity(live.len());
        let mut from = Vec::with_capacity(live.len());
        for (k, &c) in live.iter().enumerate() {
            let (src, s) = if score[k] >= best - SWITCH_COST { (k, score[k]) } else { (best_k, best - SWITCH_COST) };
            next.push(s + lp(f, c));
            from.push(src as u16);
        }
        score = next;
        back.push(from);
    }
    let mut k = score.iter().enumerate().fold((0, f32::MIN), |m, (k, &s)| if s > m.1 { (k, s) } else { m }).0;
    let mut path = vec![0usize; n];
    for t in (0..n).rev() {
        path[t] = live[k];
        k = back[t][k] as usize;
    }
    let mut out: Vec<(f64, f64, usize, f32)> = vec![];
    let mut s = 0;
    for t in 1..=n {
        if t == n || path[t] != path[s] {
            let c = path[s];
            let conf = folded[s..t].iter().map(|f| f[c]).sum::<f32>() / (t - s) as f32;
            // frames are centred: a frame covers ±half a hop
            let t0 = ((i0 + s) as f64 - 0.5).max(i0 as f64) / rate;
            let t1 = ((i0 + t) as f64 - 0.5) / rate;
            out.push((t0.max(a), if t == n { b } else { t1.min(b) }, c, conf));
            s = t;
        }
    }
    out
}

/// The chords between project beats `from..to` (each change moved to the nearest beat;
/// chords left shorter than half a beat merge into a neighbour). `beat_of(t)` maps a source
/// second to a project beat (monotonic), `transpose` shifts the roots.
pub fn decode(
    frames: &ChordFrames,
    secs: (f64, f64),
    beat_of: impl Fn(f64) -> f64,
    span: (f64, f64),
    detail: Detail,
    transpose: i32,
    flats: bool,
) -> Vec<ChordSegment> {
    let raw = decode_seconds(frames, secs.0, secs.1, detail);
    let (from, to) = span;
    // to beats, snapped
    let mut segs: Vec<(f64, f64, usize, f32)> = raw
        .into_iter()
        .map(|(a, b, c, p)| {
            let (ba, bb) = (beat_of(a), beat_of(b));
            let snap = |x: f64| if (x - from).abs() < 1e-6 || (x - to).abs() < 1e-6 { x } else { x.round() };
            (snap(ba).clamp(from, to), snap(bb).clamp(from, to), c, p)
        })
        .collect();
    // drop slivers: a chord under half a beat gives its time to the longer neighbour
    while let Some(i) = segs.iter().position(|s| s.1 - s.0 < 0.5 - 1e-9) {
        if segs.len() == 1 {
            break;
        }
        let (a, b) = (segs[i].0, segs[i].1);
        let left = i.checked_sub(1);
        let right = (i + 1 < segs.len()).then_some(i + 1);
        let into = match (left, right) {
            (Some(l), Some(r)) => {
                if segs[l].1 - segs[l].0 >= segs[r].1 - segs[r].0 {
                    l
                } else {
                    r
                }
            }
            (Some(l), None) => l,
            (None, Some(r)) => r,
            (None, None) => break,
        };
        if into < i {
            segs[into].1 = b;
        } else {
            segs[into].0 = a;
        }
        segs.remove(i);
    }
    // merge equal neighbours (a sliver between two of the same chord is gone)
    let mut merged: Vec<(f64, f64, usize, f32)> = vec![];
    for s in segs {
        if let Some(last) = merged.last_mut() {
            if last.2 == s.2 {
                let (la, lb) = (last.1 - last.0, s.1 - s.0);
                last.3 = if la + lb > 0.0 { ((last.3 as f64 * la + s.3 as f64 * lb) / (la + lb)) as f32 } else { last.3 };
                last.1 = s.1;
                continue;
            }
        }
        merged.push(s);
    }
    merged
        .into_iter()
        .filter(|s| s.2 != NO_CHORD && s.1 - s.0 > 1e-6)
        .map(|(a, b, c, p)| {
            let root = ((c / QUALITIES.len()) as i32 + transpose).rem_euclid(12) as u8;
            let quality = QUALITIES[c % QUALITIES.len()];
            ChordSegment { start: a, end: b, root, quality, name: chord_name(root, quality, flats), confidence: p.clamp(0.0, 1.0) }
        })
        .collect()
}

/// Bass notes sit in this octave (E1..D#2 for the root), the chord tones from here up
/// (F#3 and above, within an octave and a half).
const BASS_LOW: u8 = 36;
const UPPER_LOW: u8 = 54;
const UPPER_HIGH: u8 = 76;

/// One chord's notes (MIDI keys), the bass first. `prev`: the previous chord's upper voices
/// (the inversion nearest them is chosen).
pub fn voice(root: u8, q: Quality, prev: Option<&[u8]>) -> Vec<u8> {
    let root = root % 12;
    let bass = BASS_LOW + (root + 12 - BASS_LOW % 12) % 12;
    let tones: Vec<u8> = q.intervals().iter().map(|i| (root + i) % 12).collect();
    // every inversion in close position whose top stays in range
    let mut best: Option<(f32, Vec<u8>)> = None;
    for start in UPPER_LOW..UPPER_LOW + 12 {
        if !tones.contains(&(start % 12)) {
            continue;
        }
        let mut v = vec![start];
        while v.len() < tones.len() {
            let mut k = v[v.len() - 1] + 1;
            while !tones.contains(&(k % 12)) || v.iter().any(|x| x % 12 == k % 12) {
                k += 1;
            }
            v.push(k);
        }
        if *v.last().unwrap() > UPPER_HIGH {
            continue;
        }
        // nearest the previous voicing; else centred on E4
        let cost = match prev {
            Some(p) if !p.is_empty() => {
                let pc = p.iter().map(|&x| x as f32).sum::<f32>() / p.len() as f32;
                let vc = v.iter().map(|&x| x as f32).sum::<f32>() / v.len() as f32;
                let moves: f32 = v.iter().map(|&x| p.iter().map(|&y| (x as f32 - y as f32).abs()).fold(f32::MAX, f32::min)).sum();
                moves + 0.5 * (pc - vc).abs()
            }
            _ => (v.iter().map(|&x| x as f32).sum::<f32>() / v.len() as f32 - 64.0).abs(),
        };
        if best.as_ref().map(|b| cost < b.0).unwrap_or(true) {
            best = Some((cost, v));
        }
    }
    let mut out = vec![bass];
    out.extend(best.map(|b| b.1).unwrap_or_default());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames_of(seq: &[(usize, usize)], rate: f64) -> ChordFrames {
        let mut probs = vec![];
        for &(c, n) in seq {
            for _ in 0..n {
                let mut p = [0.1 / (CLASSES - 1) as f32; CLASSES];
                p[c] = 0.9;
                probs.push(p);
            }
        }
        ChordFrames { rate, probs }
    }

    #[test]
    fn names_follow_the_key() {
        assert_eq!(chord_name(10, Quality::Maj7, true), "Bbmaj7");
        assert_eq!(chord_name(6, Quality::Min, false), "F#m");
        assert!(key_uses_flats(5, false)); // F major
        assert!(key_uses_flats(2, true)); // D minor
        assert!(!key_uses_flats(9, true)); // A minor
        assert!(!key_uses_flats(7, false)); // G major
    }

    #[test]
    fn a_short_glitch_does_not_split_a_chord() {
        let c = class(0, Quality::Maj);
        let g = class(7, Quality::Maj);
        // a doubtful frame (G a little ahead of C) is absorbed by the switch cost
        let mut f = frames_of(&[(c, 41)], 10.0);
        f.probs[20][c] = 0.3;
        f.probs[20][g] = 0.6;
        let s = decode_seconds(&f, 0.0, 4.1, Detail::Full);
        assert_eq!(s.len(), 1, "{s:?}");
        assert_eq!(s[0].2, c);
        // a confident one survives the decoder but is far too short to stay on the beat grid
        let f = frames_of(&[(c, 20), (g, 1), (c, 20)], 10.0);
        let s = decode(&f, (0.0, 4.1), |t| t, (0.0, 4.1), Detail::Full, 0, false);
        assert_eq!(s.len(), 1, "{s:?}");
        assert_eq!(s[0].name, "C");
    }

    #[test]
    fn changes_land_on_beats_and_the_span_edges_hold() {
        // 1 s per beat (60 BPM): C for 2.3 s, then Am
        let f = frames_of(&[(class(0, Quality::Maj), 23), (class(9, Quality::Min), 27)], 10.0);
        let s = decode(&f, (0.0, 5.0), |t| t, (0.0, 5.0), Detail::Full, 0, false);
        assert_eq!(s.len(), 2, "{s:?}");
        assert_eq!((s[0].start, s[0].end, s[0].name.as_str()), (0.0, 2.0, "C"));
        assert_eq!((s[1].start, s[1].end, s[1].name.as_str()), (2.0, 5.0, "Am"));
    }

    #[test]
    fn simple_detail_folds_sevenths_and_sus() {
        let f = frames_of(&[(class(7, Quality::Dom7), 30), (class(2, Quality::Min7), 30)], 10.0);
        let s = decode(&f, (0.0, 6.0), |t| t, (0.0, 6.0), Detail::Simple, 0, false);
        let names: Vec<&str> = s.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["G", "Dm"]);
        let full = decode(&f, (0.0, 6.0), |t| t, (0.0, 6.0), Detail::Full, 0, false);
        let names: Vec<&str> = full.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["G7", "Dm7"]);
    }

    #[test]
    fn simple_detail_adds_up_split_evidence() {
        // a frame torn between C, C7 and Cmaj7 is a C, even though "no chord" is the single top class
        let mut p = [0f32; CLASSES];
        p[class(0, Quality::Maj)] = 0.25;
        p[class(0, Quality::Dom7)] = 0.2;
        p[class(0, Quality::Maj7)] = 0.2;
        p[NO_CHORD] = 0.35;
        let f = ChordFrames { rate: 10.0, probs: vec![p; 20] };
        let s = decode(&f, (0.0, 2.0), |t| t, (0.0, 2.0), Detail::Simple, 0, false);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "C");
        assert!((s[0].confidence - 0.65).abs() < 1e-3);
    }

    #[test]
    fn no_chord_leaves_a_gap_and_transposition_moves_roots() {
        let f = frames_of(&[(class(0, Quality::Maj), 20), (NO_CHORD, 20), (class(5, Quality::Maj), 20)], 10.0);
        let s = decode(&f, (0.0, 6.0), |t| t, (0.0, 6.0), Detail::Full, 2, false);
        let names: Vec<(&str, f64, f64)> = s.iter().map(|x| (x.name.as_str(), x.start, x.end)).collect();
        assert_eq!(names, [("D", 0.0, 2.0), ("G", 4.0, 6.0)]);
    }

    #[test]
    fn slivers_merge_into_a_neighbour() {
        // a 0.3-beat chord between two long ones disappears
        let f = frames_of(&[(class(0, Quality::Maj), 40), (class(2, Quality::Min), 6), (class(5, Quality::Maj), 40)], 20.0);
        let s = decode(&f, (0.0, 4.3), |t| t, (0.0, 4.3), Detail::Full, 0, false);
        assert!(s.iter().all(|x| x.end - x.start >= 0.5), "{s:?}");
        assert!(!s.iter().any(|x| x.name == "Dm"), "{s:?}");
    }

    #[test]
    fn voicings_keep_the_root_in_the_bass_and_move_little() {
        let c = voice(0, Quality::Maj, None);
        assert_eq!(c[0], 36); // C2
        assert!(c[1..].iter().all(|&k| (UPPER_LOW..=UPPER_HIGH).contains(&k)));
        let pcs: Vec<u8> = c[1..].iter().map(|k| k % 12).collect();
        for t in [0, 4, 7] {
            assert!(pcs.contains(&t));
        }
        // C → F → G → C: each upper voice moves at most a few semitones
        let mut prev = c[1..].to_vec();
        for (r, q) in [(5, Quality::Maj), (7, Quality::Dom7), (0, Quality::Maj)] {
            let v = voice(r, q, Some(&prev));
            assert_eq!(v[0] % 12, r);
            assert!((BASS_LOW..BASS_LOW + 12).contains(&v[0]));
            let moved: u32 = v[1..].iter().map(|&x| prev.iter().map(|&y| (x as i32 - y as i32).unsigned_abs()).min().unwrap()).sum();
            assert!(moved <= 6, "{r} {q:?}: {prev:?} → {v:?}");
            prev = v[1..].to_vec();
        }
        // a seventh chord has four upper voices
        assert_eq!(voice(2, Quality::Min7, None).len(), 5);
    }
}
