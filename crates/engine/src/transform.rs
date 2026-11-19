//! Note transforms: the deterministic muscle behind the piano roll's Tools strip and
//! `synkopa notes`. Every verb is a pure function over a pattern's notes — same inputs and
//! seed, byte-identical notes — so a run is reproducible and the tests can assert exact
//! results. [`SplitMix64`] is the whole randomness story: no deps, ten lines, stable.

use crate::model::Note;
use crate::pitch;

/// One deterministic RNG for all of this: `splitmix64`, the smallest PRNG whose output
/// passes a casual eye test for music purposes and is trivially reproducible anywhere
/// (the TypeScript mock mirrors it bit for bit).
pub struct SplitMix64(pub u64);

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_add(0x9E37_79B9_7F4A_7C15))
    }

    /// Next raw 64 bits; never all-zero. (Not `Iterator::next` — hence the name.)
    pub fn raw(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform −1.0..1.0.
    pub fn pm1(&mut self) -> f32 {
        (self.raw() >> 11) as f32 / ((1u64 << 53) as f32) * 2.0 - 1.0
    }

    /// Uniform 0..1.
    pub fn unit(&mut self) -> f32 {
        (self.raw() >> 11) as f32 / ((1u64 << 53) as f32)
    }

    /// Uniform integer `0..n` (n > 0).
    fn below(&mut self, n: usize) -> usize {
        (self.raw() % n as u64) as usize
    }
}

/// A generated/filled drum voice: channel `ch`, `k` onsets over `n` slots, rotated.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrumVoice {
    pub ch: Id,
    pub k: u32,
    pub n: u32,
    #[serde(default)]
    pub rotate: i32,
    /// Velocity of ordinary onsets; every fourth is `accent`ed to 1.0.
    #[serde(default = "drum_vel")]
    pub vel: f32,
}

fn drum_vel() -> f32 {
    0.7
}

use crate::model::Id;

// ---------------------------------------------------------------------------
// verbs (the wire shape of `transformNotes`)
// ---------------------------------------------------------------------------

/// The note transformation a `transformNotes` op applies. Params clamp into range;
/// every verb is deterministic in `seed` and the pattern's notes.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TransformVerb {
    /// Random timing and velocity jitter.
    Humanize {
        #[serde(default = "half")]
        timing: f32,
        #[serde(default = "half")]
        velocity: f32,
    },
    /// Snap note starts to `grid` steps (1 = 16th, 2 = 8th, 4 = quarter) by `amount`
    /// 0..1; `scaleAware` also snaps pitches into the project key/scale.
    Quantize {
        #[serde(default = "one_f")]
        grid: f32,
        #[serde(default = "one_f")]
        amount: f32,
        #[serde(default)]
        scale_aware: bool,
    },
    /// Replace a channel's notes with `k` onsets over `n` slots, rotated.
    EuclidFill {
        ch: Id,
        k: u32,
        n: u32,
        #[serde(default)]
        rotate: i32,
        #[serde(default = "drum_vel")]
        vel: f32,
    },
    /// Spread same-start notes by pitch (chords become strums).
    Strum {
        #[serde(default = "eighth")]
        width: f32,
        #[serde(default)]
        ramp: f32,
    },
    /// Generate one Euclidean drum voice per entry across `bars` bars.
    EuclidDrums {
        voices: Vec<DrumVoice>,
        #[serde(default = "two_bars")]
        bars: u32,
    },
    /// Generate a scale-constrained melody on one channel.
    Melody {
        ch: Id,
        #[serde(default = "two_bars")]
        bars: u32,
        #[serde(default = "half")]
        density: f32,
        #[serde(default = "one_u")]
        repeat: u32,
    },
    /// Harmonize the selected single notes into chords: "diatonic" spells the triad
    /// from the project key/scale, the named types spell by interval from each root.
    Chord {
        #[serde(default = "diatonic_harmony")]
        harmony: String,
    },
    /// Generate one diatonic 7th chord per bar on one channel: `template` cycles a
    /// named progression, `chance` walks seeded ±2 degrees, and a non-empty `degrees`
    /// list overrides both. `voice_lead` picks inversions that move the least.
    Progression {
        ch: Id,
        #[serde(default = "four_bars")]
        bars: u32,
        #[serde(default = "pop_template")]
        template: String,
        #[serde(default = "voice_lead_on")]
        voice_lead: bool,
        #[serde(default)]
        degrees: Vec<u8>,
    },
}

fn half() -> f32 {
    0.5
}
fn one_f() -> f32 {
    1.0
}
fn one_u() -> u32 {
    1
}
fn eighth() -> f32 {
    0.5
}
fn two_bars() -> u32 {
    2
}
fn four_bars() -> u32 {
    4
}
fn diatonic_harmony() -> String {
    "diatonic".into()
}
fn pop_template() -> String {
    "pop".into()
}
fn voice_lead_on() -> bool {
    true
}

// ---------------------------------------------------------------------------
// transformations
// ---------------------------------------------------------------------------

/// Random timing and velocity jitter. `timing` 0..1 scales a maximum jitter of a third
/// of a 16th (notes stay recognisable); `velocity` 0..1 scales a ±50 % multiplicative
/// nudge. Notes never start before the pattern.
pub fn humanize(notes: &mut [Note], timing: f32, velocity: f32, rng: &mut SplitMix64) {
    let max_jitter = 0.34 * timing.clamp(0.0, 1.0);
    for n in notes {
        n.step = (n.step + rng.pm1() * max_jitter).max(0.0);
        let nudge = 1.0 + rng.pm1() * 0.5 * velocity.clamp(0.0, 1.0);
        // 0..=1 is the whole valid velocity range: amount 0 must leave every note as it was
        n.v = (n.v * nudge).clamp(0.0, 1.0);
    }
}

/// Snap note starts to `grid` steps (1 = 16th, 2 = 8th, 4 = quarter) by `amount` 0..1.
/// With a key set (`Some((key, scale_id))`), pitches are snapped into the scale —
/// `Note::p` becomes explicit so the result no longer depends on the channel's root.
pub fn quantize(notes: &mut [Note], grid: f32, amount: f32, key_scale: Option<(u8, u32)>, roots: &[(Id, u8)]) {
    let grid = if grid < 0.25 { 0.25 } else { grid };
    let amount = amount.clamp(0.0, 1.0);
    let ks = key_scale.map(|(k, s)| (k as u32, pitch::scale_mask(s)));
    for n in notes {
        let target = (n.step / grid).round() * grid;
        n.step += (target - n.step) * amount;
        if let Some((key, mask)) = ks {
            let root = roots.iter().find(|(id, _)| *id == n.ch).map(|(_, r)| *r).unwrap_or(60);
            let midi = n.p.unwrap_or(root) as f32;
            n.p = Some(snap_midi(midi, key, mask));
        }
    }
}

