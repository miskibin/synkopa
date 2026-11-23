//! Playlist markers: the ops, undo, save/open, the ZIP copy, old files and sections.

use std::io::Read;
use std::sync::Arc;

use wave_engine::*;

fn marker(id: Id, at: f64, name: &str) -> Marker {
    Marker { id, at, name: name.into() }
}

fn names(p: &Project) -> Vec<&str> {
    p.markers.iter().map(|m| m.name.as_str()).collect()
}

#[test]
fn add_move_rename_remove_are_each_one_undo_step() {
    let e = Engine::new(Project::sketch(), Arc::new(NoPlugins));
    assert!(e.project().markers.is_empty());
    e.apply(&[Op::PutMarker { marker: marker(7000, 16.0, "Hook") }], false, None).unwrap();
    e.apply(&[Op::PutMarker { marker: marker(7001, 0.0, "Intro") }], false, None).unwrap();
    // sorted by position whatever order they were added in
    assert_eq!(names(&e.project()), ["Intro", "Hook"]);
    assert!(e.project().next_id > 7001);
    assert!(e.state().dirty);
    // move the intro past the hook: the order follows
    e.apply(&[Op::PutMarker { marker: marker(7001, 32.0, "Intro") }], false, None).unwrap();
    assert_eq!(names(&e.project()), ["Hook", "Intro"]);
    e.apply(&[Op::PutMarker { marker: marker(7000, 16.0, "Refren") }], false, None).unwrap();
    e.apply(&[Op::RemoveMarker { id: 7001 }], false, None).unwrap();
    assert_eq!(names(&e.project()), ["Refren"]);
    assert!(e.undo());
    assert_eq!(names(&e.project()), ["Refren", "Intro"]);
    assert!(e.undo());
    assert_eq!(names(&e.project()), ["Hook", "Intro"]);
    assert!(e.undo());
    assert_eq!(names(&e.project()), ["Intro", "Hook"]);
    assert!(e.undo());
    assert!(e.undo());
    assert!(e.project().markers.is_empty());
    assert!(e.redo());
    assert_eq!(names(&e.project()), ["Hook"]);
}

#[test]
fn invalid_markers_are_rejected_and_change_nothing() {
    let e = Engine::new(Project::sketch(), Arc::new(NoPlugins));
    e.apply(&[Op::PutMarker { marker: marker(7000, 4.0, "A") }], false, None).unwrap();
    let before = e.project();
    for bad in
        [marker(7001, -0.5, "x"), marker(7001, f64::NAN, "x"), marker(7001, TOTAL_BEATS + 1.0, "x"), marker(7001, 4.0, &"é".repeat(81))]
    {
        assert!(e.apply(&[Op::PutMarker { marker: bad }], false, None).is_err());
    }
    assert!(e.apply(&[Op::RemoveMarker { id: 99 }], false, None).is_err());
    assert_eq!(e.project(), before);
    // 80 characters (not bytes) is fine
    e.apply(&[Op::PutMarker { marker: marker(7000, 4.0, &"é".repeat(80)) }], false, None).unwrap();
}

#[test]
fn the_number_of_markers_is_bounded() {
    let mut p = Project::sketch();
    let ops: Vec<Op> = (0..MARKERS_MAX as u64).map(|i| Op::PutMarker { marker: marker(9000 + i, i as f64, "m") }).collect();
    ops::apply_all(&mut p, &ops).unwrap();
    let err = ops::apply_all(&mut p, &[Op::PutMarker { marker: marker(99_999, 1.0, "one too many") }]).unwrap_err();
    assert_eq!(err.to_string(), format!("a project has at most {MARKERS_MAX} markers"));
    // replacing one at the limit is still fine
    ops::apply_all(&mut p, &[Op::PutMarker { marker: marker(9000, 1.5, "moved") }]).unwrap();
}

#[test]
fn markers_survive_save_open_and_the_zip_copy() {
    let dir = std::env::temp_dir().join(format!("wave-markers-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("song.synkopa");
    let e = Engine::new(Project::empty(), Arc::new(NoPlugins));
    e.apply(&[Op::PutMarker { marker: marker(7000, 0.0, "Intro") }, Op::PutMarker { marker: marker(7001, 64.0, "Drop") }], false, None)
        .unwrap();
    e.save(&path).unwrap();
    let f = Engine::new(Project::sketch(), Arc::new(NoPlugins));
    f.open(&path).unwrap();
    assert_eq!(f.project().markers, e.project().markers);

    let zip_path = dir.join("song.zip");
    e.export_project_zip(&zip_path, None).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&zip_path).unwrap()).unwrap();
    let name = (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().unwrap().to_string()).find(|n| n.ends_with(".synkopa")).unwrap();
    let mut text = String::new();
    zip.by_name(&name).unwrap().read_to_string(&mut text).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["project"]["markers"][1], serde_json::json!({"id": 7001, "at": 64.0, "name": "Drop"}));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn old_projects_without_markers_load_and_none_are_written() {
    let mut v = serde_json::to_value(Project::sketch()).unwrap();
    // a project without markers does not write the field (older builds read the file unchanged)
    assert!(v.get("markers").is_none());
    v.as_object_mut().unwrap().remove("markers");
    let p: Project = serde_json::from_value(v).unwrap();
    assert!(p.markers.is_empty());
    // a hand-written marker without a name is nameless
    let m: Marker = serde_json::from_str(r#"{"id":1,"at":8}"#).unwrap();
    assert_eq!(m.name, "");
}

#[test]
fn a_section_runs_to_the_next_marker_or_the_song_end() {
    let mut p = Project::sketch();
    ops::apply_all(
        &mut p,
        &[
            Op::PutMarker { marker: marker(7000, 0.0, "Intro") },
            Op::PutMarker { marker: marker(7001, 8.0, "Hook") },
            Op::PutMarker { marker: marker(7002, 8.0, "Hook too") },
        ],
    )
    .unwrap();
    assert_eq!(p.marker_section(7000), Some(LoopRange { a: 0.0, b: 8.0 }));
    // two markers at one place: the next one further on, or the song end
    let end = p.song_end().max(8.0 + 4.0);
    assert_eq!(p.marker_section(7001), Some(LoopRange { a: 8.0, b: end }));
    assert_eq!(p.marker_section(99), None);
    // a marker past the last clip still gets a bar
    ops::apply_all(&mut p, &[Op::PutMarker { marker: marker(7003, 1000.0, "Outro") }]).unwrap();
    assert_eq!(p.marker_section(7003), Some(LoopRange { a: 1000.0, b: 1004.0 }));
}
