//! Music Box: the modeled instrument behind Channel Rack channels of kind `musicbox`.
//!
//! A comb tine is a steel bar clamped at one end and plucked by the cylinder's pin, so its
//! sound is a few flexural modes of a cantilever: an almost pure fundamental with sparse,
//! strongly inharmonic overtones (an ideal cantilever rings at 1 : 6.267 : 17.55 : 34.39,
//! from βL = 1.875, 4.694, 7.855, 10.996) that decay several times faster than the
//! fundamental, plus a short mechanical tick where the pin lets go. This module builds that
//! from per-note partials: each note is a fixed set of decaying sines whose ratios, decay
//! times and excitation follow the note's pitch, the velocity (a harder pluck excites more
//! of the overtone stack) and the channel's knobs. Voices live in
//! [`crate::synth::VoicePool`] like Synkopa Instruments; there are no samples and nothing to
//! install.

/// The note's partial frequencies, as multiples of its fundamental.
pub const PARTIAL_RATIOS: [f32; PARTIALS] = [1.0, 6.267, 17.55, 34.39];
/// How many partials a note sums.
pub const PARTIALS: usize = 4;
/// Excitation of each partial before velocity and brightness: the fundamental dominates, the
/// far overtones only speak on a hard pluck.
const PARTIAL_BASE: [f32; PARTIALS] = [1.0, 0.32, 0.13, 0.05];

/// How long a note rings after its key is let go, in milliseconds.
pub const RELEASE_MS: (f32, f32, f32) = (40.0, 3000.0, 400.0);
/// Ring: 25..250 % of the natural decay time of the note's fundamental.
pub const RING: (f32, f32, f32) = (25.0, 250.0, 100.0);
/// Brightness: how hard the pluck excites the overtone stack.
pub const BRIGHTNESS: (f32, f32, f32) = (0.0, 100.0, 45.0);
/// Body: the wooden box's own low resonance, mixed under the tines.
pub const BODY: (f32, f32, f32) = (0.0, 100.0, 30.0);
/// Mechanical: the pin's tick at the start of every note.
pub const MECHANICAL: (f32, f32, f32) = (0.0, 100.0, 25.0);
/// Spread: how far across the stereo image the comb's notes sit.
pub const SPREAD: (f32, f32, f32) = (0.0, 100.0, 35.0);

/// The two resonances of the box body every tine sits in, in Hz.
const BODY_MODES: [f32; 2] = [185.0, 330.0];
/// How long a body resonance rings after its excitation, seconds.
const BODY_T60: f32 = 0.3;
/// The pin tick's decay time, seconds (it is a click, not a tone).
const TICK_T60: f32 = 0.0015;
/// One-pole lowpass of the tick: comb noise through the plate, not white hiss.
const TICK_LP: f32 = 0.3;

/// Output level against a bare sum of partials, like [`crate::synth::KEYS_HEADROOM`]: with
/// this, one full-velocity note lands well below full scale (the render test pins the band)
/// and a ten-note chord stays under full scale, so chords sum linearly.
pub const HEADROOM: f32 = 0.28;

/// A channel's knobs, resolved for the audio thread (percentages normalized to 0..1, the
/// release in seconds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub ring: f32,
    pub brightness: f32,
    pub body: f32,
    pub mechanical: f32,
    pub spread: f32,
    pub release: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            ring: (RING.2 - RING.0) / (RING.1 - RING.0),
            brightness: BRIGHTNESS.2 / 100.0,
            body: BODY.2 / 100.0,
            mechanical: MECHANICAL.2 / 100.0,
            spread: SPREAD.2 / 100.0,
            release: RELEASE_MS.2 / 1000.0,
        }
    }
}