/// [`pitch::snap`] limited to MIDI keys: the nearest in-scale key within 0..=127 (ties
/// keep the lower). Unbounded snapping can leave the range at the edges — 127 in C#
/// major pentatonic is nearest 128 — which `check_notes` would reject for the whole op.
fn snap_midi(m: f32, key: u32, mask: u16) -> u8 {
    let base = m.round() as i32;
    let mut best: Option<i32> = None;
    let mut bd = f32::MAX;
    for d in -11..=11 {
        let n = base + d;
        if !(0..=127).contains(&n) {
            continue;
        }
        let pc = (n - key as i32).rem_euclid(12) as u16;
        if mask & (1 << pc) != 0 {
            let dist = (n as f32 - m).abs();
            if dist < bd {
                bd = dist;
                best = Some(n);
            }
        }
    }
    // an empty mask has no in-scale key: leave the note where it is
    best.unwrap_or(base).clamp(0, 127) as u8
}

/// Bjorklund's Euclidean onsets: `k` hits spread as evenly as `n` slots allows, rotated
/// by `rotate` slots. Slot `i` is an onset when `(i × k) mod n < k`.
fn euclidean_onsets(k: u32, n: u32, rotate: i32) -> Vec<bool> {
    let (k, n) = (k.max(1), n.max(1));
    (0..n)
        .map(|i| {
            let slot = (i as i32 + rotate).rem_euclid(n as i32) as u32;
            slot.wrapping_mul(k) % n < k
        })
        .collect()
}

/// Replace channel `ch`'s notes with `k` onsets over `n` slots across `len_steps`
/// (the pattern length in 16ths), rotated. Notes are 1 step long, the channel's root,
/// velocity `vel` — every fourth *emitted* onset accents to 1.0, whatever rotation does
/// to the slot positions.
pub fn euclidean_fill(notes: &mut Vec<Note>, ch: Id, k: u32, n: u32, rotate: i32, vel: f32, len_steps: u32) {
    notes.retain(|n| n.ch != ch);
    let onsets = euclidean_onsets(k, n, rotate);
    let slot = len_steps as f32 / n.max(1) as f32;
    let mut hits = 0usize;
    for (i, on) in onsets.iter().enumerate() {
        if *on {
            notes.push(Note {
                pan: 0.0,
                ch,
                step: i as f32 * slot,
                len: slot.clamp(0.25, 1.0),
                p: None,
                v: if hits.is_multiple_of(4) { 1.0 } else { vel },
                trig: Some(true),
            });
            hits += 1;
        }
    }
}

/// Spread simultaneous notes (same channel, same start — wherever they sit in the
/// array) by pitch, offsetting the i-th by `width` steps and nudging velocity by `ramp`
/// per note: chords become strums.
pub fn strum(notes: &mut [Note], width: f32, ramp: f32) {
    let mut groups: std::collections::HashMap<(Id, u64), Vec<usize>> = std::collections::HashMap::new();
    for (i, n) in notes.iter().enumerate() {
        groups.entry((n.ch, n.step.to_bits() as u64)).or_default().push(i);
    }
    let mut groups: Vec<Vec<usize>> = groups.into_values().filter(|g| g.len() > 1).collect();
    for g in &mut groups {
        g.sort_by_key(|&i| notes[i].p.unwrap_or(60));
        for (k, &i) in g.iter().enumerate() {
            let n = &mut notes[i];
            n.step += k as f32 * width;
            n.v = (n.v + k as f32 * ramp).clamp(0.0, 1.0);
        }
    }
}

/// Replace channel `ch`'s notes with a scale-constrained melody: `bars` bars of 16th
/// slots, a note on every `density` slot, the pitch a small random walk over scale
/// degrees. Bars past `repeat` replay that cycle's rhythm with the walk nudged ±1
/// degree, so the melody repeats instead of wandering. Like [`euclidean_fill`], a second
/// run (another seed) replaces the first instead of stacking on it.
pub fn melody(notes: &mut Vec<Note>, ch: Id, key_scale: (u8, u32), bars: u32, density: f32, repeat: u32, seed: u64) {
    notes.retain(|n| n.ch != ch);
    let rng = &mut SplitMix64::new(seed);
    let (key, scale_id) = key_scale;
    let mask = pitch::scale_mask(scale_id);
    let degrees: Vec<u8> = (0..12u16).filter(|pc| mask & (1 << pc) != 0).map(|pc| pc as u8).collect();
    let n = degrees.len() as i32;
    let mut deg: i32 = 0;
    let mut seq: Vec<(i32, f32)> = Vec::new(); // per-slot (degree index, len); -1 = rest
    let rep = repeat.max(1) as usize;
    for bar in 0..bars.max(1) as usize {
        // the first `repeat` bars invent the motif; every later bar replays its cycle's
        // rhythm with the walk nudged ±1 degree, so the melody repeats instead of wandering
        let build = bar < rep;
        for slot in 0..16usize {
            let (d, len) = if build {
                let len = if rng.unit() < 0.3 { 2.0 } else { 1.0 };
                if rng.unit() < density.clamp(0.05, 1.0) {
                    deg = (deg + [0, -1, 1, 2, -2][rng.below(5)]).clamp(0, n - 1);
                    seq.push((deg, len));
                    (deg, len)
                } else {
                    seq.push((-1, len));
                    (-1, len)
                }
            } else {
                let (sd, sl) = seq[(bar % rep) * 16 + slot];
                if sd < 0 {
                    (-1, sl)
                } else {
                    deg = (deg + [0, -1, 1][rng.below(3)]).clamp(0, n - 1);
                    (deg, sl)
                }
            };
            if d < 0 {
                continue;
            }
            let midi = (48 + key as i32 + degrees[d as usize] as i32) as u8;
            notes.push(Note { ch, step: (bar * 16 + slot) as f32, len, p: Some(midi), v: 0.8, trig: Some(false), pan: 0.0 });
        }
    }
}

// ---------------------------------------------------------------------------
// chord tools: tables, stamping, naming, harmonizing, progressions
// ---------------------------------------------------------------------------

