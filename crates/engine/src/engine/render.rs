//! Offline rendering: plain renders, stems, bounce, export in place and stem
//! extraction, plus pattern- and stem-scoped project projections.

use super::*;

impl Engine {
    /// Beat range the UI and CLI use for a song or loop bounce.
    ///
    /// `song` is always `0..song_end`. `loop` uses the project's loop when one
    /// exists; otherwise it falls back to the song so a missing loop never yields
    /// an empty range.
    pub fn export_range(project: &Project, use_loop: bool) -> (f64, f64) {
        if use_loop {
            if let Some(l) = project.loop_range {
                if l.b > l.a {
                    return (l.a, l.b);
                }
            }
        }
        (0.0, project.song_end())
    }

    /// Renders `[from, to)` beats (plus `tail` seconds) of the current project to
    /// interleaved stereo at `sample_rate`, independently of the live device.
    pub fn render(&self, from: f64, to: f64, tail: f64, sample_rate: u32) -> Result<Vec<f32>, EngineError> {
        self.render_cancelable(from, to, tail, sample_rate, &AtomicBool::new(false))
    }

    /// Same as [`Self::render`], but returns [`EngineError::Cancelled`] as soon as
    /// `cancel` is set. Checked between DSP blocks so a long bounce can stop without
    /// waiting for the whole file.
    pub fn render_cancelable(&self, from: f64, to: f64, tail: f64, sample_rate: u32, cancel: &AtomicBool) -> Result<Vec<f32>, EngineError> {
        self.render_export(None, from, to, RenderTail::Seconds(tail), sample_rate, cancel)
    }

    /// The mixer inserts a stem export writes a file for, in mixer order: every insert except the
    /// master that is not muted and has something routed into it (a track with an unmuted clip, or a
    /// channel sent to it). Like FL Studio's *Split mixer tracks*, unused and muted inserts are left
    /// out; a candidate that still renders silent is dropped by the caller.
    pub fn stem_inserts(project: &Project) -> Vec<(Id, String)> {
        let playing: Vec<Id> = project
            .inserts
            .iter()
            .filter(|m| m.id != 0 && !m.muted)
            .filter(|m| {
                let on_track = project.tracks.iter().enumerate().any(|(i, t)| {
                    !t.muted && project.clips.iter().any(|c| c.track as usize == i && !c.muted && project.clip_insert(c) == m.id)
                });
                let on_channel = project.channels.iter().any(|c| c.insert == Some(m.id) && !c.mute);
                on_track || on_channel
            })
            .map(|m| m.id)
            .collect();
        // a bus has nothing of its own: it has a stem when something playing is routed into it
        project
            .inserts
            .iter()
            .filter(|m| m.id != 0 && !m.muted && playing.iter().any(|&src| crate::routing::reaches(project, src, m.id)))
            .map(|m| (m.id, m.name.clone()))
            .collect()
    }

    /// Renders insert `insert` alone as a stem: what it plays after its own effects, fader, pan and
    /// width, before the master (the master's effects, fader, pan and width are skipped). Same range,
    /// tail, sample rate and delay compensation as [`Self::render`], so every stem lines up with the
    /// others and with the mix. Mute is respected (a muted track or channel stays silent); solo is
    /// ignored, so a soloed insert does not silence the other stems.
    pub fn render_stem(
        &self,
        insert: Id,
        from: f64,
        to: f64,
        tail: f64,
        sample_rate: u32,
        cancel: &AtomicBool,
    ) -> Result<Vec<f32>, EngineError> {
        self.render_export(Some(insert), from, to, RenderTail::Seconds(tail), sample_rate, cancel)
    }

    /// Render a mix or stem, optionally keeping the effect state from a previous loop pass.
    pub fn render_export(
        &self,
        stem: Option<Id>,
        from: f64,
        to: f64,
        tail: RenderTail,
        sample_rate: u32,
        cancel: &AtomicBool,
    ) -> Result<Vec<f32>, EngineError> {
        self.render_export_with(stem, from, to, tail, sample_rate, cancel, &mut |_| {})
    }

    /// [`Self::render_export`] that reports its progress to `progress` after every block.
    #[allow(clippy::too_many_arguments)]
    pub fn render_export_with(
        &self,
        stem: Option<Id>,
        from: f64,
        to: f64,
        tail: RenderTail,
        sample_rate: u32,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(RenderProgress),
    ) -> Result<Vec<f32>, EngineError> {
        self.render_bounce(stem, None, from, to, tail, sample_rate, cancel, progress)
    }

