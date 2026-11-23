//! A parser for the part of the SFZ format that Synkopa Keys plays.
//!
//! SFZ is the open, text-based description of a sampled instrument: which recording plays
//! for which keys and velocities, how it is tuned, levelled and enveloped. This module turns
//! an `.sfz` file (with its `#define`s and `#include`s) into flat [`Region`]s with every
//! inherited opcode resolved; [`crate::keys`] loads their samples and plays them.
//!
//! Supported: the `<control>`, `<global>`, `<master>`, `<group>`, `<region>` and `<curve>`
//! headers; key and velocity ranges, `key`, `pitch_keycenter`, `pitch_keytrack`, `tune`,
//! `transpose`, `volume`, `amplitude`, `pan`, `amp_veltrack`, `amp_velcurve_N`, velocity and
//! key crossfades (`xfin_*`/`xfout_*`), `trigger` (attack, release, first, legato),
//! round robins (`seq_length`/`seq_position`, `lorand`/`hirand`), the amplitude envelope
//! (`ampeg_*`), `rt_decay`, `offset`, `end`, loops (`loop_mode`, `loop_start`, `loop_end`),
//! `group`/`off_by`, keyswitches (`sw_lokey`/`sw_hikey`/`sw_last`/`sw_default`), and
//! `*_onccN` modulation of those opcodes.
//!
//! MIDI CCs are not routed into a playing instrument (except the sustain pedal, which the
//! voice pool handles), so a CC modulation is evaluated once, at load, from the CC's default
//! value (`set_cc`/`set_hdcc`, or SFZ's defaults). That is how an instrument sounds before
//! anyone moves a knob, which is what its author tuned. The extended CCs 131 (velocity) and
//! 133 (key) vary per note and are evaluated when the note starts. Regions that only a CC
//! triggers (`on_loccN`, pedal noises) are left out. Anything else unknown is ignored and
//! counted in [`Instrument::ignored`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// When a region sounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// When the key goes down.
    Attack,
    /// When the key goes up (or the sustain pedal lets the note go): key-off noises.
    Release,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopMode {
    /// Plays to the end of the sample, or until its envelope has released.
    NoLoop,
    /// Plays to the end of the sample whatever the note's length (drums, noises).
    OneShot,
    /// Loops between `loop_start` and `loop_end` for as long as it sounds.
    Continuous,
    /// Loops while the key is held, then plays on past the loop's end.
    Sustain,
}

/// What a CC modulation changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Volume,
    Amplitude,
    Pan,
    Tune,
    AmpVeltrack,
    Delay,
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
    Offset,
}

/// A modulation that depends on the note (extended CC 131 velocity or 133 key).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteMod {
    pub target: Target,
    /// 131 = the note's velocity, 133 = its key.
    pub cc: u16,
    pub amount: f32,
    pub curve: u16,
}

/// The amplitude envelope, in seconds (sustain 0..1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Eg {
    pub delay: f32,
    pub attack: f32,
    pub hold: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Default for Eg {
    fn default() -> Self {
        Self { delay: 0.0, attack: 0.0, hold: 0.0, decay: 0.0, sustain: 1.0, release: 0.0 }
    }
}

/// One region with everything it inherited resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    /// The sample, relative to the instrument's folder (`default_path` applied, `/` separators).
    pub sample: String,
    pub lokey: u8,
    pub hikey: u8,
    pub lovel: u8,
    pub hivel: u8,
    /// Key at which the sample sounds at its recorded pitch.
    pub keycenter: f32,
    /// Cents per key away from `keycenter` (100 = chromatic, 0 = every key plays the same pitch).
    pub keytrack: f32,
    /// Cents, `transpose` included.
    pub tune: f32,
    /// Level in dB (`volume`) and as a factor (`amplitude`, CC amplitude modulation).
    pub volume: f32,
    pub amplitude: f32,
    /// -1..1.
    pub pan: f32,
    /// -1..1: how much the velocity scales the level (SFZ's default is 1).
    pub amp_veltrack: f32,
    /// `amp_velcurve_N` points as (velocity, gain); empty = SFZ's default curve.
    pub velcurve: Vec<(u8, f32)>,
    pub xfin_vel: (u8, u8),
    pub xfout_vel: (u8, u8),
    pub xfin_key: (u8, u8),
    pub xfout_key: (u8, u8),
    /// Crossfades are equal power (`xf_velcurve=power`, the default) or linear (`gain`).
    pub xf_power: bool,
    pub trigger: Trigger,
    /// Round robin: the region plays on every `seq_length`-th note of its key, the
    /// `seq_position`-th one.
    pub seq_length: u8,
    pub seq_position: u8,
    pub lorand: f32,
    pub hirand: f32,
    pub eg: Eg,
    /// Release regions: dB lost per second the note was held.
    pub rt_decay: f32,
    /// First frame played, and the last (`None` = the sample's end).
    pub offset: u64,
    pub end: Option<u64>,
    pub loop_mode: LoopMode,
    pub loop_start: u64,
    pub loop_end: Option<u64>,
    pub group: i64,
    pub off_by: i64,
    /// Keyswitch this region answers to (`sw_last`), if any, and the articulation's name.
    pub sw_last: Option<u8>,
    pub sw_label: Option<String>,
    pub note_mods: Vec<NoteMod>,
}

