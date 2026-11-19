//! Multiband: upward and downward compression in three bands at once, the "OTT" sound.
//!
//! The signal is split at two Linkwitz-Riley crossovers (`dsp::xover`, 88 Hz and 2.5 kHz by
//! default: sub against low-mids, mids against the top, as in Ableton's OTT preset and Xfer
//! OTT). In each band one stereo-linked RMS detector (5 ms) drives two gain computers with
//! soft knees: above the downward threshold an almost brick-wall ratio (66.7:1, ∞:1 in the high
//! band) pulls peaks down; below the upward threshold a 4.17:1 ratio lifts quiet detail (tails,
//! breath, reverb, air) up. Both thresholds sit low, so most material is squeezed from both
//! sides into a narrow window: every band ends up dense and the bands even out against each
//! other — loud, bright and in your face.
//!
//! Two things OTT is known for are fixed here:
//! * the upward gain is capped (+18 dB) and fades to nothing below a floor (−60 dBFS in the
//!   band), so silence and room noise between phrases are not pulled up into hiss, and a note
//!   after silence does not start with a jump of +40 dB;
//! * attack and release are in seconds at every sample rate (OTT counts part of its release in
//!   samples, so it releases twice as fast at 96 kHz).
//!
//! **Depth** blends the processed bands with the dry signal. The dry path runs through the same
//! two all-passes the band sum has (`Ap2`), so at 50 % there is no comb notch at the crossovers
//! (a plain dry/wet over crossover bands has one). **Time** scales every attack and release
//! (Ableton OTT: attack 47.8 / 22.4 / 13.5 ms, release 282 / 282 / 132 ms low / mid / high).
//! **Upward** and **Downward** scale each side's gain (0–200 %). In / Out are the drive into
//! the compressors and the level of the processed sound.
//!
//! The gain is smoothed in dB: towards more reduction (or less lift) at the attack time, back
//! at the release time. Nothing allocates after `prepare`.

use super::xover::{Ap2, Lr4};
use crate::processor::{ParamBlock, ProcessCtx, Processor, METERS};

/// Parameter order (= catalog.rs MULTIBAND).
pub mod idx {
    pub const DEPTH: usize = 0;
    pub const TIME: usize = 1;
    pub const UP: usize = 2;
    pub const DOWN: usize = 3;
    pub const IN: usize = 4;
    pub const OUT: usize = 5;
    pub const X_LO: usize = 6;
    pub const X_HI: usize = 7;
    /// band output gains, low / mid / high
    pub const GAIN: usize = 8;
    /// downward thresholds, low / mid / high
    pub const DOWN_THR: usize = 11;
    /// upward thresholds, low / mid / high
    pub const UP_THR: usize = 14;
}

const BANDS: usize = 3;
/// Base attack / release, seconds (Time = 100 %).
const ATTACK: [f32; BANDS] = [0.0478, 0.0224, 0.0135];
const RELEASE: [f32; BANDS] = [0.282, 0.282, 0.132];
/// Downward ratio (1/R − 1 in the gain computer): 66.7:1, 66.7:1, ∞:1.
const DOWN_SLOPE: [f32; BANDS] = [1.0 / 66.7 - 1.0, 1.0 / 66.7 - 1.0, -1.0];
/// Upward ratio 4.17:1 → 1 − 1/R.
const UP_SLOPE: f32 = 1.0 - 1.0 / 4.17;
/// Soft knee width, dB.
const KNEE: f32 = 6.0;
/// Drive into the band compressors (Ableton OTT: +5.2 dB per band).
const BAND_IN_DB: f32 = 5.2;
/// Make-up per band at Out 0 dB. The shape is Ableton OTT's band outputs (+10.3 / +5.7 / +10.3:
/// lows and highs 4.6 dB over the mids, the "smile"); the whole set is 6.5 dB higher, so pink noise
/// at −14 dBFS RMS (a loud element) comes out about 1 dB louder at 100 % depth instead of 5 dB
/// quieter (measured, `calib` below). Everything above −30 dBFS lands near −13 dBFS RMS.
pub const MAKEUP_DB: [f32; BANDS] = [16.8, 12.2, 16.8];
/// Largest upward gain, dB, and the band level below which it fades out.
const UP_CAP: f32 = 18.0;
const FLOOR: f32 = -60.0;
const FLOOR_FADE: f32 = 18.0;
/// RMS detector time, seconds.
const RMS_S: f32 = 0.005;

