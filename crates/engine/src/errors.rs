//! Error-string prefixes shared across the IPC seam. Messages the engine and the app throw
//! start with one of these, and consumers parse the prefix to classify the failure: UI
//! views (`FxChains.tsx` offers an overwrite menu on `EXISTS:`), the CLI's `e.code`, the
//! mock backend (`ui/src/api/mock.ts`). The UI mirrors them in `ui/src/api/errors.ts`.
//! `app/src/ui_prefixes_tests.rs` pins both sides to this module, so a rename or a new
//! prefix fails CI until both languages agree.

/// A name is taken (`fxChainSave`, `fxChainRename`, `synkopa apply --force`…).
pub const ERR_EXISTS: &str = "EXISTS:";
/// The caller's `if_revision` no longer matches (`Engine::apply`, `watch`, session edits).
pub const ERR_REVISION_CONFLICT: &str = "REVISION_CONFLICT:";
/// An automation target names a parameter that does not exist (a dead envelope).
pub const ERR_INVALID_TARGET: &str = "INVALID_TARGET:";
/// A hosted plugin exposes no window of its own (`plugins.rs`).
pub const ERR_NO_EDITOR: &str = "NO_EDITOR:";
/// Stem extraction needs the HTDemucs model, which is not installed (`app/src/stems_actions.rs`):
/// the Extract stems dialog offers the download on it.
pub const ERR_NO_STEM_MODEL: &str = "STEM_MODEL_MISSING:";
