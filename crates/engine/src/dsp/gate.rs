//! Gate / expander: silences (or turns down) what falls below a threshold, or, in Duck mode,
//! turns the sound down while the detector hears something.
//!
//! - **Detector**: the insert's own input (left and right linked, the louder side decides) or a
//!   sidechain (`Fx::key`, another insert's sound), optionally band-limited by a 24 dB/oct
//!   high-pass and low-pass so a snare mic opens on the snare and not on the kick bleed. Peak
//!   (instant rise, 10 ms fall, so a 40 Hz wave does not drop out between its peaks) or RMS
//!   (10 ms mean square, ignores clicks).
//! - **Hysteresis and hold**: the gate opens at `thr` and closes only below `thr − hyst`, and not
//!   before `hold` ms have passed; a decaying tom or a breathy word end does not chatter.
//! - **Gain**: open = 0 dB; closed = `floor` (−80 = silence) with a hard gate, or below the
//!   close threshold the level falls `ratio − 1` dB per dB (a downward expander, gentler than a
//!   hard gate) down to `floor`. Duck mode mirrors it: open = `floor`, closed = 0 dB.
//! - **Smoothing**: a two-pole (critically damped, S-shaped) envelope with no overshoot and no
//!   corner, so even a 0.1 ms attack does not click. Opening is smoothed in linear gain (the
//!   attack time is how long the sound takes to come up), closing in dB (the release sounds like
//!   a natural decay, whatever the floor). Attack and release are 10–90 % times.
//! - **Look-ahead** (0–10 ms) delays the sound but not the detector, so the gate is already
//!   open when a drum hit arrives; reported to the engine for latency compensation.
//!
//! Nothing allocates after `prepare`.

use super::{db_to_gain, flush, Biquad, FilterType};
use crate::processor::{ParamBlock, ProcessCtx, Processor, METERS};

/// Parameter order (= catalog.rs GATE).
pub mod idx {
    pub const THR: usize = 0;
    pub const HYST: usize = 1;
    pub const RATIO: usize = 2;
    pub const FLOOR: usize = 3;
    pub const ATT: usize = 4;
    pub const HOLD: usize = 5;
    pub const REL: usize = 6;
    pub const LOOK: usize = 7;
    pub const MODE: usize = 8;
    pub const SCHP: usize = 9;
    pub const SCLP: usize = 10;
    pub const DET: usize = 11;
    pub const LISTEN: usize = 12;
}

pub const MODES: [&str; 2] = ["Gate", "Duck"];
pub const DETECT: [&str; 2] = ["Peak", "RMS"];
pub const LOOKS: [&str; 5] = ["0 ms", "1 ms", "2 ms", "5 ms", "10 ms"];
const LOOK_MS: [f32; 5] = [0.0, 1.0, 2.0, 5.0, 10.0];
/// At or above this ratio the expander is a hard gate (the knob's top shows "∞").
pub const RATIO_GATE: f32 = 99.5;
/// At or below this floor the closed gate is silent (the knob's bottom shows "−∞").
pub const FLOOR_SILENT: f32 = -79.9;
/// The side-chain filters are off at the ends of their ranges.
pub const SCHP_OFF: f32 = 20.5;
pub const SCLP_OFF: f32 = 19_500.0;
/// The lowest gain the envelope works with (−∞ is reached by snapping, see `process`).
const MIN_DB: f32 = -120.0;

/// Look-ahead in samples for the `look` choice at `sr` (= the effect's latency).
pub fn latency_samples(look: f64, sr: f32) -> usize {
    let i = (look.round().max(0.0) as usize).min(LOOK_MS.len() - 1);
    (LOOK_MS[i] / 1000.0 * sr).round() as usize
}

