//! deep-3: the four error-string prefixes are a parsed contract across the IPC seam —
//! UI views match on them (`FxChains.tsx` offers an overwrite menu on `EXISTS:`), the
//! session protocol derives `e.code` from them, the mock throws the same (`mock.ts`).
//! The Rust side names them once (`wave_engine::errors`); this test pins them to
//! `ui/src/api/__fixtures__/error-prefixes.json`, which `errors.test.ts` compares against
//! `ui/src/api/errors.ts` — so a rename fails CI in both languages. Same ritual as the
//! engine's ops fixture: `WAVE_WRITE_FIXTURES=1 cargo test -p wave-app ui_prefixes`
//! rewrites the file.

use serde_json::json;
use wave_engine::errors::{ERR_EXISTS, ERR_INVALID_TARGET, ERR_NO_EDITOR, ERR_REVISION_CONFLICT};

#[test]
fn the_ui_error_prefix_fixture_matches() {
    // plugins.rs's full message must keep starting with the shared prefix
    assert!(crate::plugins::NO_EDITOR.starts_with(ERR_NO_EDITOR), "{}", crate::plugins::NO_EDITOR);
    let fixture = json!({
        "exists": ERR_EXISTS,
        "revisionConflict": ERR_REVISION_CONFLICT,
        "invalidTarget": ERR_INVALID_TARGET,
        "noEditor": ERR_NO_EDITOR,
    });
    let text = serde_json::to_string_pretty(&fixture).unwrap() + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/src/api/__fixtures__/error-prefixes.json");
    if std::env::var("WAVE_WRITE_FIXTURES").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert_eq!(
        on_disk, text,
        "ui/src/api/__fixtures__/error-prefixes.json is stale: run with WAVE_WRITE_FIXTURES=1 cargo test -p wave-app ui_prefixes"
    );
}
