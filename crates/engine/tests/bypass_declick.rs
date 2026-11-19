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
    setup_with(tag, reverb, 900, 27000, |_| {})
}

/// `setup` with a custom burst length and period: a long burst over a long period builds
/// a reverb wet (and a diffuse tail) that is still loud when the burst ends, which is what
/// a flip-in-a-dry-gap test needs to cut.
fn setup_with(tag: &str, reverb: bool, burst: usize, period: usize, adjust: impl FnOnce(&mut Fx)) -> (Arc<Engine>, Rt, tempfile::TempDir) {
    let mut p = Project::routed();
    for m in p.inserts.iter_mut() {
        m.fx.clear();
    }
    if reverb {
        let ins = p.tracks[1].insert;
        let mut fx = new_fx(7000, FxKind::Reverb, p.bpm);
        adjust(&mut fx);
        p.inserts.iter_mut().find(|m| m.id == ins).unwrap().fx.push(fx);
    }
    let e = Engine::new(p, Arc::new(NoPlugins));
    let dir = tempfile::TempDir::with_prefix(format!("wave-declick-{tag}-")).expect("temp dir");
    let wav = dir.path().join("tone.wav");
    // bursts of 480 Hz (whole cycles, so both edges land on a zero crossing): dry gaps
    // long enough to measure a tail in
    let mut x = vec![0f32; SR as usize * 2 * 12];
    for start in (0..x.len()).step_by(period) {
        for i in 0..burst.min(period).min(x.len() - start) {
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

    // and back on, mid-burst (a real user re-enables while audio is playing; the processor
    // intentionally does not run while bypassed, so into a quiet gap it would fade silence
    // and only the next burst could tell wet from dry): smooth across the flip, and the
    // reverb tail is back in the gaps at a comparable level. The three engines stay in
    // frame lockstep while scanning for the burst.
    let mut last = vec![0f32; BLOCK];
    let mut burst = false;
    for _ in 0..80 {
        last = run(&mut rt, BLOCK);
        run(&mut rt_wet, BLOCK);
        run(&mut rt_dry, BLOCK);
        if last.iter().fold(0.0f32, |a, &v| a.max(v.abs())) > 0.3 {
            burst = true;
            break;
        }
    }
    assert!(burst, "no burst arrived to re-enable into");
    put(&e, 1, new_fx(7000, FxKind::Reverb, e.project().bpm));
    let mut around = vec![*last.last().unwrap()];
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
    // into the middle of a tone burst — the flip-inside-a-gap case (silence skipping must
    // not park the chain mid-fade) is pinned by the two `..._in_a_dry_gap` tests below
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

#[test]
fn bypass_in_a_dry_gap_fades_before_the_chain_parks() {
    // Flipping bypass while the input is already silent: the reverb tail still rings and
    // the ~5 ms ramp-out must play out before silence skipping parks the chain — parking
    // at the flip would cut the ringing tail in one sample at a block boundary.
    let (e, mut rt, _dir) = setup_with("gap-reverb", true, 40000, 60000, |_| {});
    // run past a ~420 ms burst and stop at the first block whose input is exact silence
    // (the burst ended inside the previous one): the reverb is still ringing the burst
    // there, while the quiet is far below the ~3 s tail — so the chain is running and not
    // parked (a parked one would pass for the wrong reason)
    let mut pre: Vec<f32> = Vec::new();
    let (mut loud, mut quiet_blocks) = (false, 0);
    for _ in 0..300 {
        let b = run(&mut rt, BLOCK);
        let p = b.iter().fold(0.0f32, |a, &v| a.max(v.abs()));
        pre.extend_from_slice(&b);
        if p > 0.3 {
            loud = true;
            quiet_blocks = 0;
        } else if loud {
            quiet_blocks += 1;
            if quiet_blocks == 1 {
                break;
            }
        }
    }
    let level = pre.iter().fold(0.0f32, |a, &v| a.max(v.abs()));
    let seam = pre.last().unwrap().abs();
    assert!(level > 0.3, "no burst played for the test to flip in (peak {level})");
    assert!(seam > 0.04, "the reverb must still ring in the first silent-input block (seam {seam})");

    put(&e, 1, Fx { bypass: true, ..new_fx(7000, FxKind::Reverb, e.project().bpm) });
    let mut around = vec![*pre.last().unwrap()];
    around.extend(run(&mut rt, BLOCK * 3));
    let down = max_step(&around);
    eprintln!("gap bypass: jump {down:.4} (seam {seam:.4})");
    // parking at the flip cuts the ring in one sample (a step of ~the whole seam); the
    // ramp-out bounds the per-sample step well under half of it
    assert!(down < seam * 0.5, "bypassing in a dry gap cut the ringing tail at a block boundary: jump {down}");

    // settled: the fades are done, the chain parks, and the gap passes through as exact
    // silence — a fade frozen part-way (or a chain that never settles) stays audible here.
    // The window stays inside the inter-burst gap (a burst passes through loud).
    let settled = run(&mut rt, BLOCK * 20);
    let loudest = settled.iter().fold(0.0f32, |a, &v| a.max(v.abs()));
    assert!(loudest < 1e-6, "the bypassed chain must settle to exact silence in the gap (peak {loudest})");
}

#[test]
fn bypassing_a_latency_effect_in_a_dry_gap_finishes_the_release() {
    // The same flip in a dry gap, on an effect with a look-ahead: after the ramp-out the
    // dry path still sits on the old latency and must hand back in the second declick
    // before the chain parks — parking between the two freezes the handoff and cuts the
    // still-late ring at a block boundary.
    let (e, mut rt, _dir) = setup("gap-deess", false);
    let mut fx = new_fx(7001, FxKind::Deess, e.project().bpm);
    fx.params.insert("look".into(), 15.0);
    put(&e, 1, fx);
    // run to just past a burst end: the input is already exact silence for those blocks,
    // but the 720-frame look-ahead is still playing the burst's delayed tail, so the wet
    // is loud and the chain is running (quiet 512 < the look-ahead)
    let pre = run(&mut rt, 6900 + BLOCK * 2);
    let level = pre.iter().fold(0.0f32, |a, &v| a.max(v.abs()));
    assert!(level > 0.2, "the delayed burst tail must still sound in the gap for the test to mean anything (peak {level})");

    let mut off = new_fx(7001, FxKind::Deess, e.project().bpm);
    off.bypass = true;
    off.params.insert("look".into(), 15.0);
    put(&e, 1, off);
    let mut around = vec![*pre.last().unwrap()];
    around.extend(run(&mut rt, BLOCK * 3));
    let down = max_step(&around);
    eprintln!("gap latency bypass: jump {down:.4} (tail level {level:.4})");
    assert!(down < 0.08, "bypassing a latency effect in a dry gap cut the still-late ring: jump {down}");

    let settled = run(&mut rt, SR as usize / 5);
    let loudest = settled.iter().fold(0.0f32, |a, &v| a.max(v.abs()));
    assert!(loudest < 1e-6, "the bypassed chain must settle to exact silence in the gap (peak {loudest})");
}
