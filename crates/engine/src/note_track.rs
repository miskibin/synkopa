//! Which notes a sung pitch curve means: the pitch editor's note blobs, and so what "Copy notes"
//! pastes into the Piano roll.
//!
//! A voice is not a keyboard. Vibrato swings up to a semitone either side of the note, a held
//! note drifts, the voice scoops into a note and glides between notes, and a repeated syllable
//! on one pitch changes no pitch at all. Splitting wherever the pitch leaves a semitone band
//! (the editor's first segmenter) turns every vibrato cycle and scoop into a note. Here:
//!
//! 1. **Note sequence by Viterbi** over semitone states (the take's own tuning as the grid
//!    offset), on the pitch averaged over about one vibrato cycle: a note change costs
//!    `switch`, each frame the squared distance to the note. Vibrato and drift average out;
//!    a step to another note that is held wins. Frames where the pitch moves fast (glides) carry
//!    no evidence. (The note HMM of pYIN/Tony works the same way, with attack/stable states.)
//! 2. **Short notes** (< `min_note`) join the neighbour nearest in pitch.
//! 3. **Glides**: a short note with no steady stretch (the voice passing through on its way) is
//!    shared out between its neighbours where the pitch crosses their midpoint; a scoop or fall
//!    at a phrase edge joins the note it leads to. Short notes the voice holds (fast scale
//!    notes, ornaments) stay.
//! 4. **Repeated notes**: a new syllable on the same pitch shows as a spectral onset (and often
//!    a level dip); a note is split there when both parts are long enough.
//!
//! Parameters were chosen on held-out singers of Annotated VocalSet and GTSinger with the
//! note-level metrics of `tune-bench transcribe` (mir_eval's COnP / COnPOff); see
//! `crates/tune-bench/RESULTS.md`.

use crate::pitch::NoteBlob;

/// Tracker settings (seconds, semitones). [`Default`] is the measured choice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackParams {
    /// Spread of a frame around its note's centre, semitones.
    pub sigma: f32,
    /// Cost of a note change, in units of one frame's squared error / 2σ².
    pub switch: f32,
    /// The pitch is averaged over this (≈ one vibrato cycle) before notes are decided.
    pub smooth: f32,
    /// Shortest note.
    pub min_note: f32,
    /// Frames moving faster than this (semitones/s, over ±15 ms) are glides. Vibrato of ±1 st at
    /// 6.5 Hz peaks at 41 st/s; a legato note change runs at 50–150.
    pub glide: f32,
    /// A short note (below `glide_note`) is a glide unless the pitch stays within a range of
    /// `steady_st` semitones for `steady` seconds.
    pub glide_note: f32,
    pub steady: f32,
    pub steady_st: f32,
    /// Spectral onset strength (robust z-score over the take) that splits a held pitch; 0 = off.
    pub onset_z: f32,
    /// A level dip this many dB below both sides' peaks splits a held pitch; 0 = off.
    pub dip_db: f32,
    /// Grid offset from the take's own tuning (else A440).
    pub own_tuning: bool,
    /// Where along a glide between two notes the second one starts: 0 = where the voice leaves
    /// the first, 1 = where it arrives at the second.
    pub arrive: f32,
    /// A note change moves to the strongest spectral onset within `snap` seconds when it reaches
    /// `snap_z` (a new syllable on the new note); 0 = off.
    pub snap_z: f32,
    pub snap: f32,
}

impl Default for TrackParams {
    /// Chosen by `tune-bench transcribe-grid --maximin`: the best worst-case COnP over Annotated
    /// VocalSet's tuning singers and two GTSinger singers (see `crates/tune-bench/RESULTS.md`).
    fn default() -> Self {
        TrackParams {
            sigma: 0.8,
            switch: 8.0,
            smooth: 0.15,
            min_note: 0.08,
            glide: 45.0,
            glide_note: 0.16,
            steady: 0.05,
            steady_st: 0.3,
            onset_z: 6.0,
            dip_db: 0.0,
            own_tuning: false,
            arrive: 0.7,
            snap_z: 3.0,
            snap: 0.06,
        }
    }
}

