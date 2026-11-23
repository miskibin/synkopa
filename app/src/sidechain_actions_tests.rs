//! Sidechain compression through the shared action, as the app and `synkopa sidechain …` use it:
//! the mixer's "Sidechain to ▸" (key the target's Compressor, else add one set up for ducking),
//! pointing one Compressor at an insert, turning it off, and the refusals.

use wave_engine::catalog::new_fx;
use wave_engine::{FxKind, Op, Project};

use super::*;
use crate::headless::Headless;

fn host() -> (Headless, u64, u64) {
    let mut p = Project::sketch();
    // no effects on the two inserts used, so "the first Compressor" is the one the test adds
    for m in p.inserts.iter_mut().skip(1).take(2) {
        m.fx.clear();
    }
    let (a, b) = (p.inserts[1].id, p.inserts[2].id);
    (Headless::new(p), a, b)
}

fn comps(h: &Headless, on: u64) -> Vec<Fx> {
    h.engine().project().insert(on).unwrap().fx.iter().filter(|f| f.kind == FxKind::Comp).cloned().collect()
}

#[test]
fn sidechain_to_adds_a_ducking_compressor_once_and_off_returns_it_to_its_own_input() {
    let (h, kick, bass) = host();
    let r = sidechain(&h, None, Some(kick), Some(bass), false, None).unwrap();
    assert_eq!(r["added"], true);
    let c = comps(&h, bass);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].key, Some(kick));
    assert_eq!(c[0].params["thr"], DUCK_PARAMS[0].1);
    assert_eq!(r["fxId"], c[0].id);
    // again: the same compressor, nothing added
    let again = sidechain(&h, None, Some(kick), Some(bass), false, None).unwrap();
    assert_eq!((again["added"].clone(), again["fxId"].clone()), (json!(false), json!(c[0].id)));
    assert_eq!(comps(&h, bass).len(), 1);
    // off: the compressor stays, on its own input
    sidechain(&h, None, Some(kick), Some(bass), true, None).unwrap();
    assert_eq!(comps(&h, bass)[0].key, None);
    // one undo step brings the key back
    assert!(h.engine().undo());
    assert_eq!(comps(&h, bass)[0].key, Some(kick));
}

#[test]
fn sidechain_to_keys_a_compressor_already_on_the_target() {
    let (h, kick, bass) = host();
    let p = h.engine().project();
    let f = new_fx(p.next_id, FxKind::Comp, p.bpm);
    h.engine().apply(&[Op::PutFx { insert_id: bass, fx: f.clone(), index: None }], false, None).unwrap();
    let r = sidechain(&h, None, Some(kick), Some(bass), false, None).unwrap();
    assert_eq!(r["added"], false);
    assert_eq!(comps(&h, bass).len(), 1);
    assert_eq!(comps(&h, bass)[0].key, Some(kick));
    assert_eq!(comps(&h, bass)[0].params, f.params, "its own settings are kept");
}

#[test]
fn one_compressor_by_id_and_the_refusals() {
    let (h, kick, bass) = host();
    let p = h.engine().project();
    let f = new_fx(p.next_id, FxKind::Comp, p.bpm);
    h.engine().apply(&[Op::PutFx { insert_id: bass, fx: f.clone(), index: None }], false, None).unwrap();
    sidechain(&h, Some(f.id), Some(kick), None, false, None).unwrap();
    assert_eq!(comps(&h, bass)[0].key, Some(kick));
    sidechain(&h, Some(f.id), None, None, true, None).unwrap();
    assert_eq!(comps(&h, bass)[0].key, None);
    assert!(sidechain(&h, Some(f.id), None, None, false, None).unwrap_err().contains("--from"));
    assert!(sidechain(&h, Some(f.id), Some(bass), None, false, None).is_err(), "not its own insert");
    assert!(sidechain(&h, Some(f.id), Some(0), None, false, None).is_err(), "not the master");
    assert!(sidechain(&h, None, Some(kick), Some(bass), true, None).unwrap_err().contains("no effect"));
    assert!(sidechain(&h, None, Some(kick), Some(424242), false, None).is_err());
}

#[test]
fn sidechain_to_keys_a_compressor_and_leaves_a_gate_alone() {
    // "Sidechain to" means ducking: a free Gate on the target is not what the user asked for
    let (h, kick, bass) = host();
    let p = h.engine().project();
    let g = new_fx(p.next_id, FxKind::Gate, p.bpm);
    h.engine().apply(&[Op::PutFx { insert_id: bass, fx: g.clone(), index: None }], false, None).unwrap();
    let r = sidechain(&h, None, Some(kick), Some(bass), false, None).unwrap();
    assert_eq!(r["added"], true, "a ducking compressor is added next to the gate");
    let p = h.engine().project();
    assert_eq!(p.find_fx(g.id).unwrap().1.key, None);
    // a gate can still be keyed by id, and then counts as listening
    sidechain(&h, Some(g.id), Some(kick), None, false, None).unwrap();
    assert_eq!(h.engine().project().find_fx(g.id).unwrap().1.key, Some(kick));
}

#[test]
fn the_command_is_dispatched() {
    let (h, kick, bass) = host();
    let r = control(&h, "sidechain", &json!({ "from": kick, "to": bass }), None, None).unwrap();
    assert_eq!(r["added"], true);
    let r = control(&h, "sidechain", &json!({ "from": kick, "to": bass, "off": true }), None, None).unwrap();
    assert_eq!(r["fxId"], Value::Null);
}
