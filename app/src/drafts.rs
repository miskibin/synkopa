//! Recently closed unnamed projects. Crash recovery has its own lifecycle.
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use wave_engine::Engine;

#[derive(Serialize, Deserialize)]
pub struct Details {
    bpm: f64,
    tracks: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    id: String,
    saved_ms: u64,
    details: Option<Details>,
}

pub struct Drafts {
    dir: PathBuf,
    active: Mutex<Option<String>>,
}

impl Drafts {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, active: Mutex::new(None) }
    }

    fn path(&self, id: &str) -> Result<PathBuf, String> {
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
            return Err("invalid draft id".into());
        }
        Ok(self.dir.join(id).with_extension("synkopa"))
    }

    pub fn list(&self) -> std::io::Result<Vec<Draft>> {
        let entries = match fs::read_dir(&self.dir) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            result => result?,
        };
        let mut rows = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("synkopa") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()).filter(|id| self.path(id).is_ok()) else { continue };
            let Ok(time) = path.metadata().and_then(|m| m.modified()) else { continue };
            let saved_ms = time.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
            let details = fs::read(path.with_extension("json")).ok().and_then(|b| serde_json::from_slice(&b).ok());
            rows.push(Draft { id: id.into(), saved_ms, details });
        }
        rows.sort_by(|a, b| b.saved_ms.cmp(&a.saved_ms).then_with(|| b.id.cmp(&a.id)));
        Ok(rows)
    }

    pub fn keep(&self, engine: &Engine, opening: Option<&str>) -> Result<(), String> {
        let state = engine.state();
        if state.path.is_some() || !state.dirty {
            return Ok(());
        }
        let mut active = self.active.lock();
        let id = active.get_or_insert_with(|| {
            static SEQ: AtomicU64 = AtomicU64::new(0);
            let time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
            format!("{time}-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed))
        });
        fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        // Serialize retention across app instances. Protect the project being opened even
        // when it is the oldest and the outgoing project needs a fourth slot.
        let lock =
            File::options().create(true).truncate(false).write(true).open(self.dir.join("drafts.lock")).map_err(|e| e.to_string())?;
        lock.lock().map_err(|e| e.to_string())?;
        let path = self.path(id)?;
        engine.save_copy(&path).map_err(|e| e.to_string())?;
        let details = Details { bpm: state.project.bpm, tracks: state.project.tracks.len() };
        if let Ok(bytes) = serde_json::to_vec(&details) {
            let _ = fs::write(path.with_extension("json"), bytes);
        }
        let rows = self.list().map_err(|e| e.to_string())?;
        let excess = rows.len().saturating_sub(3);
        for row in rows.iter().rev().filter(|r| r.id != *id && Some(r.id.as_str()) != opening).take(excess) {
            self.remove(&row.id)?;
        }
        Ok(())
    }

    fn remove(&self, id: &str) -> Result<(), String> {
        let path = self.path(id)?;
        match fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
            _ => {}
        }
        let _ = fs::remove_file(path.with_extension("json"));
        Ok(())
    }

    pub fn switched(&self, id: Option<String>) {
        *self.active.lock() = id;
    }

    pub fn named(&self) {
        let Some(id) = self.active.lock().take() else { return };
        if let Err(e) = self.remove(&id) {
            log::warn!("could not remove saved draft {id}: {e}");
        }
    }

    pub fn open(&self, engine: &Engine, id: &str, revision: u64) -> Result<Vec<String>, String> {
        engine.open_draft(&self.path(id)?, revision).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use wave_engine::{NoPlugins, Op, Project};

    #[test]
    fn unnamed_sketches_survive_restart_and_reopen_without_a_save_path() {
        let dir = tempfile::tempdir().unwrap();
        let drafts = Drafts::new(dir.path().into());
        let e = Engine::new(Project::empty(), Arc::new(NoPlugins));
        drafts.keep(&e, None).unwrap();
        assert!(drafts.list().unwrap().is_empty());
        for bpm in [91.0, 92.0, 93.0, 94.0] {
            e.apply(&[Op::SetBpm { bpm }], false, None).unwrap();
            drafts.keep(&e, None).unwrap();
            drafts.switched(None);
            e.new_project(Project::empty()).unwrap();
        }
        let reopened = Drafts::new(dir.path().into());
        let rows = reopened.list().unwrap();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.details.as_ref().unwrap().bpm != 91.0));
        let id = &rows[2].id;
        reopened.open(&e, id, e.revision()).unwrap();
        assert!(e.state().path.is_none());
        assert!(e.state().dirty);
        assert_eq!(e.state().project.bpm, rows[2].details.as_ref().unwrap().bpm);
        reopened.switched(Some(id.clone()));
        reopened.keep(&e, None).unwrap();
        assert_eq!(reopened.list().unwrap().len(), 3);
        reopened.named();
        assert_eq!(reopened.list().unwrap().len(), 2);
    }
}
