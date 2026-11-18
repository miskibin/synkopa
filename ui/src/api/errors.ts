// Error-string prefixes shared with the engine: messages the backend throws start with one
// of these, and views match the prefix to classify the failure (FxChains offers an overwrite
// menu on `EXISTS:`, pluginWindows falls back to a note on `NO_EDITOR:`). The Rust side
// names them once in `crates/engine/src/errors.rs`; `__fixtures__/error-prefixes.json`
// (written by `app/src/ui_prefixes_tests.rs`) pins both sides, so a rename fails CI until
// this file and the engine agree.
export const ERR_EXISTS = 'EXISTS:';
export const ERR_REVISION_CONFLICT = 'REVISION_CONFLICT:';
export const ERR_INVALID_TARGET = 'INVALID_TARGET:';
export const ERR_NO_EDITOR = 'NO_EDITOR:';
export const ERR_NO_STEM_MODEL = 'STEM_MODEL_MISSING:';
