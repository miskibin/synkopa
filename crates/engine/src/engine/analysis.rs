//! Pitch, melody and music analysis, auto-vocal planning and Rider baking:
//! everything that reads loaded audio and answers questions about it.

use super::*;

impl Engine {
    /// Pitch curve and notes of an audio clip's source (for the note editor).
    pub fn pitch_analysis(&self, clip_id: Id) -> Result<Arc<crate::pitch::PitchAnalysis>, EngineError> {
        let clip = self.audio_clip(clip_id)?;
        self.source_analysis(clip.source, clip.norm, clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))
    }

    fn audio_clip(&self, clip_id: Id) -> Result<Clip, EngineError> {
        let c = self.core.lock();
        let clip = c.project.clips.iter().find(|x| x.id == clip_id).cloned().ok_or(EngineError::Msg("no such clip".into()))?;
        if clip.kind != ClipKind::Audio {
            return Err(EngineError::Msg("only audio clips have pitch".into()));
        }
        Ok(clip)
    }

    /// Analysis of a source variant; slow detectors run without holding the project lock.
    fn source_analysis(&self, id: Id, norm: bool, rev: bool) -> Option<Arc<crate::pitch::PitchAnalysis>> {
        let (audio, est) = {
            let mut c = self.core.lock();
            if let Some(a) = c.sources.cached_analysis(id, norm, rev) {
                return Some(a);
            }
            (c.sources.variant(id, norm, rev)?, c.sources.estimator())
        };
        let a = Arc::new(crate::pitch::analyze_with(&crate::sources::mono_of(&audio), audio.sr as f32, est.as_deref()));
        let mut c = self.core.lock();
        c.sources.put_analysis(id, norm, rev, a.clone());
        Some(c.sources.cached_analysis(id, norm, rev).unwrap_or(a))
    }

    fn source_content(&self, id: Id, norm: bool, rev: bool) -> Option<Arc<crate::content::ContentFeatures>> {
        let audio = {
            let mut c = self.core.lock();
            if let Some(x) = c.sources.cached_content(id, norm, rev) {
                return Some(x);
            }
            c.sources.variant(id, norm, rev)?
        };
        let x = Arc::new(crate::content::mfcc(&crate::sources::mono_of(&audio), audio.sr as f32));
        self.core.lock().sources.put_content(id, norm, rev, x.clone());
        Some(x)
    }

    /// Tune to Reference target of an audio clip with `params` (the clip's own when `None`).
    /// The slow parts (analysis, features, alignment) run without the project lock and are
    /// cached, so committing the same reference afterwards only renders.
    pub fn pitch_target(&self, clip_id: Id, params: Option<RefTune>) -> Result<Arc<crate::reference::PitchTarget>, EngineError> {
        let clip = self.audio_clip(clip_id)?;
        let p = params
            .or_else(|| clip.tune.as_ref().and_then(|t| t.reference.clone()))
            .ok_or(EngineError::Msg("the clip has no reference".into()))?;
        if let Some(m) = &p.melody {
            let region = clip.source_span(self.core.lock().project.bpm);
            let take = self.source_analysis(clip.source, clip.norm, clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))?;
            let spans = crate::sources::note_spans(&clip, &take);
            let al = crate::melody::alignment(&take, region, &spans, &m.notes);
            return Ok(Arc::new(crate::reference::target(&take, &spans, &al, &p)));
        }
        let key = {
            let mut c = self.core.lock();
            if !c.project.sources.iter().any(|s| s.id == p.source) {
                return Err(EngineError::Msg("the reference source is not in the project".into()));
            }
            let bpm = c.project.bpm;
            c.sources.align_key(&clip, bpm, p.source).ok_or(EngineError::Msg("audio not loaded".into()))?
        };
        let take = self.source_analysis(clip.source, clip.norm, clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))?;
        let cached = self.core.lock().sources.cached_alignment(&key);
        let al = match cached {
            Some(a) => a,
            None => {
                let reference =
                    self.source_analysis(p.source, false, false).ok_or(EngineError::Msg("reference audio not loaded".into()))?;
                let tc = self.source_content(clip.source, clip.norm, clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))?;
                let rc = self.source_content(p.source, false, false).ok_or(EngineError::Msg("reference audio not loaded".into()))?;
                let r = crate::pitch::ANALYSIS_RATE as f64;
                let al = Arc::new(crate::reference::align(
                    &crate::reference::Performance { pitch: &take, content: &tc },
                    (key.2 as f64 / r, key.3 as f64 / r),
                    &crate::reference::Performance { pitch: &reference, content: &rc },
                ));
                self.core.lock().sources.put_alignment(key, al.clone());
                al
            }
        };
        Ok(Arc::new(crate::sources::target_for(&clip, &take, &al, &p)))
    }

    /// Everything the pitch editor shows for an audio clip, read without holding the project
    /// lock during analysis: the clip, its analysis, the Tune to Reference target (if any), the
    /// notes it is edited by (its own, else the analysis' segmentation: `own` says which) and the
    /// pitch every analysis frame will sound at.
    pub fn pitch_view(&self, clip_id: Id) -> Result<PitchView, EngineError> {
        let clip = self.audio_clip(clip_id)?;
        let analysis = self.source_analysis(clip.source, clip.norm, clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))?;
        let target = match clip.tune.as_ref().and_then(|t| t.reference.as_ref()) {
            Some(_) => Some(self.pitch_target(clip_id, None)?),
            None => None,
        };
        let (notes, own) = crate::pitch_api::clip_notes(clip.tune.as_ref(), &analysis);
        let tune = clip.tune.clone().unwrap_or_default();
        let out = crate::tune_render::edited_curve(&analysis, &tune, target.as_deref());
        let (bpm, sr, seconds) = {
            let mut c = self.core.lock();
            let bpm = c.project.bpm;
            let a = c.sources.variant(clip.source, clip.norm, clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))?;
            (bpm, a.sr, a.seconds())
        };
        Ok(PitchView { clip, analysis, target, notes, own, out, bpm, sr, seconds })
    }

    /// Compares an audio clip's singing with another source's (`crate::pitch_api::compare`).
    /// Read-only; seconds per minute of audio (two alignments at most).
    pub fn pitch_compare(&self, clip_id: Id, reference: Id) -> Result<crate::pitch_api::Comparison, EngineError> {
        let view = self.pitch_view(clip_id)?;
        if !self.core.lock().project.sources.iter().any(|s| s.id == reference) {
            return Err(EngineError::Msg(format!("no source {reference}")));
        }
        let tc = self.source_content(view.clip.source, view.clip.norm, view.clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))?;
        let ra = self.source_analysis(reference, false, false).ok_or(EngineError::Msg("reference audio not loaded".into()))?;
        let rc = self.source_content(reference, false, false).ok_or(EngineError::Msg("reference audio not loaded".into()))?;
        let spans: Vec<(f64, f64)> = view.notes.iter().map(|n| (n.start, n.end)).collect();
        Ok(crate::pitch_api::compare(&view.analysis, &tc, view.region(), &spans, &ra, &rc))
    }

    /// What an audio clip plays before time-stretching — with its pitch edits rendered (now, not
    /// in the background) when `edits`, else its plain source — and the part of it the clip
    /// shows (source seconds).
    pub fn clip_tuned_audio(&self, clip_id: Id, edits: bool) -> Result<(Arc<crate::sources::Audio>, (f64, f64)), EngineError> {
        let mut clip = self.audio_clip(clip_id)?;
        if !edits {
            clip.tune = None;
        }
        let mut c = self.core.lock();
        let bpm = c.project.bpm;
        let a = c.sources.clip_source_audio(&clip, false, bpm).ok_or(EngineError::Msg("audio not loaded".into()))?;
        Ok((a, clip.source_span(bpm)))
    }

    /// The notes `channel` plays over an audio clip, as a melody reference (`span`: seconds of
    /// the source, only notes overlapping it), and how well they fit what is sung (0..1).
    pub fn melody_reference(&self, clip_id: Id, channel: Id, span: Option<[f64; 2]>) -> Result<(crate::model::Melody, f32), EngineError> {
        let clip = self.audio_clip(clip_id)?;
        let (notes, region) = {
            let c = self.core.lock();
            if c.project.channels.iter().all(|ch| ch.id != channel) {
                return Err(EngineError::Msg("no such channel".into()));
            }
            (crate::melody::collect(&c.project, &clip, channel, span), clip.source_span(c.project.bpm))
        };
        let take = self.source_analysis(clip.source, clip.norm, clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))?;
        let fit = crate::melody::fit(&take, region, &notes);
        Ok((crate::model::Melody { channel, span, notes }, fit))
    }

    /// Channels whose notes play over an audio clip: `(channel, notes, fit 0..1)`, best fit first.
    pub fn melody_channels(&self, clip_id: Id) -> Result<Vec<(Id, usize, f32)>, EngineError> {
        let clip = self.audio_clip(clip_id)?;
        let (sets, region) = {
            let c = self.core.lock();
            let sets: Vec<(Id, Vec<MelodyNote>)> = crate::melody::channels_over(&c.project, &clip)
                .into_iter()
                .map(|(id, _)| (id, crate::melody::collect(&c.project, &clip, id, None)))
                .collect();
            (sets, clip.source_span(c.project.bpm))
        };
        if sets.is_empty() {
            return Ok(vec![]);
        }
        let take = self.source_analysis(clip.source, clip.norm, clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))?;
        let mut out: Vec<(Id, usize, f32)> = sets.iter().map(|(id, ns)| (*id, ns.len(), crate::melody::fit(&take, region, ns))).collect();
        out.sort_by(|a, b| b.2.total_cmp(&a.2).then(b.1.cmp(&a.1)));
        Ok(out)
    }

    /// Uses `est` for pitch analysis from now on (`None` = built-in YIN). Pitch-edited
    /// clips are re-rendered with the new analysis.
    pub fn set_f0_estimator(&self, est: Option<Arc<dyn crate::pitch::F0Estimator>>) {
        let mut c = self.core.lock();
        c.sources.set_estimator(est);
        self.sync(&mut c, Dirty { timeline: true, mixer: false, params: false });
    }

    /// Name of the pitch detector in use ("yin" or the estimator's).
    pub fn f0_backend(&self) -> &'static str {
        self.core.lock().sources.backend()
    }

    fn source_music(&self, id: Id, norm: bool, rev: bool) -> Option<Arc<crate::music::SourceMusic>> {
        if let Some(m) = self.core.lock().sources.cached_music(id, norm, rev) {
            return Some(m);
        }
        let audio = self.core.lock().sources.variant(id, norm, rev)?;
        let m = crate::music::analyze_audio(&audio);
        self.core.lock().sources.put_music(id, norm, rev, m.clone());
        Some(m)
    }

    /// Tempo and key of an audio clip, or key of the project's audio clips
    /// (excluding muted tracks/clips and tracks routed to `exclude_fx`'s insert,
    /// so key detection for Pitch Correct listens to the backing rather than the vocal).
    pub fn music_analysis(&self, clip_id: Option<Id>, exclude_fx: Option<Id>) -> Result<crate::music::MusicAnalysis, EngineError> {
        self.music_analysis_ex(clip_id, exclude_fx, None)
    }

    /// [`Engine::music_analysis`], also excluding the tracks routed to insert `exclude_insert`.
    fn music_analysis_ex(
        &self,
        clip_id: Option<Id>,
        exclude_fx: Option<Id>,
        exclude_insert: Option<Id>,
    ) -> Result<crate::music::MusicAnalysis, EngineError> {
        if let Some(cid) = clip_id {
            let (clip, bpm) = {
                let c = self.core.lock();
                let clip = c.project.clips.iter().find(|x| x.id == cid).cloned().ok_or(EngineError::Msg("no such clip".into()))?;
                (clip, c.project.bpm)
            };
            if clip.kind != ClipKind::Audio {
                return Err(EngineError::Msg("only audio clips have tempo and key".into()));
            }
            let m = self.source_music(clip.source, clip.norm, clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))?;
            let (a, b) = clip.source_span(bpm);
            let (mut cs, n) = m.chroma_sum(a, b);
            if n == 0 {
                (cs, _) = m.chroma_sum(0.0, 1e9);
            }
            let key = crate::music::estimate_key(&cs, m.tuning_cents);
            return Ok(crate::music::MusicAnalysis { tempo: m.tempo.clone(), key });
        }

        let (all_clips, excluded_insert, bpm) = {
            let c = self.core.lock();
            let p = &c.project;
            let ex_ins = exclude_insert.or_else(|| exclude_fx.and_then(|fid| p.find_fx(fid).map(|(ins, _)| ins.id)));
            let any_solo = p.tracks.iter().any(|t| t.solo);
            let active: Vec<(Clip, bool)> = p
                .clips
                .iter()
                .filter(|cl| cl.kind == ClipKind::Audio && !cl.muted)
                .filter_map(|cl| {
                    let tr = p.tracks.get(cl.track as usize)?;
                    if tr.muted || (any_solo && !tr.solo) {
                        return None;
                    }
                    let is_excluded = ex_ins == Some(tr.insert);
                    Some((cl.clone(), is_excluded))
                })
                .collect();
            (active, ex_ins, p.bpm)
        };
        let use_non_excluded = excluded_insert.is_some() && all_clips.iter().any(|(_, ex)| !*ex);
        let chosen: Vec<Clip> = all_clips.into_iter().filter(|(_, ex)| !use_non_excluded || !*ex).map(|(cl, _)| cl).collect();
        let mut total_chroma = [0f32; 12];
        let mut tuning_sum = 0f32;
        let mut weight_sum = 0f32;
        let mut best_tempo: Option<(f64, crate::music::TempoEstimate)> = None;
        for cl in chosen {
            let Some(m) = self.source_music(cl.source, cl.norm, cl.rev) else { continue };
            let (a, b) = cl.source_span(bpm);
            let (cs, n) = m.chroma_sum(a, b);
            if n > 0 {
                for k in 0..12 {
                    total_chroma[k] += cs[k];
                }
                tuning_sum += m.tuning_cents * n as f32;
                weight_sum += n as f32;
            }
            if let Some(ref t) = m.tempo {
                let score = cl.len * t.confidence.max(0.1) as f64;
                if best_tempo.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
                    best_tempo = Some((score, t.clone()));
                }
            }
        }
        let tuning = if weight_sum > 0.0 { tuning_sum / weight_sum } else { 0.0 };
        let key = crate::music::estimate_key(&total_chroma, tuning);
        Ok(crate::music::MusicAnalysis { tempo: best_tempo.map(|(_, t)| t), key })
    }

    /// The mixer insert the clip is heard on (its own, else its track's); `None` when the
    /// clip does not exist. What Auto Vocal's chain replaces the effects of.
    pub fn clip_insert(&self, clip_id: Id) -> Option<Id> {
        let c = self.core.lock();
        let clip = c.project.clips.iter().find(|x| x.id == clip_id)?;
        Some(c.project.clip_insert(clip))
    }

    /// "Auto vocal": measures the vocal clip (what it plays, its region only) and returns
    /// the calibrated insert chain for its track. The key comes from `beat_clip` when given,
    /// else from the project's other audio (tracks not routed to the vocal's insert); the
    /// level suggestion needs `beat_clip`. Slow (seconds): call off the UI thread.
    ///
    /// `bake_rider`: the plan's rider comes out of the chain into `rider_bake`, for
    /// [`Self::bake_rider`] (no live look-ahead on the insert, so no playback delay).
    pub fn auto_vocal_plan(
        &self,
        clip_id: Id,
        beat_clip: Option<Id>,
        style: crate::autovocal::Style,
        bake_rider: bool,
    ) -> Result<crate::autovocal::AutoVocalPlan, EngineError> {
        let clip = self.audio_clip(clip_id)?;
        let region = |cl: &Clip| -> Result<(Vec<f32>, f32), EngineError> {
            let ((audio, rate), bpm) = {
                let mut c = self.core.lock();
                let bpm = c.project.bpm;
                (c.sources.clip_audio(cl, false, bpm).ok_or(EngineError::Msg("audio not loaded".into()))?, bpm)
            };
            let mono = crate::sources::mono_of(&audio);
            let sr = audio.sr as f64;
            let (a, b) = crate::sources::clip_region(cl, bpm, rate, mono.len() as f64 / sr);
            let (i0, i1) = ((a * sr) as usize, ((b * sr) as usize).min(mono.len()));
            let gain = crate::dsp::db_to_gain(cl.gain);
            Ok((mono[i0.min(i1)..i1].iter().map(|v| v * gain).collect(), audio.sr as f32))
        };
        let (vocal, sr) = region(&clip)?;
        if vocal.len() < (sr * 1.0) as usize {
            return Err(EngineError::Msg("the clip is too short to analyse (under 1 s)".into()));
        }
        let vocal_insert = {
            let c = self.core.lock();
            Some(c.project.clip_insert(&clip))
        };
        let key = match beat_clip {
            Some(b) => self.music_analysis(Some(b), None)?.key,
            None => self.music_analysis_ex(None, None, vocal_insert).ok().and_then(|m| m.key),
        };
        let pitch = self.pitch_analysis(clip_id).ok();
        let beat = match beat_clip {
            Some(b) => {
                let bc = {
                    let c = self.core.lock();
                    c.project.clips.iter().find(|x| x.id == b).cloned().ok_or(EngineError::Msg("no such beat clip".into()))?
                };
                Some(region(&bc)?)
            }
            None => None,
        };
        let plan = crate::autovocal::plan(&crate::autovocal::PlanInput {
            vocal: &vocal,
            sr,
            style,
            key,
            pitch: pitch.as_deref(),
            beat: beat.as_ref().map(|(x, s)| (x.as_slice(), *s)),
        });
        Ok(if bake_rider { plan.baked() } else { plan })
    }

    /// Renders what an audio clip plays (its region, clip gain, normalise/reverse and pitch
    /// edits; not its fades, which stay on the clip) through the Vocal Rider offline, as
    /// `autovocal::render` does — the rider's latency removed, sample-aligned, any
    /// look-ahead — into a new source, and points the clip at it (gain 0 dB, no
    /// norm/rev/pitch edits, the region's sub-sample start kept as the offset). The original
    /// source stays in the project. One undo step. `params` are Rider params (catalog ids;
    /// missing ones take the catalog defaults), e.g. an auto-vocal plan's `rider_bake`.
    /// Returns (new source id, clip id). Slow (the whole region is rendered): call it off
    /// the UI thread. `if_revision` is checked under the commit lock, so a project that
    /// moved on while the rider was rendering refuses the bake before anything is written.
    pub fn bake_rider(
        &self,
        clip_id: Id,
        params: &std::collections::BTreeMap<String, f64>,
        if_revision: Option<u64>,
    ) -> Result<(Id, Id), EngineError> {
        let (clip, audio, bpm, src_name, color) = {
            let mut c = self.core.lock();
            let clip = c.project.clips.iter().find(|x| x.id == clip_id).cloned().ok_or(EngineError::Msg("no such clip".into()))?;
            if clip.kind != ClipKind::Audio {
                return Err(EngineError::Msg("only audio clips can be baked".into()));
            }
            let bpm = c.project.bpm;
            let (audio, rate) = c.sources.clip_audio(&clip, false, bpm).ok_or(EngineError::Msg("audio not loaded".into()))?;
            if clip.needs_stretch() && rate != clip.rendered_rate() {
                return Err(EngineError::Msg("the clip's time-stretch could not be rendered".into()));
            }
            let src = c.project.sources.iter().find(|s| s.id == clip.source);
            let (name, color) = src.map(|s| (s.name.clone(), s.color.clone())).unwrap_or_else(|| ("Vocal".into(), "#e05263".into()));
            (clip, (audio, rate), bpm, name, color)
        };
        // a resampled clip bakes its source region, and keeps playing it resampled
        let (audio, rate) = audio;
        let sr = audio.sr;
        let (a, b) = crate::sources::clip_region(&clip, bpm, rate, audio.seconds());
        // the same frames `auto_vocal_plan` measures (truncated start); the fraction of a
        // frame the region starts past it stays on the clip as its offset
        let pos = a * sr as f64;
        let i0 = (pos as usize).min(audio.frames);
        let i1 = ((b * sr as f64) as usize).clamp(i0, audio.frames);
        if i1 - i0 < 16 {
            return Err(EngineError::Msg("the clip is empty".into()));
        }
        // two guard frames (as in `flatten_comp`): playback reads a frame or two past the
        // region's last whole frame (interpolation, the clip end landing between frames)
        let i1 = (i1 + 2).min(audio.frames);
        let g = crate::dsp::db_to_gain(clip.gain);
        let content = audio.content();
        let l: Vec<f32> = (i0..i1).map(|i| content[2 * i] * g).collect();
        let r: Vec<f32> = (i0..i1).map(|i| content[2 * i + 1] * g).collect();
        let (yl, yr) = crate::autovocal::render_stereo(FxKind::Rider, params, &l, &r, sr as f32);
        let data: Vec<f32> = yl.iter().zip(&yr).flat_map(|(a, b)| [*a, *b]).collect();
        let spb = 60.0 / bpm;
        // beats of the clip (a resampled clip reads `rate` source beats per clip beat)
        let offset = (pos - i0 as f64) / sr as f64 / spb / rate;
        let (stretch, resample) = if rate != 1.0 { (clip.stretch, clip.resample) } else { (1.0, false) };

        let mut c = self.core.lock();
        if let Some(r) = if_revision {
            if c.revision != r {
                return Err(EngineError::Msg(format!("{ERR_REVISION_CONFLICT} project is at revision {}, not {r}", c.revision)));
            }
        }
        let now = c.project.clips.iter().find(|x| x.id == clip_id).cloned();
        if now.as_ref() != Some(&clip) || c.project.bpm != bpm {
            return Err(EngineError::Msg("the clip changed while the rider was rendering; try again".into()));
        }
        let id = c.project.alloc_id();
        let name = format!("{src_name} (rider)");
        let path = fresh_path(&audio_dir(&c), &format!("{} rider {}", sanitize(&src_name), id));
        write_wav(&path, &data, sr)?;
        let baked = Audio::from_stereo(data, sr, audio.channels);
        let before = c.project.clone();
        c.project.sources.push(Source {
            id,
            name,
            color,
            path: path.display().to_string(),
            dur: baked.seconds(),
            sr,
            ch: audio.channels,
            peak_db: crate::dsp::gain_to_db(baked.peak()),
        });
        c.sources.insert(id, Arc::new(baked));
        if let Some(x) = c.project.clips.iter_mut().find(|x| x.id == clip_id) {
            // a time-stretched bake holds the stretched audio: it plays as it is
            *x = Clip { source: id, offset, gain: 0.0, norm: false, rev: false, tune: None, stretch, resample, ..clip };
        }
        push_undo(&mut c, before);
        c.revision += 1;
        self.sync(&mut c, Dirty { timeline: true, ..Default::default() });
        self.changed(c);
        Ok((id, clip_id))
    }
}

