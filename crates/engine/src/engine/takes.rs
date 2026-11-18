//! Recording takes: audio and MIDI finalisation, live MIDI routing, pattern takes.

use super::*;

impl Engine {
    /// Finishes the running take. `Err`: at least one recorded pass could not be written to
    /// disk and is not in the project (passes that did write still landed, as one undo step).
    pub(crate) fn finish_take(&self) -> Result<(), EngineError> {
        let shared = self.shared();
        let mut c = self.core.lock();
        if let Some(t) = c.take.as_mut() {
            t.resolve(&shared);
        }
        let written = self.finalize_take(&mut c);
        if c.graph_deferred {
            self.sync(&mut c, Dirty { timeline: false, mixer: true, params: true });
        }
        written
    }

    pub(crate) fn finalize_take(&self, c: &mut Core) -> Result<(), EngineError> {
        let Some(mut take) = c.take.take() else { return Ok(()) };
        // whatever is still in the ring belongs to this recording
        if let Some(w) = c.wires.as_mut() {
            while let Ok(x) = w.record.pop() {
                take.push(x);
            }
        }
        if take.start.is_nan() {
            return Ok(()); // punched in, but the audio thread never started capturing
        }
        if self.midi_take.lock().played {
            return Ok(()); // the armed track was played on a MIDI keyboard: this is a MIDI take
        }
        let spb = 60.0 / c.project.bpm;
        let sr = take.sr as f64;
        // Latency compensation: input captured at frame `j` was sung to what was heard
        // `skip` frames earlier. Dropping the first `skip` frames moves the whole take
        // earlier; pass marks (written when the audio thread wrapped) then land exactly
        // on the loop start of what was heard, so they stay where they are.
        let skip = take.skip.min(take.frames());
        let data = &take.data[skip * 2..];
        let total = data.len() / 2;
        // one pass per loop cycle; a last pass cut off within a beat of the wrap is not a take
        let mut bounds = vec![0];
        bounds.extend(take.marks.iter().copied().take_while(|&m| m <= total));
        let complete = bounds.len() - 1;
        if bounds.last() != Some(&total) {
            bounds.push(total);
        }
        let mut passes = vec![];
        for k in 0..bounds.len() - 1 {
            let (f0, f1) = (bounds[k], bounds[k + 1]);
            let secs = f1.saturating_sub(f0) as f64 / sr;
            let cut_short = k > 0 && k >= complete;
            if secs < if cut_short { spb } else { 0.05 } {
                continue;
            }
            let start = if k == 0 { take.start } else { take.lap.map(|l| l.0).unwrap_or(take.start) };
            let full_len = take.lap.filter(|_| k < complete).map(|(_, end)| end - start);
            // Each loop boundary is rounded to a device frame. Normalize that rounding only;
            // a tempo or range edit during recording can change the actual pass length.
            let exact = full_len
                .map(|beats| (beats, (beats * spb * sr).round() as usize))
                .filter(|(_, frames)| *frames > 0 && frames.abs_diff(f1 - f0) <= 1);
            passes.push((start, exact, &data[f0 * 2..f1 * 2]));
        }
        if passes.is_empty() {
            return Ok(());
        }
        let before = c.project.clone();
        let dir = audio_dir(c);
        let mut landed = vec![];
        let mut failed = 0usize;
        let mut failures = vec![];
        for (start, exact, data) in passes {
            let mut data = data.to_vec();
            if let Some((_, frames)) = exact {
                data.resize(frames * 2, 0.0);
            }
            let secs = data.len() as f64 / 2.0 / sr;
            let id = c.project.alloc_id();
            let n = c.project.sources.iter().filter(|s| s.name.starts_with("Take")).count() + 1;
            let name = format!("Take {n}");
            let path = fresh_path(&dir, &format!("{} {}", name, id));
            if let Err(e) = write_wav(&path, &data, take.sr) {
                // a pass whose file did not land is not published either: a source that
                // points at a missing file would only be discovered at the next save or
                // reopen, while it plays from memory until then. Passes that did land stay.
                log::error!("could not write recording {}: {e}", path.display());
                failures.push(format!("{} ({e})", path.display()));
                failed += 1;
                continue;
            }
            let audio = Audio::from_stereo(data, take.sr, 1);
            c.project.sources.push(Source {
                id,
                name,
                color: "#e05263".into(),
                path: path.display().to_string(),
                dur: secs,
                sr: take.sr,
                ch: 1,
                peak_db: crate::dsp::gain_to_db(audio.peak()),
            });
            c.sources.insert(id, Arc::new(audio));
            landed.push((id, start, exact.map_or(secs / spb, |(beats, _)| beats).min(TOTAL_BEATS - start)));
        }
        if landed.is_empty() {
            // nothing landed: no undo step for a recording that never made it to disk
            return Err(EngineError::Msg(format!("could not write the recording: {}", failures.join("; "))));
        }
        crate::comp::land_takes(&mut c.project, take.track as u32, &landed);
        let written = if failed > 0 {
            Err(EngineError::Msg(format!(
                "could not write {} of {} recording pass(es): {}; the rest is in the project",
                failed,
                failed + landed.len(),
                failures.join("; ")
            )))
        } else {
            Ok(())
        };
        push_undo(c, before, Some("Record take".into()));
        c.revision += 1;
        self.sync(c, Dirty { timeline: true, ..Default::default() });
        written
    }