impl Default for Region {
    fn default() -> Self {
        Self {
            sample: String::new(),
            lokey: 0,
            hikey: 127,
            lovel: 1,
            hivel: 127,
            keycenter: 60.0,
            keytrack: 100.0,
            tune: 0.0,
            volume: 0.0,
            amplitude: 1.0,
            pan: 0.0,
            amp_veltrack: 1.0,
            velcurve: vec![],
            xfin_vel: (0, 0),
            xfout_vel: (127, 127),
            xfin_key: (0, 0),
            xfout_key: (127, 127),
            xf_power: true,
            trigger: Trigger::Attack,
            seq_length: 1,
            seq_position: 1,
            lorand: 0.0,
            hirand: 1.0,
            eg: Eg::default(),
            rt_decay: 0.0,
            offset: 0,
            end: None,
            loop_mode: LoopMode::NoLoop,
            loop_start: 0,
            loop_end: None,
            group: 0,
            off_by: 0,
            sw_last: None,
            sw_label: None,
            note_mods: vec![],
        }
    }
}

/// A parsed instrument.
#[derive(Debug, Default)]
pub struct Instrument {
    pub regions: Vec<Region>,
    /// Keyswitch range and the switch that is on when the instrument loads.
    pub sw_range: Option<(u8, u8)>,
    pub sw_default: Option<u8>,
    /// Opcodes this player does not know, with how often each appeared.
    pub ignored: Vec<(String, usize)>,
}

impl Instrument {
    /// The articulations a keyswitch picks, as (key, name), in key order; empty when the
    /// instrument has none.
    pub fn articulations(&self) -> Vec<(u8, String)> {
        let mut v: Vec<(u8, String)> = vec![];
        for r in &self.regions {
            let Some(k) = r.sw_last else { continue };
            if !v.iter().any(|a| a.0 == k) {
                v.push((k, r.sw_label.clone().unwrap_or_else(|| format!("Key {k}"))));
            }
        }
        v.sort_by_key(|a| a.0);
        v
    }
}

/// `c4` → 60, `C#4` → 61, `db4` → 61, `c-1` → 0, `60` → 60. SFZ's middle C is `c4`.
pub fn parse_key(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Ok(n) = s.parse::<i32>() {
        return u8::try_from(n).ok().filter(|k| *k <= 127);
    }
    let mut it = s.chars();
    let base = match it.next()?.to_ascii_lowercase() {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => return None,
    };
    let rest = it.as_str();
    let (acc, oct) = if let Some(r) = rest.strip_prefix('#') {
        (1, r)
    } else if let Some(r) = rest.strip_prefix('b').filter(|r| !r.is_empty() && r.parse::<i32>().is_ok()) {
        (-1, r)
    } else {
        (0, rest)
    };
    let oct: i32 = oct.parse().ok()?;
    u8::try_from((oct + 1) * 12 + base + acc).ok().filter(|k| *k <= 127)
}

/// ARIA's built-in CC curves 0..6, plus `<curve>` definitions (`curve_index` ≥ 7).
#[derive(Default)]
struct Curves {
    custom: HashMap<u16, [f32; 128]>,
}

impl Curves {
    /// The curve's value for a CC at `x` (0..1).
    fn eval(&self, curve: u16, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match curve {
            0 => x,
            1 => x * 2.0 - 1.0,
            2 => 1.0 - x,
            3 => 1.0 - x * 2.0,
            4 => x * x,
            5 => x.sqrt(),
            6 => (1.0 - x).sqrt(),
            n => match self.custom.get(&n) {
                Some(t) => {
                    let p = x * 127.0;
                    let i = (p as usize).min(126);
                    let f = p - i as f32;
                    t[i] + (t[i + 1] - t[i]) * f
                }
                None => x,
            },
        }
    }
}

/// Opcodes of one header level, in the order they were written.
type Ops = Vec<(String, String)>;

/// Reads `path` and parses it. Includes resolve against the `.sfz` file's folder, like
/// every SFZ player does.
pub fn load(path: &Path) -> Result<Instrument, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
    parse(&text, &base)
}

/// Parses SFZ text whose `#include`s resolve against `base`.
pub fn parse(text: &str, base: &Path) -> Result<Instrument, String> {
    let mut defines: Vec<(String, String)> = vec![];
    let mut flat = String::new();
    preprocess(text, base, &mut defines, &mut flat, 0)?;
    build(&flat)
}

