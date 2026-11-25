//! Masking detector ("Mix assistant"): finds pairs of mixer inserts that fight over the same
//! frequencies at the same time — the kick and the bass in 50–100 Hz, a vocal and a guitar in
//! 2–4 kHz — and suggests a sidechained dynamic EQ cut on the one that should give way.
//!
//! Why this shape (research: iZotope Neutron *Unmask*, FabFilter Pro-Q *collision detection*,
//! Wavesfactory *Trackspacer*): masking is a *simultaneous* overlap, so two tracks sharing a
//! band in different bars are fine; the cure is to duck the masker's band only while the other
//! track is loud there (a dynamic EQ band keyed from the other insert), not a static cut that
//! thins the masker for the whole song.
//!
//! ```text
//! audio thread: each non-master insert's post-fader output, mono ──► ring (MASK_TAP_CAPACITY)
//! housekeeping: ring ─► 8192-pt FFT every HOP samples ─► 28 third-octave band levels (dBFS)
//!               per pair of unrelated inserts, per band, per hop:
//!                 hit = both above FLOOR, both within IMPORTANT dB of their own loudest band,
//!                       levels within CLOSE dB of each other
//!                 score += hop seconds on a hit, −½·hop otherwise (0 … MAX_S)
//!               flag the pair when a band's score reaches ON_S, clear it below OFF_S
//!               (only while both play: a verse without the 808 neither adds nor forgets)
//! ```
//!
//! Scores count audio time, not wall time, so the detector behaves the same live, offline and
//! in tests. Inserts that feed each other (a kick and the drum bus it goes into) are never
//! compared: one contains the other. A pair the user ignored, or one already handled by a
//! dynamic EQ on the masker keyed from the other insert, is not reported.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex};
use serde::{Deserialize, Serialize};

use crate::model::{FxKind, Id, Project};

/// Samples per insert the audio thread can queue before the housekeeping thread drains them.
pub const MASK_TAP_CAPACITY: usize = 8192;
const FFT_N: usize = 8192;
/// Analysis hop (samples): ~43 ms at 48 kHz.
const HOP: usize = 2048;
/// Third-octave bands, centres 31.5 Hz · 2^(k/3): 31.5 Hz … 16 kHz.
pub const NB: usize = 28;
/// Below this a band is silence (post-fader dBFS of the band's energy).
const FLOOR: f32 = -60.0;
/// A band matters to a track when it is within this of the track's loudest band.
const IMPORTANT: f32 = 15.0;
/// Two levels this close compete; further apart, the louder one simply wins.
const CLOSE: f32 = 12.0;
/// Net seconds of collision before a pair is reported, and below which it is cleared. Net:
/// a hit adds its time, a miss takes half of it back, so only a collision present most of
/// the time *and* sustained over bars builds up (one loud fill does not).
const ON_S: f32 = 3.0;
const OFF_S: f32 = 1.0;
/// Score ceiling: how long a fixed problem stays reported at most.
const MAX_S: f32 = 6.0;
/// Bands with at least this score are part of the reported range.
const RANGE_S: f32 = 1.5;
/// Seconds of a pair both playing before it can be reported at all (which one is steadier,
/// the 1.5 s statistics, must have settled).
const WARMUP_S: f32 = 3.0;
/// Widest reported range, in bands (two octaves).
const MAX_RANGE: usize = 6;

/// Centre frequency of band `k`.
pub fn band_hz(k: usize) -> f32 {
    31.5 * 2f32.powf(k as f32 / 3.0)
}

/// One reported collision: `masker` covers `protect` in `lo`..`hi` Hz.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collision {
    /// The insert to turn down in the band (the steadier sound).
    pub masker: Id,
    /// The insert to keep clear (the more transient sound in that band).
    pub protect: Id,
    pub lo: f32,
    pub hi: f32,
    /// Centre of the worst band.
    pub center: f32,
    /// Share of the recent past the pair collided in its worst band, 0..1.
    pub overlap: f32,
    /// The protected insert's recent peak level in the range (dBFS): the fix keys on it.
    pub key_db: f32,
}

