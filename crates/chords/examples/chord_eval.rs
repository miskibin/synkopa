//! Chord detection accuracy on progressions played by the Synkopa Keys instruments: a test set
//! with exact labels and no copyright questions, since every note is ours.
//!
//!   cargo run --release -p wave-chords --example chord_eval -- <chordnet.onnx> <kits folder> [progressions per instrument] [out folder]
//!
//! `<kits folder>` holds one folder per library kit (`node scripts/keys-kits.mjs` fills
//! `target/keys-kits`). Per instrument it renders seeded random progressions: diatonic chords
//! of random keys, with the qualities pop songs use (triads mostly, some 7ths, maj7s, m7s, sus4s,
//! a diminished now and then), at 80–140 BPM, one or two chords a bar, played as held blocks,
//! re-struck quarters, or arpeggios over a bass root. It reports, over frames away from the
//! chord changes (±150 ms), how often the root, the major/minor triad and the full chord are
//! right, as the engine decodes them (Viterbi, no beat snapping). With an out folder the first
//! progression of each instrument is written as WAV with its labels, to listen to.

use std::path::Path;
use std::sync::Arc;

use wave_engine::chords::{class, decode_seconds, ChordModel, Detail, Quality, NO_CHORD, QUALITIES};
use wave_engine::keys::{KeysBank, CATALOG};
use wave_engine::*;

const SR: u32 = 44100;
const CH: Id = 7;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// A chord of a major key's degree (0..7), with the quality pop harmony gives it.
fn diatonic(key: u8, deg: usize, rng: &mut Rng) -> (u8, Quality) {
    const STEPS: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
    let root = (key + STEPS[deg]) % 12;
    let r = rng.unit();
    let q = match deg {
        0 | 3 => {
            if r < 0.7 {
                Quality::Maj
            } else if r < 0.85 {
                Quality::Maj7
            } else if r < 0.93 {
                Quality::Sus2
            } else {
                Quality::Sus4
            }
        }
        4 => {
            if r < 0.6 {
                Quality::Maj
            } else if r < 0.85 {
                Quality::Dom7
            } else {
                Quality::Sus4
            }
        }
        1 | 2 | 5 => {
            if r < 0.75 {
                Quality::Min
            } else {
                Quality::Min7
            }
        }
        _ => Quality::Dim,
    };
    (root, q)
}

#[derive(Clone, Copy, Debug)]
enum Style {
    Block,
    Quarters,
    Arpeggio,
}

struct Prog {
    bpm: f64,
    /// (start beat, end beat, root, quality)
    chords: Vec<(f64, f64, u8, Quality)>,
    notes: Vec<Note>,
    beats: f64,
}

fn progression(rng: &mut Rng, lo: u8, hi: u8, style: Style) -> Prog {
    let key = rng.below(12) as u8;
    let bpm = 80.0 + rng.below(61) as f64;
    let mut chords = vec![];
    let mut beat = 0.0;
    let mut last = 99;
    while beat < 32.0 {
        let mut deg = [0, 3, 4, 5, 1, 2, 6][(rng.below(100) as usize * 7 / 100).min(6)];
        if chords.is_empty() {
            deg = 0;
        }
        if deg == last {
            deg = (deg + 3) % 7;
        }
        last = deg;
        let len = if rng.unit() < 0.3 { 2.0 } else { 4.0 };
        let (root, q) = diatonic(key, deg, rng);
        chords.push((beat, beat + len, root, q));
        beat += len;
    }
    // the instrument's middle register
    let mid = (((lo as u16 + hi as u16) / 2).clamp(52, 67)) as u8;
    let mut notes = vec![];
    let n =
        |step: f64, len: f64, k: u8, v: f32| Note { v, ..Note::new(CH, (step * 4.0) as f32, (len * 4.0) as f32, Some(k.clamp(lo, hi))) };
    for &(a, b, root, q) in &chords {
        // tones from just below `mid`; the bass an octave or two down
        let base = mid - (mid % 12) + root;
        let base = if base > mid + 4 { base - 12 } else { base };
        let tones: Vec<u8> = q.intervals().iter().map(|i| base + i).collect();
        let bass = if base >= lo + 12 { base - 12 } else { base };
        let v = 0.55 + 0.25 * rng.unit() as f32;
        match style {
            Style::Block => {
                notes.push(n(a, b - a, bass, v));
                for &t in &tones {
                    notes.push(n(a, b - a, t, v));
                }
            }
            Style::Quarters => {
                let mut t0 = a;
                while t0 < b - 1e-9 {
                    for &t in tones.iter().chain([bass].iter()) {
                        notes.push(n(t0, 0.9, t, v));
                    }
                    t0 += 1.0;
                }
            }
            Style::Arpeggio => {
                notes.push(n(a, b - a, bass, v));
                let mut t0 = a;
                let mut i = 0;
                while t0 < b - 1e-9 {
                    notes.push(n(t0, 0.5, tones[i % tones.len()] + if i / tones.len() % 2 == 1 { 12 } else { 0 }, v));
                    t0 += 0.5;
                    i += 1;
                }
            }
        }
    }
    Prog { bpm, chords, notes, beats: beat }
}

