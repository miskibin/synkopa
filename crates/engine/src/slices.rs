//! Where to slice a sample: onset (transient) detection for the sampler's slice mode.
//!
//! SuperFlux (Böck & Widmer, DAFx 2013), the strongest non-neural onset detector for mixed
//! material: the magnitude spectrum (2048-point Hann, 5 ms hop) goes through a filterbank of
//! 24 bands per octave and log compression, and each band is compared with the *maximum* of its
//! neighbours one frame earlier (μ = 2 frames back, ±1 band). A vibrato or a slide moves energy
//! between neighbouring bands; the maximum filter absorbs that, so only new energy counts.
//! The positive differences summed over the bands give the onset strength per frame.
//!
//! Every local peak of that curve (±30 ms) that stands above the local mean (100 ms before,
//! 50 ms after) is a candidate, returned with its strength normalised to the loudest one. The
//! UI keeps the candidates above a threshold set by its Sensitivity slider, so moving the slider
//! never re-analyses the file. Each candidate's position is moved back from the analysis frame
//! to where the attack really starts in the waveform (the first millisecond whose level rises
//! past a fifth of the hit's peak), minus 1 ms, so a slice never begins after its transient.

use realfft::RealFftPlanner;

use crate::sources::Audio;

const N: usize = 2048;
const BANDS_PER_OCT: f32 = 24.0;
const F_LO: f32 = 30.0;
const F_HI: f32 = 16_000.0;

/// One candidate slice point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Onset {
    /// Position as a fraction of the file (0..1).
    pub pos: f32,
    /// Strength 0..1 (1 = the strongest transient of the file).
    pub strength: f32,
}

/// Mono mix of a source (no padding).
fn mono(a: &Audio) -> Vec<f32> {
    let lead = wave_audio_file::PAD_LEAD;
    (0..a.frames).map(|i| 0.5 * (a.data[2 * (lead + i)] + a.data[2 * (lead + i) + 1])).collect()
}

/// Triangular filters on a log axis: for each band, (first bin, weights).
fn filterbank(sr: f32) -> Vec<(usize, Vec<f32>)> {
    let bin_hz = sr / N as f32;
    let hi = F_HI.min(sr * 0.45);
    let n = (BANDS_PER_OCT * (hi / F_LO).log2()).floor() as usize;
    let centre = |k: usize| F_LO * 2f32.powf(k as f32 / BANDS_PER_OCT);
    let mut out = vec![];
    let mut last_lo = usize::MAX;
    for k in 1..n.saturating_sub(1) {
        let (f0, f1, f2) = (centre(k - 1), centre(k), centre(k + 1));
        let (b0, b1, b2) = ((f0 / bin_hz).round() as usize, (f1 / bin_hz).round() as usize, (f2 / bin_hz).round() as usize);
        // at the bottom several bands fall on the same bins: keep one of each
        if b2 <= b0 + 1 || b0 == last_lo {
            continue;
        }
        last_lo = b0;
        let w: Vec<f32> = (b0..=b2)
            .map(|b| if b <= b1 { (b - b0) as f32 / (b1 - b0).max(1) as f32 } else { (b2 - b) as f32 / (b2 - b1).max(1) as f32 })
            .collect();
        let sum: f32 = w.iter().sum();
        out.push((b0, w.into_iter().map(|x| x / sum.max(1e-9)).collect()));
    }
    out
}

/// Onset strength per frame and the hop in samples.
fn strength(x: &[f32], sr: f32) -> (Vec<f32>, usize) {
    let hop = (sr / 200.0).round().max(1.0) as usize;
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(N);
    let win: Vec<f32> = (0..N).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos()).collect();
    let mut buf = vec![0f32; N];
    let mut spec = fft.make_output_vec();
    let fb = filterbank(sr);
    let frames = x.len() / hop + 1;
    let mut bands: Vec<Vec<f32>> = Vec::with_capacity(frames);
    for f in 0..frames {
        // frame centred on f·hop
        let c = (f * hop) as isize - (N / 2) as isize;
        for (i, b) in buf.iter_mut().enumerate() {
            let j = c + i as isize;
            *b = if j >= 0 && (j as usize) < x.len() { x[j as usize] * win[i] } else { 0.0 };
        }
        let _ = fft.process(&mut buf, &mut spec);
        bands.push(
            fb.iter()
                .map(|(b0, w)| {
                    let e: f32 = w.iter().enumerate().map(|(k, &wk)| wk * spec.get(b0 + k).map_or(0.0, |z| z.norm())).sum();
                    (1.0 + e).ln()
                })
                .collect(),
        );
    }
    let mu = 2;
    let mut sf = vec![0f32; frames];
    for f in mu..frames {
        let (cur, prev) = (&bands[f], &bands[f - mu]);
        let nb = cur.len();
        let mut s = 0.0;
        for b in 0..nb {
            let m = prev[b].max(if b > 0 { prev[b - 1] } else { 0.0 }).max(if b + 1 < nb { prev[b + 1] } else { 0.0 });
            let d = cur[b] - m;
            if d > 0.0 {
                s += d;
            }
        }
        sf[f] = s;
    }
    (sf, hop)
}