/// Strips comments, expands `#define`d names and inlines `#include`d files.
fn preprocess(text: &str, base: &Path, defines: &mut Vec<(String, String)>, out: &mut String, depth: u32) -> Result<(), String> {
    if depth > 16 {
        return Err("#include nests too deep".into());
    }
    let text = strip_block_comments(text);
    for raw in text.lines() {
        let line = match raw.find("//") {
            Some(i) => &raw[..i],
            None => raw,
        };
        let line = expand(line, defines);
        let mut rest = line.trim_start();
        // a line can mix directives and opcodes (`<group> #include "x.txt" lovel=1`); a `#`
        // that starts no directive is a sharp in a sample name (`F#3v1.flac`)
        loop {
            let at =
                rest.match_indices('#').map(|(i, _)| i).find(|&i| rest[i..].starts_with("#define") || rest[i..].starts_with("#include"));
            let Some(i) = at else {
                out.push_str(rest);
                out.push('\n');
                break;
            };
            out.push_str(&rest[..i]);
            out.push('\n');
            let d = &rest[i..];
            if let Some(r) = d.strip_prefix("#define") {
                let mut it = r.split_whitespace();
                if let (Some(name), Some(value)) = (it.next(), it.next()) {
                    if name.starts_with('$') {
                        defines.retain(|(n, _)| n != name);
                        defines.push((name.to_string(), value.to_string()));
                        // longest first, so `$OFF1` never eats the start of `$OFF10`
                        defines.sort_by_key(|(n, _)| std::cmp::Reverse(n.len()));
                    }
                }
                break;
            }
            let r = d["#include".len()..].trim_start();
            let Some(q) = r.strip_prefix('"') else { break };
            let Some(end) = q.find('"') else { break };
            let file = base.join(q[..end].replace('\\', "/"));
            let inc = std::fs::read_to_string(&file).map_err(|e| format!("#include {}: {e}", file.display()))?;
            preprocess(&inc, base, defines, out, depth + 1)?;
            rest = q[end + 1..].trim_start();
        }
    }
    Ok(())
}

/// Removes `// line` and `/* block */` comments in one pass, so a line comment of stars
/// (`//*****`) does not open a block comment.
fn strip_block_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let line = rest.find("//");
        let block = rest.find("/*");
        match (line, block) {
            (Some(l), b) if b.is_none_or(|b| l <= b) => {
                out.push_str(&rest[..l]);
                match rest[l..].find('\n') {
                    Some(e) => rest = &rest[l + e..],
                    None => return out,
                }
            }
            (_, Some(b)) => {
                out.push_str(&rest[..b]);
                match rest[b + 2..].find("*/") {
                    Some(j) => rest = &rest[b + 2 + j + 2..],
                    None => return out,
                }
            }
            _ => {
                out.push_str(rest);
                return out;
            }
        }
    }
}

fn expand(line: &str, defines: &[(String, String)]) -> String {
    if !line.contains('$') || line.trim_start().starts_with("#define") {
        return line.to_string();
    }
    let mut s = line.to_string();
    for (n, v) in defines {
        if s.contains(n.as_str()) {
            s = s.replace(n.as_str(), v);
        }
    }
    s
}

/// One `<header>` or `opcode=value` token, in file order.
enum Tok {
    Header(String),
    Op(String, String),
}

fn tokenize(text: &str) -> Vec<Tok> {
    let mut toks = vec![];
    for line in text.lines() {
        let mut rest = line;
        loop {
            rest = rest.trim_start();
            if rest.is_empty() {
                break;
            }
            if let Some(r) = rest.strip_prefix('<') {
                if let Some(end) = r.find('>') {
                    toks.push(Tok::Header(r[..end].trim().to_ascii_lowercase()));
                    rest = &r[end + 1..];
                    continue;
                }
                break;
            }
            let Some(eq) = rest.find('=') else { break };
            let name = rest[..eq].trim();
            if name.is_empty() || name.contains(char::is_whitespace) {
                // garbage before the next opcode: skip one word
                match rest.find(char::is_whitespace) {
                    Some(i) => {
                        rest = &rest[i..];
                        continue;
                    }
                    None => break,
                }
            }
            let after = &rest[eq + 1..];
            // the value runs to the next `name=` or header; only a sample path keeps spaces
            let stop = next_token_start(after);
            let raw = after[..stop].trim();
            let value =
                if name.eq_ignore_ascii_case("sample") { raw.to_string() } else { raw.split_whitespace().next().unwrap_or("").to_string() };
            toks.push(Tok::Op(name.to_ascii_lowercase(), value));
            rest = &after[stop..];
        }
    }
    toks
}

