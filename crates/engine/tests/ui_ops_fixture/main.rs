//! The browser mock must apply ops like the engine: `ui/src/lib/applyOps.ts` is a hand port
//! of `ops::apply`, and this test is their contract. It runs canonical command sequences
//! through the real engine and writes `ui/src/lib/__fixtures__/ops.json`; the UI replays the
//! same rows through its port (`opsParity.test.ts`) and through `MockBackend`
//! (`mockParity.test.ts`). Every run regenerates the expected text and compares, so an
//! engine change the port has not caught up with fails CI — the same ritual as
//! `tune_edit.rs`'s fixture. `WAVE_WRITE_FIXTURES=1 cargo test -p wave-engine
//! --test ui_ops_fixture` rewrites the file.

mod cases;
mod common;

use serde_json::{json, Value};

use common::base;

/// `{"bases": …, "cases": …}` — bases are shared by key so the file stays readable.
fn fixture() -> Value {
    let bases = json!({ "main": base(), "one": common::one_track() });
    let rows: Vec<Value> = cases::cases()
        .into_iter()
        .map(|(bk, name, ops)| {
            let mut p: wave_engine::Project = serde_json::from_value(bases[bk].clone()).unwrap();
            match wave_engine::ops::apply_all(&mut p, &ops) {
                Ok(_) => json!({ "name": name, "base": bk, "ops": ops, "result": p, "error": null }),
                Err(e) => json!({ "name": name, "base": bk, "ops": ops, "result": null, "error": e.to_string() }),
            }
        })
        .collect();
    json!({ "bases": bases, "cases": rows })
}

#[test]
fn the_ui_ops_fixture_matches() {
    let text = serde_json::to_string_pretty(&fixture()).unwrap() + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/lib/__fixtures__/ops.json");
    if std::env::var("WAVE_WRITE_FIXTURES").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert_eq!(
        on_disk, text,
        "ui/src/lib/__fixtures__/ops.json is stale: run with WAVE_WRITE_FIXTURES=1 cargo test -p wave-engine --test ui_ops_fixture"
    );
}
