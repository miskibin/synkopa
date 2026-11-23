//! Note-level transcription accuracy: the pitch editor's notes, and the melody "Copy notes"
//! pastes into the Piano roll, against Annotated VocalSet's note annotations (onset, offset and
//! the median of the hand-checked F0 of every sung note).
//!
//! Metrics follow mir_eval.transcription (Bay et al. / MIREX note tracking): a detected note
//! matches an annotated one when its onset is within 50 ms, its pitch within 50 cents and (for
//! COnPOff) its offset within max(50 ms, 20 % of the annotated duration); notes are paired by a
//! maximum bipartite matching. Error types are counted separately so the cause of a low F1 is
//! visible: annotated notes split into several detected ones (fragmented), several annotated notes
//! covered by one detected note (merged), annotated notes with no detected note (missed),
//! detected notes over silence or transitions (spurious) and wrong-pitch notes.

use std::path::Path;

use serde_json::{json, Value};
use wave_engine::pitch::{analyze_with, F0Estimator, PitchAnalysis};
use wave_engine::sources::{mono_of, Audio};

use crate::vocalset::{self, Item};

#[derive(Clone, Copy, Debug)]
pub struct N {
    pub on: f64,
    pub off: f64,
    /// MIDI, fractional.
    pub p: f32,
}

pub fn tracker_notes(analysis: &PitchAnalysis) -> Vec<N> {
    analysis.notes.iter().map(|n| N { on: n.start, off: n.end, p: n.pitch }).collect()
}

const TECHNIQUES: [&str; 16] = [
    "belt",
    "breathy",
    "fast_forte",
    "fast_piano",
    "forte",
    "lip_trill",
    "messa",
    "slow_forte",
    "slow_piano",
    "spoken",
    "straight",
    "trillo",
    "trill",
    "vibrato",
    "vibrado",
    "pp",
];

fn technique(stem: &str) -> &'static str {
    TECHNIQUES.iter().find(|t| stem.contains(*t)).copied().map(|t| if t == "vibrado" { "vibrato" } else { t }).unwrap_or("other")
}

/// Annotated notes of `stem` (rows of type `Sound` with a ground-truth MIDI code), or None when a
/// sung note has no score pitch (spoken exercises, some trills).
/// `mid_onsets`: a note that follows an annotated transition starts at the transition's middle
/// (where the pitch change is heard) instead of where the pitch has settled (the annotation's
/// own convention).
fn truth(root: &Path, stem: &str, mid_onsets: bool) -> Option<Vec<N>> {
    let ann = root.join("Annotated VocalSet");
    for k in 1..=4 {
        let dir = ann.join(format!("extended {k}")).join("with file header");
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for tech in rd.flatten() {
            for name in [format!("{stem}.csv"), format!("{}.csv", stem.replace("vibrato", "vibrado"))] {
                let p = tech.path().join(&name);
                let Ok(text) = std::fs::read_to_string(&p).or_else(|_| {
                    // file names keep their case on disk
                    std::fs::read_dir(tech.path())
                        .ok()
                        .and_then(|r| r.flatten().find(|e| e.file_name().to_string_lossy().to_lowercase() == name))
                        .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
                        .ok_or(())
                }) else {
                    continue;
                };
                let mut lines = text.lines().skip_while(|l| !l.trim_start().starts_with("Sequence"));
                lines.next()?;
                let mut out = vec![];
                let mut transition: Option<(f64, f64)> = None;
                for l in lines {
                    let c: Vec<&str> = l.split(',').map(str::trim).collect();
                    if c.get(4) == Some(&"Transition") {
                        transition = (|| Some((c.get(1)?.parse().ok()?, c.get(2)?.parse().ok()?)))();
                        continue;
                    }
                    let before = transition.take();
                    if c.get(4) != Some(&"Sound") {
                        continue;
                    }
                    let (on, off): (f64, f64) = (c.get(1)?.parse().ok()?, c.get(2)?.parse().ok()?);
                    // the pitch sung: the median of the hand-checked F0 (the score column is often
                    // a semitone off — singers took another key — so it is not what was sung)
                    let hz: f32 = c.get(6)?.parse().ok()?;
                    c.get(14).filter(|x| !x.is_empty())?;
                    let p = 69.0 + 12.0 * (hz / 440.0).log2();
                    if off > on && hz > 0.0 {
                        // the transition into the note, when it leads straight into it
                        let mid = before.filter(|t| (on - t.1).abs() < 0.03).map(|t| (t.0 + t.1) * 0.5);
                        out.push(N { on: if mid_onsets { mid.unwrap_or(on) } else { on }, off, p });
                    }
                }
                return (!out.is_empty()).then_some(out);
            }
        }
    }
    None
}