/// What a mix assistant fix inserts: a dynamic EQ on `masker` keyed from `protect`.
#[derive(Clone, Debug, PartialEq)]
pub struct Fix {
    pub freq: f64,
    pub q: f64,
    /// Most the band is pulled down (dB, negative).
    pub range: f64,
    /// Key level at which ducking starts (dBFS).
    pub thr: f64,
}

impl Collision {
    /// The dynamic EQ band that clears this collision: a bell over the range, ducking up to
    /// 6 dB whenever the protected insert is within 15 dB of its recent peak in the band.
    pub fn fix(&self) -> Fix {
        let (lo, hi) = (self.lo.max(20.0) as f64, self.hi.max(self.lo + 1.0) as f64);
        let freq = (lo * hi).sqrt();
        // bell bandwidth ≈ the range, at least an octave (Q 1.4): a kick and a bass share more than
        // the one third-octave where they collide most
        let q = (freq / (hi - lo)).clamp(0.7, 1.4);
        let thr = (self.key_db as f64 - 15.0).clamp(-80.0, -3.0);
        Fix { freq, q, range: -6.0, thr }
    }
}

/// The effect a fix adds to the masker's chain: an EQ whose only band is a bell over the
/// collision, flat until the protected insert (its sidechain) gets loud in that band, then
/// pulled down by up to 6 dB. An ordinary EQ: it shows in the chain, can be edited, bypassed
/// and undone like any other.
pub fn fix_fx(id: Id, c: &Collision) -> crate::model::Fx {
    use crate::model::{BandType, DynBand, EqBand, Placement};
    let f = c.fix();
    let mut fx = crate::catalog::new_fx(id, FxKind::Eq, 120.0);
    fx.preset = None;
    fx.key = Some(c.protect);
    fx.bands = Some(vec![EqBand {
        on: true,
        kind: BandType::Peak,
        f: f.freq,
        g: 0.0,
        q: f.q,
        slope: 12,
        place: Placement::Stereo,
        dynamic: Some(DynBand { range: f.range, thr: f.thr }),
    }]);
    fx
}

struct Lane {
    buf: Vec<f32>,
    w: usize,
    fed: usize,
    hops: u64,
    /// Smoothed band levels (dB, ~100 ms).
    short: [f32; NB],
    /// Recent peak per band (dB, falls 3 dB/s).
    peak: [f32; NB],
    /// Mean of the raw per-hop level and its mean absolute deviation (~1.5 s): how much the
    /// band jumps around, i.e. how transient the track is there.
    mean: [f32; NB],
    dev: [f32; NB],
}

impl Lane {
    fn new() -> Self {
        Self { buf: vec![0.0; FFT_N], w: 0, fed: 0, hops: 0, short: [-120.0; NB], peak: [-120.0; NB], mean: [-120.0; NB], dev: [0.0; NB] }
    }
    fn loudest(&self) -> f32 {
        self.short.iter().copied().fold(-200.0, f32::max)
    }
}

struct Spectrum {
    plan: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    scratch: Vec<f32>,
    spec: Vec<realfft::num_complex::Complex<f32>>,
    /// Per band: (first bin, last bin, weight of the edge bins), from the band edges.
    bands: Vec<(f32, f32)>,
}

impl Spectrum {
    fn new(sr: f32) -> Self {
        let plan = RealFftPlanner::<f32>::new().plan_fft_forward(FFT_N);
        let window = (0..FFT_N).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT_N as f32).cos()).collect();
        let spec = plan.make_output_vec();
        let bin_hz = sr / FFT_N as f32;
        let edge = 2f32.powf(1.0 / 6.0);
        let bands = (0..NB).map(|k| (band_hz(k) / edge / bin_hz, (band_hz(k) * edge / bin_hz).min((FFT_N / 2) as f32))).collect();
        Self { plan, window, scratch: vec![0.0; FFT_N], spec, bands }
    }

    /// Band energies in dB of the last FFT_N samples of `lane` (a full-scale sine reads 0 dB in
    /// its band).
    fn run(&mut self, lane: &Lane, out: &mut [f32; NB]) {
        for i in 0..FFT_N {
            self.scratch[i] = lane.buf[(lane.w + i) % FFT_N] * self.window[i];
        }
        let _ = self.plan.process(&mut self.scratch, &mut self.spec);
        // Hann: a sine of amplitude A peaks at A·N/4; its energy over the main lobe is
        // (A·N/4)²·1.5 (the window's noise bandwidth)
        let norm = 1.0 / ((FFT_N as f32 / 4.0).powi(2) * 1.5);
        let last = self.spec.len() - 1;
        for (k, &(a, b)) in self.bands.iter().enumerate() {
            // fractional bins at the edges, so narrow low bands (a few bins) are not quantised
            let mut e = 0f32;
            let (i0, i1) = (a.floor() as usize, (b.ceil() as usize).min(last));
            let first = i0.max(1);
            for (i, bin) in self.spec.iter().enumerate().take(i1 + 1).skip(first) {
                let lo = (i as f32 - 0.5).max(a);
                let hi = (i as f32 + 0.5).min(b);
                if hi > lo {
                    e += bin.norm_sqr() * (hi - lo);
                }
            }
            out[k] = 10.0 * (e * norm).max(1e-14).log10();
        }
    }
}

