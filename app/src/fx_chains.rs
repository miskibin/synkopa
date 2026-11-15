//! FX chain presets: the effects of a mixer insert, in order, saved by name to the user's library
//! and loaded onto any insert of any project (FL Studio's *Save / Open mixer track state*, REAPER's
//! FX chains). A chain holds the effects only: type, parameters, EQ bands, dry/wet, bypass, Warp
//! maps and VST3 plugin state. Never the insert's volume, pan, name, colour, routing or automation.
//!
//! One JSON file per preset, `<name>.wavechain`, in `%LOCALAPPDATA%\WAVE\FX Chains` (the same
//! root as the library). The file name is the preset's name.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use wave_engine::{Engine, Fx, FxKind, Op};

use crate::actions::{arg, need_str, need_u64, Host};

type R<T> = Result<T, String>;

pub const EXT: &str = "wavechain";
const FORMAT: &str = "wave-fx-chain";
const VERSION: u32 = 1;
/// Longest preset name, in characters.
const MAX_NAME: usize = 80;
pub const COMMANDS: &[&str] = &["fx_chains", "fx_chain_save", "fx_chain_load", "fx_chain_rename", "fx_chain_delete"];

/// `%LOCALAPPDATA%\WAVE\FX Chains` (`~/.local/share/WAVE/FX Chains` elsewhere).
pub fn default_dir() -> PathBuf {
    wave_engine::factory::library_dir().parent().map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir).join("FX Chains")
}

#[derive(Serialize, Deserialize)]
struct ChainFile {
    format: String,
    version: u32,
    /// In chain order; ids are 0 (a load gives them fresh ones).
    fx: Vec<Fx>,
}

/// How a chain lands on an insert that already has effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Replace,
    Append,
}

/// A preset name that is also a valid file name on every system: no `<>:"/\|?*` or control
/// characters, no leading or trailing dot or space, not a reserved device name, 1–80 characters.
pub fn check_name(name: &str) -> R<String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("give the preset a name".into());
    }
    if n.chars().count() > MAX_NAME {
        return Err(format!("a preset name can have at most {MAX_NAME} characters"));
    }
    if crate::actions::safe_file_stem(n) != n || n.starts_with('.') {
        return Err("a preset name cannot contain < > : \" / \\ | ? * or end with a dot".into());
    }
    Ok(n.to_string())
}

fn path_of(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.{EXT}"))
}

/// The file of preset `name`, matched without regard to case (as Windows and macOS do).
fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    let want = name.trim().to_lowercase();
    std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
        p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(EXT))
            && p.file_stem().is_some_and(|s| s.to_string_lossy().to_lowercase() == want)
    })
}

fn read(path: &Path) -> R<Vec<Fx>> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let head: Value = serde_json::from_str(&text).map_err(|_| "not an FX chain preset (the file is damaged)".to_string())?;
    if head.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err("not an FX chain preset".into());
    }
    if head.get("version").and_then(Value::as_u64).is_none_or(|v| v > u64::from(VERSION)) {
        return Err("this preset was saved by a newer WAVE".into());
    }
    let file: ChainFile = serde_json::from_value(head).map_err(|e| {
        if e.to_string().contains("unknown variant") {
            "this preset uses an effect this version of WAVE does not have".to_string()
        } else {
            format!("the preset is damaged: {e}")
        }
    })?;
    Ok(file.fx)
}

/// What a list row shows about an effect: the plugin's name for VST3, else the effect's name.
fn fx_label(f: &Fx) -> String {
    match &f.plugin {
        Some(p) if f.kind == FxKind::Vst3 => p.name.clone(),
        _ => wave_engine::catalog::type_name(f.kind).to_string(),
    }
}

/// Every preset in `dir`, by name (without regard to case). A file that cannot be read is listed
/// with its `error`, so it can still be renamed or deleted.
pub fn list(dir: &Path) -> Vec<Value> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut out: Vec<(String, Value)> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(EXT)))
        .filter_map(|p| {
            let name = p.file_stem()?.to_string_lossy().into_owned();
            let row = match read(&p) {
                Ok(fx) => json!({
                    "name": name,
                    "effects": fx.iter().map(|f| json!({ "type": f.kind, "name": fx_label(f) })).collect::<Vec<_>>(),
                    "plugins": fx.iter().filter(|f| f.kind == FxKind::Vst3).count(),
                }),
                Err(e) => json!({ "name": name, "effects": [], "plugins": 0, "error": e }),
            };
            Some((name.to_lowercase(), row))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.into_iter().map(|(_, v)| v).collect()
}

/// Saves the effects of `insert` as preset `name`. An existing preset of that name is replaced
/// only with `overwrite` (else the error starts with `EXISTS:`). VST3 states are read from the
/// running plugins first. The file is written next to its final name and renamed into place.
pub fn save(engine: &Engine, dir: &Path, insert: u64, name: &str, overwrite: bool) -> R<Value> {
    let name = check_name(name)?;
    let p = engine.project_with_plugin_states();
    let m = p.insert(insert).ok_or(format!("no insert {insert}"))?;
    if m.fx.is_empty() {
        return Err(format!("{} has no effects to save", if m.name.trim().is_empty() { "the insert" } else { m.name.trim() }));
    }
    let existing = find(dir, &name);
    if existing.is_some() && !overwrite {
        return Err(format!("EXISTS: a preset called {name} already exists"));
    }
    let fx: Vec<Fx> = m.fx.iter().map(|f| Fx { id: 0, ..f.clone() }).collect();
    let body = serde_json::to_vec_pretty(&ChainFile { format: FORMAT.into(), version: VERSION, fx }).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let tmp = dir.join(format!(".{name}.{EXT}.tmp"));
    std::fs::write(&tmp, body).map_err(|e| format!("cannot write the preset: {e}"))?;
    // overwriting under another spelling of the same name keeps one file, with the new spelling
    if let Some(old) = existing.filter(|old| *old != path_of(dir, &name)) {
        let _ = std::fs::remove_file(old);
    }
    std::fs::rename(&tmp, path_of(dir, &name)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot write the preset: {e}")
    })?;
    Ok(json!({ "name": name, "effects": m.fx.len() }))
}

