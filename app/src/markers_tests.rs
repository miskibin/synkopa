//! Playlist markers through the shared action, as the app and `synkopa --project FILE markers …`
//! use it: add (by bar or beat), rename and move by name, loop a section, remove, the summary in
//! `context`, and the whole command line on a file.

use serde_json::{json, Value};
use wave_cli::Files as _;

use crate::headless::Files;

fn call(doc: &dyn wave_cli::Doc, cmd: &str, a: Value) -> Value {
    doc.call(cmd, &a, None).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn names(v: &Value) -> Vec<String> {
    v["markers"].as_array().or(v.as_array()).unwrap().iter().map(|m| m["name"].as_str().unwrap().to_string()).collect()
}

#[test]
fn add_rename_move_loop_and_remove() {
    let doc = Files.create("sketch").unwrap();
    assert_eq!(call(&*doc, "markers", json!({})), json!([]));
    let r = call(&*doc, "markers", json!({"add": "Hook", "bar": 9}));
    assert!(r["added"].as_u64().is_some());
    call(&*doc, "markers", json!({"add": " Intro ", "at": 0}));
    let r = call(&*doc, "markers", json!({}));
    assert_eq!(names(&json!({"markers": r})), ["Intro", "Hook"]);
    assert_eq!(r[1]["at"], json!(32.0));
    assert_eq!(r[1]["bar"], json!(9.0));

    // by name, case-insensitive; a move keeps the list sorted
    let r = call(&*doc, "markers", json!({"marker": "intro", "name": "Drop", "bar": 17}));
    assert_eq!(names(&r), ["Hook", "Drop"]);
    // the section of the hook runs to the drop
    let r = call(&*doc, "markers", json!({"loop": "Hook"}));
    assert_eq!(r["loop"], json!({"a": 32.0, "b": 64.0}));
    let ctx = call(&*doc, "context", json!({}));
    assert_eq!(ctx["markers"][1]["name"], json!("Drop"));
    let id = ctx["markers"][0]["id"].as_u64().unwrap();
    let r = call(&*doc, "markers", json!({"remove": id}));
    assert_eq!(names(&r), ["Drop"]);
}

#[test]
fn bad_requests_say_what_is_wrong() {
    let doc = Files.create("sketch").unwrap();
    let err = |a: Value| doc.call("markers", &a, None).unwrap_err();
    assert!(err(json!({"remove": "Nope"})).contains("no marker named `nope`"), "{}", err(json!({"remove": "Nope"})));
    assert!(err(json!({"add": "A", "at": 4, "bar": 2})).contains("not both"));
    assert!(err(json!({"add": "A", "bar": 0})).contains("bars count from 1"));
    assert!(err(json!({"add": "x".repeat(81)})).contains("at most 80"));
    assert!(err(json!({"add": "Far", "at": 5000})).contains("position must be"));
    call(&*doc, "markers", json!({"add": "A"}));
    assert!(err(json!({"marker": "A"})).contains("say what changes"));
}

#[test]
fn the_command_line_on_a_file() {
    let d = std::env::temp_dir().join(format!("wave-markers-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join("song.synkopa");
    let file = f.display().to_string();
    let run = |args: &[&str]| {
        let mut raw = vec!["--project".to_string(), file.clone()];
        raw.extend(args.iter().map(|s| s.to_string()));
        wave_cli::execute(raw, Some(&Files)).unwrap_or_else(|e| panic!("{args:?}: {e}"))
    };
    wave_cli::execute(vec!["new".into(), file.clone(), "--template".into(), "sketch".into()], Some(&Files)).unwrap();
    run(&["markers", "--add", "Verse", "--bar", "5"]);
    run(&["markers", "--add", "Hook", "--at", "32"]);
    run(&["markers", "--marker", "Verse", "--name", "Verse 1"]);
    let list = run(&["markers"]);
    assert_eq!(names(&json!({"markers": list})), ["Verse 1", "Hook"]);
    run(&["markers", "--remove", "Hook"]);
    // the file holds what was done
    let text = std::fs::read_to_string(&f).unwrap();
    assert!(text.contains("\"Verse 1\"") && !text.contains("\"Hook\""), "{text}");
    let _ = std::fs::remove_dir_all(d);
}
