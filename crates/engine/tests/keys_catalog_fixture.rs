//! The browser mock lists Synkopa Keys instruments like the engine: this test writes the
//! catalog (with each instrument's articulations) to `ui/src/lib/__fixtures__/keys-catalog.json`,
//! which the mock's `keysStatus` and the UI's op validation read. A catalog change the
//! fixture has not caught up with fails CI. `WAVE_WRITE_FIXTURES=1 cargo test -p wave-engine
//! --test keys_catalog_fixture` rewrites it.

use serde_json::{json, Value};
use wave_engine::keys::{articulations, CATALOG, RELEASE_MS};

/// Download size of each library kit (`assets-manifest.json`), for the mock's download button.
fn kit_bytes(kit: &str) -> u64 {
    let m: Value = serde_json::from_str(include_str!("../../../assets-manifest.json")).unwrap();
    m["instruments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["id"] == kit)
        .map_or(0, |k| k["samples"].as_array().unwrap().iter().map(|s| s["bytes"].as_u64().unwrap()).sum())
}

fn fixture() -> Value {
    json!({
        "releaseMs": [RELEASE_MS.0, RELEASE_MS.1],
        "instruments": CATALOG
            .iter()
            .map(|i| {
                let mut v = serde_json::to_value(i).unwrap();
                v["articulations"] = json!(articulations(i.id));
                v["bytes"] = json!(kit_bytes(i.kit));
                v
            })
            .collect::<Vec<_>>(),
    })
}

#[test]
fn the_keys_catalog_fixture_matches() {
    let text = serde_json::to_string_pretty(&fixture()).unwrap() + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/lib/__fixtures__/keys-catalog.json");
    if std::env::var("WAVE_WRITE_FIXTURES").is_ok() {
        std::fs::write(&path, &text).unwrap();
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert_eq!(
        on_disk, text,
        "ui/src/lib/__fixtures__/keys-catalog.json is stale: run with WAVE_WRITE_FIXTURES=1 cargo test -p wave-engine --test keys_catalog_fixture"
    );
}