/// Live state of a reported pair.
#[derive(Clone, Copy)]
struct Flag {
    masker: usize,
    protect: usize,
}

pub struct Detector {
    sr: f32,
    lanes: Vec<Lane>,
    ids: Vec<Id>,
    /// Pairs (by lane, a < b) that are compared.
    pairs: Vec<(usize, usize)>,
    scores: HashMap<(usize, usize), [f32; NB]>,
    /// Seconds each compared pair has been evaluated with both inserts sounding.
    heard: HashMap<(usize, usize), f32>,
    flags: HashMap<(usize, usize), Flag>,
    /// `reach[a][b]`: insert `a` is `b` or feeds it through sends (by lane).
    reach: Vec<Vec<bool>>,
    evals: u64,
    fft: Spectrum,
    raw: [f32; NB],
}

impl Detector {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            lanes: vec![],
            ids: vec![],
            pairs: vec![],
            scores: HashMap::new(),
            heard: HashMap::new(),
            flags: HashMap::new(),
            reach: vec![],
            evals: 0,
            fft: Spectrum::new(sr),
            raw: [0.0; NB],
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sr
    }

    /// Sets which inserts lane `k` carries (`ids[k]`, the project's insert order; lane 0 is the
    /// master and is never compared) and which pairs are compared. Forgets everything when the
    /// inserts changed; keeps the scores of pairs that are still compared otherwise.
    pub fn configure(&mut self, p: &Project) {
        let ids: Vec<Id> = p.inserts.iter().map(|m| m.id).collect();
        if ids != self.ids {
            self.lanes = ids.iter().map(|_| Lane::new()).collect();
            self.ids = ids;
            self.scores.clear();
            self.heard.clear();
            self.flags.clear();
            self.evals = 0;
        }
        self.pairs = compared_pairs(p);
        self.reach = p.inserts.iter().map(|a| p.inserts.iter().map(|b| crate::routing::reaches(p, a.id, b.id)).collect()).collect();
        let keep: HashSet<(usize, usize)> = self.pairs.iter().copied().collect();
        self.scores.retain(|k, _| keep.contains(k));
        self.heard.retain(|k, _| keep.contains(k));
        self.flags.retain(|k, _| keep.contains(k));
    }

    /// Feeds post-fader mono samples of lane `k`.
    pub fn push(&mut self, k: usize, x: &[f32]) {
        let Some(lane) = self.lanes.get_mut(k) else { return };
        for &s in x {
            lane.buf[lane.w] = s;
            lane.w = (lane.w + 1) % FFT_N;
            lane.fed += 1;
            if lane.fed == HOP {
                lane.fed = 0;
                Self::analyse(&mut self.fft, &mut self.raw, lane, HOP as f32 / self.sr);
            }
        }
        // evaluate once every lane has a new hop
        let dt = HOP as f32 / self.sr;
        while self.lanes.len() > 1 && self.lanes[1..].iter().all(|l| l.hops > self.evals) {
            self.evals += 1;
            self.evaluate(dt);
        }
    }

    /// Drains rtrb consumers (one per lane, in lane order).
    pub fn drain(&mut self, rings: &mut [rtrb::Consumer<f32>]) {
        for (k, c) in rings.iter_mut().enumerate().take(self.lanes.len()) {
            let n = c.slots();
            if n == 0 {
                continue;
            }
            if let Ok(chunk) = c.read_chunk(n) {
                let (a, b) = chunk.as_slices();
                self.push(k, a);
                self.push(k, b);
                chunk.commit_all();
            }
        }
    }

    fn analyse(fft: &mut Spectrum, raw: &mut [f32; NB], lane: &mut Lane, dt: f32) {
        fft.run(lane, raw);
        let a_short = (-dt / 0.1).exp();
        let a_long = (-dt / 1.5).exp();
        let fall = 3.0 * dt;
        for (k, &x) in raw.iter().enumerate() {
            // smooth in power, read in dB
            let p = 10f32.powf(lane.short[k] / 10.0) * a_short + 10f32.powf(x / 10.0) * (1.0 - a_short);
            lane.short[k] = 10.0 * p.max(1e-14).log10();
            lane.peak[k] = x.max(lane.peak[k] - fall);
            if x > FLOOR {
                lane.mean[k] = if lane.mean[k] < FLOOR { x } else { lane.mean[k] * a_long + x * (1.0 - a_long) };
                lane.dev[k] = lane.dev[k] * a_long + (x - lane.mean[k]).abs() * (1.0 - a_long);
            }
        }
        lane.hops += 1;
    }

    fn evaluate(&mut self, dt: f32) {
        for &(a, b) in &self.pairs {
            let (la, lb) = (&self.lanes[a], &self.lanes[b]);
            let (ma, mb) = (la.loudest(), lb.loudest());
            if ma <= FLOOR || mb <= FLOOR {
                // one of them is silent: nothing collides, nothing is learned (a verse without
                // the 808 does not wash out what the chorus showed)
                continue;
            }
            let heard = self.heard.entry((a, b)).or_insert(0.0);
            *heard += dt;
            let warm = *heard >= WARMUP_S;
            let s = self.scores.entry((a, b)).or_insert([0.0; NB]);
            for (k, sk) in s.iter_mut().enumerate() {
                let (x, y) = (la.short[k], lb.short[k]);
                let hit = x > FLOOR && y > FLOOR && x >= ma - IMPORTANT && y >= mb - IMPORTANT && (x - y).abs() <= CLOSE;
                *sk = (*sk + if hit { dt } else { -0.5 * dt }).clamp(0.0, MAX_S);
            }
            let worst = s.iter().copied().fold(0.0, f32::max);
            let on = self.flags.contains_key(&(a, b));
            if !on && warm && worst >= ON_S {
                // who gives way: the steadier sound in the colliding bands
                let bands: Vec<usize> = (0..NB).filter(|&k| s[k] >= RANGE_S).collect();
                let dev = |l: &Lane| bands.iter().map(|&k| l.dev[k]).sum::<f32>();
                let (masker, protect) = if dev(la) >= dev(lb) { (b, a) } else { (a, b) };
                self.flags.insert((a, b), Flag { masker, protect });
            } else if on && worst < OFF_S {
                self.flags.remove(&(a, b));
            }
        }
    }

    /// The pairs reported now, worst first. A collision is left out when a more specific one
    /// says the same (the 808 against the kick, not also against the drum bus the kick goes
    /// into): its two inserts feed this one's two, and the ranges overlap.
    pub fn collisions(&self) -> Vec<Collision> {
        let all = self.raw_collisions();
        let lane = |id: Id| self.ids.iter().position(|&x| x == id);
        let feeds = |a: Id, b: Id| match (lane(a), lane(b)) {
            (Some(a), Some(b)) => self.reach.get(a).and_then(|r| r.get(b)).copied().unwrap_or(false),
            _ => false,
        };
        all.iter()
            .filter(|c| {
                !all.iter().any(|d| {
                    !std::ptr::eq(*c, d)
                        && (d.masker, d.protect) != (c.masker, c.protect)
                        && feeds(d.masker, c.masker)
                        && feeds(d.protect, c.protect)
                        && d.lo < c.hi
                        && c.lo < d.hi
                })
            })
            .cloned()
            .collect()
    }

    fn raw_collisions(&self) -> Vec<Collision> {
        let mut out: Vec<Collision> = self
            .flags
            .iter()
            .filter_map(|(key, f)| {
                let s = self.scores.get(key)?;
                let (mut top, mut best) = (0, -1.0);
                for (k, &v) in s.iter().enumerate() {
                    if v > best {
                        best = v;
                        top = k;
                    }
                }
                // the contiguous run of strong bands around the worst one, at most MAX_RANGE
                let (mut lo, mut hi) = (top, top);
                while hi + 1 - lo < MAX_RANGE {
                    let down = lo > 0 && s[lo - 1] >= RANGE_S;
                    let up = hi + 1 < NB && s[hi + 1] >= RANGE_S;
                    match (down, up) {
                        (true, true) if s[lo - 1] >= s[hi + 1] => lo -= 1,
                        (true, false) => lo -= 1,
                        (_, true) => hi += 1,
                        _ => break,
                    }
                }
                let edge = 2f32.powf(1.0 / 6.0);
                let key_db = (lo..=hi).map(|k| self.lanes[f.protect].peak[k]).fold(-120.0, f32::max);
                Some(Collision {
                    masker: self.ids[f.masker],
                    protect: self.ids[f.protect],
                    lo: band_hz(lo) / edge,
                    hi: band_hz(hi) * edge,
                    center: band_hz(top),
                    overlap: (best / MAX_S).clamp(0.0, 1.0),
                    key_db,
                })
            })
            .collect();
        out.sort_by(|x, y| y.overlap.total_cmp(&x.overlap).then(x.masker.cmp(&y.masker)).then(x.protect.cmp(&y.protect)));
        out
    }
}

