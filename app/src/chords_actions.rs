//! Chords of an audio clip ("Detect tempo and key" → Chords): what ChordMini hears
//! (`crates/chords`), decoded on the project's beats by the engine, and "Send to piano roll":
//! a new pattern of block chords on a channel, placed on the Playlist under the clip's chords,
//! one undo step. The same functions answer the app, the session socket and
//! `synkopa --project FILE chords`.
//!
//! The model is not shipped: it is downloaded on first use (17 MB, MIT, SHA-256 checked) and
//! loaded into the engine the first time chords are asked for.

use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Value};
use wave_engine::chords::{voice, ChordSegment, Detail};
use wave_engine::errors::ERR_NO_CHORD_MODEL;
use wave_engine::{Channel, ChannelSrc, Clip, ClipKind, Engine, Note, Op, Pattern, Project};

use crate::actions::{arg, as_index, need_u64, opt_f64, Host, R};
use crate::models;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChordModelStatus {
    pub id: &'static str,
    pub name: &'static str,
    pub bytes: u64,
    pub installed: bool,
    pub license_note: &'static str,
    pub license_url: &'static str,
    pub path: String,
}

pub fn status() -> ChordModelStatus {
    let path = wave_chords::model_path();
    ChordModelStatus {
        id: wave_chords::MODEL_ID,
        name: wave_chords::MODEL_NAME,
        bytes: wave_chords::MODEL_BYTES,
        installed: path.is_file(),
        license_note: wave_chords::LICENSE_NOTE,
        license_url: wave_chords::LICENSE_URL,
        path: path.display().to_string(),
    }
}

pub fn download() -> models::Download {
    models::Download {
        url: wave_chords::MODEL_URL,
        sha256: wave_chords::MODEL_SHA256,
        bytes: wave_chords::MODEL_BYTES,
        dst: wave_chords::model_path(),
    }
}

/// Loads the installed model into the engine unless one is loaded. Without the model file:
/// [`ERR_NO_CHORD_MODEL`] (the dialog offers the download on it).
pub fn ensure_model(engine: &Engine) -> R<()> {
    if engine.chord_backend().is_some() {
        return Ok(());
    }
    let path = wave_chords::model_path();
    if !path.is_file() {
        return Err(format!(
            "{ERR_NO_CHORD_MODEL} chord detection needs its model ({} MB), which is not installed",
            wave_chords::MODEL_BYTES / 1_000_000
        ));
    }
    let m = wave_chords::ChordMini::load(&path)?;
    engine.set_chord_model(Some(Arc::new(m)));
    Ok(())
}

pub fn control(app: &dyn Host, cmd: &str, a: &Value, if_revision: Option<u64>) -> Option<R<Value>> {
    Some(match cmd {
        "chords_model" => Ok(json!(status())),
        // ~17 MB, checked inside `fetch`; session requests run on their own thread
        "chords_model_install" => models::fetch(&download(), None, |_, _| {}).map(|_| json!(status())),
        "chords_analyze" => (|| Ok(json!(analyze(app.engine(), need_u64(a, "clipId")?, detail_arg(a)?)?)))(),
        "chords_to_pattern" => to_pattern(app, a, if_revision),
        _ => return None,
    })
}

/// `detail`: "full" (default) or "simple".
pub fn detail_arg(a: &Value) -> R<Detail> {
    match arg(a, "detail").and_then(Value::as_str) {
        None | Some("full") => Ok(Detail::Full),
        Some("simple") => Ok(Detail::Simple),
        Some(x) => Err(format!("`detail`: full or simple, not `{x}`")),
    }
}

/// The chords of audio clip `clip_id` (the model is loaded first if needed).
pub fn analyze(e: &Engine, clip_id: u64, detail: Detail) -> R<wave_engine::chords::ChordAnalysis> {
    ensure_model(e)?;
    e.chord_analysis(clip_id, detail).map_err(|x| x.to_string())
}

/// Velocity of the written chords: firm, below full so accents can still be added.
const VELOCITY: f32 = 0.8;

