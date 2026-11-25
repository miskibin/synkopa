//! `synkopa pattern --loop`: the per-channel loop (FL Studio 20.1's channel looping)
//! through the shared action — set (which trims that channel's notes from the loop step
//! on), clear, display, and the undo step.

use super::*;
use crate::headless::Headless;

fn host() -> Headless {
    Headless::new(wave_engine::Project::sketch())
}

fn loops_of(h: &Headless, pt: u64) -> std::collections::BTreeMap<u64, u32> {
    h.engine().project().patterns.iter().find(|p| p.id == pt).map(|p| p.channel_loops.clone()).unwrap()
}

fn hat(h: &Headless) -> u64 {
    h.engine().project().channels.iter().find(|c| c.name == "Hat").unwrap().id
}

fn pattern_id(h: &Headless) -> u64 {
    h.engine().project().patterns[0].id
}

#[test]
fn loop_sets_trims_and_displays() {
    let h = host();
    let (pid, ch) = (pattern_id(&h), hat(&h));
    // an 8-step line, then loop its first 3 steps
    pattern(&h, &json!({ "id": pid, "ch": "Hat", "steps": "x.x.x..." }), None).unwrap();
    let notes_before =
        |h: &Headless| h.engine().project().patterns.iter().find(|p| p.id == pid).unwrap().notes.iter().filter(|n| n.ch == ch).count();
    assert_eq!(notes_before(&h), 3);
    let r = pattern(&h, &json!({ "id": pid, "ch": "Hat", "loop": 3 }), None).unwrap();
    assert_eq!(loops_of(&h, pid), std::collections::BTreeMap::from([(ch, 3)]));
    assert_eq!(r["rows"].as_array().unwrap().iter().find(|row| row["ch"] == json!(ch)).unwrap()["loop"], json!(3));
    // the notes from the loop step on are gone
    assert_eq!(notes_before(&h), 2);
    // and it is one undo step
    assert!(h.engine().undo());
    assert!(loops_of(&h, pid).is_empty());
    assert_eq!(notes_before(&h), 3);
}

#[test]
fn loop_off_clears_and_keeps_the_cell() {
    let h = host();
    let (pid, ch) = (pattern_id(&h), hat(&h));
    pattern(&h, &json!({ "id": pid, "ch": "Hat", "steps": "x.x", "loop": 2 }), None).unwrap();
    assert_eq!(loops_of(&h, pid), std::collections::BTreeMap::from([(ch, 2)]));
    let r = pattern(&h, &json!({ "id": pid, "ch": "Hat", "loop": "off" }), None).unwrap();
    assert!(loops_of(&h, pid).is_empty());
    assert_eq!(r["rows"].as_array().unwrap().iter().find(|row| row["ch"] == json!(ch)).unwrap()["loop"], serde_json::Value::Null);
    // rewriting the line afterwards keeps all four steps (no loop to trim them)
    pattern(&h, &json!({ "id": pid, "ch": "Hat", "steps": "x.x.x.x." }), None).unwrap();
    assert_eq!(h.engine().project().patterns.iter().find(|p| p.id == pid).unwrap().notes.iter().filter(|n| n.ch == ch).count(), 4);
}

#[test]
fn loop_refusals() {
    let h = host();
    let pid = pattern_id(&h);
    let steps = (h.engine().project().patterns[0].len * 4.0) as u64;
    // no --ch
    assert!(pattern(&h, &json!({ "id": pid, "loop": 3 }), None).is_err());
    // longer than the pattern
    assert!(pattern(&h, &json!({ "id": pid, "ch": "Hat", "loop": steps + 1 }), None).is_err());
    // not a number
    assert!(pattern(&h, &json!({ "id": pid, "ch": "Hat", "loop": "soon" }), None).is_err());
    // nothing changed
    assert!(loops_of(&h, pid).is_empty());
}

#[test]
fn a_loop_put_never_touches_clips_or_the_length() {
    let h = host();
    let pid = pattern_id(&h);
    let p = h.engine().project();
    let clip = p.clips.iter().find(|c| c.kind == wave_engine::ClipKind::Pat && c.source == pid).map(|c| (c.id, c.len));
    let ch = hat(&h);
    pattern(&h, &json!({ "id": pid, "ch": "Hat", "steps": "x.x", "loop": 2 }), None).unwrap();
    // the sketch's clip follows the pattern length; a loop alone must not re-target it
    if let Some((id, len)) = clip {
        assert_eq!(h.engine().project().clips.iter().find(|c| c.id == id).unwrap().len, len);
    }
    let _ = ch;
}
