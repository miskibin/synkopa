//! `Engine::apply_with_library`: library samples become sources and the ops built on them are
//! applied as one undo step (how a MIDI import adds its drum samplers); a failure changes nothing.

use std::sync::Arc;

use wave_engine::*;

#[test]
fn samples_and_ops_are_one_undo_step() {
    let e = Engine::new(Project::empty(), Arc::new(NoPlugins));
    let p0 = e.project();
    let rev = e
        .apply_with_library(&["kick-808.flac", "snare-studio.flac", "kick-808.flac"], None, |p, ids| {
            assert_eq!(ids.len(), 2);
            let ch = Channel::sampler(p.next_id, "Kick", "#fff", ids["kick-808.flac"]);
            Ok(vec![Op::PutChannel { channel: ch, index: None }])
        })
        .unwrap();
    assert_eq!(rev, e.revision());
    let p = e.project();
    assert_eq!(p.sources.len(), p0.sources.len() + 2);
    assert_eq!(p.channels.len(), p0.channels.len() + 1);
    assert!(e.undo());
    assert_eq!(e.project(), p0, "one undo removes the samples and the channel");
    assert!(!e.state().can_undo);
}

#[test]
fn a_failing_build_or_op_changes_nothing() {
    let e = Engine::new(Project::empty(), Arc::new(NoPlugins));
    let p0 = e.project();
    let rev = e.revision();
    assert!(e.apply_with_library(&["kick-808.flac"], None, |_, _| Err("no".into())).is_err());
    assert!(e.apply_with_library(&["kick-808.flac"], None, |_, _| Ok(vec![Op::RemoveChannel { id: 987_654 }])).is_err());
    assert!(e.apply_with_library(&["no-such.flac"], None, |_, _| Ok(vec![])).is_err());
    assert!(e.apply_with_library(&[], Some(rev + 5), |_, _| Ok(vec![])).unwrap_err().to_string().contains("REVISION_CONFLICT"));
    assert_eq!(e.project(), p0);
    assert_eq!(e.revision(), rev);
}
