//! Convert to MIDI, melody: the notes of a sung (or played, monophonic) audio clip as Piano roll
//! notes on the song's grid — Ableton's *Convert Melody to New MIDI Track*, FL's NewTone *Send to
//! piano roll*. The notes are the pitch editor's (the note model's when installed, else the note
//! tracker's, with the clip's pitch edits); this file only turns them into steps.
//!
//! What the musician can set is what the comparison of those tools showed people fix by hand
//! afterwards: the shortest note kept (breath noise, consonant blips), the timing (as sung, or
//! snapped to a grid), velocity (following the singing, or flat) and a key range (overtone or
//! octave slips outside the melody).

use serde::{Deserialize, Serialize};

/// Steps per beat (a step is a 1/16 at 4/4).
const STEPS_PER_BEAT: f64 = 4.0;
/// Shortest note written, steps (the engine's shortest pattern note).
pub const MIN_STEPS: f64 = 0.25;

/// A note of the audio: seconds of the source, the pitch it sounds at (MIDI, fractional) and
/// its mean level (linear, any scale: only ratios count).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeardNote {
    pub start: f64,
    pub end: f64,
    pub pitch: f32,
    pub level: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MelodyOptions {
    /// Notes shorter than this are left out, seconds.
    pub shortest: f64,
    /// Grid the notes' starts and ends snap to, steps (1 = 1/16); 0 = as sung.
    pub quantize: f64,
    /// Velocity of every note (0..1); `None` = follow the level (0.35..1 of the loudest note).
    pub velocity: Option<f32>,
    /// Keys outside this range are left out (MIDI, inclusive); `None` = no limit.
    pub low: Option<u8>,
    pub high: Option<u8>,
}

impl Default for MelodyOptions {
    fn default() -> Self {
        MelodyOptions { shortest: 0.06, quantize: 0.0, velocity: None, low: None, high: None }
    }
}