/// Maximum bipartite matching (augmenting paths), indexed by reference note.
/// Visit notes in slice order and prefer a free candidate before reassigning an existing match.
fn matching(r: &[N], e: &[N], ok: impl Fn(&N, &N) -> bool) -> Vec<Option<usize>> {
    let adj: Vec<Vec<usize>> = r.iter().map(|a| (0..e.len()).filter(|&j| ok(a, &e[j])).collect()).collect();
    let mut owner: Vec<Option<usize>> = vec![None; e.len()];
    fn aug(i: usize, adj: &[Vec<usize>], seen: &mut [bool], owner: &mut [Option<usize>]) -> bool {
        if let Some(&j) = adj[i].iter().find(|&&j| owner[j].is_none()) {
            owner[j] = Some(i);
            return true;
        }
        for &j in &adj[i] {
            if seen[j] {
                continue;
            }
            seen[j] = true;
            if owner[j].is_some_and(|k| aug(k, adj, seen, owner)) {
                owner[j] = Some(i);
                return true;
            }
        }
        false
    }
    for i in 0..r.len() {
        aug(i, &adj, &mut vec![false; e.len()], &mut owner);
    }
    let mut matches = vec![None; r.len()];
    for (j, i) in owner.into_iter().enumerate() {
        if let Some(i) = i {
            matches[i] = Some(j);
        }
    }
    matches
}

pub fn note_matches(r: &[N], e: &[N], with_offset: bool) -> Vec<Option<usize>> {
    matching(r, e, |a, b| onset_ok(a, b) && pitch_ok(a, b) && (!with_offset || offset_ok(a, b)))
}

const ON_TOL: f64 = 0.05;

fn pitch_ok(a: &N, b: &N) -> bool {
    (a.p - b.p).abs() <= 0.5
}
fn onset_ok(a: &N, b: &N) -> bool {
    (a.on - b.on).abs() <= ON_TOL
}
fn offset_ok(a: &N, b: &N) -> bool {
    (a.off - b.off).abs() <= ON_TOL.max(0.2 * (a.off - a.on))
}

fn overlap(a: &N, b: &N) -> f64 {
    (a.off.min(b.off) - a.on.max(b.on)).max(0.0)
}

#[derive(Default, Clone)]
pub struct Score {
    pub refs: usize,
    pub ests: usize,
    pub on_p_off: usize,
    pub on_p: usize,
    pub on: usize,
    pub fragmented: usize,
    pub extra_fragments: usize,
    pub merged: usize,
    pub missed: usize,
    pub spurious: usize,
    pub wrong_pitch: usize,
    pub onset_err_ms: Vec<f32>,
}

