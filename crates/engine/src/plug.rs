//! Plugin-instrument channels on the audio thread: one hosted [`Instrument`] per channel,
//! fed sample-accurate note events.
//!
//! [`PlugInst`] mirrors the built-in synth's instance API (`poly::SynthInst`): sequenced
//! notes are scheduled with a delay and a length in samples, live keys go down and up, and
//! the output lands where the latest note was played, scaled by the channel's gain and pan.
//! Everything is preallocated when the instance is made (off the audio thread); the audio
//! thread only moves events between fixed-capacity buffers.

use crate::model::Id;
use crate::processor::{InstEvent, InstMsg, Instrument, ProcessCtx, MAX_INST_EVENTS};
use crate::rt::{ChanRt, InsertRt};
use crate::synth::{pan_gains, Dest};

/// Most plugin-instrument channels the audio thread plays at once.
pub const MAX_PLUGS: usize = 64;
/// Scheduled events an instance holds ahead of time (a dense pattern block is far below).
const PENDING: usize = 4096;
/// Notes an instance tracks as sounding, so a stop can end every one of them.
const SOUNDING: usize = 512;

/// MIDI controllers the host sends.
pub const CC_MOD_WHEEL: u8 = 1;
pub const CC_SUSTAIN: u8 = 64;

/// One plugin-instrument channel's instance and its event schedule.
pub struct PlugInst {
    pub chan: Id,
    inst: Box<dyn Instrument>,
    /// Where the output goes: where the latest note was played.
    pub dest: Option<Dest>,
    /// Samples rendered so far (the schedule's clock).
    now: u64,
    /// Events not yet delivered, by time; events of one instant keep the order they were
    /// queued in (a note's off always follows its on; a retriggered key's old note ends first).
    pending: Vec<(u64, InstMsg)>,
    batch: Vec<InstEvent>,
    /// Notes the plugin is playing now (key, note id).
    sounding: Vec<(u8, i32)>,
    /// Live key → note id (0 = up).
    live: [i32; 128],
    next_id: i32,
    pedal: bool,
    bl: Vec<f32>,
    br: Vec<f32>,
    /// Output gains as the previous block played them (glide to the new ones).
    gain: (f32, f32),
    /// Level of the route the latest note took on top of the channel's (the fader of a
    /// track whose insert the channel skips).
    route: f32,
}

impl PlugInst {
    /// Wraps a prepared instrument for channel `chan` (allocates: call off the audio thread).
    pub fn new(chan: Id, inst: Box<dyn Instrument>, max_block: usize) -> Self {
        Self {
            chan,
            inst,
            dest: None,
            now: 0,
            pending: Vec::with_capacity(PENDING),
            batch: Vec::with_capacity(MAX_INST_EVENTS),
            sounding: Vec::with_capacity(SOUNDING),
            live: [0; 128],
            next_id: 1,
            pedal: false,
            bl: vec![0.0; max_block.max(1)],
            br: vec![0.0; max_block.max(1)],
            gain: (0.0, 0.0),
            route: 1.0,
        }
    }

    fn new_id(&mut self) -> i32 {
        let id = self.next_id;
        self.next_id = if id == i32::MAX { 1 } else { id + 1 };
        id
    }

    /// Queues `msg` at absolute time `at`; `false` (and nothing queued) when the schedule is full.
    fn push(&mut self, at: u64, msg: InstMsg) -> bool {
        if self.pending.len() >= self.pending.capacity() {
            return false;
        }
        let i = self.pending.partition_point(|(t, _)| *t <= at);
        self.pending.insert(i, (at, msg));
        true
    }

    /// A sequenced note: starts `delay` samples from now and lasts `len` samples. `route`: the
    /// level of where it plays on top of the channel's (see [`PlugInst::route`]).
    pub fn note(&mut self, delay: u32, len: u32, key: u8, vel: f32, dest: Dest, route: f32) {
        if self.pending.len() + 2 > self.pending.capacity() {
            return;
        }
        let id = self.new_id();
        let at = self.now + u64::from(delay);
        self.dest = Some(dest);
        self.route = if route.is_finite() { route.max(0.0) } else { 1.0 };
        let key = key.min(127);
        self.push(at, InstMsg::NoteOn { key, vel: vel.clamp(0.0, 1.0), id });
        self.push(at + u64::from(len.max(1)), InstMsg::NoteOff { key, id });
    }