    /// [`Self::render_export_with`] of one pattern alone (`pattern`: FL Studio's pattern export,
    /// what PAT mode plays) or of the song (`None`). See [`pattern_project`] for what a pattern
    /// bounce hears; [`Self::pattern_range`] is its range. The project itself is not changed.
    #[allow(clippy::too_many_arguments)]
    pub fn render_bounce(
        &self,
        stem: Option<Id>,
        pattern: Option<Id>,
        from: f64,
        to: f64,
        tail: RenderTail,
        sample_rate: u32,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(RenderProgress),
    ) -> Result<Vec<f32>, EngineError> {
        if let Some(id) = pattern {
            if !self.core.lock().project.patterns.iter().any(|p| p.id == id) {
                return Err(EngineError::Msg(format!("no pattern {id}")));
            }
        }
        let scope = move |p: Project| {
            let p = match pattern {
                Some(id) => pattern_project(p, id),
                None => p,
            };
            match stem {
                Some(id) => stem_project(p, id),
                None => p,
            }
        };
        self.render_scoped(&scope, from, to, tail, sample_rate, cancel, progress)
    }

    /// Beat range of a pattern export: the whole pattern from its first beat (`None`: no such pattern).
    pub fn pattern_range(project: &Project, pattern: Id) -> Option<(f64, f64)> {
        project.patterns.iter().find(|p| p.id == pattern).map(|p| (0.0, p.len.max(BAR_BEATS)))
    }