    // ------------------------------------------------------------ live MIDI

    /// Sets what the MIDI keyboard plays. `target`: the channel (instrument) to play, `None` =
    /// the first pitched channel. `live`: a device is connected; without one nothing is
    /// recorded and playing stops. Anything still sounding is released when this changes.
    pub fn midi_configure(&self, target: Option<Id>, live: bool) {
        let mut c = self.core.lock();
        let changed = c.midi.target != target || c.midi.live != live;
        c.midi.target = target;
        c.midi.live = live;
        self.refresh_midi(&c);
        self.midi_fast.live.store(live, Release);
        if changed {
            self.midi_fast.send(Cmd::MidiOff);
        }
    }

    /// The channel the keyboard plays right now (the chosen one, else the first pitched
    /// channel, else the first channel).
    pub fn midi_target(&self) -> Option<Id> {
        midi_channel(&self.core.lock())
    }

    /// A note from the computer keyboard used as a piano (FL "typing keyboard"): plays `chan`
    /// like a MIDI key, through the same lock-free ring, whether or not a MIDI device is connected.
    pub fn typing_note(&self, chan: Id, key: u8, vel: f32, on: bool) {
        let ins = match self.midi_fast.ins.load(Relaxed) {
            usize::MAX => None,
            i => Some(i),
        };
        let vel = if on { vel.clamp(0.0, 1.0) } else { 0.0 };
        let key = key.min(127);
        self.midi_fast.send(Cmd::MidiNote { chan, key, vel, on, ins: if on { ins } else { None } });
        // while a take records notes, the computer keyboard plays into it like a MIDI keyboard
        let (song, pat) = (self.shared().beat(), self.shared().pat_beat());
        let mut t = self.midi_take.lock();
        let beat = t.beat(song, pat);
        if let Some((r, _, _)) = t.rec.as_mut() {
            if on {
                r.note_on_ch(beat, key, (vel * 127.0).round() as u8, Some(chan));
                t.played = true;
            } else {
                r.note_off_ch(beat, key, Some(chan));
            }
        }
    }

    /// Releases every sounding keyboard note (device unplugged).
    pub fn midi_panic(&self) {
        self.midi_fast.send(Cmd::MidiOff);
    }

    /// Publishes the channel and insert the keyboard plays for the lock-free event path.
    pub(crate) fn refresh_midi(&self, c: &Core) {
        self.midi_fast.chan.store(midi_channel(c).unwrap_or(u64::MAX), Relaxed);
        self.midi_fast.ins.store(armed_insert(c).unwrap_or(usize::MAX), Relaxed);
    }