/// Pairs of non-master inserts (by index, a < b) worth comparing: neither feeds the other
/// (directly or through a bus), the user has not ignored the pair, and neither already has a
/// dynamic EQ keyed from the other.
pub fn compared_pairs(p: &Project) -> Vec<(usize, usize)> {
    let n = p.inserts.len();
    let mut out = vec![];
    for a in 1..n {
        for b in a + 1..n {
            let (x, y) = (&p.inserts[a], &p.inserts[b]);
            if crate::routing::reaches(p, x.id, y.id) || crate::routing::reaches(p, y.id, x.id) {
                continue;
            }
            if p.mask_ignore.iter().any(|q| (q[0] == x.id && q[1] == y.id) || (q[0] == y.id && q[1] == x.id)) {
                continue;
            }
            if keyed_from(x, y.id) || keyed_from(y, x.id) {
                continue;
            }
            out.push((a, b));
        }
    }
    out
}

/// Whether `m` already gives way to insert `key`: an active effect keyed from it that ducks — a
/// Compressor (classic sidechain ducking) or an EQ with a dynamic band (the mix assistant's fix).
pub fn keyed_from(m: &crate::model::Insert, key: Id) -> bool {
    m.fx.iter().any(|f| {
        !f.bypass
            && f.key == Some(key)
            && match f.kind {
                FxKind::Comp => true,
                FxKind::Eq => f.bands.as_ref().is_some_and(|bs| bs.iter().any(|b| b.on && b.dynamic.is_some())),
                _ => false,
            }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(f: f32, amp: f32, sr: f32, n: usize, start: usize) -> Vec<f32> {
        (0..n).map(|i| amp * (std::f32::consts::TAU * f * (start + i) as f32 / sr).sin()).collect()
    }

    #[test]
    fn a_full_scale_sine_reads_about_zero_db_in_its_band() {
        let sr = 48000.0;
        let mut d = Detector::new(sr);
        d.lanes = vec![Lane::new()];
        d.ids = vec![0];
        let x = sine(band_hz(10), 1.0, sr, FFT_N, 0);
        d.lanes[0].buf.copy_from_slice(&x);
        let mut out = [0.0; NB];
        d.fft.run(&d.lanes[0], &mut out);
        assert!((out[10] - 0.0).abs() < 1.5, "band 10 reads {}", out[10]);
        assert!(out[16] < -40.0, "a far band reads {}", out[16]);
    }

    #[test]
    fn band_centres_cover_the_audible_range() {
        assert!((band_hz(0) - 31.5).abs() < 0.01);
        assert!(band_hz(NB - 1) > 15000.0 && band_hz(NB - 1) < 17000.0);
    }
}
