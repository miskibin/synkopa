//! Listening demos for Patina (docs/patina.md):
//! `cargo run -p wave-engine --release --example patina_demos -- <audio file> <out dir> [start s] [seconds]`
//!
//! Writes MP3s: the excerpt dry, then through each factory preset (one file per preset, the
//! dry excerpt first and 0.5 s of silence between), and each noise type alone over silence.
//! Nothing is level-matched: what Patina does to the level is part of what is heard.

use std::path::Path;

use wave_engine::catalog::{load_preset, new_fx, param_block, presets};
use wave_engine::dsp::create;
use wave_engine::dsp::patina::NOISE_TYPES;
use wave_engine::{sources, FxKind, ProcessCtx};

fn render(fx: &wave_engine::Fx, l: &[f32], r: &[f32], sr: f32) -> (Vec<f32>, Vec<f32>) {
    let mut p = create(FxKind::Patina).unwrap();
    p.prepare(sr, 512);
    p.set_params(&param_block(fx));
    let (mut l, mut r) = (l.to_vec(), r.to_vec());
    let ctx = ProcessCtx { sample_rate: sr, bpm: 90.0, beat: 0.0, playing: true };
    for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
        p.process(a, b, &ctx);
    }
    let lat = p.latency_samples();
    (l[lat..].to_vec(), r[lat..].to_vec())
}

fn interleave(segs: &[(Vec<f32>, Vec<f32>)], sr: f32) -> Vec<f32> {
    let gap = (sr * 0.5) as usize;
    let mut out = Vec::new();
    for (i, (l, r)) in segs.iter().enumerate() {
        if i > 0 {
            out.extend(std::iter::repeat_n(0.0, gap * 2));
        }
        for (a, b) in l.iter().zip(r) {
            out.push(a.clamp(-1.0, 1.0));
            out.push(b.clamp(-1.0, 1.0));
        }
    }
    out
}

fn slug(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'ą' => 'a',
            'ć' => 'c',
            'ę' => 'e',
            'ł' => 'l',
            'ń' => 'n',
            'ó' => 'o',
            'ś' => 's',
            'ź' | 'ż' => 'z',
            c if c.is_ascii_alphanumeric() => c.to_ascii_lowercase(),
            _ => '-',
        })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let src = Path::new(a.get(1).expect("audio file"));
    let out = Path::new(a.get(2).expect("out dir"));
    let start: f64 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let secs: f64 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(10.0);
    std::fs::create_dir_all(out).unwrap();
    let d = wave_audio_file::decode_stereo(src, |_| None).expect("decode");
    let sr = d.sample_rate as f32;
    let s0 = (start * d.sample_rate) as usize;
    let n = ((secs * d.sample_rate) as usize).min(d.frames.saturating_sub(s0));
    let (mut l, mut r) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for i in s0..s0 + n {
        l.push(d.data[i * 2]);
        r.push(d.data[i * 2 + 1]);
    }
    let dry = (l.clone(), r.clone());
    for (i, pr) in presets(FxKind::Patina).iter().enumerate() {
        let mut fx = new_fx(1, FxKind::Patina, 90.0);
        load_preset(&mut fx, i, 90.0);
        let wet = render(&fx, &l, &r, sr);
        // how the preset moves the music's level (noise left out), K-weighting skipped: RMS
        let mut quiet = fx.clone();
        quiet.params.insert("noise".into(), 0.0);
        let q = render(&quiet, &l, &r, sr);
        let rms = |a: &[f32], b: &[f32]| ((a.iter().chain(b).map(|v| v * v).sum::<f32>()) / (a.len() + b.len()).max(1) as f32).sqrt();
        println!("level {:>+5.1} dB  {}", 20.0 * (rms(&q.0, &q.1) / rms(&l, &r)).log10(), pr.name);
        let path = out.join(format!("{:02}-{}.mp3", i, slug(pr.name)));
        sources::write_mp3(&path, &interleave(&[dry.clone(), wet], sr), sr as u32, 192, None).unwrap();
        println!("wrote {}", path.display());
    }
    // CPU: every module on (the last preset, with a band split), and a bus preset, 128-sample blocks
    for k in [presets(FxKind::Patina).len() - 1, 1] {
        let mut fx = new_fx(1, FxKind::Patina, 90.0);
        load_preset(&mut fx, k, 90.0);
        let mut p = create(FxKind::Patina).unwrap();
        p.prepare(sr, 128);
        p.set_params(&param_block(&fx));
        let (mut bl, mut br) = (l.clone(), r.clone());
        let ctx = ProcessCtx { sample_rate: sr, bpm: 90.0, beat: 0.0, playing: true };
        let t0 = std::time::Instant::now();
        for (a, b) in bl.chunks_mut(128).zip(br.chunks_mut(128)) {
            p.process(a, b, &ctx);
        }
        let el = t0.elapsed().as_secs_f64();
        let name = presets(FxKind::Patina)[k].name;
        println!(
            "cpu: {:.0} ns per stereo sample, {:.2} % of one core in real time ({name})",
            el * 1e9 / l.len() as f64,
            100.0 * el / (l.len() as f64 / sr as f64)
        );
    }
    let silence = vec![0.0f32; (sr * 5.0) as usize];
    for (t, name) in NOISE_TYPES.iter().enumerate() {
        let mut fx = new_fx(1, FxKind::Patina, 90.0);
        fx.params = wave_engine::catalog::default_params(FxKind::Patina);
        fx.params.insert("noise".into(), 60.0);
        fx.params.insert("ntype".into(), t as f64);
        let wet = render(&fx, &silence, &silence, sr);
        let path = out.join(format!("noise-{}-{}.mp3", t, slug(name)));
        sources::write_mp3(&path, &interleave(&[wet], sr), sr as u32, 192, None).unwrap();
        println!("wrote {}", path.display());
    }
}