/// Coefficient of each of the two cascaded one-poles so the pair's step response goes from 10 %
/// to 90 % in `ms`: two identical poles take 3.358 time constants for that.
fn scurve(ms: f32, sr: f32) -> f32 {
    if ms <= 0.0 {
        0.0
    } else {
        (-3.358 / (ms / 1000.0 * sr)).exp()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Closed,
    Open,
    Hold,
    Closing,
}

impl State {
    fn code(self) -> f32 {
        match self {
            State::Closed => 0.0,
            State::Open => 1.0,
            State::Hold => 2.0,
            State::Closing => 3.0,
        }
    }
}

pub struct Gate {
    sr: f32,
    // parameters
    thr: f32,
    hyst: f32,
    ratio: f32,
    floor: f32,
    att_ms: f32,
    hold_ms: f32,
    hold_n: u32,
    rel_ms: f32,
    look_sel: f32,
    look: usize,
    duck: bool,
    schp: f32,
    sclp: f32,
    rms: bool,
    listen: bool,
    // coefficients
    a_att: f32,
    a_rel: f32,
    k_pk: f32,
    k_ms: f32,
    // side-chain filters: two biquads each (24 dB/oct), left and right
    hp: [[Biquad; 2]; 2],
    lp: [[Biquad; 2]; 2],
    hp_on: bool,
    lp_on: bool,
    // detector, state machine, envelope
    env: f32,
    ms: f32,
    state: State,
    hold: u32,
    /// The two cascaded smoother stages, linear gain.
    g1: f32,
    g2: f32,
    // look-ahead line
    dl: Vec<f32>,
    dr: Vec<f32>,
    /// What the detector hears, delayed like the sound, for `listen`.
    kl: Vec<f32>,
    kr: Vec<f32>,
    w: usize,
    // meters
    det_db: f32,
    gr_db: f32,
    keyed: bool,
    primed: bool,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            sr: 48_000.0,
            thr: -40.0,
            hyst: 4.0,
            ratio: 100.0,
            floor: -80.0,
            att_ms: 1.0,
            hold_ms: 0.0,
            hold_n: 0,
            rel_ms: 120.0,
            look_sel: 0.0,
            look: 0,
            duck: false,
            schp: 20.0,
            sclp: 20_000.0,
            rms: false,
            listen: false,
            a_att: 0.0,
            a_rel: 0.0,
            k_pk: 0.0,
            k_ms: 0.0,
            hp: [[Biquad::default(); 2]; 2],
            lp: [[Biquad::default(); 2]; 2],
            hp_on: false,
            lp_on: false,
            env: 0.0,
            ms: 0.0,
            state: State::Closed,
            hold: 0,
            g1: 0.0,
            g2: 0.0,
            dl: Vec::new(),
            dr: Vec::new(),
            kl: Vec::new(),
            kr: Vec::new(),
            w: 0,
            det_db: -120.0,
            gr_db: 0.0,
            keyed: false,
            primed: false,
        }
    }
}

impl Gate {
    /// Values that depend on the sample rate (and the look-ahead line's size).
    fn derive(&mut self) {
        self.hold_n = (self.hold_ms / 1000.0 * self.sr) as u32;
        self.look = latency_samples(f64::from(self.look_sel), self.sr).min(self.dl.len().saturating_sub(2));
        self.a_att = scurve(self.att_ms, self.sr);
        self.a_rel = scurve(self.rel_ms, self.sr);
    }
    /// Gain (dB) the closed gate rests at: the floor, or 0 dB when ducking.
    fn rest_db(&self) -> f32 {
        if self.duck {
            0.0
        } else {
            self.floor_db()
        }
    }
    fn floor_db(&self) -> f32 {
        if self.floor <= FLOOR_SILENT {
            MIN_DB
        } else {
            self.floor
        }
    }
    fn settle(&mut self) {
        let g = db_to_gain(self.rest_db());
        self.g1 = g;
        self.g2 = g;
        self.state = State::Closed;
        self.hold = 0;
        self.gr_db = self.rest_db().max(MIN_DB);
    }
    fn update_filters(&mut self) {
        let nyq = 0.45 * self.sr;
        self.hp_on = self.schp > SCHP_OFF;
        self.lp_on = self.sclp < SCLP_OFF && self.sclp < nyq;
        // two Butterworth sections at Q 0.54 and 1.31 make a 4th-order Butterworth (24 dB/oct)
        for side in 0..2 {
            for (j, q) in [0.541_196_1, 1.306_563].into_iter().enumerate() {
                self.hp[side][j].set(FilterType::Highpass, self.sr, self.schp.min(nyq), q, 0.0);
                self.lp[side][j].set(FilterType::Lowpass, self.sr, self.sclp.min(nyq), q, 0.0);
            }
        }
    }
    #[inline(always)]
    fn key_filter(&mut self, side: usize, mut x: f32) -> f32 {
        if self.hp_on {
            let y = self.hp[side][0].tick(x);
            x = self.hp[side][1].tick(y);
        }
        if self.lp_on {
            let y = self.lp[side][0].tick(x);
            x = self.lp[side][1].tick(y);
        }
        x
    }