impl Default for Tine {
    /// A silent note (an idle voice slot): nothing rings, the body passes nothing.
    fn default() -> Self {
        Self {
            freq: [0.0; PARTIALS],
            phase: [0.0; PARTIALS],
            decay: [0.0; PARTIALS],
            amp: [0.0; PARTIALS],
            lcg: 0,
            tick: 0.0,
            tick_lp: 0.0,
            tick_coef: 0.0,
            pan: 0.0,
            body: [BodyMode { freq: 0.0, r: 0.0, a: 0.0, gain: 0.0, s1: 0.0, s2: 0.0 }; 2],
        }
    }
}

/// Validates a channel's settings (`Op::PutChannel`, the CLI's `--ch --param`).
pub fn check(ring: f32, brightness: f32, body: f32, mechanical: f32, spread: f32, release: f32) -> Result<(), String> {
    for (v, (lo, hi, _), name) in [
        (ring, RING, "ring"),
        (brightness, BRIGHTNESS, "brightness"),
        (body, BODY, "body"),
        (mechanical, MECHANICAL, "mechanical"),
        (spread, SPREAD, "spread"),
    ] {
        if !(v.is_finite() && (lo..=hi).contains(&v)) {
            return Err(format!("music box {name} must be {lo}..{hi} %"));
        }
    }
    if !(release.is_finite() && (RELEASE_MS.0..=RELEASE_MS.1).contains(&release)) {
        return Err(format!("music box release must be {}..{} ms", RELEASE_MS.0, RELEASE_MS.1));
    }
    Ok(())
}

/// Natural decay time of a tine's fundamental, seconds: long bass tines ring longest, thin
/// treble tines die quickly (music box combs run roughly two octaves either side of C5).
fn fundamental_t60(f0: f32, ring: f32) -> f32 {
    let natural = 5.0 * (180.0 / f0.max(20.0)).powf(0.35);
    // The knob promises 25..250 % of the natural decay (RANGE-relative, the same
    // normalization the UI's automation mapping uses): 100 % plays it exactly.
    let t60 = natural * (0.25 + 2.25 * ring.clamp(0.0, 1.0));
    t60.clamp(0.15, 12.0)
}

/// The box body's resonators, applied to every note.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BodyMode {
    freq: f32,
    /// Resonator pole radius: −60 dB after [`BODY_T60`] seconds.
    r: f32,
    /// Input scaling that makes the resonator's steady-state gain at its own pole exactly 1:
    /// a note sitting on the mode passes at its own amplitude, not amplified (a bare
    /// `(1 − r)` leaves a sustained tone amplified by 1/|1 − r·e^(−2jw)| ≈ 1/(2 sin w),
    /// ~20× at 185 Hz and 48 kHz).
    a: f32,
    /// Mix of the mode under the dry note.
    gain: f32,
    s1: f32,
    s2: f32,
}

impl BodyMode {
    fn new(freq: f32, gain: f32, sr: f32) -> Self {
        let r = 10f32.powf(-3.0 / (BODY_T60 * sr));
        let w = std::f32::consts::TAU * freq / sr;
        // |1 − r·e^(−2jw)|: the resonant denominator's remaining factor
        let k = (1.0 - 2.0 * r * (2.0 * w).cos() + r * r).sqrt();
        Self { freq, r, a: (1.0 - r) * k, gain, s1: 0.0, s2: 0.0 }
    }

    /// Two-pole resonator: rings at `freq`, fed by the note at unit steady-state gain.
    fn process(&mut self, x: f32, sr: f32) -> f32 {
        let w = std::f32::consts::TAU * self.freq / sr;
        let c2r = 2.0 * self.r * w.cos();
        let y = c2r * self.s1 - self.r * self.r * self.s2 + self.a * x;
        self.s2 = self.s1;
        self.s1 = y;
        y * self.gain
    }
}

