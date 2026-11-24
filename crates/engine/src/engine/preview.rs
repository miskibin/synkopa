//! One-shot previews and auditions: clips, files, sources, notes and patterns.

use super::*;

/// Most [min,max] pairs a preview's waveform carries: enough for a status line, small on the wire.
pub const PREVIEW_WAVE_PAIRS: usize = 1024;

/// What a previewed file or source looks like, for the toolbar's status line: its length and its
/// waveform as [min,max] pairs over the whole length (at most [`PREVIEW_WAVE_PAIRS`]).
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct PreviewWave {
    pub secs: f64,
    pub peaks: Vec<f32>,
}

/// The waveform of `a` in at most `PREVIEW_WAVE_PAIRS` pairs: neighbouring peak bins merged (min of
/// mins, max of maxes), so a short click keeps its 100/s detail and a long loop stays small.
pub(crate) fn preview_wave(a: &Audio) -> PreviewWave {
    let n = a.peaks.len() / 2;
    let secs = if a.sr > 0 { a.frames as f64 / a.sr as f64 } else { 0.0 };
    if n <= PREVIEW_WAVE_PAIRS {
        return PreviewWave { secs, peaks: a.peaks[..n * 2].to_vec() };
    }
    let mut peaks = Vec::with_capacity(PREVIEW_WAVE_PAIRS * 2);
    for i in 0..PREVIEW_WAVE_PAIRS {
        let (lo, hi) = (i * n / PREVIEW_WAVE_PAIRS, ((i + 1) * n / PREVIEW_WAVE_PAIRS).max(i * n / PREVIEW_WAVE_PAIRS + 1));
        let (mut mn, mut mx) = (0f32, 0f32);
        for j in lo..hi.min(n) {
            mn = mn.min(a.peaks[j * 2]);
            mx = mx.max(a.peaks[j * 2 + 1]);
        }
        peaks.push(mn);
        peaks.push(mx);
    }
    PreviewWave { secs, peaks }
}

impl Engine {
    /// Plays one clip alone on the output. `from` (seconds of the source) starts there and plays
    /// the take with its pitch edits at the source's own speed (the pitch editor's Solo), to the
    /// end of the clip's region; without it the whole clip plays as the playlist would.
    pub fn preview_clip(&self, clip_id: Id, from: Option<f64>) -> Result<(), EngineError> {
        let mut c = self.core.lock();
        let clip = c.project.clips.iter().find(|x| x.id == clip_id).cloned().ok_or(EngineError::Msg("no such clip".into()))?;
        let bpm = c.project.bpm;
        // a time-stretch that is not rendered yet previews the source at its own speed
        let ready = if from.is_none() && clip.needs_stretch() {
            c.sources.stretch_request(&clip, true, bpm).and_then(Result::ok).filter(|x| x.1 == clip.rendered_rate())
        } else {
            None
        };
        let (audio, (a, b)) = match ready {
            Some((a, rate)) => {
                let r = crate::sources::clip_region(&clip, bpm, rate, a.seconds());
                (a, r)
            }
            None => {
                (c.sources.clip_source_audio(&clip, false, bpm).ok_or(EngineError::Msg("audio not loaded".into()))?, clip.source_span(bpm))
            }
        };
        let sr = audio.sr as f64;
        let end = (b * sr).min(audio.frames as f64);
        let pos = from.map_or(a, |f| f.clamp(a, b)) * sr;
        if pos >= end {
            return Err(EngineError::Msg("nothing to play after that point".into()));
        }
        let pv = Preview {
            pos,
            start: a * sr,
            end,
            gain: 10f32.powf(clip.gain / 20.0),
            // a start inside the clip is not its beginning: no fade-in there
            fade_in: if from.is_some_and(|f| f > a + 1e-3) { 0.0 } else { clip.fade_in as f64 },
            fade_out: clip.fade_out as f64,
            rate: 1.0,
            audio,
        };
        send(&mut c, Cmd::Preview(Some(Box::new(pv))));
        Ok(())
    }

