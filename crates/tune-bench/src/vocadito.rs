//! Raw transcription against both independent note annotations in vocadito.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde_json::{json, Value};
use wave_engine::pitch::{analyze_with, F0Estimator};
use wave_engine::sources::{mono_of, Audio};

use crate::transcribe::{note_matches, tracker_notes, N};

const DOI: &str = "10.5281/zenodo.5578807";
const REFERENCES: [&str; 2] = ["A1", "A2"];
const REPEAT_GAP_MIN: f64 = -0.020;
const REPEAT_GAP_MAX: f64 = 0.120;
// Allow floating-point error in the gap subtraction at the inclusive endpoints.
const GAP_EPSILON: f64 = 1e-9;

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn tracks(path: &Path) -> Result<BTreeMap<u32, String>, String> {
    let text = read(path)?;
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    let Some(header) = lines.next() else {
        return Err(format!("{}: missing metadata header", path.display()));
    };
    let columns: Vec<_> = header.split(',').map(str::trim).collect();
    let column = |name| columns.iter().position(|&col| col == name).ok_or_else(|| format!("{}: missing {name} column", path.display()));
    let track_id = column("track_id")?;
    let singer_id = column("singer_id")?;
    let mut tracks = BTreeMap::new();
    for (row, line) in lines.enumerate().filter(|(_, line)| !line.trim().is_empty()) {
        let fields: Vec<_> = line.split(',').map(str::trim).collect();
        let track = fields.get(track_id).and_then(|id| id.parse::<u32>().ok()).filter(|&id| id > 0);
        let singer = fields.get(singer_id).filter(|id| !id.is_empty());
        let (Some(track), Some(singer)) = (track, singer) else {
            return Err(format!("{}:{}: expected a positive track_id and a singer_id", path.display(), row + 2));
        };
        if tracks.insert(track, singer.to_string()).is_some() {
            return Err(format!("{}:{}: duplicate track_id {track}", path.display(), row + 2));
        }
    }
    if tracks.is_empty() {
        return Err(format!("{}: no tracks in metadata", path.display()));
    }
    Ok(tracks)
}

// Chronological order, with pitch then end time breaking simultaneous-onset ties.
// Identical notes are interchangeable, even if the input orders them differently.
fn sort_notes(notes: &mut [N]) {
    notes.sort_by(|a, b| a.on.total_cmp(&b.on).then_with(|| a.p.total_cmp(&b.p)).then_with(|| a.off.total_cmp(&b.off)));
}

fn read_notes(path: &Path) -> Result<Vec<N>, String> {
    let text = read(path)?;
    let mut notes = vec![];
    for (row, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let invalid = || format!("{}:{}: expected onset seconds, pitch Hz, and duration seconds", path.display(), row + 1);
        let values: Vec<f64> = line.split(',').map(|v| v.trim().parse()).collect::<Result<_, _>>().map_err(|_| invalid())?;
        let [on, hz, duration] = values[..] else {
            return Err(invalid());
        };
        let off = on + duration;
        if !on.is_finite() || on < 0.0 || !hz.is_finite() || hz <= 0.0 || !off.is_finite() || off <= on {
            return Err(invalid());
        }
        let p = (69.0 + 12.0 * (hz.log2() - 440.0_f64.log2())) as f32;
        notes.push(N { on, off, p });
    }
    sort_notes(&mut notes);
    Ok(notes)
}

/// Each pair's second note index, in chronological order.
fn repeats(notes: &[N]) -> Vec<usize> {
    notes
        .windows(2)
        .enumerate()
        .filter_map(|(i, pair)| {
            let (first, second) = (pair[0], pair[1]);
            let key = |n: N| (f64::from(n.p) + 0.5).floor();
            let gap = second.on - first.off;
            (key(first) == key(second) && (REPEAT_GAP_MIN - GAP_EPSILON..=REPEAT_GAP_MAX + GAP_EPSILON).contains(&gap)).then_some(i + 1)
        })
        .collect()
}

fn rate(numerator: usize, denominator: usize) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
}

#[derive(Default)]
struct Repeats {
    preserved: usize,
    reference: usize,
}

impl Repeats {
    fn json(&self) -> Value {
        json!({
            "preservedPairs": self.preserved,
            "referencePairs": self.reference,
            "recall": rate(self.preserved, self.reference),
        })
    }
}

#[derive(Default)]
struct NoteScore {
    reference: usize,
    predicted: usize,
    onset_pitch: usize,
    onset_pitch_offset: usize,
    repeats: Repeats,
}