/// Puts preset `name` on `insert` as one undo step. On an insert with effects, `mode` must say
/// whether the chain replaces them or goes after them. Effects get fresh ids; a VST3 plugin that
/// will not load keeps its slot and state (`failed` names it), like a project opened elsewhere.
pub fn load(engine: &Engine, dir: &Path, insert: u64, name: &str, mode: Option<Mode>, if_revision: Option<u64>) -> R<Value> {
    let path = find(dir, name).ok_or_else(|| format!("no FX chain preset called {}", name.trim()))?;
    let chain = read(&path)?;
    if chain.is_empty() {
        return Err("the preset has no effects".into());
    }
    let p = engine.project();
    let m = p.insert(insert).ok_or(format!("no insert {insert}"))?;
    let mode = match (mode, m.fx.is_empty()) {
        (Some(mode), _) => mode,
        (None, true) => Mode::Append,
        (None, false) => {
            return Err(
                "the insert already has effects: say whether the preset replaces them or goes after them (mode replace or append)".into()
            )
        }
    };
    let mut ops: Vec<Op> = vec![];
    if mode == Mode::Replace {
        ops.extend(m.fx.iter().map(|f| Op::RemoveFx { fx_id: f.id }));
    }
    let ids: Vec<u64> = (0..chain.len() as u64).map(|i| p.next_id + i).collect();
    ops.extend(chain.into_iter().zip(&ids).map(|(f, &id)| Op::PutFx { insert_id: insert, fx: Fx { id, ..f }, index: None }));
    let revision = engine.apply(&ops, false, if_revision).map_err(|e| e.to_string())?;
    let errors = engine.state().fx_errors;
    let after = engine.project();
    let failed: Vec<Value> = ids
        .iter()
        .filter_map(|id| {
            let err = errors.get(id)?;
            let (_, f) = after.find_fx(*id)?;
            Some(json!({ "fxId": id, "name": fx_label(f), "error": err }))
        })
        .collect();
    Ok(json!({ "revision": revision, "fxIds": ids, "replaced": mode == Mode::Replace, "failed": failed }))
}

/// Renames a preset. Changing only the case of its name is allowed.
pub fn rename(dir: &Path, from: &str, to: &str) -> R<Value> {
    let to = check_name(to)?;
    let src = find(dir, from).ok_or_else(|| format!("no FX chain preset called {}", from.trim()))?;
    if let Some(other) = find(dir, &to) {
        if other != src {
            return Err(format!("EXISTS: a preset called {to} already exists"));
        }
    }
    std::fs::rename(&src, path_of(dir, &to)).map_err(|e| format!("cannot rename the preset: {e}"))?;
    Ok(json!({ "name": to }))
}

/// Deletes a preset's file (for good: the UI asks first).
pub fn delete(dir: &Path, name: &str) -> R<Value> {
    let path = find(dir, name).ok_or_else(|| format!("no FX chain preset called {}", name.trim()))?;
    std::fs::remove_file(&path).map_err(|e| format!("cannot delete the preset: {e}"))?;
    Ok(json!({ "deleted": path.file_stem().map(|s| s.to_string_lossy().into_owned()) }))
}

/// `fx_chains` · `fx_chain_save {insert, name, overwrite?}` · `fx_chain_load {insert, name, mode?}`
/// · `fx_chain_rename {name, to}` · `fx_chain_delete {name}`.
pub fn control(app: &dyn Host, cmd: &str, a: &Value, if_revision: Option<u64>) -> Option<R<Value>> {
    if !COMMANDS.contains(&cmd) {
        return None;
    }
    let dir = app.fx_chain_dir();
    let engine: &Arc<Engine> = app.engine();
    Some(match cmd {
        "fx_chains" => Ok(json!({ "dir": dir.display().to_string(), "presets": list(&dir) })),
        "fx_chain_save" => (|| {
            let overwrite = arg(a, "overwrite").and_then(Value::as_bool).unwrap_or(false);
            save(engine, &dir, need_u64(a, "insert")?, need_str(a, "name")?, overwrite)
        })(),
        "fx_chain_load" => (|| {
            let mode = match arg(a, "mode").and_then(Value::as_str) {
                None => None,
                Some("replace") => Some(Mode::Replace),
                Some("append") => Some(Mode::Append),
                Some(o) => return Err(format!("mode must be replace or append, not `{o}`")),
            };
            let name = need_str(a, "name")?;
            // from the command line, plugins only run (and report load errors) when asked to
            let has_plugins = find(&dir, name).and_then(|p| read(&p).ok()).is_some_and(|fx| fx.iter().any(|f| f.kind == FxKind::Vst3));
            if has_plugins {
                app.ensure_live_plugins();
            }
            load(engine, &dir, need_u64(a, "insert")?, name, mode, if_revision)
        })(),
        "fx_chain_rename" => (|| rename(&dir, need_str(a, "name")?, need_str(a, "to")?))(),
        _ => (|| delete(&dir, need_str(a, "name")?))(),
    })
}

#[cfg(test)]
#[path = "fx_chains_tests.rs"]
mod tests;