    /// Plays an audio clip's sample at MIDI `key`, like a sampler would (the sample editor's
    /// keyboard, FL's audio clip channel): from the start of the clip's region to its end, read
    /// faster or slower by the distance from the clip's root key plus its pitch knob. Its own
    /// speed — the clip's time-stretch does not apply. Not an edit.
    pub fn preview_clip_key(&self, clip_id: Id, key: u8, vel: f32) -> Result<(), EngineError> {
        let mut c = self.core.lock();
        let clip = c.project.clips.iter().find(|x| x.id == clip_id).cloned().ok_or(EngineError::Msg("no such clip".into()))?;
        if clip.kind != crate::model::ClipKind::Audio {
            return Err(EngineError::Msg("not an audio clip".into()));
        }
        let bpm = c.project.bpm;
        let audio = c.sources.clip_source_audio(&clip, false, bpm).ok_or(EngineError::Msg("audio not loaded".into()))?;
        let (a, b) = clip.source_span(bpm);
        let sr = audio.sr as f64;
        let end = (b * sr).min(audio.frames as f64);
        if a * sr >= end {
            return Err(EngineError::Msg("the clip's region is empty".into()));
        }
        let semis = key.min(127) as f64 - clip.root as f64 + clip.pitch as f64;
        let pv = Preview {
            pos: a * sr,
            start: a * sr,
            end,
            gain: 10f32.powf(clip.gain / 20.0) * vel.clamp(0.0, 1.0),
            fade_in: clip.fade_in as f64,
            fade_out: clip.fade_out as f64,
            rate: 2f64.powf(semis / 12.0),
            audio,
        };
        send(&mut c, Cmd::Preview(Some(Box::new(pv))));
        Ok(())
    }

    /// Plays a file alone on the output (the Browser's preview); returns its waveform for the toolbar.
    pub fn preview_file(&self, path: &Path) -> Result<PreviewWave, EngineError> {
        let audio = Arc::new(match path.to_str() {
            Some(s) if s.starts_with(crate::factory::SCHEME) => Audio::load_path(s, Path::new("."))?,
            _ => Audio::load(path)?,
        });
        let wave = preview_wave(&audio);
        let mut c = self.core.lock();
        let end = audio.frames as f64;
        send(
            &mut c,
            Cmd::Preview(Some(Box::new(Preview { audio, pos: 0.0, start: 0.0, end, gain: 0.8, fade_in: 0.0, fade_out: 0.0, rate: 1.0 }))),
        );
        Ok(wave)
    }

    pub fn preview_stop(&self) {
        let mut c = self.core.lock();
        send(&mut c, Cmd::Preview(None));
    }

    /// Plays a project source alone (Browser ▸ Project ▸ Audio), from its loaded audio: no
    /// file I/O, and a recorded take with no file of its own yet plays too.
    pub fn preview_source(&self, id: Id) -> Result<PreviewWave, EngineError> {
        let mut c = self.core.lock();
        let audio = c.sources.get(id).cloned().ok_or(EngineError::Msg("audio not loaded".into()))?;
        let end = audio.frames as f64;
        let wave = preview_wave(&audio);
        send(
            &mut c,
            Cmd::Preview(Some(Box::new(Preview { audio, pos: 0.0, start: 0.0, end, gain: 0.8, fade_in: 0.0, fade_out: 0.0, rate: 1.0 }))),
        );
        Ok(wave)
    }

    /// Panic: cut everything still ringing after a stop (see [`Cmd::Silence`]). Not an edit.
    pub fn silence(&self) {
        let mut c = self.core.lock();
        send(&mut c, Cmd::Silence);
    }

    /// Plays one note of a channel right away (step clicks, the piano-roll keyboard, Ctrl+click
    /// on a channel). `pattern`: the pattern being edited, so a channel that follows its clip's
    /// track is heard through that track. Not an edit.
    pub fn note_audition(&self, chan: Id, key: Option<u8>, vel: f32, pattern: Option<Id>) -> Result<(), EngineError> {
        let mut c = self.core.lock();
        let ch = c.project.channel(chan).ok_or(EngineError::Msg(format!("no channel {chan}")))?;
        let key = key.unwrap_or(ch.root).min(127);
        // a one-shot rings out by itself; held sounds get a short note
        let len = 0.35;
        send(&mut c, Cmd::Audition { chan, key, vel: vel.clamp(0.0, 1.0), len, pattern });
        Ok(())
    }