/// A sounding note: partial phases and running amplitudes, the pin tick, and the body
/// resonators. Held in the voice (see [`crate::synth::VoiceKind::MusicBox`]); built per note
/// on the audio thread (plain math, no allocation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tine {
    pub freq: [f32; PARTIALS],
    pub phase: [f32; PARTIALS],
    /// Per-sample decay coefficient of each partial.
    pub decay: [f32; PARTIALS],
    /// Running amplitude of each partial.
    pub amp: [f32; PARTIALS],
    /// Pin tick: LCG state, its decaying level, the lowpass state, and the precomputed
    /// per-sample decay coefficient.
    pub lcg: u32,
    pub tick: f32,
    pub tick_lp: f32,
    pub tick_coef: f32,
    /// Per-note pan offset (the comb's notes sit across the stereo image).
    pub pan: f32,
    body: [BodyMode; 2],
}

impl Tine {
    /// The note `key` struck at `vel` 0..1 with the channel's knobs, at sample rate `sr`.
    pub fn new(key: u8, vel: f32, p: &Params, sr: f32) -> Self {
        let f0 = crate::synth::mtof(key as f32);
        // brightness and velocity together decide how much of the overtone stack speaks
        let b = 0.45 + 1.1 * (0.55 * vel.clamp(0.0, 1.0) + 0.45 * p.brightness.clamp(0.0, 1.0));
        let t60 = fundamental_t60(f0, p.ring);
        let mut amp = [0.0; PARTIALS];
        let mut decay = [1.0; PARTIALS];
        let mut freq = [1.0; PARTIALS];
        let mut sum = 0.0;
        for k in 0..PARTIALS {
            // the true partial frequency is kept: one above the rendering bandwidth is
            // suppressed at render time (see `sample`), not clamped into an in-band tone
            freq[k] = f0 * PARTIAL_RATIOS[k];
            let t60_k = t60 / (1.0 + 5.0 * k as f32);
            decay[k] = 10f32.powf(-3.0 / (t60_k.max(0.01) * sr));
            amp[k] = PARTIAL_BASE[k] * b.powf(k as f32);
            sum += amp[k];
        }
        // keep a hard pluck from clipping the sum
        if sum > 1.6 {
            for a in &mut amp {
                *a *= 1.6 / sum;
            }
        }
        // a deterministic per-note LCG (the tick needs noise, tests need repeatability)
        let lcg = 0x2545_F491 ^ (u32::from(key).wrapping_mul(0x9E37_79B9));
        // comb layout: neighbouring tines alternate a little across the stereo image
        let pan = p.spread.clamp(0.0, 1.0) * ((key % 12) as f32 / 6.0 - 1.0) * 0.6;
        Self {
            freq,
            phase: [0.0; PARTIALS],
            decay,
            amp,
            lcg,
            tick: 0.55 * p.mechanical.clamp(0.0, 1.0) * (0.35 + 0.65 * vel.clamp(0.0, 1.0)),
            tick_lp: 0.0,
            tick_coef: 10f32.powf(-3.0 / (TICK_T60.max(0.001) * sr)),
            pan,
            body: [
                BodyMode::new(BODY_MODES[0], 0.9 * p.body.clamp(0.0, 1.0), sr),
                BodyMode::new(BODY_MODES[1], 0.6 * p.body.clamp(0.0, 1.0), sr),
            ],
        }
    }

    /// One sample of the note (before pan, gain and the release envelope).
    pub fn sample(&mut self, bend: f32, sr: f32) -> f32 {
        let mut x = 0.0;
        for k in 0..PARTIALS {
            // a partial bent or played past Nyquist (C6's fourth partial is ~36 kHz at 48 kHz)
            // would fold back as garbage: it stays silent, its phase frozen, until in band again
            let inc = self.freq[k] * bend / sr;
            if inc < 0.5 {
                self.phase[k] += inc;
                if self.phase[k] >= 1.0 {
                    self.phase[k] -= 1.0;
                }
                self.amp[k] *= self.decay[k];
                x += self.amp[k] * (std::f32::consts::TAU * self.phase[k]).sin();
            } else {
                self.amp[k] *= self.decay[k];
            }
        }
        // the pin tick: decaying noise through a one-pole lowpass
        self.lcg = self.lcg.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let n = ((self.lcg >> 8) as f32 / 8_388_608.0 - 1.0).clamp(-1.0, 1.0);
        self.tick_lp += TICK_LP * (n - self.tick_lp);
        self.tick *= self.tick_coef;
        x += self.tick * self.tick_lp;
        let body = self.body[0].process(x, sr) + self.body[1].process(x, sr);
        x + body
    }