/// A converted note: song steps (from the song's start), length in steps, key, velocity.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNote {
    pub step: f64,
    pub len: f64,
    pub p: u8,
    pub v: f32,
}

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// The notes in `region` (source seconds the clip plays), with `beat_of` mapping a source second
/// to the song's beat. The voice is monophonic: a note still sounding when the next starts ends
/// there (and goes when that leaves it shorter than [`MIN_STEPS`]).
pub fn melody(heard: &[HeardNote], region: (f64, f64), beat_of: impl Fn(f64) -> f64, o: &MelodyOptions) -> Vec<MidiNote> {
    let mut ns: Vec<HeardNote> = heard
        .iter()
        .filter(|n| n.pitch.is_finite() && n.end > n.start)
        .map(|n| HeardNote { start: n.start.max(region.0), end: n.end.min(region.1), ..*n })
        .filter(|n| n.end - n.start >= o.shortest.max(1e-6))
        .filter(|n| {
            let k = n.pitch.round();
            o.low.is_none_or(|l| k >= l as f32) && o.high.is_none_or(|h| k <= h as f32)
        })
        .collect();
    ns.sort_by(|a, b| a.start.total_cmp(&b.start));
    let loud = ns.iter().fold(1e-9f32, |m, n| m.max(n.level));
    let q = o.quantize.max(0.0);
    let snap = |s: f64| if q > 0.0 { (s / q).round() * q } else { s };
    let mut out: Vec<MidiNote> = vec![];
    for n in ns {
        let (a, b) = (beat_of(n.start) * STEPS_PER_BEAT, beat_of(n.end) * STEPS_PER_BEAT);
        let s = r3(snap(a));
        let e = r3(snap(b).max(s + q.max(MIN_STEPS)));
        let v = o.velocity.unwrap_or_else(|| 0.35 + 0.65 * (n.level / loud).clamp(0.0, 1.0)).clamp(0.0, 1.0);
        let v = (v * 100.0).round() / 100.0;
        let p = n.pitch.round().clamp(0.0, 127.0) as u8;
        if let Some(prev) = out.last_mut() {
            if prev.step + prev.len > s {
                prev.len = r3(s - prev.step);
                if prev.len < MIN_STEPS {
                    out.pop();
                }
            }
        }
        // two notes snapped onto one step: the later (the one heard last) wins
        if out.last().is_some_and(|x| x.step >= s) {
            out.pop();
        }
        out.push(MidiNote { step: s, len: r3(e - s).max(MIN_STEPS), p, v });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(start: f64, end: f64, pitch: f32, level: f32) -> HeardNote {
        HeardNote { start, end, pitch, level }
    }
    // 120 BPM, clip at beat 8 playing the source from 0 s: 2 beats per second
    fn beat(t: f64) -> f64 {
        8.0 + t * 2.0
    }

    #[test]
    fn as_sung_notes_keep_their_timing_keys_and_accents() {
        let n = melody(&[h(0.5, 1.0, 60.2, 0.5), h(1.0, 1.5, 61.6, 0.25)], (0.0, 10.0), beat, &MelodyOptions::default());
        assert_eq!(n.len(), 2);
        assert_eq!((n[0].step, n[0].len, n[0].p, n[0].v), (36.0, 4.0, 60, 1.0));
        assert_eq!((n[1].step, n[1].p), (40.0, 62));
        assert!((n[1].v - 0.675).abs() < 0.006, "{}", n[1].v);
    }

    #[test]
    fn blips_out_of_range_keys_and_what_the_clip_does_not_play_are_left_out() {
        let heard =
            [h(0.0, 0.03, 60.0, 1.0), h(0.1, 0.6, 40.0, 1.0), h(0.7, 1.2, 64.0, 1.0), h(1.3, 2.5, 67.0, 1.0), h(3.0, 3.5, 69.0, 1.0)];
        let o = MelodyOptions { low: Some(48), high: Some(84), ..Default::default() };
        let n = melody(&heard, (0.0, 2.0), beat, &o);
        assert_eq!(n.iter().map(|x| x.p).collect::<Vec<_>>(), vec![64, 67]);
        // the last kept note ends where the clip does
        assert_eq!(n[1].step + n[1].len, beat(2.0) * 4.0);
    }

    #[test]
    fn quantize_snaps_and_never_collapses_notes() {
        let heard = [h(0.02, 0.13, 60.0, 1.0), h(0.14, 0.2, 62.0, 1.0), h(0.26, 0.6, 64.0, 1.0)];
        let o = MelodyOptions { quantize: 1.0, shortest: 0.0, velocity: Some(0.8), ..Default::default() };
        let n = melody(&heard, (0.0, 10.0), beat, &o);
        // 0.02 s → step 32.16 → 32; 0.14 s → 33.12 → 33; 0.26 s → 34.08 → 34
        assert_eq!(n.iter().map(|x| (x.step, x.p)).collect::<Vec<_>>(), vec![(32.0, 60), (33.0, 62), (34.0, 64)]);
        assert!(n.iter().all(|x| x.len >= 1.0 && x.v == 0.8));
        // two notes landing on one step: one note, the later
        let n = melody(
            &[h(0.0, 0.05, 60.0, 1.0), h(0.05, 0.3, 62.0, 1.0)],
            (0.0, 10.0),
            beat,
            &MelodyOptions { quantize: 4.0, shortest: 0.0, ..Default::default() },
        );
        assert_eq!(n.iter().map(|x| x.p).collect::<Vec<_>>(), vec![62]);
    }

    #[test]
    fn overlapping_notes_are_made_monophonic_and_empty_input_is_fine() {
        let n = melody(&[h(0.0, 1.0, 60.0, 1.0), h(0.5, 1.0, 62.0, 1.0)], (0.0, 10.0), beat, &MelodyOptions::default());
        assert_eq!(n[0].len, 4.0);
        assert!(melody(&[], (0.0, 1.0), beat, &MelodyOptions::default()).is_empty());
        assert!(melody(&[h(0.0, 1.0, f32::NAN, 1.0)], (0.0, 1.0), beat, &MelodyOptions::default()).is_empty());
    }
}
