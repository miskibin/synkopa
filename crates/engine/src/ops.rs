//! Every change to a [`Project`] is an [`Op`]. The GUI, the CLI and undo all go through here.
//!
//! Ops are deliberately coarse "put" operations: the caller sends the whole new
//! clip / track / effect. That keeps the wire format obvious, makes ops
//! idempotent, and means the engine never has to merge field patches.

use serde::{Deserialize, Serialize};

use crate::model::*;

/// Which notes a `transformNotes` verb touches.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TransformSelect {
    /// Note indices into the pattern's current notes (the piano roll's selection).
    Indices { indices: Vec<usize> },
    /// Notes starting in `from`..`to` steps, optionally one channel only.
    Range {
        from: f32,
        to: f32,
        #[serde(default)]
        ch: Option<Id>,
    },
}

fn major_scale_id() -> u32 {
    1
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Op {
    /// Insert or replace clips by id.
    PutClips {
        clips: Vec<Clip>,
    },
    RemoveClips {
        ids: Vec<Id>,
    },
    /// Insert (new id) or replace a track. New tracks are appended.
    PutTrack {
        track: Track,
    },
    /// Remove a track (lane) with its clips; lanes below move up one.
    RemoveTrack {
        id: Id,
    },
    /// Insert or replace a mixer insert *without* touching its effect chain
    /// (`fx` in the payload is ignored for an existing insert).
    PutInsert {
        insert: Insert,
    },
    RemoveInsert {
        id: Id,
    },
    /// Insert or replace an effect. `index` places a new effect (default: end of chain).
    PutFx {
        insert_id: Id,
        fx: Fx,
        #[serde(default)]
        index: Option<usize>,
    },
    RemoveFx {
        fx_id: Id,
    },
    MoveFx {
        fx_id: Id,
        to: usize,
    },
    /// Insert or replace a pattern. Notes may still say `inst` instead of `ch` (old
    /// scripts): they get that instrument's built-in channel (created if missing). `len` is
    /// derived (the notes' end rounded up to a bar, at least `minLen`); whatever is sent is
    /// replaced. When an existing pattern's length changes, its clips that show exactly one
    /// whole pass of it (`offset` 0, `len` = old length) take the new length; other clips
    /// keep theirs.
    PutPattern {
        pattern: Pattern,
    },
    /// Remove a pattern and every clip that plays it.
    RemovePattern {
        id: Id,
    },
    /// Apply a deterministic note transformation to a pattern. Same verb, params, seed
    /// and notes always produce identical notes (seeded splitmix64; default seed 0).
    /// `select` limits which notes the verb touches: the UI's selected note indices, or
    /// a step range (`from`..`to` in steps, optionally one channel). Absent = all notes.
    TransformNotes {
        pattern: Id,
        verb: crate::transform::TransformVerb,
        #[serde(default)]
        seed: Option<u64>,
        #[serde(default)]
        select: Option<TransformSelect>,
    },
    /// Set the project's musical key and scale for the scale-aware note tools. `key`
    /// `null` clears the key (scale tools stay pitch-neutral). `scale` absent = major.
    SetKey {
        #[serde(default)]
        key: Option<u8>,
        #[serde(default = "major_scale_id")]
        scale: u32,
    },
    /// Insert or replace a Channel Rack channel. `index` places a new channel (default:
    /// last) or moves an existing one.
    PutChannel {
        channel: Channel,
        #[serde(default)]
        index: Option<usize>,
    },
    /// Remove a channel and its notes in every pattern.
    RemoveChannel {
        id: Id,
    },
    /// Rename / recolour a source. Audio itself is changed with dedicated commands.
    PutSource {
        source: Source,
    },
    RemoveSource {
        id: Id,
    },
    /// Insert or replace an automation envelope.
    PutAutomation {
        automation: Automation,
    },
    /// Remove an automation and every clip that plays it.
    RemoveAutomation {
        id: Id,
    },
    /// Insert or replace a take group with its comp (takes, regions, crossfade).
    PutComp {
        comp: Comp,
    },
    /// Remove a take group and every clip that plays it.
    RemoveComp {
        id: Id,
    },
    /// Replace the lyrics notebook (all pages, in tab order). Never touches the audio graph.
    PutNotebook {
        notebook: Notebook,
    },
    /// Insert or replace a Playlist marker by id (add, move and rename are all this). Markers
    /// stay sorted by position. Never touches the audio graph.
    PutMarker {
        marker: Marker,
    },
    /// Remove a Playlist marker.
    RemoveMarker {
        id: Id,
    },
    /// Note-level pitch edits of an audio clip (`crate::tune_edit`): move, reset, split and
    /// merge notes and draw pitch, addressed by note index in the clip's current segmentation.
    /// Nothing else of the clip changes.
    EditTune {
        clip: Id,
        edit: crate::tune_edit::TuneEdit,
    },
    SetBpm {
        bpm: f64,
    },
    /// Set the song's tempo points (the tempo map). An empty list = the constant `bpm`
    /// everywhere. Points are beat-sorted and their BPM clamped on the way in; two points
    /// at one beat are a jump; at most [`crate::model::MAX_TEMPO_POINTS`]. `SetBpm`
    /// clears the map (the tempo knob edits the constant case).
    SetTempoPoints {
        #[serde(default)]
        points: Vec<crate::model::TempoPoint>,
    },
    /// Set the song's signature points (mid-song meter changes). An empty list = 4/4
    /// everywhere. Points are beat-sorted; `num` clamps 1..=32 and `den` snaps to 1/2/4/
    /// 8/16 (anything else reads as 4) on the way in; two points at one beat: the last
    /// governs; at most [`crate::model::MAX_SIG_POINTS`].
    SetSigPoints {
        #[serde(default)]
        points: Vec<crate::model::SigPoint>,
    },
    SetLoop {
        #[serde(rename = "loop")]
        range: Option<LoopRange>,
    },
    /// Replace the pairs of inserts the mix assistant does not report as masking each other
    /// (`crate::masking`). Never touches the audio.
    SetMaskIgnore {
        pairs: Vec<[Id; 2]>,
    },
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum OpError {
    #[error("no {what} with id {id}")]
    Missing { what: &'static str, id: Id },
    #[error("{0}")]
    Invalid(String),
}

/// What an op touched, so the engine knows how much of the audio graph to rebuild.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dirty {
    /// Clips, patterns, sources, tempo, loop: rebuild the timeline.
    pub timeline: bool,
    /// Inserts, routing, effect chains: rebuild the mixer graph.
    pub mixer: bool,
    /// Only parameters of existing things changed: the fast path is enough.
    pub params: bool,
}

impl Dirty {
    pub fn merge(&mut self, o: Dirty) {
        self.timeline |= o.timeline;
        self.mixer |= o.mixer;
        self.params |= o.params;
    }
    pub fn any(&self) -> bool {
        self.timeline || self.mixer || self.params
    }
}

/// The per-note sanity PutPattern and TransformNotes share: finite timing, velocity in
/// range, a key that exists, inside the arrangement's total length.
fn check_notes(p: &Project, pt: &Pattern) -> Result<(), OpError> {
    for n in &pt.notes {
        if !(n.step.is_finite() && n.step >= 0.0 && n.len.is_finite() && n.len > 0.0) {
            return Err(OpError::Invalid(format!("pattern {}: a note has an invalid step/len", pt.id)));
        }
        if !(n.v.is_finite() && (0.0..=1.0).contains(&n.v)) {
            return Err(OpError::Invalid("note velocity `v` must be 0..1".into()));
        }
        if !(n.pan.is_finite() && (-1.0..=1.0).contains(&n.pan)) {
            return Err(OpError::Invalid("note pan `pan` must be -1..1".into()));
        }
        if n.p.is_some_and(|k| k > 127) {
            return Err(OpError::Invalid("note key `p` must be 0..127".into()));
        }
        if (n.step + n.len) as f64 > TOTAL_BEATS * 4.0 {
            return Err(OpError::Invalid(format!("pattern {}: notes must end within {TOTAL_BEATS} beats", pt.id)));
        }
    }
    if let Some(n) = pt.notes.iter().find(|n| p.channel(n.ch).is_none()) {
        return Err(OpError::Missing { what: "channel", id: n.ch });
    }
    Ok(())
}

/// Checks a pattern's per-channel loops (FL Studio 20.1's channel looping): each entry
/// names a channel, sits inside the pattern, and expands into a bounded number of
/// scheduled notes.
pub(crate) fn check_channel_loops(p: &Project, pt: &Pattern) -> Result<(), OpError> {
    let mut extra = 0u64;
    for (ch, &l) in &pt.channel_loops {
        if p.channel(*ch).is_none() {
            return Err(OpError::Missing { what: "channel", id: *ch });
        }
        let steps = (pt.len * 4.0).round() as u32;
        if l == 0 || l > steps {
            return Err(OpError::Invalid(format!("pattern {}: a channel loop must be 1..={steps} steps", pt.id)));
        }
        let cell = pt.notes.iter().filter(|n| n.ch == *ch).count() as u64;
        let reps = ((f64::from(steps) / f64::from(l)).ceil() as u64).saturating_sub(1);
        extra += reps * cell;
    }
    // the expansion adds every entry's repeats: the bound is on the sum, not per entry
    if pt.notes.len() as u64 + extra > crate::model::MAX_LOOP_NOTES as u64 {
        return Err(OpError::Invalid(format!(
            "pattern {}: the channel loops may not repeat into more than {} notes",
            pt.id,
            crate::model::MAX_LOOP_NOTES
        )));
    }
    Ok(())
}

fn bump(p: &mut Project, id: Id) {
    if id >= p.next_id {
        p.next_id = id + 1;
    }
}

/// Applies one op. On error the project is unchanged.
pub fn apply(p: &mut Project, op: &Op) -> Result<Dirty, OpError> {
    let mut d = Dirty::default();
    match op {
        Op::PutClips { clips } => {
            for c in clips {
                if c.len <= 0.0 || c.start < 0.0 || !c.start.is_finite() || !c.len.is_finite() {
                    return Err(OpError::Invalid(format!("clip {} has an invalid range", c.id)));
                }
                if c.track as usize >= p.tracks.len() {
                    return Err(OpError::Invalid(format!("clip {} is on track {} which does not exist", c.id, c.track)));
                }
                if !(c.stretch.is_finite() && (crate::model::STRETCH_MIN - 1e-9..=crate::model::STRETCH_MAX + 1e-9).contains(&c.stretch)) {
                    return Err(OpError::Invalid(format!(
                        "clip {}: stretch must be {}..{}",
                        c.id,
                        crate::model::STRETCH_MIN,
                        crate::model::STRETCH_MAX
                    )));
                }
                if let Some(ins) = c.insert {
                    if c.kind != ClipKind::Audio {
                        return Err(OpError::Invalid(format!("clip {}: only audio clips have their own mixer insert", c.id)));
                    }
                    if p.insert(ins).is_none() {
                        return Err(OpError::Missing { what: "insert", id: ins });
                    }
                }
                if c.kind != ClipKind::Audio && (c.stretch != 1.0 || c.resample) {
                    return Err(OpError::Invalid(format!("clip {}: only audio clips can be stretched", c.id)));
                }
                if !(c.pitch.is_finite() && c.pitch.abs() <= crate::model::CLIP_PITCH + 1e-4) {
                    return Err(OpError::Invalid(format!("clip {}: pitch must be within ±{} semitones", c.id, crate::model::CLIP_PITCH)));
                }
                if c.root > 127 {
                    return Err(OpError::Invalid(format!("clip {}: root key must be 0..127", c.id)));
                }
                if c.kind != ClipKind::Audio && c.pitch != 0.0 {
                    return Err(OpError::Invalid(format!("clip {}: only audio clips can be transposed", c.id)));
                }
                let known = match c.kind {
                    ClipKind::Pat => p.patterns.iter().any(|x| x.id == c.source),
                    ClipKind::Audio => p.sources.iter().any(|x| x.id == c.source),
                    ClipKind::Auto => p.automations.iter().any(|x| x.id == c.source),
                    ClipKind::Comp => p.comps.iter().any(|x| x.id == c.source),
                };
                if !known {
                    return Err(OpError::Missing { what: "clip source", id: c.source });
                }
                if let Some(r) = c.tune.as_ref().and_then(|t| t.reference.as_ref()) {
                    if r.melody.is_none() && !p.sources.iter().any(|x| x.id == r.source) {
                        return Err(OpError::Missing { what: "reference source", id: r.source });
                    }
                    if let Some(m) = &r.melody {
                        if m.notes.iter().any(|n| n.end.partial_cmp(&n.start) != Some(std::cmp::Ordering::Greater) || n.key > 127) {
                            return Err(OpError::Invalid(format!("clip {}: melody notes need end > start and a key 0..127", c.id)));
                        }
                    }
                    if r.speed.is_some_and(|v| !(0.0..=1000.0).contains(&v)) {
                        return Err(OpError::Invalid(format!("clip {}: reference speed must be 0..1000 ms", c.id)));
                    }
                    if [r.correction, r.vibrato, r.influence].iter().any(|v| !(0.0..=1.0).contains(v)) {
                        return Err(OpError::Invalid(format!("clip {}: reference correction, vibrato and influence must be 0..1", c.id)));
                    }
                    if r.octave.is_some_and(|o| o % 12 != 0 || o.abs() > 36) || r.key > 11 || r.scale > 4 {
                        return Err(OpError::Invalid(format!(
                            "clip {}: reference octave must be a multiple of 12 within ±36, key 0..11, scale 0..4",
                            c.id
                        )));
                    }
                }
            }
            for c in clips {
                bump(p, c.id);
                match p.clips.iter_mut().find(|x| x.id == c.id) {
                    Some(x) => *x = c.clone(),
                    None => p.clips.push(c.clone()),
                }
            }
            d.timeline = true;
        }
        Op::RemoveClips { ids } => {
            p.clips.retain(|c| !ids.contains(&c.id));
            d.timeline = true;
        }
        Op::PutTrack { track } => {
            if p.insert(track.insert).is_none() {
                return Err(OpError::Missing { what: "insert", id: track.insert });
            }
            bump(p, track.id);
            match p.tracks.iter_mut().find(|x| x.id == track.id) {
                Some(x) => {
                    if x.insert != track.insert {
                        d.mixer = true;
                    }
                    *x = track.clone();
                }
                None => {
                    p.tracks.push(track.clone());
                    d.mixer = true;
                }
            }
            d.params = true;
            d.timeline = true;
        }
        Op::RemoveTrack { id } => {
            let idx = p.tracks.iter().position(|t| t.id == *id).ok_or(OpError::Missing { what: "track", id: *id })? as u32;
            if p.tracks.len() == 1 {
                return Err(OpError::Invalid("the last track cannot be removed".into()));
            }
            p.tracks.remove(idx as usize);
            p.clips.retain(|c| c.track != idx);
            for c in p.clips.iter_mut() {
                if c.track > idx {
                    c.track -= 1;
                }
            }
            d.timeline = true;
            d.mixer = true;
        }
        Op::PutInsert { insert } => {
            crate::routing::validate(p, insert).map_err(OpError::Invalid)?;
            bump(p, insert.id);
            match p.inserts.iter_mut().find(|x| x.id == insert.id) {
                Some(x) => {
                    // where it sends (not how loud) changes the graph; a level is a fast-path value
                    let rewired = x.master != insert.master
                        || x.sends.len() != insert.sends.len()
                        || x.sends.iter().zip(&insert.sends).any(|(a, b)| a.to != b.to);
                    let fx = std::mem::take(&mut x.fx);
                    *x = Insert { fx, ..insert.clone() };
                    if rewired {
                        d.mixer = true;
                    } else {
                        d.params = true;
                    }
                }
                None => {
                    let mut m = insert.clone();
                    for f in &m.fx {
                        bump(p, f.id);
                    }
                    m.fx.retain(|f| f.kind != FxKind::Plugin);
                    p.inserts.push(m);
                    d.mixer = true;
                }
            }
        }
        Op::RemoveInsert { id } => {
            if *id == 0 {
                return Err(OpError::Invalid("the master cannot be removed".into()));
            }
            if p.insert(*id).is_none() {
                return Err(OpError::Missing { what: "insert", id: *id });
            }
            p.inserts.retain(|m| m.id != *id);
            crate::routing::forget(p, *id);
            // effects listening to it as their sidechain go back to their own signal
            for f in p.inserts.iter_mut().flat_map(|m| m.fx.iter_mut()) {
                if f.key == Some(*id) {
                    f.key = None;
                }
            }
            // ...and the mix assistant forgets the pairs it was in
            p.mask_ignore.retain(|q| q[0] != *id && q[1] != *id);
            for t in &mut p.tracks {
                if t.insert == *id {
                    t.insert = 0;
                }
            }
            // a clip routed there plays through its track again
            for c in &mut p.clips {
                if c.insert == Some(*id) {
                    c.insert = None;
                }
            }
            d.mixer = true;
        }
        Op::PutFx { insert_id, fx, index } => {
            validate_fx(fx)?;
            validate_key(p, *insert_id, fx)?;
            if let Some((m, old)) = p.find_fx(fx.id) {
                if m.id != *insert_id || old.kind != fx.kind {
                    return Err(OpError::Invalid(format!("effect {} cannot change insert or type; remove and add it", fx.id)));
                }
                let plugin_changed =
                    old.plugin.as_ref().map(|x| (&x.format, &x.path, &x.cid)) != fx.plugin.as_ref().map(|x| (&x.format, &x.path, &x.cid));
                if plugin_changed {
                    return Err(OpError::Invalid("a plugin slot cannot swap its plugin; remove and add it".into()));
                }
                // a parameter that changes the effect's latency (look-ahead) re-plans the PDC
                // so does a new sidechain: it reorders the inserts (the key is processed first)
                let bypass_changed = old.bypass != fx.bypass
                    || old.key != fx.key
                    || crate::catalog::latency(old, 48000.0) != crate::catalog::latency(fx, 48000.0);
                let m = p.inserts.iter_mut().find(|m| m.id == *insert_id).unwrap();
                let slot = m.fx.iter_mut().find(|f| f.id == fx.id).unwrap();
                *slot = fx.clone();
                d.params = true;
                d.mixer |= bypass_changed;
            } else {
                let m = p.inserts.iter_mut().find(|m| m.id == *insert_id).ok_or(OpError::Missing { what: "insert", id: *insert_id })?;
                let at = index.unwrap_or(m.fx.len()).min(m.fx.len());
                m.fx.insert(at, fx.clone());
                bump(p, fx.id);
                d.mixer = true;
            }
        }
        Op::RemoveFx { fx_id } => {
            let mut found = false;
            for m in &mut p.inserts {
                let n = m.fx.len();
                m.fx.retain(|f| f.id != *fx_id);
                found |= m.fx.len() != n;
            }
            if !found {
                return Err(OpError::Missing { what: "effect", id: *fx_id });
            }
            d.mixer = true;
        }
        Op::MoveFx { fx_id, to } => {
            let m = p
                .inserts
                .iter_mut()
                .find(|m| m.fx.iter().any(|f| f.id == *fx_id))
                .ok_or(OpError::Missing { what: "effect", id: *fx_id })?;
            let from = m.fx.iter().position(|f| f.id == *fx_id).unwrap();
            let f = m.fx.remove(from);
            let to = (*to).min(m.fx.len());
            m.fx.insert(to, f);
            d.mixer = true;
        }
        Op::PutPattern { pattern } => {
            if !(pattern.swing.is_finite() && (0.0..=1.0).contains(&pattern.swing)) {
                return Err(OpError::Invalid("swing must be 0..1".into()));
            }
            let mut pt = pattern.clone();
            check_notes(p, &pt)?;
            // a pattern keeps the bar length it was created with: the fit rounds to whole
            // bars of that grid, never to the signature where a clip happens to sit now
            if !(pt.bar.is_finite() && (0.25..=TOTAL_BEATS).contains(&pt.bar)) {
                return Err(OpError::Invalid("pattern bar must be 0.25..2048 beats".into()));
            }
            pt.fit_len();
            check_channel_loops(p, &pt)?;
            bump(p, pt.id);
            let (id, len) = (pt.id, pt.len);
            match p.patterns.iter_mut().find(|x| x.id == id) {
                Some(x) => {
                    let old = x.len;
                    *x = pt;
                    if (old - len).abs() > 1e-9 {
                        follow_pattern_length(p, id, old, len);
                    }
                }
                None => p.patterns.push(pt),
            }
            d.timeline = true;
        }
        Op::PutChannel { channel, index } => {
            let c = channel;
            if !(c.vol.is_finite() && (0.0..=1.0).contains(&c.vol)) {
                return Err(OpError::Invalid("channel vol is a fader position 0..1".into()));
            }
            if !(c.pan.is_finite() && (-1.0..=1.0).contains(&c.pan)) {
                return Err(OpError::Invalid("channel pan must be -1..1".into()));
            }
            if c.root > 127 {
                return Err(OpError::Invalid("channel root must be 0..127".into()));
            }
            if c.group.as_deref().is_some_and(|g| g.trim().is_empty() || g.chars().count() > 40) {
                return Err(OpError::Invalid("channel group: 1..40 characters (absent = unsorted)".into()));
            }
            if let ChannelSrc::Sample { source, sampler, slices } = &c.src {
                if !p.sources.iter().any(|s| s.id == *source) {
                    return Err(OpError::Missing { what: "source", id: *source });
                }
                sampler.check().map_err(OpError::Invalid)?;
                if slices.len() > crate::model::MAX_SLICES {
                    return Err(OpError::Invalid(format!("at most {} slices", crate::model::MAX_SLICES)));
                }
                if slices.iter().any(|x| !(x.is_finite() && (0.0..1.0).contains(x))) || slices.windows(2).any(|w| w[0] >= w[1]) {
                    return Err(OpError::Invalid("slices must be increasing fractions of the file in 0..1".into()));
                }
            }
            if let ChannelSrc::Synth { synth } = &c.src {
                for (k, v) in &synth.params {
                    if crate::poly::params::index_of(k).is_none() {
                        return Err(OpError::Invalid(format!("synth has no parameter `{k}`")));
                    }
                    if !v.is_finite() {
                        return Err(OpError::Invalid(format!("synth parameter `{k}` must be a number")));
                    }
                }
            }
            if let ChannelSrc::Keys { keys } = &c.src {
                let (r, y) = (crate::keys::RELEASE_MS, crate::keys::DYNAMICS);
                if keys.release.is_some_and(|ms| !(ms.is_finite() && (r.0..=r.1).contains(&ms))) {
                    return Err(OpError::Invalid(format!("keys release must be {}..{} ms", r.0, r.1)));
                }
                if crate::keys::instrument(&keys.instrument).is_none() {
                    return Err(OpError::Invalid(format!("Synkopa Instruments has no instrument `{}`", keys.instrument)));
                }
                if let Some(a) = keys.articulation {
                    if !crate::keys::articulations(&keys.instrument).iter().any(|x| x.key == a) {
                        return Err(OpError::Invalid(format!("{} has no articulation on key {a}", keys.instrument)));
                    }
                }
                if !(keys.dynamics.is_finite() && (y.0..=y.1).contains(&keys.dynamics)) {
                    return Err(OpError::Invalid("keys dynamics must be 0..100 %".into()));
                }
                use crate::keys::{ATTACK_MS, NOISE, TONE, TUNE, WIDTH};
                for (name, v, (lo, hi, _), unit) in [
                    ("attack", keys.attack, ATTACK_MS, "ms"),
                    ("tone", keys.tone, TONE, ""),
                    ("width", keys.width, WIDTH, "%"),
                    ("tune", keys.tune, TUNE, "cents"),
                    ("noise", keys.noise, NOISE, "%"),
                ] {
                    if !(v.is_finite() && (lo..=hi).contains(&v)) {
                        return Err(OpError::Invalid(format!("keys {name} must be {lo}..{hi} {unit}").trim_end().to_string()));
                    }
                }
            }
            if let ChannelSrc::MusicBox { music_box } = &c.src {
                crate::musicbox::check(
                    music_box.ring,
                    music_box.brightness,
                    music_box.body,
                    music_box.mechanical,
                    music_box.spread,
                    music_box.release,
                )
                .map_err(OpError::Invalid)?;
            }
            if let ChannelSrc::Plugin { plugin } = &c.src {
                if plugin.format != PluginRef::VST3 {
                    return Err(OpError::Invalid(format!("{} instruments are not supported (VST3 only)", plugin.format_label())));
                }
                if plugin.cid.trim().is_empty() || plugin.path.trim().is_empty() {
                    return Err(OpError::Invalid("a plugin instrument needs the plugin's path and class id".into()));
                }
            }
            if let Some(ins) = c.insert {
                if p.insert(ins).is_none() {
                    return Err(OpError::Missing { what: "insert", id: ins });
                }
            }
            bump(p, c.id);
            match p.channels.iter().position(|x| x.id == c.id) {
                Some(i) => {
                    let x = &p.channels[i];
                    // knobs and mute/solo take the fast path; what a note plays or where it goes rebuilds
                    // a synth's sound (its parameters) takes the fast path too
                    let same_kind = match (&x.src, &c.src) {
                        (ChannelSrc::Synth { .. }, ChannelSrc::Synth { .. }) => true,
                        // a plugin's saved state is not what its notes play: the live instance is
                        (ChannelSrc::Plugin { plugin: a }, ChannelSrc::Plugin { plugin: b }) => a.same_class(b),
                        _ => false,
                    };
                    // (a piano's knobs are read from the channel table, which the timeline carries)
                    if (x.src != c.src && !same_kind) || x.insert != c.insert || x.root != c.root {
                        d.timeline = true;
                    } else {
                        d.params = true;
                    }
                    p.channels[i] = c.clone();
                    if let Some(to) = index {
                        let ch = p.channels.remove(i);
                        let to = (*to).min(p.channels.len());
                        p.channels.insert(to, ch);
                        d.timeline = true;
                    }
                }
                None => {
                    let at = index.unwrap_or(p.channels.len()).min(p.channels.len());
                    p.channels.insert(at, c.clone());
                    d.timeline = true;
                }
            }
        }
        Op::RemoveChannel { id } => {
            if p.channel(*id).is_none() {
                return Err(OpError::Missing { what: "channel", id: *id });
            }
            p.channels.retain(|c| c.id != *id);
            for pt in p.patterns.iter_mut() {
                pt.notes.retain(|n| n.ch != *id);
                pt.channel_loops.remove(id);
            }
            d.timeline = true;
        }
        Op::PutSource { source } => {
            let s = p.sources.iter_mut().find(|x| x.id == source.id).ok_or(OpError::Missing { what: "source", id: source.id })?;
            s.name = source.name.clone();
            s.color = source.color.clone();
        }
        Op::RemoveSource { id } => {
            p.sources.retain(|s| s.id != *id);
            // sampler channels playing it go, with their notes
            let gone: Vec<Id> = p.channels.iter().filter(|c| c.sample_source() == Some(*id)).map(|c| c.id).collect();
            p.channels.retain(|c| !gone.contains(&c.id));
            for pt in p.patterns.iter_mut() {
                pt.notes.retain(|n| !gone.contains(&n.ch));
            }
            p.clips.retain(|c| !(c.kind == ClipKind::Audio && c.source == *id));
            // takes recorded into this source go too; a comp left without takes goes with its clips
            for cp in p.comps.iter_mut() {
                let gone: Vec<Id> = cp.takes.iter().filter(|t| t.source == *id).map(|t| t.id).collect();
                for t in gone {
                    cp.remove_take(t);
                }
            }
            let empty: Vec<Id> = p.comps.iter().filter(|c| c.takes.is_empty()).map(|c| c.id).collect();
            p.comps.retain(|c| !c.takes.is_empty());
            p.clips.retain(|c| !(c.kind == ClipKind::Comp && empty.contains(&c.source)));
            for t in p.clips.iter_mut().filter_map(|c| c.tune.as_mut()) {
                if t.reference.as_ref().is_some_and(|r| r.melody.is_none() && r.source == *id) {
                    t.reference = None;
                }
            }
            d.timeline = true;
        }
        Op::PutAutomation { automation } => {
            let mut a = automation.clone();
            if a.points.iter().any(|q| !q.t.is_finite() || !q.v.is_finite() || !q.tension.is_finite()) {
                return Err(OpError::Invalid("automation points must be numbers".into()));
            }
            a.points.sort_by(|x, y| x.t.total_cmp(&y.t));
            for q in a.points.iter_mut() {
                q.v = q.v.clamp(0.0, 1.0);
                q.tension = q.tension.clamp(-1.0, 1.0);
            }
            bump(p, a.id);
            match p.automations.iter_mut().find(|x| x.id == a.id) {
                Some(x) => *x = a,
                None => p.automations.push(a),
            }
            d.timeline = true;
        }
        Op::RemovePattern { id } => {
            if !p.patterns.iter().any(|x| x.id == *id) {
                return Err(OpError::Missing { what: "pattern", id: *id });
            }
            p.patterns.retain(|x| x.id != *id);
            p.clips.retain(|c| !(c.kind == ClipKind::Pat && c.source == *id));
            d.timeline = true;
        }
        Op::EditTune { clip, edit } => {
            let Some(c) = p.clips.iter_mut().find(|c| c.id == *clip) else {
                return Err(OpError::Missing { what: "clip", id: *clip });
            };
            if c.kind != ClipKind::Audio {
                return Err(OpError::Invalid(format!("clip {clip}: only audio clips have pitch edits")));
            }
            c.tune = crate::tune_edit::apply(c.tune.as_ref(), edit).map_err(|m| OpError::Invalid(format!("clip {clip}: {m}")))?;
            d.timeline = true;
        }
        Op::TransformNotes { pattern: pid, verb, seed, select } => {
            let Some(pi) = p.patterns.iter().position(|x| x.id == *pid) else {
                return Err(OpError::Missing { what: "pattern", id: *pid });
            };
            // everything the verb needs, read before the pattern is borrowed mutably.
            // Generator bars are bars of the signature where the pattern plays — steps per
            // bar is always a whole number (`SIG_DENS` keeps bars on the step grid).
            let spb = (p.patterns[pi].bar * 4.0).round().max(1.0) as u32;
            let len_steps = (p.patterns[pi].len * 4.0).round().max(f64::from(spb)) as u32;
            let ks = p.key.map(|k| (k, p.scale));
            let roots: Vec<(Id, u8)> = p.channels.iter().map(|c| (c.id, c.root)).collect();
            let chan_ok = |id: Id| roots.iter().any(|(cid, _)| *cid == id);
            // generator sizes are bounded before anything allocates: a CLI or UI typo
            // ("--n 4000000000") must fail as a validation error, not as an OOM
            const MAX_SLOTS: u32 = 512;
            const MAX_BARS: u32 = 32;
            match verb {
                crate::transform::TransformVerb::EuclidFill { ch, k, n, .. } => {
                    if !chan_ok(*ch) {
                        return Err(OpError::Missing { what: "channel", id: *ch });
                    }
                    if !(1..=MAX_SLOTS).contains(n) || *k == 0 || *k > MAX_SLOTS {
                        return Err(OpError::Invalid(format!("euclidFill k/n must be 1..={MAX_SLOTS}")));
                    }
                }
                crate::transform::TransformVerb::Melody { ch, bars, .. } => {
                    if !chan_ok(*ch) {
                        return Err(OpError::Missing { what: "channel", id: *ch });
                    }
                    if !(1..=MAX_BARS).contains(bars) {
                        return Err(OpError::Invalid(format!("melody bars must be 1..={MAX_BARS}")));
                    }
                }
                crate::transform::TransformVerb::EuclidDrums { voices, bars } => {
                    if let Some(v) = voices.iter().find(|v| !chan_ok(v.ch)) {
                        return Err(OpError::Missing { what: "channel", id: v.ch });
                    }
                    if !(1..=MAX_BARS).contains(bars) {
                        return Err(OpError::Invalid(format!("drums bars must be 1..={MAX_BARS}")));
                    }
                    if voices.iter().any(|v| v.k == 0 || v.k > MAX_SLOTS || v.n == 0 || v.n > MAX_SLOTS) {
                        return Err(OpError::Invalid(format!("drums voice k/n must be 1..={MAX_SLOTS}")));
                    }
                }
                crate::transform::TransformVerb::Chord { harmony } => {
                    if !crate::transform::HARMONY_TYPES.contains(&harmony.as_str()) {
                        return Err(OpError::Invalid(format!(
                            "chord harmony must be one of {} (the other chord types are stamp-only)",
                            crate::transform::HARMONY_TYPES.join(", ")
                        )));
                    }
                    if harmony == "diatonic" && (p.key.is_none() || p.scale == 0) {
                        return Err(OpError::Invalid("chord diatonic needs a project key and scale (`synkopa key A minor`)".into()));
                    }
                }
                crate::transform::TransformVerb::Progression { ch, bars, template, degrees, .. } => {
                    if !chan_ok(*ch) {
                        return Err(OpError::Missing { what: "channel", id: *ch });
                    }
                    if p.key.is_none() || p.scale == 0 {
                        return Err(OpError::Invalid("progression needs a project key and scale (`synkopa key A minor`)".into()));
                    }
                    if degrees.is_empty() {
                        if !(1..=MAX_BARS).contains(bars) {
                            return Err(OpError::Invalid(format!("progression bars must be 1..={MAX_BARS}")));
                        }
                        if template != "chance" && !crate::transform::PROGRESSION_TEMPLATES.iter().any(|(id, _)| id == template) {
                            let known: Vec<&str> = crate::transform::PROGRESSION_TEMPLATES.iter().map(|(id, _)| *id).collect();
                            return Err(OpError::Invalid(format!(
                                "unknown progression template `{template}` ({}, chance)",
                                known.join(", ")
                            )));
                        }
                    } else {
                        // an explicit degree list replaces template and bars; bounded the same
                        let n = crate::transform::scale_offsets(p.scale).len();
                        if degrees.len() > MAX_BARS as usize {
                            return Err(OpError::Invalid(format!("progression degrees: at most {MAX_BARS} entries")));
                        }
                        if let Some(bad) = degrees.iter().find(|&&d| d as usize >= n) {
                            return Err(OpError::Invalid(format!("progression degrees must be 0..{} in this scale (got {bad})", n - 1)));
                        }
                    }
                }
                crate::transform::TransformVerb::Chop { len } => {
                    if !(len.is_finite() && *len >= crate::transform::MIN_PIECE) {
                        return Err(OpError::Invalid("chop len must be at least 0.25 steps (a 64th)".into()));
                    }
                }
                crate::transform::TransformVerb::Arpeggiate { mode, rate, gate, octaves } => {
                    if !crate::transform::ARP_MODES.contains(&mode.as_str()) {
                        return Err(OpError::Invalid(format!("arpeggiate mode must be one of {}", crate::transform::ARP_MODES.join(", "))));
                    }
                    if !(rate.is_finite() && *rate >= crate::transform::MIN_PIECE) {
                        return Err(OpError::Invalid("arpeggiate rate must be at least 0.25 steps (a 64th)".into()));
                    }
                    if !(gate.is_finite() && (0.05..=1.0).contains(gate)) {
                        return Err(OpError::Invalid("arpeggiate gate must be 0.05..1".into()));
                    }
                    if !(1..=4).contains(octaves) {
                        return Err(OpError::Invalid("arpeggiate octaves must be 1..4".into()));
                    }
                }
                crate::transform::TransformVerb::Humanize { .. }
                | crate::transform::TransformVerb::Quantize { .. }
                | crate::transform::TransformVerb::Strum { .. }
                | crate::transform::TransformVerb::Glue {} => {}
            }
            // positional verbs (humanize/quantize/strum) map every selected note 1:1.
            // Generators replace every note of their target channel(s) and leave the
            // other channels alone, so a selection does not scope them: scoping would keep
            // unselected notes of the target channel under the new ones (duplicates)
            // generators are unscoped (the selection is ignored) and replace their
            // channel; `chord` stays scoped to the selection but grows it (roots stay,
            // tones are appended), so its result merges with the unselected notes
            let unscoped = matches!(
                verb,
                crate::transform::TransformVerb::EuclidFill { .. }
                    | crate::transform::TransformVerb::EuclidDrums { .. }
                    | crate::transform::TransformVerb::Melody { .. }
                    | crate::transform::TransformVerb::Progression { .. }
            );
            // `chord` grows the selection; chop, glue and the arpeggiator reshape it (more or
            // fewer notes in place of the selected ones): all of them merge the same way
            let growing = matches!(
                verb,
                crate::transform::TransformVerb::Chord { .. }
                    | crate::transform::TransformVerb::Chop { .. }
                    | crate::transform::TransformVerb::Glue {}
                    | crate::transform::TransformVerb::Arpeggiate { .. }
            );
            let all_notes = select.is_none() || unscoped;
            // a copy: every refusal below leaves the pattern as it was (`apply` promises that)
            let mut notes = p.patterns[pi].notes.clone();
            let idx: Vec<usize> = match select {
                None => (0..notes.len()).collect(),
                Some(crate::ops::TransformSelect::Indices { indices }) => {
                    if indices.iter().any(|&i| i >= notes.len()) {
                        return Err(OpError::Invalid("select: a note index is out of range".into()));
                    }
                    indices.clone()
                }
                Some(crate::ops::TransformSelect::Range { from, to, ch }) => {
                    let (from, to) = ((*from).min(*to), (*to).max(*from));
                    (0..notes.len())
                        .filter(|&i| {
                            let n = &notes[i];
                            n.step >= from && n.step < to && ch.is_none_or(|c| n.ch == c)
                        })
                        .collect()
                }
            };
            // (the selection is still validated above, so a bad index fails either way)
            let idx: Vec<usize> = if unscoped { (0..notes.len()).collect() } else { idx };
            let mut target: Vec<Note> = idx.iter().map(|&i| notes[i].clone()).collect();
            // chop and the arpeggiator multiply notes: bounded before they allocate
            const MAX_MADE: usize = 20_000;
            let made = match verb {
                crate::transform::TransformVerb::Chop { len } => crate::transform::chop_count(&target, *len),
                crate::transform::TransformVerb::Arpeggiate { rate, .. } => crate::transform::arpeggiate_count(&target, *rate),
                _ => 0,
            };
            if made > MAX_MADE {
                return Err(OpError::Invalid(format!(
                    "that would make {made} notes (at most {MAX_MADE}): pick fewer notes or a longer step"
                )));
            }
            let rng = &mut crate::transform::SplitMix64::new(seed.unwrap_or(0));
            match verb {
                crate::transform::TransformVerb::Humanize { timing, velocity } => {
                    crate::transform::humanize(&mut target, *timing, *velocity, rng);
                }
                crate::transform::TransformVerb::Quantize { grid, amount, scale_aware } => {
                    crate::transform::quantize(&mut target, *grid, *amount, if *scale_aware { ks } else { None }, &roots);
                }
                crate::transform::TransformVerb::EuclidFill { ch, k, n, rotate, vel } => {
                    crate::transform::euclidean_fill(&mut target, *ch, *k, *n, *rotate, *vel, len_steps);
                }
                crate::transform::TransformVerb::Strum { width, ramp } => {
                    crate::transform::strum(&mut target, *width, *ramp);
                }
                crate::transform::TransformVerb::EuclidDrums { voices, bars } => {
                    // the seed shades each voice's velocity ±15 %: same seed, same shading;
                    // a new seed gives the same groove a subtly different punch
                    let shade = &mut crate::transform::SplitMix64::new(seed.unwrap_or(0));
                    for v in voices {
                        let vel = (v.vel * (1.0 + shade.pm1() * 0.15)).clamp(0.1, 1.0);
                        crate::transform::euclidean_fill(&mut target, v.ch, v.k, v.n, v.rotate, vel, (*bars).max(1) * spb);
                    }
                }
                crate::transform::TransformVerb::Melody { ch, bars, density, repeat } => {
                    crate::transform::melody(
                        &mut target,
                        *ch,
                        (ks.map(|(k, _)| k).unwrap_or(0), p.scale),
                        *bars,
                        spb,
                        *density,
                        *repeat,
                        seed.unwrap_or(0),
                    );
                }
                crate::transform::TransformVerb::Chord { harmony } => {
                    crate::transform::chord(&mut target, &notes, ks, harmony, &roots);
                }
                crate::transform::TransformVerb::Chop { len } => crate::transform::chop(&mut target, *len),
                crate::transform::TransformVerb::Glue {} => crate::transform::glue(&mut target, &roots),
                crate::transform::TransformVerb::Arpeggiate { mode, rate, gate, octaves } => {
                    crate::transform::arpeggiate(&mut target, &roots, mode, *rate, *gate, *octaves, rng);
                }
                crate::transform::TransformVerb::Progression { ch, bars, template, voice_lead, degrees } => {
                    let degs: Vec<usize> = if degrees.is_empty() {
                        let n = crate::transform::scale_offsets(p.scale).len();
                        crate::transform::progression_degrees(template, *bars as usize, seed.unwrap_or(0), n).unwrap_or_default()
                    } else {
                        degrees.iter().map(|&d| d as usize).collect()
                    };
                    crate::transform::progression(&mut target, *ch, p.key.unwrap_or(0), p.scale, &degs, spb, *voice_lead);
                }
            }
            let new_notes = if all_notes {
                target
            } else if growing {
                // `chord` changed cardinality: everything unselected stays, the roots
                // keep their array slots and the appended tones follow the target
                let selected: std::collections::HashSet<usize> = idx.iter().copied().collect();
                let mut merged: Vec<Note> =
                    notes.iter().enumerate().filter(|(i, _)| !selected.contains(i)).map(|(_, n)| n.clone()).collect();
                merged.extend(target);
                merged
            } else {
                for (slot, i) in idx.iter().enumerate() {
                    notes[*i] = target[slot].clone();
                }
                notes
            };
            // the final note set must satisfy the same sanity as a putPattern
            for n in &new_notes {
                if !(n.step.is_finite() && n.step >= 0.0 && n.len.is_finite() && n.len > 0.0) {
                    return Err(OpError::Invalid(format!("pattern {}: a note has an invalid step/len", pid)));
                }
                if !(n.v.is_finite() && (0.0..=1.0).contains(&n.v)) {
                    return Err(OpError::Invalid("note velocity `v` must be 0..1".into()));
                }
                if !(n.pan.is_finite() && (-1.0..=1.0).contains(&n.pan)) {
                    return Err(OpError::Invalid("note pan `pan` must be -1..1".into()));
                }
                if n.p.is_some_and(|k| k > 127) {
                    return Err(OpError::Invalid("note key `p` must be 0..127".into()));
                }
                if (n.step + n.len) as f64 > TOTAL_BEATS * 4.0 {
                    return Err(OpError::Invalid(format!("pattern {}: notes must end within {TOTAL_BEATS} beats", pid)));
                }
                if !chan_ok(n.ch) {
                    return Err(OpError::Missing { what: "channel", id: n.ch });
                }
            }
            let old_len = p.patterns[pi].len;
            p.patterns[pi].notes = new_notes;
            // a generator can push notes past the pattern's end: grow the pattern like
            // putPattern does, so whole-pass clips follow
            p.patterns[pi].fit_len();
            // and a longer pattern (or rewritten channels) re-validates the loops' bounds
            check_channel_loops(p, &p.patterns[pi])?;
            let new_len = p.patterns[pi].len;
            if (old_len - new_len).abs() > 1e-9 {
                follow_pattern_length(p, *pid, old_len, new_len);
                d.timeline = true;
            }
            bump(p, *pid);
            d.timeline = true;
        }
        Op::SetKey { key, scale } => {
            if let Some(k) = key {
                if *k > 11 {
                    return Err(OpError::Invalid("key must be 0..11 (0 = C)".into()));
                }
            }
            if *scale > 4 {
                return Err(OpError::Invalid("scale must be 0..4 (see pitch::scale_mask)".into()));
            }
            p.key = *key;
            p.scale = *scale;
            d.timeline = true;
            d.params = true;
        }
        Op::RemoveAutomation { id } => {
            p.automations.retain(|a| a.id != *id);
            p.clips.retain(|c| !(c.kind == ClipKind::Auto && c.source == *id));
            d.timeline = true;
        }
        Op::PutComp { comp } => {
            let c = validate_comp(p, comp)?;
            bump(p, c.id);
            for t in &c.takes {
                bump(p, t.id);
            }
            match p.comps.iter_mut().find(|x| x.id == c.id) {
                Some(x) => *x = c,
                None => p.comps.push(c),
            }
            d.timeline = true;
        }
        Op::RemoveComp { id } => {
            if p.comp(*id).is_none() {
                return Err(OpError::Missing { what: "comp", id: *id });
            }
            p.comps.retain(|c| c.id != *id);
            p.clips.retain(|c| !(c.kind == ClipKind::Comp && c.source == *id));
            d.timeline = true;
        }
        Op::PutNotebook { notebook } => {
            validate_notebook(notebook)?;
            for pg in &notebook.pages {
                bump(p, pg.id);
            }
            p.notebook = notebook.clone();
        }
        Op::PutMarker { marker } => {
            validate_marker(marker)?;
            match p.markers.iter().position(|m| m.id == marker.id) {
                Some(i) => p.markers[i] = marker.clone(),
                None if p.markers.len() >= MARKERS_MAX => {
                    return Err(OpError::Invalid(format!("a project has at most {MARKERS_MAX} markers")));
                }
                None => p.markers.push(marker.clone()),
            }
            // by position, then by id: two markers at one place keep a stable order
            p.markers.sort_by(|a, b| a.at.total_cmp(&b.at).then(a.id.cmp(&b.id)));
            bump(p, marker.id);
        }
        Op::RemoveMarker { id } => {
            if p.marker(*id).is_none() {
                return Err(OpError::Missing { what: "marker", id: *id });
            }
            p.markers.retain(|m| m.id != *id);
        }
        Op::SetBpm { bpm } => {
            if !(crate::model::MIN_TEMPO_BPM..=crate::model::MAX_BASE_BPM).contains(bpm) {
                return Err(OpError::Invalid("bpm must be 20..999".into()));
            }
            p.bpm = *bpm;
            // the knob edits the constant case: a tempo map would be overwritten by it
            p.tempo_points.clear();
            d.timeline = true;
        }
        Op::SetTempoPoints { points } => {
            if points.len() > crate::model::MAX_TEMPO_POINTS {
                return Err(OpError::Invalid(format!("at most {} tempo points", crate::model::MAX_TEMPO_POINTS)));
            }
            let mut pts: Vec<crate::model::TempoPoint> = points
                .iter()
                .map(|p| crate::model::TempoPoint {
                    beat: p.beat.clamp(0.0, TOTAL_BEATS),
                    bpm: p.bpm.clamp(crate::model::MIN_TEMPO_BPM, crate::model::MAX_TEMPO_BPM),
                    ramp: p.ramp,
                })
                .collect();
            pts.sort_by(|a, b| a.beat.total_cmp(&b.beat));
            p.tempo_points = pts;
            d.timeline = true;
        }
        Op::SetSigPoints { points } => {
            if points.len() > crate::model::MAX_SIG_POINTS {
                return Err(OpError::Invalid(format!("at most {} signature points", crate::model::MAX_SIG_POINTS)));
            }
            let mut pts: Vec<crate::model::SigPoint> = points
                .iter()
                .map(|p| crate::model::SigPoint {
                    beat: p.beat.clamp(0.0, TOTAL_BEATS),
                    num: p.num.clamp(1, crate::model::MAX_SIG_NUM),
                    den: if crate::model::SIG_DENS.contains(&p.den) { p.den } else { 4 },
                })
                .collect();
            pts.sort_by(|a, b| a.beat.total_cmp(&b.beat));
            p.sig_points = pts;
            // a pattern keeps the bar length it was created with: the map reflows where the
            // bars fall (the ruler, the accents, the snap) but never resizes a pattern
            d.timeline = true;
        }
        Op::SetMaskIgnore { pairs } => {
            let mut out: Vec<[Id; 2]> = vec![];
            for q in pairs {
                for id in q {
                    if *id == 0 || p.insert(*id).is_none() {
                        return Err(OpError::Invalid(format!("mask ignore: insert {id} is not a mixer insert")));
                    }
                }
                if q[0] == q[1] {
                    return Err(OpError::Invalid("mask ignore: a pair needs two different inserts".into()));
                }
                let q = [q[0].min(q[1]), q[0].max(q[1])];
                if !out.contains(&q) {
                    out.push(q);
                }
            }
            p.mask_ignore = out;
        }
        Op::SetLoop { range } => {
            if let Some(r) = range {
                if !(r.b > r.a && r.a >= 0.0) {
                    return Err(OpError::Invalid("loop end must be after its start".into()));
                }
            }
            p.loop_range = *range;
            d.timeline = true;
        }
    }
    Ok(d)
}

/// A pattern's length went from `old` to `new` beats; its clips follow without ever
/// overlapping another clip or showing more than the pattern holds:
///
/// * grown: clips that show exactly one whole pass (offset 0, length = old length) take the
///   new length, but stop at the next clip on their track (never shorter than they were), so
///   a longer pattern does not silently run over the arrangement. Longer, shorter or offset
///   clips keep their length (they were sized on purpose).
/// * shrunk: clips whose window lay within the old pattern and now reaches past its end are
///   cut back to it, so they do not start looping the pattern. Clips that already reached
///   past the old end (loops from older projects) keep theirs: nothing is removed silently.
///
/// Clips never run past the playlist end. The same rule is `followPatternLength` in
/// `ui/src/lib/pattern.ts`.
fn follow_pattern_length(p: &mut Project, id: Id, old: f64, new: f64) {
    const EPS: f64 = 1e-6;
    for i in 0..p.clips.len() {
        let c = &p.clips[i];
        if c.kind != ClipKind::Pat || c.source != id {
            continue;
        }
        let len = if new > old {
            if c.offset.abs() >= 1e-9 || (c.len - old).abs() >= EPS {
                continue;
            }
            // the next clip on the lane (any kind): growing stops at its start
            let next = p
                .clips
                .iter()
                .filter(|o| o.id != c.id && o.track == c.track && o.start > c.start + EPS)
                .map(|o| o.start)
                .fold(f64::INFINITY, f64::min);
            new.min(next - c.start).max(c.len)
        } else {
            let end = c.offset + c.len;
            if end > old + EPS || end <= new + EPS || c.offset >= new - EPS {
                continue;
            }
            new - c.offset
        };
        p.clips[i].len = len.min(TOTAL_BEATS - p.clips[i].start).max(1.0 / 16.0);
    }
}

/// Checks a marker: a position on the playlist, a bounded name.
pub fn validate_marker(m: &Marker) -> Result<(), OpError> {
    if !(m.at.is_finite() && (0.0..=TOTAL_BEATS).contains(&m.at)) {
        return Err(OpError::Invalid(format!("marker {}: position must be 0..{TOTAL_BEATS} beats", m.id)));
    }
    if m.name.chars().count() > MARKER_NAME_MAX {
        return Err(OpError::Invalid(format!("marker {}: name is longer than {MARKER_NAME_MAX} characters", m.id)));
    }
    Ok(())
}

/// Checks a notebook: unique page ids, bars 1..=NOTEBOOK_MAX_BAR, bounded size.
pub fn validate_notebook(nb: &Notebook) -> Result<(), OpError> {
    let bad = |m: String| Err(OpError::Invalid(m));
    if nb.pages.len() > NOTEBOOK_MAX_PAGES {
        return bad(format!("a notebook has at most {NOTEBOOK_MAX_PAGES} pages"));
    }
    let mut ids = std::collections::HashSet::new();
    for pg in &nb.pages {
        if !ids.insert(pg.id) {
            return bad(format!("notebook page id {} is used twice", pg.id));
        }
        if !(1..=NOTEBOOK_MAX_BAR).contains(&pg.start_bar) {
            return bad(format!("page {}: start bar must be 1..{NOTEBOOK_MAX_BAR}", pg.id));
        }
        if pg.text.len() > NOTEBOOK_MAX_TEXT {
            return bad(format!("page {}: text is longer than {NOTEBOOK_MAX_TEXT} bytes", pg.id));
        }
        if pg.name.chars().count() > 80 {
            return bad(format!("page {}: name is longer than 80 characters", pg.id));
        }
    }
    Ok(())
}

/// Checks a comp and returns it normalised (regions sorted, same-take neighbours joined).
pub fn validate_comp(p: &Project, comp: &Comp) -> Result<Comp, OpError> {
    let bad = |m: String| Err(OpError::Invalid(m));
    if comp.takes.is_empty() {
        return bad(format!("comp {} has no takes", comp.id));
    }
    if !(comp.xfade.is_finite() && (0.0..=MAX_XFADE).contains(&comp.xfade)) {
        return bad(format!("crossfade must be 0..{} s", MAX_XFADE));
    }
    for (i, t) in comp.takes.iter().enumerate() {
        if !(t.start.is_finite() && t.len.is_finite() && t.offset.is_finite()) || t.len <= 0.0 || t.offset < 0.0 {
            return bad(format!("take {} has an invalid range", t.id));
        }
        if comp.takes[..i].iter().any(|x| x.id == t.id) {
            return bad(format!("take id {} is used twice", t.id));
        }
        if !p.sources.iter().any(|s| s.id == t.source) {
            return Err(OpError::Missing { what: "take source", id: t.source });
        }
    }
    let mut regions = comp.regions.clone();
    for r in &regions {
        if !(r.from.is_finite() && r.to.is_finite()) || r.to <= r.from {
            return bad(format!("comp region {}..{} is empty or invalid", r.from, r.to));
        }
        if comp.take(r.take).is_none() {
            return Err(OpError::Missing { what: "take", id: r.take });
        }
        if let Some(x) = r.xfade {
            if !(x.is_finite() && (0.0..=MAX_XFADE).contains(&x)) {
                return bad(format!("crossfade must be 0..{} s", MAX_XFADE));
            }
        }
    }
    regions.sort_by(|a, b| a.from.total_cmp(&b.from));
    if regions.windows(2).any(|w| w[1].from < w[0].to - 1e-9) {
        return bad("comp regions overlap".into());
    }
    Ok(Comp { regions: merge_regions(regions), ..comp.clone() })
}

/// A sidechain (`Fx::key`): only on an effect that takes one ([`FxKind::takes_key`]), from
/// another non-master insert that exists and that does not already wait for this insert
/// (through sends or other sidechains): the key is processed before the listening insert.
fn validate_key(p: &Project, insert_id: Id, fx: &Fx) -> Result<(), OpError> {
    let Some(k) = fx.key else { return Ok(()) };
    if !fx.kind.takes_key() {
        return Err(OpError::Invalid("only a Compressor, Gate, Filter or EQ takes a sidechain (`key`)".into()));
    }
    if k == 0 {
        return Err(OpError::Invalid("the master cannot be a sidechain: it is processed last".into()));
    }
    if k == insert_id {
        return Err(OpError::Invalid("a sidechain is another insert (an effect hears its own insert without one)".into()));
    }
    if p.insert(k).is_none() {
        return Err(OpError::Missing { what: "insert", id: k });
    }
    if crate::routing::depends(p, insert_id, k) {
        return Err(OpError::Invalid(format!(
            "insert {k} already takes insert {insert_id}'s sound: a sidechain from it would close a loop"
        )));
    }
    Ok(())
}

fn validate_fx(fx: &Fx) -> Result<(), OpError> {
    if fx.kind == FxKind::Eq {
        match &fx.bands {
            Some(b) if b.len() <= MAX_BANDS => {}
            _ => return Err(OpError::Invalid(format!("an EQ has 0..{MAX_BANDS} bands"))),
        }
    }
    if fx.kind == FxKind::Plugin && fx.plugin.is_none() {
        return Err(OpError::Invalid("a plugin slot needs `plugin`".into()));
    }
    match (&fx.maps, fx.kind == FxKind::Warp) {
        (None, true) => return Err(OpError::Invalid("a Warp needs `maps` (see fx_catalog().warp for the factory slots)".into())),
        (Some(_), false) => return Err(OpError::Invalid("only a Warp has `maps`".into())),
        (Some(m), true) => validate_maps(m)?,
        (None, false) => {}
    }
    if fx.listen.is_some() && fx.kind != FxKind::Master {
        return Err(OpError::Invalid("only a Master has `listen`".into()));
    }
    for (k, v) in &fx.params {
        if !v.is_finite() {
            return Err(OpError::Invalid(format!("parameter {k} is not a number")));
        }
    }
    Ok(())
}

fn validate_maps(m: &crate::model::WarpMaps) -> Result<(), OpError> {
    use crate::model::{WARP_BAR, WARP_POINTS, WARP_SLOTS};
    for (bank, slots) in [("time", &m.time), ("vol", &m.vol)] {
        if slots.len() > WARP_SLOTS {
            return Err(OpError::Invalid(format!("Warp {bank} maps: at most {WARP_SLOTS} slots")));
        }
        for (i, s) in slots.iter().enumerate() {
            let bad = |why: &str| Err(OpError::Invalid(format!("Warp {bank} slot {}: {why}", i + 1)));
            if s.points.len() > WARP_POINTS {
                return bad(&format!("at most {WARP_POINTS} points"));
            }
            if s.points
                .iter()
                .any(|p| !(0.0..=WARP_BAR).contains(&p.t) || !(0.0..=1.0).contains(&p.v) || !p.tension.is_finite() || p.tension.abs() > 1.0)
            {
                return bad("points need t 0..4 beats, v 0..1, tension -1..1");
            }
            if s.points.windows(2).any(|w| w[1].t < w[0].t) {
                return bad("points must be in time order");
            }
        }
    }
    Ok(())
}

/// Applies a batch atomically: all ops or none.
pub fn apply_all(p: &mut Project, ops: &[Op]) -> Result<Dirty, OpError> {
    let mut work = p.clone();
    let mut d = Dirty::default();
    for op in ops {
        d.merge(apply(&mut work, op)?);
    }
    *p = work;
    Ok(d)
}

#[cfg(test)]
mod transform_tests {
    use super::*;

    fn project_with_notes() -> Project {
        let mut p = crate::demo::routed_project();
        p.patterns[0].notes = vec![
            Note { ch: 1001, step: 0.0, len: 1.0, p: Some(61), v: 0.8, trig: Some(false), pan: 0.0, slide: false },
            Note { ch: 1001, step: 4.3, len: 1.0, p: Some(64), v: 0.8, trig: Some(false), pan: 0.0, slide: false },
            Note { ch: 1001, step: 8.0, len: 1.0, p: Some(60), v: 0.8, trig: Some(false), pan: 0.0, slide: false },
        ];
        p
    }

    #[test]
    fn transform_notes_is_deterministic_in_the_seed() {
        let notes_at = |seed: u64| {
            let mut p = project_with_notes();
            crate::ops::apply_all(
                &mut p,
                &[Op::TransformNotes {
                    pattern: 1,
                    verb: crate::transform::TransformVerb::Humanize { timing: 0.5, velocity: 0.4 },
                    seed: Some(seed),
                    select: None,
                }],
            )
            .unwrap();
            p.patterns[0].notes.clone()
        };
        assert_eq!(notes_at(41), notes_at(41));
        assert_ne!(notes_at(41), notes_at(42));
    }

    #[test]
    fn quantize_with_scale_aware_uses_the_project_key() {
        let mut p = project_with_notes();
        crate::ops::apply_all(&mut p, &[Op::SetKey { key: Some(0), scale: 1 }]).unwrap();
        crate::ops::apply_all(
            &mut p,
            &[Op::TransformNotes {
                pattern: 1,
                verb: crate::transform::TransformVerb::Quantize { grid: 1.0, amount: 1.0, scale_aware: true },
                seed: None,
                select: None,
            }],
        )
        .unwrap();
        // the C# snaps to C, the E and C are already in scale
        let keys: Vec<u8> = p.patterns[0].notes.iter().map(|n| n.p.unwrap()).collect();
        assert_eq!(keys, vec![60, 64, 60]);
    }

    #[test]
    fn a_generator_replaces_its_whole_channel_whatever_the_selection() {
        let mut p = project_with_notes();
        p.channels.push(crate::model::Channel { id: 1002, ..p.channels[0].clone() });
        p.patterns[0].notes[1].ch = 1002; // a note on another channel must survive
        crate::ops::apply_all(
            &mut p,
            &[Op::TransformNotes {
                pattern: 1,
                verb: crate::transform::TransformVerb::EuclidFill { ch: 1001, k: 3, n: 8, rotate: 0, vel: 0.7 },
                seed: None,
                // only the first ch-1001 note is selected: the unselected one at step 8
                // must not stay under the generated onsets
                select: Some(crate::ops::TransformSelect::Indices { indices: vec![0] }),
            }],
        )
        .unwrap();
        let notes = &p.patterns[0].notes;
        assert_eq!(notes.iter().filter(|n| n.ch == 1001).count(), 3);
        assert!(notes.iter().filter(|n| n.ch == 1001).all(|n| n.p.is_none()), "only generated onsets on ch 1001");
        assert_eq!(notes.iter().filter(|n| n.ch == 1002).count(), 1);
        assert_eq!(notes.len(), 4);
        // the same with a melody: selected or not, the result equals the unscoped run
        let melody = |select: Option<crate::ops::TransformSelect>| {
            let mut q = project_with_notes();
            crate::ops::apply_all(
                &mut q,
                &[Op::TransformNotes {
                    pattern: 1,
                    verb: crate::transform::TransformVerb::Melody { ch: 1001, bars: 1, density: 0.5, repeat: 1 },
                    seed: Some(3),
                    select,
                }],
            )
            .unwrap();
            q.patterns[0].notes.clone()
        };
        assert_eq!(melody(Some(crate::ops::TransformSelect::Indices { indices: vec![1] })), melody(None));
        // a bad index is still an error for a generator
        let err = crate::ops::apply_all(
            &mut project_with_notes(),
            &[Op::TransformNotes {
                pattern: 1,
                verb: crate::transform::TransformVerb::EuclidFill { ch: 1001, k: 3, n: 8, rotate: 0, vel: 0.7 },
                seed: None,
                select: Some(crate::ops::TransformSelect::Indices { indices: vec![9] }),
            }],
        )
        .unwrap_err();
        assert!(err.to_string().contains("out of range"), "{err}");
    }

    #[test]
    fn applying_a_melody_twice_equals_applying_it_once() {
        let run = |p: &mut Project, seed: u64| {
            crate::ops::apply_all(
                p,
                &[Op::TransformNotes {
                    pattern: 1,
                    verb: crate::transform::TransformVerb::Melody { ch: 1001, bars: 1, density: 0.5, repeat: 1 },
                    seed: Some(seed),
                    select: None,
                }],
            )
            .unwrap();
        };
        let (mut once, mut twice) = (project_with_notes(), project_with_notes());
        run(&mut once, 2);
        run(&mut twice, 1);
        run(&mut twice, 2);
        assert_eq!(once.patterns[0].notes, twice.patterns[0].notes);
    }

    #[test]
    fn scale_aware_quantize_keeps_the_top_key_in_range() {
        // C# major pentatonic: 127 (G) is nearest 128 (G#) — out of MIDI range; the
        // nearest in-range in-scale key is 125 (F)
        let mut p = project_with_notes();
        p.patterns[0].notes[2].p = Some(127);
        crate::ops::apply_all(&mut p, &[Op::SetKey { key: Some(1), scale: 3 }]).unwrap();
        crate::ops::apply_all(
            &mut p,
            &[Op::TransformNotes {
                pattern: 1,
                verb: crate::transform::TransformVerb::Quantize { grid: 1.0, amount: 1.0, scale_aware: true },
                seed: None,
                select: None,
            }],
        )
        .unwrap();
        assert_eq!(p.patterns[0].notes[2].p, Some(125));
    }

    /// Golden values for the drum generator's seeded velocity shading; the TS mirror
    /// (`ui/src/lib/transforms.test.ts`, "euclidDrums") pins the same numbers.
    #[test]
    fn golden_euclid_drums_seeded_velocity() {
        let mut p = project_with_notes();
        p.channels.push(crate::model::Channel { id: 1002, ..p.channels[0].clone() });
        crate::ops::apply_all(
            &mut p,
            &[Op::TransformNotes {
                pattern: 1,
                verb: crate::transform::TransformVerb::EuclidDrums {
                    voices: vec![
                        crate::transform::DrumVoice { ch: 1001, k: 4, n: 16, rotate: 0, vel: 0.7 },
                        crate::transform::DrumVoice { ch: 1002, k: 3, n: 8, rotate: 1, vel: 0.5 },
                    ],
                    bars: 1,
                },
                seed: Some(42),
                select: None,
            }],
        )
        .unwrap();
        let notes = &p.patterns[0].notes;
        let of = |ch: Id| notes.iter().filter(|n| n.ch == ch).map(|n| (n.step, n.v)).collect::<Vec<_>>();
        // every fourth onset accents to 1.0; the rest are shaded by the seed
        assert_eq!(of(1001), [(0.0, 1.0), (4.0, 0.62858117), (8.0, 0.62858117), (12.0, 0.62858117)]);
        assert_eq!(of(1002), [(4.0, 1.0), (10.0, 0.46679017), (14.0, 0.46679017)]);
    }

    #[test]
    fn generator_sizes_are_bounded_before_allocating() {
        let mut p = project_with_notes();
        let err = crate::ops::apply_all(
            &mut p,
            &[Op::TransformNotes {
                pattern: 1,
                verb: crate::transform::TransformVerb::EuclidFill { ch: 1001, k: 1, n: 4_000_000_000, rotate: 0, vel: 0.7 },
                seed: None,
                select: None,
            }],
        )
        .unwrap_err();
        assert!(err.to_string().contains("1..=512"), "{err}");
        // and the pattern is untouched by the rejected op
        assert_eq!(p.patterns[0].notes.len(), 3);
    }

    #[test]
    fn a_melody_past_the_pattern_end_grows_the_pattern_and_whole_pass_clips() {
        let mut p = project_with_notes();
        crate::ops::apply_all(&mut p, &[Op::SetKey { key: Some(9), scale: 2 }]).unwrap();
        crate::ops::apply_all(
            &mut p,
            &[Op::TransformNotes {
                pattern: 1,
                verb: crate::transform::TransformVerb::Melody { ch: 1001, bars: 2, density: 0.55, repeat: 1 },
                seed: Some(7),
                select: None,
            }],
        )
        .unwrap();
        // the melody's last note ends at step 33 = 8.25 beats: content length ceils to
        // whole bars, so the 1-bar pattern grew to 3 bars
        assert_eq!(p.patterns[0].len, 12.0);
    }

    #[test]
    fn quantize_without_a_key_is_pitch_neutral() {
        let mut p = project_with_notes();
        crate::ops::apply_all(
            &mut p,
            &[Op::TransformNotes {
                pattern: 1,
                verb: crate::transform::TransformVerb::Quantize { grid: 1.0, amount: 1.0, scale_aware: true },
                seed: None,
                select: None,
            }],
        )
        .unwrap();
        // no project key: pitches pass through unchanged
        let keys: Vec<u8> = p.patterns[0].notes.iter().map(|n| n.p.unwrap()).collect();
        assert_eq!(keys, vec![61, 64, 60]);
    }

    #[test]
    fn select_range_touches_only_notes_in_range() {
        let mut p = project_with_notes();
        crate::ops::apply_all(
            &mut p,
            &[Op::TransformNotes {
                pattern: 1,
                verb: crate::transform::TransformVerb::Quantize { grid: 1.0, amount: 1.0, scale_aware: false },
                seed: None,
                select: Some(crate::ops::TransformSelect::Range { from: 4.0, to: 8.0, ch: None }),
            }],
        )
        .unwrap();
        let notes = &p.patterns[0].notes;
        assert_eq!(notes[0].step, 0.0); // outside the range: untouched
        assert_eq!(notes[1].step, 4.0); // 4.3 snapped into the range
        assert_eq!(notes[2].step, 8.0); // range end is exclusive
    }

    #[test]
    fn unknown_channel_is_an_error_not_a_partial_apply() {
        let mut p = project_with_notes();
        let err = crate::ops::apply_all(
            &mut p,
            &[Op::TransformNotes {
                pattern: 1,
                verb: crate::transform::TransformVerb::EuclidFill { ch: 9999, k: 4, n: 16, rotate: 0, vel: 0.7 },
                seed: None,
                select: None,
            }],
        )
        .unwrap_err();
        assert!(matches!(err, crate::ops::OpError::Missing { what: "channel", id: 9999 }));
        assert_eq!(p.patterns[0].notes.len(), 3); // untouched
    }

    #[test]
    fn set_key_validates_and_clears() {
        let mut p = project_with_notes();
        crate::ops::apply_all(&mut p, &[Op::SetKey { key: Some(12), scale: 1 }]).unwrap_err();
        crate::ops::apply_all(&mut p, &[Op::SetKey { key: Some(9), scale: 2 }]).unwrap();
        assert_eq!(p.key, Some(9));
        assert_eq!(p.scale, 2);
        crate::ops::apply_all(&mut p, &[Op::SetKey { key: None, scale: 1 }]).unwrap();
        assert_eq!(p.key, None);
    }

    #[test]
    fn euclid_drums_fill_only_their_channels() {
        let mut p = project_with_notes();
        crate::ops::apply_all(
            &mut p,
            &[Op::TransformNotes {
                pattern: 1,
                verb: crate::transform::TransformVerb::EuclidDrums {
                    voices: vec![crate::transform::DrumVoice { ch: 1001, k: 4, n: 16, rotate: 0, vel: 0.7 }],
                    bars: 1,
                },
                seed: None,
                select: None,
            }],
        )
        .unwrap();
        let notes = &p.patterns[0].notes;
        assert_eq!(notes.len(), 4);
        assert!(notes.iter().all(|n| n.ch == 1001));
        assert_eq!(notes[0].step, 0.0);
    }

    // --- chord tools -------------------------------------------------------

    fn set_key_a_minor(p: &mut Project) {
        crate::ops::apply_all(p, &[Op::SetKey { key: Some(9), scale: 2 }]).unwrap();
    }

    #[test]
    fn chord_verb_is_channel_pinned_and_merges() {
        let mut p = project_with_notes();
        set_key_a_minor(&mut p);
        // notes at 0.0 (C#4), 4.3 (E4), 8.0 (C4) on ch 1001; a hit on another channel at 5.0
        let other = p.channels.iter().map(|c| c.id).find(|id| *id != 1001).unwrap();
        p.patterns[0].notes.push(Note { ch: other, step: 5.0, len: 1.0, p: None, v: 0.8, trig: Some(false), pan: 0.0, slide: false });
        // range select pinned to ch 1001, steps 4..9: E4 → Em, C4 → C; the C# (out of
        // scale) is outside the range and the drum hit is on another channel — both stay
        crate::ops::apply_all(
            &mut p,
            &[Op::TransformNotes {
                pattern: 1,
                verb: crate::transform::TransformVerb::Chord { harmony: "diatonic".into() },
                seed: None,
                select: Some(crate::ops::TransformSelect::Range { from: 4.0, to: 9.0, ch: Some(1001) }),
            }],
        )
        .unwrap();
        let notes = &p.patterns[0].notes;
        assert_eq!(notes.len(), 8); // 4 originals + 2 + 2 added tones
        let step_ps = |s: f32| -> Vec<u8> {
            let mut v: Vec<u8> = notes.iter().filter(|n| n.step == s).map(|n| n.p.unwrap()).collect();
            v.sort_unstable();
            v
        };
        assert_eq!(step_ps(0.0), [61]); // out of the range: untouched
        assert_eq!(step_ps(4.3), [64, 67, 71]); // Em above E4
        assert_eq!(notes.iter().filter(|n| n.step == 5.0).count(), 1); // the other-channel hit
        assert_eq!(notes.iter().find(|n| n.step == 5.0).unwrap().p, None); // no tones added on it
        assert_eq!(step_ps(8.0), [60, 64, 67]); // C above C4
    }

    #[test]
    fn chord_verb_dedupe_sees_unselected_notes() {
        let mut p = project_with_notes();
        set_key_a_minor(&mut p);
        // E3 selected on ch 1001; B3 (explicit) on another channel at the same step,
        // OUTSIDE the selection — Em must not double the B that is already sounding
        let other = p.channels.iter().map(|c| c.id).find(|id| *id != 1001).unwrap();
        p.patterns[0].notes[1].p = Some(52); // the 4.3 note becomes E3
        p.patterns[0].notes.push(Note { ch: other, step: 4.3, len: 2.0, p: Some(59), v: 0.8, trig: Some(false), pan: 0.0, slide: false });
        crate::ops::apply_all(
            &mut p,
            &[Op::TransformNotes {
                pattern: 1,
                verb: crate::transform::TransformVerb::Chord { harmony: "diatonic".into() },
                seed: None,
                select: Some(crate::ops::TransformSelect::Range { from: 4.0, to: 9.0, ch: Some(1001) }),
            }],
        )
        .unwrap();
        let notes = &p.patterns[0].notes;
        assert_eq!(notes.len(), 7); // 4 originals + G3 (E3), + E4 and G4 (C4) — no second B3
        assert_eq!(notes.iter().filter(|n| n.p == Some(59)).count(), 1);
    }

    #[test]
    fn chord_verb_diatonic_needs_key_and_scale() {
        let mut p = project_with_notes();
        let err = crate::ops::apply_all(
            &mut p,
            &[Op::TransformNotes {
                pattern: 1,
                verb: crate::transform::TransformVerb::Chord { harmony: "diatonic".into() },
                seed: None,
                select: None,
            }],
        )
        .unwrap_err();
        assert!(matches!(err, crate::ops::OpError::Invalid(_)));
        // named types work without a key: interval spelling anywhere
        crate::ops::apply_all(
            &mut p,
            &[Op::TransformNotes {
                pattern: 1,
                verb: crate::transform::TransformVerb::Chord { harmony: "min".into() },
                seed: None,
                select: Some(crate::ops::TransformSelect::Range { from: 0.0, to: 1.0, ch: None }),
            }],
        )
        .unwrap();
        let mut ps: Vec<u8> = p.patterns[0].notes.iter().filter(|n| n.step == 0.0).map(|n| n.p.unwrap()).collect();
        ps.sort_unstable();
        assert_eq!(ps, [61, 64, 68]); // C#m above the first note
    }

    #[test]
    fn chord_verb_rejects_unknown_harmony() {
        let mut p = project_with_notes();
        set_key_a_minor(&mut p);
        let err = crate::ops::apply_all(
            &mut p,
            &[Op::TransformNotes {
                pattern: 1,
                verb: crate::transform::TransformVerb::Chord { harmony: "m7b5".into() },
                seed: None,
                select: None,
            }],
        )
        .unwrap_err();
        assert!(matches!(err, crate::ops::OpError::Invalid(_)));
    }

    #[test]
    fn progression_verb_generates_and_grows_the_pattern() {
        let mut p = project_with_notes();
        set_key_a_minor(&mut p);
        crate::ops::apply_all(
            &mut p,
            &[Op::TransformNotes {
                pattern: 1,
                verb: crate::transform::TransformVerb::Progression {
                    ch: 1001,
                    bars: 4,
                    template: "minloop".into(),
                    voice_lead: true,
                    degrees: vec![],
                },
                seed: None,
                select: None,
            }],
        )
        .unwrap();
        let notes = &p.patterns[0].notes;
        // the generator replaced the pattern's notes and grew it to 4 bars
        assert_eq!(notes.len(), 16);
        assert_eq!(p.patterns[0].len, 16.0);
        let ps: Vec<u8> = notes.iter().map(|n| n.p.unwrap()).collect();
        assert_eq!(ps, [57, 60, 64, 67, 57, 60, 64, 65, 59, 60, 64, 67, 59, 62, 65, 67]);
    }

    #[test]
    fn progression_verb_validates_bars_templates_and_degrees() {
        let mut p = project_with_notes();
        set_key_a_minor(&mut p);
        let run = |p: &mut Project, verb| crate::ops::apply_all(p, &[Op::TransformNotes { pattern: 1, verb, seed: None, select: None }]);
        let prog = |bars: u32, template: &str, degrees: Vec<u8>| crate::transform::TransformVerb::Progression {
            ch: 1001,
            bars,
            template: template.into(),
            voice_lead: false,
            degrees,
        };
        assert!(matches!(run(&mut p, prog(33, "pop", vec![])), Err(crate::ops::OpError::Invalid(_))));
        assert!(matches!(run(&mut p, prog(4, "nope", vec![])), Err(crate::ops::OpError::Invalid(_))));
        assert!(matches!(run(&mut p, prog(4, "pop", vec![33])), Err(crate::ops::OpError::Invalid(_))));
        assert!(matches!(run(&mut p, prog(4, "pop", (0..33).collect())), Err(crate::ops::OpError::Invalid(_))));
        assert_eq!(p.patterns[0].notes.len(), 3); // nothing applied
                                                  // the legal degree list works and replaces bars
        run(&mut p, prog(4, "pop", vec![0, 5, 2, 6])).unwrap();
        assert_eq!(p.patterns[0].notes.len(), 16);
    }

    #[test]
    fn progression_verb_needs_a_key() {
        let mut p = project_with_notes();
        let err = crate::ops::apply_all(
            &mut p,
            &[Op::TransformNotes {
                pattern: 1,
                verb: crate::transform::TransformVerb::Progression {
                    ch: 1001,
                    bars: 4,
                    template: "pop".into(),
                    voice_lead: false,
                    degrees: vec![],
                },
                seed: None,
                select: None,
            }],
        )
        .unwrap_err();
        assert!(matches!(err, crate::ops::OpError::Invalid(_)));
    }

    #[test]
    fn progression_chance_is_deterministic_in_the_seed() {
        let mut a = project_with_notes();
        let mut b = project_with_notes();
        set_key_a_minor(&mut a);
        set_key_a_minor(&mut b);
        let verb = crate::transform::TransformVerb::Progression {
            ch: 1001,
            bars: 4,
            template: "chance".into(),
            voice_lead: true,
            degrees: vec![],
        };
        let op = |seed| Op::TransformNotes { pattern: 1, verb: verb.clone(), seed, select: None };
        crate::ops::apply_all(&mut a, &[op(Some(41))]).unwrap();
        crate::ops::apply_all(&mut b, &[op(Some(41))]).unwrap();
        assert_eq!(a.patterns[0].notes, b.patterns[0].notes);
        crate::ops::apply_all(&mut a, &[op(Some(42))]).unwrap();
        assert_ne!(a.patterns[0].notes, b.patterns[0].notes);
    }
}
