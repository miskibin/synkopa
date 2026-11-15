//! The sound of WAVE Demo Synth, independent of VST3: parameters, voices, rendering.
//!
//! A subtractive synth: one oscillator per voice (band-limited saw or square, or a
//! triangle), a resonant low-pass (TPT state-variable filter) opened by the mod wheel, and
//! an ADSR on the amplitude. 16 voices, the oldest is stolen. Everything is allocated in
//! [`Synth::new`]; [`Synth::render`] and the event methods never allocate.

/// Parameter ids (the VST3 `ParamID`s).
pub mod id {
    pub const WAVE: u32 = 0;
    pub const CUTOFF: u32 = 1;
    pub const RESONANCE: u32 = 2;
    pub const ATTACK: u32 = 3;
    pub const DECAY: u32 = 4;
    pub const SUSTAIN: u32 = 5;
    pub const RELEASE: u32 = 6;
    pub const VOLUME: u32 = 7;
    pub const MOD_WHEEL: u32 = 8;
    /// Mapped from MIDI pitch bend; not listed for people.
    pub const BEND: u32 = 9;
    /// Mapped from MIDI CC 64; not listed for people.
    pub const PEDAL: u32 = 10;
}

/// How a parameter maps its normalized 0..1 value, and how it reads.
#[derive(Clone, Copy, Debug)]
pub struct ParamDef {
    pub id: u32,
    pub name: &'static str,
    pub units: &'static str,
    /// Normalized default.
    pub def: f64,
    /// Discrete positions minus one (0 = continuous).
    pub steps: i32,
    /// Shown to people (false: driven by a MIDI controller only).
    pub listed: bool,
}

pub const PARAMS: [ParamDef; 11] = [
    ParamDef { id: id::WAVE, name: "Waveform", units: "", def: 0.0, steps: 2, listed: true },
    ParamDef { id: id::CUTOFF, name: "Cutoff", units: "Hz", def: 0.693, steps: 0, listed: true },
    ParamDef { id: id::RESONANCE, name: "Resonance", units: "%", def: 0.2, steps: 0, listed: true },
    ParamDef { id: id::ATTACK, name: "Attack", units: "ms", def: 0.2117, steps: 0, listed: true },
    ParamDef { id: id::DECAY, name: "Decay", units: "ms", def: 0.593, steps: 0, listed: true },
    ParamDef { id: id::SUSTAIN, name: "Sustain", units: "%", def: 0.7, steps: 0, listed: true },
    ParamDef { id: id::RELEASE, name: "Release", units: "ms", def: 0.634, steps: 0, listed: true },
    ParamDef { id: id::VOLUME, name: "Volume", units: "dB", def: 30.0 / 42.0, steps: 0, listed: true },
    ParamDef { id: id::MOD_WHEEL, name: "Mod Wheel", units: "%", def: 0.0, steps: 0, listed: true },
    ParamDef { id: id::BEND, name: "Pitch Bend", units: "st", def: 0.5, steps: 0, listed: false },
    ParamDef { id: id::PEDAL, name: "Sustain Pedal", units: "", def: 0.0, steps: 1, listed: false },
];

pub const WAVE_NAMES: [&str; 3] = ["Saw", "Square", "Triangle"];

/// The plain value of parameter `id` at normalized `v`.
pub fn plain(id: u32, v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    match id {
        id::WAVE => (v * 2.0).round(),
        id::CUTOFF => 20.0 * 1000f64.powf(v),
        id::ATTACK => 2000f64.powf(v),                    // 1 ms .. 2 s
        id::DECAY | id::RELEASE => 5.0 * 1000f64.powf(v), // 5 ms .. 5 s
        id::VOLUME => -36.0 + 42.0 * v,
        id::BEND => (v - 0.5) * 4.0, // ±2 semitones
        id::PEDAL => (v >= 0.5) as u8 as f64,
        _ => v,
    }
}

/// What a person reads for parameter `id` at normalized `v`.
pub fn display(id: u32, v: f64) -> String {
    let p = plain(id, v);
    match id {
        id::WAVE => WAVE_NAMES[p as usize].to_owned(),
        id::CUTOFF if p >= 1000.0 => format!("{:.2} kHz", p / 1000.0),
        id::CUTOFF => format!("{p:.0} Hz"),
        id::ATTACK | id::DECAY | id::RELEASE if p >= 1000.0 => format!("{:.2} s", p / 1000.0),
        id::ATTACK | id::DECAY | id::RELEASE => format!("{p:.0} ms"),
        id::VOLUME => format!("{p:+.1} dB"),
        id::BEND => format!("{p:+.2} st"),
        id::PEDAL => if p > 0.5 { "Down" } else { "Up" }.to_owned(),
        _ => format!("{:.0} %", v * 100.0),
    }
}

