//! Synkopa Keys: the sampled instruments behind Channel Rack channels of kind `keys`.
//!
//! Each instrument of the [`CATALOG`] is an SFZ mapping (embedded in the program, see
//! `crates/engine/instruments`) over the recordings of a library kit: the grand piano ships
//! inside the app, the others are downloaded on demand. [`KeysBank::load`] parses the
//! mapping ([`crate::sfz`]), decodes the recordings on a few threads and keeps them as 16-bit
//! PCM (half the memory of `f32`; the recordings' own noise is far above 16-bit's).
//!
//! A note picks every region its key, velocity, round robin and keyswitch select (layers
//! crossfaded by velocity sound together), resampled to the key with cubic interpolation;
//! release regions sound when the key (or the pedal) lets it go. Banks are built off the
//! audio thread and shared as an `Arc`; voices live in [`crate::synth::VoicePool`] and only
//! read the bank.

use std::path::Path;
use std::sync::Arc;

use crate::sfz::{self, Eg, LoopMode, NoteMod, Target, Trigger};
use crate::sources::Audio;

/// Instrument a Keys channel plays when its project does not say (projects made before
/// there was more than one).
pub const DEFAULT_INSTRUMENT: &str = "grand-piano";
/// Library kit of the bundled grand piano (also its folder in the app's resources).
pub const KIT_ID: &str = "salamander-piano";
/// Recordings are cut to this length (with a fade): the longest a note can ring. Samples a
/// region loops are left whole.
pub const MAX_SECONDS: f64 = 9.0;
const FADE_SECONDS: f64 = 1.5;

/// Release override, ms (min, max); `None` in the project = the instrument's own.
pub const RELEASE_MS: (f32, f32) = (40.0, 3000.0);
pub const DYNAMICS: (f32, f32, f32) = (0.0, 100.0, 100.0);

/// What kind of instrument it is (the browser groups by it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Piano,
    Electric,
    Strings,
    Mallets,
}

/// One instrument Synkopa Keys can play.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub category: Category,
    /// One line on what it is and what it suits.
    pub description: &'static str,
    /// Library kit whose files the mapping plays.
    pub kit: &'static str,
    /// Ships inside the app (no download needed).
    pub bundled: bool,
    /// Credit owed under the recordings' licence (shown in the window and in the notices).
    pub credit: &'static str,
    pub license: &'static str,
    /// Playable range, MIDI keys (for the keyboard's shading).
    pub range: (u8, u8),
    #[serde(skip)]
    pub sfz: &'static str,
}