    /// One decoded message from the keyboard. Plays through the audio thread's MIDI ring
    /// (no allocation there) and, while a take is armed, goes into the recorder at the
    /// playhead. Cheap and never waits for the core lock (the project being edited, saved
    /// or rebuilt); safe to call from the MIDI input thread.
    pub fn midi_event(&self, m: MidiMsg) {
        let mf = &self.midi_fast;
        if !mf.live.load(Acquire) {
            return;
        }
        let chan = match mf.chan.load(Relaxed) {
            u64::MAX => None,
            id => Some(id),
        };
        let ins = match mf.ins.load(Relaxed) {
            usize::MAX => None,
            i => Some(i),
        };
        // one snapshot of the audio thread's position for the whole event: beats and the loop
        // generation must answer for the same instant, and every recorder input below decides
        // on that same generation (a recorder just armed — the audio thread still plays the
        // previous loop — must not timestamp anything on the wrong loop)
        let shared = self.shared();
        let (gen, song, pat) = (shared.pat_gen(), shared.beat(), shared.pat_beat());
        match m {
            MidiMsg::NoteOn { key, vel, .. } => {
                let Some(chan) = chan else { return };
                mf.send(Cmd::MidiNote { chan, key, vel: MidiMsg::vel_gain(vel), on: true, ins });
                let mut t = self.midi_take.lock();
                let beat = t.beat(song, pat);
                let this_loop = t.pat.as_ref().is_none_or(|p| p.gen == gen);
                if let Some((r, _, _)) = t.rec.as_mut() {
                    if this_loop {
                        r.note_on(beat, key, vel);
                        t.played = true;
                    }
                }
            }
            MidiMsg::NoteOff { key, .. } => {
                let Some(chan) = chan else { return };
                mf.send(Cmd::MidiNote { chan, key, vel: 0.0, on: false, ins: None });
                let mut t = self.midi_take.lock();
                let beat = t.beat(song, pat);
                let this_loop = t.pat.as_ref().is_none_or(|p| p.gen == gen);
                if let Some((r, _, _)) = t.rec.as_mut() {
                    if this_loop {
                        r.note_off(beat, key);
                    }
                }
            }
            MidiMsg::Sustain { down, .. } => {
                mf.send(Cmd::MidiSustain(down));
                let mut t = self.midi_take.lock();
                let beat = t.beat(song, pat);
                let this_loop = t.pat.as_ref().is_none_or(|p| p.gen == gen);
                if let Some((r, _, _)) = t.rec.as_mut() {
                    if this_loop {
                        r.sustain(beat, down);
                    }
                }
            }
            MidiMsg::PitchBend { value, .. } => mf.send(Cmd::MidiBend(MidiMsg::bend_semitones(value))),
            MidiMsg::ModWheel { value, .. } => mf.send(Cmd::MidiMod(value as f32 / 127.0)),
            MidiMsg::AllNotesOff { .. } => mf.send(Cmd::MidiOff),
        }
    }

    /// PAT mode with Record on and Notes ticked: what the keyboard plays over the pattern loop
    /// goes into that pattern (on the channel the keyboard plays), added to what it holds.
    pub(crate) fn begin_pat_take(&self, c: &mut Core) {
        let Some((id, range)) = c.pat_loop else { return };
        if !(c.transport.rec && c.transport.rec_notes) {
            return;
        }
        let Some(len) = c.project.patterns.iter().find(|p| p.id == id).map(|p| p.len) else { return };
        let Some(ch) = midi_channel(c) else { return };
        let (la, lb) = crate::rt::loop_span(range, len);
        let mut t = self.midi_take.lock();
        if t.rec.is_some() {
            return;
        }
        t.played = false;
        t.rec = Some((MidiRecorder::new(f64::NEG_INFINITY), usize::MAX, ch));
        t.pat = Some(PatLoopRec { id, la, span: lb - la, prev: f64::NEG_INFINITY, laps: 0.0, gen: c.pat_gen });
    }

