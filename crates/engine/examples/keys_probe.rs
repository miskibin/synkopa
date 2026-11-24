//! Prints which regions a Keys instrument picks for repeated notes (a check of round robins and articulations).
//!   cargo run --release -p wave-engine --example keys_probe -- <instrument> <key> <velocity 0..1> [keyswitch]
use std::path::Path;
use wave_engine::keys::instrument;
use wave_engine::sfz::{self, Trigger};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let info = instrument(&a[0]).expect("instrument");
    let inst = sfz::parse(info.sfz, Path::new(".")).unwrap();
    let names: Vec<String> = inst.regions.iter().map(|r| r.sample.clone()).collect();
    let bank = wave_engine::keys::bank_from(
        info.sfz,
        names.iter().map(|n| (n.as_str(), wave_engine::sources::Audio::from_stereo(vec![0.1; 8], 48000, 2))).collect(),
    )
    .unwrap();
    let (key, vel): (u8, f32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let sw = a.get(3).and_then(|s| s.parse().ok());
    for rr in 0..6 {
        let mut z = vec![];
        bank.pick(Trigger::Attack, key, vel, rr, 0.5, sw, 1.0, 0.0, |h| z.push(names[h.zone as usize].clone()));
        println!("rr {rr}: {z:?}");
    }
}
