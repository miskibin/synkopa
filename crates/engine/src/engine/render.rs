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
        self.render_scoped(&scope, from, to, tail, sample_rate, cancel, progress, None)
    }

    /// The mix assistant over `[from, to)` beats, offline (what a live run reports after playing
    /// that span once): renders the project at 48 kHz and analyses every insert. For the CLI and
    /// the agent API, where no stream is running.
    pub fn masking_scan(&self, from: f64, to: f64, cancel: &AtomicBool) -> Result<Vec<crate::masking::Collision>, EngineError> {
        let mut d = crate::masking::Detector::new(48000.0);
        self.render_scoped(&|p| p, from, to, RenderTail::Seconds(0.0), 48000, cancel, &mut |_| {}, Some(&mut d))?;
        Ok(d.collisions())
    }

    /// What reaches effect `fx_id` over the whole song (`0..song_end`, 2 s of tail): the project
    /// with that effect and everything after it on its insert taken out, the insert's fader,
    /// pan and width at unity (they come after the effects in the chain). On the master that is
    /// the mix; on another insert its stem as the effect hears it. Interleaved stereo at 48 kHz.
    pub fn render_fx_input(
        &self,
        fx_id: Id,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(RenderProgress),
    ) -> Result<Vec<f32>, EngineError> {
        let (insert, end) = {
            let c = self.core.lock();
            let insert = c.project.find_fx(fx_id).map(|(m, _)| m.id).ok_or_else(|| EngineError::Msg(format!("no effect {fx_id}")))?;
            (insert, c.project.song_end())
        };
        if end <= 0.0 {
            return Err(EngineError::Msg("the song is empty: there is nothing to listen to".into()));
        }
        let scope = move |p: Project| {
            let mut p = if insert == 0 { p } else { stem_project(p, insert) };
            if let Some(m) = p.inserts.iter_mut().find(|m| m.id == insert) {
                if let Some(at) = m.fx.iter().position(|f| f.id == fx_id) {
                    m.fx.truncate(at);
                }
                m.vol = crate::model::VOL_UNITY;
                m.pan = 0.0;
                m.width = 1.0;
                m.muted = false;
            }
            p
        };
        self.render_scoped(&scope, 0.0, end, RenderTail::Seconds(2.0), 48000, cancel, progress, None)
    }

    /// The Master's assistant on effect `fx_id` (`mastering::listen`): renders what reaches it
    /// over the whole song, then measures and sets it. `target`: index into
    /// `dsp::master::TARGETS` (`None` = the effect's own Target). Read-only: the caller applies
    /// the returned params and report with `PutFx`.
    pub fn master_listen(
        &self,
        fx_id: Id,
        target: Option<usize>,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(f32),
    ) -> Result<crate::mastering::Listened, EngineError> {
        let (params, own_target) = {
            let c = self.core.lock();
            let (_, fx) = c.project.find_fx(fx_id).ok_or_else(|| EngineError::Msg(format!("no effect {fx_id}")))?;
            if fx.kind != FxKind::Master {
                return Err(EngineError::Msg(format!("effect {fx_id} is not a Master")));
            }
            (fx.params.clone(), fx.params.get("target").copied().unwrap_or(1.0).round().max(0.0) as usize)
        };
        // the render is the first ~40 % of the work, the assistant's own renders the rest
        let mut on_render = |r: RenderProgress| {
            if let RenderProgress::Rendering { done, total, .. } = r {
                progress(0.4 * done as f32 / total.max(1) as f32);
            }
        };
        let x = self.render_fx_input(fx_id, cancel, &mut on_render)?;
        let (l, r): (Vec<f32>, Vec<f32>) = x.as_chunks::<2>().0.iter().map(|f| (f[0], f[1])).unzip();
        crate::mastering::listen(&l, &r, 48000.0, &params, target.unwrap_or(own_target), cancel, &mut |p| progress(0.4 + 0.6 * p))
            .map_err(|e| if e == "cancelled" { EngineError::Cancelled } else { EngineError::Msg(e) })
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
        mut mask: Option<&mut crate::masking::Detector>,
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
        let mut mask_rx = vec![];
        if let Some(d) = mask.as_deref_mut() {
            d.configure(&project);
            let mut tx = vec![];
            for _ in 0..project.inserts.len() {
                let (a, b) = rtrb::RingBuffer::<f32>::new(block * 2);
                tx.push(a);
                mask_rx.push(b);
            }
            rt.set_mask_taps(tx);
        }
        rt.set_skip_silent(self.skip_silent.load(Relaxed));
        rt.set_workers(self.render_workers.load(Relaxed));
        let mut errs = HashMap::new();
        let mut hosted = HashMap::new();
        let graph = build_graph(&project, &HashMap::new(), sr, block, &*self.factory, false, &mut errs, &mut hosted);
        // the offline render is the song itself: a live tweak's override does not apply to it
        let mut tl =
            build_timeline(&project, &mut |clip| store_snapshot.get(&clip.id).cloned(), &takes, &HashMap::new(), &keys, &HashSet::new());
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
        // the render walks the tempo map: the duration is the map's integral over the range
        // (a constant project compiles to one point, so nothing changes for it)
        let tempo = super::tempo::TempoMap::compile(&project.tempo_points, project.bpm);
        let duration = tempo.seconds_between(from, to);
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
        let beat_at = |done: usize| {
            let rendered = done.saturating_sub(pdc);
            let at = if wrap { rendered % frames } else { rendered };
            if tempo.is_constant() {
                from + at as f64 * (60.0 / project.bpm) / f64::from(sample_rate)
            } else {
                // the seconds position inside the (possibly wrapped) pass → the beat
                tempo.beat_at(tempo.seconds_between(0.0, from) + at as f64 / f64::from(sample_rate))
            }
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
                    if let Some(d) = mask.as_deref_mut() {
                        d.drain(&mut mask_rx);
                    }
                    keep(&mut out, done, &l[..k], &r[..k]);
                    done += k;
                    progress(RenderProgress::Rendering { done, total, peak: peak_of(&l[..k], &r[..k]), beat: beat_at(done) });
                }
                let _ = cmd_tx.push(Cmd::Stop);
                stopped = true;
                continue;
            }
            rt.process(&mut l[..n], &mut r[..n], None);
            if let Some(d) = mask.as_deref_mut() {
                d.drain(&mut mask_rx);
            }
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
    ///
    /// With a `range` (the playlist's time selection, in beats) only that part is rendered, like
    /// FL's consolidate of a time selection: the clips are the selected ones that overlap it, or
    /// — with no clip selected — every clip that plays there (not muted, on an audible track).
    /// A clip crossing an edge of the range is split there: only its inside part is rendered and
    /// muted, the parts outside keep playing.
    pub fn export_in_place(
        &self,
        clip_ids: &[Id],
        range: Option<(f64, f64)>,
        sample_rate: u32,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(RenderProgress),
    ) -> Result<InPlace, EngineError> {
        let err = |m: &str| EngineError::Msg(m.into());
        if let Some((a, b)) = range {
            if !a.is_finite() || !b.is_finite() || b - a < 1e-6 || a < 0.0 {
                return Err(err("the time selection is empty"));
            }
        }
        let (picked, from, to, bpm, at_rev) = {
            let c = self.core.lock();
            let p = &c.project;
            let any_solo = p.tracks.iter().any(|t| t.solo);
            let audible = |x: &Clip| !x.muted && p.tracks.get(x.track as usize).is_some_and(|t| p.track_audible_with(t, any_solo));
            let inside = |x: &Clip| range.is_none_or(|(a, b)| x.start < b - 1e-6 && x.start + x.len > a + 1e-6);
            let picked: Vec<Clip> = p
                .clips
                .iter()
                .filter(|x| x.kind != ClipKind::Auto && inside(x))
                .filter(|x| if clip_ids.is_empty() && range.is_some() { audible(x) } else { clip_ids.contains(&x.id) })
                .cloned()
                .collect();
            if picked.is_empty() {
                return Err(err(if range.is_some() {
                    "nothing plays in the time selection to export in place"
                } else {
                    "select the clips to export in place (automation clips alone make no sound)"
                }));
            }
            let mut from = picked.iter().map(|x| x.start).fold(f64::INFINITY, f64::min);
            let mut to = picked.iter().map(|x| x.start + x.len).fold(f64::NEG_INFINITY, f64::max);
            if let Some((a, b)) = range {
                from = from.max(a);
                to = to.min(b);
            }
            (picked, from, to, p.bpm, c.revision)
        };
        let ids: HashSet<Id> = picked.iter().map(|x| x.id).collect();
        let tracks: HashSet<u32> = picked.iter().map(|x| x.track).collect();
        let scope = |mut p: Project| {
            // only the selection sounds (automation keeps moving its targets), and the sidechain
            // keys its effects listen to; what the user muted or soloed elsewhere does not
            // decide what an explicit selection renders
            keep_keys(&mut p, &ids);
            for x in p.clips.iter_mut() {
                if ids.contains(&x.id) {
                    x.muted = false;
                    // only the part inside the time selection sounds: what plays before it must
                    // not ring into the render (the part before keeps playing live)
                    if range.is_some() {
                        crop_clip(x, from, to);
                    }
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
        let data = self.render_scoped(&scope, from, to, tail, sample_rate, cancel, progress, None)?;
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
        c.project.tracks.insert(at as usize, Track { h, ..Track::new(track_id, name.clone(), color.clone(), 0) });
        let mut outside = vec![];
        for x in c.project.clips.iter_mut() {
            if x.track >= at {
                x.track += 1;
            }
            if ids.contains(&x.id) {
                // a clip crossing an edge of the time selection is split there: the parts
                // outside keep playing as they were, the rendered part inside is muted
                if range.is_some() {
                    let (start, end) = (x.start, x.start + x.len);
                    if start < from - 1e-6 {
                        let mut before = x.clone();
                        crop_clip(&mut before, start, from);
                        outside.push(before);
                    }
                    if end > to + 1e-6 {
                        let mut after = x.clone();
                        crop_clip(&mut after, to, end);
                        outside.push(after);
                    }
                    crop_clip(x, from, to);
                }
                x.muted = true;
            }
        }
        for mut x in outside {
            x.id = c.project.alloc_id();
            c.project.clips.push(x);
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
        push_undo(&mut c, before, Some("Export in place".into()));
        c.revision += 1;
        self.sync(&mut c, Dirty { timeline: true, mixer: true, ..Default::default() });
        self.changed(c);
        Ok(InPlace { source_id, clip_id, track: at })
    }
}

/// Longest effect tail [`Engine::export_in_place`] keeps after the selection (a feedback
/// delay set to never die out would otherwise ring forever).
pub const IN_PLACE_MAX_TAIL_S: f64 = 30.0;

/// `x` cut down to the beats `a..b` of the timeline (both inside it), the way the playlist's
/// slice tool cuts a clip: the content stays where it plays, a fade at a cut edge goes.
fn crop_clip(x: &mut Clip, a: f64, b: f64) {
    let (start, end) = (x.start, x.start + x.len);
    let (a, b) = (a.max(start), b.min(end));
    if a > start {
        x.offset += a - start;
        x.fade_in = 0.0;
    }
    if b < end {
        x.fade_out = 0.0;
    }
    x.start = a;
    x.len = b - a;
}

/// The inserts clip `x` plays into: its own or its track's, and for a pattern each of its
/// channels' own (the track's only for a channel without one).
fn clip_sources(p: &Project, x: &Clip) -> Vec<Id> {
    let pat = (x.kind == ClipKind::Pat).then(|| p.patterns.iter().find(|pt| pt.id == x.source)).flatten();
    let Some(pt) = pat else { return vec![p.clip_insert(x)] };
    let mut v: Vec<Id> = vec![];
    for c in p.channels.iter().filter(|c| pt.notes.iter().any(|n| n.ch == c.id)) {
        let i = c.insert.filter(|i| p.insert(*i).is_some()).unwrap_or_else(|| p.clip_insert(x));
        if !v.contains(&i) {
            v.push(i);
        }
    }
    v
}

/// Keeps only the clips `picked` (and automation), and the sidechains they need: the clips play through effects
/// keyed from other inserts (a bass ducked by the kick), and those keys must sound in the render
/// as they do in the mix, heard nowhere (like a stem's, see [`stem_project`]). Adds back the
/// clips that only feed such keys, and mutes every insert neither path needs.
fn keep_keys(p: &mut Project, picked: &HashSet<Id>) {
    let reach = |p: &Project, from: &[Id]| -> Vec<Id> {
        p.inserts.iter().filter(|m| from.iter().any(|&a| crate::routing::reaches(p, a, m.id))).map(|m| m.id).collect()
    };
    let sources: Vec<Id> = p.clips.iter().filter(|x| picked.contains(&x.id)).flat_map(|x| clip_sources(p, x)).collect();
    let path = reach(p, &sources);
    // the inserts feeding the keys of the path's effects, and of theirs in turn
    let mut need = path.clone();
    let mut silent: Vec<Id> = vec![];
    let mut i = 0;
    while i < need.len() {
        let keys: Vec<Id> = p.insert(need[i]).map(|m| m.fx.iter().filter_map(|f| f.key).collect()).unwrap_or_default();
        for k in keys {
            for x in p.inserts.iter().filter(|m| m.id != 0 && crate::routing::reaches(p, m.id, k)).map(|m| m.id) {
                if !need.contains(&x) {
                    need.push(x);
                    silent.push(x);
                }
            }
        }
        i += 1;
    }
    if silent.is_empty() {
        p.clips.retain(|x| picked.contains(&x.id) || x.kind == ClipKind::Auto);
        return;
    }
    // a clip that feeds only those keys plays again; one heard anywhere else stays out
    let feeds_key = |x: &Clip, p: &Project| {
        let src = clip_sources(p, x);
        src.iter().any(|i| silent.contains(i)) && src.iter().all(|i| *i != 0 && !path.contains(i))
    };
    let keep: HashSet<Id> =
        p.clips.iter().filter(|x| !picked.contains(&x.id) && x.kind != ClipKind::Auto && feeds_key(x, p)).map(|x| x.id).collect();
    p.clips.retain(|x| picked.contains(&x.id) || keep.contains(&x.id) || x.kind == ClipKind::Auto);
    for m in p.inserts.iter_mut().filter(|m| m.id != 0) {
        if silent.contains(&m.id) {
            m.master = false;
            m.sends.retain(|s| silent.contains(&s.to));
        } else if !path.contains(&m.id) {
            m.muted = true;
        }
    }
}

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
            c.project.tracks.insert(ti as usize, Track { h: track.h, insert: track.insert, ..Track::new(track_id, name, s.color, 0) });
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
        push_undo(&mut c, before, Some("Extract stems".into()));
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
        p.tracks.push(Track::new(1, "Pattern", "#4f8ef7", 0));
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
    let was_muted: Vec<Id> = p.inserts.iter().filter(|m| m.muted).map(|m| m.id).collect();
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
    // sidechains the stem's effects listen to keep running, heard nowhere: the stem must sound
    // as it does in the mix (a bass ducked by the kick stays ducked in its stem)
    let mut need: Vec<Id> = feeds.iter().copied().chain([id]).collect();
    let mut silent: Vec<Id> = vec![];
    let mut i = 0;
    while i < need.len() {
        let keys: Vec<Id> = p.insert(need[i]).map(|m| m.fx.iter().filter_map(|f| f.key).collect()).unwrap_or_default();
        for k in keys {
            let kfeeds: Vec<Id> = p.inserts.iter().filter(|m| m.id != 0 && crate::routing::reaches(&p, m.id, k)).map(|m| m.id).collect();
            for x in kfeeds {
                if !need.contains(&x) {
                    need.push(x);
                    silent.push(x);
                }
            }
        }
        i += 1;
    }
    for m in p.inserts.iter_mut().filter(|m| silent.contains(&m.id)) {
        m.muted = was_muted.contains(&m.id);
        m.master = false;
        m.sends.retain(|s| silent.contains(&s.to));
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