    /// A live key went down; it sounds until [`Self::key_up`] (the plugin applies the pedal).
    pub fn key_down(&mut self, key: u8, vel: f32, dest: Dest) {
        let k = key as usize & 127;
        if self.pending.len() + 2 > self.pending.capacity() {
            return;
        }
        if self.live[k] != 0 {
            let old = self.live[k];
            self.push(self.now, InstMsg::NoteOff { key: k as u8, id: old });
        }
        let id = self.new_id();
        self.live[k] = id;
        self.dest = Some(dest);
        self.route = 1.0;
        self.push(self.now, InstMsg::NoteOn { key: k as u8, vel: vel.clamp(0.0, 1.0), id });
    }

    pub fn key_up(&mut self, key: u8) {
        let k = key as usize & 127;
        let id = std::mem::take(&mut self.live[k]);
        if id != 0 {
            self.push(self.now, InstMsg::NoteOff { key: k as u8, id });
        }
    }

    pub fn sustain(&mut self, down: bool) {
        if self.pedal != down {
            self.pedal = down;
            self.push(self.now, InstMsg::Control { cc: CC_SUSTAIN, value: if down { 1.0 } else { 0.0 } });
        }
    }

    /// Pitch bend in semitones within the MIDI bend range (`midi::BEND_RANGE`).
    pub fn set_bend(&mut self, st: f32) {
        let v = (0.5 + st / (2.0 * crate::midi::BEND_RANGE)).clamp(0.0, 1.0);
        self.push(self.now, InstMsg::Bend(v));
    }

    pub fn set_wheel(&mut self, v: f32) {
        self.push(self.now, InstMsg::Control { cc: CC_MOD_WHEEL, value: v.clamp(0.0, 1.0) });
    }

    /// Releases every live key, the pedal and the bend.
    pub fn all_notes_off(&mut self) {
        for k in 0..128 {
            let id = std::mem::take(&mut self.live[k]);
            if id != 0 {
                self.push(self.now, InstMsg::NoteOff { key: k as u8, id });
            }
        }
        self.sustain(false);
        self.push(self.now, InstMsg::Bend(0.5));
    }

    /// Transport stop or jump: nothing scheduled plays any more, and every sounding note
    /// gets its note-off now (its release still rings, as on a hardware synth).
    pub fn stop_all(&mut self) {
        self.pending.clear();
        self.live = [0; 128];
        for i in 0..self.sounding.len() {
            let (key, id) = self.sounding[i];
            self.pending.push((self.now, InstMsg::NoteOff { key, id }));
        }
        if self.pedal {
            self.pedal = false;
            self.pending.push((self.now, InstMsg::Control { cc: CC_SUSTAIN, value: 0.0 }));
        }
    }

    /// Notes the plugin is playing now.
    pub fn sounding(&self) -> usize {
        self.sounding.len()
    }