/// Where the attack of a hit found near sample `at` starts in the waveform.
fn attack_start(x: &[f32], at: usize, sr: f32) -> usize {
    let ms = (sr / 1000.0).max(1.0) as usize;
    // the analysis frame is centred (it can see an attack up to half a window, 23 ms, early) and
    // the maximum filter can make it a few ms late: look on both sides
    let from = at.saturating_sub(25 * ms);
    let to = (at + 35 * ms).min(x.len());
    if to <= from + 4 * ms {
        return at.min(x.len().saturating_sub(1));
    }
    // level per quarter millisecond (peak over the next millisecond)
    let step = (ms / 4).max(1);
    let level = |i: usize| x[i..(i + ms).min(x.len())].iter().fold(0f32, |m, v| m.max(v.abs()));
    let pts: Vec<(usize, f32)> = (from..to).step_by(step).map(|i| (i, level(i))).collect();
    let (pk, peak) = pts.iter().enumerate().fold((0, 0f32), |b, (k, &(_, v))| if v > b.1 { (k, v) } else { b });
    if peak <= 1e-6 {
        return at;
    }
    // the quietest point before the peak is where the previous sound had faded most: the attack
    // rises from there (a tail or a pad underneath is not part of it)
    let (vk, valley) = pts[..=pk].iter().enumerate().fold((0, f32::MAX), |b, (k, &(_, v))| if v <= b.1 { (k, v) } else { b });
    if peak <= valley * 1.2 {
        return at;
    }
    // walk back from the peak while the level is still above the rise threshold: the attack
    // closest to the peak, never a bump of the tail before it
    let thr = valley + 0.15 * (peak - valley);
    let mut k = pk;
    while k > vk && pts[k - 1].1 > thr {
        k -= 1;
    }
    pts[k].0.saturating_sub(ms)
}

/// Candidate slice points of `a`, in order, strongest = 1. A slice at the very start is not
/// listed (the first slice always starts at 0).
pub fn onsets(a: &Audio) -> Vec<Onset> {
    let x = mono(a);
    if x.len() < N / 2 {
        return vec![];
    }
    let sr = a.sr as f32;
    let (sf, hop) = strength(&x, sr);
    let fps = sr / hop as f32;
    let w_max = (0.03 * fps).round() as usize;
    let (pre, post) = ((0.1 * fps).round() as usize, (0.05 * fps).round() as usize);
    let top = sf.iter().copied().fold(0f32, f32::max);
    if top <= 1e-9 {
        return vec![];
    }
    let mut out: Vec<Onset> = vec![];
    for f in 0..sf.len() {
        let lo = f.saturating_sub(w_max);
        let hi = (f + w_max + 1).min(sf.len());
        if sf[f] <= 0.0 || sf[lo..hi].iter().any(|&v| v > sf[f]) {
            continue;
        }
        let (mlo, mhi) = (f.saturating_sub(pre), (f + post + 1).min(sf.len()));
        let mean = sf[mlo..mhi].iter().sum::<f32>() / (mhi - mlo) as f32;
        let s = (sf[f] - mean) / top;
        if s <= 0.01 {
            continue;
        }
        let at = attack_start(&x, f * hop, sr);
        let pos = at as f32 / x.len() as f32;
        if pos < 0.002 {
            continue;
        }
        // two peaks on one attack: keep the stronger
        if let Some(last) = out.last_mut() {
            if (pos - last.pos) * (x.len() as f32) < 0.02 * sr {
                if s > last.strength {
                    *last = Onset { pos, strength: s };
                }
                continue;
            }
        }
        out.push(Onset { pos, strength: s });
    }
    let m = out.iter().map(|o| o.strength).fold(0f32, f32::max).max(1e-9);
    for o in &mut out {
        o.strength = (o.strength / m).clamp(0.0, 1.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hits at the given seconds: a kick (falling sine), then noise bursts, over a quiet pad.
    fn loop_with_hits(sr: u32, hits: &[f32], len: f32) -> Audio {
        let n = (len * sr as f32) as usize;
        let mut seed = 5u32;
        let mut x = vec![0f32; n];
        for (i, v) in x.iter_mut().enumerate() {
            // a steady chord underneath: must not make slices of its own
            let t = i as f32 / sr as f32;
            *v = 0.05 * ((std::f32::consts::TAU * 220.0 * t).sin() + (std::f32::consts::TAU * 277.0 * t).sin());
        }
        for (k, &h) in hits.iter().enumerate() {
            let s0 = (h * sr as f32) as usize;
            for i in 0..(0.25 * sr as f32) as usize {
                if s0 + i >= n {
                    break;
                }
                let t = i as f32 / sr as f32;
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5;
                x[s0 + i] += if k % 2 == 0 {
                    0.8 * (std::f32::consts::TAU * (50.0 + 120.0 * (-t * 30.0).exp()) * t).sin() * (-t * 8.0).exp()
                } else {
                    0.7 * noise * (-t * 25.0).exp()
                };
            }
        }
        let inter: Vec<f32> = x.iter().flat_map(|&v| [v, v]).collect();
        Audio::from_stereo(inter, sr, 2)
    }

    #[test]
    fn finds_each_hit_close_to_where_it_starts() {
        let sr = 44_100;
        let hits = [0.5f32, 0.875, 1.25, 1.625, 2.0, 2.375, 2.75, 3.125];
        let a = loop_with_hits(sr, &hits, 3.6);
        let found = onsets(&a);
        let strong: Vec<f32> = found.iter().filter(|o| o.strength > 0.15).map(|o| o.pos * 3.6).collect();
        assert_eq!(strong.len(), hits.len(), "found {strong:?}");
        for (h, f) in hits.iter().zip(&strong) {
            // never after the attack, at most a few ms before it
            assert!(*f <= h + 0.002 && *f >= h - 0.015, "hit {h} s, slice at {f} s");
        }
    }
}