impl NoteScore {
    fn add(&mut self, reference: &[N], predicted: &[N], pairs: &[usize]) -> Vec<Option<usize>> {
        let matches = note_matches(reference, predicted, false);
        self.reference += reference.len();
        self.predicted += predicted.len();
        self.onset_pitch += matches.iter().flatten().count();
        self.onset_pitch_offset += note_matches(reference, predicted, true).iter().flatten().count();
        self.repeats.reference += pairs.len();
        self.repeats.preserved += pairs.iter().filter(|&&j| matches[j - 1].is_some() && matches[j].is_some()).count();
        matches
    }

    fn json(&self) -> Value {
        let metric = |matched| {
            json!({
                "matchedNotes": matched,
                "precision": rate(matched, self.predicted),
                "recall": rate(matched, self.reference),
                "f1": rate(2 * matched, self.predicted + self.reference),
            })
        };
        json!({
            "referenceNotes": self.reference,
            "predictedNotes": self.predicted,
            "COnP": metric(self.onset_pitch),
            "COnPOff": metric(self.onset_pitch_offset),
            "sameKeyRepeats": self.repeats.json(),
        })
    }
}

#[derive(Default)]
struct Scores {
    a1: NoteScore,
    a2: NoteScore,
    agreed: Repeats,
}

impl Scores {
    fn add(&mut self, a1: &[N], a2: &[N], predicted: &[N]) {
        let pairs1 = repeats(a1);
        let pairs2 = repeats(a2);
        let hits1 = self.a1.add(a1, predicted, &pairs1);
        let hits2 = self.a2.add(a2, predicted, &pairs2);
        let agreement = note_matches(a1, a2, false);

        for j in pairs1 {
            let (Some(k1), Some(k2)) = (agreement[j - 1], agreement[j]) else { continue };
            if k1 + 1 != k2 || pairs2.binary_search(&k2).is_err() {
                continue;
            }
            self.agreed.reference += 1;
            if let (Some(p1), Some(p2)) = (hits1[j - 1], hits1[j]) {
                // One-to-one matching already guarantees p1 != p2.
                self.agreed.preserved += usize::from(hits2[k1] == Some(p1) && hits2[k2] == Some(p2));
            }
        }
    }
}