    /// Renders what `scope` makes of the current project (a stem, the selected clips alone …).
    #[allow(clippy::too_many_arguments)]
    fn render_scoped(
        &self,
        scope: &dyn Fn(Project) -> Project,
        from: f64,
        to: f64,
        tail: RenderTail,
        sample_rate: u32,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(RenderProgress),
    ) -> Result<Vec<f32>, EngineError> {
        if !from.is_finite() || !to.is_finite() || to <= from {
            return Err(EngineError::Msg("render range must be finite and `to` must be after `from`".into()));
        }
        if sample_rate < 8000 {
            return Err(EngineError::Msg("sample rate must be at least 8000 Hz".into()));
        }
        if cancel.load(Relaxed) {
            return Err(EngineError::Cancelled);
        }
        let (project, mut store_snapshot, stretches, takes, keys) = {
            let mut guard = self.core.lock();
            let c = &mut *guard;
            self.snapshot_plugin_states(&mut c.project);
            let mut map = HashMap::new();
            let mut stretches = vec![];
            let bpm = c.project.bpm;
            for clip in &c.project.clips {
                if clip.kind == ClipKind::Audio {
                    match c.sources.stretch_request(clip, false, bpm) {
                        Some(Ok(a)) => {
                            map.insert(clip.id, a);
                        }
                        Some(Err(job)) => stretches.push((clip.id, job)),
                        None => {}
                    }
                }
            }
            let mut audio = comp_audio(&c.sources, &c.project.comps);
            audio.extend(sample_audio(&c.sources, &c.project.channels));
            (c.project.clone(), map, stretches, audio, c.keys.clone())
        };
        let project = scope(project);
        // time-stretches nobody played yet are rendered here, without holding the project
        let mut rendered: HashMap<crate::sources::StretchKey, (Arc<Audio>, f64)> = HashMap::new();
        let stretch_total = stretches.len();
        for (i, (clip_id, job)) in stretches.into_iter().enumerate() {
            progress(RenderProgress::Preparing { done: i, total: stretch_total });
            if cancel.load(Relaxed) {
                return Err(EngineError::Cancelled);
            }
            let got = match rendered.get(&job.key) {
                Some(x) => x.clone(),
                None => {
                    let r = crate::sources::render_stretch(&job);
                    let got = match &r {
                        Ok(a) => (a.clone(), job.rate),
                        Err(_) => (job.audio.clone(), job.fallback_rate),
                    };
                    self.core.lock().sources.put_stretch(job.key, r);
                    rendered.insert(job.key, got.clone());
                    got
                }
            };
            store_snapshot.insert(clip_id, got);
        }
        let sr = sample_rate as f32;
        let block = 512;
        let (mut cmd_tx, cmd_rx) = rtrb::RingBuffer::<Cmd>::new(64 + project.channels.len());
        let (gb_tx, mut gb_rx) = rtrb::RingBuffer::<Garbage>::new(64);
        let (rec_tx, _rec_rx) = rtrb::RingBuffer::<f32>::new(16);
        let mut taps = Taps { input: vec![], output: vec![] };
        let mut keep = vec![];
        for _ in 0..WATCH_SLOTS {
            let (a, b) = rtrb::RingBuffer::<f32>::new(16);
            let (c2, d) = rtrb::RingBuffer::<f32>::new(16);
            taps.input.push(a);
            taps.output.push(c2);
            keep.push((b, d));
        }
        let mut rt = Rt::new(sr, block, cmd_rx, gb_tx, Arc::new(Shared::new()), taps, rec_tx);
        rt.set_skip_silent(self.skip_silent.load(Relaxed));
        rt.set_workers(self.render_workers.load(Relaxed));
        let mut errs = HashMap::new();
        let mut hosted = HashMap::new();
        let graph = build_graph(&project, &HashMap::new(), sr, block, &*self.factory, false, &mut errs, &mut hosted);
        let mut tl = build_timeline(&project, &mut |clip| store_snapshot.get(&clip.id).cloned(), &takes, &HashMap::new(), keys.as_ref());
        let wrap = matches!(tail, RenderTail::Wrap);
        let decay = matches!(tail, RenderTail::Decay { .. });
        tl.loop_range = wrap.then_some((from, to));
        tl.song_end = TOTAL_BEATS;
        let _ = cmd_tx.push(Cmd::Graph(Box::new(graph)));
        for ch in &project.channels {
            if let ChannelSrc::Synth { synth } = &ch.src {
                let _ = cmd_tx.push(Cmd::SynthAdd(Box::new(crate::poly::SynthInst::new(ch.id, crate::poly::params::resolve(synth), sr))));
            }
        }
        // plugin instruments: the render's own instances, from the saved state, in offline mode
        for ch in &project.channels {
            if let ChannelSrc::Plugin { plugin } = &ch.src {
                match self.factory.create_instrument(ch.id, plugin, sr, block, false) {
                    Ok(inst) => {
                        let _ = cmd_tx.push(Cmd::PlugAdd(Box::new(crate::plug::PlugInst::new(ch.id, inst, block))));
                    }
                    Err(e) => log::warn!("render: plugin instrument '{}' is silent: {e}", plugin.name),
                }
            }
        }
        let _ = cmd_tx.push(Cmd::Timeline(Box::new(tl)));
        let _ = cmd_tx.push(Cmd::Play { from, count_in: 0.0, rec: false, rec_loop: false });
        let spb = 60.0 / project.bpm;
        let duration = (to - from) * spb;
        let play_frames = (duration * f64::from(sample_rate)) as usize;
        let (frames, warmup) = match tail {
            RenderTail::Seconds(tail) | RenderTail::Decay { max: tail } => {
                (((duration + tail.max(0.0)) * f64::from(sample_rate)) as usize, 0)
            }
            RenderTail::Wrap => {
                let period = (duration * f64::from(sample_rate)).round() as usize;
                if period == 0 {
                    return Err(EngineError::Msg("loop is shorter than one sample".into()));
                }
                (period, period)
            }
        };
        // plugin delay compensation delays everything by `pdc` frames: render that much
        // longer and drop it from the front, so the file starts on `from` and the last
        // `pdc` frames of the song are not cut off
        let pdc = playback_latency_with(&project, sr, &hosted);
        let too_long = || EngineError::Msg("render is too long".into());
        let discard = warmup.checked_add(pdc).ok_or_else(too_long)?;
        let total = frames.checked_add(discard).ok_or_else(too_long)?;
        let mut out = Vec::new();
        out.try_reserve_exact(frames.checked_mul(2).ok_or_else(too_long)?).map_err(|_| too_long())?;
        let mut l = vec![0f32; block];
        let mut r = vec![0f32; block];
        let mut done = 0;
        let mut stopped = false;
        let hold = (DECAY_HOLD_S * f64::from(sample_rate)) as usize;
        let mut quiet = 0usize;
        let frames_per_beat = spb * f64::from(sample_rate);
        let beat_at = |done: usize| {
            let rendered = done.saturating_sub(pdc);
            let at = if wrap { rendered % frames } else { rendered };
            from + at as f64 / frames_per_beat
        };
        let peak_of = |l: &[f32], r: &[f32]| l.iter().chain(r).fold(0f32, |m, x| if x.abs() > m { x.abs() } else { m });
        let keep = |out: &mut Vec<f32>, at: usize, l: &[f32], r: &[f32]| {
            for i in discard.saturating_sub(at)..l.len() {
                out.push(l[i]);
                out.push(r[i]);
            }
        };
        while done < total {
            if cancel.load(Relaxed) {
                return Err(EngineError::Cancelled);
            }
            let mut n = block.min(total - done);
            if wrap {
                let pos = done % frames;
                if done != 0 && pos == 0 {
                    let _ = cmd_tx.push(Cmd::LoopTo(from));
                }
                n = n.min(frames - pos);
            } else if !stopped && done + n > play_frames {
                // stop exactly at `to`: render up to it, then let tails ring
                let k = play_frames.saturating_sub(done);
                if k > 0 {
                    rt.process(&mut l[..k], &mut r[..k], None);
                    keep(&mut out, done, &l[..k], &r[..k]);
                    done += k;
                    progress(RenderProgress::Rendering { done, total, peak: peak_of(&l[..k], &r[..k]), beat: beat_at(done) });
                }
                let _ = cmd_tx.push(Cmd::Stop);
                stopped = true;
                continue;
            }
            rt.process(&mut l[..n], &mut r[..n], None);
            keep(&mut out, done, &l[..n], &r[..n]);
            done += n;
            // the discarded warm-up and delay-compensation frames do not reach the file
            let peak = if done > discard { peak_of(&l[..n], &r[..n]) } else { 0.0 };
            progress(RenderProgress::Rendering { done, total, peak, beat: beat_at(done) });
            while gb_rx.pop().is_ok() {}
            if decay && stopped && done > discard {
                quiet = if peak < DECAY_FLOOR { quiet + n } else { 0 };
                if quiet >= hold {
                    break;
                }
            }
        }
        if decay {
            // end on the last audible frame of the tail, but never before `to`
            let last = out.iter().rposition(|x| x.abs() >= DECAY_FLOOR).map_or(0, |i| i / 2 + 1);
            out.truncate(last.max(play_frames).min(out.len() / 2) * 2);
        }
        // the final progress callback can still cancel: then the finished audio is not a result
        if cancel.load(Relaxed) {
            return Err(EngineError::Cancelled);
        }
        Ok(out)
    }
}

