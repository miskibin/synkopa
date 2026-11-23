//! Renders a short phrase on every Synkopa Keys instrument whose recordings are on disk, and
//! reports load time, memory and levels: the check that instruments are levelled alike.
//!   cargo run --release -p wave-engine --example keys_demo -- <kits folder> <out folder> [instrument ...]
//! `<kits folder>` holds one folder per library kit (`node scripts/keys-kits.mjs` fills
//! `target/keys-kits`); the bundled piano may also be its prepared folder (`app/resources/keys`).

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use wave_engine::keys::{KeysBank, CATALOG};
use wave_engine::*;

const SR: u32 = 48000;
const CH: Id = 7;

fn note(step: f32, len: f32, key: u8, v: f32) -> Note {
    Note { v, ..Note::new(CH, step, len, Some(key)) }
}

/// Four bars at 100 BPM: a soft-to-loud chord progression with a melody over it, in the
/// instrument's range.
fn phrase(lo: u8, hi: u8) -> Vec<Note> {
    let mid = ((lo as u16 + hi as u16) / 2).clamp(48, 64) as u8;
    let root = mid - (mid % 12);
    let chords: [[i8; 4]; 4] = [[0, 4, 7, 11], [-3, 0, 4, 7], [-7, -3, 0, 5], [-5, -1, 2, 7]];
    let mut n = vec![];
    for (bar, c) in chords.iter().enumerate() {
        let v = 0.35 + 0.18 * bar as f32;
        for (i, iv) in c.iter().enumerate() {
            let k = (root as i16 + *iv as i16 - 12 * (i == 0) as i16).clamp(lo as i16, hi as i16) as u8;
            n.push(note(bar as f32 * 16.0, 14.0, k, v));
        }
        for (j, iv) in [12i8, 14, 16, 19].iter().enumerate() {
            let k = (root as i16 + *iv as i16 + c[0] as i16).clamp(lo as i16, hi as i16) as u8;
            n.push(note(bar as f32 * 16.0 + 4.0 * j as f32, 3.0, k, (v + 0.15).min(1.0)));
        }
    }
    n
}

fn project(instrument: &str, articulation: Option<u8>, notes: Vec<Note>) -> Project {
    let mut p = Project::empty();
    p.bpm = 100.0;
    p.inserts[0].fx.clear();
    let keys = KeysState { instrument: instrument.into(), articulation, ..Default::default() };
    p.channels.push(Channel { name: "Keys".into(), root: 60, ..Channel::new(CH, "", "#4f8ef7", ChannelSrc::Keys { keys }) });
    p.patterns = vec![Pattern { id: 1, name: "P".into(), color: "#4f8ef7".into(), len: 16.0, min_len: 16.0, notes, swing: 0.0 }];
    p.clips = vec![Clip {
        id: 1,
        track: 0,
        start: 0.0,
        len: 16.0,
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
    p
}

fn write_wav(path: &Path, x: &[f32]) {
    let data: Vec<u8> = x.iter().flat_map(|v| ((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
    let mut h = vec![];
    h.extend(b"RIFF");
    h.extend((36 + data.len() as u32).to_le_bytes());
    h.extend(b"WAVEfmt ");
    h.extend(16u32.to_le_bytes());
    h.extend(1u16.to_le_bytes());
    h.extend(2u16.to_le_bytes());
    h.extend(SR.to_le_bytes());
    h.extend((SR * 4).to_le_bytes());
    h.extend(4u16.to_le_bytes());
    h.extend(16u16.to_le_bytes());
    h.extend(b"data");
    h.extend((data.len() as u32).to_le_bytes());
    h.extend(data);
    std::fs::write(path, h).unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (kits, out) = (Path::new(&args[0]), Path::new(&args[1]));
    std::fs::create_dir_all(out).unwrap();
    // `strings@26`: an instrument with one of its articulations
    let wanted: Vec<(String, Option<u8>)> = if args.len() < 3 {
        CATALOG.iter().map(|i| (i.id.to_string(), None)).collect()
    } else {
        args[2..].iter().map(|a| a.split_once('@').map_or((a.clone(), None), |(i, k)| (i.to_string(), k.parse().ok()))).collect()
    };
    for (id, art) in wanted {
        let Some(info) = wave_engine::keys::instrument(&id) else { continue };
        let tag = art.map_or(id.clone(), |k| format!("{id}@{k}"));
        let dir = kits.join(info.kit);
        if !KeysBank::present(info, &dir) {
            println!("{:<14} not on disk ({})", info.id, dir.display());
            continue;
        }
        let t = Instant::now();
        let bank = KeysBank::load_instrument(info, &dir).unwrap_or_else(|e| panic!("{}: {e}", info.id));
        let load = t.elapsed();
        let e = Engine::new(project(info.id, art, phrase(info.range.0, info.range.1)), Arc::new(NoPlugins));
        println!("{:<14} {} zones, {} recordings, {:.0} MB in memory", info.id, bank.zones(), bank.samples(), bank.bytes() as f64 / 1e6);
        e.set_keys_bank(info.id, Some(Arc::new(bank)));
        let x = e.render(0.0, 18.0, 0.0, SR).unwrap();
        let pk = x.iter().fold(0f32, |a, b| a.max(b.abs()));
        let rms = (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len() as f64).sqrt();
        println!(
            "{:<14} load {:>5.2} s  peak {:>6.1} dBFS  rms {:>6.1} dBFS",
            info.id,
            load.as_secs_f64(),
            20.0 * pk.log10(),
            20.0 * rms.log10()
        );
        write_wav(&out.join(format!("{}.wav", tag.replace('@', "-"))), &x);
    }
}
