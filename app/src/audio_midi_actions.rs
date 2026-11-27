//! Convert to MIDI (Playlist audio clip menu, `synkopa audio-to-midi`): the melody of an audio
//! clip as a new pattern under it, on a new piano channel or one the musician picks — in one undo
//! step. `preview: true` answers the same notes without changing the project, so the dialog
//! shows exactly what Convert will write.

use serde_json::{json, Value};
use wave_engine::audio_midi::{self, HeardNote, MelodyOptions};
use wave_engine::Note;

use crate::actions::{arg, as_index, need_u64, Host, R};
use crate::chords_actions::{put_pattern_under_clip, PatternPlan};

pub fn control(app: &dyn Host, cmd: &str, a: &Value, if_revision: Option<u64>) -> Option<R<Value>> {
    Some(match cmd {
        "audio_to_midi" => audio_to_midi(app, a, if_revision),
        _ => return None,
    })
}

const COLOR: &str = "#4f8ef7";

fn options(a: &Value) -> R<MelodyOptions> {
    let d = MelodyOptions::default();
    let num = |k: &str| -> R<Option<f64>> {
        match arg(a, k) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v.as_f64().filter(|x| x.is_finite()).map(Some).ok_or(format!("BAD_ARGS: `{k}` must be a number")),
        }
    };
    let key =
        |k: &str| -> R<Option<u8>> {
            num(k)?
                .map(|x| {
                    if (0.0..=127.0).contains(&x) {
                        Ok(x.round() as u8)
                    } else {
                        Err(format!("BAD_ARGS: `{k}` must be a MIDI key 0–127"))
                    }
                })
                .transpose()
        };
    let o = MelodyOptions {
        shortest: num("shortest")?.unwrap_or(d.shortest).clamp(0.0, 2.0),
        quantize: num("quantize")?.unwrap_or(0.0).clamp(0.0, 16.0),
        velocity: num("velocity")?.map(|v| v.clamp(0.0, 1.0) as f32),
        low: key("low")?,
        high: key("high")?,
    };
    if let (Some(l), Some(h)) = (o.low, o.high) {
        if l > h {
            return Err("BAD_ARGS: `low` is above `high`".into());
        }
    }
    Ok(o)
}

/// `audio_to_midi {clipId, mode?: "melody", shortest?, quantize?, velocity?, low?, high?, channel?, preview?}`.
pub fn audio_to_midi(app: &dyn Host, a: &Value, if_revision: Option<u64>) -> R<Value> {
    let mode = arg(a, "mode").and_then(Value::as_str).unwrap_or("melody");
    if mode != "melody" {
        return Err(format!("BAD_ARGS: mode `{mode}`: only `melody` can be converted"));
    }
    let o = options(a)?;
    let channel = match arg(a, "channel") {
        None | Some(Value::Null) => None,
        Some(v) => Some(as_index(v).ok_or("BAD_ARGS: `channel`: a channel id")?),
    };
    let preview = arg(a, "preview").and_then(Value::as_bool).unwrap_or(false);
    app.ensure_pitch_model();
    let e = app.engine();
    let clip_id = need_u64(a, "clipId")?;
    let v = e.pitch_view(clip_id).map_err(|x| x.to_string())?;
    let region = v.region();
    let stats = wave_engine::pitch_api::note_stats(&v.analysis, &v.notes, &v.out);
    let rate = v.analysis.rate as f64;
    // the keys the Playlist plays: the clip's own transpose and, for a resampled stretch, the
    // octaves its speed moves (the same shift Chords to Pattern applies)
    let speed = if v.clip.resample { -12.0 * v.clip.stretch.max(wave_engine::model::STRETCH_MIN).log2() } else { 0.0 };
    let transpose = (f64::from(v.clip.pitch) + speed).round() as f32;
    let heard: Vec<HeardNote> = stats
        .iter()
        .zip(&v.notes)
        .filter_map(|(s, n)| {
            let pitch = s.out.or(s.sung)? + transpose;
            let (f0, f1) = ((n.start * rate) as usize, ((n.end * rate).ceil() as usize).min(v.analysis.levels.len()));
            let lv = v.analysis.levels.get(f0..f1).filter(|l| !l.is_empty()).map(|l| l.iter().sum::<f32>() / l.len() as f32).unwrap_or(0.0);
            Some(HeardNote { start: n.start, end: n.end, pitch, level: lv * 10f32.powf(n.gain / 20.0) })
        })
        .collect();
    let notes = audio_midi::melody(&heard, region, |t| v.beat_of(t), &o);
    // the keys the take holds most of the time (5th–95th percentile of its notes): the dialog's
    // range starts there
    let mut keys: Vec<u8> =
        heard.iter().filter(|n| n.end > region.0 && n.start < region.1).map(|n| n.pitch.round().clamp(0.0, 127.0) as u8).collect();
    keys.sort_unstable();
    let q = |p: f64| keys.get(((keys.len().saturating_sub(1)) as f64 * p).round() as usize).copied();
    // the bar grid of the map where the melody lands (a 7/8 bar anchors to its own line)
    let first = notes.first().map(|n| n.step / 4.0).unwrap_or(0.0);
    let steps_per_bar = e.project().sig_map().bar_beats_at(first) * 4.0;
    let bar = notes.first().map(|n| (n.step / steps_per_bar).floor() * steps_per_bar).unwrap_or(0.0);
    let end = notes.iter().fold(bar, |m, n| m.max(n.step + n.len));
    let mut out = json!({
        "clipId": clip_id,
        "count": notes.len(),
        "notes": notes.iter().map(|n| json!({ "step": n.step - bar, "len": n.len, "p": n.p, "v": n.v })).collect::<Vec<_>>(),
        "start": bar / 4.0,
        "takeRange": match (q(0.05), q(0.95)) { (Some(l), Some(h)) => json!([l, h]), _ => Value::Null },
        "low": notes.iter().map(|n| n.p).min(),
        "high": notes.iter().map(|n| n.p).max(),
        "backend": v.analysis.backend,
        "noteBackend": v.analysis.note_backend,
        "edited": v.own,
    });
    if preview {
        return Ok(out);
    }
    if notes.is_empty() {
        return Err("no notes were found in the part of the clip the Playlist plays".into());
    }
    let src_name = e.project().sources.iter().find(|s| s.id == v.clip.source).map(|s| s.name.clone()).unwrap_or_default();
    let name = if src_name.is_empty() { "Melody".to_string() } else { format!("Melody – {src_name}") };
    let made = put_pattern_under_clip(
        e,
        &PatternPlan { clip_id, channel, name: &name, color: COLOR, start: bar / 4.0, end: end / 4.0, label: "Convert melody to MIDI" },
        if_revision,
        |ch| {
            notes
                .iter()
                .map(|n| Note {
                    ch,
                    step: (n.step - bar) as f32,
                    len: n.len as f32,
                    p: Some(n.p),
                    v: n.v,
                    trig: Some(false),
                    pan: 0.0,
                    slide: false,
                })
                .collect()
        },
    )?;
    if let (Some(o), Value::Object(mut m)) = (out.as_object_mut(), made) {
        // `clipId` stays the audio clip; the pattern's Playlist clip is `patternClipId`
        if let Some(c) = m.remove("clipId") {
            m.insert("patternClipId".into(), c);
        }
        o.extend(m);
    }
    Ok(out)
}