impl Engine {
    /// FL's *Export in place* (consolidate): renders the selected clips — through their mixer
    /// inserts and effects, without the master's — over the span they cover, saves the audio
    /// with the project's media and puts it as one audio clip on a new track right below the
    /// selection, routed to the master; the originals are muted. One undo step. Automation
    /// still applies; other clips are left out. Like FL's *Render to audio in place*, effect
    /// tails and releases are kept: the clip runs past the span until the decay has died away
    /// (at most [`IN_PLACE_MAX_TAIL_S`]); on its own new track it overlaps nothing.
    pub fn export_in_place(
        &self,
        clip_ids: &[Id],
        sample_rate: u32,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(RenderProgress),
    ) -> Result<InPlace, EngineError> {
        let err = |m: &str| EngineError::Msg(m.into());
        let (picked, from, to, bpm, at_rev) = {
            let c = self.core.lock();
            let picked: Vec<Clip> =
                c.project.clips.iter().filter(|x| clip_ids.contains(&x.id) && x.kind != ClipKind::Auto).cloned().collect();
            if picked.is_empty() {
                return Err(err("select the clips to export in place (automation clips alone make no sound)"));
            }
            let from = picked.iter().map(|x| x.start).fold(f64::INFINITY, f64::min);
            let to = picked.iter().map(|x| x.start + x.len).fold(f64::NEG_INFINITY, f64::max);
            (picked, from, to, c.project.bpm, c.revision)
        };
        let ids: HashSet<Id> = picked.iter().map(|x| x.id).collect();
        let tracks: HashSet<u32> = picked.iter().map(|x| x.track).collect();
        let scope = |mut p: Project| {
            // only the selection sounds (automation keeps moving its targets); what the user
            // muted or soloed elsewhere does not decide what an explicit selection renders
            p.clips.retain(|x| ids.contains(&x.id) || x.kind == ClipKind::Auto);
            for x in p.clips.iter_mut() {
                if ids.contains(&x.id) {
                    x.muted = false;
                }
            }
            for (i, t) in p.tracks.iter_mut().enumerate() {
                t.solo = false;
                if tracks.contains(&(i as u32)) {
                    t.muted = false;
                }
            }
            for ch in p.channels.iter_mut() {
                ch.solo = false;
            }
            // the result plays into the master: rendering the master's chain too would apply it twice
            for m in p.inserts.iter_mut() {
                m.solo = false;
                if m.id == 0 {
                    m.fx.clear();
                    m.vol = crate::model::VOL_UNITY;
                    m.pan = 0.0;
                    m.width = 1.0;
                    m.muted = false;
                }
            }
            p
        };
        let tail = RenderTail::Decay { max: IN_PLACE_MAX_TAIL_S };
        let data = self.render_scoped(&scope, from, to, tail, sample_rate, cancel, progress)?;
        // the render's own tempo turns its length into beats: the span plus the tail
        let len = ((data.len() / 2) as f64 / f64::from(sample_rate) * bpm / 60.0).max(to - from);
        let audio = Audio::from_stereo(data.clone(), sample_rate, 2);
        if audio.peak() < 1e-6 {
            return Err(err("the selection renders silent: nothing to export"));
        }
        let mut c = self.core.lock();
        // the bounce answers for the project it rendered: any edit after the snapshot (a clip
        // moved, its gain or sample changed, a track added) refuses the export — it must not
        // mute clips over audio that is not theirs
        if c.revision != at_rev {
            return Err(err("the project changed while the selection was rendered; export it again"));
        }
        for x in &picked {
            let now = c
                .project
                .clips
                .iter()
                .find(|y| y.id == x.id)
                .ok_or(err("the selection changed while it was rendered (a clip was deleted); export it again"))?;
            if now.start != x.start || now.len != x.len || now.track != x.track {
                return Err(err("the selection changed while it was rendered (a clip moved); export it again"));
            }
        }
        let first = picked.iter().min_by(|a, b| a.track.cmp(&b.track).then(a.start.total_cmp(&b.start))).unwrap();
        let low = picked.iter().map(|x| x.track).max().unwrap();
        let name = match &picked[..] {
            [one] => format!("{} (in place)", clip_name(&c.project, one)),
            _ => format!("{} + {} (in place)", clip_name(&c.project, first), picked.len() - 1),
        };
        let color = clip_color(&c.project, first);
        let path = fresh_path(&audio_dir(&c), &sanitize(&name));
        write_wav(&path, &data, sample_rate)?;
        let before = c.project.clone();
        let source_id = c.project.alloc_id();
        c.project.sources.push(Source {
            id: source_id,
            name: name.clone(),
            color: color.clone(),
            path: path.display().to_string(),
            dur: audio.seconds(),
            sr: sample_rate,
            ch: 2,
            peak_db: crate::dsp::gain_to_db(audio.peak()),
        });
        c.sources.insert(source_id, Arc::new(audio));
        // the project may have changed while rendering: the track goes under where the selection is now
        let at = (low + 1).min(c.project.tracks.len() as u32);
        let h = c.project.tracks.get(low as usize).map_or(60.0, |t| t.h);
        let track_id = c.project.alloc_id();
        c.project.tracks.insert(
            at as usize,
            Track {
                id: track_id,
                name: name.clone(),
                color: color.clone(),
                muted: false,
                solo: false,
                arm: false,
                vol: VOL_UNITY,
                h,
                insert: 0,
                icon: None,
            },
        );
        for x in c.project.clips.iter_mut() {
            if x.track >= at {
                x.track += 1;
            }
            if ids.contains(&x.id) {
                x.muted = true;
            }
        }
        let clip_id = c.project.alloc_id();
        c.project.clips.push(Clip {
            id: clip_id,
            track: at,
            start: from,
            len,
            offset: 0.0,
            kind: ClipKind::Audio,
            source: source_id,
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
            root: crate::model::CLIP_ROOT,
        });
        push_undo(&mut c, before);
        c.revision += 1;
        self.sync(&mut c, Dirty { timeline: true, mixer: true, ..Default::default() });
        self.changed(c);
        Ok(InPlace { source_id, clip_id, track: at })
    }
}

