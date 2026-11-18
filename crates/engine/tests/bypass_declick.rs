//! Bypassing an effect with a tail must declick — and it must declick to dry: the wet
//! contribution ramps out over ~5 ms (and back in on re-enable) instead of cutting in one
//! sample or staying stuck at full wet.

use std::sync::Arc;

use wave_engine::catalog::new_fx;
use wave_engine::*;

const SR: u32 = 48000;
const BLOCK: usize = 256;

/// Loudest jump between neighbouring samples.
fn max_step(x: &[f32]) -> f32 {
    x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
}

/// Loudest absolute difference between two equally long signals.
fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

/// Start and length of the longest run of exact zeros (a source gap passes through as
/// exact silence in the dry reference).
fn longest_zero_run(x: &[f32]) -> (usize, usize) {
    let (mut best_len, mut best_start, mut cur_start, mut cur_len) = (0usize, 0usize, 0usize, 0usize);
    for (k, &v) in x.iter().enumerate() {
        if v == 0.0 {
            if cur_len == 0 {
                cur_start = k;
            }
            cur_len += 1;
            if cur_len > best_len {
                best_len = cur_len;
                best_start = cur_start;
            }
        } else {
            cur_len = 0;
        }
    }
    (best_start, best_len)
}

/// Tone bursts (whole cycles, so the dry signal has exact-silence gaps) on track 1,
/// optionally through a reverb; the first second is played already. The `TempDir` guard
/// must stay bound in the caller: it removes the fixture WAV when the test ends.
fn setup(tag: &str, reverb: bool) -> (Arc<Engine>, Rt, tempfile::TempDir) {
    let mut p = Project::routed();
    for m in p.inserts.iter_mut() {
        m.fx.clear();
    }
    if reverb {
        let ins = p.tracks[1].insert;
        p.inserts.iter_mut().find(|m| m.id == ins).unwrap().fx.push(new_fx(7000, FxKind::Reverb, p.bpm));
    }
    let e = Engine::new(p, Arc::new(NoPlugins));
    let dir = tempfile::TempDir::with_prefix(format!("wave-declick-{tag}-")).expect("temp dir");
    let wav = dir.path().join("tone.wav");
    // 900-sample bursts of 480 Hz (9 whole cycles, so both edges land on a zero crossing),
    // every 27000 frames: dry gaps long enough to measure a tail in
    let (period, burst) = (27000usize, 900usize);
    let mut x = vec![0f32; SR as usize * 2 * 12];
    for start in (0..x.len()).step_by(period) {
        for i in 0..burst {
            x[start + i] = 0.45 * (std::f32::consts::TAU * i as f32 / 100.0).sin();
        }
    }
    wave_engine::sources::write_wav(&wav, &x, SR).unwrap();
    e.import_audio(std::slice::from_ref(&wav), Some((1, 0.0))).unwrap();
    let mut rt = e.create_rt(SR as f32, BLOCK);
    e.play(0.0);
    run(&mut rt, SR as usize);
    (e, rt, dir)
}

fn run(rt: &mut Rt, n: usize) -> Vec<f32> {
    let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n / BLOCK {
        rt.process(&mut l, &mut r, None);
        out.extend_from_slice(&l);
    }
    out
}

fn put(e: &Engine, track: usize, fx: Fx) {
    let ins = e.project().tracks[track].insert;
    e.apply(&[Op::PutFx { insert_id: ins, fx, index: None }], false, None).unwrap();
}