pub fn run(root: &Path, est: Option<&dyn F0Estimator>) -> Result<Value, String> {
    let tracks = tracks(&root.join("vocadito_metadata.csv"))?;
    let singers: BTreeSet<_> = tracks.values().collect();
    let notes = root.join("Annotations/Notes");
    let audio = root.join("Audio");
    let mut scores = Scores::default();
    let mut backends = BTreeMap::new();

    for id in tracks.keys() {
        let stem = format!("vocadito_{id}");
        let [a1, a2] = REFERENCES.map(|annotator| notes.join(format!("{stem}_notes{annotator}.csv")));
        let a1 = read_notes(&a1)?;
        let a2 = read_notes(&a2)?;
        let wav = audio.join(format!("{stem}.wav"));
        let audio = Audio::load(&wav).map_err(|e| format!("{}: {e}", wav.display()))?;
        let mono = mono_of(&audio);
        let analysis = analyze_with(&mono, audio.sr as f32, est);
        let mut predicted = tracker_notes(&analysis);
        sort_notes(&mut predicted);
        scores.add(&a1, &a2, &predicted);
        *backends.entry(analysis.backend).or_insert(0usize) += 1;
        eprintln!("{stem}: {} predicted, {} A1, {} A2 notes", predicted.len(), a1.len(), a2.len());
    }
    Ok(json!({
        "dataset": "vocadito",
        "datasetDoi": DOI,
        "recordings": tracks.len(),
        "singers": singers.len(),
        "predictionSource": "raw tracker notes",
        "backendRecordings": backends,
        "byAnnotator": { "A1": scores.a1.json(), "A2": scores.a2.json() },
        "agreedSameKeyRepeats": {
            "references": REFERENCES,
            "preservedPairs": scores.agreed.preserved,
            "referencePairs": scores.agreed.reference,
            "recall": rate(scores.agreed.preserved, scores.agreed.reference),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(on: f64, off: f64, p: f32) -> N {
        N { on, off, p }
    }

    #[test]
    fn long_note_and_two_attacks_are_scored_separately() {
        let long = note(0.0, 0.8, 60.0);
        let first = note(1.0, 1.15, 62.0);
        let second = note(1.2, 1.35, 62.0);
        let a1 = [long, first, second];
        let a2 = [long, note(1.01, 1.15, 62.0), note(1.21, 1.35, 62.0)];
        let mut split = Scores::default();
        split.add(&a1, &a2, &a1);

        let mut merged = Scores::default();
        merged.add(&a1, &a2, &[long, note(first.on, second.off, first.p)]);
        for (split, merged) in [(&split.a1, &merged.a1), (&split.a2, &merged.a2)] {
            assert_eq!(split.reference, 3);
            assert_eq!((split.onset_pitch, split.onset_pitch_offset), (3, 3));
            assert_eq!((merged.onset_pitch, merged.onset_pitch_offset), (2, 1));
            assert_eq!((split.repeats.reference, split.repeats.preserved), (1, 1));
            assert_eq!((merged.repeats.reference, merged.repeats.preserved), (1, 0));
        }
        assert_eq!((split.agreed.reference, split.agreed.preserved), (1, 1));
        assert_eq!((merged.agreed.reference, merged.agreed.preserved), (1, 0));
    }

    #[test]
    fn onset_pitch_and_offset_each_have_their_own_gate() {
        let reference = [note(0.0, 0.1, 60.0), note(1.0, 2.0, 64.0), note(3.0, 3.1, 67.0), note(4.0, 4.1, 69.0)];
        let predictions = [
            note(0.05, 0.20, 60.5), // qualifies for COnP at both boundaries; offset is too late
            note(1.0, 2.19, 64.0),  // within the 20% offset window for a long note
            note(3.0, 3.1, 67.51),  // wrong pitch
            note(4.051, 4.1, 69.0), // onset outside the window
        ];
        let mut scores = Scores::default();
        scores.add(&reference, &reference, &predictions);
        assert_eq!((scores.a1.onset_pitch, scores.a1.onset_pitch_offset), (2, 1));
        assert_eq!(scores.a1.repeats.json(), json!({ "preservedPairs": 0, "referencePairs": 0, "recall": null }));
    }

    #[test]
    fn split_and_sustained_annotations_keep_separate_denominators() {
        let a1 = [note(0.0, 0.1, 60.0), note(0.11, 0.3, 60.0)];
        let a2 = [note(0.01, 0.3, 60.0)];
        let mut scores = Scores::default();
        scores.add(&a1, &a2, &a1);
        assert_eq!((scores.a1.repeats.reference, scores.a1.repeats.preserved), (1, 1));
        assert_eq!(scores.a2.repeats.reference, 0);
        assert_eq!(scores.agreed.reference, 0);
        assert_eq!(scores.agreed.json()["recall"], Value::Null);

        let empty = NoteScore::default().json();
        assert_eq!(empty["referenceNotes"], 0);
        assert_eq!(empty["predictedNotes"], 0);
        assert_eq!(empty["COnP"], json!({ "matchedNotes": 0, "precision": null, "recall": null, "f1": null }));
    }

    #[test]
    fn agreement_requires_the_same_two_predictions() {
        let a1 = [note(0.05, 0.20, 60.0), note(0.30, 0.40, 60.0)];
        let a2 = [note(0.09, 0.20, 60.0), note(0.30, 0.40, 60.0)];
        let mut predictions = [note(0.01, 0.20, 60.0), note(0.085, 0.20, 60.0), a1[1]];
        for _ in 0..2 {
            sort_notes(&mut predictions);
            let mut scores = Scores::default();
            scores.add(&a1, &a2, &predictions);
            assert_eq!((scores.a1.repeats.preserved, scores.a2.repeats.preserved), (1, 1));
            assert_eq!((scores.agreed.reference, scores.agreed.preserved), (1, 0));
            predictions.reverse();
        }
    }

    #[test]
    fn matching_can_move_an_earlier_assignment_to_keep_both_notes() {
        let reference = [note(0.01, 0.04, 60.4), note(0.04, 0.10, 60.0)];
        let predictions = [note(0.01, 0.04, 60.0), note(0.05, 0.10, 60.8)];
        assert_eq!(note_matches(&reference, &predictions, false), [Some(1), Some(0)]);
    }

    #[test]
    fn repeats_respect_key_adjacency_and_gap_bounds() {
        let a = note(0.0, 0.2, 60.49);
        for (on, pitch, expected) in [
            (0.18, 59.50, 1),  // inclusive 20 ms overlap
            (0.32, 60.49, 1),  // inclusive 120 ms gap
            (0.179, 60.49, 0), // too much overlap
            (0.321, 60.49, 0), // too far apart
            (0.21, 60.50, 0),  // a different rounded MIDI key
        ] {
            assert_eq!(repeats(&[a, note(on, on + 0.1, pitch)]).len(), expected);
        }
        assert!(repeats(&[a, note(0.21, 0.25, 61.0), note(0.28, 0.4, 60.0)]).is_empty());
    }
}