/// Notes of a pitch curve (`rate` frames/s; MIDI, NaN unvoiced) with its frame levels and
/// spectral onset strength (`onsets`, may be empty).
pub fn track(curve: &[f32], levels: &[f32], onsets: &[f32], rate: f32, p: &TrackParams) -> Vec<NoteBlob> {
    let fr = |s: f32| (s * rate).round().max(0.0) as usize;
    let (w, min_len) = (fr(p.smooth) / 2, fr(p.min_note).max(1));
    let grid = if p.own_tuning { tuning(curve) } else { 0.0 };
    let z = robust_z(onsets, curve);
    let dt = 1.0 / rate as f64;
    let mut out = vec![];
    let mut f = 0;
    while f < curve.len() {
        if !curve[f].is_finite() {
            f += 1;
            continue;
        }
        let mut g = f;
        while g < curve.len() && curve[g].is_finite() {
            g += 1;
        }
        let run = &curve[f..g];
        if run.len() >= min_len.max(4) {
            let gl = glides(run, p.glide, rate);
            let mut segs = decode(run, &gl, grid, w, p.sigma, p.switch);
            absorb_short(&mut segs, min_len);
            absorb_glides(&mut segs, run, fr(p.glide_note), fr(p.steady), p.steady_st, p.arrive);
            segs.dedup_by(|b, a| {
                // neighbours left on one key by a glide between them are one note
                let same = a.2 == b.2;
                if same {
                    a.1 = b.1;
                }
                same
            });
            if p.snap_z > 0.0 {
                snap_changes(&mut segs, z.get(f..g).unwrap_or(&[]), p.snap_z, fr(p.snap), min_len);
            }
            for (a, b, _) in segs {
                let lv = levels.get(f..g).unwrap_or(&[]);
                let zz = z.get(f..g).unwrap_or(&[]);
                for (a, b) in split_repeats(lv, zz, a, b, p.onset_z, p.dip_db, min_len.max(fr(0.08)), rate) {
                    // the pitch without the glides into and out of the note (vibrato inside stays:
                    // its median is the note's centre)
                    let (mut s, mut e) = (a, b);
                    while s < e && gl[s] {
                        s += 1;
                    }
                    while e > s && gl[e - 1] {
                        e -= 1;
                    }
                    let pitch = median(run[if e - s >= 4 { s..e } else { a..b }].to_vec());
                    let level = levels.get(f + a..f + b).unwrap_or(&[]).iter().fold(0f32, |m, x| m.max(*x));
                    out.push(NoteBlob { start: (f + a) as f64 * dt, end: (f + b) as f64 * dt, pitch, level });
                }
            }
        }
        f = g;
    }
    out
}

/// How far below a note's loudest frame the voice may fall before the note has ended, dB.
/// A note-segmentation model (GAME) runs each note on until the next one starts, as a score
/// would; the Piano roll wants the note to end where the singing does (vocadito: COnPOff
/// 0.32 → 0.44 and 0.43 → 0.59 against the two annotators; 6–12 dB all within 0.02, see
/// `crates/tune-bench/RESULTS.md`).
pub const MODEL_END_DB: f32 = 8.0;
/// Shortest note kept from a model, seconds.
const MODEL_MIN_NOTE: f64 = 0.03;