/// Every instrument, in the order the browser lists them.
pub const CATALOG: &[InstrumentInfo] = &[
    InstrumentInfo {
        id: "grand-piano",
        name: "Grand Piano",
        category: Category::Piano,
        description: "Yamaha C5 concert grand: four dynamic layers, key-release noises",
        kit: KIT_ID,
        bundled: true,
        credit: "Salamander Grand Piano V3 by Alexander Holm",
        license: "CC-BY-3.0",
        range: (21, 108),
        sfz: include_str!("../instruments/grand-piano.sfz"),
    },
    InstrumentInfo {
        id: "upright-piano",
        name: "Upright Piano",
        category: Category::Piano,
        description: "Intimate upright with felt and wood: three dynamics, two round robins",
        kit: "upright-piano",
        bundled: false,
        credit: "VSCO 2 Community Edition by Versilian Studios",
        license: "CC0-1.0",
        range: (21, 108),
        sfz: include_str!("../instruments/upright-piano.sfz"),
    },
    InstrumentInfo {
        id: "wurlitzer",
        name: "Wurlitzer EP200",
        category: Category::Electric,
        description: "Reedy, barking electric piano of soul and pop records",
        kit: "e-pianos",
        bundled: false,
        credit: "Wurlitzer EP200 by Greg Sullivan (SFZ mapping by kinwie)",
        license: "CC-BY-3.0",
        range: (33, 96),
        sfz: include_str!("../instruments/wurlitzer.sfz"),
    },
    InstrumentInfo {
        id: "cp80",
        name: "Electric Grand CP80",
        category: Category::Electric,
        description: "Yamaha's stringed electric grand: bright, glassy 80s ballads",
        kit: "e-pianos",
        bundled: false,
        credit: "Yamaha CP80 by Greg Sullivan (SFZ mapping by kinwie)",
        license: "CC-BY-3.0",
        range: (21, 108),
        sfz: include_str!("../instruments/cp80.sfz"),
    },
    InstrumentInfo {
        id: "pianet",
        name: "Pianet T",
        category: Category::Electric,
        description: "Hohner's plucked-reed piano: soft, woody, short",
        kit: "e-pianos",
        bundled: false,
        credit: "Hohner Pianet T by Greg Sullivan (SFZ mapping by kinwie)",
        license: "CC-BY-3.0",
        range: (29, 88),
        sfz: include_str!("../instruments/pianet.sfz"),
    },
    InstrumentInfo {
        id: "strings",
        name: "String Section",
        category: Category::Strings,
        description: "Violins, violas and cellos sustaining with vibrato, blended across the keyboard",
        kit: "string-section",
        bundled: false,
        credit: "VSCO 2 Community Edition by Versilian Studios",
        license: "CC0-1.0",
        range: (33, 93),
        sfz: include_str!("../instruments/strings.sfz"),
    },
    InstrumentInfo {
        id: "solo-violin",
        name: "Solo Violin",
        category: Category::Strings,
        description: "One violin for melodies and runs: arco with vibrato, tremolo, pizzicato, spiccato",
        kit: "solo-violin",
        bundled: false,
        credit: "VSCO 2 Community Edition by Versilian Studios",
        license: "CC0-1.0",
        range: (55, 103),
        sfz: include_str!("../instruments/solo-violin.sfz"),
    },
    InstrumentInfo {
        id: "harp",
        name: "Concert Harp",
        category: Category::Strings,
        description: "Plucked concert harp for arpeggios and glissandi",
        kit: "harp",
        bundled: false,
        credit: "VSCO 2 Community Edition by Versilian Studios",
        license: "CC0-1.0",
        range: (21, 105),
        sfz: include_str!("../instruments/harp.sfz"),
    },
    InstrumentInfo {
        id: "vibraphone",
        name: "Vibraphone",
        category: Category::Mallets,
        description: "Soft-mallet vibraphone with the motor off: two dynamics",
        kit: "vibraphone",
        bundled: false,
        credit: "VCSL by Versilian Studios",
        license: "CC0-1.0",
        range: (48, 91),
        sfz: include_str!("../instruments/vibraphone.sfz"),
    },
];

pub fn instrument(id: &str) -> Option<&'static InstrumentInfo> {
    CATALOG.iter().find(|i| i.id == id)
}

/// An articulation of an instrument: the keyswitch that picks it and its name.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Articulation {
    pub key: u8,
    pub name: String,
}

/// The articulations of a catalog instrument (empty for most), parsed once from its mapping.
pub fn articulations(id: &str) -> &'static [Articulation] {
    static ALL: std::sync::OnceLock<Vec<Vec<Articulation>>> = std::sync::OnceLock::new();
    let all = ALL.get_or_init(|| {
        CATALOG
            .iter()
            .map(|i| {
                sfz::parse(i.sfz, Path::new("."))
                    .map(|inst| inst.articulations().into_iter().map(|(key, name)| Articulation { key, name }).collect())
                    .unwrap_or_default()
            })
            .collect()
    });
    CATALOG.iter().position(|i| i.id == id).map_or(&[], |i| &all[i])
}

/// Interleaved 16-bit PCM with interpolation padding (one frame before, two after).
pub struct Pcm {
    data: Box<[i16]>,
    pub frames: usize,
    pub sr: u32,
    /// 1 or 2: a mono recording is stored once and plays on both sides.
    pub channels: u16,
}