/// Longest effect tail [`Engine::export_in_place`] keeps after the selection (a feedback
/// delay set to never die out would otherwise ring forever).
pub const IN_PLACE_MAX_TAIL_S: f64 = 30.0;

/// What [`Engine::export_in_place`] made: the new source, its clip and the track it is on.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InPlace {
    pub source_id: Id,
    pub clip_id: Id,
    pub track: u32,
}

/// Longest region "Extract stems" separates: what [`Audio::load`] decodes again when the project
/// is reopened (longer stems would come back cut).
pub const STEMS_MAX_S: f64 = 1800.0;

/// The audio an audio clip plays, for stem separation ([`Engine::stem_region`]): the source
/// region under the clip with `lead` seconds of context before it and up to as much after,
/// normalised and/or reversed as the clip plays it (interleaved stereo at the source's rate).
pub struct StemRegion {
    pub clip: Clip,
    /// The clip's name as the playlist shows it (its sample's).
    pub name: String,
    pub data: Vec<f32>,
    pub sr: u32,
    /// Seconds of the (normalised/reversed) source where `data` starts.
    pub from_s: f64,
    /// Length of the whole source in seconds.
    pub source_s: f64,
    /// Where new media of this project are written (the project's audio folder, or the
    /// scratch folder of an unsaved project, which the first save moves next to it).
    pub dir: PathBuf,
    /// The exact audio the region was cut from: a relink that swaps the file under the
    /// same source id rejects the placement ([`Engine::place_stems`]).
    pub source_audio: Arc<Audio>,
}