/// Notes from a note-segmentation model, fitted to the analysed pitch curve: each note ends at
/// its last voiced frame within [`MODEL_END_DB`] of its peak level, and its pitch is the median
/// of what was sung, kept within the half-step of the model's key (the model chooses the key
/// better than the median does: a scoop or a vibrato leaning one way pulls the median; the
/// median places the note's centre, which pitch correction moves).
pub fn from_model(model: &[crate::pitch::ModelNote], curve: &[f32], levels: &[f32], rate: f32) -> Vec<NoteBlob> {
    let fr = |s: f64| ((s * rate as f64).round().max(0.0) as usize).min(curve.len());
    let db = |x: f32| 20.0 * x.max(1e-6).log10();
    let mut sorted: Vec<_> = model.iter().filter(|n| n.end > n.start && n.start.is_finite() && n.end.is_finite()).copied().collect();
    sorted.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut out: Vec<NoteBlob> = vec![];
    for n in sorted {
        let (a, b) = (fr(n.start), fr(n.end));
        let mut end = n.end;
        if b > a {
            let peak = levels.get(a..b).unwrap_or(&[]).iter().fold(0f32, |m, &x| m.max(x));
            let last = (a..b).rev().find(|&f| curve[f].is_finite() && levels.get(f).is_some_and(|&l| db(l) >= db(peak) - MODEL_END_DB));
            if let Some(f) = last {
                end = end.min((f + 1) as f64 / rate as f64);
            }
        }
        if end - n.start < MODEL_MIN_NOTE {
            continue;
        }
        let e = fr(end).max(a);
        let sung: Vec<f32> = curve[a..e].iter().copied().filter(|x| x.is_finite()).collect();
        let key = n.pitch.round();
        let pitch = match (n.pitch.is_finite(), sung.is_empty()) {
            (true, true) => n.pitch,
            (true, false) => median(sung).clamp(key - 0.49, key + 0.49),
            (false, false) => median(sung),
            (false, true) => continue,
        };
        let level = levels.get(a..e).unwrap_or(&[]).iter().fold(0f32, |m, &x| m.max(x));
        // a model never overlaps its notes; keep it so after rounding to frames
        if let Some(p) = out.last_mut() {
            p.end = p.end.min(n.start);
        }
        out.push(NoteBlob { start: n.start, end, pitch, level });
    }
    out
}

/// The take's offset from A440 in semitones (−0.5..0.5): circular mean of the voiced frames.
fn tuning(curve: &[f32]) -> f32 {
    let (mut s, mut c) = (0f64, 0f64);
    for &x in curve.iter().filter(|x| x.is_finite()) {
        let a = std::f64::consts::TAU * x as f64;
        s += a.sin();
        c += a.cos();
    }
    if s == 0.0 && c == 0.0 {
        0.0
    } else {
        (s.atan2(c) / std::f64::consts::TAU) as f32
    }
}

fn median(mut v: Vec<f32>) -> f32 {
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

/// Floor of the onset spread (mean log-band rise, nepers): on a steady sound the spread is near
/// zero and numeric ripple would read as onsets; with the floor a split needs a real rise
/// (`onset_z` × 0.15 nepers ≈ 4 dB across the bands at the default; a steady synthetic tone
/// ripples by up to 0.4).
const ONSET_MAD_FLOOR: f32 = 0.15;

/// Onset strength as a robust z-score over the voiced frames (median, 1.4826·MAD).
fn robust_z(onsets: &[f32], curve: &[f32]) -> Vec<f32> {
    let v: Vec<f32> = onsets.iter().zip(curve).filter(|(_, c)| c.is_finite()).map(|(o, _)| *o).collect();
    if v.len() < 20 {
        return vec![0.0; onsets.len()];
    }
    let m = median(v.clone());
    let mad = (median(v.iter().map(|x| (x - m).abs()).collect()) * 1.4826).max(ONSET_MAD_FLOOR);
    onsets.iter().map(|o| (o - m) / mad).collect()
}

/// Frames of a voiced run where the pitch moves faster than `limit` st/s (over ±15 ms).
fn glides(run: &[f32], limit: f32, rate: f32) -> Vec<bool> {
    let h = ((0.015 * rate).round() as usize).max(1);
    (0..run.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(h), (i + h).min(run.len() - 1));
            limit > 0.0 && b > a && ((run[b] - run[a]) * rate / (b - a) as f32).abs() > limit
        })
        .collect()
}

/// Half-width of the light smoothing, frames (±15 ms at 200 fps).
const LIGHT: usize = 3;
/// A frame is steady inside a plateau: `STEADY` frames (70 ms) or more within `STEADY_RANGE`
/// semitones. A held note is one; a vibrato deep enough to reach the next note's half (±0.5 st
/// or more, 4.5–7 Hz) stays that close to its peak for under 60 ms.
const STEADY: usize = 14;
const STEADY_RANGE: f32 = 0.25;

