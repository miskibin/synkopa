//! The sampler, the piano voices and the metronome click, rendered sample by sample from a
//! fixed voice pool.
//!
//! Every note voice belongs to a channel: an index into the timeline's channel table
//! (re-mapped by channel id when a new timeline arrives). The voice reads its channel's
//! gain, pan and mute every block, so Channel Rack knobs also move notes that are ringing.

use crate::model::Id;
use crate::rt::{ChanRt, InsertRt};

pub const MAX_VOICES: usize = 128;

/// How a scheduled note ends. Sampler triggers have no note-off; explicit held sampler notes
/// stop at their end (envelope off) or release from it (envelope on), while auditions and
/// legacy notes ring out (envelope off) or release (envelope on).
#[derive(Clone, Copy)]
pub enum NoteEnd {
    Natural,
    Release(f32),
    Timed(f32),
}

impl NoteEnd {
    pub fn seconds(self) -> Option<f32> {
        match self {
            Self::Natural => None,
            Self::Release(s) | Self::Timed(s) => Some(s),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VoiceKind {
    /// The sample of the voice's channel, pitched by the note and shaped by the channel's
    /// sampler settings (part of the file, direction, fades, volume envelope).
    Sample,
    /// Synkopa Keys: a region of the channel's instrument (see `crate::keys`), picked by key,
    /// velocity and round robin; it rings for the note's length under its envelope, then
    /// releases over the region's (or the channel's) release time.
    Keys,
    /// A synth channel: its notes go to the channel's `poly::SynthInst`, not this pool.
    Synth,
    /// A Music Box channel: a modeled comb tine (see `crate::musicbox`).
    MusicBox,
    /// A plugin-instrument channel: its notes go to the channel's `plug::PlugInst`.
    Plugin,
    Click {
        accent: bool,
    },
}

/// Where a voice plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dest {
    /// A playlist track (then its fader and its insert).
    Track(usize),
    /// Straight into a mixer insert (by index).
    Insert(usize),
    /// The metronome bus.
    Click,
}

/// Voice without a channel (the metronome).
pub const NO_CHAN: usize = usize::MAX;

#[derive(Clone, Copy)]
pub struct Voice {
    pub kind: VoiceKind,
    pub dest: Dest,
    /// Index into the channel table, or [`NO_CHAN`].
    pub chan: usize,
    /// Id of that channel, to find it again in a rebuilt table.
    pub chan_id: Id,
    /// Note velocity (gain).
    vel: f32,
    /// The note's own pan, added to its channel's (-1..=1).
    pan: f32,
    /// Sampler: read position in source frames and step per output sample.
    pos: f64,
    rate: f64,
    /// Samples to wait before the voice starts (sub-block accuracy).
    pub delay: u32,
    t: u32,
    /// Note length in samples until release. A step trigger uses `u32::MAX`.
    len: u32,
    timed: bool,
    phase: f32,
    pub alive: bool,
    /// Live (MIDI) note whose key is still down: it sounds until released.
    held: bool,
    /// Key let go while the sustain pedal was down: released when the pedal comes up.
    sus: bool,
    /// MIDI key of a live note (and of every piano note).
    key: u8,
    /// Keys: the bank's zone (region) this voice plays.
    zone: u16,
    /// Keys: the note's envelope (after its per-note modulations), its level now, its stage
    /// (0 delay, 1 attack, 2 hold, 3 decay, 4 sustain) and the samples spent in that stage.
    eg: crate::sfz::Eg,
    lvl: f32,
    stage: u8,
    stage_t: u32,
    /// Keys: a key-release voice (it plays its noise to the end and is never damped), and the
    /// velocity the note was struck with (its release noise follows it).
    rel: bool,
    note_vel: f32,
    /// Music Box: the sounding note's tine model.
    pub mb: crate::musicbox::Tine,
    /// Release (and cut) level: 1 until the note ends, then it falls.
    env: f32,
    /// Sampler: the channel's cut group when the note started (0 = none), and the voice's
    /// own time at which a note of that group cuts it (`u32::MAX` = not cut).
    cut: u8,
    cut_at: u32,
    /// Sampler in slice mode: the slice this note plays ([`NO_SLICE`] = the channel's region).
    slice: u16,
    /// Slide notes: the pitch offset in semitones now, where its glide ends, the step per
    /// sample and samples left of it.
    st: f32,
    st_goal: f32,
    st_step: f32,
    st_left: u32,
    /// A slide waiting to start: the voice's own time it starts at (`u32::MAX` = none), the
    /// offset it glides to and over how many samples.
    slide_at: u32,
    slide_to: f32,
    slide_len: u32,
}

impl Voice {
    /// One sample of a slide: the playback-rate factor of the pitch offset now.
    #[inline]
    fn slide_tick(&mut self) -> f64 {
        if self.t == self.slide_at {
            self.slide_at = u32::MAX;
            self.st_goal = self.slide_to;
            if self.slide_len == 0 {
                self.st = self.slide_to;
                self.st_left = 0;
            } else {
                self.st_left = self.slide_len;
                self.st_step = (self.slide_to - self.st) / self.slide_len as f32;
            }
        }
        if self.st_left > 0 {
            self.st_left -= 1;
            self.st = if self.st_left == 0 { self.st_goal } else { self.st + self.st_step };
        }
        if self.st == 0.0 {
            1.0
        } else {
            (self.st as f64 / 12.0).exp2()
        }
    }
}

/// A sampler voice that plays the channel's region, not a slice.
const NO_SLICE: u16 = u16::MAX;

impl Voice {
    fn dead() -> Self {
        Self {
            kind: VoiceKind::Sample,
            dest: Dest::Click,
            chan: NO_CHAN,
            chan_id: 0,
            vel: 1.0,
            pan: 0.0,
            pos: 0.0,
            rate: 1.0,
            delay: 0,
            t: 0,
            len: 0,
            timed: false,
            phase: 0.0,
            alive: false,
            held: false,
            sus: false,
            key: 0,
            zone: 0,
            eg: crate::sfz::Eg { delay: 0.0, attack: 0.0, hold: 0.0, decay: 0.0, sustain: 1.0, release: 0.0 },
            lvl: 0.0,
            stage: 0,
            stage_t: 0,
            rel: false,
            note_vel: 0.0,
            mb: crate::musicbox::Tine::default(),
            env: 1.0,
            cut: 0,
            cut_at: u32::MAX,
            slice: NO_SLICE,
            st: 0.0,
            st_goal: 0.0,
            st_step: 0.0,
            st_left: 0,
            slide_at: u32::MAX,
            slide_to: 0.0,
            slide_len: 0,
        }
    }
}

/// Output level of Synkopa Keys against its recordings (−12 dB). The Salamander bank is
/// levelled hot: one full-velocity note peaks at +1 dBFS, ten keys at +10.5 dBFS. With this
/// headroom a loud note sits near −11 dBFS and a ten-note chord near −1.8 dBFS, so chords sum
/// linearly instead of being squashed by a limiter (which made them pump and change tone).
pub const KEYS_HEADROOM: f32 = 0.251;

pub struct VoicePool {
    pub voices: Vec<Voice>,
    sr: f32,
    /// Sustain pedal state (live MIDI).
    pedal: bool,
    /// Pitch-bend factor applied to live notes (1 = none).
    bend: f32,
    /// Keys: notes struck so far per key (round robins rotate on it), a random generator
    /// (`lorand`/`hirand`), and the keyswitch each channel last pressed.
    rr: [u32; 128],
    rng: u32,
    switches: [(Id, u8); 16],
}

/// A live note's length: it lasts until its key comes up.
const LIVE_SECONDS: f32 = 3600.0;
/// No channel in a keyswitch slot.
const NO_ID: Id = Id::MAX;

#[inline]
fn exp_ramp(v0: f32, v1: f32, t: f32, dur: f32) -> f32 {
    v0 * (v1 / v0).powf((t / dur).min(1.0))
}

pub fn mtof(key: f32) -> f32 {
    440.0 * 2f32.powf((key - 69.0) / 12.0)
}

/// Balance pan law shared with the mixer: the far side is attenuated linearly.
#[inline]
pub fn pan_gains(pan: f32) -> (f32, f32) {
    (if pan > 0.0 { 1.0 - pan } else { 1.0 }, if pan < 0.0 { 1.0 + pan } else { 1.0 })
}

/// Ends a held voice: its release (the channel's release time) starts now. A sampler with its
/// envelope off ignores it and rings out.
fn release(v: &mut Voice) {
    v.held = false;
    v.sus = false;
    v.len = v.t;
}

/// Time for a cut voice to fall 60 dB: fast, but not a click.
const CUT_SECONDS: f32 = 0.006;
/// Fade at a trimmed edge of a sample, where the waveform does not start or end at zero.
const EDGE_SECONDS: f32 = 0.0015;
/// Fades at the start and the end of a slice, seconds.
const SLICE_HEAD_SECONDS: f32 = 0.0004;
const SLICE_TAIL_SECONDS: f32 = 0.005;

impl VoicePool {
    pub fn new(sr: f32) -> Self {
        Self {
            voices: vec![Voice::dead(); MAX_VOICES],
            sr,
            pedal: false,
            bend: 1.0,
            rr: [0; 128],
            rng: 0x9e37_79b9,
            switches: [(NO_ID, 0); 16],
        }
    }

    pub fn set_sr(&mut self, sr: f32) {
        self.sr = sr;
        self.clear();
    }

    pub fn clear(&mut self) {
        self.voices.iter_mut().for_each(|v| v.alive = false);
        self.pedal = false;
        self.bend = 1.0;
        self.rr = [0; 128];
        self.switches = [(NO_ID, 0); 16];
    }

    /// A live key went down: the note sounds until [`Self::note_off`] (and the pedal) let it go.
    /// Same routing and voices as a sequenced note; percussion and samples ring out by themselves.
    pub fn note_on(&mut self, ch: &ChanRt, idx: usize, dest: Dest, key: u8, vel: f32) -> bool {
        // the same key struck again releases its previous note
        self.release_where(|v| v.held && v.chan_id == ch.id && v.key == key);
        let Some(i) = self.note_slot(ch, idx, dest, 0, NoteEnd::Release(LIVE_SECONDS), key, vel, 1.0, 0.0) else { return false };
        let v = &mut self.voices[i];
        v.held = true;
        v.key = key;
        true
    }

    /// A live key went up. With the pedal down the note keeps sounding until it comes up.
    /// A Keys note that ends now plays its instrument's key-release noise.
    pub fn note_off(&mut self, chans: &[ChanRt], chan_id: Id, key: u8) {
        let pedal = self.pedal;
        let mut ended: Option<Voice> = None;
        for v in self.voices.iter_mut().filter(|v| v.alive && v.held && v.chan_id == chan_id && v.key == key) {
            if pedal {
                v.sus = true;
            } else {
                if v.kind == VoiceKind::Keys && !v.rel && ended.is_none() {
                    ended = Some(*v);
                }
                release(v);
            }
        }
        if let Some(v) = ended {
            self.keys_release(chans, &v);
        }
    }

    pub fn sustain(&mut self, chans: &[ChanRt], down: bool) {
        self.pedal = down;
        if down {
            return;
        }
        // the dampers fall on every key the pedal was holding: one release noise per key
        let mut done: [(Id, u128); 8] = [(NO_ID, 0); 8];
        for i in 0..self.voices.len() {
            let v = self.voices[i];
            if !(v.alive && v.sus) {
                continue;
            }
            release(&mut self.voices[i]);
            if v.kind != VoiceKind::Keys || v.rel {
                continue;
            }
            let bit = 1u128 << (v.key & 127);
            match done.iter().position(|d| d.0 == v.chan_id).or_else(|| done.iter().position(|d| d.0 == NO_ID)) {
                Some(s) if done[s].1 & bit != 0 => {}
                Some(s) => {
                    done[s] = (v.chan_id, done[s].1 | bit);
                    self.keys_release(chans, &v);
                }
                None => self.keys_release(chans, &v),
            }
        }
    }

    /// Starts the key-release regions of the Keys note `v`, which ends now.
    fn keys_release(&mut self, chans: &[ChanRt], v: &Voice) {
        let Some(ch) = chans.get(v.chan).filter(|c| c.id == v.chan_id) else { return };
        let Some(bank) = ch.keys.clone() else { return };
        if !bank.has_release(v.key) {
            return;
        }
        let held = v.t as f32 / self.sr;
        let rand = self.rand();
        let sw = self.switch(ch.id).or(ch.keys_sw);
        let rr = self.rr[v.key as usize & 127];
        let (dest, chan, key, pan, vel) = (v.dest, v.chan, v.key, v.pan, v.note_vel);
        bank.pick(crate::sfz::Trigger::Release, key, vel, rr, rand, sw, ch.keys_dynamics, held, |h| {
            self.keys_zone(ch, &bank, chan, dest, 0, NoteEnd::Natural, key, vel, 1.0, pan, h, true);
        });
    }

    fn rand(&mut self) -> f32 {
        // xorshift32: deterministic and allocation-free
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    fn switch(&self, chan_id: Id) -> Option<u8> {
        self.switches.iter().find(|s| s.0 == chan_id).map(|s| s.1)
    }

    fn set_switch(&mut self, chan_id: Id, key: u8) {
        let i = self.switches.iter().position(|s| s.0 == chan_id).or_else(|| self.switches.iter().position(|s| s.0 == NO_ID)).unwrap_or(0);
        self.switches[i] = (chan_id, key);
    }

    /// Starts one zone of a Keys note; returns its voice slot.
    #[allow(clippy::too_many_arguments)]
    fn keys_zone(
        &mut self,
        ch: &ChanRt,
        bank: &crate::keys::KeysBank,
        idx: usize,
        dest: Dest,
        delay: u32,
        end: NoteEnd,
        key: u8,
        vel: f32,
        gain: f32,
        pan: f32,
        h: crate::keys::Hit,
        rel: bool,
    ) -> Option<usize> {
        let z = bank.zone(h.zone)?;
        // a group's note silences the voices that are `off_by` it (an open hi-hat by a closed one)
        if z.group != 0 {
            for v in self.voices.iter_mut().filter(|v| v.alive && v.kind == VoiceKind::Keys && v.chan_id == ch.id && v.cut_at == u32::MAX) {
                if bank.zone(v.zone).is_some_and(|o| o.off_by == z.group) {
                    v.cut_at = v.t.saturating_add(delay.saturating_sub(v.delay));
                }
            }
        }
        let rate = bank.pcm(z).sr as f64 / self.sr as f64 * 2f64.powf(h.semis as f64 / 12.0);
        // a one-shot region plays to its end whatever the note's length
        let end = if z.loop_mode == crate::sfz::LoopMode::OneShot { NoteEnd::Natural } else { end };
        let i = self.trigger(VoiceKind::Keys, dest, idx, ch.id, delay, end, h.gain * gain * KEYS_HEADROOM, rate);
        let v = &mut self.voices[i];
        v.zone = h.zone;
        v.key = key;
        v.pan = (pan + h.pan).clamp(-1.0, 1.0);
        v.pos = z.offset;
        v.eg = h.eg;
        v.rel = rel;
        v.note_vel = vel;
        Some(i)
    }

    /// Pitch bend of live notes, in semitones.
    pub fn set_bend(&mut self, semitones: f32) {
        self.bend = 2f32.powf(semitones / 12.0);
    }

    /// Lets go of every live note and the pedal (device unplugged, panic).
    pub fn all_notes_off(&mut self) {
        self.pedal = false;
        self.bend = 1.0;
        self.release_where(|v| v.held);
    }

    fn release_where(&mut self, f: impl Fn(&Voice) -> bool) {
        for v in self.voices.iter_mut().filter(|v| v.alive && f(v)) {
            release(v);
        }
    }

    /// A new channel table arrived: point every voice at its channel's new index, and
    /// silence voices whose channel is gone. RT-safe (no allocation).
    pub fn remap(&mut self, chans: &[ChanRt]) {
        // the project's articulation is the truth again: a keyswitch played live lasts until the next edit
        self.switches = [(NO_ID, 0); 16];
        for v in self.voices.iter_mut().filter(|v| v.alive && v.chan != NO_CHAN) {
            match chans.iter().position(|c| c.id == v.chan_id) {
                Some(i) => v.chan = i,
                None => v.alive = false,
            }
        }
    }

    /// The metronome click.
    pub fn click(&mut self, accent: bool, delay: u32) {
        self.trigger(VoiceKind::Click { accent }, Dest::Click, NO_CHAN, 0, delay, NoteEnd::Release(0.06), 1.0, 1.0);
    }

    /// A note of channel `ch` (index `idx` in the table). `key` is a MIDI key, `vel` the
    /// velocity 0..1 and `gain` level on top of it (the track gain when the channel skips the track).
    /// Returns false when nothing can play (a sampler channel without audio).
    #[allow(clippy::too_many_arguments)]
    pub fn note(&mut self, ch: &ChanRt, idx: usize, dest: Dest, delay: u32, end: NoteEnd, key: u8, vel: f32, gain: f32, pan: f32) -> bool {
        self.note_slot(ch, idx, dest, delay, end, key, vel, gain, pan).is_some()
    }

    /// A slide note of channel `chan_id`, `delay` samples from now: the channel's sampler and
    /// piano voices that sound then (started, not yet at their end) glide by the distance from
    /// the highest of them to `key`, over `len` samples. A slice plays at the file's pitch, so
    /// it does not slide.
    pub fn slide(&mut self, chan_id: Id, delay: u32, key: u8, len: u32) {
        let at = |v: &Voice| v.t.saturating_add(delay - v.delay);
        let sounding = |v: &Voice| {
            v.alive
                && v.chan_id == chan_id
                && matches!(v.kind, VoiceKind::Sample | VoiceKind::Keys | VoiceKind::MusicBox)
                && !v.rel
                && v.slice == NO_SLICE
                && v.delay <= delay
                && v.cut_at == u32::MAX
                && at(v) < v.len
        };
        // where each voice is headed: a slide still waiting, else the end of its glide
        let goal = |v: &Voice| if v.slide_at != u32::MAX { v.slide_to } else { v.st_goal };
        let Some(top) = self.voices.iter().filter(|v| sounding(v)).map(|v| v.key as f32 + goal(v)).max_by(f32::total_cmp) else {
            return;
        };
        let by = key as f32 - top;
        for v in self.voices.iter_mut().filter(|v| sounding(v)) {
            // a second slide before the first began: the first jumps to its end there
            if v.slide_at != u32::MAX {
                v.st = v.slide_to;
                v.st_goal = v.slide_to;
                v.st_left = 0;
            }
            v.slide_at = at(v);
            v.slide_to = goal(v) + by;
            v.slide_len = len;
        }
    }

    /// Like [`Self::note`], returning the voice slot that was started.
    #[allow(clippy::too_many_arguments)]
    fn note_slot(
        &mut self,
        ch: &ChanRt,
        idx: usize,
        dest: Dest,
        delay: u32,
        end: NoteEnd,
        key: u8,
        vel: f32,
        gain: f32,
        pan: f32,
    ) -> Option<usize> {
        let level = vel * gain;
        match ch.kind {
            VoiceKind::Sample => {
                let a = ch.sample.as_ref()?;
                let m = &ch.smp;
                // slice mode: the key picks the slice (root = the first), the pitch stays the file's
                let slice = match &ch.slices {
                    Some(sl) => {
                        let i = key as i32 - ch.root as i32;
                        if i < 0 || i as usize >= sl.len() {
                            return None;
                        }
                        // a new slice cuts the one sounding (chops play one at a time, as on an MPC)
                        for v in self.voices.iter_mut().filter(|v| {
                            v.alive && v.kind == VoiceKind::Sample && v.chan_id == ch.id && v.slice != NO_SLICE && v.cut_at == u32::MAX
                        }) {
                            v.cut_at = v.t.saturating_add(delay.saturating_sub(v.delay));
                        }
                        i as u16
                    }
                    None => NO_SLICE,
                };
                let pitch = if slice == NO_SLICE { key as f64 - ch.root as f64 } else { 0.0 };
                let rate = a.sr as f64 / self.sr as f64 * 2f64.powf(pitch / 12.0) * m.rate;
                // a note of a cut group silences what the group is playing, when it starts
                if m.cut != 0 {
                    for v in
                        self.voices.iter_mut().filter(|v| v.alive && v.kind == VoiceKind::Sample && v.cut == m.cut && v.cut_at == u32::MAX)
                    {
                        v.cut_at = v.t.saturating_add(delay.saturating_sub(v.delay));
                    }
                }
                let i = self.trigger(VoiceKind::Sample, dest, idx, ch.id, delay, end, level, rate);
                let (lo, hi) = match &ch.slices {
                    Some(sl) if slice != NO_SLICE => slice_region(sl, slice, a.frames),
                    _ => region(m.start, m.end, a.frames),
                };
                let v = &mut self.voices[i];
                v.pos = if m.reverse { hi - 1.0 } else { lo };
                v.cut = m.cut;
                v.key = key;
                v.slice = slice;
                v.pan = pan;
                Some(i)
            }
            VoiceKind::Keys => {
                let bank = ch.keys.clone()?;
                if bank.is_switch(key) {
                    // a keyswitch picks the articulation; it does not sound
                    self.set_switch(ch.id, key);
                    return None;
                }
                // the same key struck again: the string's previous note is damped
                for v in
                    self.voices.iter_mut().filter(|v| v.alive && v.kind == VoiceKind::Keys && !v.rel && v.chan_id == ch.id && v.key == key)
                {
                    v.held = false;
                    v.sus = false;
                    v.len = v.len.min(v.t.saturating_add(delay.saturating_sub(v.delay)));
                }
                let k = key as usize & 127;
                let rr = self.rr[k];
                self.rr[k] = rr.wrapping_add(1);
                let rand = self.rand();
                // a keyswitch played live, else the channel's articulation, else the auto one by length
                let sw = self.switch(ch.id).or(ch.keys_sw).or_else(|| {
                    ch.keys_auto.map(|a| match end.seconds().filter(|s| *s < LIVE_SECONDS) {
                        Some(s) if s < a.seconds => a.short,
                        _ => a.long,
                    })
                });
                let mut first = None;
                bank.pick(crate::sfz::Trigger::Attack, key, vel, rr, rand, sw, ch.keys_dynamics, 0.0, |h| {
                    let i = self.keys_zone(ch, &bank, idx, dest, delay, end, key, vel, gain, pan, h, false);
                    first = first.or(i);
                });
                // a sequenced note knows when it ends: its key-release noise is scheduled with it
                // (a live note plays it at its note-off)
                if let Some(s) = end.seconds().filter(|s| *s < LIVE_SECONDS) {
                    if first.is_some() && bank.has_release(key) {
                        let at = delay.saturating_add((s * self.sr) as u32);
                        bank.pick(crate::sfz::Trigger::Release, key, vel, rr, rand, sw, ch.keys_dynamics, s, |h| {
                            self.keys_zone(ch, &bank, idx, dest, at, NoteEnd::Natural, key, vel, gain, pan, h, true);
                        });
                    }
                }
                first
            }
            VoiceKind::MusicBox => {
                let p = crate::musicbox::Params {
                    ring: ch.mb_ring,
                    brightness: ch.mb_brightness,
                    body: ch.mb_body,
                    mechanical: ch.mb_mechanical,
                    spread: ch.mb_spread,
                    release: ch.mb_release,
                };
                // the same key struck again: the pin catches the tine and damps it — at the new
                // note's start, not here (the new voice begins after its delay)
                for v in self.voices.iter_mut().filter(|v| v.alive && v.kind == VoiceKind::MusicBox && v.chan_id == ch.id && v.key == key) {
                    v.held = false;
                    v.sus = false;
                    v.len = v.len.min(v.t.saturating_add(delay.saturating_sub(v.delay)));
                }
                // velocity is the gain: a zero-velocity note is silent, and above that it shapes
                // the timbre (a harder pluck excites the overtones) on top of the level
                if vel <= 0.0 {
                    return None;
                }
                let level = vel.clamp(0.0, 1.0).powf(1.5) * gain * crate::musicbox::HEADROOM;
                let tine = crate::musicbox::Tine::new(key, vel, &p, self.sr);
                let i = self.trigger(VoiceKind::MusicBox, dest, idx, ch.id, delay, end, level, 1.0);
                let v = &mut self.voices[i];
                v.mb = tine;
                v.key = key;
                // the pattern's own pan plus the comb's spread across the image
                v.pan = (pan + tine.pan).clamp(-1.0, 1.0);
                Some(i)
            }
            VoiceKind::Synth | VoiceKind::Plugin | VoiceKind::Click { .. } => None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn trigger(&mut self, kind: VoiceKind, dest: Dest, chan: usize, chan_id: Id, delay: u32, end: NoteEnd, vel: f32, rate: f64) -> usize {
        // steal the oldest voice if the pool is full
        let idx = match self.voices.iter().position(|v| !v.alive) {
            Some(i) => i,
            None => (0..self.voices.len()).max_by_key(|&i| self.voices[i].t).unwrap_or(0),
        };
        self.voices[idx] = Voice {
            kind,
            dest,
            chan,
            chan_id,
            vel: vel.max(0.0),
            rate,
            delay,
            len: end.seconds().map_or(u32::MAX, |seconds| (seconds * self.sr) as u32),
            timed: matches!(end, NoteEnd::Timed(_)),
            alive: true,
            ..Voice::dead()
        };
        idx
    }

    /// Adds every live voice into its destination: a track buffer, an insert's input, or
    /// the click bus. Samples are stereo (the channel's pan is a balance).
    pub fn render(
        &mut self,
        n: usize,
        tracks: &mut [(Vec<f32>, Vec<f32>)],
        inserts: &mut [InsertRt],
        click: &mut [f32],
        chans: &[ChanRt],
        track_dirty: &mut [bool],
    ) {
        let sr = self.sr;
        let bend = self.bend;
        for v in self.voices.iter_mut().filter(|v| v.alive) {
            let bend = if v.held { bend } else { 1.0 };
            let start = (v.delay as usize).min(n);
            v.delay = v.delay.saturating_sub(n as u32);
            // channel gain × velocity, split by the channel's pan
            let (g, pan, g0, pan0, sample, piano, mb) = match chans.get(v.chan) {
                Some(c) => {
                    let on = if c.on { v.vel } else { 0.0 } * if v.kind == VoiceKind::Sample { c.smp.gain } else { 1.0 };
                    let mb = matches!(c.kind, VoiceKind::MusicBox).then(|| crate::musicbox::Params {
                        ring: c.mb_ring,
                        brightness: c.mb_brightness,
                        body: c.mb_body,
                        mechanical: c.mb_mechanical,
                        spread: c.mb_spread,
                        release: c.mb_release,
                    });
                    (
                        c.gain * on,
                        c.pan,
                        c.gain0 * on,
                        c.pan0,
                        c.sample.as_deref().map(|a| (a, c.smp, c.slices.as_deref())),
                        c.keys.as_deref().map(|b| (b, c.keys_release)),
                        mb,
                    )
                }
                None if v.chan == NO_CHAN => (v.vel, 0.0, v.vel, 0.0, None, None, None),
                None => {
                    v.alive = false;
                    continue;
                }
            };
            // the note's own pan moves it from where the channel sits
            let (pan, pan0) = ((pan + v.pan).clamp(-1.0, 1.0), (pan0 + v.pan).clamp(-1.0, 1.0));
            let (pl, pr) = pan_gains(pan);
            let (gl, gr) = (g * pl, g * pr);
            // an automated channel gain or pan glides from the previous block's value
            let (pl0, pr0) = pan_gains(pan0);
            let (gl0, gr0) = (g0 * pl0, g0 * pr0);
            let inv_n = 1.0 / n.max(1) as f32;
            let side = |i: usize| -> (f32, f32) {
                let t = i as f32 * inv_n;
                (gl0 + (gl - gl0) * t, gr0 + (gr - gr0) * t)
            };
            let (out_l, out_r): (&mut [f32], &mut [f32]) = match v.dest {
                // the click bus is mono
                Dest::Click => (&mut click[..], &mut []),
                Dest::Track(t) => match tracks.get_mut(t) {
                    Some((l, r)) => {
                        track_dirty[t] = true;
                        (&mut l[..], &mut r[..])
                    }
                    None => {
                        v.alive = false;
                        continue;
                    }
                },
                Dest::Insert(k) => match inserts.get_mut(k) {
                    Some(ins) => {
                        ins.dirty = true;
                        (&mut ins.l[..], &mut ins.r[..])
                    }
                    None => {
                        v.alive = false;
                        continue;
                    }
                },
            };
            let stereo = !out_r.is_empty();
            if v.kind == VoiceKind::Keys {
                let Some((bank, release_override)) = piano else {
                    v.alive = false;
                    continue;
                };
                let Some(z) = bank.zone(v.zone) else {
                    v.alive = false;
                    continue;
                };
                let p = bank.pcm(z);
                // a key-release noise plays out; otherwise the channel's release (if set) or the region's
                let release = if release_override >= 0.0 { release_override } else { v.eg.release };
                // -60 dB after `release` seconds
                let coef = 10f32.powf(-3.0 / (release.max(0.005) * sr));
                let eg = v.eg;
                let (delay_n, attack_n, hold_n) = ((eg.delay * sr) as u32, (eg.attack * sr).max(0.001 * sr), (eg.hold * sr) as u32);
                // the decay falls 60 dB of the way to the sustain level in `decay` seconds
                let dcoef = if eg.decay > 0.0 { 10f32.powf(-3.0 / (eg.decay * sr)) } else { 0.0 };
                let looping = z.loop_len > 0.0;
                for i in start..n {
                    if looping && (z.loop_mode == crate::sfz::LoopMode::Continuous || v.t < v.len) && v.pos >= z.loop_start + z.loop_len {
                        v.pos -= z.loop_len * ((v.pos - z.loop_start) / z.loop_len).floor();
                    }
                    if v.pos >= z.end {
                        v.alive = false;
                        break;
                    }
                    if v.cut_at != u32::MAX && v.t >= v.cut_at {
                        v.env *= 10f32.powf(-3.0 / (CUT_SECONDS * sr));
                        if v.env < 1e-4 {
                            v.alive = false;
                            break;
                        }
                    } else if v.t >= v.len {
                        v.env *= coef;
                        if v.env < 1e-4 {
                            v.alive = false;
                            break;
                        }
                    } else {
                        // the envelope advances while the note is held; a release starts from its level
                        match v.stage {
                            0 => {
                                v.lvl = 0.0;
                                if v.stage_t >= delay_n {
                                    v.stage = 1;
                                    v.stage_t = 0;
                                }
                            }
                            1 => {
                                v.lvl = (v.stage_t as f32 / attack_n).min(1.0);
                                if v.lvl >= 1.0 {
                                    v.stage = 2;
                                    v.stage_t = 0;
                                }
                            }
                            2 => {
                                v.lvl = 1.0;
                                if v.stage_t >= hold_n {
                                    v.stage = 3;
                                    v.stage_t = 0;
                                }
                            }
                            3 => {
                                v.lvl = eg.sustain + (v.lvl - eg.sustain) * dcoef;
                                if (v.lvl - eg.sustain).abs() < 1e-4 {
                                    v.lvl = eg.sustain;
                                    v.stage = 4;
                                }
                            }
                            _ => {}
                        }
                        v.stage_t += 1;
                        // a fully decayed note (an electric piano's tine) is over
                        if v.stage == 4 && eg.sustain <= 0.0 {
                            v.alive = false;
                            break;
                        }
                    }
                    let (x, y) = p.sample(v.pos);
                    let a = v.lvl * v.env;
                    let (gl, gr) = side(i);
                    out_l[i] += x * gl * a;
                    if stereo {
                        out_r[i] += y * gr * a;
                    }
                    v.pos += v.rate * bend as f64 * v.slide_tick();
                    v.t += 1;
                }
                continue;
            }
            if v.kind == VoiceKind::MusicBox {
                let mb = mb.unwrap_or_default();
                // −60 dB after `release` seconds
                let coef = 10f32.powf(-3.0 / (mb.release.max(0.005) * sr));
                for i in start..n {
                    // decayed into inaudibility on its own
                    if v.mb.spent() {
                        v.alive = false;
                        break;
                    }
                    if v.t >= v.len {
                        v.env *= coef;
                        if v.env < 1e-4 {
                            v.alive = false;
                            break;
                        }
                    }
                    let slide = v.slide_tick() as f32;
                    let y = v.mb.sample(bend * slide, sr);
                    let fade = (v.t as f32 / (0.001 * sr)).min(1.0) * v.env;
                    let (gl, gr) = side(i);
                    out_l[i] += y * gl * fade;
                    if stereo {
                        out_r[i] += y * gr * fade;
                    }
                    v.t += 1;
                }
                continue;
            }
            if v.kind == VoiceKind::Sample {
                let Some((a, m, slices)) = sample else {
                    v.alive = false;
                    continue;
                };
                // a slice whose markers moved or vanished while it played: the channel's region
                let slice = slices.filter(|sl| (v.slice as usize) < sl.len());
                let (lo, hi) = match slice {
                    Some(sl) => slice_region(sl, v.slice, a.frames),
                    None => region(m.start, m.end, a.frames),
                };
                let (del, att, hold, dec, rel) = (m.delay * sr, m.attack * sr, m.hold * sr, m.decay * sr, m.release * sr);
                // the envelope's level `t` samples into the note, before its note-off
                let held = |t: f32| -> f32 {
                    let t = t - del;
                    if t < 0.0 {
                        0.0
                    } else if t < att {
                        curve(t / att, m.attack_bend)
                    } else if t < att + hold {
                        1.0
                    } else if t < att + hold + dec {
                        1.0 - (1.0 - m.sustain) * curve((t - att - hold) / dec, m.decay_bend)
                    } else {
                        m.sustain
                    }
                };
                let shaped = del + att + hold + dec;
                // a note-off (`len`) starts the release from wherever the envelope was
                let released = m.env && v.len != u32::MAX;
                let from = if released { held(v.len as f32) } else { 0.0 };
                let cut = fall(CUT_SECONDS, sr);
                // a trimmed or reversed part starts and ends inside the waveform: fade its edges
                let edge = EDGE_SECONDS * sr;
                // a slice ends inside a sound (a kick's tail): a longer fade than the start's, which
                // must keep the attack sharp
                let edge_tail = if slice.is_some() { SLICE_TAIL_SECONDS * sr } else { edge };
                // and its start a shorter one than a trimmed region's: the attack is right there
                let edge_head = if slice.is_some() { SLICE_HEAD_SECONDS * sr } else { edge };
                let note_edge = (edge.min(v.len as f32)).recip();
                let (edge_in, edge_out) = (slice.is_some() || m.reverse || m.start > 0.0, slice.is_some() || m.reverse || m.end < 1.0);
                let step = v.rate * bend as f64;
                let span = (hi - lo - 1.0).max(0.0);
                let fade_in = (m.fade_in * a.sr as f64).min(span);
                let fade_out = (m.fade_out * a.sr as f64).min(span);
                let in_scale = if fade_in > 0.0 { fade_in.recip() } else { 0.0 };
                let out_scale = if fade_out > 0.0 { fade_out.recip() } else { 0.0 };
                // without the envelope a sized note stops at its end (declicked); with it, it releases
                let stops = v.timed && !m.env;
                for i in start..n {
                    if (stops && v.t >= v.len) || v.pos < lo || v.pos >= hi {
                        v.alive = false;
                        break;
                    }
                    let t = v.t as f32;
                    let shape = if !m.env {
                        1.0
                    } else if released && v.t >= v.len {
                        let x = (t - v.len as f32) / rel.max(1.0);
                        if x >= 1.0 {
                            v.alive = false;
                            break;
                        }
                        from * (1.0 - curve(x, m.release_bend))
                    } else {
                        held(t)
                    };
                    if v.t >= v.cut_at {
                        v.env *= cut;
                    }
                    // decayed to nothing or cut: the voice is over
                    if v.env < 1e-4 || (m.env && m.sustain <= 0.0 && t >= shaped) {
                        v.alive = false;
                        break;
                    }
                    let mut fade = shape * v.env;
                    let from_start = if m.reverse { hi - 1.0 - v.pos } else { v.pos - lo };
                    let to_end = if m.reverse { v.pos - lo } else { hi - v.pos };
                    let edge_start = if edge_in { (t / edge_head).min(1.0) } else { 1.0 };
                    let edge_end = if edge_out { (to_end / step.max(1e-9) / edge_tail as f64).min(1.0) as f32 } else { 1.0 };
                    let fade_start = if in_scale > 0.0 { (from_start * in_scale).clamp(0.0, 1.0) as f32 } else { 1.0 };
                    let fade_end = if out_scale > 0.0 {
                        let remaining = if m.reverse { to_end } else { to_end - 1.0 };
                        (remaining * out_scale).clamp(0.0, 1.0) as f32
                    } else {
                        1.0
                    };
                    fade *= edge_start.min(fade_start);
                    let note_end = if stops { ((v.len - v.t) as f32 * note_edge).min(1.0) } else { 1.0 };
                    fade *= edge_end.min(fade_end).min(note_end);
                    let (x, y) = a.sample(v.pos);
                    let (gl, gr) = side(i);
                    out_l[i] += x * gl * fade;
                    if stereo {
                        out_r[i] += y * gr * fade;
                    }
                    let step = step * v.slide_tick();
                    v.pos += if m.reverse { -step } else { step };
                    v.t += 1;
                }
                continue;
            }
            // the metronome click
            let VoiceKind::Click { accent } = v.kind else {
                v.alive = false;
                continue;
            };
            for i in start..n {
                let t = v.t as f32 / sr;
                let f = if accent { 1760.0 } else { 1320.0 };
                v.phase = (v.phase + f / sr).fract();
                let sq = if v.phase < 0.5 { 1.0 } else { -1.0 };
                let a = if t < 0.002 { exp_ramp(0.0001, 0.22, t, 0.002) } else { exp_ramp(0.22, 0.0001, t - 0.002, 0.048) };
                let (gl, gr) = side(i);
                out_l[i] += sq * a * gl;
                if stereo {
                    out_r[i] += sq * a * gr;
                }
                v.t += 1;
                if t > 0.06 {
                    v.alive = false;
                    break;
                }
            }
        }
    }
}

/// The bend factor of an envelope tension −1..1 for [`curve`]: 1 is straight, 1/8..8 the extremes.
#[inline]
pub fn bend(tension: f32) -> f32 {
    8f32.powf(tension.clamp(-1.0, 1.0))
}

/// A segment's progress 0..1 at `x` 0..1 of its time, bent by `g` (1 = straight, below 1 rises
/// early, above 1 rises late). One division, so it is cheap per sample.
#[inline]
pub fn curve(x: f32, g: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x / (x + g * (1.0 - x))
}

/// Per-sample factor that falls 60 dB in `seconds`.
#[inline]
fn fall(seconds: f32, sr: f32) -> f32 {
    10f32.powf(-3.0 / (seconds.max(0.0005) * sr))
}

/// The frames `[lo, hi)` a sampler plays: fractions of the file, at least one frame.
#[inline]
/// Frames of slice `i`: from its marker to the next one (the last one to the end of the file).
fn slice_region(slices: &[f32], i: u16, frames: usize) -> (f64, f64) {
    let i = i as usize;
    let end = slices.get(i + 1).copied().unwrap_or(1.0);
    region(slices[i], end, frames)
}

fn region(start: f32, end: f32, frames: usize) -> (f64, f64) {
    let n = frames as f64;
    let lo = (start as f64 * n).floor().clamp(0.0, (n - 1.0).max(0.0));
    let hi = (end as f64 * n).ceil().clamp(lo + 1.0, n.max(lo + 1.0));
    (lo, hi)
}