/// One separated stem for [`Engine::place_stems`]: its label ("Drums"), the WAV written to the
/// project's media and the decoded audio of that file.
pub struct StemFile {
    pub label: String,
    pub color: String,
    pub path: PathBuf,
    pub audio: Audio,
}

/// What [`Engine::place_stems`] made: one source, track and clip per stem (in order), the
/// index of the first new track; the source clip is now muted.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacedStems {
    pub source_ids: Vec<Id>,
    pub clip_ids: Vec<Id>,
    pub track: u32,
    pub muted_clip: Id,
}

impl Engine {
    /// The audio a clip plays from its source, for "Extract stems": the region under the clip
    /// plus up to `context_s` seconds either side (the separator hears what leads into the
    /// region), with the clip's normalise/reverse applied. Pitch, stretch, gain, fades and pan are
    /// left to the stem clips, which copy them. The lock is held only to copy the clip and the
    /// source's `Arc`.
    pub fn stem_region(&self, clip_id: Id, context_s: f64) -> Result<StemRegion, EngineError> {
        let err = |m: &str| EngineError::Msg(m.into());
        let (clip, name, base, bpm, dir, source_audio) = {
            let c = self.core.lock();
            let clip = c.project.clips.iter().find(|x| x.id == clip_id).cloned().ok_or(err("no such clip"))?;
            if clip.kind != ClipKind::Audio {
                return Err(err("stems can be extracted from audio clips only"));
            }
            let Some(base) = c.sources.get(clip.source).cloned() else {
                let why = c.source_errors.get(&clip.source).cloned().unwrap_or_else(|| "not loaded".into());
                return Err(EngineError::Msg(format!("the clip's audio file is missing ({why}); relink it first")));
            };
            (clip.clone(), clip_name(&c.project, &clip), base.clone(), c.project.bpm, audio_dir(&c), base)
        };
        let audio = if clip.norm || clip.rev { Arc::new(base.derived(clip.norm, clip.rev)) } else { base };
        let sr = f64::from(audio.sr);
        let (sa, sb) = clip.source_span(bpm);
        let sb = sb.min(audio.seconds());
        if sb - sa < 0.05 {
            return Err(err("the clip plays less than 50 ms of its audio: nothing to separate"));
        }
        if sb - sa > STEMS_MAX_S {
            return Err(EngineError::Msg(format!(
                "the clip plays {:.0} minutes of audio; stems can be extracted from at most {:.0} minutes at a time (split the clip first)",
                (sb - sa) / 60.0,
                STEMS_MAX_S / 60.0
            )));
        }
        // the written file is what `Audio::load` decodes again when the project reopens, and
        // that decode stops at STEMS_MAX_S: the context shrinks so the whole region fits
        let (a, b) = stem_region_bounds(sa, sb, context_s, sr, audio.frames);
        Ok(StemRegion {
            data: audio.content()[a * 2..b * 2].to_vec(),
            sr: audio.sr,
            from_s: a as f64 / sr,
            source_s: audio.seconds(),
            clip,
            name,
            dir,
            source_audio,
        })
    }

