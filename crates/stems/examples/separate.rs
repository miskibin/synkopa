//! Headless stem separation: `separate <model.onnx> <mix.wav|flac|mp3> <out-dir> [threads]`
//! writes `<out-dir>/<source>.wav` (32-bit float, 44.1 kHz) for every source of the model and
//! prints the time it took per minute of audio.

use std::path::{Path, PathBuf};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: separate <model.onnx> <mix> <out-dir> [threads]");
        std::process::exit(2);
    }
    if let Err(e) =
        run(Path::new(&args[0]), Path::new(&args[1]), Path::new(&args[2]), args.get(3).and_then(|t| t.parse().ok()).unwrap_or(0))
    {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(model: &Path, input: &Path, out: &Path, threads: usize) -> Result<(), String> {
    let t0 = Instant::now();
    let sep = wave_stems::Separator::load(model, threads)?;
    let t_load = t0.elapsed();

    let t1 = Instant::now();
    let audio = wave_audio_file::decode_stereo(input, |_| None).map_err(|e| e.to_string())?;
    let (mut l, mut r): (Vec<f32>, Vec<f32>) = audio.data.as_chunks::<2>().0.iter().map(|f| (f[0], f[1])).unzip();
    let sr = wave_stems::SR as f64;
    if (audio.sample_rate - sr).abs() > 0.5 {
        l = wave_stems::resample(&l, audio.sample_rate, sr);
        r = wave_stems::resample(&r, audio.sample_rate, sr);
    }
    let t_decode = t1.elapsed();

    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let spec = hound::WavSpec { channels: 2, sample_rate: wave_stems::SR, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let paths: Vec<PathBuf> = sep.sources().iter().map(|s| out.join(format!("{s}.wav"))).collect();
    let mut writers = paths.iter().map(|p| hound::WavWriter::create(p, spec).map_err(|e| e.to_string())).collect::<Result<Vec<_>, _>>()?;

    let t2 = Instant::now();
    sep.separate(
        &l,
        &r,
        |f| {
            eprint!("\r{:5.1}%", f * 100.0);
            true
        },
        |stems| {
            for (w, s) in writers.iter_mut().zip(stems) {
                for v in s {
                    w.write_sample(*v).map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        },
    )
    .map_err(|e| e.to_string())?;
    let t_sep = t2.elapsed();
    for w in writers {
        w.finalize().map_err(|e| e.to_string())?;
    }
    let minutes = l.len() as f64 / sr / 60.0;
    eprintln!(
        "\r{:.1} s of audio: load {:.2} s, decode {:.2} s, separate {:.2} s = {:.1} s per minute of audio ({:.2}x realtime)",
        minutes * 60.0,
        t_load.as_secs_f64(),
        t_decode.as_secs_f64(),
        t_sep.as_secs_f64(),
        t_sep.as_secs_f64() / minutes,
        minutes * 60.0 / t_sep.as_secs_f64()
    );
    for p in paths {
        println!("{}", p.display());
    }
    Ok(())
}
