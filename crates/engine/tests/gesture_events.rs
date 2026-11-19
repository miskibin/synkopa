//! A drag sends one transient batch per frame. Listeners (the app's "state" event: the whole
//! project, ~1 MB on a big song) hear about the gesture once, when it ends, not per batch.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use wave_engine::*;

fn engine() -> (Arc<Engine>, Arc<AtomicUsize>, Arc<std::sync::Mutex<u64>>) {
    let e = Engine::new(Project::empty(), Arc::new(NoPlugins));
    let (n, last) = (Arc::new(AtomicUsize::new(0)), Arc::new(std::sync::Mutex::new(0)));
    let (n2, l2) = (n.clone(), last.clone());
    e.on_change(move |dto| {
        n2.fetch_add(1, Ordering::Relaxed);
        *l2.lock().unwrap() = dto.revision;
    });
    (e, n, last)
}

fn vol(e: &Engine, v: f32) -> Op {
    let mut t = e.project().tracks[0].clone();
    t.vol = v;
    Op::PutTrack { track: t }
}

#[test]
fn a_drag_is_reported_once_when_it_ends() {
    let (e, n, last) = engine();
    e.begin_gesture(None);
    for k in 0..20 {
        let op = vol(&e, 0.3 + k as f32 * 0.01);
        e.apply(&[op], true, None).unwrap();
    }
    assert_eq!(n.load(Ordering::Relaxed), 0, "no state per transient batch");
    e.end_gesture();
    assert_eq!(n.load(Ordering::Relaxed), 1);
    assert_eq!(*last.lock().unwrap(), e.revision(), "the report carries the final revision");
    assert!(e.state().can_undo);
}

#[test]
fn a_drag_back_to_the_start_still_reports_its_revisions() {
    let (e, n, last) = engine();
    let v0 = e.project().tracks[0].vol;
    e.begin_gesture(None);
    let op = vol(&e, v0 * 0.5);
    e.apply(&[op], true, None).unwrap();
    let op = vol(&e, v0);
    e.apply(&[op], true, None).unwrap();
    e.end_gesture();
    assert_eq!(n.load(Ordering::Relaxed), 1);
    assert_eq!(*last.lock().unwrap(), e.revision());
}

#[test]
fn edits_outside_a_gesture_are_reported_each_time() {
    let (e, n, _) = engine();
    for k in 0..3 {
        let op = vol(&e, 0.3 + k as f32 * 0.1);
        e.apply(&[op], true, None).unwrap();
        let op = vol(&e, 0.2 + k as f32 * 0.1);
        e.apply(&[op], false, None).unwrap();
    }
    assert_eq!(n.load(Ordering::Relaxed), 6);
}

#[test]
fn a_noop_gesture_never_moves_the_revision() {
    let (e, n, _) = engine();
    // a snap can swallow drag moves into one grid point: the batch changes nothing
    let rev = e.revision();
    e.begin_gesture(None);
    e.apply(&[vol(&e, e.project().tracks[0].vol)], true, None).unwrap();
    assert_eq!(e.revision(), rev, "a no-op gesture frame must not bump the revision");
    e.end_gesture();
    assert_eq!(n.load(Ordering::Relaxed), 0, "nothing changed, nothing to report");
    assert!(!e.state().can_undo);
}

#[test]
fn lane_height_drags_alone_are_not_an_undo_step() {
    let (e, _, _) = engine();
    let mut t = e.project().tracks[0].clone();
    t.h *= 2.0;
    e.apply(&[Op::PutTrack { track: t.clone() }], false, None).unwrap();
    assert!(!e.state().can_undo, "a lane-height edit carries into history, it is not history");
    assert_eq!(e.project().tracks[0].h, t.h);
}

#[test]
fn undoing_a_drag_keeps_the_lanes_as_they_are_now() {
    let (e, n, _) = engine();
    let h0 = e.project().tracks[0].h;
    e.begin_gesture(None);
    // the lane is dragged, then the clip too: one undo step for the whole drag, but the
    // lane keeps its new height when the drag is undone
    let mut t = e.project().tracks[0].clone();
    t.h = h0 * 2.0;
    e.apply(&[Op::PutTrack { track: t }], true, None).unwrap();
    e.apply(&[vol(&e, 0.4)], true, None).unwrap();
    e.end_gesture();
    assert!(e.state().can_undo);
    assert_eq!(n.load(Ordering::Relaxed), 1);
    e.undo();
    let p = e.project();
    assert_eq!(p.tracks[0].h, h0 * 2.0, "undoing the drag must keep the lane's new height");
    assert_ne!(p.tracks[0].vol, 0.4, "the drag itself is undone");
}

#[test]
fn the_menu_sees_what_each_step_was_called() {
    let (e, _, _) = engine();
    e.begin_gesture(Some("Move clips".into()));
    e.apply(&[vol(&e, 0.3)], true, None).unwrap();
    e.end_gesture();
    assert_eq!(e.state().undo_label.as_deref(), Some("Move clips"), "the drag's name is on the step");
    // an unnamed edit takes the menu's name: it is the step Ctrl+Z would take back now
    e.apply(&[vol(&e, 0.5)], false, None).unwrap();
    assert_eq!(e.state().undo_label, None);
    e.undo();
    assert_eq!(e.state().undo_label.as_deref(), Some("Move clips"), "undoing reveals the step below");
    assert_eq!(e.state().redo_label, None, "the undone step was unnamed");
    e.redo();
    assert_eq!(e.state().undo_label, None);
    assert_eq!(e.state().redo_label, None);
}

#[test]
fn a_frame_that_returns_to_the_start_still_syncs_the_graph() {
    let (e, n, _) = engine();
    let v0 = e.project().tracks[0].vol;
    let rev0 = e.revision();
    e.begin_gesture(Some("Fader".into()));
    e.apply(&[vol(&e, v0 * 0.5 + 0.1)], true, None).unwrap();
    let rev_mid = e.revision();
    assert!(rev_mid > rev0, "the moved frame is a real edit");
    // back to the fader's starting value: the project matches the gesture snapshot again,
    // but the live graph was left at the moved value and still has to be repaired
    e.apply(&[vol(&e, v0)], true, None).unwrap();
    assert!(e.revision() > rev_mid, "the return frame syncs");
    e.end_gesture();
    assert!(!e.state().can_undo, "a drag back to its start is no undo step");
    assert_eq!(n.load(Ordering::Relaxed), 1);
}