/// Viterbi over semitone states; returns (start, end, state pitch) per note of the run.
fn decode(run: &[f32], gl: &[bool], grid: f32, w: usize, sigma: f32, switch: f32) -> Vec<(usize, usize, f32)> {
    let t = run.len();
    let mut pre = vec![0f64; t + 1];
    for i in 0..t {
        pre[i + 1] = pre[i] + run[i] as f64;
    }
    // pieces between glides: the vibrato average never reaches across a note change, which
    // would leave the pitch "between notes" for half a window on each side of it
    let mut piece = vec![(0usize, t); t];
    let mut a = 0;
    for i in 0..=t {
        if i == t || gl[i] {
            for p in piece.iter_mut().take(i).skip(a) {
                *p = (a, i);
            }
            if i < t {
                piece[i] = (i, i + 1);
            }
            a = i + 1;
        }
    }
    let avg = |w: usize, bounded: bool| -> Vec<f32> {
        (0..t)
            .map(|i| {
                let (lo, hi) = if bounded { piece[i] } else { (0, t) };
                let (a, b) = (i.saturating_sub(w).max(lo), (i + w + 1).min(t).min(hi));
                ((pre[b] - pre[a]) / (b - a) as f64) as f32
            })
            .collect()
    };
    // over a vibrato cycle within the piece (y), and over 30 ms (x: detector noise only)
    let (y, x) = (avg(w, true), avg(LIGHT, false));
    // plateaus: stretches of at least STEADY frames within STEADY_RANGE
    let mut steady = vec![false; t];
    let mut i = 0;
    while i < t {
        let (mut lo, mut hi, mut j) = (x[i], x[i], i + 1);
        while j < t && hi.max(x[j]) - lo.min(x[j]) <= STEADY_RANGE {
            lo = lo.min(x[j]);
            hi = hi.max(x[j]);
            j += 1;
        }
        if j - i >= STEADY {
            steady[i..j].fill(true);
            i = j;
        } else {
            i += 1;
        }
    }
    let lo = y.iter().fold(f32::INFINITY, |m, &x| m.min(x)).floor() as i32 - 1;
    let hi = y.iter().fold(f32::NEG_INFINITY, |m, &x| m.max(x)).ceil() as i32 + 1;
    let k = (hi - lo + 1).max(1) as usize;
    let inv = 1.0 / (2.0 * sigma * sigma);
    let em = |i: usize, s: usize| {
        if gl[i] {
            return 0.0;
        }
        // a steady stretch is read as it is (a short held note); elsewhere the pitch averaged
        // over a vibrato cycle (vibrato and drift move around the note, not to another)
        let c = (lo + s as i32) as f32 + grid;
        let d = if steady[i] { x[i] - c } else { y[i] - c };
        d * d * inv
    };
    let mut cost: Vec<f32> = (0..k).map(|s| em(0, s)).collect();
    let mut next = vec![0f32; k];
    let mut back = vec![0u16; t * k];
    let best = |c: &[f32]| c.iter().enumerate().fold((0usize, f32::INFINITY), |m, (s, &v)| if v < m.1 { (s, v) } else { m });
    for i in 1..t {
        let (bs, bc) = best(&cost);
        for s in 0..k {
            // staying wins ties, so a note changes where the evidence for the next one starts
            let (from, v) = if cost[s] <= bc + switch { (s, cost[s]) } else { (bs, bc + switch) };
            next[s] = v + em(i, s);
            back[i * k + s] = from as u16;
        }
        std::mem::swap(&mut cost, &mut next);
    }
    let mut s = best(&cost).0;
    let mut path = vec![0usize; t];
    for i in (0..t).rev() {
        path[i] = s;
        if i > 0 {
            s = back[i * k + s] as usize;
        }
    }
    let mut segs = vec![];
    let mut a = 0;
    for i in 1..=t {
        if i == t || path[i] != path[a] {
            segs.push((a, i, (lo + path[a] as i32) as f32 + grid));
            a = i;
        }
    }
    segs
}

/// Notes shorter than `min_len` join the neighbour nearest in pitch, shortest first.
fn absorb_short(segs: &mut Vec<(usize, usize, f32)>, min_len: usize) {
    while segs.len() > 1 {
        let Some(j) = (0..segs.len()).filter(|&j| segs[j].1 - segs[j].0 < min_len).min_by_key(|&j| segs[j].1 - segs[j].0) else {
            break;
        };
        let into = nearest(segs, j);
        join(segs, j, into);
    }
}

fn nearest(segs: &[(usize, usize, f32)], j: usize) -> usize {
    let d = |k: usize| (segs[k].2 - segs[j].2).abs();
    match (j.checked_sub(1), (j + 1 < segs.len()).then_some(j + 1)) {
        (Some(l), Some(r)) => {
            if d(l) <= d(r) {
                l
            } else {
                r
            }
        }
        (Some(l), None) => l,
        (None, Some(r)) => r,
        (None, None) => j,
    }
}