/// Where the next `<header>` or `opcode=` starts in `s` (or its length).
fn next_token_start(s: &str) -> usize {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'<' {
            return i;
        }
        if b[i].is_ascii_whitespace() {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            let mut k = j;
            while k < b.len() && (b[k].is_ascii_alphanumeric() || b[k] == b'_') {
                k += 1;
            }
            if k > j && k < b.len() && b[k] == b'=' {
                return i;
            }
            if j < b.len() && b[j] == b'<' {
                return j;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    b.len()
}

struct Ctx {
    default_path: String,
    note_offset: i32,
    octave_offset: i32,
    /// CC values 0..1 before anything moves (`set_cc`, `set_hdcc`).
    cc: HashMap<u16, f32>,
    curves: Curves,
    ignored: HashMap<String, usize>,
}

impl Ctx {
    fn cc(&self, n: u16) -> f32 {
        if let Some(v) = self.cc.get(&n) {
            return *v;
        }
        // SFZ's defaults: volume 100, pan centre, expression full
        match n {
            7 => 100.0 / 127.0,
            10 => 0.5,
            11 => 1.0,
            _ => 0.0,
        }
    }

    fn key(&self, v: &str) -> Option<u8> {
        let k = parse_key(v)? as i32 + self.note_offset + 12 * self.octave_offset;
        u8::try_from(k).ok().filter(|k| *k <= 127)
    }
}

fn build(text: &str) -> Result<Instrument, String> {
    let mut ctx = Ctx {
        default_path: String::new(),
        note_offset: 0,
        octave_offset: 0,
        cc: HashMap::new(),
        curves: Curves::default(),
        ignored: HashMap::new(),
    };
    let mut inst = Instrument::default();
    let (mut global, mut master, mut group): (Ops, Ops, Ops) = (vec![], vec![], vec![]);
    let mut region: Option<Ops> = None;
    let mut curve: Option<Ops> = None;
    let mut level = "";
    let mut sw: Option<(u8, u8)> = None;

    let flush_region = |r: Option<Ops>, global: &Ops, master: &Ops, group: &Ops, ctx: &mut Ctx, inst: &mut Instrument| {
        if let Some(r) = r {
            let all = global.iter().chain(master).chain(group).chain(&r);
            if let Some(reg) = resolve(all, ctx, &mut inst.sw_default) {
                inst.regions.push(reg);
            }
        }
    };
    let flush_curve = |c: Option<Ops>, ctx: &mut Ctx| {
        let Some(c) = c else { return };
        let Some(idx) = c.iter().find(|(n, _)| n == "curve_index").and_then(|(_, v)| v.parse::<u16>().ok()) else { return };
        let mut pts: Vec<(usize, f32)> =
            c.iter().filter_map(|(n, v)| Some((n.strip_prefix('v')?.parse::<usize>().ok()?.min(127), v.parse::<f32>().ok()?))).collect();
        pts.sort_by_key(|p| p.0);
        if pts.is_empty() {
            return;
        }
        let mut t = [0f32; 128];
        for (i, slot) in t.iter_mut().enumerate() {
            *slot = interp(&pts, i);
        }
        ctx.curves.custom.insert(idx, t);
    };

    for tok in tokenize(text) {
        match tok {
            Tok::Header(h) => {
                flush_region(region.take(), &global, &master, &group, &mut ctx, &mut inst);
                flush_curve(curve.take(), &mut ctx);
                match h.as_str() {
                    "control" => level = "control",
                    "global" => {
                        level = "global";
                        global.clear();
                        master.clear();
                        group.clear();
                    }
                    "master" => {
                        level = "master";
                        master.clear();
                        group.clear();
                    }
                    "group" => {
                        level = "group";
                        group.clear();
                    }
                    "region" => {
                        level = "region";
                        region = Some(vec![]);
                    }
                    "curve" => {
                        level = "curve";
                        curve = Some(vec![]);
                    }
                    other => {
                        level = "skip";
                        *ctx.ignored.entry(format!("<{other}>")).or_default() += 1;
                    }
                }
            }
            Tok::Op(name, value) => {
                // the keyswitch range is declared once but applies to the whole instrument
                match name.as_str() {
                    "sw_lokey" => {
                        if let Some(k) = ctx.key(&value) {
                            sw = Some((k, sw.map_or(k, |s| s.1)));
                        }
                    }
                    "sw_hikey" => {
                        if let Some(k) = ctx.key(&value) {
                            sw = Some((sw.map_or(k, |s| s.0), k));
                        }
                    }
                    _ => {}
                }
                match level {
                    "control" => control(&name, &value, &mut ctx),
                    "global" => global.push((name, value)),
                    "master" => master.push((name, value)),
                    "group" => group.push((name, value)),
                    "region" => region.get_or_insert_with(Vec::new).push((name, value)),
                    "curve" => curve.get_or_insert_with(Vec::new).push((name, value)),
                    // opcodes before any header behave as global ones
                    "" => global.push((name, value)),
                    _ => {}
                }
            }
        }
    }
    flush_region(region.take(), &global, &master, &group, &mut ctx, &mut inst);
    inst.sw_range = sw;
    if inst.regions.is_empty() {
        return Err("the instrument has no playable regions".into());
    }
    let mut ignored: Vec<(String, usize)> = ctx.ignored.into_iter().collect();
    ignored.sort();
    inst.ignored = ignored;
    Ok(inst)
}

fn interp(pts: &[(usize, f32)], i: usize) -> f32 {
    match pts.iter().position(|p| p.0 >= i) {
        Some(0) => pts[0].1,
        None => pts[pts.len() - 1].1,
        Some(j) => {
            let (a, b) = (pts[j - 1], pts[j]);
            if b.0 == a.0 {
                b.1
            } else {
                a.1 + (b.1 - a.1) * (i - a.0) as f32 / (b.0 - a.0) as f32
            }
        }
    }
}

fn control(name: &str, value: &str, ctx: &mut Ctx) {
    if name == "default_path" {
        ctx.default_path = value.replace('\\', "/");
    } else if name == "note_offset" {
        ctx.note_offset = value.parse().unwrap_or(0);
    } else if name == "octave_offset" {
        ctx.octave_offset = value.parse().unwrap_or(0);
    } else if let Some(n) = name.strip_prefix("set_hdcc").and_then(|n| n.parse::<u16>().ok()) {
        ctx.cc.insert(n, value.parse::<f32>().unwrap_or(0.0).clamp(0.0, 1.0));
    } else if let Some(n) = name.strip_prefix("set_cc").and_then(|n| n.parse::<u16>().ok()) {
        ctx.cc.insert(n, (value.parse::<f32>().unwrap_or(0.0) / 127.0).clamp(0.0, 1.0));
    } else if name.starts_with("label_cc") || name == "label_key" || name.starts_with("label_") {
        // names for a player's knobs: nothing to play
    } else {
        *ctx.ignored.entry(name.to_string()).or_default() += 1;
    }
}

/// Splits `ampeg_release_oncc72` / `ampeg_holdcc133` into (`ampeg_release`, 72).
fn cc_suffix(name: &str) -> Option<(&str, u16)> {
    let digits = name.len() - name.bytes().rev().take_while(u8::is_ascii_digit).count();
    let n: u16 = name[digits..].parse().ok()?;
    let head = name[..digits].strip_suffix("cc")?;
    let head = head.strip_suffix("_on").or_else(|| head.strip_suffix("_")).unwrap_or(head);
    Some((head, n))
}

fn target(name: &str) -> Option<Target> {
    Some(match name {
        "volume" | "gain" => Target::Volume,
        "amplitude" => Target::Amplitude,
        "pan" => Target::Pan,
        "tune" | "pitch" => Target::Tune,
        "amp_veltrack" => Target::AmpVeltrack,
        "ampeg_delay" => Target::Delay,
        "ampeg_attack" => Target::Attack,
        "ampeg_hold" => Target::Hold,
        "ampeg_decay" => Target::Decay,
        "ampeg_sustain" => Target::Sustain,
        "ampeg_release" => Target::Release,
        "offset" => Target::Offset,
        _ => return None,
    })
}

/// Applies a modulation amount to the region's base value.
pub fn apply(r: &mut Region, t: Target, x: f32) {
    match t {
        Target::Volume => r.volume += x,
        Target::Amplitude => r.amplitude *= x / 100.0,
        Target::Pan => r.pan = (r.pan + x / 100.0).clamp(-1.0, 1.0),
        Target::Tune => r.tune += x,
        Target::AmpVeltrack => r.amp_veltrack = (r.amp_veltrack + x / 100.0).clamp(-1.0, 1.0),
        Target::Delay => r.eg.delay = (r.eg.delay + x).max(0.0),
        Target::Attack => r.eg.attack = (r.eg.attack + x).max(0.0),
        Target::Hold => r.eg.hold = (r.eg.hold + x).max(0.0),
        Target::Decay => r.eg.decay = (r.eg.decay + x).max(0.0),
        Target::Sustain => r.eg.sustain = (r.eg.sustain + x / 100.0).clamp(0.0, 1.0),
        Target::Release => r.eg.release = (r.eg.release + x).max(0.0),
        Target::Offset => r.offset = (r.offset as f32 + x).max(0.0) as u64,
    }
}

/// The value a note-dependent modulation adds for `key` and `vel` (0..127).
pub fn note_mod_value(m: &NoteMod, key: u8, vel: u8) -> f32 {
    let x = if m.cc == 131 { vel as f32 / 127.0 } else { key as f32 / 127.0 };
    // only the built-in curves: custom curves are a load-time affair
    let c = Curves::default().eval(m.curve, x);
    m.amount * c
}

/// Builds a region from its inherited opcodes (global → master → group → region), or `None`
/// when it can never sound from a key (CC-triggered, or its CC conditions are not met).
fn resolve<'a>(ops: impl Iterator<Item = &'a (String, String)>, ctx: &mut Ctx, sw_default: &mut Option<u8>) -> Option<Region> {
    let mut r = Region::default();
    let mut key_center_set = false;
    // CC modulations: target → (cc → (amount, curve)); later levels override earlier ones
    let mut mods: Vec<(Target, u16, f32, u16)> = vec![];
    let mut curves: Vec<(Target, u16, u16)> = vec![];
    let mut conds: Vec<(u16, Option<f32>, Option<f32>)> = vec![];
    let mut transpose = 0.0;
    // `global_volume`, `master_volume`, `group_volume`: add to the region's `volume`
    let mut level_volume = 0.0;
    let mut sample_set = false;
    for (name, v) in ops {
        let f = || v.parse::<f32>().ok();
        let key = || ctx.key(v);
        let vel = || v.parse::<i32>().ok().map(|x| x.clamp(0, 127) as u8);
        match name.as_str() {
            "sample" => {
                r.sample = format!("{}{}", ctx.default_path, v.replace('\\', "/"));
                sample_set = true;
            }
            "lokey" => r.lokey = key()?,
            "hikey" => r.hikey = key().unwrap_or(0),
            "key" => {
                let k = key()?;
                r.lokey = k;
                r.hikey = k;
                if !key_center_set {
                    r.keycenter = k as f32;
                }
            }
            "pitch_keycenter" => {
                if let Some(k) = key() {
                    r.keycenter = k as f32;
                    key_center_set = true;
                }
            }
            "lovel" => r.lovel = vel()?,
            "hivel" => r.hivel = vel()?,
            "pitch_keytrack" => r.keytrack = f().unwrap_or(100.0),
            "tune" | "pitch" => r.tune = f().unwrap_or(0.0),
            "transpose" => transpose = f().unwrap_or(0.0),
            "volume" | "gain" => r.volume = f().unwrap_or(0.0),
            "global_volume" | "master_volume" | "group_volume" => level_volume += f().unwrap_or(0.0),
            "amplitude" => r.amplitude = f().unwrap_or(100.0) / 100.0,
            "pan" => r.pan = (f().unwrap_or(0.0) / 100.0).clamp(-1.0, 1.0),
            "amp_veltrack" => r.amp_veltrack = (f().unwrap_or(100.0) / 100.0).clamp(-1.0, 1.0),
            "xfin_lovel" => r.xfin_vel.0 = vel()?,
            "xfin_hivel" => r.xfin_vel.1 = vel()?,
            "xfout_lovel" => r.xfout_vel.0 = vel()?,
            "xfout_hivel" => r.xfout_vel.1 = vel()?,
            "xfin_lokey" => r.xfin_key.0 = key()?,
            "xfin_hikey" => r.xfin_key.1 = key()?,
            "xfout_lokey" => r.xfout_key.0 = key()?,
            "xfout_hikey" => r.xfout_key.1 = key()?,
            "xf_velcurve" | "xf_keycurve" => r.xf_power = v != "gain",
            "trigger" => match v.as_str() {
                "release" | "release_key" => r.trigger = Trigger::Release,
                "attack" | "first" | "legato" => r.trigger = Trigger::Attack,
                _ => *ctx.ignored.entry(format!("trigger={v}")).or_default() += 1,
            },
            "seq_length" => r.seq_length = v.parse::<u8>().unwrap_or(1).max(1),
            "seq_position" => r.seq_position = v.parse::<u8>().unwrap_or(1).max(1),
            "lorand" => r.lorand = f().unwrap_or(0.0),
            "hirand" => r.hirand = f().unwrap_or(1.0),
            "ampeg_delay" => r.eg.delay = f().unwrap_or(0.0).max(0.0),
            "ampeg_attack" => r.eg.attack = f().unwrap_or(0.0).max(0.0),
            "ampeg_hold" => r.eg.hold = f().unwrap_or(0.0).max(0.0),
            "ampeg_decay" => r.eg.decay = f().unwrap_or(0.0).max(0.0),
            "ampeg_sustain" => r.eg.sustain = (f().unwrap_or(100.0) / 100.0).clamp(0.0, 1.0),
            "ampeg_release" => r.eg.release = f().unwrap_or(0.0).max(0.0),
            "rt_decay" => r.rt_decay = f().unwrap_or(0.0).max(0.0),
            "offset" => r.offset = v.parse().unwrap_or(0),
            "end" => r.end = v.parse().ok(),
            "loop_mode" | "loopmode" => {
                r.loop_mode = match v.as_str() {
                    "one_shot" => LoopMode::OneShot,
                    "loop_continuous" => LoopMode::Continuous,
                    "loop_sustain" => LoopMode::Sustain,
                    _ => LoopMode::NoLoop,
                }
            }
            "loop_start" | "loopstart" => r.loop_start = v.parse().unwrap_or(0),
            "loop_end" | "loopend" => r.loop_end = v.parse().ok(),
            "group" => r.group = v.parse().unwrap_or(0),
            "off_by" => r.off_by = v.parse().unwrap_or(0),
            "sw_last" => r.sw_last = key(),
            "sw_label" => r.sw_label = Some(v.replace('_', " ")),
            "sw_default" => {
                if let Some(k) = key() {
                    *sw_default = Some(k);
                }
            }
            n if n.starts_with("amp_velcurve_") => {
                if let (Ok(i), Some(g)) = (n["amp_velcurve_".len()..].parse::<u8>(), f()) {
                    r.velcurve.retain(|p| p.0 != i);
                    r.velcurve.push((i.min(127), g));
                }
            }
            n if n.starts_with("on_locc") || n.starts_with("on_hicc") => return None,
            n if n.starts_with("locc") || n.starts_with("hicc") => {
                let lo = n.starts_with("lo");
                if let Ok(c) = n[4..].parse::<u16>() {
                    let x = f().unwrap_or(0.0) / 127.0;
                    match conds.iter_mut().find(|c0| c0.0 == c) {
                        Some(e) => {
                            if lo {
                                e.1 = Some(x)
                            } else {
                                e.2 = Some(x)
                            }
                        }
                        None => conds.push(if lo { (c, Some(x), None) } else { (c, None, Some(x)) }),
                    }
                }
            }
            // names, polyphony and off modes: the pool plays one note per key and damps it
            "region_label" | "group_label" | "master_label" | "global_label" | "note_polyphony" | "polyphony" | "off_time" | "off_mode"
            | "sw_lokey" | "sw_hikey" | "sw_vel" | "sw_lolast" | "sw_hilast" | "sw_up" | "sw_down" | "sw_previous" => {}
            n => {
                if let Some((head, cc)) = cc_suffix(n) {
                    if let Some(h) = head.strip_suffix("_curve") {
                        if let Some(t) = target(h) {
                            curves.retain(|c| !(c.0 == t && c.1 == cc));
                            curves.push((t, cc, v.parse().unwrap_or(0)));
                            continue;
                        }
                    } else if let Some(t) = target(head) {
                        mods.retain(|m| !(m.0 == t && m.1 == cc));
                        mods.push((t, cc, f().unwrap_or(0.0), 0));
                        continue;
                    }
                    if head.ends_with("_smooth") || head.ends_with("_step") {
                        continue;
                    }
                }
                *ctx.ignored.entry(n.to_string()).or_default() += 1;
            }
        }
    }
    if !sample_set || r.sample.starts_with('*') {
        // `*sine` and friends are generators, not recordings
        if sample_set {
            *ctx.ignored.entry(format!("sample={}", r.sample)).or_default() += 1;
        }
        return None;
    }
    for (cc, lo, hi) in conds {
        let x = ctx.cc(cc);
        if lo.is_some_and(|lo| x < lo - 1e-6) || hi.is_some_and(|hi| x > hi + 1e-6) {
            return None;
        }
    }
    r.tune += transpose * 100.0;
    r.volume += level_volume;
    for (t, cc, amount, _) in mods {
        let curve = curves.iter().find(|c| c.0 == t && c.1 == cc).map_or(0, |c| c.2);
        if cc == 131 || cc == 133 {
            r.note_mods.push(NoteMod { target: t, cc, amount, curve });
            continue;
        }
        let x = ctx.curves.eval(curve, ctx.cc(cc));
        apply(&mut r, t, amount * x);
    }
    if r.lokey > r.hikey || r.lovel > r.hivel {
        return None;
    }
    r.velcurve.sort_by_key(|p| p.0);
    Some(r)
}

/// The folder an instrument's sample paths are relative to.
pub fn sample_root(sfz: &Path) -> PathBuf {
    sfz.parent().map(Path::to_path_buf).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Instrument {
        parse(s, Path::new(".")).unwrap()
    }

    #[test]
    fn key_names() {
        assert_eq!(parse_key("c4"), Some(60));
        assert_eq!(parse_key("C#4"), Some(61));
        assert_eq!(parse_key("db4"), Some(61));
        assert_eq!(parse_key("c-1"), Some(0));
        assert_eq!(parse_key("a0"), Some(21));
        assert_eq!(parse_key("b3"), Some(59));
        assert_eq!(parse_key("bb3"), Some(58));
        assert_eq!(parse_key("72"), Some(72));
        assert_eq!(parse_key("h2"), None);
        assert_eq!(parse_key("200"), None);
    }

    #[test]
    fn regions_inherit_global_master_group() {
        let i = p("<global> volume=-6 ampeg_release=1
             <master> tune=10
             <group> lovel=1 hivel=64 <region> sample=a.wav key=60
             <group> lovel=65 <region> sample=b.wav lokey=c4 hikey=d4 pitch_keycenter=c#4 volume=-3");
        assert_eq!(i.regions.len(), 2);
        let (a, b) = (&i.regions[0], &i.regions[1]);
        assert_eq!((a.sample.as_str(), a.lokey, a.hikey, a.keycenter), ("a.wav", 60, 60, 60.0));
        assert_eq!((a.lovel, a.hivel, a.volume, a.tune, a.eg.release), (1, 64, -6.0, 10.0, 1.0));
        assert_eq!((b.lokey, b.hikey, b.keycenter, b.lovel, b.hivel, b.volume), (60, 62, 61.0, 65, 127, -3.0));
    }

    #[test]
    fn new_headers_reset_lower_levels() {
        let i = p("<global> volume=-6 <group> tune=50 <region> sample=a.wav
             <global> <region> sample=b.wav");
        assert_eq!((i.regions[0].volume, i.regions[0].tune), (-6.0, 50.0));
        assert_eq!((i.regions[1].volume, i.regions[1].tune), (0.0, 0.0));
    }

    #[test]
    fn sample_paths_keep_spaces_and_default_path() {
        let i = p("<control> default_path=Samples\\Piano/\n<region> sample=My Piano C4 v1.wav lokey=60 hikey=61");
        assert_eq!(i.regions[0].sample, "Samples/Piano/My Piano C4 v1.wav");
        assert_eq!((i.regions[0].lokey, i.regions[0].hikey), (60, 61));
    }

    #[test]
    fn a_line_comment_of_stars_is_not_a_block_comment() {
        let i = p("//*******
<global> group_volume=-2 <region> sample=a.wav volume=-1 /* x */ key=60");
        assert_eq!(i.regions.len(), 1);
        assert_eq!((i.regions[0].volume, i.regions[0].lokey), (-3.0, 60));
    }

    #[test]
    fn comments_defines_and_sharps() {
        let i = p("// header\n/* block\n comment */\n#define $EXT flac\n#define $V v1\n#define $V10 v10\n\
             <region> sample=F#3$V10.$EXT key=54 // trailing\n<region> sample=F#3$V.$EXT key=54");
        assert_eq!(i.regions[0].sample, "F#3v10.flac");
        assert_eq!(i.regions[1].sample, "F#3v1.flac");
    }

    #[test]
    fn includes_resolve_against_the_base_folder() {
        let dir = std::env::temp_dir().join(format!("sfz-inc-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Data")).unwrap();
        std::fs::write(dir.join("Data/r.txt"), "<region> sample=$NOTE.wav key=60").unwrap();
        std::fs::write(dir.join("x.sfz"), "#define $NOTE c4\n<group> #include \"Data/r.txt\" volume=-2").unwrap();
        let i = load(&dir.join("x.sfz")).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(i.regions.len(), 1);
        assert_eq!(i.regions[0].sample, "c4.wav");
    }

    #[test]
    fn cc_modulations_use_the_default_cc_values() {
        let i = p("<control> set_hdcc72=0.5 set_cc20=127
             <global> ampeg_release_oncc72=2 amplitude_oncc20=200 amplitude_oncc7=100 amplitude_curvecc7=4
             <region> sample=a.wav");
        let r = &i.regions[0];
        assert!((r.eg.release - 1.0).abs() < 1e-6, "{}", r.eg.release);
        // 200 % × 1.0 × 100 % × (100/127)²
        let want = 2.0 * (100.0f32 / 127.0).powi(2);
        assert!((r.amplitude - want).abs() < 1e-4, "{} vs {want}", r.amplitude);
    }

    #[test]
    fn cc_conditions_and_cc_triggers_drop_regions() {
        let i = p("<control> set_cc64=0
             <region> sample=up.wav on_locc64=0 on_hicc64=0
             <region> sample=needs_pedal.wav locc64=64
             <region> sample=ok.wav hicc64=63");
        assert_eq!(i.regions.iter().map(|r| r.sample.as_str()).collect::<Vec<_>>(), ["ok.wav"]);
    }

    #[test]
    fn note_modulations_are_kept_for_the_note() {
        let i = p("<global> ampeg_hold=5 ampeg_holdcc133=-5 <region> sample=a.wav");
        let r = &i.regions[0];
        assert_eq!(r.eg.hold, 5.0);
        assert_eq!(r.note_mods, vec![NoteMod { target: Target::Hold, cc: 133, amount: -5.0, curve: 0 }]);
        assert!((note_mod_value(&r.note_mods[0], 127, 64) + 5.0).abs() < 1e-6);
    }

    #[test]
    fn release_round_robin_loops_and_keyswitches() {
        let i = p("<global> sw_lokey=c0 sw_hikey=c#0 sw_default=c0
             <master> sw_last=c0 <region> sample=a.wav trigger=release rt_decay=2 seq_length=2 seq_position=2
             <region> sample=b.wav loop_mode=loop_continuous loop_start=10 loop_end=99 lorand=0.5");
        assert_eq!(i.sw_range, Some((12, 13)));
        assert_eq!(i.articulations(), [(12, "Key 12".to_string())]);
        assert_eq!(i.sw_default, Some(12));
        let (a, b) = (&i.regions[0], &i.regions[1]);
        assert_eq!((a.trigger, a.rt_decay, a.seq_length, a.seq_position, a.sw_last), (Trigger::Release, 2.0, 2, 2, Some(12)));
        assert_eq!((b.loop_mode, b.loop_start, b.loop_end, b.lorand), (LoopMode::Continuous, 10, Some(99), 0.5));
    }

    #[test]
    fn custom_curves_and_velcurves() {
        let i = p("<curve> curve_index=7 v000=0 v001=1 v127=1
             <region> sample=a.wav offset_oncc98=1000 offset_curvecc98=7 amp_velcurve_1=0.2 amp_velcurve_127=1
             <control> set_cc98=0");
        let r = &i.regions[0];
        // cc98 is 0 at the region's flush (the later <control> comes after it)
        assert_eq!(r.offset, 0);
        assert_eq!(r.velcurve, vec![(1, 0.2), (127, 1.0)]);
    }

    #[test]
    fn unknown_opcodes_are_counted_not_fatal() {
        let i = p("<region> sample=a.wav fil_type=lpf_2p cutoff=500 <effect> type=reverb");
        assert_eq!(i.regions.len(), 1);
        let names: Vec<&str> = i.ignored.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"cutoff") && names.contains(&"fil_type") && names.contains(&"<effect>"), "{names:?}");
    }

    #[test]
    fn an_instrument_without_regions_is_an_error() {
        assert!(parse("<group> volume=1", Path::new(".")).is_err());
        assert!(parse("<region> sample=*sine", Path::new(".")).is_err());
    }
}