/// Block chords for `segs` (project beats) in a pattern that starts at beat `start`: the
/// root in the bass, close voicings above that move as little as possible (see [`voice`]).
pub fn chord_notes(segs: &[ChordSegment], start: f64, ch: u64) -> Vec<Note> {
    let mut notes = vec![];
    let mut prev: Option<Vec<u8>> = None;
    for s in segs {
        let keys = voice(s.root, s.quality, prev.as_deref());
        let step = ((s.start - start) * 4.0) as f32;
        let len = ((s.end - s.start) * 4.0) as f32;
        for &k in &keys {
            notes.push(Note { ch, step, len, p: Some(k), v: VELOCITY, trig: Some(false), pan: 0.0, slide: false });
        }
        prev = Some(keys[1..].to_vec());
    }
    notes
}

/// `chords_to_pattern {clipId, from?, to?, detail?, channel?}`: the clip's chords between
/// project beats `from..to` (default: the whole clip) as a new pattern of block chords on
/// `channel` (default: a new Synkopa Keys piano), its clip on the Playlist from the bar the
/// first chord is in, on the first free track below the audio. One undo step.
pub fn to_pattern(app: &dyn Host, a: &Value, if_revision: Option<u64>) -> R<Value> {
    let e = app.engine();
    let clip_id = need_u64(a, "clipId")?;
    let detail = detail_arg(a)?;
    let an = analyze(e, clip_id, detail)?;
    let from = opt_f64(a, "from").unwrap_or(an.start).clamp(an.start, an.end);
    let to = opt_f64(a, "to").unwrap_or(an.end).clamp(from, an.end);
    let segs: Vec<ChordSegment> = an
        .segments
        .iter()
        .filter(|s| s.end > from + 1e-6 && s.start < to - 1e-6)
        .map(|s| ChordSegment { start: s.start.max(from), end: s.end.min(to), ..s.clone() })
        .collect();
    if segs.is_empty() {
        return Err("no chords were detected in that part of the clip".into());
    }
    let channel = match arg(a, "channel") {
        None | Some(Value::Null) => None,
        Some(v) => Some(as_index(v).ok_or("`channel`: a channel id")?),
    };
    let src_name = {
        let p = e.project();
        let clip = p.clips.iter().find(|c| c.id == clip_id).ok_or("no such clip")?;
        p.sources.iter().find(|s| s.id == clip.source).map(|s| s.name.clone()).unwrap_or_default()
    };
    let name = if src_name.is_empty() { "Chords".to_string() } else { format!("Chords – {src_name}") };
    // the pattern starts on the bar of the first chord, so it lines up with the grid
    let start = (segs[0].start / 4.0).floor() * 4.0;
    let end = segs.last().map(|s| s.end).unwrap_or(start);
    let notes = |ch: u64| chord_notes(&segs, start, ch);
    let mut made = put_pattern_under_clip(
        e,
        &PatternPlan { clip_id, channel, name: &name, color: "#e0a44a", start, end, label: "Send chords to piano roll" },
        if_revision,
        notes,
    )?;
    made["chords"] = json!(segs.len());
    Ok(made)
}

/// Where a converted part goes: a new pattern (on `channel`, else a new Synkopa Keys piano)
/// from beat `start` (a bar line) to at least `end`, its Playlist clip on the first free track
/// below the audio clip.
pub struct PatternPlan<'a> {
    pub clip_id: u64,
    pub channel: Option<u64>,
    pub name: &'a str,
    pub color: &'a str,
    pub start: f64,
    pub end: f64,
    pub label: &'a str,
}