/// See [`Engine::pitch_view`].
pub struct PitchView {
    pub clip: Clip,
    pub analysis: Arc<crate::pitch::PitchAnalysis>,
    pub target: Option<Arc<crate::reference::PitchTarget>>,
    pub notes: Vec<crate::model::NoteEdit>,
    /// The notes are the clip's own edits (else the analysis' segmentation, unedited).
    pub own: bool,
    /// MIDI per analysis frame as it will sound (NaN = unvoiced).
    pub out: Vec<f32>,
    pub bpm: f64,
    pub sr: u32,
    /// Length of the source, seconds.
    pub seconds: f64,
}

impl PitchView {
    /// The part of the source the clip plays, seconds (clamped to the audio).
    pub fn region(&self) -> (f64, f64) {
        let (a, b) = self.clip.source_span(self.bpm);
        (a.clamp(0.0, self.seconds), b.clamp(0.0, self.seconds).max(a.clamp(0.0, self.seconds)))
    }
    /// Timeline beat of a source second of this clip.
    pub fn beat_of(&self, t: f64) -> f64 {
        self.clip.start + t * self.clip.stretch / (60.0 / self.bpm.max(1.0)) - self.clip.offset
    }
    pub fn spans(&self) -> Vec<(f64, f64)> {
        self.notes.iter().map(|n| (n.start, n.end)).collect()
    }
}