impl Score {
    pub fn add(&mut self, r: &[N], e: &[N]) {
        self.refs += r.len();
        self.ests += e.len();
        self.on_p_off += note_matches(r, e, true).iter().flatten().count();
        self.on_p += note_matches(r, e, false).iter().flatten().count();
        self.on += matching(r, e, onset_ok).iter().flatten().count();
        for a in r {
            let dur = a.off - a.on;
            // detected notes that mostly sit inside this annotated note
            let inside: Vec<&N> = e.iter().filter(|b| overlap(a, b) > 0.5 * (b.off - b.on).min(dur)).collect();
            if inside.len() >= 2 {
                self.fragmented += 1;
                self.extra_fragments += inside.len() - 1;
            }
            let cover: f64 = e.iter().map(|b| overlap(a, b)).sum();
            if cover < 0.3 * dur {
                self.missed += 1;
            } else if let Some(b) = e.iter().max_by(|x, y| overlap(a, x).total_cmp(&overlap(a, y))) {
                if !pitch_ok(a, b) {
                    self.wrong_pitch += 1;
                }
                if let Some(d) = e.iter().map(|b| (b.on - a.on).abs()).min_by(f64::total_cmp) {
                    self.onset_err_ms.push((d * 1000.0) as f32);
                }
            }
        }
        for b in e {
            let covered = r.iter().filter(|a| overlap(a, b) > 0.5 * (a.off - a.on)).count();
            if covered >= 2 {
                self.merged += 1;
            }
            if r.iter().all(|a| overlap(a, b) < 0.3 * (b.off - b.on)) {
                self.spurious += 1;
            }
        }
    }
    fn f1(&self, m: usize) -> Value {
        let p = m as f64 / self.ests.max(1) as f64;
        let r = m as f64 / self.refs.max(1) as f64;
        let f = if p + r > 0.0 { 2.0 * p * r / (p + r) } else { 0.0 };
        json!({ "p": round3(p), "r": round3(r), "f1": round3(f) })
    }
    pub fn json(&self) -> Value {
        let mut ms = self.onset_err_ms.clone();
        ms.sort_by(f32::total_cmp);
        let q = |x: f32| if ms.is_empty() { 0.0 } else { ms[((ms.len() - 1) as f32 * x) as usize] };
        json!({
            "annotated": self.refs, "detected": self.ests,
            "COnPOff": self.f1(self.on_p_off), "COnP": self.f1(self.on_p), "COn": self.f1(self.on),
            "fragmented": self.fragmented, "extraFragments": self.extra_fragments, "merged": self.merged,
            "missed": self.missed, "spurious": self.spurious, "wrongPitch": self.wrong_pitch,
            "onsetErrMs": { "median": q(0.5), "p90": q(0.9) },
        })
    }
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// What "Copy notes" pastes (`ui/src/lib/vocalNotes.ts` `sungToPattern`, ported): sung timing,
/// semitone keys, a note still sounding when the next starts ends there, at least a quarter step
/// at `bpm`. In seconds, so it can be scored.
pub fn pasted(sung: &[N], bpm: f64) -> Vec<N> {
    let min = 60.0 / bpm / 16.0;
    let mut v = sung.to_vec();
    v.sort_by(|a, b| a.on.total_cmp(&b.on));
    let mut out: Vec<N> = vec![];
    for n in v {
        if let Some(prev) = out.last_mut() {
            if prev.off > n.on {
                prev.off = n.on;
                if prev.off - prev.on < min {
                    out.pop();
                }
            }
        }
        out.push(N { on: n.on, off: n.off.max(n.on + min), p: n.p.round() });
    }
    out
}

/// The earlier "Copy notes": the notes on the 1/16 grid at `bpm` (clip at song
/// start, no stretch), returned in seconds so they can be scored.
pub fn exported(sung: &[N], bpm: f64) -> Vec<N> {
    let step = 60.0 / bpm / 4.0;
    let mut raw: Vec<(i64, i64, f32)> = vec![];
    let mut v = sung.to_vec();
    v.sort_by(|a, b| a.on.total_cmp(&b.on));
    for n in v {
        let s = (n.on / step).round() as i64;
        let e = (n.off / step).round() as i64;
        raw.push((s, (e - s).max(1), n.p.round()));
    }
    let mut out: Vec<(i64, i64, f32)> = vec![];
    for n in raw {
        if let Some(prev) = out.last_mut() {
            if n.0 <= prev.0 {
                if n.1 > prev.1 {
                    *prev = n;
                }
                continue;
            }
            if prev.0 + prev.1 > n.0 {
                prev.1 = n.0 - prev.0;
            }
        }
        out.push(n);
    }
    out.into_iter().map(|(s, l, p)| N { on: s as f64 * step, off: (s + l) as f64 * step, p }).collect()
}

/// Which recordings a score covers.
pub type Pick<'a> = &'a dyn Fn(&Cached) -> bool;
/// A transcriber working from the analysis alone.
pub type FromAnalysis<'a> = &'a dyn Fn(&wave_engine::pitch::PitchAnalysis) -> Vec<N>;

/// A named method: (pitch analysis, mono audio, sample rate) → notes.
pub type Transcriber<'a> = (&'a str, &'a dyn Fn(&wave_engine::pitch::PitchAnalysis, &[f32], f32) -> Vec<N>);

/// Scores every transcriber on up to `limit` recordings (spread over singers and techniques).
pub fn run(
    root: &Path,
    limit: usize,
    est: Option<&dyn F0Estimator>,
    methods: &[Transcriber],
    bpm: f64,
    dump: Option<&Path>,
) -> Result<Value, String> {
    let items = vocalset::index(root)?;
    let usable: Vec<(&Item, Vec<N>)> =
        items.iter().filter(|i| !i.is_excerpt()).filter_map(|i| truth(root, &i.stem, false).map(|t| (i, t))).collect();
    eprintln!("{} recordings with scored notes", usable.len());
    let picked = vocalset::spread(&usable, limit);
    let mut total: Vec<(Score, Score)> = vec![Default::default(); methods.len()];
    let mut by_tech: std::collections::BTreeMap<&str, Vec<(Score, Score)>> = Default::default();
    let mut off_score = (0usize, 0usize);
    let mut files = vec![];
    for (k, (it, gt)) in picked.into_iter().enumerate() {
        let a = Audio::load(&it.audio).map_err(|e| format!("{}: {e}", it.audio.display()))?;
        let mono = mono_of(&a);
        let an = analyze_with(&mono, a.sr as f32, est);
        let tech = technique(&it.stem);
        // how often the singer is more than 50 ct from the score (ceiling for pitch metrics)
        for n in gt {
            let f0 = it.curve(an.curve.len());
            let (f, g) = ((n.on * 200.0) as usize, ((n.off * 200.0) as usize).min(f0.len()));
            let mut v: Vec<f32> = f0.get(f..g).unwrap_or(&[]).iter().copied().filter(|x| x.is_finite()).collect();
            if !v.is_empty() {
                v.sort_by(f32::total_cmp);
                off_score.1 += 1;
                if (v[v.len() / 2] - n.p).abs() > 0.5 {
                    off_score.0 += 1;
                }
            }
        }
        let row = by_tech.entry(tech).or_insert_with(|| vec![Default::default(); methods.len()]);
        let mut fj = json!({ "stem": it.stem, "truth": gt.iter().map(|n| [n.on, n.off, n.p as f64]).collect::<Vec<_>>() });
        for (m, (name, f)) in methods.iter().enumerate() {
            let notes = f(&an, &mono, a.sr as f32);
            let exp = exported(&notes, bpm);
            total[m].0.add(gt, &notes);
            total[m].1.add(gt, &exp);
            row[m].0.add(gt, &notes);
            row[m].1.add(gt, &exp);
            fj[*name] = json!(notes.iter().map(|n| [n.on, n.off, n.p as f64]).collect::<Vec<_>>());
        }
        files.push(fj);
        eprintln!("[{k}] {} ({tech}): {} notes", it.stem, gt.len());
    }
    if let Some(d) = dump {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        std::fs::write(d.join("notes.json"), serde_json::to_string(&files).unwrap_or_default()).map_err(|e| e.to_string())?;
    }
    let pack = |s: &[(Score, Score)]| -> Value {
        methods
            .iter()
            .zip(s)
            .map(|((n, _), (a, b))| (n.to_string(), json!({ "notes": a.json(), "pianoRoll": b.json() })))
            .collect::<serde_json::Map<_, _>>()
            .into()
    };
    Ok(json!({
        "dataset": "Annotated VocalSet", "recordings": files.len(), "exportBpm": bpm,
        "singerOffScoreShare": round3(off_score.0 as f64 / off_score.1.max(1) as f64),
        "total": pack(&total),
        "byTechnique": by_tech.iter().map(|(t, s)| (t.to_string(), pack(s))).collect::<serde_json::Map<_, _>>(),
    }))
}

/// One analysed recording with its annotated notes, kept in memory so many transcriber
/// settings can be scored without analysing again.
pub struct Cached {
    pub name: String,
    /// VocalSet test singer (female2/8, male3/5/10) or GTSinger: held out from tuning.
    pub held_out: bool,
    pub set: &'static str,
    pub gt: Vec<N>,
    /// VocalSet with onsets at the middle of the transition into each legato note.
    pub gt_mid: Option<Vec<N>>,
    pub an: wave_engine::pitch::PitchAnalysis,
}

const TEST_SINGERS: [&str; 5] = ["f2", "f8", "m3", "m5", "m10"];

pub fn cache_vocalset(root: &Path, limit: usize, est: Option<&dyn F0Estimator>) -> Result<Vec<Cached>, String> {
    let items = vocalset::index(root)?;
    let usable: Vec<(&Item, Vec<N>)> =
        items.iter().filter(|i| !i.is_excerpt()).filter_map(|i| truth(root, &i.stem, false).map(|t| (i, t))).collect();
    let mut out = vec![];
    for (it, gt) in vocalset::spread(&usable, limit) {
        let a = Audio::load(&it.audio).map_err(|e| format!("{}: {e}", it.audio.display()))?;
        let an = analyze_with(&mono_of(&a), a.sr as f32, est);
        let held_out = TEST_SINGERS.contains(&it.singer());
        if let Some(c) = incomplete(&an.curve, gt) {
            eprintln!(
                "
{}: notes annotated over only {:.0} % of the singing; left out",
                it.stem,
                c * 100.0
            );
            continue;
        }
        if let Some(lag) = misaligned(&an.curve, gt) {
            eprintln!(
                "
{}: annotation is {lag:+.2} s off the recording; left out",
                it.stem
            );
            continue;
        }
        let gt_mid = truth(root, &it.stem, true);
        out.push(Cached { name: it.stem.clone(), held_out, set: "vocalset", gt: gt.clone(), gt_mid, an });
        eprint!(".");
    }
    eprintln!(" {} VocalSet", out.len());
    Ok(out)
}

/// Share of annotated-note frames where the analysis is within half a semitone of the note,
/// with the annotation shifted by `lag` seconds.
fn agreement(curve: &[f32], gt: &[N], lag: f64) -> f64 {
    let (mut hit, mut n) = (0usize, 0usize);
    for a in gt {
        let (f0, f1) = (((a.on + lag) * 200.0).max(0.0) as usize, (((a.off + lag) * 200.0).max(0.0) as usize).min(curve.len()));
        for x in curve.get(f0..f1).unwrap_or(&[]) {
            n += 1;
            hit += (x.is_finite() && (x - a.p).abs() < 0.5) as usize;
        }
    }
    hit as f64 / n.max(1) as f64
}

/// The annotation's time offset when it fits the recording clearly better shifted (≥ 0.1 s and
/// 15 points more agreement): such a file cannot score onsets.
fn misaligned(curve: &[f32], gt: &[N]) -> Option<f64> {
    let at0 = agreement(curve, gt, 0.0);
    let (best, lag) =
        (-30..=30).map(|l| (agreement(curve, gt, l as f64 / 20.0), l as f64 / 20.0)).fold((0.0, 0.0), |m, x| if x.0 > m.0 { x } else { m });
    (lag.abs() >= 0.1 && best - at0 > 0.15).then_some(lag)
}

/// Share of the voiced frames between the first and the last annotated note that lie in an
/// annotated note (transitions between legato notes are short), when it is below one half: the
/// fast runs of some exercises are annotated only at their ends.
fn incomplete(curve: &[f32], gt: &[N]) -> Option<f64> {
    let (a, b) = ((gt[0].on * 200.0) as usize, ((gt[gt.len() - 1].off * 200.0) as usize).min(curve.len()));
    let voiced: Vec<usize> = (a..b).filter(|&f| curve[f].is_finite()).collect();
    let inside = voiced.iter().filter(|&&f| gt.iter().any(|n| (n.on * 200.0) as usize <= f && f < (n.off * 200.0) as usize)).count();
    let c = inside as f64 / voiced.len().max(1) as f64;
    (c < 0.5).then_some(c)
}

/// GTSinger segments: real songs with lyrics (repeated notes, melismas, glissandi); truth is the
/// score note aligned to the recording.
pub fn cache_gtsinger(root: &Path, limit: usize, est: Option<&dyn F0Estimator>) -> Result<Vec<Cached>, String> {
    let segs = crate::gtsinger::index(root)?;
    let mut out = vec![];
    for s in vocalset::spread(&segs, limit) {
        let a = Audio::load(&s.wav).map_err(|e| format!("{}: {e}", s.wav.display()))?;
        let an = analyze_with(&mono_of(&a), a.sr as f32, est);
        let gt = s.notes.iter().map(|n| N { on: n.start, off: n.end, p: n.midi }).collect();
        // the score is what was sung only in plain singing; technique groups ornament it
        let set = if s.group == "Control_Group" { "gtsinger" } else { "gtsinger-technique" };
        // tuning may use two singers; the others stay held out
        let held_out = !matches!(s.singer.as_str(), "EN-Alto-1" | "EN-Tenor-1");
        out.push(Cached { name: s.name(), held_out, set, gt, gt_mid: None, an });
        eprint!(".");
    }
    eprintln!(" {} GTSinger", out.len());
    Ok(out)
}

/// Scores `f` on the recordings `pick` selects: (what Copy notes pastes, the old 1/16 grid at `bpm`).
pub fn score(
    cache: &[Cached],
    pick: impl Fn(&Cached) -> bool,
    f: &dyn Fn(&wave_engine::pitch::PitchAnalysis) -> Vec<N>,
    bpm: f64,
    mid: bool,
) -> (Score, Score) {
    let (mut a, mut b) = (Score::default(), Score::default());
    for c in cache.iter().filter(|c| pick(c)) {
        let n = f(&c.an);
        let gt = if mid { c.gt_mid.as_ref().unwrap_or(&c.gt) } else { &c.gt };
        a.add(gt, &pasted(&n, bpm));
        b.add(gt, &exported(&n, bpm));
    }
    (a, b)
}

impl Score {
    pub fn f1_of(&self, m: usize) -> f64 {
        let p = m as f64 / self.ests.max(1) as f64;
        let r = m as f64 / self.refs.max(1) as f64;
        if p + r > 0.0 {
            2.0 * p * r / (p + r)
        } else {
            0.0
        }
    }
    pub fn line(&self) -> String {
        format!(
            "COn {:.3} COnP {:.3} COnPOff {:.3} | det {}/{} frag {} merged {} missed {} spur {} wrongP {}",
            self.f1_of(self.on),
            self.f1_of(self.on_p),
            self.f1_of(self.on_p_off),
            self.ests,
            self.refs,
            self.extra_fragments,
            self.merged,
            self.missed,
            self.spurious,
            self.wrong_pitch
        )
    }
}
