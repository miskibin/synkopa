//! Mix assistant actions (`wave_engine::masking`), shared by the GUI and the `synkopa` CLI:
//!
//! * `mix_assistant` — `{on?}`: read or switch the live detector (the app's setting).
//! * `masking` — what collides. In the app: what the detector heard while playing. On a project
//!   file (no stream), or with `scan: true`: an offline pass over `from`..`to` beats (default:
//!   the song or the loop).
//! * `unmask` — `{masker, protect, lo, hi, keyDb}` (a collision as `masking` reports it): adds
//!   the fix, a dynamic EQ on `masker` keyed from `protect`, at the end of its chain. One undo step.
//! * `mask_ignore` — `{a, b}`: stop reporting the pair (stored in the project, undoable).

use std::sync::atomic::AtomicBool;

use serde_json::{json, Value};
use wave_engine::masking::{fix_fx, Collision};
use wave_engine::{Engine, Op};

use crate::actions::{arg, need_u64, opt_f64, Host};

type R<T> = Result<T, String>;

pub const COMMANDS: &[&str] = &["mix_assistant", "masking", "unmask", "mask_ignore"];

pub fn control(app: &dyn Host, cmd: &str, a: &Value, if_revision: Option<u64>) -> Option<R<Value>> {
    if !COMMANDS.contains(&cmd) {
        return None;
    }
    let e = app.engine();
    Some(match cmd {
        "mix_assistant" => {
            if let Some(on) = arg(a, "on") {
                match on.as_bool() {
                    Some(on) => e.set_mix_assistant(on),
                    None => return Some(Err("`on` must be true or false".into())),
                }
            }
            Ok(json!({ "on": e.mix_assistant() }))
        }
        "masking" => masking(app, a),
        "unmask" => collision(a).and_then(|c| unmask(e, &c, if_revision)),
        "mask_ignore" => (|| {
            let (x, y) = (need_u64(a, "a")?, need_u64(a, "b")?);
            ignore(e, x, y, if_revision)
        })(),
        _ => unreachable!(),
    })
}

fn masking(app: &dyn Host, a: &Value) -> R<Value> {
    let e = app.engine();
    let scan = arg(a, "scan").and_then(Value::as_bool).unwrap_or(!app.live());
    let names = |c: &Collision| {
        let p = e.project();
        let name = |id| p.insert(id).map(|m| m.name.clone()).unwrap_or_default();
        json!({
            "masker": c.masker, "maskerName": name(c.masker), "protect": c.protect, "protectName": name(c.protect),
            "lo": c.lo, "hi": c.hi, "center": c.center, "overlap": c.overlap, "keyDb": c.key_db,
        })
    };
    if !scan {
        if !e.mix_assistant() {
            return Ok(json!({ "on": false, "collisions": [] }));
        }
        let found = e.collisions();
        return Ok(json!({ "on": true, "live": true, "collisions": found.iter().map(names).collect::<Vec<_>>() }));
    }
    let p = e.project();
    let (from, to) = match (opt_f64(a, "from"), opt_f64(a, "to")) {
        (Some(f), Some(t)) => (f, t),
        (None, None) => Engine::export_range(&p, p.loop_range.is_some()),
        _ => return Err("give both `from` and `to` (beats), or neither".into()),
    };
    let found = e.masking_scan(from, to, &AtomicBool::new(false)).map_err(|er| er.to_string())?;
    Ok(json!({ "live": false, "from": from, "to": to, "collisions": found.iter().map(names).collect::<Vec<_>>() }))
}

fn collision(a: &Value) -> R<Collision> {
    let f = |k: &str| opt_f64(a, k).map(|x| x as f32).ok_or(format!("`{k}` (number) is required"));
    let c = Collision {
        masker: need_u64(a, "masker")?,
        protect: need_u64(a, "protect")?,
        lo: f("lo")?,
        hi: f("hi")?,
        center: opt_f64(a, "center").map(|x| x as f32).unwrap_or(0.0),
        overlap: opt_f64(a, "overlap").map(|x| x as f32).unwrap_or(0.0),
        key_db: f("keyDb")?,
    };
    if !(c.lo > 0.0 && c.hi > c.lo && c.hi <= 24000.0) {
        return Err("`lo` and `hi` must be frequencies with lo < hi".into());
    }
    if !(-120.0..=24.0).contains(&c.key_db) {
        return Err("`keyDb` must be a level in dBFS".into());
    }
    Ok(c)
}

/// Adds the fix to the masker's chain; returns the new effect's id.
pub fn unmask(e: &Engine, c: &Collision, if_revision: Option<u64>) -> R<Value> {
    let p = e.project();
    for id in [c.masker, c.protect] {
        if id == 0 || p.insert(id).is_none() {
            return Err(format!("insert {id} is not a mixer insert"));
        }
    }
    let fx = fix_fx(p.next_id, c);
    let id = fx.id;
    let rev = e.apply(&[Op::PutFx { insert_id: c.masker, fx, index: None }], false, if_revision).map_err(|er| er.to_string())?;
    Ok(json!({ "fxId": id, "insert": c.masker, "revision": rev }))
}

pub fn ignore(e: &Engine, a: u64, b: u64, if_revision: Option<u64>) -> R<Value> {
    let mut pairs = e.project().mask_ignore.clone();
    pairs.push([a, b]);
    let rev = e.apply(&[Op::SetMaskIgnore { pairs }], false, if_revision).map_err(|er| er.to_string())?;
    Ok(json!({ "revision": rev }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_collision_needs_a_frequency_range_and_a_key_level() {
        let ok = json!({"masker": 6, "protect": 7, "lo": 44.5, "hi": 56.1, "keyDb": -12.7});
        assert_eq!(collision(&ok).unwrap().masker, 6);
        assert!(collision(&json!({"masker": 6, "protect": 7, "lo": 60, "hi": 50, "keyDb": -12})).is_err());
        assert!(collision(&json!({"masker": 6, "protect": 7, "lo": 40, "hi": 50})).is_err());
    }

    #[test]
    fn unmask_adds_one_keyed_eq_in_one_undo_step() {
        let e = Engine::new(wave_engine::demo::demo_project(), std::sync::Arc::new(wave_engine::NoPlugins));
        let p = e.project();
        let (kick, b808) =
            (p.inserts.iter().find(|m| m.name == "Kick").unwrap().id, p.inserts.iter().find(|m| m.name == "808").unwrap().id);
        let n = p.insert(b808).unwrap().fx.len();
        let c = Collision { masker: b808, protect: kick, lo: 44.5, hi: 56.1, center: 50.0, overlap: 0.6, key_db: -12.7 };
        let r = unmask(&e, &c, None).unwrap();
        let p = e.project();
        let m = p.insert(b808).unwrap();
        assert_eq!(m.fx.len(), n + 1);
        let fx = m.fx.last().unwrap();
        assert_eq!(fx.id, r["fxId"].as_u64().unwrap());
        assert_eq!(fx.key, Some(kick));
        assert!(wave_engine::masking::keyed_from(m, kick));
        assert!(e.undo());
        assert_eq!(e.project().insert(b808).unwrap().fx.len(), n);
        assert!(unmask(&e, &Collision { masker: 0, ..c }, None).is_err());
    }
}