/// Makes the pattern of `plan` with the notes `notes(channel id)`, in one undo step labelled
/// `plan.label`. Returns `{patternId, channelId, newChannel, clipId, track, start, revision}`.
pub fn put_pattern_under_clip(e: &Engine, plan: &PatternPlan, if_revision: Option<u64>, notes: impl Fn(u64) -> Vec<Note>) -> R<Value> {
    let mut made = Value::Null;
    let rev = e
        .apply_with_library_labeled(&[], if_revision, Some(plan.label), |p: &Project, _| {
            let clip = p.clips.iter().find(|c| c.id == plan.clip_id).ok_or("the clip is gone")?.clone();
            let mut next = p.next_id;
            let mut id = || {
                next += 1;
                next - 1
            };
            let mut ops = vec![];
            let ch = match plan.channel {
                Some(c) => {
                    p.channel(c).ok_or(format!("no channel {c}"))?;
                    c
                }
                None => {
                    let c = id();
                    let name =
                        wave_engine::keys::instrument(wave_engine::keys::DEFAULT_INSTRUMENT).map(|i| i.name).unwrap_or("Grand Piano");
                    let mut chan = Channel::new(c, name, plan.color, ChannelSrc::Keys { keys: Default::default() });
                    chan.root = 60;
                    ops.push(Op::PutChannel { channel: chan, index: None });
                    c
                }
            };
            let start = plan.start;
            let len = (((plan.end - start) / 4.0).ceil() * 4.0).max(4.0);
            let pattern_id = id();
            let name = fresh_name(p, plan.name);
            ops.push(Op::PutPattern {
                pattern: Pattern {
                    id: pattern_id,
                    name,
                    color: plan.color.into(),
                    len,
                    min_len: 4.0,
                    notes: notes(ch),
                    swing: 0.0,
                    channel_loops: std::collections::BTreeMap::new(),
                },
            });
            let ntracks = p.tracks.len() as u32;
            let free = |t: u32| !p.clips.iter().any(|c| c.track == t && c.start < start + len && c.start + c.len > start);
            let track = (clip.track + 1..ntracks)
                .chain(0..=clip.track.min(ntracks.saturating_sub(1)))
                .find(|t| free(*t))
                .unwrap_or(ntracks.saturating_sub(1));
            let pclip = id();
            if ntracks > 0 {
                ops.push(Op::PutClips {
                    clips: vec![Clip {
                        warp: Vec::new(),
                        id: pclip,
                        track,
                        start,
                        len,
                        offset: 0.0,
                        kind: ClipKind::Pat,
                        source: pattern_id,
                        muted: false,
                        gain: 0.0,
                        fade_in: 0.0,
                        fade_out: 0.0,
                        pan: 0.0,
                        norm: false,
                        rev: false,
                        tune: None,
                        stretch: 1.0,
                        resample: false,
                        pitch: 0.0,
                        insert: None,
                        root: wave_engine::CLIP_ROOT,
                    }],
                });
            }
            made = json!({
                "patternId": pattern_id,
                "channelId": ch,
                "newChannel": plan.channel.is_none(),
                "clipId": if ntracks > 0 { json!(pclip) } else { Value::Null },
                "track": track,
                "start": start,
            });
            Ok(ops)
        })
        .map_err(|x| x.to_string())?;
    made["revision"] = json!(rev);
    Ok(made)
}

fn fresh_name(p: &Project, base: &str) -> String {
    let taken = |n: &str| p.patterns.iter().any(|x| x.name == n);
    if !taken(base) {
        return base.to_string();
    }
    (2..).map(|k| format!("{base} {k}")).find(|n| !taken(n)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wave_engine::chords::Quality;

    fn seg(start: f64, end: f64, root: u8, quality: Quality) -> ChordSegment {
        ChordSegment { start, end, root, quality, name: String::new(), confidence: 1.0 }
    }

    #[test]
    fn notes_sit_on_the_chords_steps_with_the_root_below() {
        let segs = [seg(5.0, 7.0, 9, Quality::Min), seg(7.0, 9.0, 5, Quality::Maj7)];
        let n = chord_notes(&segs, 4.0, 3);
        assert_eq!(n.len(), 3 + 1 + 4 + 1);
        assert!(n.iter().all(|x| x.ch == 3 && x.trig == Some(false)));
        // Am from step 4 for 8 steps, its lowest note an A
        let am: Vec<&Note> = n.iter().filter(|x| x.step == 4.0).collect();
        assert_eq!(am.len(), 4);
        assert!(am.iter().all(|x| x.len == 8.0));
        assert_eq!(am.iter().map(|x| x.p.unwrap()).min().unwrap() % 12, 9);
        let f: Vec<u8> = n.iter().filter(|x| x.step == 12.0).map(|x| x.p.unwrap()).collect();
        assert_eq!(f.iter().min().unwrap() % 12, 5);
        assert_eq!(f.len(), 5);
    }
}
