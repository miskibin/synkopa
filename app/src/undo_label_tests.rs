//! The undo-step name through the `control` door: the CLI's file mode carries `--label` in the
//! args (no session header field), so `apply` and the other label-named verbs honor it there;
//! the session header keeps precedence when both are present.

use wave_engine::Project;

use super::*;
use crate::headless::Headless;

#[test]
fn an_args_label_names_the_apply_undo_step_in_file_mode() {
    let h = Headless::new(Project::sketch());
    let r = control(&h, "apply", &json!({ "ops": [{ "op": "setBpm", "bpm": 140.0 }], "label": "Tempo to 140" }), None, None).unwrap();
    assert!(r["revision"].is_u64());
    assert_eq!(h.engine().state().undo_label.as_deref(), Some("Tempo to 140"), "the args label names the step");
}

#[test]
fn the_session_header_label_wins_over_an_args_one() {
    let h = Headless::new(Project::sketch());
    control(&h, "apply", &json!({ "ops": [{ "op": "setBpm", "bpm": 140.0 }], "label": "FromArgs" }), None, Some("FromHeader")).unwrap();
    assert_eq!(h.engine().state().undo_label.as_deref(), Some("FromHeader"));
}

#[test]
fn an_import_names_its_library_commit() {
    let h = Headless::new(Project::sketch());
    let d = std::env::temp_dir().join(format!("wave-undo-import-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let path = d.join("in.mid");
    let tracks = vec![wave_engine::midi_file::OutTrack {
        name: "Piano".into(),
        channel: 0,
        program: Some(0),
        notes: vec![wave_engine::midi_file::SmfNote { key: 60, vel: 100, start: 0.0, len: 1.0 }],
    }];
    std::fs::write(&path, wave_engine::midi_file::write(120.0, "in", &tracks).unwrap()).unwrap();
    control(&h, "midi_import", &json!({ "path": path.display().to_string() }), None, None).unwrap();
    assert_eq!(h.engine().state().undo_label.as_deref(), Some("Import MIDI"), "the import says what it is, not \"Relink audio\"");
    let _ = std::fs::remove_dir_all(&d);
}
