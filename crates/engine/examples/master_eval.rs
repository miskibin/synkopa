//! Runs the Master's whole-song listen on an audio file with a given assistant config, for
//! listening tests of the assistant's settings:
//!
//! ```text
//! cargo run --release -p wave-engine --example master_eval -- IN.(wav|mp3|flac) OUT.wav TARGET [CONFIG.json]
//! ```
//!
//! TARGET indexes `dsp::master::TARGETS` (2 = −12 LUFS). CONFIG overrides fields of
//! `mastering::Config` (missing fields keep the shipped defaults). Prints the parameters and the
//! report as JSON; writes the mastered audio as 32-bit float WAV at the file's own rate.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use wave_engine::autovocal::render_stereo_with;
use wave_engine::dsp::master::Master;
use wave_engine::mastering::{listen_with, Config};
use wave_engine::model::FxKind;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: master_eval IN OUT TARGET [CONFIG.json]");
        std::process::exit(2);
    }
    let cfg: Config = match a.get(4) {
        Some(p) => serde_json::from_str(&std::fs::read_to_string(p).expect("read config")).expect("config json"),
        None => Config::default(),
    };
    let target: usize = a[3].parse().expect("target 0..4");
    let d = wave_audio_file::decode_stereo(Path::new(&a[1]), |_| None).expect("decode");
    let sr = d.sample_rate as f32;
    let (l, r): (Vec<f32>, Vec<f32>) = d.data.as_chunks::<2>().0.iter().map(|f| (f[0], f[1])).unzip();
    let cur = wave_engine::catalog::default_params(FxKind::Master);
    let got = listen_with(&l, &r, sr, &cur, target, &cfg, &AtomicBool::new(false), &mut |_| {}).expect("listen");
    let params: BTreeMap<String, f64> = got.params.clone();
    let (ol, or) = render_stereo_with(&mut Master::default(), FxKind::Master, &params, &l, &r, sr);
    let inter: Vec<f32> = ol.iter().zip(&or).flat_map(|(x, y)| [*x, *y]).collect();
    wave_engine::sources::write_wav(Path::new(&a[2]), &inter, sr as u32).expect("write");
    println!("{}", serde_json::json!({ "config": cfg, "params": params, "report": got.report }));
}