/// Merges segment `j` into its neighbour `into` (which keeps its pitch).
fn join(segs: &mut Vec<(usize, usize, f32)>, j: usize, into: usize) {
    let (lo, hi) = (j.min(into), j.max(into));
    segs[lo] = (segs[lo].0, segs[hi].1, segs[into].2);
    segs.remove(hi);
}

/// Short notes the voice only passes through are shared out between their neighbours.
fn absorb_glides(segs: &mut Vec<(usize, usize, f32)>, run: &[f32], max_len: usize, steady: usize, steady_st: f32, arrive: f32) {
    let mut j = 0;
    while j < segs.len() {
        let (a, b, _) = segs[j];
        if segs.len() < 2 || b - a >= max_len || held(&run[a..b], steady, steady_st) {
            j += 1;
            continue;
        }
        let (prev, next) = (j.checked_sub(1).map(|k| segs[k]), segs.get(j + 1).copied());
        match (prev, next) {
            (Some(l), Some(r)) => {
                // where the pitch crosses `arrive` of the way to the next note, else the middle
                let mid = l.2 + (r.2 - l.2) * arrive.clamp(0.05, 0.95);
                let up = r.2 >= l.2;
                let cut = (a..b).find(|&i| if up { run[i] >= mid } else { run[i] <= mid }).unwrap_or((a + b) / 2);
                segs[j - 1].1 = cut;
                segs[j + 1].0 = cut;
                segs.remove(j);
                // the left neighbour may now be the end of a glide chain: look again from it
                j = j.saturating_sub(1);
            }
            (None, Some(_)) => {
                segs[j + 1].0 = a;
                segs.remove(j);
            }
            (Some(_), None) => {
                segs[j - 1].1 = b;
                segs.remove(j);
                j = j.saturating_sub(1);
            }
            (None, None) => j += 1,
        }
    }
}

/// Whether the voice holds a pitch somewhere in `seg`: `need` frames in a row within a range of
/// `range` semitones (a vibrato peak is shorter, see [`STEADY`]).
fn held(seg: &[f32], need: usize, range: f32) -> bool {
    let mut i = 0;
    while i + need <= seg.len() {
        let (mut lo, mut hi, mut j) = (seg[i], seg[i], i + 1);
        while j < seg.len() && hi.max(seg[j]) - lo.min(seg[j]) <= range {
            lo = lo.min(seg[j]);
            hi = hi.max(seg[j]);
            j += 1;
        }
        if j - i >= need {
            return true;
        }
        i += 1;
    }
    false
}

/// Moves each note change to the strongest onset within ±`reach` frames that reaches `min_z`,
/// keeping both notes at least `min_len` long.
fn snap_changes(segs: &mut [(usize, usize, f32)], z: &[f32], min_z: f32, reach: usize, min_len: usize) {
    for k in 1..segs.len() {
        let c = segs[k].0;
        if segs[k - 1].1 != c {
            continue; // not adjacent (cannot happen inside a run)
        }
        let lo = c.saturating_sub(reach).max(segs[k - 1].0 + min_len);
        let hi = (c + reach + 1).min(segs[k].1.saturating_sub(min_len)).min(z.len());
        let best = (lo..hi).filter(|&i| z[i] >= min_z).max_by(|&a, &b| z[a].total_cmp(&z[b]));
        if let Some(i) = best {
            segs[k - 1].1 = i;
            segs[k].0 = i;
        }
    }
}

