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