const PAD_LEAD: usize = 1;
const PAD_TRAIL: usize = 2;

impl Pcm {
    pub fn from_audio(a: &Audio) -> Self {
        let ch = if a.channels == 1 { 1 } else { 2 };
        let src = a.content();
        let mut data = Vec::with_capacity((a.frames + PAD_LEAD + PAD_TRAIL) * ch);
        data.extend(std::iter::repeat_n(0i16, PAD_LEAD * ch));
        let q = |x: f32| (x * 32767.0).round().clamp(-32768.0, 32767.0) as i16;
        for f in src.as_chunks::<2>().0 {
            data.push(q(f[0]));
            if ch == 2 {
                data.push(q(f[1]));
            }
        }
        data.extend(std::iter::repeat_n(0i16, PAD_TRAIL * ch));
        Self { data: data.into_boxed_slice(), frames: a.frames, sr: a.sr, channels: ch as u16 }
    }

    pub fn bytes(&self) -> usize {
        self.data.len() * 2
    }

    /// Frame `pos` (fractional), cubic-interpolated, as (left, right). RT-safe.
    #[inline]
    pub fn sample(&self, pos: f64) -> (f32, f32) {
        if pos < 0.0 || pos >= self.frames as f64 {
            return (0.0, 0.0);
        }
        let i = pos as usize;
        let x = (pos - i as f64) as f32;
        let ch = self.channels as usize;
        let tap = |c: usize| -> f32 {
            let b = i * ch + c;
            let s = 1.0 / 32768.0;
            let t =
                [self.data[b] as f32 * s, self.data[b + ch] as f32 * s, self.data[b + 2 * ch] as f32 * s, self.data[b + 3 * ch] as f32 * s];
            wave_audio_file::cubic(t, x)
        };
        let l = tap(0);
        (l, if ch == 2 { tap(1) } else { l })
    }
}

/// A region ready to play: [`sfz::Region`] with its sample resolved and its level in linear gain.
pub struct Zone {
    pub lokey: u8,
    pub hikey: u8,
    pub lovel: u8,
    pub hivel: u8,
    pub keycenter: f32,
    pub keytrack: f32,
    pub tune: f32,
    /// `volume` and `amplitude` as one factor.
    pub gain: f32,
    pub pan: f32,
    pub amp_veltrack: f32,
    /// 128 gains when the region has its own velocity curve.
    pub velcurve: Option<Box<[f32; 128]>>,
    pub xfin_vel: (u8, u8),
    pub xfout_vel: (u8, u8),
    pub xfin_key: (u8, u8),
    pub xfout_key: (u8, u8),
    pub xf_power: bool,
    pub trigger: Trigger,
    pub seq_length: u8,
    pub seq_position: u8,
    pub lorand: f32,
    pub hirand: f32,
    pub eg: Eg,
    pub rt_decay: f32,
    pub offset: f64,
    /// One past the last frame played.
    pub end: f64,
    pub loop_mode: LoopMode,
    /// Loop points in frames (`end` exclusive); `loop_len` 0 = no loop.
    pub loop_start: f64,
    pub loop_len: f64,
    pub group: i64,
    pub off_by: i64,
    pub sw_last: Option<u8>,
    pub note_mods: Box<[NoteMod]>,
    pub sample: u32,
}

/// What a note of a zone starts with: the gain, the rate's pitch part (semitones) and the
/// envelope after the note's own modulations.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub zone: u16,
    pub gain: f32,
    pub semis: f32,
    pub eg: Eg,
    pub pan: f32,
}

pub struct KeysBank {
    zones: Vec<Zone>,
    samples: Vec<Pcm>,
    /// Zones whose key range covers each key, attack and release separately.
    attack: Vec<Vec<u16>>,
    release: Vec<Vec<u16>>,
    pub sw_range: Option<(u8, u8)>,
    pub sw_default: Option<u8>,
}