    /// Puts separated stems into the playlist as one undo step: each stem becomes a source, a new
    /// track right below the source clip's track (in the order given, playing into that track's
    /// insert) and a clip at the same place as the source clip, playing the same part of the audio
    /// the same way (stretch, pitch, gain, fades, pan, mixer insert); the source clip is muted.
    ///
    /// `region` is what was separated. The project may have changed meanwhile: the clip must
    /// still exist, play the same source the same way (normalise/reverse) and stay inside the
    /// separated region, else nothing changes and the files are the caller's to delete.
    pub fn place_stems(&self, region: &StemRegion, stems: Vec<StemFile>) -> Result<PlacedStems, EngineError> {
        let err = |m: &str| EngineError::Msg(m.into());
        if stems.is_empty() {
            return Err(err("no stems to place"));
        }
        let mut c = self.core.lock();
        let now = c
            .project
            .clips
            .iter()
            .find(|x| x.id == region.clip.id)
            .cloned()
            .ok_or(err("the clip was deleted while its stems were extracted; nothing was added"))?;
        let (sa, sb) = now.source_span(c.project.bpm);
        let to_s = region.from_s + region.data.len() as f64 / 2.0 / f64::from(region.sr);
        if now.kind != ClipKind::Audio
            || now.source != region.clip.source
            || now.norm != region.clip.norm
            || now.rev != region.clip.rev
            || sa < region.from_s - 1e-6
            || sb.min(region.source_s) > to_s + 1e-3
            // a relink can swap the audio under the same source id: then the stems came
            // out of audio the clip no longer plays
            || !c.sources.get(region.clip.source).is_some_and(|a| Arc::ptr_eq(a, &region.source_audio))
        {
            return Err(err("the clip changed while its stems were extracted (another sample or region); extract them again"));
        }
        let before = c.project.clone();
        let track = c.project.tracks.get(now.track as usize).cloned().ok_or(err("the clip's track is missing"))?;
        let at = now.track + 1;
        let n = stems.len() as u32;
        for x in c.project.clips.iter_mut() {
            if x.track >= at {
                x.track += n;
            }
            if x.id == now.id {
                x.muted = true;
            }
        }
        let spb = 60.0 / c.project.bpm.max(1.0);
        // the stem file starts at `from_s` of the source: the same second sits that much earlier in it
        let offset = (sa - region.from_s).max(0.0) * now.stretch.max(STRETCH_MIN) / spb;
        let (mut source_ids, mut clip_ids) = (Vec::new(), Vec::new());
        for (k, s) in stems.into_iter().enumerate() {
            let name = format!("{} – {}", region.name, s.label);
            let source_id = c.project.alloc_id();
            c.project.sources.push(Source {
                id: source_id,
                name: name.clone(),
                color: s.color.clone(),
                path: s.path.display().to_string(),
                dur: s.audio.seconds(),
                sr: s.audio.sr,
                ch: s.audio.channels,
                peak_db: crate::dsp::gain_to_db(s.audio.peak()),
            });
            c.sources.insert(source_id, Arc::new(s.audio));
            let track_id = c.project.alloc_id();
            let ti = at + k as u32;
            c.project.tracks.insert(
                ti as usize,
                Track {
                    id: track_id,
                    name,
                    color: s.color,
                    muted: false,
                    solo: false,
                    arm: false,
                    vol: VOL_UNITY,
                    h: track.h,
                    insert: track.insert,
                    icon: None,
                },
            );
            let clip_id = c.project.alloc_id();
            c.project.clips.push(Clip {
                id: clip_id,
                track: ti,
                source: source_id,
                offset,
                muted: false,
                norm: false,
                rev: false,
                // note edits belong to the analysis of the mix; the stems play the source's pitch
                tune: None,
                ..now.clone()
            });
            source_ids.push(source_id);
            clip_ids.push(clip_id);
        }
        push_undo(&mut c, before);
        c.revision += 1;
        self.sync(&mut c, Dirty { timeline: true, mixer: true, ..Default::default() });
        self.changed(c);
        Ok(PlacedStems { source_ids, clip_ids, track: at, muted_clip: now.id })
    }
}