/// Most voices sounding at once.
pub const VOICES: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    Off,
    Attack,
    Decay,
    Release,
}

#[derive(Clone, Copy)]
struct Voice {
    stage: Stage,
    key: u8,
    note_id: i32,
    vel: f32,
    /// Held by the pedal after its note-off.
    pedal_held: bool,
    phase: f64,
    env: f32,
    /// Filter state (TPT SVF).
    ic1: f32,
    ic2: f32,
    /// When it started, for stealing the oldest.
    age: u64,
}

impl Voice {
    const OFF: Voice =
        Voice { stage: Stage::Off, key: 0, note_id: -1, vel: 0.0, pedal_held: false, phase: 0.0, env: 0.0, ic1: 0.0, ic2: 0.0, age: 0 };
}

/// PolyBLEP correction of a discontinuity at phase 0 (`t` in 0..1, `dt` the phase step).
#[inline]
fn blep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

pub struct Synth {
    sr: f64,
    params: [f64; PARAMS.len()],
    voices: [Voice; VOICES],
    clock: u64,
    /// Output gain as the previous sample had it (glides to the volume parameter).
    gain: f32,
}

impl Synth {
    pub fn new(sample_rate: f64) -> Self {
        let mut params = [0.0; PARAMS.len()];
        for p in PARAMS {
            params[p.id as usize] = p.def;
        }
        Self { sr: sample_rate.max(1.0), params, voices: [Voice::OFF; VOICES], clock: 0, gain: 0.0 }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sr = sample_rate.max(1.0);
    }

    /// Normalized parameter values, by id.
    pub fn params(&self) -> &[f64; PARAMS.len()] {
        &self.params
    }

    pub fn set_param(&mut self, id: u32, v: f64) {
        if let Some(slot) = self.params.get_mut(id as usize) {
            *slot = v.clamp(0.0, 1.0);
        }
        if id == id::PEDAL && plain(id, v) < 0.5 {
            for voice in self.voices.iter_mut().filter(|v| v.pedal_held) {
                voice.pedal_held = false;
                voice.stage = Stage::Release;
            }
        }
    }

    fn p(&self, id: u32) -> f64 {
        plain(id, self.params[id as usize])
    }

    /// Voices sounding (any stage but off).
    pub fn active(&self) -> usize {
        self.voices.iter().filter(|v| v.stage != Stage::Off).count()
    }

    pub fn note_on(&mut self, key: u8, vel: f32, note_id: i32) {
        self.clock += 1;
        let slot = self
            .voices
            .iter()
            .position(|v| v.stage == Stage::Off)
            .unwrap_or_else(|| (0..VOICES).min_by_key(|&i| self.voices[i].age).unwrap_or(0));
        let v = &mut self.voices[slot];
        let restart = v.stage != Stage::Off;
        *v = Voice {
            stage: Stage::Attack,
            key: key.min(127),
            note_id,
            vel: vel.clamp(0.0, 1.0),
            pedal_held: false,
            phase: 0.0,
            // a stolen voice starts from where it was: no click
            env: if restart { v.env } else { 0.0 },
            ic1: v.ic1,
            ic2: v.ic2,
            age: self.clock,
        };
    }

    /// Ends the note `note_id` (or, with -1, every note of `key`).
    pub fn note_off(&mut self, key: u8, note_id: i32) {
        let pedal = self.p(id::PEDAL) > 0.5;
        for v in self.voices.iter_mut().filter(|v| v.stage != Stage::Off && v.stage != Stage::Release) {
            if (note_id != -1 && v.note_id == note_id) || (note_id == -1 && v.key == key) {
                if pedal {
                    v.pedal_held = true;
                } else {
                    v.stage = Stage::Release;
                }
            }
        }
    }

    /// Silences everything at once (deactivation).
    pub fn reset(&mut self) {
        self.voices = [Voice::OFF; VOICES];
        self.gain = 0.0;
    }

