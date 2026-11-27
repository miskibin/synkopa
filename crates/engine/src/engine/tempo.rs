//! The compiled tempo map: beats ↔ seconds over the song.
//!
//! The project stores tempo points ([`crate::model::TempoPoint`]); this compiles them once
//! per timeline rebuild into a cumulative-seconds prefix, so the audio thread only searches
//! a small vector and evaluates closed forms — no integration, no allocation (the same
//! snapshot contract as the rest of the timeline). Ramps glide the BPM linearly in beats —
//! the value the editor draws is the value that plays — which keeps every conversion
//! closed-form (a logarithm at worst, Zrythm's integration formula).

use crate::model::{TempoPoint, MAX_BASE_BPM, MAX_TEMPO_BPM, MAX_TEMPO_POINTS, MIN_TEMPO_BPM, TOTAL_BEATS};

impl crate::model::Project {
    /// The compiled tempo map of this project (no points = the constant base tempo).
    /// Everything that converts beats to seconds or back outside the audio thread goes
    /// through this, so a mapped tempo is measured the way it plays.
    pub(crate) fn tempo_map(&self) -> TempoMap {
        TempoMap::compile(&self.tempo_points, self.bpm)
    }

    /// The map as `(beat, bpm)` events a discrete serializer can write — see
    /// [`TempoMap::midi_tempo_events`].
    pub fn midi_tempo_events(&self) -> Vec<(f64, f64)> {
        self.tempo_map().midi_tempo_events()
    }
}

/// The tempo map of one timeline snapshot.
#[derive(Clone, Debug)]
pub(crate) struct TempoMap {
    /// `(beat, bpm, ramps to the next point)`, sorted by beat; the first entry is beat 0.
    points: Vec<(f64, f64, bool)>,
    /// Cumulative seconds from beat 0 to each point (`secs[i]` ↔ `points[i].0`).
    secs: Vec<f64>,
}

/// Seconds spent between `a` and `x` beats (with `b` the segment's end) of one segment
/// whose BPM glides linearly (in beats) from `bpm_a` to `bpm_b`:
/// ∫ 60/bpm dx = 60·d·ln(bpm(x)/bpm_a)/(bpm_b − bpm_a), exact (Zrythm's formula).
fn segment_seconds(a: f64, x: f64, b: f64, bpm_a: f64, bpm_b: f64, ramp: bool) -> f64 {
    let d = b - a;
    if x <= a {
        return 0.0;
    }
    if !ramp || d <= 0.0 || (bpm_a - bpm_b).abs() < 1e-9 {
        return (x - a) * 60.0 / bpm_a;
    }
    let bpm_x = bpm_a + (bpm_b - bpm_a) * ((x - a) / d);
    60.0 * d * (bpm_x / bpm_a).ln() / (bpm_b - bpm_a)
}

/// The BPM at `x` beats inside the segment `[a, b)` gliding from `bpm_a` to `bpm_b`.
fn bpm_at_in(x: f64, a: f64, b: f64, bpm_a: f64, bpm_b: f64, ramp: bool) -> f64 {
    if !ramp || b <= a {
        return bpm_a;
    }
    let f = ((x - a) / (b - a)).clamp(0.0, 1.0);
    bpm_a + (bpm_b - bpm_a) * f
}

impl TempoMap {
    /// The constant map: one tempo everywhere (what an empty point list compiles to).
    /// The base tempo keeps the `SetBpm` range: it is the constant case, not a point.
    pub(crate) fn constant(bpm: f64) -> Self {
        Self { points: vec![(0.0, bpm.clamp(MIN_TEMPO_BPM, MAX_BASE_BPM), false)], secs: vec![0.0] }
    }