    fn run(&mut self, l: &mut [f32], r: &mut [f32], key: Option<&[f32]>) {
        let n = l.len().min(r.len());
        if self.dl.is_empty() {
            return;
        }
        let t_open = self.thr;
        let t_close = self.thr - self.hyst;
        let hard = self.ratio >= RATIO_GATE;
        let slope = self.ratio - 1.0;
        let floor = self.floor_db();
        let silent = self.floor <= FLOOR_SILENT;
        let mask = self.dl.len() - 1;
        let (up, down) = if self.duck { (self.a_rel, self.a_att) } else { (self.a_att, self.a_rel) };
        let (mut det_max, mut gr_min) = (MIN_DB, 0f32);
        for i in 0..n {
            let (xl, xr) = (l[i], r[i]);
            // the side-chain: its own input (both sides, linked) or the key, band-limited
            let (kl, kr) = match key {
                Some(k) => {
                    let v = self.key_filter(0, k[i]);
                    (v, v)
                }
                None => (self.key_filter(0, xl), self.key_filter(1, xr)),
            };
            let lvl_db = if self.rms {
                self.ms = flush(self.ms + (0.5 * (kl * kl + kr * kr) - self.ms) * self.k_ms);
                10.0 * self.ms.max(1e-12).log10()
            } else {
                let a = kl.abs().max(kr.abs());
                self.env = if a > self.env { a } else { flush(self.env + (a - self.env) * self.k_pk) };
                20.0 * self.env.max(1e-6).log10()
            };
            // the meter shows the level the state machine compares with the thresholds
            det_max = det_max.max(lvl_db);

            // open at the threshold, close below threshold − hysteresis once the hold is over
            match self.state {
                State::Closed | State::Closing => {
                    if lvl_db >= t_open {
                        self.state = State::Open;
                        self.hold = self.hold_n;
                    }
                }
                State::Open => {
                    if lvl_db >= t_close {
                        self.hold = self.hold_n;
                    } else if self.hold > 0 {
                        self.state = State::Hold;
                        self.hold -= 1;
                    } else {
                        self.state = State::Closing;
                    }
                }
                State::Hold => {
                    if lvl_db >= t_close {
                        self.state = State::Open;
                        self.hold = self.hold_n;
                    } else if self.hold > 0 {
                        self.hold -= 1;
                    } else {
                        self.state = State::Closing;
                    }
                }
            }
            let gate_db = match self.state {
                State::Open | State::Hold => 0.0,
                _ if hard => floor,
                // the expander: ratio − 1 dB down per dB below the close threshold
                _ => (slope * (lvl_db - t_close).min(0.0)).max(floor),
            };
            let target_db = if self.duck { floor - gate_db } else { gate_db };
            let target = db_to_gain(target_db);

            // two-pole envelope: up in linear gain, down in dB. Gating, up is the attack; ducking,
            // down is (the attack is how fast the sound gets out of the way)
            if target > self.g2 {
                let a = up;
                self.g1 = target + (self.g1 - target) * a;
                self.g2 = self.g1 + (self.g2 - self.g1) * a;
            } else if self.g2 - target > 1e-7 || self.g1 - target > 1e-7 {
                let a = down;
                let td = target_db.max(MIN_DB);
                let d1 = td + (super::gain_to_db(self.g1).max(MIN_DB) - td) * a;
                let d2 = d1 + (super::gain_to_db(self.g2).max(MIN_DB) - d1) * a;
                self.g1 = db_to_gain(d1);
                self.g2 = db_to_gain(d2);
            } else {
                self.g1 = target;
                self.g2 = target;
            }
            let g2_db = super::gain_to_db(self.g2);
            // closed once the gain has arrived where it is headed: the floor for a gate, the
            // expander's own reduction (often above the floor) otherwise
            if self.state == State::Closing && (g2_db.max(MIN_DB) - target_db.max(MIN_DB)).abs() < 0.1 {
                self.state = State::Closed;
            }
            // the silent floor is silence, not −120 dB
            let g = if silent && g2_db <= MIN_DB + 0.5 { 0.0 } else { self.g2 };
            gr_min = gr_min.min(g2_db.max(MIN_DB));

            // look-ahead: the sound comes out `look` samples late, the gain is already moving
            // (listening, the detector's sound is delayed the same way, so it lines up with the mix)
            self.dl[self.w] = xl;
            self.dr[self.w] = xr;
            self.kl[self.w] = kl;
            self.kr[self.w] = kr;
            let rd = (self.w + self.dl.len() - self.look) & mask;
            let (yl, yr) = (self.dl[rd], self.dr[rd]);
            self.w = (self.w + 1) & mask;
            if self.listen {
                l[i] = self.kl[rd];
                r[i] = self.kr[rd];
            } else {
                l[i] = yl * g;
                r[i] = yr * g;
            }
        }
        self.det_db = det_max.max(MIN_DB);
        self.gr_db = gr_min;
        self.keyed = key.is_some();
    }
}

