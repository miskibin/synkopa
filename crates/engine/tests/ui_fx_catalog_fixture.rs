//! The built-in effects' parameters and presets, as the engine serves them (`fx_catalog`), written
//! to `ui/src/lib/__fixtures__/fx-catalog.json`. The UI's in-browser mock keeps its own copy
//! (`ui/src/api/mockData.ts`); a UI test checks the copy against this file, so the mock cannot
//! drift from the engine unnoticed.

use wave_engine::catalog::{catalog, params, sets_latency, BUILTIN};

#[test]
fn the_ui_fx_catalog_fixture_matches() {
    let c = catalog();
    let mut v = serde_json::json!({ "types": c.types, "presets": c.presets });
    // only what the mock mirrors: built-in types and their presets
    v["order"] = serde_json::to_value(BUILTIN).unwrap();
    // the parameters automation leaves alone because they move latency (`sets_latency`)
    let lat: Vec<String> = BUILTIN
        .iter()
        .flat_map(|&k| {
            params(k)
                .iter()
                .filter(move |d| sets_latency(k, d.id))
                .map(move |d| format!("{}.{}", serde_json::to_value(k).unwrap().as_str().unwrap(), d.id))
        })
        .collect();
    v["latencyParams"] = serde_json::to_value(lat).unwrap();
    let text = serde_json::to_string_pretty(&v).unwrap() + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/lib/__fixtures__/fx-catalog.json");
    if std::env::var("WAVE_WRITE_FIXTURES").is_ok() {
        std::fs::write(&path, &text).unwrap();
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert_eq!(
        on_disk, text,
        "ui/src/lib/__fixtures__/fx-catalog.json is stale: run with WAVE_WRITE_FIXTURES=1 cargo test -p wave-engine --test ui_fx_catalog_fixture"
    );
}
