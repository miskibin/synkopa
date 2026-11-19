//! Standard MIDI files in and out (`midi_inspect`, `midi_import`, `midi_export`): the same
//! functions answer the running app and `synkopa --project FILE midi …`.
//!
//! Import makes one pattern with a Channel Rack channel per part (General MIDI drums become
//! samplers of the standard library, one per drum; other parts the closest Synkopa instrument)
//! and places it on the Playlist, or puts one part's notes into a channel of a pattern. Export
//! writes the song (pattern clips as the Playlist plays them) or one pattern as a format 1
//! file with the tempo, one track per channel.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use wave_engine::midi_file::{self, Instrument, MidiFile, OutTrack, SmfNote, DRUM_CHANNEL};
use wave_engine::{factory, Channel, ChannelSrc, Clip, ClipKind, Id, Note, Op, Pattern, Project};

use crate::actions::{arg, as_index, need_str, opt_f64, Host, R};

pub fn control(app: &dyn Host, cmd: &str, a: &Value, if_revision: Option<u64>) -> Option<R<Value>> {
    Some(match cmd {
        "midi_inspect" => inspect(app, a),
        "midi_import" => import(app, a, if_revision),
        "midi_export" => export(app, a),
        _ => return None,
    })
}

fn read(path: &str) -> R<MidiFile> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    midi_file::parse(&bytes).map_err(|e| format!("{path}: {e}"))
}

fn r4(x: f64) -> f64 {
    (x * 1e4).round() / 1e4
}

/// What a part becomes, for the dialog: `{kind: "drums", drums: [{key, name, notes}]}`,
/// `{kind: "keys"}` or `{kind: "synth", preset}`.
fn plan_json(part: &midi_file::Part, keys: bool) -> Value {
    if part.channel == DRUM_CHANNEL {
        let mut by: BTreeMap<u8, usize> = BTreeMap::new();
        for n in &part.notes {
            *by.entry(n.key).or_default() += 1;
        }
        let drums: Vec<Value> = by.iter().map(|(k, n)| json!({ "key": k, "name": midi_file::drum_for(*k).1, "notes": n })).collect();
        return json!({ "kind": "drums", "drums": drums });
    }
    json!(midi_file::instrument_for(part.program, keys))
}

/// `midi_inspect {path}` → the file's parts and what each would become.
pub fn inspect(app: &dyn Host, a: &Value) -> R<Value> {
    let f = read(need_str(a, "path")?)?;
    let keys = app.keys_installed();
    let parts: Vec<Value> = f
        .parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let (lo, hi) = p.notes.iter().fold((127u8, 0u8), |(l, h), n| (l.min(n.key), h.max(n.key)));
            json!({
                "index": i, "track": p.track, "channel": p.channel + 1, "name": p.name,
                "program": p.program, "gmName": if p.channel == DRUM_CHANNEL { "Drums".to_string() } else { p.program.map(midi_file::gm_name).unwrap_or("Piano").to_string() },
                "drums": p.channel == DRUM_CHANNEL, "notes": p.notes.len(), "lowest": lo, "highest": hi,
                "end": r4(p.notes.iter().map(|n| n.start + n.len).fold(0.0, f64::max)),
                "becomes": plan_json(p, keys),
            })
        })
        .collect();
    Ok(json!({
        "path": need_str(a, "path")?, "format": f.format, "ppq": f.ppq, "bpm": f.bpm.map(r4), "tempoChanges": f.tempo_changes,
        "end": r4(f.end), "warnings": f.warnings, "parts": parts, "projectBpm": app.engine().project().bpm,
    }))
}

fn steps(beats: f64) -> f32 {
    (beats * 4.0) as f32
}

fn vel(v: u8) -> f32 {
    (f32::from(v) / 127.0).clamp(0.0, 1.0)
}

/// The parts `a.parts` names (indices; all with notes when absent).
fn chosen<'a>(f: &'a MidiFile, a: &Value) -> R<Vec<&'a midi_file::Part>> {
    let list: Vec<&midi_file::Part> = match arg(a, "parts").and_then(Value::as_array) {
        Some(ix) => ix
            .iter()
            .map(|v| {
                let i = as_index(v).ok_or("`parts` holds part indices")? as usize;
                f.parts.get(i).ok_or(format!("the file has no part {i} (it has {})", f.parts.len()))
            })
            .collect::<R<_>>()?,
        None => f.parts.iter().collect(),
    };
    let list: Vec<_> = list.into_iter().filter(|p| !p.notes.is_empty()).collect();
    if list.is_empty() {
        return Err("no notes to import (choose at least one part with notes)".into());
    }
    Ok(list)
}