/// The recording, cut to [`MAX_SECONDS`] with a fade so the cut is not heard. A recording
/// already this short is returned as is, so the release build can apply it ahead of time
/// (`wave-keys-prep`) and loading then leaves the file untouched.
pub fn trimmed(a: Audio) -> Audio {
    let max = (MAX_SECONDS * a.sr as f64) as usize;
    if a.frames <= max {
        return a;
    }
    let mut data = a.content()[..max * 2].to_vec();
    let fade = ((FADE_SECONDS * a.sr as f64) as usize).min(max);
    for i in 0..fade {
        let g = 1.0 - (i as f32 + 1.0) / fade as f32;
        let f = max - fade + i;
        data[f * 2] *= g * g;
        data[f * 2 + 1] *= g * g;
    }
    Audio::from_stereo(data, a.sr, a.channels)
}

fn db(x: f32) -> f32 {
    10f32.powf(x / 20.0)
}

/// The fade-in (`lo..hi` rising) or fade-out weight of `x`.
fn xf(x: u8, (lo, hi): (u8, u8), rising: bool, power: bool) -> f32 {
    let w = if hi <= lo {
        if rising {
            if x >= hi {
                1.0
            } else {
                0.0
            }
        } else if x <= lo {
            1.0
        } else {
            0.0
        }
    } else {
        let t = ((x as f32 - lo as f32) / (hi - lo) as f32).clamp(0.0, 1.0);
        if rising {
            t
        } else {
            1.0 - t
        }
    };
    if power {
        w.sqrt()
    } else {
        w
    }
}

impl KeysBank {
    /// Builds a bank from a parsed instrument and its decoded samples (`sample(path)`
    /// returns the index into `samples`).
    pub fn build(inst: sfz::Instrument, samples: Vec<Pcm>, index: impl Fn(&str) -> Option<usize>) -> Result<Self, String> {
        let mut zones = vec![];
        for r in inst.regions {
            let Some(si) = index(&r.sample) else { continue };
            let p = &samples[si];
            let frames = p.frames as f64;
            let end = r.end.map_or(frames, |e| (e as f64 + 1.0).min(frames));
            let offset = (r.offset as f64).min(end);
            let looping = matches!(r.loop_mode, LoopMode::Continuous | LoopMode::Sustain);
            let (loop_start, loop_len) = match r.loop_end {
                Some(le) if looping => {
                    let le = (le as f64 + 1.0).min(end);
                    let ls = (r.loop_start as f64).min(le);
                    (ls, if le - ls >= 2.0 { le - ls } else { 0.0 })
                }
                _ => (0.0, 0.0),
            };
            let velcurve = (!r.velcurve.is_empty()).then(|| {
                let pts: Vec<(usize, f32)> = r.velcurve.iter().map(|&(v, g)| (v as usize, g)).collect();
                let mut t = Box::new([0f32; 128]);
                for (i, slot) in t.iter_mut().enumerate() {
                    *slot = match pts.iter().position(|p| p.0 >= i) {
                        Some(0) => pts[0].1 * i as f32 / pts[0].0.max(1) as f32,
                        None => pts[pts.len() - 1].1,
                        Some(j) => {
                            let (a, b) = (pts[j - 1], pts[j]);
                            a.1 + (b.1 - a.1) * (i - a.0) as f32 / (b.0 - a.0).max(1) as f32
                        }
                    };
                }
                t
            });
            zones.push(Zone {
                lokey: r.lokey,
                hikey: r.hikey,
                lovel: r.lovel,
                hivel: r.hivel,
                keycenter: r.keycenter,
                keytrack: r.keytrack,
                tune: r.tune,
                gain: db(r.volume) * r.amplitude,
                pan: r.pan,
                amp_veltrack: r.amp_veltrack,
                velcurve,
                xfin_vel: r.xfin_vel,
                xfout_vel: r.xfout_vel,
                xfin_key: r.xfin_key,
                xfout_key: r.xfout_key,
                xf_power: r.xf_power,
                trigger: r.trigger,
                seq_length: r.seq_length,
                seq_position: r.seq_position,
                lorand: r.lorand,
                hirand: r.hirand,
                eg: r.eg,
                rt_decay: r.rt_decay,
                offset,
                end,
                loop_mode: r.loop_mode,
                loop_start,
                loop_len,
                group: r.group,
                off_by: r.off_by,
                sw_last: r.sw_last,
                note_mods: r.note_mods.into_boxed_slice(),
                sample: si as u32,
            });
        }
        if zones.is_empty() {
            return Err("none of the instrument's samples could be found".into());
        }
        if zones.len() > u16::MAX as usize {
            return Err("the instrument has too many regions".into());
        }
        let mut attack = vec![vec![]; 128];
        let mut release = vec![vec![]; 128];
        for (i, z) in zones.iter().enumerate() {
            for k in z.lokey..=z.hikey.min(127) {
                let list = if z.trigger == Trigger::Release { &mut release } else { &mut attack };
                list[k as usize].push(i as u16);
            }
        }
        Ok(Self { zones, samples, attack, release, sw_range: inst.sw_range, sw_default: inst.sw_default })
    }

