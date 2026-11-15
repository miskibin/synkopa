//! Hosting real LV2 plugins: scan, instantiate, process a deterministic test signal,
//! state round trip, latency, several sample rates and odd block sizes.
//!
//! Needs liblilv and installed plugins. CI installs them (`swh-lv2`, `x42-plugins`,
//! `lsp-plugins-lv2`) and sets `WAVE_REQUIRE_LV2=1`, which turns "nothing to test" into a
//! failure; elsewhere the tests skip with a message.

#![cfg(target_os = "linux")]

use std::time::{Duration, Instant};

use wave_engine::{ProcessCtx, Processor};
use wave_lv2::{instantiate_effect, scan, status, Lv2Effect, Lv2Handle, PluginInfo};

/// Mono amplifiers with a `gain` port in dB: a known gain, through the dual-mono path.
const GAIN_PLUGINS: [&str; 2] = ["http://plugin.org.uk/swh-plugins/amp", "http://lv2plug.in/plugins/eg-amp"];

fn required() -> bool {
    std::env::var_os("WAVE_REQUIRE_LV2").is_some_and(|v| v == "1")
}

/// The scan, or `None` (skip) when LV2 cannot be tested here.
fn effects() -> Option<Vec<PluginInfo>> {
    if let Err(e) = status() {
        assert!(!required(), "WAVE_REQUIRE_LV2=1 but {e}");
        eprintln!("skipping: {e}");
        return None;
    }
    let list = scan(false).expect("scan");
    if list.is_empty() {
        assert!(!required(), "WAVE_REQUIRE_LV2=1 but no LV2 effects are installed");
        eprintln!("skipping: no LV2 effects installed");
        return None;
    }
    Some(list)
}

fn ctx(sr: f32) -> ProcessCtx {
    ProcessCtx { sample_rate: sr, bpm: 120.0, beat: 0.0, playing: true }
}

/// A 440 Hz sine of amplitude 0.25 on the left, 660 Hz on the right, from frame `at`.
fn signal(sr: f32, at: usize, l: &mut [f32], r: &mut [f32]) {
    for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
        let t = (at + i) as f32 / sr;
        *a = 0.25 * (std::f32::consts::TAU * 440.0 * t).sin();
        *b = 0.25 * (std::f32::consts::TAU * 660.0 * t).sin();
    }
}

/// Run `seconds` of the test signal through `fx` in calls of `call` frames; returns
/// (rms in, rms out) per side over the second half, and whether every sample was finite.
fn run(fx: &mut dyn Processor, sr: f32, seconds: f32, call: usize) -> ([f64; 2], [f64; 2], bool) {
    let total = (sr * seconds) as usize;
    let (mut l, mut r) = (vec![0.0f32; call], vec![0.0f32; call]);
    let (mut sin, mut sout, mut n, mut finite) = ([0.0f64; 2], [0.0f64; 2], 0usize, true);
    let mut at = 0;
    while at < total {
        let k = call.min(total - at);
        signal(sr, at, &mut l[..k], &mut r[..k]);
        let (il, ir) = (l.clone(), r.clone());
        fx.process(&mut l[..k], &mut r[..k], &ctx(sr));
        finite &= l[..k].iter().chain(r[..k].iter()).all(|v| v.is_finite());
        if at >= total / 2 {
            for (((a, b), c), d) in il[..k].iter().zip(&ir[..k]).zip(&l[..k]).zip(&r[..k]) {
                sin[0] += f64::from(*a).powi(2);
                sin[1] += f64::from(*b).powi(2);
                sout[0] += f64::from(*c).powi(2);
                sout[1] += f64::from(*d).powi(2);
            }
            n += k;
        }
        at += k;
    }
    let rms = |x: [f64; 2]| x.map(|v| (v / n.max(1) as f64).sqrt());
    (rms(sin), rms(sout), finite)
}

fn param_id(h: &Lv2Handle, symbol: &str) -> u32 {
    h.params().iter().find(|p| p.symbol == symbol).unwrap_or_else(|| panic!("no `{symbol}` parameter in {:?}", h.params())).id
}