    /// Loops one pattern on its own (`None` stops), from `from` beats into it and within
    /// `range` (beats; `None` = the whole pattern; kept inside the pattern). Starting it stops
    /// the song; starting the song stops it (they are exclusive, like FL Studio's PAT / SONG).
    /// Starting the pattern that plays again jumps to `from`. Not an edit.
    pub fn pattern_play(&self, pattern: Option<Id>, from: f64, range: Option<(f64, f64)>) -> Result<(), EngineError> {
        let mut c = self.core.lock();
        if let Some(id) = pattern {
            if !c.project.patterns.iter().any(|p| p.id == id) {
                return Err(EngineError::Msg(format!("no pattern {id}")));
            }
            let song_notes = {
                let t = self.midi_take.lock();
                t.rec.is_some() && t.pat.is_none()
            };
            if c.take.is_some() || song_notes {
                return Err(EngineError::Msg("recording: stop it first".into()));
            }
        }
        if !from.is_finite() || range.is_some_and(|(a, b)| !a.is_finite() || !b.is_finite()) {
            return Err(EngineError::Msg("pattern position: not a number".into()));
        }
        // notes recorded over the loop so far join their pattern before it stops or restarts
        let recorded = self.finish_pat_take(&mut c);
        c.pat_gen += 1; // the recorder armed below accepts notes only once this loop is live
        let gen = c.pat_gen;
        send(&mut c, Cmd::PatPlay { id: pattern, from: from.max(0.0), range, gen });
        c.pat_loop = pattern.map(|id| (id, range));
        self.begin_pat_take(&mut c);
        if pattern.is_some() && c.transport.playing {
            c.transport.playing = false;
            self.changed(c);
        } else if recorded {
            self.changed(c);
        }
        Ok(())
    }

    /// Changes the range the pattern loop plays (`None` = the whole pattern) without
    /// restarting it: the position stays, or goes to the range's start when outside it.
    /// Does nothing while no pattern loops. Not an edit.
    pub fn pattern_loop(&self, range: Option<(f64, f64)>) -> Result<(), EngineError> {
        if range.is_some_and(|(a, b)| !a.is_finite() || !b.is_finite()) {
            return Err(EngineError::Msg("pattern loop: not a number".into()));
        }
        let mut c = self.core.lock();
        // notes recorded over the old range are folded into it before the range changes
        let recorded = self.finish_pat_take(&mut c);
        c.pat_gen += 1;
        let gen = c.pat_gen;
        send(&mut c, Cmd::PatRange(range, gen));
        if let Some(p) = c.pat_loop.as_mut() {
            p.1 = range;
        }
        self.begin_pat_take(&mut c);
        if recorded {
            self.changed(c);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio_of_secs(secs: f64, sr: u32) -> Audio {
        let frames = (secs * sr as f64) as usize;
        // a decaying 1 kHz tone: loud at the start, quiet at the end
        let mut data = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let t = i as f64 / sr as f64;
            let v = ((t * 1000.0 * std::f64::consts::TAU).sin() * (-t * 2.0).exp()) as f32;
            data.push(v);
            data.push(v);
        }
        Audio::from_stereo(data, sr, 2)
    }

    #[test]
    fn a_short_file_keeps_every_peak() {
        let a = audio_of_secs(1.5, 44_100);
        let w = preview_wave(&a);
        assert!((w.secs - 1.5).abs() < 1e-3);
        assert_eq!(w.peaks, a.peaks, "under the bound the 100/s peaks go as they are");
    }

    #[test]
    fn a_long_file_is_merged_down_keeping_the_loudest() {
        let a = audio_of_secs(30.0, 8_000);
        let w = preview_wave(&a);
        assert_eq!(w.peaks.len(), PREVIEW_WAVE_PAIRS * 2);
        let max_src = a.peaks.chunks(2).map(|p| p[1]).fold(0f32, f32::max);
        let max_w = w.peaks.chunks(2).map(|p| p[1]).fold(0f32, f32::max);
        assert!((max_src - max_w).abs() < 1e-6, "merging keeps the loudest peak ({max_src} vs {max_w})");
        assert!(w.peaks[1] > w.peaks[w.peaks.len() - 1], "the decay still reads left to right");
    }
}