#[test]
fn bypass_ramps_to_dry_and_back_without_steps() {
    // three engines kept in frame lockstep: the one under test, a never-bypassed wet
    // reference, and a dry reference with no effect
    let (e, mut rt, _dir) = setup("bypass", true);
    let (_, mut rt_wet, _dir_wet) = setup("bypass-wet", true);
    let (_, mut rt_dry, _dir_dry) = setup("bypass-dry", false);
    let pre = run(&mut rt, SR as usize / 2);
    run(&mut rt_wet, SR as usize / 2);
    run(&mut rt_dry, SR as usize / 2);
    let window = SR as usize / 5; // spans a burst and part of a gap, well past the ~5 ms ramp
    let step_bound = 0.08;
    let peak = pre.iter().fold(0.0f32, |a, &v| a.max(v.abs()));
    assert!(peak > 0.3, "the reverb must be audible for the test to mean anything (peak {peak})");

    // bypass mid-ring: smooth across the flip, and afterwards the output must be exactly the
    // dry passthrough (not stuck at the wet one — the failure a smoothness-only check cannot
    // see). Comparing to the dry reference is exact: a bypassed slot writes nothing.
    put(&e, 1, Fx { bypass: true, ..new_fx(7000, FxKind::Reverb, e.project().bpm) });
    let mut around = vec![*pre.last().unwrap()];
    around.extend(run(&mut rt, BLOCK * 2));
    let down = max_step(&around);
    run(&mut rt_wet, BLOCK * 2);
    run(&mut rt_dry, BLOCK * 2);
    let muted = run(&mut rt, window);
    let wet_win = run(&mut rt_wet, window);
    let dry_win = run(&mut rt_dry, window);
    let to_dry = max_diff(&muted, &dry_win);
    let still_wet = max_diff(&muted, &wet_win);
    eprintln!("bypass on: jump {down:.4}, vs dry {to_dry:.6}, vs still-wet {still_wet:.4} (peak {peak:.4})");
    assert!(down < step_bound, "bypassing cut the wet in one sample: jump {down}");
    assert!(to_dry < 1e-4, "bypassed output did not reach the dry level: max diff {to_dry}");
    assert!(still_wet > 0.005, "bypassed output is still the wet signal (diff {still_wet}) — bypass is a no-op");

    // and back on: smooth across the flip, and reverb tail energy returns in the gaps.
    // (The processor intentionally does not run while fully bypassed, so its state has
    // missed that input: a bit-comparison against the never-bypassed reference is
    // impossible here — what must hold is that the tail is back, at a comparable level.)
    put(&e, 1, new_fx(7000, FxKind::Reverb, e.project().bpm));
    let mut around = vec![*muted.last().unwrap()];
    around.extend(run(&mut rt, BLOCK * 2));
    let up = max_step(&around);
    run(&mut rt_wet, BLOCK * 2);
    run(&mut rt_dry, BLOCK * 2);
    let rewet = run(&mut rt, window);
    let wet_win = run(&mut rt_wet, window);
    let dry_win = run(&mut rt_dry, window);
    let (gap_start, gap_len) = longest_zero_run(&dry_win);
    assert!(gap_len >= 2000, "no silence gap found in the dry window (longest run {gap_len})");
    let mid = gap_len / 2;
    let gap_slice = |s: &[f32]| s[gap_start + mid - 250..gap_start + mid + 250].to_vec();
    let tail = rms(&gap_slice(&rewet));
    let tail_ref = rms(&gap_slice(&wet_win));
    let wet_returned = max_diff(&rewet, &dry_win);
    eprintln!("bypass off: jump {up:.4}, tail rms {tail:.4} (reference {tail_ref:.4}), vs dry {wet_returned:.4}");
    assert!(up < step_bound, "re-enabling stepped the wet in: jump {up}");
    assert!(tail > 0.001, "re-enabled output has no reverb tail in the gaps: rms {tail}");
    assert!(tail > 0.25 * tail_ref, "re-enabled tail is far below the never-bypassed level: {tail} vs {tail_ref}");
    assert!(wet_returned > 0.005, "re-enabled output is still dry (diff {wet_returned})");

    // one more hit cycle: the state the bypass missed has been flushed, so the tail must
    // converge on the never-bypassed reference (a frozen partial fade would stay low forever)
    run(&mut rt, SR as usize / 2);
    run(&mut rt_wet, SR as usize / 2);
    run(&mut rt_dry, SR as usize / 2);
    // all three signals advance together: the reference windows index the gap the tail is
    // measured in, so `rt` must sit on the very same playback frames
    run(&mut rt, BLOCK * 2);
    run(&mut rt_wet, BLOCK * 2);
    run(&mut rt_dry, BLOCK * 2);
    let rewet2 = run(&mut rt, window);
    let wet_win2 = run(&mut rt_wet, window);
    let dry_win2 = run(&mut rt_dry, window);
    let (gap_start, gap_len) = longest_zero_run(&dry_win2);
    let mid = gap_len / 2;
    let gap_slice = |s: &[f32]| s[gap_start + mid - 250..gap_start + mid + 250].to_vec();
    let tail2 = rms(&gap_slice(&rewet2));
    let tail_ref2 = rms(&gap_slice(&wet_win2));
    eprintln!("cycle 2: tail rms {tail2:.4} vs reference {tail_ref2:.4} (ratio {:.2})", tail2 / tail_ref2);
    // near the reference, not merely "some tail": a fade frozen halfway passes nothing below
    assert!(tail2 > 0.8 * tail_ref2, "the wet never fully returned: tail {tail2} vs {tail_ref2}");
}