    /// Ends a take over the pattern loop: the notes played join the pattern (one undo step).
    /// False when no such take runs.
    pub(crate) fn finish_pat_take(&self, c: &mut Core) -> bool {
        let shared = self.shared();
        // the playhead counts as this loop's end only while the audio thread plays it
        let pb =
            if shared.pat_gen() == self.midi_take.lock().pat.as_ref().map_or(0, |p| p.gen) { shared.pat_beat() } else { f64::NEG_INFINITY };
        let (rec, ch, mut pl) = {
            let mut t = self.midi_take.lock();
            let Some(pl) = t.pat.take() else { return false };
            t.played = false;
            let Some((rec, _, ch)) = t.rec.take() else { return true };
            (rec, ch, pl)
        };
        let end = pl.unwrap(pb);
        let mut played = rec.finish(end);
        // quantize in pattern beats: unwrapped beats are off the grid after the first pass
        // whenever the loop is not a whole number of grid steps
        if pl.span > 0.0 {
            for n in &mut played {
                let at = pl.la + (n.start - pl.la).rem_euclid(pl.span);
                n.end += at - n.start;
                n.start = at;
            }
        }
        midi::quantize(&mut played, c.transport.rec_quantize);
        let notes = midi::fold_loop(&played, ch, pl.la, pl.span);
        let Some(mut pat) = c.project.patterns.iter().find(|p| p.id == pl.id).cloned() else { return true };
        if notes.is_empty() {
            return true;
        }
        pat.notes.extend(notes);
        let before = c.project.clone();
        match ops::apply_all(&mut c.project, &[Op::PutPattern { pattern: pat }]) {
            Ok(d) => {
                push_undo(c, before, Some("Record notes".into()));
                c.revision += 1;
                self.sync(c, d);
            }
            Err(e) => log::error!("could not add the recorded notes to the pattern: {e}"),
        }
        true
    }

    /// Starts collecting keyboard notes for a take on armed track `track` from beat `from`.
    pub(crate) fn begin_midi_take(&self, c: &mut Core, track: usize, from: f64) {
        let mut t = self.midi_take.lock();
        t.played = false;
        if !c.transport.rec_notes {
            return;
        }
        if let Some(ch) = midi_channel(c) {
            t.rec = Some((MidiRecorder::new(from), track, ch));
        }
    }

    /// Ends the MIDI take at the playhead: the played notes become a pattern and a pattern
    /// clip on the armed track (one undo step). Nothing played = nothing added.
    pub(crate) fn finish_midi_take(&self) {
        let end = self.shared().beat();
        let mut c = self.core.lock();
        if self.finish_pat_take(&mut c) {
            return;
        }
        let Some((rec, track, ch)) = ({
            let mut t = self.midi_take.lock();
            t.played = false;
            t.rec.take()
        }) else {
            return;
        };
        let mut notes = rec.finish(end);
        midi::quantize(&mut notes, c.transport.rec_quantize);
        let Some(laid) = midi::layout(&notes, ch, 4.0) else { return };
        let color = c.project.channel(ch).map(|x| x.color.clone()).unwrap_or_else(|| "#69c3d6".into());
        let n = c.project.patterns.iter().filter(|p| p.name.starts_with("MIDI take")).count() + 1;
        let pid = c.project.alloc_id();
        let cid = c.project.alloc_id();
        let pattern =
            Pattern { id: pid, name: format!("MIDI take {n}"), color, len: laid.len, min_len: laid.len, notes: laid.notes, swing: 0.0 };
        let clip = Clip {
            id: cid,
            track: track as u32,
            start: laid.start,
            len: laid.len,
            offset: 0.0,
            kind: ClipKind::Pat,
            source: pid,
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
        };
        if let Err(e) = self.apply_locked(c, &[Op::PutPattern { pattern }, Op::PutClips { clips: vec![clip] }], false, None, None) {
            log::error!("could not place the MIDI take: {e}");
        }
    }
}