    /// Renders `n` frames and hands them to `sink`, scaled by the output gains (gliding from
    /// the previous call's).
    pub fn process(&mut self, n: usize, ctx: &ProcessCtx, gl: f32, gr: f32, mut sink: impl FnMut(usize, &[f32], &[f32])) {
        let max = self.bl.len();
        let (gl, gr) = (gl * self.route, gr * self.route);
        let mut done = 0;
        let (g0l, g0r) = self.gain;
        while done < n {
            let m = (n - done).min(max);
            let end = self.now + m as u64;
            // this chunk's events, oldest first; what does not fit waits for the next chunk
            self.batch.clear();
            let mut take = 0;
            while take < self.pending.len() && self.pending[take].0 < end && self.batch.len() < self.batch.capacity() {
                let (at, msg) = self.pending[take];
                let offset = at.saturating_sub(self.now) as u32;
                self.batch.push(InstEvent { offset, msg });
                take += 1;
            }
            self.pending.drain(..take);
            for e in &self.batch {
                match e.msg {
                    InstMsg::NoteOn { key, id, .. } => {
                        if self.sounding.len() < self.sounding.capacity() {
                            self.sounding.push((key, id));
                        }
                    }
                    InstMsg::NoteOff { id, .. } => {
                        if let Some(i) = self.sounding.iter().position(|s| s.1 == id) {
                            self.sounding.swap_remove(i);
                        }
                    }
                    _ => {}
                }
            }
            let beat = ctx.beat + done as f64 / f64::from(ctx.sample_rate.max(1.0)) * ctx.bpm / 60.0;
            let c = ProcessCtx { beat, ..*ctx };
            let (l, r) = (&mut self.bl[..m], &mut self.br[..m]);
            self.inst.process(&self.batch, l, r, &c);
            let inv = 1.0 / n as f32;
            for i in 0..m {
                let t = (done + i + 1) as f32 * inv;
                let (a, b) = (l[i], r[i]);
                // a misbehaving plugin must not poison the mix
                let (a, b) = (if a.is_finite() { a } else { 0.0 }, if b.is_finite() { b } else { 0.0 });
                l[i] = a * (g0l + (gl - g0l) * t);
                r[i] = b * (g0r + (gr - g0r) * t);
            }
            sink(done, l, r);
            self.now = end;
            done += m;
        }
        self.gain = (gl, gr);
    }
}

/// The audio thread's plugin-instrument instances, by channel id.
pub struct PlugBank {
    // boxed: an instance arrives in a command and leaves as garbage by pointer, never copied
    #[allow(clippy::vec_box)]
    insts: Vec<Box<PlugInst>>,
}

impl Default for PlugBank {
    fn default() -> Self {
        Self { insts: Vec::with_capacity(MAX_PLUGS) }
    }
}

impl PlugBank {
    /// Takes an instance; returns what has to be thrown away (a replaced one, or this one
    /// when the bank is full).
    pub fn add(&mut self, inst: Box<PlugInst>) -> Option<Box<PlugInst>> {
        if let Some(i) = self.insts.iter().position(|x| x.chan == inst.chan) {
            return Some(std::mem::replace(&mut self.insts[i], inst));
        }
        if self.insts.len() < self.insts.capacity() {
            self.insts.push(inst);
            None
        } else {
            Some(inst)
        }
    }

    pub fn remove(&mut self, chan: Id) -> Option<Box<PlugInst>> {
        let i = self.insts.iter().position(|x| x.chan == chan)?;
        Some(self.insts.swap_remove(i))
    }

    pub fn get(&mut self, chan: Id) -> Option<&mut PlugInst> {
        self.insts.iter_mut().find(|x| x.chan == chan).map(|b| &mut **b)
    }

    pub fn len(&self) -> usize {
        self.insts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.insts.is_empty()
    }

    pub fn key_up(&mut self, chan: Id, key: u8) {
        if let Some(s) = self.get(chan) {
            s.key_up(key);
        }
    }

    pub fn sustain(&mut self, down: bool) {
        self.insts.iter_mut().for_each(|s| s.sustain(down));
    }

    pub fn set_bend(&mut self, st: f32) {
        self.insts.iter_mut().for_each(|s| s.set_bend(st));
    }

    pub fn set_wheel(&mut self, v: f32) {
        self.insts.iter_mut().for_each(|s| s.set_wheel(v));
    }

    pub fn all_notes_off(&mut self) {
        self.insts.iter_mut().for_each(|s| s.all_notes_off());
    }

    pub fn stop_all(&mut self) {
        self.insts.iter_mut().for_each(|s| s.stop_all());
    }