    /// Compiles `points` over the song; before the first point the tempo is `base_bpm`.
    /// Points are clamped and beat-sorted here too, so a hand-edited file cannot produce a
    /// map the write path would refuse.
    pub(crate) fn compile(points: &[TempoPoint], base_bpm: f64) -> Self {
        let mut pts: Vec<(f64, f64, bool)> =
            points.iter().map(|p| (p.beat.clamp(0.0, TOTAL_BEATS), p.bpm.clamp(MIN_TEMPO_BPM, MAX_TEMPO_BPM), p.ramp)).collect();
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        pts.truncate(MAX_TEMPO_POINTS);
        // the opener is the base tempo: it governs until the first point (a point at beat 0
        // overwrites it below, the last one at 0 winning). It keeps the `SetBpm` range —
        // clamping a 480 BPM project to the point maximum would slow the whole opener.
        let mut map = Self { points: vec![(0.0, base_bpm.clamp(MIN_TEMPO_BPM, MAX_BASE_BPM), false)], secs: vec![0.0] };
        for &(beat, bpm, ramp) in &pts {
            if beat <= 0.0 {
                let last = map.points.last_mut().unwrap();
                *last = (0.0, bpm, ramp);
                continue;
            }
            let &(pb, pbpm, pramp) = map.points.last().unwrap();
            let secs = map.secs.last().unwrap() + segment_seconds(pb, beat, beat, pbpm, bpm, pramp);
            map.points.push((beat, bpm, ramp));
            map.secs.push(secs);
        }
        map
    }

    /// One tempo everywhere: every conversion reduces to the constant case.
    pub(crate) fn is_constant(&self) -> bool {
        self.points.len() == 1
    }

    /// The tempo at `beat` (what `ProcessCtx::bpm` reports at a block starting there).
    pub(crate) fn bpm_at(&self, beat: f64) -> f64 {
        let i = self.segment_of(beat);
        let (a, bpm_a, ramp) = self.points[i];
        let b = self.points.get(i + 1).map_or(beat.max(a) + 1.0, |&(b, ..)| b);
        let bpm_b = self.points.get(i + 1).map_or(bpm_a, |&(_, bpm, _)| bpm);
        bpm_at_in(beat, a, b, bpm_a, bpm_b, ramp)
    }

    /// Seconds from beat `a` to beat `b` (both inside the song), integrating the map.
    pub(crate) fn seconds_between(&self, a: f64, b: f64) -> f64 {
        if b <= a {
            return 0.0;
        }
        self.seconds_at(b) - self.seconds_at(a)
    }

    /// The beat at `secs` seconds from beat 0 (the inverse of [`Self::seconds_between`]).
    /// Negative seconds back-extrapolate linearly at the opening tempo (a count-in).
    pub(crate) fn beat_at(&self, secs: f64) -> f64 {
        if secs < 0.0 {
            return secs * self.bpm_at(0.0) / 60.0;
        }
        let i = match self.secs.partition_point(|&s| s <= secs) {
            0 => 0,
            n => n - 1,
        };
        let (a, bpm_a, ramp) = self.points[i];
        let b = self.points.get(i + 1).map_or(TOTAL_BEATS, |&(b, ..)| b);
        let spb_a = 60.0 / bpm_a;
        let rem = secs - self.secs[i];
        if !ramp || b <= a {
            return a + rem / spb_a;
        }
        let bpm_b = self.points.get(i + 1).map_or(bpm_a, |&(_, bpm, _)| bpm);
        let d = b - a;
        if (bpm_b - bpm_a).abs() < 1e-9 {
            return a + rem * bpm_a / 60.0;
        }
        // s(u) = 60·d·ln(1 + c·u)/... with bpm(u) = bpm_a + (bpm_b−bpm_a)·u/d:
        // u = d·(exp(rem·(bpm_b−bpm_a)/(60·d)) − 1)·bpm_a/(bpm_b−bpm_a) — the log's inverse
        let k = rem * (bpm_b - bpm_a) / (60.0 * d);
        let u = d * (k.exp() - 1.0) * bpm_a / (bpm_b - bpm_a);
        a + u.clamp(0.0, d)
    }

    /// The first tempo point strictly after `beat` (the block walk splits there), if any.
    pub(crate) fn next_break_after(&self, beat: f64) -> Option<f64> {
        self.points.partition_point(|&(b, ..)| b <= beat + 1e-9).checked_sub(1).and_then(|i| {
            // partition_point over `b <= beat` gives the count of points at or before; the
            // next point after that is the first strictly after — except the opener at 0
            self.points.get(i + 1).map(|&(b, ..)| b).filter(|&b| b > beat + 1e-9)
        })
    }