/// `midi_import {path, parts?, setTempo?, beat?, track?}` → a new pattern on the Playlist;
/// `midi_import {path, parts: [i], into: {pattern, ch, replace?}}` → that part's notes in a channel.
pub fn import(app: &dyn Host, a: &Value, if_revision: Option<u64>) -> R<Value> {
    let e = app.engine();
    let path = need_str(a, "path")?;
    let f = read(path)?;
    let parts = chosen(&f, a)?;
    if let Some(into) = arg(a, "into") {
        return import_into(app, &parts, into, if_revision);
    }
    let set_tempo = arg(a, "setTempo").and_then(Value::as_bool).unwrap_or(false);
    let beat = opt_f64(a, "beat").unwrap_or(0.0).max(0.0);
    let keys = app.keys_installed();
    let stem = Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "MIDI".into());
    // the drum samples the project must have
    let mut drum_keys: Vec<u8> = parts.iter().filter(|p| p.channel == DRUM_CHANNEL).flat_map(|p| p.notes.iter().map(|n| n.key)).collect();
    drum_keys.sort_unstable();
    drum_keys.dedup();
    let files: Vec<&str> = drum_keys.iter().map(|k| midi_file::drum_for(*k).0).collect();
    let mut made: Value = Value::Null;
    let track_arg = opt_f64(a, "track").map(|t| t.max(0.0) as u32);
    let rev = e
        .apply_with_library(&files, if_revision, |p: &Project, src: &HashMap<String, Id>| {
            let mut next = p.next_id;
            let mut id = || {
                next += 1;
                next - 1
            };
            let mut ops = vec![];
            if set_tempo {
                if let Some(bpm) = f.bpm {
                    ops.push(Op::SetBpm { bpm: bpm.clamp(20.0, 999.0) });
                }
            }
            let pattern_id = id();
            let mut notes: Vec<Note> = vec![];
            let mut channels: Vec<Id> = vec![];
            // drums: one sampler per drum key, shared by every drum part
            let mut drum_ch: HashMap<u8, Id> = HashMap::new();
            for k in &drum_keys {
                let (file, name) = midi_file::drum_for(*k);
                let ff = factory::file(file).ok_or(format!("no library sample {file}"))?;
                let source = *src.get(file).ok_or(format!("the library sample {file} did not load"))?;
                let cid = id();
                // rooted at the drum key: the notes play the sample unpitched and export as that key
                let ch = Channel { root: *k, ..Channel::sampler(cid, name, factory::color(ff), source) };
                ops.push(Op::PutChannel { channel: ch, index: None });
                drum_ch.insert(*k, cid);
                channels.push(cid);
            }
            for part in &parts {
                if part.channel == DRUM_CHANNEL {
                    for n in &part.notes {
                        notes.push(Note { ch: drum_ch[&n.key], step: steps(n.start), len: steps(n.len).max(0.01), p: None, v: vel(n.vel), trig: Some(false), pan: 0.0 });
                    }
                    continue;
                }
                let cid = id();
                let ch = match midi_file::instrument_for(part.program, keys) {
                    Instrument::Keys => Channel::new(cid, &part.name, "#e0a44a", ChannelSrc::Keys { keys: Default::default() }),
                    Instrument::Synth { preset } => {
                        let synth = wave_engine::poly::preset_state(preset).ok_or(format!("no synth preset {preset}"))?;
                        let bass = wave_engine::poly::presets::all().iter().any(|x| x.name == preset && matches!(x.category, "808" | "Bas"));
                        Channel { root: if bass { 36 } else { 60 }, ..Channel::new(cid, &part.name, "#9b7cf8", ChannelSrc::Synth { synth }) }
                    }
                };
                ops.push(Op::PutChannel { channel: ch, index: None });
                channels.push(cid);
                for n in &part.notes {
                    notes.push(Note { ch: cid, step: steps(n.start), len: steps(n.len).max(0.01), p: Some(n.key), v: vel(n.vel), trig: Some(false), pan: 0.0 });
                }
            }
            let end = parts.iter().flat_map(|p| &p.notes).map(|n| n.start + n.len).fold(0.0, f64::max);
            let len = ((end / 4.0).ceil() * 4.0).max(4.0);
            ops.push(Op::PutPattern { pattern: Pattern { id: pattern_id, name: stem.clone(), color: "#7c8cf8".into(), len, min_len: 4.0, notes, swing: 0.0 } });
            // the first track with nothing where the pattern goes (else the chosen or first one)
            let free = |t: u32| !p.clips.iter().any(|c| c.track == t && c.start < beat + len && c.start + c.len > beat);
            let ntracks = p.tracks.len() as u32;
            let track = track_arg.filter(|t| *t < ntracks).or_else(|| (0..ntracks).find(|t| free(*t))).unwrap_or(0);
            let clip_id = id();
            if ntracks > 0 {
                ops.push(Op::PutClips {
                    clips: vec![Clip {
                        id: clip_id,
                        track,
                        start: beat,
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
            made = json!({ "patternId": pattern_id, "channelIds": channels, "clipId": if ntracks > 0 { json!(clip_id) } else { Value::Null }, "track": track, "beat": beat, "len": len });
            Ok(ops)
        })
        .map_err(|x| x.to_string())?;
    made["revision"] = json!(rev);
    made["warnings"] = json!(f.warnings);
    made["tempoSet"] = json!(set_tempo && f.bpm.is_some());
    Ok(made)
}

fn import_into(app: &dyn Host, parts: &[&midi_file::Part], into: &Value, if_revision: Option<u64>) -> R<Value> {
    let e = app.engine();
    let [part] = parts else { return Err("import into a channel takes exactly one part".into()) };
    let p = e.project();
    let pid = arg(into, "pattern").and_then(as_index).ok_or("`into.pattern` is required")?;
    let ch = arg(into, "ch").and_then(as_index).ok_or("`into.ch` is required")?;
    let replace = arg(into, "replace").and_then(Value::as_bool).unwrap_or(false);
    let mut pat = p.patterns.iter().find(|x| x.id == pid).cloned().ok_or(format!("no pattern {pid}"))?;
    if p.channel(ch).is_none() {
        return Err(format!("no channel {ch}"));
    }
    if replace {
        pat.notes.retain(|n| n.ch != ch);
    }
    let added = part.notes.len();
    pat.notes.extend(part.notes.iter().map(|n| Note {
        pan: 0.0,
        ch,
        step: steps(n.start),
        len: steps(n.len).max(0.01),
        p: Some(n.key),
        v: vel(n.vel),
        trig: Some(false),
    }));
    let rev = e.apply(&[Op::PutPattern { pattern: pat }], false, if_revision).map_err(|x| x.to_string())?;
    Ok(json!({ "patternId": pid, "channelIds": [ch], "notes": added, "revision": rev }))
}

// ---------------------------------------------------------------- export

/// How a channel is written: its MIDI channel, program and the key a note plays.
struct Voice {
    drum_key: Option<u8>,
    program: Option<u8>,
}

fn voice(p: &Project, c: &Channel) -> Voice {
    match &c.src {
        ChannelSrc::Keys { .. } => Voice { drum_key: None, program: Some(0) },
        // a hosted plugin's sound is its own: no General MIDI program stands for it
        ChannelSrc::Plugin { .. } => Voice { drum_key: None, program: None },
        ChannelSrc::Synth { synth } => {
            let cat = synth
                .preset
                .as_deref()
                .and_then(|n| wave_engine::poly::presets::all().into_iter().find(|x| x.name == n).map(|x| x.category));
            Voice { drum_key: None, program: Some(cat.map(midi_file::program_for_category).unwrap_or(81)) }
        }
        ChannelSrc::Sample { source, .. } => {
            // an unpitched standard-library sample is a drum: written on channel 10 as the key
            // it was imported from (its root), or the General MIDI key of its kind
            let lib = p.sources.iter().find(|s| s.id == *source).and_then(|s| factory::embedded(&s.path)).filter(|f| !f.pitched);
            match lib {
                Some(f) => Voice {
                    drum_key: Some(if c.root != wave_engine::SAMPLE_ROOT { c.root } else { midi_file::drum_key_for(f.file) }),
                    program: None,
                },
                None => Voice { drum_key: None, program: None },
            }
        }
    }
}

/// The notes a pattern clip plays, in song beats (the pattern repeats over the clip).
fn clip_notes(c: &Clip, pat: &Pattern, out: &mut Vec<(Id, SmfNote, Option<u8>)>) {
    let plen = pat.len.max(0.25);
    let off = c.offset.rem_euclid(plen);
    let end = c.start + c.len;
    let mut base = c.start - off;
    while base < end {
        for n in &pat.notes {
            let t = f64::from(n.step) / 4.0;
            if t >= plen {
                continue;
            }
            let at = base + t;
            if at < c.start - 1e-9 || at >= end {
                continue;
            }
            let len = (f64::from(n.len) / 4.0).min(end - at);
            out.push((n.ch, SmfNote { key: 0, vel: (n.v * 127.0).round().clamp(1.0, 127.0) as u8, start: at, len }, n.p));
        }
        base += plen;
    }
}

/// `midi_export {out, pattern?}`: the song (pattern clips on the Playlist, muted ones left out)
/// or one pattern, as a format 1 file.
pub fn export(app: &dyn Host, a: &Value) -> R<Value> {
    let p = app.engine().project();
    let out = PathBuf::from(need_str(a, "out")?);
    let mut raw: Vec<(Id, SmfNote, Option<u8>)> = vec![];
    let what = match arg(a, "pattern").and_then(as_index) {
        Some(pid) => {
            let pat = p.patterns.iter().find(|x| x.id == pid).ok_or(format!("no pattern {pid}"))?;
            let clip = Clip { start: 0.0, len: pat.len, offset: 0.0, ..placeholder_clip(pid) };
            clip_notes(&clip, pat, &mut raw);
            pat.name.clone()
        }
        None => {
            for c in p.clips.iter().filter(|c| c.kind == ClipKind::Pat && !c.muted) {
                if let Some(pat) = p.patterns.iter().find(|x| x.id == c.source) {
                    clip_notes(c, pat, &mut raw);
                }
            }
            "song".to_string()
        }
    };
    let mut by_ch: BTreeMap<usize, Vec<SmfNote>> = BTreeMap::new();
    let order: HashMap<Id, usize> = p.channels.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
    let voices: HashMap<Id, Voice> = p.channels.iter().map(|c| (c.id, voice(&p, c))).collect();
    for (ch, mut n, key) in raw {
        let (Some(&i), Some(v)) = (order.get(&ch), voices.get(&ch)) else { continue };
        n.key = match v.drum_key {
            Some(k) => k,
            None => key.unwrap_or(p.channels[i].root),
        };
        by_ch.entry(i).or_default().push(n);
    }
    if by_ch.is_empty() {
        return Err(format!("the {what} has no notes to export"));
    }
    let mut melodic = (0u8..16).filter(|c| *c != DRUM_CHANNEL).cycle();
    let tracks: Vec<OutTrack> = by_ch
        .into_iter()
        .map(|(i, mut notes)| {
            let c = &p.channels[i];
            let v = &voices[&c.id];
            notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.key.cmp(&b.key)));
            let channel = if v.drum_key.is_some() { DRUM_CHANNEL } else { melodic.next().unwrap_or(0) };
            OutTrack { name: c.name.clone(), channel, program: v.program, notes }
        })
        .collect();
    let title = app
        .engine()
        .state()
        .path
        .as_deref()
        .and_then(|s| Path::new(s).file_stem().map(|x| x.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "Synkopa".into());
    let bytes = midi_file::write(p.bpm, &title, &tracks)?;
    if let Some(d) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    std::fs::write(&out, bytes).map_err(|e| format!("{}: {e}", out.display()))?;
    let notes: usize = tracks.iter().map(|t| t.notes.len()).sum();
    Ok(json!({ "out": out.display().to_string(), "what": what, "tracks": tracks.len(), "notes": notes, "bpm": p.bpm }))
}

fn placeholder_clip(pattern: Id) -> Clip {
    Clip {
        id: 0,
        track: 0,
        start: 0.0,
        len: 0.0,
        offset: 0.0,
        kind: ClipKind::Pat,
        source: pattern,
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
    }
}

#[cfg(test)]
#[path = "midi_actions_tests.rs"]
mod tests;
