//! Wet-signal ducking shared by the wet/dry effects (Reverb, Delay): an envelope follower on the
//! dry input drives a gain on the effect's wet output only, so the tail or the echoes sit under
//! a phrase and bloom back in the gaps. Applied after the reverb tank / the delay's feedback loop:
//! the effect keeps regenerating, only what is heard of it is lowered.
//!
//! ```text
//! dry ─ |peak| ─ envelope (5 ms up, 200 ms down) ─ dB ─ amount (−45 … −15 dBFS ramps 0 … 100 %)
//!       ─ target gain 10^(−Duck·amount/20) (every 16 samples) ─ glide (15 ms down, 250 ms up) ─ × wet
//! ```
//!
//! The parameter is the most the wet is pulled down, in dB (0 = off). No allocation, no branches
//! that depend on anything but the input level; `tick` is a handful of multiplies per sample plus
//! a `log10`/`powf` every 16 samples.

/// One-pole coefficient for a time constant of `ms` at `sr`.
fn coef(sr: f32, ms: f32) -> f32 {
    1.0 - (-1.0 / (sr * ms / 1000.0)).exp()
}

/// Largest depth the ducker accepts (dB); the catalogs expose 0–12.
pub const MAX_DUCK_DB: f32 = 24.0;

#[derive(Clone, Copy, Debug)]
pub struct WetDucker {
    depth_db: f32,
    env: f32,
    cur: f32,
    target: f32,
    k_env_att: f32,
    k_env_rel: f32,
    k_att: f32,
    k_rel: f32,
}

impl Default for WetDucker {
    fn default() -> Self {
        Self { depth_db: 0.0, env: 0.0, cur: 1.0, target: 1.0, k_env_att: 1.0, k_env_rel: 1.0, k_att: 1.0, k_rel: 1.0 }
    }
}

impl WetDucker {
    /// Sets the time constants for `sr`. Call from `prepare`.
    pub fn prepare(&mut self, sr: f32) {
        self.k_env_att = coef(sr, 5.0);
        self.k_env_rel = coef(sr, 200.0);
        self.k_att = coef(sr, 15.0);
        self.k_rel = coef(sr, 250.0);
    }

    /// Most the wet is pulled down (dB, ≥ 0; 0 = off). The gain glides to the new value.
    pub fn set_depth_db(&mut self, db: f32) {
        self.depth_db = if db.is_finite() { db.clamp(0.0, MAX_DUCK_DB) } else { 0.0 };
    }

    /// Feeds one stereo sample of the dry input and returns the wet gain (≤ 1) for it.
    /// `recompute`: refresh the target gain this sample (the callers do it every 16 samples).
    #[inline]
    pub fn tick(&mut self, xl: f32, xr: f32, recompute: bool) -> f32 {
        let pk = xl.abs().max(xr.abs());
        let ka = if pk > self.env { self.k_env_att } else { self.k_env_rel };
        self.env += (pk - self.env) * ka;
        if recompute {
            self.target = if self.depth_db > 0.0 {
                let lvl = 20.0 * (self.env + 1e-9).log10();
                let amt = ((lvl + 45.0) / 30.0).clamp(0.0, 1.0);
                10f32.powf(-self.depth_db * amt / 20.0)
            } else {
                1.0
            };
        }
        let t = self.target;
        let k = if t < self.cur { self.k_att } else { self.k_rel };
        self.cur += (t - self.cur) * k;
        self.cur
    }

    /// The gain now (1 = no ducking).
    pub fn gain(&self) -> f32 {
        self.cur
    }