/// The project as a pattern export hears it, like PAT mode plays it: pattern `id` once from beat 0
/// and nothing else of the arrangement (no other clip, no automation clip, no take). Its channels play
/// into their own inserts, or the master when they have none, never through a playlist track's insert.
pub fn pattern_project(mut p: Project, id: Id) -> Project {
    let len = p.patterns.iter().find(|x| x.id == id).map_or(BAR_BEATS, |x| x.len.max(BAR_BEATS));
    p.clips.clear();
    p.comps.clear();
    p.loop_range = None;
    if p.tracks.is_empty() {
        p.tracks.push(Track {
            id: 1,
            name: "Pattern".into(),
            color: "#4f8ef7".into(),
            muted: false,
            solo: false,
            arm: false,
            vol: VOL_UNITY,
            h: 56.0,
            insert: 0,
            icon: None,
        });
    }
    for (i, t) in p.tracks.iter_mut().enumerate() {
        t.solo = false;
        if i == 0 {
            t.muted = false;
            t.vol = VOL_UNITY;
            t.insert = 0;
        }
    }
    let cid = p.alloc_id();
    p.clips.push(Clip {
        id: cid,
        track: 0,
        start: 0.0,
        len,
        offset: 0.0,
        kind: ClipKind::Pat,
        source: id,
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
        root: crate::model::CLIP_ROOT,
    });
    p
}

/// The project as a stem export hears it: only insert `id` is audible, nothing is soloed, and the
/// master is a plain unity sum (no effects, fader at 0 dB, centre, normal width). Inserts keep their
/// effects so delay compensation stays the same as in the mix and every stem lines up.
fn stem_project(mut p: Project, id: Id) -> Project {
    for t in p.tracks.iter_mut() {
        t.solo = false;
    }
    for c in p.channels.iter_mut() {
        c.solo = false;
    }
    // the inserts routed into this one (its sources, through any number of sends): they stay,
    // sending only on the way here; this one goes straight to the master; the rest are muted
    let feeds: Vec<Id> =
        p.inserts.iter().filter(|m| m.id != 0 && m.id != id && crate::routing::reaches(&p, m.id, id)).map(|m| m.id).collect();
    for m in p.inserts.iter_mut() {
        m.solo = false;
        if m.id == 0 {
            m.fx.clear();
            m.vol = crate::model::VOL_UNITY;
            m.pan = 0.0;
            m.width = 1.0;
            m.muted = false;
        } else if m.id == id {
            m.muted = false;
            m.sends.clear();
            m.master = true;
        } else if feeds.contains(&m.id) {
            m.sends.retain(|s| s.to == id || feeds.contains(&s.to));
            m.master = false;
        } else {
            m.muted = true;
        }
    }
    p
}

/// The sample range [`Engine::stem_region`] cuts, in frames: the clip's span with up to
/// `context_s` of context either side, clamped to the source and to what a reopen decodes
/// (`STEMS_MAX_S`, in frames — rounding never grows the region past it).
pub(crate) fn stem_region_bounds(sa: f64, sb: f64, context_s: f64, sr: f64, frames: usize) -> (usize, usize) {
    let context = context_s.min(((STEMS_MAX_S - (sb - sa)) / 2.0).max(0.0)).max(0.0);
    let max_frames = (STEMS_MAX_S * sr).floor() as usize;
    let a = ((sa - context).max(0.0) * sr).floor() as usize;
    let b = (((sb + context) * sr).ceil() as usize).min(frames).min(a.saturating_add(max_frames));
    (a.min(b), b.max(a))
}

#[cfg(test)]
mod stem_bounds_tests {
    use super::*;

    #[test]
    fn the_region_never_exceeds_the_reopen_limit() {
        let sr = 48_000.0;
        let max = (STEMS_MAX_S * sr) as usize;
        // a clip of exactly the limit: context must shrink to nothing on both sides
        let (a, b) = stem_region_bounds(10.0, 10.0 + STEMS_MAX_S, 30.0, sr, 90_000_000);
        assert_eq!(b - a, max, "exactly at the limit: no context, no frame over");
        // a 2 s clip far into the source with an absurd context: the capped context fills the
        // limit on both sides of the span
        let (a, b) = stem_region_bounds(899.0, 901.0, 100_000.0, sr, 90_000_000);
        assert_eq!(b - a, max, "span plus the capped context fills the limit");
        // the source itself bounds the region
        let (a, b) = stem_region_bounds(0.0, 1.0, 50.0, sr, 48_000);
        assert_eq!((a, b), (0, 48_000));
        // rounding up of the context's end cannot push past the limit
        let (a, b) = stem_region_bounds(0.0, STEMS_MAX_S - 0.001, 0.001, sr, usize::MAX);
        assert!(b - a <= max, "rounded region {} over {}", b - a, max);
    }
}