#[derive(Clone, Copy, Default)]
struct Smooth {
    v: f32,
    target: f32,
}

impl Smooth {
    #[inline(always)]
    fn tick(&mut self, k: f32) -> f32 {
        self.v += (self.target - self.v) * k;
        self.v
    }
}

#[inline(always)]
fn db_to_lin(db: f32) -> f32 {
    // exp is cheaper than powf
    (db * (std::f32::consts::LN_10 / 20.0)).exp()
}

/// Gain of the downward side at level `x` dB (≤ 0): slope `s` = 1/R − 1 above `t`, soft knee.
#[inline(always)]
pub fn down_gain(x: f32, t: f32, s: f32) -> f32 {
    let o = x - t;
    if 2.0 * o <= -KNEE {
        0.0
    } else if 2.0 * o < KNEE {
        let u = o + KNEE / 2.0;
        s * u * u / (2.0 * KNEE)
    } else {
        s * o
    }
}

/// Gain of the upward side at level `x` dB (≥ 0): lift (1 − 1/R)·(t − x) below `t`, soft knee.
#[inline(always)]
pub fn up_gain(x: f32, t: f32) -> f32 {
    let o = t - x;
    if 2.0 * o <= -KNEE {
        0.0
    } else if 2.0 * o < KNEE {
        let u = o + KNEE / 2.0;
        UP_SLOPE * u * u / (2.0 * KNEE)
    } else {
        UP_SLOPE * o
    }
}