    /// Loads a catalog instrument whose recordings are in `dir`.
    pub fn load_instrument(info: &InstrumentInfo, dir: &Path) -> Result<Self, String> {
        let inst = sfz::parse(info.sfz, dir)?;
        Self::load_parsed(inst, dir)
    }

    /// Loads an `.sfz` file and the recordings it names (relative to its folder).
    pub fn load_sfz(path: &Path) -> Result<Self, String> {
        let inst = sfz::load(path)?;
        Self::load_parsed(inst, &sfz::sample_root(path))
    }

    fn load_parsed(inst: sfz::Instrument, dir: &Path) -> Result<Self, String> {
        let mut names: Vec<String> = inst.regions.iter().map(|r| r.sample.clone()).collect();
        names.sort();
        names.dedup();
        // a looped recording plays past the cut
        let looped: Vec<bool> = names
            .iter()
            .map(|n| inst.regions.iter().any(|r| &r.sample == n && matches!(r.loop_mode, LoopMode::Continuous | LoopMode::Sustain)))
            .collect();
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 6);
        let next = std::sync::atomic::AtomicUsize::new(0);
        let out: Vec<std::sync::Mutex<Option<Result<Pcm, String>>>> = names.iter().map(|_| std::sync::Mutex::new(None)).collect();
        std::thread::scope(|s| {
            for _ in 0..threads {
                s.spawn(|| loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(name) = names.get(i) else { break };
                    let path = dir.join(name);
                    let r = Audio::load(&path)
                        .map(|a| if looped[i] { a } else { trimmed(a) })
                        .map(|a| Pcm::from_audio(&a))
                        .map_err(|e| format!("{}: {e}", path.display()));
                    *out[i].lock().unwrap() = Some(r);
                });
            }
        });
        let mut samples = Vec::with_capacity(names.len());
        for slot in out {
            samples.push(slot.into_inner().unwrap().unwrap_or_else(|| Err("not decoded".into()))?);
        }
        Self::build(inst, samples, |n| names.binary_search_by(|x| x.as_str().cmp(n)).ok())
    }

    /// True when every recording the instrument names is in `dir`.
    pub fn present(info: &InstrumentInfo, dir: &Path) -> bool {
        match sfz::parse(info.sfz, dir) {
            Ok(inst) => inst.regions.iter().all(|r| dir.join(&r.sample).is_file()),
            Err(_) => false,
        }
    }

    pub fn zone(&self, i: u16) -> Option<&Zone> {
        self.zones.get(i as usize)
    }

    pub fn pcm(&self, z: &Zone) -> &Pcm {
        &self.samples[z.sample as usize]
    }

    pub fn zones(&self) -> usize {
        self.zones.len()
    }

    pub fn samples(&self) -> usize {
        self.samples.len()
    }

    /// Memory the recordings take.
    pub fn bytes(&self) -> usize {
        self.samples.iter().map(Pcm::bytes).sum()
    }

    pub fn has_release(&self, key: u8) -> bool {
        !self.release[key as usize & 127].is_empty()
    }

    /// True when `key` is a keyswitch of the instrument (it switches, it does not sound).
    pub fn is_switch(&self, key: u8) -> bool {
        self.sw_range.is_some_and(|(lo, hi)| (lo..=hi).contains(&key))
    }

    /// Calls `f` for every zone a note starts: `trigger` picks attack or release zones,
    /// `rr` is the key's round-robin count, `rand` 0..1, `sw` the active keyswitch,
    /// `dynamics` 0..1 scales the velocity tracking, `held` the seconds the key was down
    /// (release zones lose `rt_decay` dB per second). RT-safe: no allocation.
    #[allow(clippy::too_many_arguments)]
    pub fn pick(
        &self,
        trigger: Trigger,
        key: u8,
        vel: f32,
        rr: u32,
        rand: f32,
        sw: Option<u8>,
        dynamics: f32,
        held: f32,
        mut f: impl FnMut(Hit),
    ) {
        let list = match trigger {
            Trigger::Attack => &self.attack,
            Trigger::Release => &self.release,
        };
        let v = ((vel.clamp(0.0, 1.0) * 127.0).round() as u8).max(1);
        let sw = sw.or(self.sw_default);
        for &zi in &list[key as usize & 127] {
            let z = &self.zones[zi as usize];
            if v < z.lovel || v > z.hivel {
                continue;
            }
            if z.seq_length > 1 && (rr % z.seq_length as u32) + 1 != z.seq_position as u32 {
                continue;
            }
            if rand < z.lorand || (rand >= z.hirand && z.hirand < 1.0) {
                continue;
            }
            if z.sw_last.is_some() && z.sw_last != sw {
                continue;
            }
            let mut gain = z.gain;
            let mut eg = z.eg;
            let mut pan = z.pan;
            let mut tune = z.tune;
            let mut veltrack = z.amp_veltrack;
            let mut vol = 0.0;
            for m in z.note_mods.iter() {
                let x = sfz::note_mod_value(m, key, v);
                match m.target {
                    Target::Volume => vol += x,
                    Target::Amplitude => gain *= x / 100.0,
                    Target::Pan => pan = (pan + x / 100.0).clamp(-1.0, 1.0),
                    Target::Tune => tune += x,
                    Target::AmpVeltrack => veltrack = (veltrack + x / 100.0).clamp(-1.0, 1.0),
                    Target::Delay => eg.delay = (eg.delay + x).max(0.0),
                    Target::Attack => eg.attack = (eg.attack + x).max(0.0),
                    Target::Hold => eg.hold = (eg.hold + x).max(0.0),
                    Target::Decay => eg.decay = (eg.decay + x).max(0.0),
                    Target::Sustain => eg.sustain = (eg.sustain + x / 100.0).clamp(0.0, 1.0),
                    Target::Release => eg.release = (eg.release + x).max(0.0),
                    Target::Offset => {}
                }
            }
            let d = dynamics.clamp(0.0, 1.0);
            let x = v as f32 / 127.0;
            let vg = match &z.velcurve {
                Some(c) => 1.0 - d + d * c[v as usize],
                None => {
                    let t = veltrack * d;
                    let curve = if t >= 0.0 { x * x } else { (1.0 - x) * (1.0 - x) };
                    1.0 - t.abs() + t.abs() * curve
                }
            };
            let xg = xf(v, z.xfin_vel, true, z.xf_power)
                * xf(v, z.xfout_vel, false, z.xf_power)
                * xf(key, z.xfin_key, true, z.xf_power)
                * xf(key, z.xfout_key, false, z.xf_power);
            if xg <= 0.0 {
                continue;
            }
            let rt = if z.trigger == Trigger::Release { db(-z.rt_decay * held.max(0.0)) } else { 1.0 };
            gain *= vg * xg * rt * db(vol);
            if gain <= 1e-5 {
                continue;
            }
            let semis = (key as f32 - z.keycenter) * z.keytrack / 100.0 + tune / 100.0;
            f(Hit { zone: zi, gain, semis, eg, pan });
        }
    }
}