impl Processor for Gate {
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sr = sample_rate.max(8000.0);
        // 10 ms of look-ahead at any rate, plus room for the read position
        let n = ((0.010 * self.sr) as usize + 4).next_power_of_two();
        self.dl = vec![0.0; n];
        self.dr = vec![0.0; n];
        self.kl = vec![0.0; n];
        self.kr = vec![0.0; n];
        self.k_pk = 1.0 - (-1.0 / (0.010 * self.sr)).exp();
        self.k_ms = 1.0 - (-1.0 / (0.010 * self.sr)).exp();
        self.derive();
        self.update_filters();
        self.reset();
    }

    fn set_params(&mut self, p: &ParamBlock) {
        let v = &p.values;
        self.thr = v[idx::THR].clamp(-80.0, 0.0);
        self.hyst = v[idx::HYST].clamp(0.0, 24.0);
        self.ratio = v[idx::RATIO].clamp(1.0, 100.0);
        self.floor = v[idx::FLOOR].clamp(-80.0, 0.0);
        self.att_ms = v[idx::ATT].clamp(0.05, 200.0);
        self.hold_ms = v[idx::HOLD].clamp(0.0, 2000.0);
        self.rel_ms = v[idx::REL].clamp(1.0, 5000.0);
        self.look_sel = v[idx::LOOK];
        let duck = v[idx::MODE] >= 0.5;
        self.rms = v[idx::DET] >= 0.5;
        self.listen = v[idx::LISTEN] >= 0.5;
        self.derive();
        let (hp, lp) = (v[idx::SCHP].clamp(20.0, 4000.0), v[idx::SCLP].clamp(200.0, 20_000.0));
        if hp != self.schp || lp != self.sclp {
            (self.schp, self.sclp) = (hp, lp);
            self.update_filters();
        }
        // a new mode glides from the gain it has now to the new mode's (its attack or release),
        // so switching it while the music plays does not click
        self.duck = duck;
        if !self.primed {
            self.primed = true;
            self.settle();
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessCtx) {
        self.run(l, r, None);
    }

    /// Sidechain: the detector hears `key` (another insert's post-fader sound, mono) and the
    /// gate opens and closes on it: a pad chopped by a hi-hat, a bass ducked by the kick.
    fn process_keyed(&mut self, l: &mut [f32], r: &mut [f32], key: &[f32], _ctx: &ProcessCtx) {
        self.run(l, r, Some(key));
    }

    fn reset(&mut self) {
        self.dl.fill(0.0);
        self.dr.fill(0.0);
        self.kl.fill(0.0);
        self.kr.fill(0.0);
        self.w = 0;
        self.env = 0.0;
        self.ms = 0.0;
        for f in self.hp.iter_mut().chain(self.lp.iter_mut()).flatten() {
            f.reset();
        }
        self.settle();
    }

    fn tail_samples(&self) -> Option<usize> {
        // listening to a sidechain, it plays the key, which goes on while its own input is silent
        if self.listen && self.keyed {
            return None;
        }
        // silent in = silent out once the look-ahead line has emptied; what has to settle is the
        // hold, the release (to −120 dB, ~2.5 release times in dB) and the detector
        let rel = (self.rel_ms / 1000.0 * self.sr * 2.5) as usize;
        Some(self.look + self.hold_n as usize + rel + (0.1 * self.sr) as usize)
    }

    fn gain_reduction_db(&self) -> f32 {
        self.gr_db.min(0.0)
    }

    /// `[0]` the detector's level in dBFS, the highest of the last block: peak or RMS as `det`
    /// picks, of its own input or the key, after the side-chain filters, the level the thresholds
    /// are compared with, `[1]` gain dB now, `[2]` state (0 closed, 1 open, 2 hold, 3 closing), `[3]` 1 when
    /// the detector hears a sidechain.
    fn meters(&self, out: &mut [f32; METERS]) {
        out[0] = self.det_db;
        out[1] = super::gain_to_db(self.g2).max(MIN_DB);
        out[2] = self.state.code();
        out[3] = if self.keyed { 1.0 } else { 0.0 };
    }
}

#[cfg(test)]
#[path = "gate_tests.rs"]
mod tests;