/// The stampable chord types in picker order: `(id, semitone intervals from the root)`.
/// Pure theory data — the TS mirror pins the same table and the golden tests assert the
/// same voicings, so engine, browser and CLI agree on every chord.
pub const CHORD_TYPES: &[(&str, &[u8])] = &[
    ("maj", &[0, 4, 7]),
    ("min", &[0, 3, 7]),
    ("dim", &[0, 3, 6]),
    ("aug", &[0, 4, 8]),
    ("sus2", &[0, 2, 7]),
    ("sus4", &[0, 5, 7]),
    ("7", &[0, 4, 7, 10]),
    ("maj7", &[0, 4, 7, 11]),
    ("min7", &[0, 3, 7, 10]),
    ("9", &[0, 4, 7, 10, 14]),
    ("add9", &[0, 4, 7, 14]),
    ("maj9", &[0, 4, 7, 11, 14]),
    ("min9", &[0, 3, 7, 10, 14]),
    ("6", &[0, 4, 7, 9]),
    ("m6", &[0, 3, 7, 9]),
    ("dim7", &[0, 3, 6, 9]),
    ("m7b5", &[0, 3, 6, 10]),
    ("5", &[0, 7]),
];

/// The suffix a chord type contributes to a detected name ([`chord_name`]).
const CHORD_SUFFIX: &[(&str, &str)] = &[
    ("maj", ""),
    ("min", "m"),
    ("dim", "°"),
    ("aug", "+"),
    ("sus2", "sus2"),
    ("sus4", "sus4"),
    ("7", "7"),
    ("maj7", "maj7"),
    ("min7", "m7"),
    ("9", "9"),
    ("add9", "add9"),
    ("maj9", "maj9"),
    ("min9", "m9"),
    ("6", "6"),
    ("m6", "m6"),
    ("dim7", "°7"),
    ("m7b5", "m7♭5"),
    ("5", "5"),
];

/// Harmony types the `chord` verb can spell; "diatonic" reads the project key/scale.
/// The stamp picker's exotic types stay stamp-only — they harmonize poorly by rule.
pub const HARMONY_TYPES: &[&str] = &["diatonic", "maj", "min", "7", "maj7", "min7"];

/// Progression templates: one diatonic 7th chord per bar, as scale-degree indices.
/// `chance` (a seeded random walk) is a verb mode, not a row here.
pub const PROGRESSION_TEMPLATES: &[(&str, &[u8])] = &[
    ("pop", &[0, 4, 5, 3]),               // I V vi IV
    ("pop2", &[5, 3, 0, 4]),              // vi IV I V
    ("minloop", &[0, 5, 2, 6]),           // i VI III VII
    ("trap", &[0, 3, 5, 4]),              // i iv VI v
    ("iivI", &[1, 4, 0]),                 // ii V I
    ("doowop", &[0, 5, 3, 4]),            // I vi IV V
    ("andalusian", &[0, 6, 5, 4]),        // i VII VI v
    ("blues", &[0, 3, 4]),                // i iv v
    ("canon", &[0, 4, 5, 2, 3, 0, 3, 4]), // 8-bar canon cycle
];

const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// Intervals of a chord type, or `None` for an unknown id.
pub fn chord_intervals(type_id: &str) -> Option<&'static [u8]> {
    CHORD_TYPES.iter().find(|(id, _)| *id == type_id).map(|(_, iv)| *iv)
}

fn chord_suffix(type_id: &str) -> &'static str {
    CHORD_SUFFIX.iter().find(|(id, _)| *id == type_id).map(|(_, s)| *s).unwrap_or("")
}

/// Absolute pitches of a chord rooted at `root` (a MIDI note), rotated `inversion`
/// times: each rotation lifts the lowest voice an octave, clamped to the voicing's
/// last inversion. Returned ascending. `None` for an unknown type or a voicing that
/// leaves the MIDI range — checked *after* rotating, since lifting a voice can push
/// the top past 127 even when the root position fit.
pub fn chord_pitches(root: u8, type_id: &str, inversion: u32) -> Option<Vec<u8>> {
    let iv = chord_intervals(type_id)?;
    let mut p: Vec<u8> = iv.iter().map(|&i| root + i).collect();
    for _ in 0..inversion.min(iv.len() as u32 - 1) {
        let lo = p.iter().enumerate().min_by_key(|(_, &v)| v).map(|(i, _)| i).unwrap();
        p[lo] += 12;
    }
    if p.iter().any(|&v| v > 127) {
        return None;
    }
    p.sort_unstable();
    Some(p)
}

/// The chord a pitch set spells, bass-first: the lowest note's pitch class is tried
/// as the root before any other, so a root-position C6 is "C6", never "Am7" — while
/// the same set with A in the bass reads "Am7", which is what the ear hears. Unknown
/// sets fall back to the bass pitch class and "?".
pub fn chord_name(pitches: &[u8]) -> String {
    let Some(&bass) = pitches.iter().min() else {
        return "?".into();
    };
    let mut pcs: Vec<u8> = pitches.iter().map(|p| p % 12).collect();
    pcs.sort_unstable();
    pcs.dedup();
    let bass_pc = bass % 12;
    let candidates = std::iter::once(bass_pc).chain(pcs.iter().copied().filter(|pc| *pc != bass_pc));
    for pc in candidates {
        let mut rel: Vec<u8> = pcs.iter().map(|p| (p + 12 - pc) % 12).collect();
        rel.sort_unstable();
        for (id, iv) in CHORD_TYPES {
            let mut sig: Vec<u8> = iv.iter().map(|i| i % 12).collect();
            sig.sort_unstable();
            sig.dedup();
            if rel == sig {
                return format!("{}{}", NOTE_NAMES[pc as usize], chord_suffix(id));
            }
        }
    }
    format!("{}?", NOTE_NAMES[bass_pc as usize])
}

/// The scale's semitone offsets from the key, ascending (the set bits of the mask).
pub(crate) fn scale_offsets(scale_id: u32) -> Vec<u8> {
    let mask = pitch::scale_mask(scale_id);
    (0..12u16).filter(|k| mask & (1 << k) != 0).map(|k| k as u8).collect()
}

/// The diatonic stack on scale degree `deg` — degrees deg, deg+2, deg+4 (+deg+6 for a
/// seventh) — as semitones above the key, octave-wrapped.
fn diatonic_stack(scale_id: u32, deg: usize, seventh: bool) -> Vec<u8> {
    let offs = scale_offsets(scale_id);
    let n = offs.len();
    let idx: &[usize] = if seventh { &[0, 2, 4, 6] } else { &[0, 2, 4] };
    idx.iter()
        .map(|&i| {
            let k = deg + i;
            offs[k % n] + 12 * (k / n) as u8
        })
        .collect()
}