/// A bank from SFZ text and in-memory recordings (tests and examples).
pub fn bank_from(text: &str, samples: Vec<(&str, Audio)>) -> Result<KeysBank, String> {
    let inst = sfz::parse(text, Path::new("."))?;
    let names: Vec<String> = samples.iter().map(|(n, _)| n.to_string()).collect();
    let pcm = samples.iter().map(|(_, a)| Pcm::from_audio(a)).collect();
    KeysBank::build(inst, pcm, |n| names.iter().position(|x| x == n))
}

/// The bank shared by many voices.
pub type Bank = Arc<KeysBank>;

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(v: f32, frames: usize) -> Audio {
        Audio::from_stereo(vec![v; frames * 2], 48000, 2)
    }

    fn hits(b: &KeysBank, t: Trigger, key: u8, vel: f32, rr: u32) -> Vec<Hit> {
        let mut v = vec![];
        b.pick(t, key, vel, rr, 0.3, None, 1.0, 0.0, |h| v.push(h));
        v
    }

    #[test]
    fn the_catalog_parses_and_names_its_samples() {
        for i in CATALOG {
            let inst = sfz::parse(i.sfz, Path::new(".")).unwrap_or_else(|e| panic!("{}: {e}", i.id));
            assert!(!inst.regions.is_empty(), "{}", i.id);
            assert!(inst.ignored.is_empty(), "{}: unsupported opcodes {:?}", i.id, inst.ignored);
            let (lo, hi) = (inst.regions.iter().map(|r| r.lokey).min().unwrap(), inst.regions.iter().map(|r| r.hikey).max().unwrap());
            assert!(lo <= i.range.0 && hi >= i.range.1, "{}: regions cover {lo}..{hi}, catalog says {:?}", i.id, i.range);
        }
        let mut ids: Vec<&str> = CATALOG.iter().map(|i| i.id).collect();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
        assert!(instrument(DEFAULT_INSTRUMENT).is_some_and(|i| i.bundled));
    }

    #[test]
    fn velocity_layers_crossfade() {
        let b = bank_from(
            "<region> sample=soft lokey=50 hikey=70 pitch_keycenter=60 hivel=80 xfout_lovel=40 xfout_hivel=80 amp_veltrack=0
             <region> sample=loud lokey=50 hikey=70 pitch_keycenter=60 lovel=40 xfin_lovel=40 xfin_hivel=80 amp_veltrack=0",
            vec![("soft", tone(0.1, 100)), ("loud", tone(0.9, 100))],
        )
        .unwrap();
        let soft = hits(&b, Trigger::Attack, 62, 20.0 / 127.0, 0);
        assert_eq!(soft.len(), 1);
        assert_eq!(soft[0].zone, 0);
        assert!((soft[0].semis - 2.0).abs() < 1e-6);
        let mid = hits(&b, Trigger::Attack, 60, 60.0 / 127.0, 0);
        assert_eq!(mid.len(), 2, "both layers sound in the crossfade");
        // equal power: the two gains' squares add to one
        let p: f32 = mid.iter().map(|h| h.gain * h.gain).sum();
        assert!((p - 1.0).abs() < 1e-3, "{p}");
        let loud = hits(&b, Trigger::Attack, 60, 1.0, 0);
        assert_eq!(loud.iter().map(|h| h.zone).collect::<Vec<_>>(), [1]);
        assert!(hits(&b, Trigger::Attack, 80, 1.0, 0).is_empty(), "outside every key range");
    }

    #[test]
    fn round_robins_rotate_and_release_zones_decay_with_the_hold() {
        let b = bank_from(
            "<group> seq_length=2 <region> sample=a key=60 seq_position=1 <region> sample=b key=60 seq_position=2
             <group> trigger=release rt_decay=6 <region> sample=r key=60",
            vec![("a", tone(0.5, 10)), ("b", tone(0.5, 10)), ("r", tone(0.5, 10))],
        )
        .unwrap();
        assert_eq!(hits(&b, Trigger::Attack, 60, 0.5, 0)[0].zone, 0);
        assert_eq!(hits(&b, Trigger::Attack, 60, 0.5, 1)[0].zone, 1);
        assert_eq!(hits(&b, Trigger::Attack, 60, 0.5, 2)[0].zone, 0);
        assert!(b.has_release(60) && !b.has_release(61));
        let mut short = vec![];
        let mut long = vec![];
        b.pick(Trigger::Release, 60, 1.0, 0, 0.0, None, 1.0, 0.0, |h| short.push(h));
        b.pick(Trigger::Release, 60, 1.0, 0, 0.0, None, 1.0, 1.0, |h| long.push(h));
        assert!((long[0].gain / short[0].gain - db(-6.0)).abs() < 1e-4);
    }

    #[test]
    fn dynamics_scales_velocity_tracking() {
        let b = bank_from("<region> sample=a key=60", vec![("a", tone(0.5, 10))]).unwrap();
        let g = |vel: f32, dynamics: f32| {
            let mut g = 0.0;
            b.pick(Trigger::Attack, 60, vel, 0, 0.0, None, dynamics, 0.0, |h| g = h.gain);
            g
        };
        assert!(g(0.3, 1.0) < g(0.9, 1.0));
        assert_eq!(g(0.3, 0.0), g(0.9, 0.0));
        assert!((g(1.0, 1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn keyswitches_pick_the_articulation() {
        let b = bank_from(
            "<global> sw_lokey=c1 sw_hikey=d1 sw_default=c1 <group> sw_last=c1 <region> sample=a key=60
             <group> sw_last=d1 <region> sample=b key=60",
            vec![("a", tone(0.5, 10)), ("b", tone(0.5, 10))],
        )
        .unwrap();
        assert!(b.is_switch(24) && b.is_switch(26) && !b.is_switch(60));
        let zone = |sw| {
            let mut z = vec![];
            b.pick(Trigger::Attack, 60, 0.5, 0, 0.0, sw, 1.0, 0.0, |h| z.push(h.zone));
            z
        };
        assert_eq!(zone(None), [0]);
        assert_eq!(zone(Some(26)), [1]);
    }

    #[test]
    fn note_modulations_shape_the_envelope_per_key() {
        let b = bank_from("<region> sample=a ampeg_hold=5 ampeg_holdcc133=-5", vec![("a", tone(0.5, 10))]).unwrap();
        let hold = |key| {
            let mut h = 0.0;
            b.pick(Trigger::Attack, key, 0.5, 0, 0.0, None, 1.0, 0.0, |x| h = x.eg.hold);
            h
        };
        assert!(hold(20) > hold(100));
    }

    #[test]
    fn pcm_keeps_mono_once_and_reads_both_sides() {
        let a = Audio::from_stereo(vec![0.5; 20], 48000, 1);
        let p = Pcm::from_audio(&a);
        assert_eq!(p.channels, 1);
        let (l, r) = p.sample(3.0);
        assert!((l - 0.5).abs() < 1e-3 && l == r);
        assert_eq!(p.sample(10.0), (0.0, 0.0));
    }

    #[test]
    fn long_recordings_are_cut_with_a_fade() {
        let sr = 1000u32;
        let a = trimmed(Audio::from_stereo(vec![1.0; (sr as usize) * 2 * 20], sr, 2));
        assert_eq!(a.frames, (MAX_SECONDS * sr as f64) as usize);
        assert!(a.content()[a.frames * 2 - 1].abs() < 1e-3);
        assert_eq!(a.content()[0], 1.0);
    }
}