fn project(instrument: &str, p: &Prog) -> Project {
    let mut pr = Project::empty();
    pr.bpm = p.bpm;
    pr.inserts[0].fx.clear();
    let keys = KeysState { instrument: instrument.into(), ..Default::default() };
    pr.channels.push(Channel { name: "Keys".into(), root: 60, ..Channel::new(CH, "", "#4f8ef7", ChannelSrc::Keys { keys }) });
    pr.patterns =
        vec![Pattern { id: 1, name: "P".into(), color: "#4f8ef7".into(), len: p.beats, min_len: 4.0, notes: p.notes.clone(), swing: 0.0 }];
    pr.clips = vec![Clip {
        id: 1,
        track: 0,
        start: 0.0,
        len: p.beats,
        offset: 0.0,
        kind: ClipKind::Pat,
        source: 1,
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
        root: 60,
    }];
    pr
}

#[derive(Default)]
struct Score {
    frames: usize,
    root: usize,
    majmin: usize,
    full: usize,
    /// The same at the frame's most likely chord (no decoder): what the decoder adds.
    raw_majmin: usize,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: chord_eval <chordnet.onnx> <kits folder> [progressions per instrument] [out folder]");
        std::process::exit(1);
    }
    let model = wave_chords::ChordMini::load(Path::new(&args[0])).unwrap();
    let kits = Path::new(&args[1]);
    let per: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(6);
    let out = args.get(3).map(Path::new);
    if let Some(o) = out {
        std::fs::create_dir_all(o).unwrap();
    }
    let mut total = Score::default();
    let mut confusions: std::collections::BTreeMap<String, usize> = Default::default();
    println!("{:<16} {:>6} {:>7} {:>7} {:>7} {:>9}", "instrument", "frames", "root", "maj/min", "full", "raw m/m");
    for info in CATALOG.iter() {
        let dir = kits.join(info.kit);
        if !KeysBank::present(info, &dir) {
            println!("{:<16} not on disk", info.id);
            continue;
        }
        let bank = Arc::new(KeysBank::load_instrument(info, &dir).unwrap());
        let mut s = Score::default();
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (info.id.len() as u64 * 7919));
        for k in 0..per {
            let style = [Style::Block, Style::Quarters, Style::Arpeggio][k % 3];
            let p = progression(&mut rng, info.range.0, info.range.1, style);
            let e = Engine::new(project(info.id, &p), Arc::new(NoPlugins));
            e.set_keys_bank(info.id, Some(bank.clone()));
            let spb = 60.0 / p.bpm;
            let secs = p.beats * spb;
            let x = e.render(0.0, p.beats, 0.0, SR).unwrap();
            let mono: Vec<f32> = x.as_chunks::<2>().0.iter().map(|f| (f[0] + f[1]) * 0.5).collect();
            let frames = model.analyze(&mono, SR).unwrap();
            let truth = |t: f64| -> Option<usize> {
                let b = t / spb;
                let c = p.chords.iter().find(|c| b >= c.0 && b < c.1)?;
                let near = p.chords.iter().any(|c| ((c.0 * spb) - t).abs() < 0.15 || ((c.1 * spb) - t).abs() < 0.15);
                (!near).then_some(class(c.2, c.3))
            };
            let full = decode_seconds(&frames, 0.0, secs, Detail::Full);
            let simple = decode_seconds(&frames, 0.0, secs, Detail::Simple);
            let at = |segs: &[(f64, f64, usize, f32)], t: f64| segs.iter().find(|s| t >= s.0 && t < s.1).map(|s| s.2).unwrap_or(NO_CHORD);
            let rate = frames.rate;
            for (i, probs) in frames.probs.iter().enumerate() {
                let t = i as f64 / rate;
                if t < 0.3 || t > secs - 0.3 {
                    continue;
                }
                let Some(want) = truth(t) else { continue };
                let (wr, wq) = (want / QUALITIES.len(), QUALITIES[want % QUALITIES.len()]);
                let (gf, gs) = (at(&full, t), at(&simple, t));
                s.frames += 1;
                s.full += (gf == want) as usize;
                s.majmin += (gs == class(wr as u8, wq.simple())) as usize;
                s.root += (gf != NO_CHORD && gf / QUALITIES.len() == wr) as usize;
                let mut folded = [0f32; wave_engine::chords::CLASSES];
                for (c, v) in probs.iter().enumerate() {
                    let f = if c == NO_CHORD { c } else { class((c / QUALITIES.len()) as u8, QUALITIES[c % QUALITIES.len()].simple()) };
                    folded[f] += v;
                }
                let raw = (0..folded.len()).max_by(|&a, &b| folded[a].total_cmp(&folded[b])).unwrap();
                s.raw_majmin += (raw == class(wr as u8, wq.simple())) as usize;
                if gf != want {
                    let name = |c: usize| if c == NO_CHORD { "N".to_string() } else { format!("{:?}", QUALITIES[c % QUALITIES.len()]) };
                    let rel = if gf == NO_CHORD { "N".into() } else { format!("+{}", (gf / QUALITIES.len() + 12 - wr) % 12) };
                    *confusions.entry(format!("{} → {} {}", name(want), rel, name(gf))).or_default() += 1;
                }
            }
            if k == 0 {
                if let Some(o) = out {
                    let path = o.join(format!("{}.wav", info.id));
                    wave_engine::sources::write_wav(&path, &x, SR).unwrap();
                    let labels: Vec<String> = p
                        .chords
                        .iter()
                        .map(|c| format!("{:.3}\t{:.3}\t{}", c.0 * spb, c.1 * spb, wave_engine::chords::chord_name(c.2, c.3, false)))
                        .collect();
                    std::fs::write(path.with_extension("txt"), labels.join("\n")).unwrap();
                }
            }
        }
        let pct = |n: usize| 100.0 * n as f64 / s.frames.max(1) as f64;
        println!(
            "{:<16} {:>6} {:>6.1}% {:>6.1}% {:>6.1}% {:>8.1}%",
            info.id,
            s.frames,
            pct(s.root),
            pct(s.majmin),
            pct(s.full),
            pct(s.raw_majmin)
        );
        total.frames += s.frames;
        total.root += s.root;
        total.majmin += s.majmin;
        total.full += s.full;
        total.raw_majmin += s.raw_majmin;
    }
    let pct = |n: usize| 100.0 * n as f64 / total.frames.max(1) as f64;
    println!(
        "{:<16} {:>6} {:>6.1}% {:>6.1}% {:>6.1}% {:>8.1}%",
        "all",
        total.frames,
        pct(total.root),
        pct(total.majmin),
        pct(total.full),
        pct(total.raw_majmin)
    );
    let mut top: Vec<_> = confusions.into_iter().collect();
    top.sort_by_key(|x| std::cmp::Reverse(x.1));
    println!("\nmost frequent errors (truth → interval and quality heard):");
    for (k, n) in top.iter().take(12) {
        println!("  {n:>5}  {k}");
    }
}
