//! Listening and measurement renders for Stereo, Multiband and Analyzer:
//! `cargo run -p wave-engine --release --example plugin_demos -- <out dir>`
//!
//! Renders 16 bars of the demo song through the engine (offline bounce) with the new effects on
//! the master or one insert, and writes 44.1 kHz stereo WAVs. Segments of one file are RMS
//! matched, so only the character differs. Also prints the Analyzer's integrated loudness and
//! true peak for the dry mix, to compare with an independent meter (ffmpeg's ebur128).

use std::path::Path;
use std::sync::Arc;

use wave_engine::catalog::{load_preset, new_fx, param_block, presets};
use wave_engine::demo::demo_project;
use wave_engine::dsp::create;
use wave_engine::ops::Op;
use wave_engine::processor::METERS;
use wave_engine::{Engine, FxKind, NoPlugins, ProcessCtx, Project};

const SR: u32 = 44_100;
/// Bars 17–33 of the demo (beats): the drop, with drums, 808, pads and lead.
const FROM: f64 = 64.0;
const TO: f64 = 128.0;

fn fx_with(kind: FxKind, id: u64, preset: &str, edit: &[(&str, f64)]) -> wave_engine::Fx {
    let mut f = new_fx(id, kind, 136.0);
    let i = presets(kind).iter().position(|p| p.name == preset).unwrap_or_else(|| panic!("no preset {preset}"));
    assert!(load_preset(&mut f, i, 136.0));
    for (k, v) in edit {
        f.params.insert((*k).to_string(), *v);
    }
    f
}

