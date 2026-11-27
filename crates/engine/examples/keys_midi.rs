//! Renders a MIDI file on a Synkopa Instruments instrument (all parts merged on one channel).
//!   cargo run --release -p wave-engine --example keys_midi -- <kit folder | file.sfz> <instrument> <file.mid> <out.wav> [tempo scale] [articulation key]
use std::path::Path;
use std::sync::Arc;
use wave_engine::keys::{instrument, KeysBank};
use wave_engine::*;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let info = instrument(&a[1]).expect("instrument");
    // `<kit folder>` may also be an .sfz file: its own mapping over its recordings (to compare mappings)
    let bank =
        if a[0].ends_with(".sfz") { KeysBank::load_sfz(Path::new(&a[0])) } else { KeysBank::load_instrument(info, Path::new(&a[0])) }
            .unwrap();
    let m = wave_engine::midi_file::parse(&std::fs::read(&a[2]).unwrap()).unwrap();
    let bpm = m.bpm.unwrap_or(120.0) * a.get(4).and_then(|s| s.parse::<f64>().ok()).unwrap_or(1.0);
    const CH: Id = 7;
    let notes: Vec<Note> = m
        .parts
        .iter()
        .flat_map(|p| p.notes.iter())
        .map(|n| Note { v: n.vel as f32 / 127.0, ..Note::new(CH, (n.start * 4.0) as f32, (n.len * 4.0).max(0.25) as f32, Some(n.key)) })
        .collect();
    let end = m.end.ceil();
    let mut p = Project::empty();
    p.bpm = bpm;
    p.inserts[0].fx.clear();
    let articulation = a.get(5).and_then(|s| s.parse().ok());
    let keys = KeysState { instrument: info.id.into(), articulation, ..Default::default() };
    p.channels.push(Channel { name: "Keys".into(), root: 60, ..Channel::new(CH, "", "#4f8ef7", ChannelSrc::Keys { keys }) });
    p.patterns = vec![Pattern {
        id: 1,
        name: "P".into(),
        color: "#4f8ef7".into(),
        len: end,
        min_len: end,
        bar: 4.0,
        notes,
        swing: 0.0,
        channel_loops: std::collections::BTreeMap::new(),
    }];
    p.clips = vec![Clip {
        id: 1,
        track: 0,
        start: 0.0,
        len: end,
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
    let e = Engine::new(p, Arc::new(NoPlugins));
    e.set_keys_bank(info.id, Some(Arc::new(bank)));
    let x = e.render(0.0, end, 2.0, 48000).unwrap();
    let pk = x.iter().fold(0f32, |a, b| a.max(b.abs()));
    let g = 0.7 / pk.max(1e-6); // normalise for listening
    let data: Vec<u8> = x.iter().flat_map(|v| (((v * g).clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
    let mut h = vec![];
    h.extend(b"RIFF");
    h.extend((36 + data.len() as u32).to_le_bytes());
    h.extend(b"WAVEfmt ");
    h.extend(16u32.to_le_bytes());
    h.extend(1u16.to_le_bytes());
    h.extend(2u16.to_le_bytes());
    h.extend(48000u32.to_le_bytes());
    h.extend((48000u32 * 4).to_le_bytes());
    h.extend(4u16.to_le_bytes());
    h.extend(16u16.to_le_bytes());
    h.extend(b"data");
    h.extend((data.len() as u32).to_le_bytes());
    h.extend(data);
    std::fs::write(&a[3], h).unwrap();
    println!(
        "{} notes, {:.0} beats at {bpm:.0} BPM, peak {:.1} dBFS",
        m.parts.iter().map(|p| p.notes.len()).sum::<usize>(),
        end,
        20.0 * pk.log10()
    );
}