    /// True once the note has decayed into inaudibility on its own.
    pub fn spent(&self) -> bool {
        self.amp[0] < 1e-4 && self.tick < 1e-4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_ratios_follow_the_cantilever() {
        // ideal cantilever flexural modes: (βL)² with βL = 1.875, 4.694, 7.855, 10.996
        let b = [1.875_f32, 4.694, 7.855, 10.996];
        for (k, r) in PARTIAL_RATIOS.iter().enumerate() {
            let want = (b[k] / b[0]).powi(2);
            assert!((r - want).abs() < 0.01, "partial {k}: {r} vs {want}");
        }
    }

    #[test]
    fn higher_partials_decay_faster_and_the_note_ends() {
        let p = Params::default();
        let mut t = Tine::new(60, 0.8, &p, 48_000.0);
        for k in 1..PARTIALS {
            assert!(t.decay[k] < t.decay[k - 1], "partial {k} outlives the one below it");
        }
        let mut samples = 0;
        while !t.spent() {
            t.sample(1.0, 48_000.0);
            samples += 1;
            assert!(samples < 48_000 * 14, "note never ends ({samples} samples)");
        }
        // the default ring at C4 is a few seconds of fundamental
        let seconds = samples as f32 / 48_000.0;
        assert!((2.0..9.0).contains(&seconds), "C4 rang {seconds:.1} s");
    }

    #[test]
    fn velocity_brightens_and_loudens() {
        let p = Params::default();
        let soft = Tine::new(60, 0.1, &p, 48_000.0);
        let hard = Tine::new(60, 1.0, &p, 48_000.0);
        let soft_high: f32 = soft.amp[1..].iter().sum();
        let hard_high: f32 = hard.amp[1..].iter().sum();
        assert!(hard_high > soft_high * 2.0, "a hard pluck must excite the overtones far more");
        // the fundamental's level comes from the voice gain; the timbre is the ratio that moves
        assert!(hard_high / hard.amp[0] > soft_high / soft.amp[0] * 2.0, "timbre must brighten");
    }

    #[test]
    fn the_tick_is_short_and_deterministic() {
        let p = Params::default();
        let mut a = Tine::new(64, 0.8, &p, 48_000.0);
        let mut b = Tine::new(64, 0.8, &p, 48_000.0);
        let sr = 48_000.0;
        for i in 0..(0.05 * sr) as usize {
            let (x, y) = (a.sample(1.0, sr), b.sample(1.0, sr));
            assert_eq!(x, y, "same note twice must be identical");
            if i > 0 {
                assert!(a.tick < 0.5 * p.mechanical, "tick outlives its envelope at {i}");
            }
        }
        assert!(a.tick.abs() < 1e-3, "tick should be silent after 50 ms");
    }

    #[test]
    fn check_enforces_the_knob_ranges() {
        assert!(check(100.0, 45.0, 30.0, 25.0, 35.0, 400.0).is_ok());
        assert!(check(999.0, 45.0, 30.0, 25.0, 35.0, 400.0).is_err());
        assert!(check(100.0, 45.0, 30.0, 25.0, 35.0, f32::NAN).is_err());
        assert!(check(100.0, 45.0, 30.0, 25.0, 35.0, 9999.0).is_err());
    }

    #[test]
    fn out_of_band_partials_are_suppressed_and_in_band_ones_ring() {
        let p = Params::default();
        let sr = 48_000.0;
        // C6's fourth partial is ~36 kHz: the true frequency is kept, the partial is silent,
        // so zeroing it changes nothing
        let mut t = Tine::new(84, 0.8, &p, sr);
        assert!((t.freq[3] - 1046.5 * PARTIAL_RATIOS[3]).abs() < 1.0, "the true frequency is kept");
        let mut without = t;
        without.freq[3] = 0.0;
        for _ in 0..2000 {
            assert!((t.sample(1.0, sr) - without.sample(1.0, sr)).abs() < 1e-4, "an out-of-band partial leaked");
        }
        // C4's fourth partial (~1.6 kHz at 48 kHz) is in band: zeroing it must change the sound
        let mut t = Tine::new(60, 0.8, &p, sr);
        let mut without = t;
        without.freq[3] = 0.0;
        let mut different = false;
        for _ in 0..2000 {
            if t.sample(1.0, sr) != without.sample(1.0, sr) {
                different = true;
                break;
            }
        }
        assert!(different, "an in-band partial was suppressed");
        // a +2-semitone bend pushes E6's third partial (~23 kHz) past Nyquist: suppressed too
        let bend = 2.0_f32.powf(2.0 / 12.0);
        let mut t = Tine::new(88, 0.8, &p, sr);
        let mut without = t;
        without.freq[2] = 0.0;
        let mut same_while_bent = true;
        let mut different_while_plain = false;
        for _ in 0..1000 {
            // f32 accumulation differs by ulps between the two partial sets; a real leak of
            // the suppressed partial would be a percent of the signal, not a rounding step
            if (t.sample(bend, sr) - without.sample(bend, sr)).abs() > 1e-4 {
                same_while_bent = false;
            }
            if (t.sample(1.0, sr) - without.sample(1.0, sr)).abs() > 1e-4 {
                different_while_plain = true;
            }
        }
        assert!(same_while_bent, "the bent-out partial leaked");
        assert!(different_while_plain, "the partial must come back (and sound) once in band");
    }

    #[test]
    fn the_body_resonator_passes_its_own_pitch_at_unit_gain() {
        let sr = 48_000.0;
        for (mode_freq, f_in) in [(BODY_MODES[0], 185.0_f32), (BODY_MODES[1], 330.0_f32)] {
            let mut mode = BodyMode::new(mode_freq, 1.0, sr);
            for i in 0..(2.0 * sr) as usize {
                mode.process((std::f32::consts::TAU * f_in * i as f32 / sr).sin(), sr);
            }
            let y = mode.process((std::f32::consts::TAU * f_in * 40000.0 / sr).sin(), sr);
            assert!((y.abs() - 1.0).abs() < 0.1, "mode {} steady-state gain {}", mode_freq, y.abs());
        }
    }

    #[test]
    fn body_zero_matches_an_independent_dry_signal() {
        // Body 0 must be exactly the dry tine: the sample equals the partial-plus-tick
        // definition, computed here without the resonator code ever being invoked
        let sr = 48_000.0;
        let mut t = Tine::new(64, 0.8, &Params { body: 0.0, ..Params::default() }, sr);
        let mut reference = t;
        for i in 0..4000 {
            let mut want = 0.0;
            for k in 0..PARTIALS {
                reference.phase[k] += reference.freq[k] / sr;
                if reference.phase[k] >= 1.0 {
                    reference.phase[k] -= 1.0;
                }
                reference.amp[k] *= reference.decay[k];
                want += reference.amp[k] * (std::f32::consts::TAU * reference.phase[k]).sin();
            }
            reference.lcg = reference.lcg.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let n = ((reference.lcg >> 8) as f32 / 8_388_608.0 - 1.0).clamp(-1.0, 1.0);
            reference.tick_lp += TICK_LP * (n - reference.tick_lp);
            reference.tick *= reference.tick_coef;
            want += reference.tick * reference.tick_lp;
            let got = t.sample(1.0, sr);
            assert!((got - want).abs() < 1e-9, "Body 0 diverges from the dry signal at sample {i}: {got} vs {want}");
        }
    }
}
