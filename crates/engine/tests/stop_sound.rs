//! A second Stop ("Stop sound", FL's Ctrl+H): what still rings after a stop — reverb and delay
//! tails, released voices, a preview — is cut at once, through a short ramp instead of a click.

use std::sync::Arc;

use wave_engine::catalog::new_fx;
use wave_engine::*;

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0f32, |m, v| m.max(v.abs()))
}

/// The demo project with a long, wet reverb and delay on every insert.
fn ringing() -> Project {
    let mut p = Project::sketch();
    let bpm = p.bpm;
    let mut next = 9000;
    for ins in p.inserts.iter_mut().skip(1) {
        for kind in [FxKind::Delay, FxKind::Reverb] {
            next += 1;
            let mut fx = new_fx(next, kind, bpm);
            fx.mix = 0.6;
            ins.fx.push(fx);
        }
    }
    p
}

const SR: f32 = 48000.0;
const BLOCK: usize = 256;

/// Plays two seconds, stops, and lets 50 ms ring: the tail is still audible.
fn stopped_while_ringing(e: &Engine) -> Box<dyn FnMut() -> (Vec<f32>, Vec<f32>)> {
    let mut rt = e.create_rt(SR, BLOCK);
    let mut run = move || {
        let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
        rt.process(&mut l, &mut r, None);
        (l, r)
    };
    e.play(0.0);
    for _ in 0..(2 * SR as usize / BLOCK) {
        run();
    }
    let _ = e.stop();
    for _ in 0..(SR as usize / 20 / BLOCK) {
        run();
    }
    Box::new(run)
}

#[test]
fn stop_lets_tails_ring_and_a_second_stop_cuts_them() {
    // control: after a plain stop the effects keep ringing for a while
    let e = Engine::new(ringing(), Arc::new(NoPlugins));
    let mut run = stopped_while_ringing(&e);
    let ring = (0..(SR as usize / 4 / BLOCK)).map(|_| peak(&run().0)).fold(0f32, f32::max);
    assert!(ring > 1e-3, "the project must still ring after a stop ({ring})");

    let e = Engine::new(ringing(), Arc::new(NoPlugins));
    let mut run = stopped_while_ringing(&e);
    let before = peak(&run().0);
    assert!(before > 1e-3, "a tail must be sounding before the second stop ({before})");
    e.silence();
    // the cut ramps over a few milliseconds: never louder than what was heard
    let (first, _) = run();
    assert!(peak(&first) <= before * 1.05 + 1e-4, "the cut clicked: {} after {before}", peak(&first));
    // then nothing is heard any more
    let after = (0..(SR as usize / BLOCK))
        .map(|_| {
            let (l, r) = run();
            peak(&l).max(peak(&r))
        })
        .fold(0f32, f32::max);
    assert!(after < 1e-5, "still ringing after the second stop ({after})");
}

#[test]
fn a_second_stop_also_ends_a_preview() {
    let e = Engine::new(Project::sketch(), Arc::new(NoPlugins));
    let id = e.project().sources[0].id;
    let mut rt = e.create_rt(SR, BLOCK);
    let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
    e.preview_source(id).unwrap();
    let mut heard = 0f32;
    for _ in 0..40 {
        rt.process(&mut l, &mut r, None);
        heard = heard.max(peak(&l));
    }
    assert!(heard > 1e-3, "the source preview must sound ({heard})");
    e.silence();
    rt.process(&mut l, &mut r, None);
    let mut after = 0f32;
    for _ in 0..40 {
        rt.process(&mut l, &mut r, None);
        after = after.max(peak(&l));
    }
    assert!(after < 1e-6, "the preview kept playing ({after})");
    assert!(e.preview_source(u64::MAX).is_err(), "an unknown source is an error, not a panic");
}