/// 0 below `FLOOR − FLOOR_FADE`, 1 above `FLOOR`, smooth between.
#[inline(always)]
fn floor_weight(x: f32) -> f32 {
    let u = ((x - (FLOOR - FLOOR_FADE)) / FLOOR_FADE).clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

pub struct Multiband {
    sr: f32,
    k: f32,
    k_rms: f32,
    depth: Smooth,
    up: Smooth,
    down: Smooth,
    in_g: Smooth,
    out_g: Smooth,
    band_db: [Smooth; BANDS],
    down_thr: [Smooth; BANDS],
    up_thr: [Smooth; BANDS],
    time: f32,
    att: [f32; BANDS],
    rel: [f32; BANDS],
    x_lo: f32,
    x_hi: f32,
    /// crossover frequencies the filters are set to (glide per block)
    f_lo: f32,
    f_hi: f32,
    split_lo: [Lr4; 2],
    split_hi: [Lr4; 2],
    low_ap: [Ap2; 2],
    dry_ap: [[Ap2; 2]; 2],
    rms: [f32; BANDS],
    gain: [f32; BANDS],
    // meters: band level in, gain, band level out (dB), smoothed for display
    m_in: [f32; BANDS],
    m_out: [f32; BANDS],
    primed: bool,
}

impl Default for Multiband {
    fn default() -> Self {
        Self {
            sr: 48_000.0,
            k: 0.002,
            k_rms: 0.004,
            depth: Smooth::default(),
            up: Smooth::default(),
            down: Smooth::default(),
            in_g: Smooth { v: 1.0, target: 1.0 },
            out_g: Smooth { v: 1.0, target: 1.0 },
            band_db: [Smooth::default(); BANDS],
            down_thr: [Smooth::default(); BANDS],
            up_thr: [Smooth::default(); BANDS],
            time: 1.0,
            att: [0.0; BANDS],
            rel: [0.0; BANDS],
            x_lo: 88.3,
            x_hi: 2500.0,
            f_lo: 88.3,
            f_hi: 2500.0,
            split_lo: [Lr4::default(); 2],
            split_hi: [Lr4::default(); 2],
            low_ap: [Ap2::default(); 2],
            dry_ap: [[Ap2::default(); 2]; 2],
            rms: [0.0; BANDS],
            gain: [0.0; BANDS],
            m_in: [-120.0; BANDS],
            m_out: [-120.0; BANDS],
            primed: false,
        }
    }
}

impl Multiband {
    fn set_filters(&mut self) {
        let (sr, lo, hi) = (f64::from(self.sr), f64::from(self.f_lo), f64::from(self.f_hi));
        for c in 0..2 {
            self.split_lo[c].set(lo, sr);
            self.split_hi[c].set(hi, sr);
            self.low_ap[c].set(hi, sr);
            self.dry_ap[c][0].set(lo, sr);
            self.dry_ap[c][1].set(hi, sr);
        }
    }

    fn set_times(&mut self) {
        let sr = self.sr;
        for b in 0..BANDS {
            self.att[b] = 1.0 - (-1.0 / (ATTACK[b] * self.time * sr)).exp();
            self.rel[b] = 1.0 - (-1.0 / (RELEASE[b] * self.time * sr)).exp();
        }
    }
}

impl Processor for Multiband {
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sr = sample_rate.max(8000.0);
        self.k = 1.0 - (-1.0 / (0.01 * self.sr)).exp();
        self.k_rms = 1.0 - (-1.0 / (RMS_S * self.sr)).exp();
        self.f_lo = self.x_lo;
        self.f_hi = self.x_hi;
        self.set_filters();
        self.set_times();
        self.reset();
    }

    fn set_params(&mut self, p: &ParamBlock) {
        let v = &p.values;
        self.depth.target = (v[idx::DEPTH] / 100.0).clamp(0.0, 1.0);
        self.up.target = (v[idx::UP] / 100.0).clamp(0.0, 2.0);
        self.down.target = (v[idx::DOWN] / 100.0).clamp(0.0, 2.0);
        self.in_g.target = db_to_lin(v[idx::IN].clamp(-24.0, 24.0));
        self.out_g.target = db_to_lin(v[idx::OUT].clamp(-24.0, 24.0));
        for b in 0..BANDS {
            self.band_db[b].target = v[idx::GAIN + b].clamp(-24.0, 24.0);
            self.down_thr[b].target = v[idx::DOWN_THR + b].clamp(-72.0, 0.0);
            self.up_thr[b].target = v[idx::UP_THR + b].clamp(-90.0, 0.0);
        }
        let time = (v[idx::TIME] / 100.0).clamp(0.1, 10.0);
        if (time - self.time).abs() > 1e-6 {
            self.time = time;
            self.set_times();
        }
        let lo = v[idx::X_LO].clamp(30.0, 1000.0);
        let hi = v[idx::X_HI].clamp(lo * 2.0, 16_000.0);
        self.x_lo = lo;
        self.x_hi = hi;
        if !self.primed {
            self.primed = true;
            for s in [&mut self.depth, &mut self.up, &mut self.down, &mut self.in_g, &mut self.out_g] {
                s.v = s.target;
            }
            for b in 0..BANDS {
                for s in [&mut self.band_db[b], &mut self.down_thr[b], &mut self.up_thr[b]] {
                    s.v = s.target;
                }
            }
            self.f_lo = lo;
            self.f_hi = hi;
            self.set_filters();
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessCtx) {
        let n = l.len().min(r.len());
        // crossovers glide (log) towards their targets
        if (self.f_lo - self.x_lo).abs() > 0.01 || (self.f_hi - self.x_hi).abs() > 0.01 {
            let a = 1.0 - (-(n as f32 / self.sr) / 0.03).exp();
            let glide = |f: f32, t: f32| {
                let v = (f.ln() + (t.ln() - f.ln()) * a).exp();
                if (v - t).abs() < 0.05 {
                    t
                } else {
                    v
                }
            };
            self.f_lo = glide(self.f_lo, self.x_lo);
            self.f_hi = glide(self.f_hi, self.x_hi);
            self.set_filters();
        }
        let k = self.k;
        let kr = self.k_rms;
        let band_in = db_to_lin(BAND_IN_DB);
        let mut level_peak = [-120f32; BANDS];
        let mut out_peak = [0f32; BANDS];
        for i in 0..n {
            let depth = self.depth.tick(k);
            let up = self.up.tick(k);
            let down = self.down.tick(k);
            let gi = self.in_g.tick(k);
            let go = self.out_g.tick(k);
            let x = [f64::from(l[i]), f64::from(r[i])];
            let mut dry = [0f64; 2];
            let mut bands = [[0f64; BANDS]; 2];
            for c in 0..2 {
                let d0 = self.dry_ap[c][0].run(x[c]);
                dry[c] = self.dry_ap[c][1].run(d0);
                let (lo, rest) = self.split_lo[c].split(x[c] * f64::from(gi));
                let (mid, hi) = self.split_hi[c].split(rest);
                bands[c] = [self.low_ap[c].run(lo), mid, hi];
            }
            let (mut wl, mut wr) = (0f32, 0f32);
            for b in 0..BANDS {
                let (bl, br) = (bands[0][b] as f32 * band_in, bands[1][b] as f32 * band_in);
                let p = 0.5 * (bl * bl + br * br);
                self.rms[b] += (p - self.rms[b]) * kr;
                let lvl = 10.0 * (self.rms[b] + 1e-20).log10();
                let thr_d = self.down_thr[b].tick(k);
                let thr_u = self.up_thr[b].tick(k);
                let g_down = down_gain(lvl, thr_d, DOWN_SLOPE[b]) * down;
                let g_up = (up_gain(lvl, thr_u) * up).min(UP_CAP * up) * floor_weight(lvl);
                let target = g_down + g_up;
                let c = if target < self.gain[b] { self.att[b] } else { self.rel[b] };
                self.gain[b] += (target - self.gain[b]) * c;
                let g = db_to_lin(self.gain[b] + MAKEUP_DB[b] + self.band_db[b].tick(k)) * go;
                wl += bl * g;
                wr += br * g;
                if lvl > level_peak[b] {
                    level_peak[b] = lvl;
                }
                let o = (bl * g).abs().max((br * g).abs());
                if o > out_peak[b] {
                    out_peak[b] = o;
                }
            }
            l[i] = (dry[0] as f32) * (1.0 - depth) + wl * depth;
            r[i] = (dry[1] as f32) * (1.0 - depth) + wr * depth;
        }
        if self.rms.iter().any(|v| *v < 1e-30) {
            for v in &mut self.rms {
                if *v < 1e-30 {
                    *v = 0.0;
                }
            }
        }
        // display: band level (fast up, ~300 ms down)
        let fall = (-(n as f32 / self.sr) / 0.3).exp();
        for b in 0..BANDS {
            let lv = level_peak[b].max(-120.0);
            self.m_in[b] = if lv > self.m_in[b] { lv } else { lv + (self.m_in[b] - lv) * fall };
            let o = 20.0 * (out_peak[b] + 1e-9).log10();
            self.m_out[b] = if o > self.m_out[b] { o } else { o + (self.m_out[b] - o) * fall };
        }
    }

    fn reset(&mut self) {
        for c in 0..2 {
            self.split_lo[c].clear();
            self.split_hi[c].clear();
            self.low_ap[c].clear();
            self.dry_ap[c][0].clear();
            self.dry_ap[c][1].clear();
        }
        self.rms = [0.0; BANDS];
        self.gain = [0.0; BANDS];
        self.m_in = [-120.0; BANDS];
        self.m_out = [-120.0; BANDS];
    }

    fn tail_samples(&self) -> Option<usize> {
        // filters ring for a few ms; the envelopes settle within the longest release
        Some((RELEASE[0] * self.time * 6.0 * self.sr) as usize)
    }

    fn gain_reduction_db(&self) -> f32 {
        self.gain.iter().copied().fold(0.0, f32::min)
    }

    fn meters(&self, out: &mut [f32; METERS]) {
        out[..BANDS].copy_from_slice(&self.m_in);
        out[3..3 + BANDS].copy_from_slice(&self.gain);
        out[6..6 + BANDS].copy_from_slice(&self.m_out);
        out[9] = self.f_lo;
        out[10] = self.f_hi;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processor::MAX_PARAMS;

    pub fn defaults() -> ParamBlock {
        let mut p = ParamBlock { values: [0.0; MAX_PARAMS], ..Default::default() };
        let v = &mut p.values;
        v[idx::DEPTH] = 100.0;
        v[idx::TIME] = 100.0;
        v[idx::UP] = 100.0;
        v[idx::DOWN] = 100.0;
        v[idx::X_LO] = 88.3;
        v[idx::X_HI] = 2500.0;
        v[idx::DOWN_THR..idx::DOWN_THR + 3].copy_from_slice(&[-33.8, -30.3, -35.5]);
        v[idx::UP_THR..idx::UP_THR + 3].copy_from_slice(&[-40.8, -41.8, -40.8]);
        p
    }

    fn pink(n: usize, amp: f32) -> Vec<f32> {
        // Paul Kellet's economy pink filter on white noise
        let mut seed = 99u32;
        let (mut b0, mut b1, mut b2) = (0f32, 0f32, 0f32);
        (0..n)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let w = (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
                b0 = 0.99765 * b0 + w * 0.0990460;
                b1 = 0.96300 * b1 + w * 0.2965164;
                b2 = 0.57000 * b2 + w * 1.0526913;
                (b0 + b1 + b2 + w * 0.1848) * amp
            })
            .collect()
    }

    pub fn pink_db(n: usize, db: f32) -> Vec<f32> {
        let x = pink(n, 1.0);
        let g = 10f32.powf(db / 20.0) / 10f32.powf(rms_db(&x) / 20.0);
        x.iter().map(|v| v * g).collect()
    }

    pub fn rms(x: &[f32]) -> f32 {
        rms_db(x)
    }

    fn rms_db(x: &[f32]) -> f32 {
        10.0 * (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).log10()
    }

    fn run(m: &mut Multiband, l: &mut [f32], r: &mut [f32]) {
        let ctx = ProcessCtx { sample_rate: 48_000.0, bpm: 120.0, beat: 0.0, playing: true };
        for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
            m.process(a, b, &ctx);
        }
    }

    #[test]
    fn depth_zero_is_the_input_through_two_allpasses() {
        let mut m = Multiband::default();
        m.prepare(48_000.0, 512);
        let mut p = defaults();
        p.values[idx::DEPTH] = 0.0;
        m.set_params(&p);
        let x = pink(48_000, 0.1);
        let (mut l, mut r) = (x.clone(), x.clone());
        run(&mut m, &mut l, &mut r);
        // power is kept (all-pass), only the phase turns
        assert!((rms_db(&l[24_000..]) - rms_db(&x[24_000..])).abs() < 0.05);
    }

    #[test]
    fn neutral_settings_sum_flat_at_half_depth() {
        // no compression and the bands back at unity: 50 % depth must not notch the crossovers
        let mut m = Multiband::default();
        m.prepare(48_000.0, 512);
        let mut p = defaults();
        p.values[idx::DEPTH] = 50.0;
        p.values[idx::UP] = 0.0;
        p.values[idx::DOWN] = 0.0;
        for (v, m) in p.values[idx::GAIN..idx::GAIN + 3].iter_mut().zip(MAKEUP_DB) {
            *v = -(m + BAND_IN_DB);
        }
        m.set_params(&p);
        for hz in [40.0f32, 88.3, 300.0, 2500.0, 9000.0] {
            m.reset();
            let mut l: Vec<f32> = (0..48_000).map(|i| (std::f32::consts::TAU * hz * i as f32 / 48_000.0).sin() * 0.25).collect();
            let mut r = l.clone();
            run(&mut m, &mut l, &mut r);
            let g = rms_db(&l[24_000..]) - 20.0 * (0.25 / 2f32.sqrt()).log10();
            assert!(g.abs() < 0.05, "{hz} Hz: {g} dB");
        }
    }

    #[test]
    fn evens_out_loud_and_quiet_and_keeps_a_mix_level() {
        // 24 dB between a loud and a quiet stretch shrinks to a few dB; a loud element
        // comes out near its own level (the make-up is calibrated for it)
        let mut m = Multiband::default();
        m.prepare(48_000.0, 512);
        m.set_params(&defaults());
        let loud = pink_db(48_000, -14.0);
        let quiet = pink_db(48_000, -38.0);
        let mut l: Vec<f32> = loud.iter().chain(quiet.iter()).copied().collect();
        let mut r = l.clone();
        run(&mut m, &mut l, &mut r);
        let out_loud = rms_db(&l[24_000..48_000]);
        let out_quiet = rms_db(&l[72_000..]);
        assert!(out_loud - out_quiet < 8.0, "loud {out_loud} quiet {out_quiet}");
        assert!((out_loud + 14.0).abs() < 3.0, "loud out {out_loud}");
    }

    #[test]
    fn noise_below_the_floor_gets_no_upward_lift() {
        // quiet detail (−62 dBFS) is lifted a lot; hiss (−95 dBFS) only by the fixed make-up
        let lift = |db: f32| {
            let mut m = Multiband::default();
            m.prepare(48_000.0, 512);
            m.set_params(&defaults());
            let x = pink_db(48_000, db);
            let (mut l, mut r) = (x.clone(), x.clone());
            run(&mut m, &mut l, &mut r);
            rms_db(&l[24_000..]) - db
        };
        let (detail, hiss) = (lift(-62.0), lift(-95.0));
        assert!(detail - hiss > 10.0, "detail +{detail} dB, hiss +{hiss} dB");
        assert!(hiss < MAKEUP_DB[0] + BAND_IN_DB + 1.0, "hiss +{hiss} dB");
    }

    #[test]
    fn gain_computers() {
        // well above / below threshold: the straight lines; at the threshold: the knee
        assert!((down_gain(-10.0, -30.0, -1.0) + 20.0).abs() < 1e-4);
        assert_eq!(down_gain(-40.0, -30.0, -1.0), 0.0);
        assert!((up_gain(-60.0, -40.0) - 20.0 * UP_SLOPE).abs() < 1e-4);
        assert_eq!(up_gain(-30.0, -40.0), 0.0);
        assert!(down_gain(-30.0, -30.0, -1.0) < 0.0 && up_gain(-40.0, -40.0) > 0.0);
    }
}

#[cfg(test)]
mod calib {
    use super::tests::*;
    use super::*;
    #[test]
    #[ignore]
    fn print_levels() {
        for db in [-40.0f32, -30.0, -24.0, -18.0, -14.0, -10.0, -6.0] {
            let mut m = Multiband::default();
            m.prepare(48_000.0, 512);
            m.set_params(&defaults());
            let x = pink_db(96_000, db);
            let (mut l, mut r) = (x.clone(), x.clone());
            let ctx = ProcessCtx { sample_rate: 48_000.0, bpm: 120.0, beat: 0.0, playing: true };
            for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
                m.process(a, b, &ctx);
            }
            println!("in {db} -> out {:.2}", rms(&l[48_000..]));
        }
    }
}