    /// Forgets the envelope and the gain (silence, no ducking).
    pub fn reset(&mut self) {
        self.env = 0.0;
        self.cur = 1.0;
        self.target = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_is_unity_and_full_depth_reaches_the_setting() {
        let mut d = WetDucker::default();
        d.prepare(48000.0);
        for i in 0..48000 {
            assert_eq!(d.tick(0.9, -0.9, i % 16 == 0), 1.0);
        }
        d.set_depth_db(12.0);
        for i in 0..48000 {
            d.tick(0.9, -0.9, i % 16 == 0);
        }
        let db = 20.0 * d.gain().log10();
        assert!((db + 12.0).abs() < 0.05, "loud input: {db} dB");
        // silence: the envelope falls ~43 dB/s, so the wet blooms back over about a second
        // (−15 … −45 dBFS), then the 250 ms glide settles: unity within 2.5 s
        for i in 0..120_000 {
            d.tick(0.0, 0.0, i % 16 == 0);
        }
        assert!(d.gain() > 0.99, "after the phrase: {}", d.gain());
        // a quiet input (below −45 dBFS) never ducks
        d.reset();
        for i in 0..48000 {
            d.tick(0.004, 0.004, i % 16 == 0);
        }
        assert_eq!(d.gain(), 1.0);
    }

    /// The reverb's ducker before it moved here, verbatim: the shared one must give the very same
    /// gain, bit for bit, so the reverb's output did not change (checked on 3 × 100 k samples).
    #[test]
    fn identical_to_the_reverbs_inline_ducker() {
        struct Old {
            duck_db: f32,
            env: f32,
            duck_cur: f32,
            duck_t: f32,
            k_env_att: f32,
            k_env_rel: f32,
            k_duck_att: f32,
            k_duck_rel: f32,
        }
        for (sr, db) in [(44100.0f32, 12.0f32), (48000.0, 6.0), (96000.0, 0.0), (48000.0, 30.0)] {
            let mut o = Old {
                duck_db: db.clamp(0.0, 24.0),
                env: 0.0,
                duck_cur: 1.0,
                duck_t: 1.0,
                k_env_att: coef(sr, 5.0),
                k_env_rel: coef(sr, 200.0),
                k_duck_att: coef(sr, 15.0),
                k_duck_rel: coef(sr, 250.0),
            };
            let mut d = WetDucker::default();
            d.prepare(sr);
            d.set_depth_db(db);
            let mut s = 3u32;
            for n in 0..100_000usize {
                // phrases and gaps of noise at several levels, in blocks of 128
                let i = n % 128;
                let lvl = [0.9f32, 0.0, 0.05, 0.0, 0.3, 0.001][(n / 9000) % 6];
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                let xl = (s as f32 / u32::MAX as f32 - 0.5) * 2.0 * lvl;
                let xr = -0.7 * xl;
                // --- the old inline code ---
                let duck_on = o.duck_db > 0.0;
                let pk = xl.abs().max(xr.abs());
                let ka = if pk > o.env { o.k_env_att } else { o.k_env_rel };
                o.env += (pk - o.env) * ka;
                if i % 16 == 0 {
                    o.duck_t = if duck_on {
                        let lvl = 20.0 * (o.env + 1e-9).log10();
                        let amt = ((lvl + 45.0) / 30.0).clamp(0.0, 1.0);
                        10f32.powf(-o.duck_db * amt / 20.0)
                    } else {
                        1.0
                    };
                }
                let duck_t = o.duck_t;
                let kd = if duck_t < o.duck_cur { o.k_duck_att } else { o.k_duck_rel };
                o.duck_cur += (duck_t - o.duck_cur) * kd;
                // ---
                let g = d.tick(xl, xr, i % 16 == 0);
                assert_eq!(g.to_bits(), o.duck_cur.to_bits(), "sr {sr} depth {db} sample {n}");
            }
        }
    }

    #[test]
    fn non_finite_depth_is_off() {
        let mut d = WetDucker::default();
        d.prepare(44100.0);
        d.set_depth_db(f32::NAN);
        for i in 0..4410 {
            assert_eq!(d.tick(1.0, 1.0, i % 16 == 0), 1.0);
        }
    }
}