    /// Adds `l.len()` frames of sound into `l` and `r`.
    pub fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len());
        let sr = self.sr;
        let wave = self.p(id::WAVE) as u8;
        let attack = (1.0 / (self.p(id::ATTACK) * 0.001 * sr).max(1.0)) as f32;
        // exponential segments: about -60 dB over the set time
        let dec = (-6.9 / (self.p(id::DECAY) * 0.001 * sr).max(1.0)).exp() as f32;
        let rel = (-6.9 / (self.p(id::RELEASE) * 0.001 * sr).max(1.0)).exp() as f32;
        let sustain = self.p(id::SUSTAIN) as f32;
        let bend = 2f64.powf(self.p(id::BEND) / 12.0);
        // the mod wheel opens the filter by up to three octaves
        let cutoff = (self.p(id::CUTOFF) * 2f64.powf(3.0 * self.p(id::MOD_WHEEL))).min(sr * 0.45);
        let g = (std::f64::consts::PI * cutoff / sr).tan() as f32;
        let k = (2.0 - 1.9 * self.p(id::RESONANCE)) as f32;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let target = 10f32.powf(self.p(id::VOLUME) as f32 / 20.0) * 0.25;
        let glide = (1.0 - (-1.0 / (0.01 * sr)).exp()) as f32;
        for v in self.voices.iter_mut().filter(|v| v.stage != Stage::Off) {
            let dt = 440.0 * 2f64.powf((f64::from(v.key) - 69.0) / 12.0) * bend / sr;
            let mut gain = self.gain;
            for i in 0..n {
                match v.stage {
                    Stage::Attack => {
                        v.env += attack;
                        if v.env >= 1.0 {
                            v.env = 1.0;
                            v.stage = Stage::Decay;
                        }
                    }
                    Stage::Decay => v.env = sustain + (v.env - sustain) * dec,
                    Stage::Release => {
                        v.env *= rel;
                        if v.env < 1e-4 {
                            v.stage = Stage::Off;
                            v.env = 0.0;
                            v.ic1 = 0.0;
                            v.ic2 = 0.0;
                            break;
                        }
                    }
                    Stage::Off => break,
                }
                let t = v.phase;
                let osc = match wave {
                    0 => 2.0 * t - 1.0 - blep(t, dt),
                    1 => {
                        let half = (t + 0.5) % 1.0;
                        (if t < 0.5 { 1.0 } else { -1.0 }) + blep(t, dt) - blep(half, dt)
                    }
                    _ => 1.0 - 4.0 * (t - 0.5).abs(),
                } as f32;
                v.phase += dt;
                if v.phase >= 1.0 {
                    v.phase -= 1.0;
                }
                // TPT state-variable low-pass
                let v3 = osc - v.ic2;
                let v1 = a1 * v.ic1 + a2 * v3;
                let v2 = v.ic2 + a2 * v.ic1 + a3 * v3;
                v.ic1 = 2.0 * v1 - v.ic1;
                v.ic2 = 2.0 * v2 - v.ic2;
                gain += (target - gain) * glide;
                let s = v2 * v.env * v.vel * gain;
                l[i] += s;
                r[i] += s;
            }
        }
        // the gain glide is shared: advance it by the block even with no voice
        let mut gain = self.gain;
        for _ in 0..n {
            gain += (target - gain) * glide;
        }
        self.gain = gain;
    }

    /// The parameters as bytes: what the plugin's state stream holds.
    pub fn save(&self) -> Vec<u8> {
        save_params(&self.params)
    }
}

const MAGIC: &[u8; 4] = b"WDSY";