#[test]
fn bypassing_a_latency_effect_stays_on_one_time_base_until_the_fade_is_done() {
    // A de-esser with a 15 ms look-ahead plays the tone; flipping its bypass keeps the wet
    // running for the ~5 ms declick. The dry path must stay delayed along with it (one time
    // base for the blend), or the two copies of the tone comb each other out. After the fade
    // the latency leaves in a second, smooth declick — dropping it in one step would click —
    // and then the output is exactly the dry passthrough again.
    let (e, mut rt, _dir) = setup("align", false);
    let (e_wet, mut rt_wet, _dir_wet) = setup("align-wet", false);
    let (_, mut rt_dry, _dir_dry) = setup("align-dry", false);
    let mut fx = new_fx(7001, FxKind::Deess, e.project().bpm);
    fx.params.insert("look".into(), 15.0);
    put(&e, 1, fx);
    let mut fx_wet = new_fx(7001, FxKind::Deess, e_wet.project().bpm);
    fx_wet.params.insert("look".into(), 15.0);
    put(&e_wet, 1, fx_wet);
    // the warm-up inside `setup` rendered 187 blocks; 130 more put the flip frame (81152)
    // into the middle of a tone burst — flipping inside a gap would park the chain silent
    // (silence skipping) and freeze the fade mid-state instead of exercising it
    const PRE: usize = 130 * BLOCK;
    let pre = run(&mut rt, PRE);
    let wet_pre = run(&mut rt_wet, PRE);
    let _ = run(&mut rt_dry, PRE);
    let peak = pre.iter().fold(0.0f32, |a, &v| a.max(v.abs()));
    assert!(peak > 0.3, "the effect must be audible for the test to mean anything (peak {peak})");
    let lock = max_diff(&pre, &wet_pre);
    assert!(lock < 1e-5, "both engines must run in lockstep before the flip (diff {lock})");

    let mut off = new_fx(7001, FxKind::Deess, e.project().bpm);
    off.bypass = true;
    off.params.insert("look".into(), 15.0);
    put(&e, 1, off);
    // the fade block: still wet (ramping down), and on the delayed time base like the reference
    let fade = run(&mut rt, BLOCK);
    let wet_fade = run(&mut rt_wet, BLOCK);
    let _ = run(&mut rt_dry, BLOCK);
    let combed = max_diff(&fade, &wet_fade);
    eprintln!("fade vs still-wet {combed:.4}");
    assert!(combed < 0.12, "the fade blended late wet with undelayed dry: diff {combed}");

    // the release: smooth through the second declick (still inside the burst), then quiet
    let mut around = vec![*fade.last().unwrap()];
    around.extend(run(&mut rt, BLOCK * 3));
    run(&mut rt_wet, BLOCK * 3);
    run(&mut rt_dry, BLOCK * 3);
    let down = max_step(&around);
    // the inter-burst gap parks the chains; 100 more blocks land the next window on a burst
    let gap = 100 * BLOCK;
    run(&mut rt, gap);
    run(&mut rt_wet, gap);
    run(&mut rt_dry, gap);
    let muted = run(&mut rt, SR as usize / 5);
    let wet_win = run(&mut rt_wet, SR as usize / 5);
    let dry_win = run(&mut rt_dry, SR as usize / 5);
    let to_dry = max_diff(&muted, &dry_win);
    let to_wet = max_diff(&muted, &wet_win);
    eprintln!("release: jump {down:.4}, vs dry {to_dry:.6}, vs still-wet {to_wet:.4}");
    assert!(down < 0.08, "the latency handoff stepped the signal: jump {down}");
    assert!(to_wet > 0.005, "the output is still the wet signal (diff {to_wet}) — the bypass never landed");
    assert!(to_dry < 1e-4, "after the release the output must be exactly dry: diff {to_dry}");
}
