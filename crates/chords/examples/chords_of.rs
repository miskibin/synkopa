//! Chords of an audio file, as the app decodes them (its own detected tempo as the beat grid).
//!   cargo run --release -p wave-chords --example chords_of -- <chordnet.onnx> <audio file> [simple]
use std::path::Path;
use wave_engine::chords::{decode, key_uses_flats, ChordModel, Detail};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let m = wave_chords::ChordMini::load(Path::new(&a[0])).unwrap();
    let audio = wave_engine::sources::Audio::load_path(&a[1], Path::new(".")).unwrap();
    let mono = wave_engine::sources::mono_of(&audio);
    let secs = mono.len() as f64 / audio.sr as f64;
    let music = wave_engine::music::analyze(&mono, audio.sr);
    let tempo = music.tempo.clone().expect("no tempo");
    let (cs, _) = music.chroma_sum(0.0, 1e9);
    let key = wave_engine::music::estimate_key(&cs, music.tuning_cents).unwrap();
    let flats = key_uses_flats(key.root, key.minor);
    println!(
        "{:.1} s, {:.2} BPM (first beat {:.3} s), key {}",
        secs,
        tempo.bpm,
        tempo.first_beat,
        wave_engine::music::key_name(key.root, key.minor)
    );
    let f = m.analyze(&mono, audio.sr).unwrap();
    let spb = 60.0 / tempo.bpm;
    let beat_of = |t: f64| (t - tempo.first_beat) / spb;
    let detail = if a.get(2).is_some_and(|s| s == "simple") { Detail::Simple } else { Detail::Full };
    let segs = decode(&f, (0.0, secs), beat_of, (beat_of(0.0), beat_of(secs)), detail, 0, flats);
    for s in &segs {
        let bar = |b: f64| b / 4.0 + 1.0;
        println!("bar {:>6.2} – {:>6.2}  {:<8} {:>3.0}%", bar(s.start), bar(s.end), s.name, s.confidence * 100.0);
    }
}
