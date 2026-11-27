//! Pitch-preserving time-stretch of a whole buffer, offline.
//!
//! Wraps [Signalsmith Stretch](https://github.com/Signalsmith-Audio/signalsmith-stretch)
//! (MIT, vendored in `vendor/`). Never call this from the audio callback: it allocates
//! and does work proportional to the buffer.

use std::os::raw::c_int;

extern "C" {
    fn wave_stretch_exact(
        sample_rate: f32,
        channels: c_int,
        input: *const *const f32,
        in_len: usize,
        output: *const *mut f32,
        out_len: usize,
        semitones: f32,
    ) -> c_int;
    fn wave_stretch_warp(
        sample_rate: f32,
        channels: c_int,
        input: *const *const f32,
        in_len: usize,
        output: *const *mut f32,
        out_len: usize,
        anchors: *const f64,
        n_anchors: usize,
        semitones: f32,
    ) -> c_int;
    fn wave_stretch_min_output(sample_rate: f32, channels: c_int) -> usize;
}

/// Shortest and longest supported ratio (output length / input length).
pub const MIN_RATIO: f64 = 0.25;
pub const MAX_RATIO: f64 = 4.0;

/// One piecewise anchor: input frame `at` plays at output frame `to`. Both are exact
/// frame positions; the stretch between two anchors is the segment's slope.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WarpAnchor {
    pub at: f64,
    pub to: f64,
}

/// Piecewise time-stretch through `anchors` (strictly increasing, starting `(0, 0)`,
/// ending at the full output length), keeping pitch and transposing by `semitones` in
/// the same pass. The result has exactly `anchors.last().to.round()` frames and no seams
/// at the anchors — one continuous stretcher serves the whole map.
pub fn stretch_warp_stereo(input: &[f32], sr: u32, anchors: &[WarpAnchor], semitones: f32) -> Result<Vec<f32>, String> {
    if !(semitones.is_finite() && semitones.abs() <= MAX_SEMITONES + 1e-4) {
        return Err(format!("transposition {semitones} is outside ±{MAX_SEMITONES} semitones"));
    }
    if anchors.len() < 2 || anchors[0].at != 0.0 || anchors[0].to != 0.0 {
        return Err("warp anchors must start at frame 0 on both sides".into());
    }
    let mut last = (0.0f64, 0.0f64);
    for a in &anchors[1..] {
        if !(a.at.is_finite() && a.to.is_finite()) {
            return Err("warp anchors must be finite".into());
        }
        if a.at <= last.0 || a.to <= last.1 {
            return Err("warp anchors must increase on both sides".into());
        }
        let r = (a.to - last.1) / (a.at - last.0);
        if !(MIN_RATIO - 1e-9..=MAX_RATIO + 1e-9).contains(&r) {
            return Err(format!("a warp segment stretches by {r:.3}, outside {MIN_RATIO}–{MAX_RATIO}"));
        }
        last = (a.at, a.to);
    }
    let frames = input.len() / 2;
    if frames == 0 {
        return Ok(vec![]);
    }
    if frames as f64 + 0.5 < last.0 {
        return Err("warp anchors reach past the audio".into());
    }
    let out_len = last.1.round() as usize;
    let srf = sr.max(1) as f32;
    let mut l = vec![0.0f32; frames];
    let mut r = vec![0.0f32; frames];
    for (i, [a, b]) in input.as_chunks::<2>().0.iter().enumerate() {
        l[i] = *a;
        r[i] = *b;
    }
    let mut ol = vec![0.0f32; out_len];
    let mut or = vec![0.0f32; out_len];
    let flat: Vec<f64> = anchors.iter().flat_map(|a| [a.at, a.to]).collect();
    let ins = [l.as_ptr(), r.as_ptr()];
    let outs = [ol.as_mut_ptr(), or.as_mut_ptr()];
    let rc = unsafe { wave_stretch_warp(srf, 2, ins.as_ptr(), frames, outs.as_ptr(), out_len, flat.as_ptr(), anchors.len(), semitones) };
    match rc {
        0 => {}
        1 | 3 => return Err("audio too short to stretch".into()),
        _ => return Err("not enough memory to stretch".into()),
    }
    let mut out = Vec::with_capacity(out_len * 2);
    for i in 0..out_len {
        let (a, b) = (ol[i], or[i]);
        out.push(if a.is_finite() { a } else { 0.0 });
        out.push(if b.is_finite() { b } else { 0.0 });
    }
    Ok(out)
}

/// Output length in frames for `frames` stretched by `ratio`.
pub fn out_frames(frames: usize, ratio: f64) -> usize {
    ((frames as f64) * ratio).round().max(0.0) as usize
}