/// Expand a progression mode into per-bar scale degrees: a named template cycles, or
/// `chance` walks ±2 degrees from a seeded random start. `None` for an unknown id.
pub fn progression_degrees(mode: &str, bars: usize, seed: u64, scale_degrees: usize) -> Option<Vec<usize>> {
    let n = scale_degrees;
    if mode == "chance" {
        let rng = &mut SplitMix64::new(seed);
        let mut d = rng.below(n);
        let mut out = Vec::with_capacity(bars);
        for _ in 0..bars {
            out.push(d);
            d = (d + rng.below(5) + n - 2) % n.max(1);
        }
        Some(out)
    } else {
        let t = PROGRESSION_TEMPLATES.iter().find(|(id, _)| *id == mode).map(|(_, d)| *d)?;
        Some((0..bars).map(|b| t[b % t.len()] as usize).collect())
    }
}

/// Generate one diatonic 7th chord per degree on channel `ch`: chord on beat 1 of
/// each bar (16 steps, a bar long on the 16th grid), roots in the C3..B3 window.
/// With `voice_lead`, every chord after the first picks the rotation (lift the
/// lowest voices an octave) minimizing total movement against the previous voicing
/// — the harmony stays, the hands move less.
pub fn progression(notes: &mut Vec<Note>, ch: Id, key: u8, scale_id: u32, degrees: &[usize], voice_lead: bool) {
    // the generator owns its channel: like `euclidean_fill`, its previous notes make
    // way (a selection limits which old notes the verb sees at all)
    notes.retain(|n| n.ch != ch);
    let mut chords: Vec<Vec<u8>> = degrees
        .iter()
        .map(|&d| {
            let stack = diatonic_stack(scale_id, d, true);
            let base = 48 + ((key + stack[0]) % 12);
            stack.iter().map(|&s| base + (s - stack[0])).collect()
        })
        .collect();
    if voice_lead {
        for i in 1..chords.len() {
            let mut prev = chords[i - 1].clone();
            prev.sort_unstable();
            let mut best: Option<(u32, Vec<u8>)> = None;
            for r in 0..chords[i].len() {
                let mut cand = chords[i].clone();
                for c in cand.iter_mut().take(r) {
                    *c += 12;
                }
                let mut sorted = cand.clone();
                sorted.sort_unstable();
                let cost: u32 =
                    sorted.iter().enumerate().map(|(j, &v)| (v as i32 - prev[j.min(prev.len() - 1)] as i32).unsigned_abs()).sum();
                if best.as_ref().is_none_or(|(bc, _)| cost < *bc) {
                    best = Some((cost, cand));
                }
            }
            chords[i] = best.map(|(_, c)| c).unwrap_or_else(|| chords[i].clone());
        }
    }
    for (bar, chd) in chords.iter().enumerate() {
        let mut ps = chd.clone();
        ps.sort_unstable();
        for p in ps {
            notes.push(Note { ch, step: (bar * 16) as f32, len: 16.0, p: Some(p), v: 0.75, trig: Some(false), pan: 0.0 });
        }
    }
}