    /// Adds every instance's output into where its notes were played (a track buffer or an
    /// insert input), scaled by its channel's gain and pan. An instance that has not played
    /// a note yet still runs (a plugin may sound by itself), into nowhere.
    pub fn render(&mut self, n: usize, tracks: &mut [(Vec<f32>, Vec<f32>)], inserts: &mut [InsertRt], chans: &[ChanRt], ctx: &ProcessCtx) {
        for s in self.insts.iter_mut() {
            let (g, pan) = chans.iter().find(|c| c.id == s.chan).map(|c| (if c.on { c.gain } else { 0.0 }, c.pan)).unwrap_or((0.0, 0.0));
            let (pl, pr) = pan_gains(pan);
            let out: Option<(&mut [f32], &mut [f32])> = match s.dest {
                Some(Dest::Track(t)) => tracks.get_mut(t).map(|(l, r)| (&mut l[..], &mut r[..])),
                Some(Dest::Insert(k)) => inserts.get_mut(k).map(|i| (&mut i.l[..], &mut i.r[..])),
                _ => None,
            };
            match out {
                Some((l, r)) => s.process(n, ctx, g * pl, g * pr, |o, a, b| {
                    for i in 0..a.len() {
                        l[o + i] += a[i];
                        r[o + i] += b[i];
                    }
                }),
                None => s.process(n, ctx, 0.0, 0.0, |_, _, _| {}),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records what it is sent, outputs a constant.
    struct Probe {
        log: Log,
        t: u64,
    }

    impl Instrument for Probe {
        fn prepare(&mut self, _: f32, _: usize) {}
        fn process(&mut self, events: &[InstEvent], l: &mut [f32], r: &mut [f32], _: &ProcessCtx) {
            let mut log = self.log.lock();
            for e in events {
                assert!((e.offset as usize) < l.len(), "offset {} in a block of {}", e.offset, l.len());
                log.push((self.t, *e));
            }
            l.fill(0.5);
            r.fill(0.25);
            self.t += l.len() as u64;
        }
    }

    type Log = std::sync::Arc<parking_lot::Mutex<Vec<(u64, InstEvent)>>>;

    fn probe(max_block: usize) -> (PlugInst, Log) {
        let log = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
        (PlugInst::new(7, Box::new(Probe { log: log.clone(), t: 0 }), max_block), log)
    }

    fn ctx() -> ProcessCtx {
        ProcessCtx { sample_rate: 48_000.0, bpm: 120.0, beat: 0.0, playing: true }
    }

    /// Absolute sample time and message of everything delivered.
    fn times(log: &[(u64, InstEvent)]) -> Vec<(u64, InstMsg)> {
        log.iter().map(|(t, e)| (t + u64::from(e.offset), e.msg)).collect()
    }

    #[test]
    fn notes_arrive_sample_accurate_across_block_boundaries() {
        let (mut p, log) = probe(64);
        p.note(100, 30, 60, 0.8, Dest::Insert(0), 1.0);
        p.note(127, 1, 62, 1.0, Dest::Insert(0), 1.0);
        for _ in 0..4 {
            p.process(64, &ctx(), 1.0, 1.0, |_, _, _| {});
        }
        let got = times(&log.lock());
        assert_eq!(
            got,
            vec![
                (100, InstMsg::NoteOn { key: 60, vel: 0.8, id: 1 }),
                (127, InstMsg::NoteOn { key: 62, vel: 1.0, id: 2 }),
                (128, InstMsg::NoteOff { key: 62, id: 2 }),
                (130, InstMsg::NoteOff { key: 60, id: 1 }),
            ]
        );
        assert_eq!(p.sounding(), 0);
    }

    #[test]
    fn a_long_host_block_is_split_into_the_instruments_block_size() {
        let (mut p, log) = probe(32);
        p.note(70, 10, 60, 1.0, Dest::Insert(0), 1.0);
        let mut seen = vec![];
        p.process(100, &ctx(), 1.0, 1.0, |o, a, _| seen.push((o, a.len())));
        assert_eq!(seen, vec![(0, 32), (32, 32), (64, 32), (96, 4)]);
        assert_eq!(times(&log.lock())[0].0, 70);
    }

    #[test]
    fn overlapping_notes_of_one_key_end_by_their_own_ids() {
        let (mut p, log) = probe(256);
        p.note(0, 100, 60, 1.0, Dest::Insert(0), 1.0);
        p.note(50, 100, 60, 1.0, Dest::Insert(0), 1.0);
        p.process(256, &ctx(), 1.0, 1.0, |_, _, _| {});
        let offs: Vec<_> = times(&log.lock()).into_iter().filter(|(_, m)| matches!(m, InstMsg::NoteOff { .. })).collect();
        assert_eq!(offs, vec![(100, InstMsg::NoteOff { key: 60, id: 1 }), (150, InstMsg::NoteOff { key: 60, id: 2 })]);
    }

    #[test]
    fn a_stop_ends_sounding_notes_and_drops_scheduled_ones() {
        let (mut p, log) = probe(64);
        p.note(0, 10_000, 60, 1.0, Dest::Insert(0), 1.0);
        p.note(5_000, 10, 64, 1.0, Dest::Insert(0), 1.0);
        p.sustain(true);
        p.process(64, &ctx(), 1.0, 1.0, |_, _, _| {});
        assert_eq!(p.sounding(), 1);
        p.stop_all();
        p.process(64, &ctx(), 1.0, 1.0, |_, _, _| {});
        for _ in 0..200 {
            p.process(64, &ctx(), 1.0, 1.0, |_, _, _| {});
        }
        let got = times(&log.lock());
        assert!(got.contains(&(64, InstMsg::NoteOff { key: 60, id: 1 })), "{got:?}");
        assert!(got.contains(&(64, InstMsg::Control { cc: CC_SUSTAIN, value: 0.0 })), "{got:?}");
        assert!(!got.iter().any(|(_, m)| matches!(m, InstMsg::NoteOn { key: 64, .. })), "the scheduled note never starts");
        assert_eq!(p.sounding(), 0);
    }

    #[test]
    fn live_keys_retrigger_and_controllers_are_mapped() {
        let (mut p, log) = probe(64);
        p.key_down(60, 0.5, Dest::Insert(1));
        p.key_down(60, 0.7, Dest::Insert(1));
        p.set_bend(crate::midi::BEND_RANGE);
        p.set_wheel(0.25);
        p.key_up(60);
        p.process(64, &ctx(), 1.0, 1.0, |_, _, _| {});
        let got: Vec<_> = times(&log.lock()).into_iter().map(|(_, m)| m).collect();
        assert_eq!(
            got,
            vec![
                InstMsg::NoteOn { key: 60, vel: 0.5, id: 1 },
                InstMsg::NoteOff { key: 60, id: 1 },
                InstMsg::NoteOn { key: 60, vel: 0.7, id: 2 },
                InstMsg::Bend(1.0),
                InstMsg::Control { cc: CC_MOD_WHEEL, value: 0.25 },
                InstMsg::NoteOff { key: 60, id: 2 },
            ]
        );
        assert_eq!(p.sounding(), 0);
        assert_eq!(p.dest, Some(Dest::Insert(1)));
    }

    #[test]
    fn output_glides_to_the_channel_gain_and_drops_non_finite_samples() {
        struct Nan;
        impl Instrument for Nan {
            fn prepare(&mut self, _: f32, _: usize) {}
            fn process(&mut self, _: &[InstEvent], l: &mut [f32], r: &mut [f32], _: &ProcessCtx) {
                l.fill(f32::NAN);
                r.fill(1.0);
            }
        }
        let mut p = PlugInst::new(1, Box::new(Nan), 16);
        let mut last = (0.0, 0.0);
        p.process(16, &ctx(), 0.5, 0.5, |_, a, b| last = (a[15], b[15]));
        assert_eq!(last, (0.0, 0.5));
        let mut first = 0.0;
        p.process(16, &ctx(), 1.0, 1.0, |_, _, b| first = b[0]);
        assert!(first > 0.5 && first < 1.0, "glides from 0.5: {first}");
    }

    #[test]
    fn a_full_schedule_refuses_rather_than_allocates() {
        let (mut p, _) = probe(64);
        for i in 0..(PENDING as u32) {
            p.note(i, 1, 60, 1.0, Dest::Insert(0), 1.0);
        }
        assert_eq!(p.pending.len(), PENDING);
        assert_eq!(p.pending.capacity(), PENDING);
    }
}