/// Splits [a, b) where a new syllable starts on the same pitch: an onset-strength peak at or
/// above `onset_z`, or a level dip `dip_db` below the peaks within 100 ms on both sides. Each
/// part keeps at least `min_len` frames.
#[allow(clippy::too_many_arguments)]
fn split_repeats(
    levels: &[f32],
    z: &[f32],
    a: usize,
    b: usize,
    onset_z: f32,
    dip_db: f32,
    min_len: usize,
    rate: f32,
) -> Vec<(usize, usize)> {
    if b - a < 2 * min_len || (onset_z <= 0.0 && dip_db <= 0.0) {
        return vec![(a, b)];
    }
    let reach = (0.1 * rate) as usize;
    let db = |x: f32| 20.0 * x.max(1e-6).log10();
    let is_cut = |i: usize| -> bool {
        let peak = |r: std::ops::Range<usize>| levels.get(r).unwrap_or(&[]).iter().fold(0f32, |m, &x| m.max(x));
        let onset =
            onset_z > 0.0 && z.get(i).is_some_and(|&v| v >= onset_z && v >= z[i - 1] && v >= z.get(i + 1).copied().unwrap_or(f32::MIN));
        let dip = dip_db > 0.0
            && levels.get(i).is_some_and(|&l| {
                let lv = db(l);
                db(peak(i.saturating_sub(reach)..i)) - lv >= dip_db && db(peak(i + 1..i + 1 + reach)) - lv >= dip_db
            });
        onset || dip
    };
    let mut cuts = vec![];
    let mut i = a + min_len;
    while i + min_len <= b {
        if is_cut(i) {
            cuts.push(i);
            i += min_len;
        } else {
            i += 1;
        }
    }
    let mut out = vec![];
    let mut s = a;
    for c in cuts {
        out.push((s, c));
        s = c;
    }
    out.push((s, b));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 200.0;

    /// A pitch curve from (seconds, MIDI start, MIDI end) pieces; NaN for `None`.
    fn curve(pieces: &[(f32, Option<(f32, f32)>)]) -> Vec<f32> {
        let mut v = vec![];
        for &(dur, p) in pieces {
            let n = (dur * RATE) as usize;
            for i in 0..n {
                v.push(match p {
                    Some((a, b)) => a + (b - a) * i as f32 / n.max(1) as f32,
                    None => f32::NAN,
                });
            }
        }
        v
    }

    fn vibrato(v: &mut [f32], from: f32, to: f32, rate_hz: f32, depth_st: f32) {
        for (i, x) in v.iter_mut().enumerate() {
            let t = i as f32 / RATE;
            if t >= from && t < to && x.is_finite() {
                *x += depth_st * (std::f32::consts::TAU * rate_hz * t).sin();
            }
        }
    }

    fn run(c: &[f32], onsets: &[f32]) -> Vec<(f64, f64, i32)> {
        let lv = vec![0.3; c.len()];
        track(c, &lv, onsets, RATE, &TrackParams::default()).iter().map(|n| (n.start, n.end, n.pitch.round() as i32)).collect()
    }

    fn keys(c: &[f32]) -> Vec<i32> {
        run(c, &[]).iter().map(|n| n.2).collect()
    }

    #[test]
    fn wide_vibrato_is_one_note() {
        for (rate, depth) in [(5.5, 0.5), (6.5, 1.0), (4.5, 0.8)] {
            let mut c = curve(&[(2.0, Some((60.0, 60.0)))]);
            vibrato(&mut c, 0.0, 2.0, rate, depth);
            assert_eq!(keys(&c), vec![60], "vibrato {rate} Hz ±{depth} st");
        }
    }

    #[test]
    fn slow_drift_is_one_note() {
        assert_eq!(keys(&curve(&[(3.0, Some((59.7, 60.4)))])), vec![60]);
    }

    #[test]
    fn legato_steps_with_glides_and_vibrato_give_one_note_each() {
        // C E G C' sung legato: 80 ms glides between notes, vibrato on every note
        let mut c = curve(&[
            (0.6, Some((60.0, 60.0))),
            (0.08, Some((60.0, 64.0))),
            (0.6, Some((64.0, 64.0))),
            (0.08, Some((64.0, 67.0))),
            (0.6, Some((67.0, 67.0))),
            (0.1, Some((67.0, 72.0))),
            (0.8, Some((72.0, 72.0))),
        ]);
        vibrato(&mut c, 0.0, 4.0, 5.5, 0.6);
        let n = run(&c, &[]);
        assert_eq!(n.iter().map(|x| x.2).collect::<Vec<_>>(), vec![60, 64, 67, 72]);
        // each change lands inside its glide, not a vibrato cycle away
        for (k, at) in [0.64, 1.32, 2.0].iter().enumerate() {
            assert!((n[k + 1].0 - at).abs() < 0.06, "change {k} at {:.3}, glide around {at}", n[k + 1].0);
        }
    }

    #[test]
    fn scoop_into_a_note_is_not_a_note() {
        // a 120 ms scoop from 2 semitones below, then the note
        assert_eq!(keys(&curve(&[(0.12, Some((58.0, 62.0))), (0.8, Some((62.0, 62.0)))])), vec![62]);
        // a fall-off at the end of a phrase
        assert_eq!(keys(&curve(&[(0.8, Some((62.0, 62.0))), (0.15, Some((62.0, 57.0)))])), vec![62]);
    }

    #[test]
    fn fast_scale_notes_are_kept() {
        // 110 ms per note, a quick 20 ms move between them
        let mut pieces = vec![];
        for (k, p) in [60.0, 62.0, 64.0, 65.0, 67.0, 69.0, 71.0, 72.0].iter().enumerate() {
            if k > 0 {
                pieces.push((0.02, Some((p - 2.0, *p))));
            }
            pieces.push((0.11, Some((*p, *p))));
        }
        assert_eq!(keys(&curve(&pieces)), vec![60, 62, 64, 65, 67, 69, 71, 72]);
    }

    #[test]
    fn short_held_ornaments_are_kept() {
        let orn =
            |st: f32, s: f32| keys(&curve(&[(0.5, Some((64.0, 64.0))), (s, Some((64.0 + st, 64.0 + st))), (0.5, Some((64.0, 64.0)))]));
        // a whole-tone neighbour from 90 ms, a half-tone one from 150 ms
        assert_eq!(orn(2.0, 0.09), vec![64, 66, 64]);
        assert_eq!(orn(1.0, 0.15), vec![64, 65, 64]);
        // known limit: a half-step flick of 120 ms or less is within what vibrato does and stays
        // in the note (the trade-off chosen on the benchmark; split it in the editor)
        assert_eq!(orn(1.0, 0.12), vec![64]);
    }

    #[test]
    fn repeated_note_splits_at_a_syllable_onset_or_a_gap() {
        let c = curve(&[(1.2, Some((67.0, 67.0)))]);
        // no evidence of a new syllable: one note
        assert_eq!(keys(&c), vec![67]);
        // a spectral onset at 0.6 s: two notes
        let mut on = vec![1.0; c.len()];
        for (i, x) in on.iter_mut().enumerate() {
            *x += (i % 7) as f32 * 0.1; // background flux
        }
        on[120] = 20.0;
        let n = run(&c, &on);
        assert_eq!(n.iter().map(|x| x.2).collect::<Vec<_>>(), vec![67, 67]);
        assert!((n[1].0 - 0.6).abs() < 0.01);
        // a consonant gap (unvoiced) also separates them
        assert_eq!(keys(&curve(&[(0.5, Some((67.0, 67.0))), (0.04, None), (0.5, Some((67.0, 67.0)))])), vec![67, 67]);
    }

    #[test]
    fn short_voiced_blips_are_dropped_and_empty_input_is_fine() {
        assert!(keys(&curve(&[(0.01, Some((60.0, 60.0)))])).is_empty());
        assert!(keys(&[]).is_empty());
        assert!(keys(&curve(&[(1.0, None)])).is_empty());
    }

    #[test]
    fn a_flat_singer_keeps_the_nearest_keys() {
        // 40 cents flat throughout, with vibrato: still C D E
        let mut c = curve(&[
            (0.5, Some((59.6, 59.6))),
            (0.05, Some((59.6, 61.6))),
            (0.5, Some((61.6, 61.6))),
            (0.05, Some((61.6, 63.6))),
            (0.5, Some((63.6, 63.6))),
        ]);
        vibrato(&mut c, 0.0, 2.0, 5.5, 0.4);
        assert_eq!(keys(&c), vec![60, 62, 64]);
    }

    fn mn(start: f64, end: f64, pitch: f32) -> crate::pitch::ModelNote {
        crate::pitch::ModelNote { start, end, pitch }
    }

    #[test]
    fn model_notes_end_where_the_singing_does() {
        // two notes the model runs on into each other; the first is sung for 0.3 s, then a
        // 0.2 s breath (unvoiced, quiet) before the second
        let c = curve(&[(0.3, Some((60.0, 60.0))), (0.2, None), (0.5, Some((62.0, 62.0)))]);
        let mut lv = vec![0.3; c.len()];
        lv[60..100].fill(0.001);
        let n = from_model(&[mn(0.0, 0.5, 60.0), mn(0.5, 1.0, 62.0)], &c, &lv, RATE);
        assert_eq!(n.len(), 2);
        assert!((n[0].end - 0.3).abs() < 0.011, "first note ends with its singing: {}", n[0].end);
        assert!((n[1].end - 1.0).abs() < 0.011);
        // a note fading out ends when it is 8 dB below its peak
        let c = curve(&[(1.0, Some((64.0, 64.0)))]);
        let lv: Vec<f32> = (0..c.len()).map(|i| if i < 100 { 0.5 } else { 0.5 * 10f32.powf(-0.6 * (i - 100) as f32 / 20.0) }).collect();
        let n = from_model(&[mn(0.0, 1.0, 64.0)], &c, &lv, RATE);
        // 8 dB at 0.6 dB per frame: 13 frames past the decay start
        assert!((n[0].end - 0.57).abs() < 0.011, "ends at {}", n[0].end);
    }

    #[test]
    fn model_notes_take_the_sung_centre_within_the_models_key() {
        // sung 30 ct sharp: the note's centre is what was sung
        let c = curve(&[(0.5, Some((60.3, 60.3)))]);
        let n = from_model(&[mn(0.0, 0.5, 60.0)], &c, &vec![0.3; c.len()], RATE);
        assert!((n[0].pitch - 60.3).abs() < 1e-4);
        // a long scoop pulls the median below the half-step: the model's key holds
        let c = curve(&[(0.3, Some((57.0, 59.4))), (0.2, Some((60.0, 60.0)))]);
        let n = from_model(&[mn(0.0, 0.5, 60.0)], &c, &vec![0.3; c.len()], RATE);
        assert_eq!(n[0].pitch.round(), 60.0);
        // repeated notes on one pitch stay two notes
        let c = curve(&[(1.0, Some((67.0, 67.0)))]);
        let n = from_model(&[mn(0.0, 0.5, 67.0), mn(0.5, 1.0, 67.0)], &c, &vec![0.3; c.len()], RATE);
        assert_eq!(n.iter().map(|x| x.pitch.round() as i32).collect::<Vec<_>>(), vec![67, 67]);
    }

    #[test]
    fn model_notes_without_singing_keep_the_models_pitch_and_bad_notes_go() {
        let c = vec![f32::NAN; 200];
        let n =
            from_model(&[mn(0.0, 0.4, 55.0), mn(0.5, 0.51, 57.0), mn(0.6, 0.6, 58.0), mn(0.7, 0.9, f32::NAN)], &c, &vec![0.3; 200], RATE);
        // the unvoiced note keeps its key and length; the 10 ms blip, the empty one and the
        // pitchless one over silence are dropped
        assert_eq!(n.len(), 1);
        assert_eq!((n[0].start, n[0].end, n[0].pitch), (0.0, 0.4, 55.0));
        assert!(from_model(&[], &c, &[], RATE).is_empty());
    }

    /// Cost of the note decisions on a minute of singing (release: `cargo test --release -p
    /// wave-engine note_track_speed -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn note_track_speed() {
        let sr = 48000.0;
        let secs = 60usize;
        // a phrase every 4 s: 8 notes with vibrato, a breath between phrases
        let mut c = vec![];
        for k in 0..secs / 4 {
            for j in 0..8 {
                c.extend(curve(&[(0.45, Some((57.0 + ((k + j) % 7) as f32, 57.0 + ((k + j) % 7) as f32)))]));
            }
            c.extend(curve(&[(0.4, None)]));
        }
        vibrato(&mut c, 0.0, secs as f32, 5.5, 0.5);
        let mono: Vec<f32> = (0..secs * sr as usize).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
        let t0 = std::time::Instant::now();
        let on = crate::pitch::onset_strength(&mono, sr, c.len());
        let t1 = std::time::Instant::now();
        let n = track(&c, &vec![0.3; c.len()], &on, RATE, &TrackParams::default());
        let t2 = std::time::Instant::now();
        eprintln!("60 s: onset strength {:?}, note tracking {:?}, {} notes", t1 - t0, t2 - t1, n.len());
    }
}