/// Normalized value that makes `symbol` read `real`.
fn set_real(h: &Lv2Handle, symbol: &str, real: f64) {
    let p = h.params().into_iter().find(|p| p.symbol == symbol).unwrap();
    // bisection over the normalized range (works for linear and log mappings)
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        h.set_param(p.id, mid).unwrap();
        let now = h.params().into_iter().find(|q| q.id == p.id).unwrap().real;
        if now < real {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let now = h.params().into_iter().find(|q| q.id == p.id).unwrap().real;
    assert!((now - real).abs() < 1e-3, "{symbol} reads {now}, wanted {real}");
}

#[test]
fn the_scan_lists_effects_with_reasons_and_no_instruments() {
    let Some(list) = effects() else { return };
    eprintln!("{} LV2 effects, {} unsupported", list.len(), list.iter().filter(|p| p.unsupported.is_some()).count());
    for p in list.iter().filter(|p| p.unsupported.is_some()).take(10) {
        eprintln!("  unsupported: {} ({}): {}", p.name, p.cid, p.unsupported.as_deref().unwrap());
    }
    assert!(list.iter().any(|p| p.unsupported.is_none()), "nothing usable");
    for p in &list {
        assert!(!p.cid.is_empty() && !p.name.is_empty(), "{p:?}");
        assert!(p.unsupported.as_deref().is_none_or(|w| !w.is_empty()), "{p:?}");
    }
    let json = serde_json::to_value(&list[0]).unwrap();
    for key in ["path", "cid", "name", "vendor", "category", "realtime"] {
        assert!(json.get(key).is_some(), "{key} missing from {json}");
    }
    // a rescan reads the directories again and finds the same plugins
    let again = scan(true).unwrap();
    assert_eq!(again.iter().map(|p| &p.cid).collect::<Vec<_>>(), list.iter().map(|p| &p.cid).collect::<Vec<_>>());
}

#[test]
fn a_mono_amplifier_runs_dual_mono_with_its_exact_gain_at_every_rate_and_block_size() {
    let Some(list) = effects() else { return };
    let Some(uri) = GAIN_PLUGINS.iter().find(|u| list.iter().any(|p| p.cid == **u && p.unsupported.is_none())) else {
        assert!(!required(), "WAVE_REQUIRE_LV2=1 but none of {GAIN_PLUGINS:?} is installed");
        eprintln!("skipping: none of {GAIN_PLUGINS:?} installed");
        return;
    };
    for sr in [44_100.0f32, 48_000.0, 96_000.0] {
        for (max_block, call) in [(64usize, 37usize), (256, 256), (512, 1000), (4096, 1)] {
            let (mut fx, h) = instantiate_effect(uri, None, sr, max_block).unwrap();
            set_real(&h, "gain", -6.0);
            let (rin, rout, finite) = run(&mut fx, sr, 0.25, call);
            assert!(finite, "{uri} @ {sr}/{max_block}/{call}: NaN or inf");
            let want = 10f64.powf(-6.0 / 20.0);
            for side in 0..2 {
                let g = rout[side] / rin[side];
                assert!((g - want).abs() < 0.01, "{uri} @ {sr} Hz, max {max_block}, calls of {call}: side {side} gain {g}, want {want}");
            }
        }
    }
}

#[test]
fn state_round_trips_and_survives_a_sample_rate_change() {
    let Some(list) = effects() else { return };
    let Some(uri) = GAIN_PLUGINS.iter().find(|u| list.iter().any(|p| p.cid == **u && p.unsupported.is_none())) else {
        assert!(!required(), "WAVE_REQUIRE_LV2=1 but none of {GAIN_PLUGINS:?} is installed");
        return;
    };
    let (mut fx, h) = instantiate_effect(uri, None, 48_000.0, 256).unwrap();
    assert!(!h.take_state_changed(), "a fresh instance is not edited");
    set_real(&h, "gain", -12.0);
    assert!(h.take_state_changed());
    let blob = h.get_state().unwrap();
    assert!(!blob.is_empty());
    let (_fx2, h2) = instantiate_effect(uri, Some(&blob), 44_100.0, 128).unwrap();
    let gain = |h: &Lv2Handle| h.params().into_iter().find(|p| p.symbol == "gain").unwrap().real;
    assert!((gain(&h2) - -12.0).abs() < 1e-3, "restored {}", gain(&h2));
    assert!(instantiate_effect(uri, Some(b"not turtle at all {"), 48_000.0, 256).is_err(), "garbage state is an error");

    // prepare() at another rate re-creates the plugin and keeps its settings
    fx.prepare(96_000.0, 1024);
    assert!((gain(&h) - -12.0).abs() < 1e-3);
    let (rin, rout, finite) = run(&mut fx, 96_000.0, 0.2, 1024);
    assert!(finite);
    assert!((rout[0] / rin[0] - 10f64.powf(-12.0 / 20.0)).abs() < 0.01, "after prepare: {}", rout[0] / rin[0]);
    // automation goes through set_plugin_param (normalized)
    let id = param_id(&h, "gain");
    let p = h.params().into_iter().find(|p| p.id == id).unwrap();
    fx.set_plugin_param(id, p.def as f32);
    assert!((h.params().into_iter().find(|p| p.id == id).unwrap().value - p.def).abs() < 1e-6);
}

/// Every usable installed effect (within a time budget): it instantiates, processes the
/// test signal without NaN at an odd block size, reports a sane latency, and its state
/// comes back.
#[test]
fn every_installed_effect_processes_the_test_signal_and_round_trips_its_state() {
    let Some(list) = effects() else { return };
    let budget = Instant::now() + Duration::from_secs(240);
    let (mut ran, mut failed_to_load, mut with_latency, mut with_worker) = (0, vec![], 0, 0);
    for p in list.iter().filter(|p| p.unsupported.is_none()) {
        if Instant::now() > budget {
            eprintln!("time budget reached after {ran} plugins");
            break;
        }
        let (mut fx, h): (Lv2Effect, Lv2Handle) = match instantiate_effect(&p.cid, None, 48_000.0, 512) {
            Ok(x) => x,
            Err(e) => {
                failed_to_load.push(format!("{}: {e}", p.cid));
                continue;
            }
        };
        let (_rin, _rout, finite) = run(&mut fx, 48_000.0, 0.1, 333);
        assert!(finite, "{} produced NaN/inf from a quiet sine", p.cid);
        let lat = fx.latency_samples();
        assert_eq!(lat, h.latency());
        // (a parameter may set it: swh Artificial Latency goes to seconds)
        assert!(lat <= 48_000 * 60, "{} reports {lat} samples of latency", p.cid);
        with_latency += usize::from(lat > 0);
        with_worker += usize::from(p.worker);
        // move every visible parameter a little, then the state must bring it back
        for q in h.params() {
            let v = if q.value < 0.5 { (q.value + 0.3).min(1.0) } else { q.value - 0.3 };
            h.set_param(q.id, v).unwrap();
        }
        let before = h.params();
        let blob = h.get_state().unwrap_or_else(|e| panic!("{}: {e}", p.cid));
        let (_fx2, h2) = instantiate_effect(&p.cid, Some(&blob), 48_000.0, 512).unwrap_or_else(|e| panic!("{} restore: {e}", p.cid));
        for (a, b) in before.iter().zip(h2.params()) {
            assert_eq!(a.id, b.id);
            assert!((a.real - b.real).abs() <= 1e-3 * a.real.abs().max(1.0), "{} `{}`: {} restored as {}", p.cid, a.symbol, a.real, b.real);
        }
        ran += 1;
    }
    eprintln!(
        "{ran} effects ran; {with_latency} report latency; {with_worker} have a worker, which ran {} jobs; {} refused to load",
        wave_lv2::worker_jobs_run(),
        failed_to_load.len()
    );
    for f in &failed_to_load {
        eprintln!("  {f}");
    }
    assert!(ran > 0, "no effect could be run");
    if required() {
        assert!(ran >= 10, "only {ran} effects ran");
    }
}