    /// Seconds from beat 0 to `beat` (closed form over the prefix + one partial segment).
    /// Before the song's start (a count-in plays negative beats) it back-extrapolates
    /// linearly at the opening tempo, so the walk crosses beat 0 without teleporting.
    /// Callers that may pass negative beats use this — [`Self::seconds_between`] only
    /// covers `a <= b` inside the song.
    pub(crate) fn seconds_at(&self, beat: f64) -> f64 {
        if beat < 0.0 {
            return beat * 60.0 / self.bpm_at(0.0);
        }
        let i = self.segment_of(beat);
        let (a, bpm_a, ramp) = self.points[i];
        let b = self.points.get(i + 1).map_or(beat.max(a) + 1.0, |&(b, ..)| b);
        let bpm_b = self.points.get(i + 1).map_or(bpm_a, |&(_, bpm, _)| bpm);
        self.secs[i] + segment_seconds(a, beat, b, bpm_a, bpm_b, ramp)
    }

    /// The index of the segment `beat` plays in (the last point at or before it).
    fn segment_of(&self, beat: f64) -> usize {
        self.points.partition_point(|&(b, ..)| b <= beat + 1e-9).saturating_sub(1).min(self.points.len() - 1)
    }

    /// The map as `(beat, bpm)` events a discrete serializer can write — a MIDI file's tempo
    /// meta, which has no ramps: a hold segment is one event at its start, a ramp is one event
    /// per beat along it carrying that interval's *effective* BPM, `60·len / seconds_between`,
    /// so the file's elapsed time at every event beat is the engine's own integral (stepping
    /// the segment's start BPM instead accumulates drift along the glide). The opener (the
    /// base tempo at beat 0) is always the first event, so a constant map writes exactly one
    /// tempo event, as it always did.
    pub(crate) fn midi_tempo_events(&self) -> Vec<(f64, f64)> {
        let mut out: Vec<(f64, f64)> = Vec::with_capacity(self.points.len() + 8);
        for (i, &(a, bpm_a, ramp)) in self.points.iter().enumerate() {
            let Some(&(b, _, _)) = self.points.get(i + 1).filter(|_| ramp) else {
                out.push((a, bpm_a));
                continue;
            };
            // the interval [u, v) plays the BPM that spends exactly seconds_between(u, v)
            // seconds over its length (a degenerate interval keeps the segment's own value)
            let eff = |u: f64, v: f64| match self.seconds_between(u, v) {
                s if s > 1e-12 => 60.0 * (v - u) / s,
                _ => bpm_a,
            };
            // whole beats inside the glide, then the glide's end: one interval each, the
            // last one open (the glide's end beat is the next point's own event)
            let mut bounds: Vec<f64> = Vec::new();
            let mut x = a.floor() + 1.0;
            while x < b - 1e-9 {
                bounds.push(x);
                x += 1.0;
            }
            bounds.push(b);
            let mut u = a;
            for &v in &bounds {
                out.push((u, eff(u, v)));
                u = v;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(points: &[(f64, f64, bool)]) -> TempoMap {
        let pts: Vec<TempoPoint> = points.iter().map(|&(beat, bpm, ramp)| TempoPoint { beat, bpm, ramp }).collect();
        TempoMap::compile(&pts, 120.0)
    }

    #[test]
    fn the_constant_map_converts_like_the_old_bpm_math() {
        let m = TempoMap::constant(120.0);
        assert!(m.is_constant());
        assert_eq!(m.seconds_between(0.0, 4.0), 2.0);
        assert_eq!(m.seconds_between(2.0, 6.0), 2.0);
        assert_eq!(m.beat_at(3.0), 6.0);
        assert_eq!(m.bpm_at(17.0), 120.0);
    }

    #[test]
    fn a_step_changes_the_tempo_at_its_beat() {
        let m = map(&[(0.0, 120.0, false), (8.0, 60.0, false)]);
        assert_eq!(m.bpm_at(7.999), 120.0);
        assert_eq!(m.bpm_at(8.0), 60.0);
        // 8 beats at 120 (4 s) + 4 beats at 60 (4 s)
        assert_eq!(m.seconds_between(0.0, 12.0), 8.0);
        assert_eq!(m.beat_at(6.0), 10.0);
    }

    #[test]
    fn a_ramp_glides_linearly_in_beats() {
        let m = map(&[(0.0, 60.0, true), (8.0, 120.0, false)]);
        assert_eq!(m.bpm_at(4.0), 90.0, "halfway in beats is halfway in tempo");
        // linear BPM in beats: the exact integral is the log formula (Zrythm's)
        assert!((m.seconds_between(0.0, 8.0) - 60.0 * 8.0 * (2.0f64).ln() / 60.0).abs() < 1e-9);
        // the inversion returns to the beat
        for b in [0.0, 2.0, 4.0, 6.5, 8.0] {
            assert!((m.beat_at(m.seconds_between(0.0, b)) - b).abs() < 1e-9, "beat {b}");
        }
    }

    #[test]
    fn midi_tempo_events_elapsed_time_is_the_maps_own() {
        // MIDI's tempo meta has no ramps: each beat steps at its interval's effective BPM and
        // the file's elapsed time must equal the map's integral at every event beat (stepping
        // the segment's start BPM drifted 121 ms over one 90→140 glide — the #322 finding)
        let m = map(&[(0.0, 90.0, true), (8.0, 140.0, false)]);
        let evs = m.midi_tempo_events();
        assert_eq!(evs.len(), 9, "one event per beat of the glide plus its end: {evs:?}");
        // an event's BPM governs from its beat to the next event's: summing the intervals
        // under their starting event must rebuild the map's own integral
        let mut t = 0.0;
        let mut prev = evs[0];
        assert_eq!(prev.0, 0.0, "the opener is the first event");
        for &(beat, bpm) in &evs[1..] {
            t += (beat - prev.0) * 60.0 / prev.1;
            prev = (beat, bpm);
            assert!((t - m.seconds_between(0.0, beat)).abs() < 1e-9, "elapsed at beat {beat}: {t} vs {}", m.seconds_between(0.0, beat));
        }
        // and the whole glide lands on the engine's own number
        assert!((m.seconds_between(0.0, 8.0) - 60.0 * 8.0 * (140.0f64 / 90.0).ln() / 50.0).abs() < 1e-9);
    }

    #[test]
    fn a_ramp_with_fractional_bounds_still_times_exactly() {
        // a glide starting mid-beat between holds: one event per beat of the glide, the
        // partial first and last intervals included, each timing exactly
        let m = map(&[(0.0, 120.0, false), (4.5, 90.0, true), (8.5, 140.0, false), (10.0, 60.0, false)]);
        let evs = m.midi_tempo_events();
        assert_eq!(evs.iter().map(|e| e.0).collect::<Vec<_>>(), vec![0.0, 4.5, 5.0, 6.0, 7.0, 8.0, 8.5, 10.0], "{evs:?}");
        let mut t = 0.0;
        let mut prev = evs[0];
        for &(beat, bpm) in &evs[1..] {
            t += (beat - prev.0) * 60.0 / prev.1;
            prev = (beat, bpm);
            assert!((t - m.seconds_between(0.0, beat)).abs() < 1e-9, "elapsed at beat {beat}: {t} vs {}", m.seconds_between(0.0, beat));
        }
        // the holds keep their own values, not an interval average
        assert_eq!(evs[0], (0.0, 120.0));
        assert_eq!(evs[6], (8.5, 140.0));
        assert_eq!(evs[7], (10.0, 60.0));
    }

    #[test]
    fn before_the_first_point_the_base_bpm_governs() {
        let m = map(&[(8.0, 60.0, false)]);
        assert_eq!(m.bpm_at(4.0), 120.0);
        assert_eq!(m.bpm_at(8.0), 60.0);
        assert_eq!(m.seconds_between(0.0, 12.0), 4.0 + 4.0);
    }

    #[test]
    fn jumps_clamp_and_points_stay_sorted() {
        // bpm clamps; unsorted input sorts; two points at one beat = the last governs
        let m = map(&[(8.0, 1000.0, false), (0.0, 5.0, false), (4.0, 240.0, false), (4.0, 90.0, false)]);
        assert_eq!(m.bpm_at(1.0), 20.0);
        assert_eq!(m.bpm_at(5.0), 90.0);
        assert_eq!(m.bpm_at(9.0), 300.0);
        // beyond the last point the tempo holds
        assert_eq!(m.bpm_at(1000.0), 300.0);
    }

    #[test]
    fn next_break_splits_the_walk() {
        let m = map(&[(0.0, 120.0, false), (8.0, 60.0, false), (16.0, 90.0, false)]);
        assert_eq!(m.next_break_after(0.0), Some(8.0));
        assert_eq!(m.next_break_after(7.5), Some(8.0));
        assert_eq!(m.next_break_after(8.0), Some(16.0));
        assert_eq!(m.next_break_after(16.0), None);
        let c = TempoMap::constant(120.0);
        assert_eq!(c.next_break_after(0.0), None);
    }

    #[test]
    fn round_trips_through_both_conversions() {
        let m = map(&[(0.0, 90.0, false), (12.0, 180.0, true), (24.0, 60.0, false)]);
        let mut t = 0.0;
        let mut b = 0.0;
        for _ in 0..60 {
            let dt = 0.037;
            t += dt;
            b = m.beat_at(t);
            assert!((m.seconds_between(0.0, b) - t).abs() < 1e-9, "t {t} beat {b}");
        }
        assert!(b > 0.0);
    }
}

#[cfg(test)]
mod render_checks {
    use super::*;

    #[test]
    fn double_time_from_beat_four_integrates_to_three_seconds() {
        let pts = vec![TempoPoint { beat: 4.0, bpm: 240.0, ramp: false }];
        let m = TempoMap::compile(&pts, 120.0);
        assert_eq!(m.seconds_between(0.0, 8.0), 3.0);
        assert_eq!(m.bpm_at(4.0), 240.0);
    }

    #[test]
    fn negative_beats_back_extrapolate_at_the_opening_tempo() {
        // a count-in plays negative beats: the conversions cover them linearly so the walk
        // crosses beat 0 without teleporting (seconds_between itself assumes a <= b)
        let m = TempoMap::compile(&[TempoPoint { beat: 2.0, bpm: 240.0, ramp: false }], 120.0);
        assert_eq!(m.seconds_at(-4.0), -4.0 * 60.0 / 120.0);
        assert_eq!(m.seconds_between(-4.0, 0.0), 4.0 * 60.0 / 120.0);
        assert_eq!(m.beat_at(-2.0), -2.0 * 120.0 / 60.0);
        // the walk's arrival: the seconds at a negative beat plus the block's time, then back
        let b1 = m.beat_at(m.seconds_at(-4.0) + 0.5);
        assert!((b1 - (-4.0 + 0.5 * 120.0 / 60.0)).abs() < 1e-12);
    }

    #[test]
    fn the_base_tempo_keeps_the_setbpm_range() {
        // the base tempo is the constant case, not a point: 480 BPM is a legal project
        // (SetBpm allows 20..999) and must not be clamped to the point maximum of 300
        assert_eq!(TempoMap::constant(480.0).bpm_at(4.0), 480.0);
        assert_eq!(TempoMap::constant(999.0).bpm_at(1.0), 999.0);
        // the opener governs until the first point, at its own tempo
        let pts = vec![TempoPoint { beat: 2.0, bpm: 120.0, ramp: false }];
        let m = TempoMap::compile(&pts, 480.0);
        assert_eq!(m.bpm_at(1.0), 480.0);
        assert_eq!(m.bpm_at(2.0), 120.0);
        assert_eq!(m.seconds_between(0.0, 2.0), 2.0 * 60.0 / 480.0);
        assert_eq!(m.seconds_between(0.0, 3.0), 0.25 + 60.0 / 120.0);
    }
}