/// `WDSY`, a version (1), the count, then each normalized value as little-endian f64.
pub fn save_params(params: &[f64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + params.len() * 8);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&(params.len() as u32).to_le_bytes());
    for v in params {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Reads [`save_params`]' bytes into `params` (ids it does not know keep their values);
/// `false` for bytes that are not such a state.
pub fn load_params(bytes: &[u8], params: &mut [f64]) -> bool {
    if bytes.len() < 12 || &bytes[..4] != MAGIC {
        return false;
    }
    let count = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    for (i, raw) in bytes[12..].as_chunks::<8>().0.iter().take(count).enumerate() {
        if let Some(slot) = params.get_mut(i) {
            let v = f64::from_le_bytes(*raw);
            if v.is_finite() {
                *slot = v.clamp(0.0, 1.0);
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(s: &mut Synth, frames: usize) -> Vec<f32> {
        let (mut l, mut r) = (vec![0f32; frames], vec![0f32; frames]);
        s.render(&mut l, &mut r);
        assert_eq!(l, r);
        l
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    /// Fundamental by autocorrelation (`lo`..`hi` Hz).
    fn pitch(x: &[f32], sr: f64, lo: f64, hi: f64) -> f64 {
        let (min, max) = ((sr / hi) as usize, (sr / lo) as usize);
        let best = (min..max)
            .max_by(|a, b| {
                let c = |lag: usize| x.iter().zip(&x[lag..]).map(|(p, q)| p * q).sum::<f32>();
                c(*a).total_cmp(&c(*b))
            })
            .unwrap();
        sr / best as f64
    }

    #[test]
    fn a_held_note_sounds_at_its_pitch_and_ends_after_its_release() {
        for wave in [0.0, 0.5, 1.0] {
            let mut s = Synth::new(48_000.0);
            s.set_param(id::WAVE, wave);
            assert_eq!(rms(&run(&mut s, 4800)), 0.0, "silent before a note");
            s.note_on(69, 1.0, 1);
            let held = run(&mut s, 9600);
            let f = pitch(&held[4800..], 48_000.0, 200.0, 1000.0);
            assert!((f - 440.0).abs() < 3.0, "A4 plays at {f} Hz ({})", WAVE_NAMES[plain(id::WAVE, wave) as usize]);
            assert!(rms(&held) > 0.01);
            s.note_off(69, 1);
            run(&mut s, 48_000 * 6);
            assert_eq!(s.active(), 0, "the release ends the voice");
            assert!(rms(&run(&mut s, 4800)) < 1e-6);
        }
    }

    #[test]
    fn overlapping_notes_of_one_key_end_by_id_and_the_pedal_holds_them() {
        let mut s = Synth::new(48_000.0);
        s.note_on(60, 1.0, 1);
        s.note_on(60, 1.0, 2);
        s.note_off(60, 1);
        run(&mut s, 48_000 * 6);
        assert_eq!(s.active(), 1, "note 2 still holds");
        s.set_param(id::PEDAL, 1.0);
        s.note_off(60, 2);
        run(&mut s, 48_000 * 6);
        assert_eq!(s.active(), 1, "the pedal holds it");
        s.set_param(id::PEDAL, 0.0);
        run(&mut s, 48_000 * 6);
        assert_eq!(s.active(), 0);
    }

    #[test]
    fn bend_and_wheel_move_pitch_and_brightness() {
        let mut s = Synth::new(48_000.0);
        s.note_on(57, 1.0, 1);
        run(&mut s, 4800);
        s.set_param(id::BEND, 1.0);
        let up = run(&mut s, 9600);
        let f = pitch(&up[2400..], 48_000.0, 150.0, 600.0);
        assert!((f - 246.9).abs() < 3.0, "two semitones up from A3: {f}");
        s.set_param(id::BEND, 0.5);
        s.set_param(id::CUTOFF, 0.3);
        let dark = run(&mut s, 9600);
        s.set_param(id::MOD_WHEEL, 1.0);
        let bright = run(&mut s, 9600);
        let edge = |x: &[f32]| rms(&x.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>());
        assert!(edge(&bright[4800..]) > edge(&dark[4800..]) * 2.0, "the wheel opens the filter");
    }

    #[test]
    fn more_notes_than_voices_steal_the_oldest_and_stay_finite() {
        let mut s = Synth::new(44_100.0);
        s.set_param(id::RESONANCE, 1.0);
        for k in 0..40 {
            s.note_on(30 + k, 1.0, k as i32);
        }
        assert_eq!(s.active(), VOICES);
        let x = run(&mut s, 44_100);
        assert!(x.iter().all(|v| v.is_finite() && v.abs() < 8.0));
    }

    #[test]
    fn the_state_round_trips_and_garbage_is_refused() {
        let mut s = Synth::new(48_000.0);
        s.set_param(id::CUTOFF, 0.25);
        s.set_param(id::WAVE, 1.0);
        let bytes = s.save();
        let mut back = [0.0; PARAMS.len()];
        assert!(load_params(&bytes, &mut back));
        assert_eq!(&back, s.params());
        assert!(!load_params(b"nope", &mut back));
        assert_eq!(display(id::CUTOFF, 0.25), format!("{:.0} Hz", 20.0 * 1000f64.powf(0.25)));
        assert_eq!(display(id::WAVE, 1.0), "Triangle");
        assert_eq!(display(id::VOLUME, PARAMS[id::VOLUME as usize].def), "-6.0 dB");
        assert_eq!(display(id::ATTACK, PARAMS[id::ATTACK as usize].def), "5 ms");
    }
}