/// Interleaved stereo render of the demo with `fx` appended to `insert` (master when None).
fn render(fx: &[(Option<&str>, wave_engine::Fx)]) -> Vec<f32> {
    let p: Project = demo_project();
    let e = Engine::new(p, Arc::new(NoPlugins));
    let mut ops = vec![];
    for (ins, f) in fx {
        let proj = e.project();
        let id = match ins {
            None => proj.inserts.iter().find(|m| m.master).unwrap().id,
            Some(name) => proj.inserts.iter().find(|m| m.name == *name).unwrap_or_else(|| panic!("no insert {name}")).id,
        };
        ops.push(Op::PutFx { insert_id: id, fx: f.clone(), index: None });
    }
    if !ops.is_empty() {
        e.apply(&ops, false, None).unwrap();
    }
    e.render(FROM, TO, 0.5, SR).unwrap()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn write(path: &Path, segs: &[Vec<f32>]) {
    let spec = hound::WavSpec { channels: 2, sample_rate: SR, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    let r0 = rms(&segs[0]);
    let peak = segs.iter().map(|s| s.iter().fold(0f32, |m, v| m.max(v.abs())) * r0 / rms(s).max(1e-9)).fold(0f32, f32::max);
    let k = (0.891 / peak.max(1e-9)).min(1.0);
    for (i, s) in segs.iter().enumerate() {
        if i > 0 {
            for _ in 0..SR as usize * 2 {
                w.write_sample(0i16).unwrap();
            }
        }
        let g = k * r0 / rms(s).max(1e-9);
        for &v in s {
            w.write_sample(((v * g).clamp(-1.0, 1.0) * 32767.0) as i16).unwrap();
        }
    }
    w.finalize().unwrap();
    println!("wrote {}", path.display());
}

/// The Analyzer run over an interleaved render, as the engine would while the song plays.
fn analyze(x: &[f32]) -> [f32; METERS] {
    let f = new_fx(1, FxKind::Analyzer, 136.0);
    let mut p = create(FxKind::Analyzer).unwrap();
    p.prepare(SR as f32, 512);
    p.set_params(&param_block(&f));
    let ctx = ProcessCtx { sample_rate: SR as f32, bpm: 136.0, beat: 0.0, playing: true };
    for c in x.chunks(1024) {
        let (mut l, mut r): (Vec<f32>, Vec<f32>) = c.chunks(2).map(|s| (s[0], s[1])).unzip();
        p.process(&mut l, &mut r, &ctx);
    }
    let mut m = [0f32; METERS];
    p.meters(&mut m);
    m
}

fn main() {
    let out = std::env::args().nth(1).expect("out dir");
    let out = Path::new(&out);
    std::fs::create_dir_all(out).unwrap();

    let dry = render(&[]);
    let m = analyze(&dry);
    println!("dry mix: integrated {:.2} LUFS, LRA {:.1} LU, true peak {:.2} dBTP, correlation {:.2}", m[2], m[3], m[4], m[6]);
    write(&out.join("dry.wav"), std::slice::from_ref(&dry));

    // Multiband on the master: dry, default (40 %), classic OTT (100 %)
    let glue = render(&[(None, fx_with(FxKind::Multiband, 90_001, "Domyślny — klej", &[]))]);
    let ott = render(&[(None, fx_with(FxKind::Multiband, 90_001, "Klasyczny OTT (100 %)", &[]))]);
    write(&out.join("multiband_master.wav"), &[dry.clone(), glue, ott]);

    // Stereo on the master: dry, mono, wider with the bass mono
    let mono = render(&[(None, fx_with(FxKind::Stereo, 90_002, "Mono", &[]))]);
    let wide = render(&[(None, fx_with(FxKind::Stereo, 90_002, "Szerzej, bas w mono", &[("width", 160.0)]))]);
    write(&out.join("stereo_master.wav"), &[dry.clone(), mono, wide]);
    let mw = analyze(&dry);
    println!("correlation dry {:.2}", mw[6]);

    // Spread on a mono source: the demo summed to mono, then spread 70 % with the bass mono
    let mono_mix: Vec<f32> = dry
        .chunks(2)
        .flat_map(|s| {
            let m = 0.5 * (s[0] + s[1]);
            [m, m]
        })
        .collect();
    let mut p = create(FxKind::Stereo).unwrap();
    p.prepare(SR as f32, 512);
    p.set_params(&param_block(&fx_with(FxKind::Stereo, 1, "Mono → stereo (spread)", &[("spread", 70.0)])));
    let ctx = ProcessCtx { sample_rate: SR as f32, bpm: 136.0, beat: 0.0, playing: true };
    let mut spread = Vec::with_capacity(mono_mix.len());
    for c in mono_mix.chunks(1024) {
        let (mut l, mut r): (Vec<f32>, Vec<f32>) = c.chunks(2).map(|s| (s[0], s[1])).unzip();
        p.process(&mut l, &mut r, &ctx);
        for i in 0..l.len() {
            spread.push(l[i]);
            spread.push(r[i]);
        }
    }
    let ms = analyze(&spread);
    println!("spread: correlation {:.2}; mono sum error {:.2e}", ms[6], {
        mono_mix.chunks(2).zip(spread.chunks(2)).map(|(a, b)| (a[0] - 0.5 * (b[0] + b[1])).abs()).fold(0f32, f32::max)
    });
    write(&out.join("stereo_spread.wav"), &[mono_mix, spread]);
    slicer_demo(out);
}

/// Slicer check on real drums: the demo's drum stem, sliced at its transients; prints where the
/// slices fall against the 16th grid, and writes the loop then a re-chopped version.
fn slicer_demo(out: &Path) {
    let e = Engine::new(demo_project(), Arc::new(NoPlugins));
    let p = e.project();
    let names: Vec<String> = p.inserts.iter().map(|m| m.name.clone()).collect();
    println!("inserts: {names:?}");
    let ins = p
        .inserts
        .iter()
        .find(|m| m.name == "Drum Bus")
        .or_else(|| p.inserts.iter().find(|m| m.name.to_lowercase().contains("kick")))
        .unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let (from, to) = (64.0, 72.0);
    let mut stem = e.render_stem(ins.id, from, to, 0.0, SR, &cancel).unwrap();
    // + hats and snares if they have their own inserts
    for m in p
        .inserts
        .iter()
        .filter(|m| ins.name != "Drum Bus" && ["hat", "snare", "clap", "perc", "tops"].iter().any(|k| m.name.to_lowercase().contains(k)))
    {
        let s = e.render_stem(m.id, from, to, 0.0, SR, &cancel).unwrap();
        for (a, b) in stem.iter_mut().zip(s) {
            *a += b;
        }
    }
    let audio = wave_engine::sources::Audio::from_stereo(stem.clone(), SR, 2);
    let onsets = wave_engine::slices::onsets(&audio);
    let secs = audio.frames as f64 / SR as f64;
    let spb = 60.0 / p.bpm;
    let kept: Vec<f32> = onsets.iter().filter(|o| o.strength > 0.08).map(|o| o.pos).collect();
    let mut off = vec![];
    for pos in &kept {
        let beat = *pos as f64 * secs / spb;
        let grid = (beat * 4.0).round() / 4.0;
        off.push((beat - grid) * spb * 1000.0);
    }
    println!(
        "bpm {}, {} slices; offsets from the 16th grid (ms): {:?}",
        p.bpm,
        kept.len(),
        off.iter().map(|x| x.round() as i32).collect::<Vec<_>>()
    );
    // is each marker before its attack? level in the 3 ms before it against the peak after it
    let mono: Vec<f32> = stem.chunks(2).map(|c| 0.5 * (c[0] + c[1])).collect();
    let ms = SR as usize / 1000;
    let pre: Vec<String> = kept
        .iter()
        .map(|pos| {
            let i = (*pos as f64 * mono.len() as f64) as usize;
            let before = mono[i.saturating_sub(3 * ms)..i].iter().fold(0f32, |m, v| m.max(v.abs()));
            let after = mono[i..(i + 20 * ms).min(mono.len())].iter().fold(0f32, |m, v| m.max(v.abs()));
            format!("{:.0}%", 100.0 * before / after.max(1e-9))
        })
        .collect();
    println!("level just before each marker vs its hit: {pre:?}");
    // re-chop through the sampler's slice mode: the stem on a sampler channel, its slices played
    // in reverse order, each for its own length (what "Dump to pattern" + moving notes gives)
    let wav = out.join("drum_stem.wav");
    wave_engine::sources::write_wav(&wav, &stem, SR).unwrap();
    let mut q = Project::empty();
    q.bpm = p.bpm;
    q.inserts[0].fx.clear();
    let e2 = Engine::new(q, Arc::new(NoPlugins));
    let (srcs, _) = e2.import_audio(&[wav], None).unwrap();
    let mut q = e2.project();
    let ch = q.alloc_id();
    let mut slices = vec![0.0f32];
    slices.extend(kept.iter().copied());
    let channel = wave_engine::Channel::new(
        ch,
        "Chop",
        "#4f8ef7",
        wave_engine::ChannelSrc::Sample { source: srcs[0], sampler: Default::default(), slices: slices.clone() },
    );
    let steps_per_s = p.bpm / 60.0 * 4.0;
    let mut t = 0.0f64;
    let mut notes = vec![];
    for k in (0..slices.len()).rev() {
        let len_s = (slices.get(k + 1).copied().unwrap_or(1.0) - slices[k]) as f64 * secs;
        notes.push(wave_engine::Note::new(ch, (t * steps_per_s) as f32, (len_s * steps_per_s) as f32, Some(60 + k as u8)));
        t += len_s;
    }
    let beats = to - from;
    let pattern = wave_engine::Pattern { id: 1, name: "P".into(), color: "#4f8ef7".into(), len: beats, min_len: beats, notes, swing: 0.0 };
    let mut clip = e2.project().clips.first().cloned();
    let _ = clip.take();
    let clip = wave_engine::Clip {
        id: 2,
        track: 0,
        start: 0.0,
        len: beats,
        offset: 0.0,
        kind: wave_engine::ClipKind::Pat,
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
    };
    e2.apply(
        &[
            wave_engine::Op::PutChannel { channel, index: None },
            wave_engine::Op::PutPattern { pattern },
            wave_engine::Op::PutClips { clips: vec![clip] },
        ],
        false,
        None,
    )
    .unwrap();
    let chop = e2.render(0.0, beats, 0.0, SR).unwrap();
    write(&out.join("slicer_drums.wav"), &[stem, chop]);
}
