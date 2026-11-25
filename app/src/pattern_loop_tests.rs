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
    let clip = p
        .clips
        .iter()
        .find(|c| c.kind == wave_engine::ClipKind::Pat && c.source == pid)
        .map(|c| (c.id, c.len))
        .expect("the sketch has a pattern clip");
    let ch = hat(&h);
    pattern(&h, &json!({ "id": pid, "ch": "Hat", "steps": "x.x", "loop": 2 }), None).unwrap();
    // the loop landed, the pattern's length is untouched
    assert_eq!(loops_of(&h, pid), std::collections::BTreeMap::from([(ch, 2)]));
    let proj = h.engine().project();
    let pt = proj.patterns.iter().find(|x| x.id == pid).unwrap();
    assert_eq!((pt.len, pt.min_len), (4.0, 4.0));
    // and the sketch's clip follows the pattern length; a loop alone must not re-target it
    let (id, len) = clip;
    assert_eq!(h.engine().project().clips.iter().find(|c| c.id == id).unwrap().len, len);
}

#[test]
fn a_write_past_an_existing_loop_is_trimmed_in_the_same_step() {
    // the invariant holds on every write, not only --loop: steps past a set loop would
    // never play, so they go instead of sitting in the pattern (and in the reply's steps)
    let h = host();
    let pid = pattern_id(&h);
    let ch = hat(&h);
    pattern(&h, &json!({ "id": pid, "ch": "Hat", "loop": 3 }), None).unwrap();
    let r = pattern(&h, &json!({ "id": pid, "ch": "Hat", "steps": "x.x.x.x." }), None).unwrap();
    let row = r["rows"].as_array().unwrap().iter().find(|row| row["ch"] == json!(ch)).unwrap().clone();
    assert_eq!(row["steps"], json!("x.x. .... .... ...."));
    assert_eq!(row["loop"], json!(3));
    let proj = h.engine().project();
    let notes: Vec<_> = proj.patterns.iter().find(|x| x.id == pid).unwrap().notes.iter().filter(|n| n.ch == ch).collect();
    assert_eq!(notes.iter().map(|n| n.step as u32).collect::<Vec<_>>(), vec![0, 2]);
}

#[test]
fn a_loop_validates_against_the_requested_bars() {
    // `--bars 2` makes the pattern 8 beats: a 32-step loop is valid against the length the
    // pattern will have, not against its length before the bars landed
    let h = host();
    let pid = pattern_id(&h);
    pattern(&h, &json!({ "id": pid, "bars": 2, "ch": "Hat", "steps": "x.", "loop": 32 }), None).unwrap();
    assert_eq!(loops_of(&h, pid), std::collections::BTreeMap::from([(hat(&h), 32)]));
}

#[test]
fn setting_a_loop_keeps_the_pattern_length_and_its_clips() {
    // the trimmed note can be the pattern's last content: the length the pattern had stays
    // the minimum, so fitting never shrinks it and whole-pass clips never re-target
    let h = host();
    let pid = pattern_id(&h);
    pattern(&h, &json!({ "id": pid, "ch": "Hat", "notes": [{ "step": 0, "len": 1 }, { "step": 24, "len": 1 }] }), None).unwrap();
    let proj = h.engine().project();
    let clip = proj
        .clips
        .iter()
        .find(|c| c.kind == wave_engine::ClipKind::Pat && c.source == pid)
        .map(|c| (c.id, c.len))
        .expect("a whole-pass clip of the pattern");
    pattern(&h, &json!({ "id": pid, "ch": "Hat", "loop": 3 }), None).unwrap();
    let proj = h.engine().project();
    let pt = proj.patterns.iter().find(|x| x.id == pid).unwrap();
    assert_eq!((pt.len, pt.min_len), (8.0, 8.0), "the length the pattern had becomes the minimum");
    assert_eq!(proj.clips.iter().find(|c| c.id == clip.0).unwrap().len, clip.1, "the clip never re-targets");
}

#[test]
fn a_loop_set_raises_min_len_only_when_the_trim_would_shorten() {
    // another channel's late note holds the length: the loop on Hat trims nothing that
    // matters, so min_len stays the sketch's — and deleting that late note shrinks the
    // pattern again
    let h = host();
    let pid = pattern_id(&h);
    pattern(&h, &json!({ "id": pid, "ch": "Hat", "steps": "x." }), None).unwrap();
    pattern(&h, &json!({ "id": pid, "ch": "Clap", "notes": [{ "step": 24, "len": 1 }] }), None).unwrap();
    pattern(&h, &json!({ "id": pid, "ch": "Hat", "loop": 3 }), None).unwrap();
    let proj = h.engine().project();
    let pt = proj.patterns.iter().find(|x| x.id == pid).unwrap();
    assert_eq!((pt.len, pt.min_len), (8.0, 4.0), "min_len stays the user's");
    // the later deletion shrinks again
    pattern(&h, &json!({ "id": pid, "ch": "Clap", "clear": true }), None).unwrap();
    let proj = h.engine().project();
    let pt = proj.patterns.iter().find(|x| x.id == pid).unwrap();
    assert_eq!((pt.len, pt.min_len), (4.0, 4.0));
}

#[test]
fn a_stamp_past_the_loop_refuses() {
    // a chord past the loop would be trimmed right after: the stamp refuses instead of
    // returning a successful reply full of notes nothing plays
    let h = host();
    let pid = pattern_id(&h);
    pattern(&h, &json!({ "id": pid, "ch": "Hat", "steps": "x.", "loop": 3 }), None).unwrap();
    assert!(pattern(&h, &json!({ "id": pid, "ch": "Hat", "stamp": { "step": 4, "root": 60, "type": "min" } }), None).is_err());
    // inside the cell it works
    pattern(&h, &json!({ "id": pid, "ch": "Hat", "stamp": { "step": 1, "root": 60, "type": "min" } }), None).unwrap();
}