/// Stretches interleaved stereo `input` at `sr` to `ratio` times its length, keeping
/// pitch. The result has exactly [`out_frames`] frames.
pub fn stretch_stereo(input: &[f32], sr: u32, ratio: f64) -> Result<Vec<f32>, String> {
    stretch_shift_stereo(input, sr, ratio, 0.0)
}

/// Farthest supported transposition, in semitones either way.
pub const MAX_SEMITONES: f32 = 24.0;

/// [`stretch_stereo`] that also transposes by `semitones` (length unchanged by it), in
/// the same pass.
pub fn stretch_shift_stereo(input: &[f32], sr: u32, ratio: f64, semitones: f32) -> Result<Vec<f32>, String> {
    if !(semitones.is_finite() && semitones.abs() <= MAX_SEMITONES + 1e-4) {
        return Err(format!("transposition {semitones} is outside ±{MAX_SEMITONES} semitones"));
    }
    if !(ratio.is_finite() && (MIN_RATIO - 1e-9..=MAX_RATIO + 1e-9).contains(&ratio)) {
        return Err(format!("stretch ratio {ratio} is outside {MIN_RATIO}–{MAX_RATIO}"));
    }
    let frames = input.len() / 2;
    let out_len = out_frames(frames, ratio);
    if frames == 0 || out_len == 0 {
        return Ok(vec![0.0; out_len * 2]);
    }
    if (ratio - 1.0).abs() < 1e-9 && semitones == 0.0 {
        return Ok(input[..frames * 2].to_vec());
    }
    let srf = sr.max(1) as f32;
    // very short buffers: pad with silence so the algorithm has room, then crop
    let min_out = unsafe { wave_stretch_min_output(srf, 2) } + 1;
    let (in_n, out_n) = if out_len < min_out { (((min_out as f64) / ratio).ceil() as usize, min_out) } else { (frames, out_len) };
    let mut l = vec![0.0f32; in_n];
    let mut r = vec![0.0f32; in_n];
    for (i, [a, b]) in input.as_chunks::<2>().0.iter().enumerate() {
        l[i] = *a;
        r[i] = *b;
    }
    let mut ol = vec![0.0f32; out_n];
    let mut or = vec![0.0f32; out_n];
    let ins = [l.as_ptr(), r.as_ptr()];
    let outs = [ol.as_mut_ptr(), or.as_mut_ptr()];
    let rc = unsafe { wave_stretch_exact(srf, 2, ins.as_ptr(), in_n, outs.as_ptr(), out_n, semitones) };
    match rc {
        0 => {}
        1 => return Err("audio too short to stretch".into()),
        _ => return Err("not enough memory to stretch".into()),
    }
    let mut out = Vec::with_capacity(out_len * 2);
    for i in 0..out_len {
        let (a, b) = (ol[i], or[i]);
        out.push(if a.is_finite() { a } else { 0.0 });
        out.push(if b.is_finite() { b } else { 0.0 });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warp_anchors_validated() {
        let x = sine(440.0, 48_000, 1.0);
        let n = x.len() as f64 / 2.0;
        // must start at frame 0 on both sides
        assert!(stretch_warp_stereo(&x, 48_000, &[WarpAnchor { at: 1.0, to: 0.0 }, WarpAnchor { at: n, to: n }], 0.0).is_err());
        // must increase on both sides
        assert!(stretch_warp_stereo(
            &x,
            48_000,
            &[WarpAnchor { at: 0.0, to: 0.0 }, WarpAnchor { at: n, to: n }, WarpAnchor { at: n - 1.0, to: n }],
            0.0
        )
        .is_err());
        assert!(stretch_warp_stereo(
            &x,
            48_000,
            &[WarpAnchor { at: 0.0, to: 0.0 }, WarpAnchor { at: n / 2.0, to: n / 2.0 }, WarpAnchor { at: n, to: n / 2.0 }],
            0.0
        )
        .is_err());
        // a segment outside the ratio bounds
        let half = &[WarpAnchor { at: 0.0, to: 0.0 }, WarpAnchor { at: n, to: n * 0.1 }];
        assert!(stretch_warp_stereo(&x, 48_000, half, 0.0).is_err());
        // anchors past the input
        let past = &[WarpAnchor { at: 0.0, to: 0.0 }, WarpAnchor { at: n * 2.0, to: n * 2.0 }];
        assert!(stretch_warp_stereo(&x, 48_000, past, 0.0).is_err());
        // not finite
        let nan = &[WarpAnchor { at: 0.0, to: 0.0 }, WarpAnchor { at: n, to: f64::NAN }];
        assert!(stretch_warp_stereo(&x, 48_000, nan, 0.0).is_err());
        assert!(stretch_warp_stereo(&x, 48_000, &[WarpAnchor { at: 0.0, to: 0.0 }, WarpAnchor { at: n, to: n }], 30.0).is_err());
    }

    #[test]
    fn warp_lands_hits_where_asked() {
        let sr = 48_000u32;
        let n = (1.5 * sr as f64) as usize;
        // clicks at 0.1 s, 0.6 s, 1.1 s
        let mut x = vec![0.0f32; n * 2];
        for t in [0.1, 0.6, 1.1, 1.2] {
            let i = (t * sr as f64) as usize;
            x[i * 2] = 1.0;
            x[i * 2 + 1] = 1.0;
        }
        // the 1.1 s hit plays at 1.3 s; the rest maps at the two segment slopes
        let anchors = [
            WarpAnchor { at: 0.0, to: 0.0 },
            WarpAnchor { at: 1.1 * sr as f64, to: 1.3 * sr as f64 },
            WarpAnchor { at: n as f64, to: n as f64 },
        ];
        let y = stretch_warp_stereo(&x, sr, &anchors, 0.0).unwrap();
        assert_eq!(y.len() / 2, n);
        // every click found where its segment puts it, within 10 ms
        let seg1 = 1.3 / 1.1;
        let seg2 = (n as f64 - 1.3 * sr as f64) / (0.4 * sr as f64);
        let want = [(0.1, 0.1 * seg1), (0.6, 0.6 * seg1), (1.1, 1.3), (1.2, 1.3 + 0.1 * seg2)];
        for (t, out) in want {
            let expect = out * sr as f64;
            // strongest sample within ±10 ms of the expectation
            let lo = (expect - 480.0).max(0.0) as usize;
            let hi = ((expect + 480.0) as usize).min(n - 1);
            let peak = (lo..hi).map(|i| (y[i * 2].abs(), i)).max_by(|a, b| a.0.total_cmp(&b.0)).unwrap().1 as f64;
            assert!((peak - expect).abs() < 480.0, "click at {t} s: landed at frame {peak}, wanted {expect}");
        }
    }

    #[test]
    fn warp_keeps_pitch_and_exact_length() {
        let sr = 48_000;
        let x = sine(440.0, sr, 2.0);
        let n = x.len() as f64 / 2.0;
        // squeeze the second half to three quarters of the whole
        let anchors = [WarpAnchor { at: 0.0, to: 0.0 }, WarpAnchor { at: n / 2.0, to: n / 2.0 }, WarpAnchor { at: n, to: n * 0.75 }];
        let y = stretch_warp_stereo(&x, sr, &anchors, 0.0).unwrap();
        assert_eq!(y.len() / 2, (n * 0.75) as usize);
        let f = freq_of(&y, sr);
        assert!((f - 440.0).abs() < 4.4, "{f} Hz");
        // an identity-shaped map keeps the level and the exact length (a sample-exact
        // check would demand phase alignment the stretcher's seek does not promise)
        let one = [WarpAnchor { at: 0.0, to: 0.0 }, WarpAnchor { at: n, to: n }];
        let same = stretch_warp_stereo(&x, sr, &one, 0.0).unwrap();
        assert_eq!(same.len() / 2, n as usize);
        let rms = |y: &[f32]| (y.iter().map(|v| v * v).sum::<f32>() / y.len() as f32).sqrt();
        assert!((rms(&same) - rms(&x)).abs() < 0.03, "identity warp rms {} vs {}", rms(&same), rms(&x));
        // a marker in the middle still keeps the length
        let mid = [WarpAnchor { at: 0.0, to: 0.0 }, WarpAnchor { at: n * 0.4, to: n * 0.5 }, WarpAnchor { at: n, to: n }];
        let z = stretch_warp_stereo(&x, sr, &mid, 0.0).unwrap();
        assert_eq!(z.len() / 2, n as usize);
        assert!(z.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn warp_short_buffer_errors_cleanly() {
        let x = sine(1000.0, 44_100, 0.02);
        let n = x.len() as f64 / 2.0;
        let anchors = [WarpAnchor { at: 0.0, to: 0.0 }, WarpAnchor { at: n, to: n * 2.0 }];
        match stretch_warp_stereo(&x, 44_100, &anchors, 0.0) {
            Ok(y) => assert_eq!(y.len() / 2, (n * 2.0) as usize),
            Err(e) => assert_eq!(e, "audio too short to stretch"),
        }
        assert!(stretch_warp_stereo(&[], 44_100, &[WarpAnchor { at: 0.0, to: 0.0 }, WarpAnchor { at: 10.0, to: 10.0 }], 0.0)
            .unwrap()
            .is_empty());
    }

    fn sine(freq: f64, sr: u32, secs: f64) -> Vec<f32> {
        let n = (sr as f64 * secs) as usize;
        (0..n)
            .flat_map(|i| {
                let v = (0.5 * (2.0 * std::f64::consts::PI * freq * i as f64 / sr as f64).sin()) as f32;
                [v, v]
            })
            .collect()
    }

    /// Frequency from upward zero crossings of the left channel in the middle half.
    fn freq_of(x: &[f32], sr: u32) -> f64 {
        let l: Vec<f32> = x.as_chunks::<2>().0.iter().map(|f| f[0]).collect();
        let (a, b) = (l.len() / 4, l.len() * 3 / 4);
        let mut cross = vec![];
        for i in a..b {
            if l[i - 1] < 0.0 && l[i] >= 0.0 {
                let t = (i - 1) as f64 + (-l[i - 1] / (l[i] - l[i - 1])) as f64;
                cross.push(t);
            }
        }
        (cross.len() - 1) as f64 / ((cross[cross.len() - 1] - cross[0]) / sr as f64)
    }

    #[test]
    fn exact_length_and_pitch_kept() {
        let sr = 48_000;
        let x = sine(440.0, sr, 2.0);
        for ratio in [0.5, 0.8, 1.5, 2.0] {
            let y = stretch_stereo(&x, sr, ratio).unwrap();
            assert_eq!(y.len() / 2, out_frames(x.len() / 2, ratio), "ratio {ratio}");
            let f = freq_of(&y, sr);
            assert!((f - 440.0).abs() < 4.4, "ratio {ratio}: {f} Hz");
            // steady-state level (a 0.5 sine has an RMS of 0.354)
            let mid = &y[y.len() / 4..y.len() * 3 / 4];
            let rms = (mid.iter().map(|v| v * v).sum::<f32>() / mid.len() as f32).sqrt();
            assert!((rms - 0.354).abs() < 0.04, "ratio {ratio}: rms {rms}");
            let peak = y.iter().fold(0f32, |m, v| m.max(v.abs()));
            assert!(peak < 1.0, "ratio {ratio}: peak {peak}");
        }
    }

    #[test]
    fn short_and_empty_buffers() {
        let sr = 44_100;
        let x = sine(1000.0, sr, 0.02);
        let y = stretch_stereo(&x, sr, 0.5).unwrap();
        assert_eq!(y.len() / 2, out_frames(x.len() / 2, 0.5));
        assert!(y.iter().all(|v| v.is_finite()));
        assert!(stretch_stereo(&[], sr, 2.0).unwrap().is_empty());
        assert!(stretch_stereo(&x, sr, 10.0).is_err());
        assert!(stretch_stereo(&x, sr, f64::NAN).is_err());
    }

    /// `cargo test -p wave-stretch --release -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn speed() {
        let sr = 44_100;
        let x = sine(330.0, sr, 180.0);
        for ratio in [0.5, 1.5] {
            let t = std::time::Instant::now();
            stretch_stereo(&x, sr, ratio).unwrap();
            println!("3 min stereo ×{ratio}: {:?}", t.elapsed());
        }
    }

    #[test]
    fn transposes_and_keeps_the_length() {
        let sr = 48_000;
        let x = sine(440.0, sr, 1.0);
        for (ratio, semis) in [(1.0, 7.0f32), (1.0, -12.0), (1.0, 12.0), (2.0, 3.0), (0.5, -5.5)] {
            let y = stretch_shift_stereo(&x, sr, ratio, semis).unwrap();
            assert_eq!(y.len() / 2, out_frames(x.len() / 2, ratio), "×{ratio} {semis} st");
            let want = 440.0 * 2f64.powf(semis as f64 / 12.0);
            let f = freq_of(&y, sr);
            // within 10 cents
            assert!((1200.0 * (f / want).log2()).abs() < 10.0, "×{ratio} {semis} st: {f} Hz, want {want}");
            let mid = &y[y.len() / 4..y.len() * 3 / 4];
            let rms = (mid.iter().map(|v| v * v).sum::<f32>() / mid.len() as f32).sqrt();
            assert!((rms - 0.354).abs() < 0.06, "×{ratio} {semis} st: rms {rms}");
        }
        assert!(stretch_shift_stereo(&x, sr, 1.0, 30.0).is_err());
        assert!(stretch_shift_stereo(&x, sr, 1.0, f32::NAN).is_err());
    }

    #[test]
    fn unity_is_identity() {
        let x = sine(220.0, 44_100, 0.3);
        assert_eq!(stretch_stereo(&x, 44_100, 1.0).unwrap(), x);
    }
}