/// Harmonize single notes: every (channel, step) group holding exactly one note gains
/// the tones of `harmony` above it (roots stay; a tone any channel already sounds at
/// that step is not doubled). `diatonic` spells the triad from the project key/scale
/// and leaves out-of-scale roots alone — chromatic passing notes stay unharmonized;
/// named types spell by interval from the root, whatever the key. A `None` pitch
/// resolves to the channel root, like [`quantize`].
/// Harmonize single notes: every (channel, step) group of `notes` holding exactly one
/// note gains the tones of `harmony` above it (roots stay; a tone already sounding is
/// not doubled). Duplication is checked against `all` — the pattern's full note set —
/// so a tone another channel (or an unselected note) already sounds at that step is
/// never doubled. `diatonic` spells the triad from the project key/scale and leaves
/// out-of-scale roots alone — chromatic passing notes stay unharmonized; named types
/// spell by interval from the root, whatever the key. A `None` pitch resolves to the
/// channel root, like [`quantize`].
pub fn chord(notes: &mut Vec<Note>, all: &[Note], key_scale: Option<(u8, u32)>, harmony: &str, roots: &[(Id, u8)]) {
    // group indices by (channel, step) — the same grouping rule as `strum`; processed
    // in key order so the appended tones land deterministically, and indices stay
    // valid because only pushes happen below
    let mut groups: std::collections::HashMap<(Id, u64), Vec<usize>> = std::collections::HashMap::new();
    for (i, n) in notes.iter().enumerate() {
        groups.entry((n.ch, n.step.to_bits() as u64)).or_default().push(i);
    }
    let mut groups: Vec<((Id, u64), Vec<usize>)> = groups.into_iter().collect();
    groups.sort_by_cached_key(|(k, _)| *k);
    for ((cid, _), g) in groups {
        if g.len() != 1 {
            continue; // a step that already has a chord is not a root
        }
        let i = g[0];
        let step = notes[i].step;
        let len = notes[i].len;
        let vel = (notes[i].v * 0.9).clamp(0.05, 1.0);
        let Some(root) = notes[i].p.or_else(|| roots.iter().find(|(id, _)| *id == cid).map(|(_, r)| *r)) else {
            continue;
        };
        let ivs = if harmony == "diatonic" {
            let Some((key, scale_id)) = key_scale else { continue };
            let pc = root % 12;
            let deg = scale_offsets(scale_id).iter().position(|&o| (key + o) % 12 == pc);
            let Some(deg) = deg else { continue }; // out of scale: leave it alone
            let stack = diatonic_stack(scale_id, deg, false);
            stack[1..].iter().map(|&s| s - stack[0]).collect::<Vec<u8>>()
        } else {
            match chord_intervals(harmony) {
                Some(iv) => iv[1..].to_vec(),
                None => continue,
            }
        };
        for v in ivs {
            let p = root + v;
            // `notes` covers tones added earlier in this run, `all` the rest of the
            // pattern (unselected notes, other channels)
            if !notes
                .iter()
                .chain(all.iter())
                .any(|x| x.step == step && x.p.or_else(|| roots.iter().find(|(id, _)| *id == x.ch).map(|(_, r)| *r)) == Some(p))
            {
                notes.push(Note { ch: cid, step, len, p: Some(p), v: vel, trig: Some(false), pan: 0.0 });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn note(ch: Id, step: f32, v: f32) -> Note {
        Note { ch, step, len: 1.0, p: Some(60), v, trig: Some(false), pan: 0.0 }
    }

    #[test]
    fn same_seed_same_notes() {
        let a = &mut [note(1, 4.0, 0.8), note(1, 8.0, 0.9)];
        let b = &mut [note(1, 4.0, 0.8), note(1, 8.0, 0.9)];
        humanize(a, 0.5, 0.4, &mut SplitMix64::new(41));
        humanize(b, 0.5, 0.4, &mut SplitMix64::new(41));
        assert_eq!(a, b);
    }

    #[test]
    fn different_seed_different_notes() {
        let a = &mut [note(1, 4.0, 0.8)];
        let b = &mut [note(1, 4.0, 0.8)];
        humanize(a, 0.5, 0.4, &mut SplitMix64::new(1));
        humanize(b, 0.5, 0.4, &mut SplitMix64::new(2));
        assert_ne!(a, b);
    }

    #[test]
    fn humanize_zero_is_identity() {
        let a = &mut [note(1, 4.0, 0.8)];
        let b = [note(1, 4.0, 0.8)];
        humanize(a, 0.0, 0.0, &mut SplitMix64::new(9));
        assert_eq!(a[0].step, b[0].step);
        assert_eq!(a[0].v, b[0].v);
    }

    #[test]
    fn humanize_never_starts_before_zero() {
        let a = &mut [note(1, 0.05, 0.8)];
        humanize(a, 1.0, 0.0, &mut SplitMix64::new(3));
        assert!(a[0].step >= 0.0);
    }

    #[test]
    fn quantize_snaps_to_the_grid_by_amount() {
        let a = &mut [note(1, 4.4, 0.8)];
        quantize(a, 1.0, 1.0, None, &[]);
        assert_eq!(a[0].step, 4.0);
        let b = &mut [note(1, 4.4, 0.8)];
        quantize(b, 1.0, 0.5, None, &[]);
        assert!((b[0].step - 4.2).abs() < 1e-5);
    }

    #[test]
    fn scale_aware_quantize_snaps_pitch_into_the_scale() {
        // C major: the C# is equidistant from C and D; pitch::snap keeps the lower
        let a = &mut [Note { ch: 1, step: 4.0, len: 1.0, p: Some(61), v: 0.8, trig: Some(false), pan: 0.0 }];
        quantize(a, 1.0, 1.0, Some((0, 1)), &[(1, 60)]);
        assert_eq!(a[0].p, Some(60));
        // the D# is equidistant from D and E; pitch::snap keeps the lower
        let b = &mut [Note { ch: 1, step: 4.0, len: 1.0, p: Some(63), v: 0.8, trig: Some(false), pan: 0.0 }];
        quantize(b, 1.0, 1.0, Some((0, 1)), &[(1, 60)]);
        assert_eq!(b[0].p, Some(62));
        // every out-of-scale chromatic note ties between its in-scale neighbours;
        // pitch::snap keeps the lower — F# (66) lands on F (65)
        let c = &mut [Note { ch: 1, step: 4.0, len: 1.0, p: Some(66), v: 0.8, trig: Some(false), pan: 0.0 }];
        quantize(c, 1.0, 1.0, Some((0, 1)), &[(1, 60)]);
        assert_eq!(c[0].p, Some(65));
    }

    #[test]
    fn zero_amount_keeps_quiet_velocities() {
        // velocities below the old 0.05 floor must survive amount 0 unchanged
        let h = &mut [note(1, 4.0, 0.02)];
        humanize(h, 0.0, 0.0, &mut SplitMix64::new(9));
        assert_eq!(h[0].v, 0.02);
        let s = &mut [note(1, 4.0, 0.02), Note { ch: 1, step: 4.0, len: 1.0, p: Some(64), v: 0.02, trig: Some(false), pan: 0.0 }];
        strum(s, 0.25, 0.0);
        assert!(s.iter().all(|n| n.v == 0.02));
        // and a full nudge still stays inside 0..=1
        let h = &mut [note(1, 4.0, 0.02), note(1, 5.0, 1.0)];
        humanize(h, 0.0, 1.0, &mut SplitMix64::new(9));
        assert!(h.iter().all(|n| (0.0..=1.0).contains(&n.v)));
    }

    #[test]
    fn scale_aware_quantize_stays_in_midi_range() {
        // C# major pentatonic (key 1, scale 3): 127 would snap up to 128
        let a = &mut [Note { ch: 1, step: 0.0, len: 1.0, p: Some(127), v: 0.8, trig: Some(false), pan: 0.0 }];
        quantize(a, 1.0, 1.0, Some((1, 3)), &[]);
        assert_eq!(a[0].p, Some(125));
        // the bottom edge: 0 (C) in B major pentatonic (B C# D# F# G#) — nearest in-range is 1
        let b = &mut [Note { ch: 1, step: 0.0, len: 1.0, p: Some(0), v: 0.8, trig: Some(false), pan: 0.0 }];
        quantize(b, 1.0, 1.0, Some((11, 3)), &[]);
        assert_eq!(b[0].p, Some(1));
        // everywhere in range, the bounded snap agrees with pitch::snap
        for key in 0..12u32 {
            for scale in 0..5u32 {
                let mask = pitch::scale_mask(scale);
                for m in 0..=127u8 {
                    let s = pitch::snap(m as f32, key, mask);
                    if (0.0..=127.0).contains(&s) {
                        assert_eq!(snap_midi(m as f32, key, mask), s as u8, "key {key} scale {scale} m {m}");
                    }
                }
            }
        }
    }

    #[test]
    fn melody_replaces_its_channel() {
        let a = &mut vec![note(3, 0.5, 0.9), note(4, 2.0, 0.9)];
        melody(a, 3, (0, 1), 1, 0.5, 1, 1);
        melody(a, 3, (0, 1), 1, 0.5, 1, 2);
        let b = &mut vec![note(4, 2.0, 0.9)];
        melody(b, 3, (0, 1), 1, 0.5, 1, 2);
        assert_eq!(a, b, "a second run replaces the first; other channels stay");
    }

    #[test]
    fn euclidean_four_on_floor() {
        let onsets = euclidean_onsets(4, 16, 0);
        assert_eq!(onsets.iter().filter(|o| **o).count(), 4);
        assert!(onsets[0] && onsets[4] && onsets[8] && onsets[12]);
    }

    #[test]
    fn euclidean_fill_replaces_its_channel_and_keeps_the_others() {
        let notes = &mut vec![note(1, 0.0, 1.0), note(2, 2.0, 1.0)];
        euclidean_fill(notes, 1, 5, 16, 0, 0.7, 16);
        assert_eq!(notes.iter().filter(|n| n.ch == 1).count(), 5);
        assert!(notes.iter().any(|n| n.ch == 2 && n.step == 2.0)); // the other channel survives
    }

    #[test]
    fn strum_spreads_a_chord_by_pitch() {
        let notes = &mut [
            note(1, 4.0, 0.8),
            Note { ch: 1, step: 4.0, len: 1.0, p: Some(64), v: 0.8, trig: Some(false), pan: 0.0 },
            Note { ch: 1, step: 4.0, len: 1.0, p: Some(57), v: 0.8, trig: Some(false), pan: 0.0 },
        ];
        strum(notes, 0.25, 0.05);
        // array positions stay, offsets follow pitch rank: lowest starts first
        let by = |p: u8| notes.iter().find(|n| n.p == Some(p)).unwrap().step;
        assert_eq!(by(57), 4.0);
        assert_eq!(by(60), 4.25);
        assert_eq!(by(64), 4.5);
        let vof = |p: u8| notes.iter().find(|n| n.p == Some(p)).unwrap().v;
        assert_eq!(vof(57), 0.8);
        assert_eq!(vof(60), 0.85);
        assert_eq!(vof(64), 0.90000004);
    }

    #[test]
    fn melody_is_scale_constrained_and_deterministic() {
        let a = &mut vec![];
        melody(a, 3, (0, 1), 2, 0.5, 1, 77);
        assert!(a.iter().all(|n| n.ch == 3));
        assert!(a.iter().all(|n| n.p.is_some()));
        // every pitch in C major
        assert!(a.iter().all(|n| [0, 2, 4, 5, 7, 9, 11].contains(&(n.p.unwrap() % 12))));
        let b = &mut vec![];
        melody(b, 3, (0, 1), 2, 0.5, 1, 77);
        assert_eq!(a, b);
    }

    #[test]
    fn melody_repeat_replays_the_motif() {
        let a = &mut vec![];
        melody(a, 3, (0, 1), 2, 0.5, 2, 41);
        let cycle = a.len();
        assert!(cycle > 0);
        let b = &mut vec![];
        melody(b, 3, (0, 1), 4, 0.5, 2, 41);
        // bars 3–4 replay bars 1–2's rhythm: exactly twice the notes, first half identical
        assert_eq!(b.len(), cycle * 2);
        assert_eq!(&b[..cycle], &a[..]);
        // the replay nudges the walk, so it is not a byte-copy of the cycle
        let same = a.iter().zip(b[cycle..].iter()).filter(|(x, y)| x.p == y.p).count();
        assert!(same < cycle, "replayed bars must deviate from the motif");
    }

    #[test]
    fn strum_groups_nonadjacent_same_step_notes() {
        let notes = &mut [
            note(1, 4.0, 0.8),
            Note { ch: 1, step: 8.0, len: 1.0, p: Some(64), v: 0.8, trig: Some(false), pan: 0.0 },
            Note { ch: 1, step: 4.0, len: 1.0, p: Some(57), v: 0.8, trig: Some(false), pan: 0.0 },
        ];
        strum(notes, 0.25, 0.0);
        // the two step-4 notes form one chord (lowest first) even though they are
        // separated by the step-8 note; the lone step-8 note does not move
        assert!(notes.iter().any(|n| n.p == Some(57) && (n.step - 4.0).abs() < 1e-4));
        assert!(notes.iter().any(|n| n.p == Some(60) && (n.step - 4.25).abs() < 1e-4));
        assert!(notes.iter().any(|n| n.p == Some(64) && (n.step - 8.0).abs() < 1e-4));
    }

    // --- chord tools -------------------------------------------------------

    const AT_C4: &[(&str, &str)] = &[
        ("maj", "60,64,67"),
        ("min", "60,63,67"),
        ("dim", "60,63,66"),
        ("aug", "60,64,68"),
        ("sus2", "60,62,67"),
        ("sus4", "60,65,67"),
        ("7", "60,64,67,70"),
        ("maj7", "60,64,67,71"),
        ("min7", "60,63,67,70"),
        ("9", "60,64,67,70,74"),
        ("add9", "60,64,67,74"),
        ("maj9", "60,64,67,71,74"),
        ("min9", "60,63,67,70,74"),
        ("6", "60,64,67,69"),
        ("m6", "60,63,67,69"),
        ("dim7", "60,63,66,69"),
        ("m7b5", "60,63,66,70"),
        ("5", "60,67"),
    ];

    #[test]
    fn stamp_table_matches_the_pinned_voicings() {
        for (id, want) in AT_C4 {
            let got = chord_pitches(60, id, 0).unwrap();
            let got: Vec<String> = got.iter().map(|p| p.to_string()).collect();
            assert_eq!(&got.join(","), want, "type {id}");
        }
        assert_eq!(CHORD_TYPES.len(), 18);
    }

    #[test]
    fn inversions_lift_the_lowest_voice_and_clamp() {
        // min7 at A3, root..3rd
        let want = [[57, 60, 64, 67], [60, 64, 67, 69], [64, 67, 69, 72], [67, 69, 72, 76]];
        for (inv, want) in want.iter().enumerate() {
            assert_eq!(chord_pitches(57, "min7", inv as u32).unwrap(), want);
        }
        // a two-note voicing has one inversion; a triad has two (inv 5 clamps to the 2nd)
        assert_eq!(chord_pitches(60, "5", 3).unwrap(), [67, 72]);
        assert_eq!(chord_pitches(60, "maj", 5).unwrap(), [67, 72, 76]);
    }

    #[test]
    fn chord_pitches_rejects_unknown_types_and_roots_past_127() {
        assert!(chord_pitches(60, "nope", 0).is_none());
        assert!(chord_pitches(118, "maj", 0).is_some()); // top voice 125
        assert!(chord_pitches(121, "maj", 0).is_none()); // top voice would be 128
        assert!(chord_pitches(114, "maj9", 0).is_none()); // top voice would be 128
                                                          // the limit is checked after rotating: lifting the root an octave overflows
        assert!(chord_pitches(118, "maj", 1).is_none()); // would be [122, 125, 130]
    }

    #[test]
    fn chord_name_is_bass_first() {
        assert_eq!(chord_name(&[60, 64, 67, 69]), "C6"); // never "Am7"
        assert_eq!(chord_name(&[57, 60, 64, 67]), "Am7"); // the same set, A in the bass
        assert_eq!(chord_name(&[60, 63, 66, 69]), "C°7");
        assert_eq!(chord_name(&[60, 63, 66, 70]), "Cm7♭5");
        assert_eq!(chord_name(&[60, 64, 67, 70, 74]), "C9");
        assert_eq!(chord_name(&[60, 64, 67, 74]), "Cadd9");
        assert_eq!(chord_name(&[60, 61]), "C?"); // unknown sets say so
        assert_eq!(chord_name(&[]), "?");
    }

    fn a_minor() -> (u8, u32) {
        (9, 2)
    }

    #[test]
    fn chord_harmonizes_diatonic_triads_and_skips_out_of_scale_roots() {
        // E3 in A natural minor is degree v: E G B
        let notes = &mut vec![Note { ch: 1, step: 0.0, len: 2.0, p: Some(52), v: 0.8, trig: Some(false), pan: 0.0 }];
        chord(notes, &notes.clone(), Some(a_minor()), "diatonic", &[(1, 60)]);
        let ps: Vec<u8> = notes.iter().map(|n| n.p.unwrap()).collect();
        assert_eq!(ps, [52, 55, 59]);
        // a chromatic root stays unharmonized — diatonic mode leaves passing notes alone
        let notes = &mut vec![Note { ch: 1, step: 0.0, len: 1.0, p: Some(61), v: 0.8, trig: Some(false), pan: 0.0 }];
        chord(notes, &notes.clone(), Some(a_minor()), "diatonic", &[(1, 60)]);
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn chord_harmonizes_named_types_by_interval_anywhere() {
        // C#3 + minor 7th = C#m7, spelled by the table whatever the project key is
        let notes = &mut vec![Note { ch: 1, step: 0.0, len: 1.0, p: Some(49), v: 0.8, trig: Some(false), pan: 0.0 }];
        chord(notes, &notes.clone(), Some(a_minor()), "min7", &[(1, 60)]);
        let ps: Vec<u8> = notes.iter().map(|n| n.p.unwrap()).collect();
        assert_eq!(ps, [49, 52, 56, 59]);
    }

    #[test]
    fn chord_harmonizes_each_channel_and_never_doubles_a_sounding_tone() {
        // E3 (ch 1) and B3 (ch 2) share step 0; groups are (channel, step), so both are
        // roots — but E's chord must not double the B that is already sounding
        let notes = &mut vec![
            Note { ch: 1, step: 0.0, len: 2.0, p: Some(52), v: 0.8, trig: Some(false), pan: 0.0 },
            Note { ch: 2, step: 0.0, len: 2.0, p: Some(59), v: 0.8, trig: Some(false), pan: 0.0 },
        ];
        chord(notes, &notes.clone(), Some(a_minor()), "diatonic", &[(1, 60), (2, 60)]);
        // E gains G (B exists → skipped); B (degree ii°) gains D and F
        assert_eq!(notes.len(), 5);
        assert_eq!(notes.iter().filter(|n| n.p == Some(59)).count(), 1);
        let ch2: Vec<u8> = notes.iter().filter(|n| n.ch == 2).map(|n| n.p.unwrap()).collect();
        assert_eq!(ch2, [59, 62, 65]);
    }

    #[test]
    fn chord_ignores_steps_that_already_hold_a_chord() {
        let notes = &mut vec![
            Note { ch: 1, step: 0.0, len: 1.0, p: Some(52), v: 0.8, trig: Some(false), pan: 0.0 },
            Note { ch: 1, step: 0.0, len: 1.0, p: Some(55), v: 0.8, trig: Some(false), pan: 0.0 },
            Note { ch: 1, step: 4.0, len: 1.0, p: Some(57), v: 0.8, trig: Some(false), pan: 0.0 },
        ];
        chord(notes, &notes.clone(), Some(a_minor()), "diatonic", &[(1, 60)]);
        // step 0 has two notes (not a single root) and stays; step 4 becomes Am (+C, +E)
        assert_eq!(notes.len(), 5);
        assert_eq!(notes.iter().filter(|n| n.step == 0.0).count(), 2);
        let ps: Vec<u8> = notes.iter().filter(|n| n.step == 4.0).map(|n| n.p.unwrap()).collect();
        assert_eq!(ps, [57, 60, 64]);
    }

    #[test]
    fn chord_resolves_a_missing_pitch_to_the_channel_root() {
        // the root note keeps `p: None` (it still follows the channel root); the added
        // tones are spelled out explicitly
        let notes = &mut vec![Note { ch: 1, step: 0.0, len: 1.0, p: None, v: 0.8, trig: Some(false), pan: 0.0 }];
        chord(notes, &notes.clone(), Some(a_minor()), "maj", &[(1, 60)]);
        assert_eq!(notes[0].p, None);
        let ps: Vec<u8> = notes.iter().skip(1).map(|n| n.p.unwrap()).collect();
        assert_eq!(ps, [64, 67]);
    }

    #[test]
    fn chord_duplicate_detection_resolves_implicit_pitches() {
        // ch 2 has `p: None` with root 59 (B3) — it sounds B3 without spelling it, so
        // E3's chord must not add an explicit B3
        let notes = &mut vec![
            Note { ch: 1, step: 0.0, len: 2.0, p: Some(52), v: 0.8, trig: Some(false), pan: 0.0 },
            Note { ch: 2, step: 0.0, len: 2.0, p: None, v: 0.8, trig: Some(false), pan: 0.0 },
        ];
        chord(notes, &notes.clone(), Some(a_minor()), "diatonic", &[(1, 60), (2, 59)]);
        assert_eq!(notes.len(), 5); // E gains G only; B (implicit) gains D and F
        assert_eq!(notes.iter().filter(|n| n.p == Some(59)).count(), 0);
    }

    #[test]
    fn progression_minloop_matches_the_golden_voicings() {
        // i–VI–III–VII in A natural minor, 7th stacks, roots in C3..B3
        let notes = &mut vec![];
        progression(notes, 1, 9, 2, &[0, 5, 2, 6], false);
        let ps: Vec<u8> = notes.iter().map(|n| n.p.unwrap()).collect();
        assert_eq!(ps, [57, 60, 64, 67, 53, 57, 60, 64, 48, 52, 55, 59, 55, 59, 62, 65]);
        assert!(notes.iter().enumerate().all(|(i, n)| n.step == (i / 4 * 16) as f32 && n.len == 16.0 && (n.v - 0.75).abs() < 1e-6));
        // voice-lead picks inversions: F→C→G move as little as possible
        let notes = &mut vec![];
        progression(notes, 1, 9, 2, &[0, 5, 2, 6], true);
        let ps: Vec<u8> = notes.iter().map(|n| n.p.unwrap()).collect();
        assert_eq!(ps, [57, 60, 64, 67, 57, 60, 64, 65, 59, 60, 64, 67, 59, 62, 65, 67]);
    }

    #[test]
    fn progression_templates_cover_trap_andalusian_and_canon() {
        let names = |notes: &Vec<Note>, bars: usize| {
            (0..bars)
                .map(|b| {
                    let ps: Vec<u8> = notes.iter().skip(b * 4).take(4).map(|n| n.p.unwrap()).collect();
                    chord_name(&ps)
                })
                .collect::<Vec<String>>()
        };
        // root position (voice-lead off): the degree names read cleanly
        let t = &mut vec![];
        progression(t, 1, 9, 2, &[0, 3, 5, 4], false);
        assert_eq!(names(t, 4), ["Am7", "Dm7", "Fmaj7", "Em7"]);
        let a = &mut vec![];
        progression(a, 1, 9, 2, &[0, 6, 5, 4], false);
        assert_eq!(names(a, 4), ["Am7", "G7", "Fmaj7", "Em7"]);
        let c = &mut vec![];
        progression(c, 1, 9, 2, &[0, 4, 5, 2, 3, 0, 3, 4], false);
        assert_eq!(names(c, 8), ["Am7", "Em7", "Fmaj7", "Cmaj7", "Dm7", "Am7", "Dm7", "Em7"]);
        // voice-lead on: trap's last chord lands G-in-bass ([55,59,62,64]) — the same
        // pitch set as Em7, but bass-first naming reads it "G6", exactly like a chart
        let t = &mut vec![];
        progression(t, 1, 9, 2, &[0, 3, 5, 4], true);
        let last: Vec<u8> = t[12..].iter().map(|n| n.p.unwrap()).collect();
        assert_eq!(last, [55, 59, 62, 64]);
        assert_eq!(chord_name(&last), "G6");
    }

    #[test]
    fn progression_chance_is_seeded_and_deterministic() {
        let run = |seed: u64| {
            let degs = progression_degrees("chance", 4, seed, 7).unwrap();
            let notes = &mut vec![];
            progression(notes, 1, 9, 2, &degs, true);
            notes.iter().map(|n| n.p.unwrap()).collect::<Vec<u8>>()
        };
        assert_eq!(run(7), run(7));
        assert_ne!(run(7), run(8));
        // every degree stays inside the scale
        for d in progression_degrees("chance", 64, 3, 7).unwrap() {
            assert!(d < 7);
        }
    }

    #[test]
    fn progression_degrees_expands_templates() {
        assert_eq!(progression_degrees("pop", 6, 0, 7).unwrap(), [0, 4, 5, 3, 0, 4]);
        assert_eq!(progression_degrees("canon", 10, 0, 7).unwrap(), [0, 4, 5, 2, 3, 0, 3, 4, 0, 4]);
        assert!(progression_degrees("nope", 4, 0, 7).is_none());
    }

    /// Golden values for the chord tools at one fixed seed; the TS mirror pins the same
    /// numbers (`ui/src/lib/transforms.test.ts`), so engine and browser agree bit for bit.
    #[test]
    fn golden_chord_tool_values() {
        // chance walk, seed 2026, A minor, voice-lead on
        let degs = progression_degrees("chance", 4, 2026, 7).unwrap();
        assert_eq!(degs, [2, 4, 3, 2]);
        let notes = &mut vec![];
        progression(notes, 2, 9, 2, &degs, true);
        let ps: Vec<u8> = notes.iter().map(|n| n.p.unwrap()).collect();
        assert_eq!(ps, [48, 52, 55, 59, 52, 55, 59, 62, 53, 57, 60, 62, 52, 55, 59, 60]);
        let steps: Vec<f32> = notes.iter().map(|n| n.step).collect();
        assert_eq!(steps, [0.0, 0.0, 0.0, 0.0, 16.0, 16.0, 16.0, 16.0, 32.0, 32.0, 32.0, 32.0, 48.0, 48.0, 48.0, 48.0]);
    }

    /// Golden values for one fixed seed per verb; the TS mirror pins the same numbers
    /// (`ui/src/lib/transforms.test.ts`), so engine and browser mock agree bit for bit.
    /// Refresh by running with `--nocapture` and copying the H/Q/S/E/M lines.
    #[test]
    fn golden_fixed_seed_values() {
        let h = &mut [note(1, 4.0, 0.8)];
        humanize(h, 0.5, 0.4, &mut SplitMix64::new(41));
        assert_eq!(h[0].step, 4.033452);
        assert_eq!(h[0].v, 0.67452526);
        let q = &mut [note(1, 4.3, 0.8)];
        quantize(q, 1.0, 1.0, None, &[]);
        assert_eq!(q[0].step, 4.0);
        let s = &mut [
            note(1, 4.0, 0.8),
            Note { ch: 1, step: 4.0, len: 1.0, p: Some(64), v: 0.8, trig: Some(false), pan: 0.0 },
            Note { ch: 1, step: 4.0, len: 1.0, p: Some(57), v: 0.8, trig: Some(false), pan: 0.0 },
        ];
        strum(s, 0.25, 0.05);
        // array positions stay; the lowest pitch starts first, each next +width
        let by = |p: u8| s.iter().find(|n| n.p == Some(p)).unwrap();
        assert_eq!(by(57).step, 4.0);
        assert_eq!(by(60).step, 4.25);
        assert_eq!(by(64).step, 4.5);
        assert_eq!(by(64).v, 0.90000004);
        let e = &mut vec![note(1, 2.0, 0.9)];
        euclidean_fill(e, 1, 5, 16, 0, 0.7, 16);
        let steps: Vec<f32> = e.iter().map(|n| n.step).collect();
        assert_eq!(steps, [0.0, 4.0, 7.0, 10.0, 13.0]);
        let vel: Vec<f32> = e.iter().map(|n| n.v).collect();
        assert_eq!(vel, [1.0, 0.7, 0.7, 0.7, 1.0]);
        let m = &mut vec![];
        melody(m, 3, (9, 2), 2, 0.55, 1, 7);
        let mp: Vec<u8> = m.iter().map(|n| n.p.unwrap()).collect();
        assert_eq!(mp, [57, 57, 57, 59, 59, 59, 62, 65, 64, 62, 64, 64, 62, 62, 62, 62]);
        let ms: Vec<f32> = m.iter().map(|n| n.step).collect();
        assert_eq!(ms, [1.0, 2.0, 3.0, 5.0, 8.0, 10.0, 12.0, 15.0, 17.0, 18.0, 19.0, 21.0, 24.0, 26.0, 28.0, 31.0]);
    }
}
