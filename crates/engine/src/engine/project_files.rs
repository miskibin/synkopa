//! Project files: save, save a copy, zip export, open and recovery — plus where a
//! project's media and scratch files live.

use super::*;

impl Engine {
    pub fn new_project(&self, project: Project) -> Result<(), EngineError> {
        self.vet_project(&project);
        let mut c = self.core.lock();
        // a notes-only take lives outside `c.take`: it would otherwise finish into the new
        // project (old track index, old channel id), after the reset already cleared recovery
        if c.take.is_some() || self.midi_take.lock().rec.is_some() {
            return Err(EngineError::Msg("recording: stop it first".into()));
        }
        send(&mut c, Cmd::Stop);
        c.pat_gen += 1;
        let gen = c.pat_gen;
        send(&mut c, Cmd::PatPlay { id: None, from: 0.0, range: None, gen });
        c.transport.playing = false;
        let mut project = project;
        project.migrate();
        // ids start over in a new project: audio of the previous one must not answer to them
        let mut sources = SourceStore::default();
        load_library_sources(&project, &mut sources);
        c.sources = sources;
        c.source_errors.clear();
        c.project = project;
        c.undo.clear();
        c.redo.clear();
        // an open drag belongs to the replaced project: its snapshot must not land in the
        // new one's undo stack at `end_gesture` (as after undo/redo)
        c.gesture = None;
        c.gesture_unreported = false;
        c.audition.clear();
        c.path = None;
        c.revision += 1;
        c.saved_revision = c.revision;
        c.fx_errors.clear();
        // another project's channel may reuse an id: its plugin instruments start fresh
        self.forget_plugs(&mut c);
        self.sync(&mut c, Dirty { timeline: true, mixer: true, params: true });
        self.changed(c);
        Ok(())
    }

    pub fn save(&self, path: &Path) -> Result<(), EngineError> {
        let mut c = self.core.lock();
        self.snapshot_plugin_states(&mut c.project);
        let dir = path.parent().unwrap_or(Path::new("."));
        let mut p = c.project.clone();
        // move recordings that live in the scratch folder next to the project. The copy
        // never replaces an existing file (two projects save `Take.wav` next to their own
        // `Song Audio` too), and the live project learns the chosen path only after the
        // document itself was written — a failed save must not re-point the audio.
        let audio_dir = dir.join(format!("{} Audio", path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()));
        let scratch = scratch_dir();
        let mut moved: Vec<(String, String)> = vec![];
        for s in p.sources.iter_mut().filter(|s| !s.path.starts_with(crate::factory::SCHEME)) {
            let sp = PathBuf::from(&s.path);
            if sp.starts_with(&scratch) {
                std::fs::create_dir_all(&audio_dir).map_err(|e| e.to_string())?;
                let dst = copy_no_replace(&sp, &audio_dir).map_err(|e| format!("copying {}: {e}", sp.display()))?;
                let dst = dst.display().to_string();
                moved.push((s.path.clone(), dst.clone()));
                s.path = dst;
            }
            if let Ok(rel) = Path::new(&s.path).strip_prefix(dir) {
                s.path = rel.display().to_string();
            }
        }
        write_project_file(&p, path, true)?;
        // keep absolute paths in memory, at exactly the paths the document names
        for s in c.project.sources.iter_mut() {
            if let Some((_, dst)) = moved.iter().find(|(old, _)| *old == s.path) {
                s.path = dst.clone();
            }
        }
        c.path = Some(path.to_path_buf());
        c.saved_revision = c.revision;
        self.changed(c);
        Ok(())
    }

    /// Writes the current project to `path` without making it the project's file: the
    /// path, the dirty flag and the sources stay as they are (recordings stay in the
    /// scratch folder, paths are absolute). Crash recovery uses it: the autosave never
    /// writes over the project's own file.
    pub fn save_copy(&self, path: &Path) -> Result<(), EngineError> {
        let mut c = self.core.lock();
        self.snapshot_plugin_states(&mut c.project);
        let p = c.project.clone();
        drop(c);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        write_project_file(&p, path, false)
    }

    /// Exports the current project and its audio sources without changing the open document.
    pub fn export_project_zip(&self, path: &Path, if_revision: Option<u64>) -> Result<(), EngineError> {
        let c = self.core.lock();
        if c.take.is_some() || self.midi_take.lock().rec.is_some() {
            return Err(EngineError::Msg("recording: stop it first".into()));
        }
        if let Some(r) = if_revision {
            if r != c.revision {
                return Err(EngineError::Msg(format!("{ERR_REVISION_CONFLICT} project is at revision {}, not {r}", c.revision)));
            }
        }
        let origin = c.path.clone();
        let mut project = c.project.clone();
        self.snapshot_plugin_states(&mut project);
        drop(c);
        crate::project_zip::write(project, path, origin.as_deref()).map_err(EngineError::Msg)
    }

    /// Opens a copy written by [`Engine::save_copy`] as an unsaved project with unsaved
    /// changes: Save asks where to put it instead of overwriting the copy.
    pub fn open_recovered(&self, path: &Path) -> Result<Vec<String>, EngineError> {
        self.open_inner(path, None, Some(None))
    }

    /// [`Self::open_recovered`] of a copy of the project saved at `file`: the project is that
    /// file again, with the copy's changes unsaved, so Save writes them there and closing asks
    /// first. `None`: a project that was never saved (Save asks where to put it).
    pub fn open_recovered_for(&self, path: &Path, file: Option<&Path>) -> Result<Vec<String>, EngineError> {
        self.open_inner(path, None, Some(file))
    }

    /// Loads a project file. Missing audio is reported, not fatal.
    pub fn open(&self, path: &Path) -> Result<Vec<String>, EngineError> {
        self.open_at_revision(path, None)
    }

    /// Checks the expected revision after decoding, before replacing the current project.
    pub fn open_at_revision(&self, path: &Path, if_revision: Option<u64>) -> Result<Vec<String>, EngineError> {
        self.open_inner(path, if_revision, None)
    }

    /// One open path for all callers. `recovered`: the project opens from a recovery copy —
    /// `None` for a never-saved one, `Some(file)` for a copy of `file` — and its final path
    /// and unsaved state are installed in the same commit as the project, so no open in
    /// between can take them. `None`: an ordinary open of `path` itself.
    fn open_inner(&self, path: &Path, if_revision: Option<u64>, recovered: Option<Option<&Path>>) -> Result<Vec<String>, EngineError> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let doc: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("not a Synkopa project: {e}"))?;
        let mut p = Project::from_json(doc.get("project").cloned().unwrap_or(doc)).map_err(|e| format!("not a Synkopa project: {e}"))?;
        p.migrate();
        // media paths are joined against the project's directory: that join must produce
        // absolute paths even when the caller passed a relative one (or a bare file name,
        // whose parent is the empty string, not ".")
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let dir = std::path::absolute(parent).map_err(|e| e.to_string())?;
        let mut warnings = vec![];
        let mut errors = HashMap::new();
        let mut store = SourceStore::default();
        for s in p.sources.iter_mut() {
            // library paths stay portable; everything else is absolute in memory
            if !s.path.starts_with(crate::factory::SCHEME) {
                let sp = PathBuf::from(&s.path);
                let abs = if sp.is_absolute() { sp } else { dir.join(sp) };
                s.path = abs.display().to_string();
            }
            match Audio::load_path(&s.path, &dir) {
                Ok(a) => store.insert(s.id, Arc::new(a)),
                Err(e) => {
                    errors.insert(s.id, e.clone());
                    warnings.push(e);
                }
            }
        }
        if p.inserts.first().map(|m| m.id) != Some(0) {
            return Err(EngineError::Msg("the project has no master insert".into()));
        }
        self.vet_project(&p);
        let mut c = self.core.lock();
        if c.take.is_some() || self.midi_take.lock().rec.is_some() {
            return Err(EngineError::Msg("recording: stop it first".into()));
        }
        if let Some(r) = if_revision {
            if r != c.revision {
                return Err(EngineError::Msg(format!("{ERR_REVISION_CONFLICT} project is at revision {}, not {r}", c.revision)));
            }
        }
        send(&mut c, Cmd::Stop);
        c.pat_gen += 1;
        let gen = c.pat_gen;
        send(&mut c, Cmd::PatPlay { id: None, from: 0.0, range: None, gen });
        c.transport.playing = false;
        c.sources = store;
        c.source_errors = errors;
        c.project = p;
        c.audition.clear();
        c.undo.clear();
        c.redo.clear();
        // an open drag belongs to the replaced project: its snapshot must not land in the
        // new one's undo stack at `end_gesture` (as after undo/redo and in new_project)
        c.gesture = None;
        c.gesture_unreported = false;
        c.path = match recovered {
            Some(file) => file.map(Path::to_path_buf),
            None => Some(path.to_path_buf()),
        };
        c.revision += 1;
        c.saved_revision = c.revision;
        if recovered.is_some() {
            // the recovery copy's changes are unsaved: Save must ask where to put them
            c.revision += 1;
        }
        c.fx_errors.clear();
        // another project's channel may reuse an id: its plugin instruments start fresh
        self.forget_plugs(&mut c);
        self.sync(&mut c, Dirty { timeline: true, mixer: true, params: true });
        self.changed(c);
        Ok(warnings)
    }
}

/// Decodes the sample-library sources of a project that did not come from a file (a new
/// project's starter kit, the demo project). A kit that is not installed leaves its channels silent.
pub(crate) fn load_library_sources(p: &Project, store: &mut SourceStore) {
    for s in p.sources.iter().filter(|s| s.path.starts_with(crate::factory::SCHEME)) {
        if let Ok(a) = Audio::load_path(&s.path, Path::new(".")) {
            store.insert(s.id, Arc::new(a));
        }
    }
}

/// `Song.synkopa` -> `Song (backup).synkopa`: the previous save, kept next to the project.
/// The backup keeps the project's own extension, so a `.wave` project from before the rename
/// gets a `.wave` backup.
/// It is an ordinary project file, so File > Open shows and opens it.
pub fn backup_path(path: &Path) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "synkopa".into());
    path.with_file_name(format!("{stem} (backup).{ext}"))
}

pub(crate) fn project_document(p: &Project) -> serde_json::Value {
    serde_json::json!({ "format": "wave-project", "version": 2, "project": p })
}

/// Serializes `p` and replaces `path` atomically (temp file + rename). With `backup`,
/// the file being replaced is kept as [`backup_path`] first; a failed backup never
/// blocks the save.
fn write_project_file(p: &Project, path: &Path, backup: bool) -> Result<(), EngineError> {
    static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let text = serde_json::to_string_pretty(&project_document(p)).map_err(|e| e.to_string())?;
    // a unique temporary per call: two overlapping saves (`Song.synkopa` and `Song.wave`)
    // must never share one, or the first could publish the second's snapshot
    let tmp = path.with_extension(format!("tmp-{}-{}", std::process::id(), TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    if backup && path.is_file() {
        if let Err(e) = std::fs::copy(path, backup_path(path)) {
            log::warn!("could not keep a backup of {}: {e}", path.display());
        }
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string().into());
    }
    Ok(())
}

pub fn scratch_dir() -> PathBuf {
    dirs_data().join("WAVE").join("Recordings")
}

fn dirs_data() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(p) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(p);
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(h) = std::env::var_os("HOME") {
            return PathBuf::from(h).join("Library/Application Support");
        }
    }
    if let Some(p) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(p);
    }
    if let Some(h) = std::env::var_os("HOME") {
        return PathBuf::from(h).join(".local/share");
    }
    std::env::temp_dir()
}

pub(crate) fn audio_dir(c: &Core) -> PathBuf {
    match &c.path {
        Some(p) => p
            .parent()
            .unwrap_or(Path::new("."))
            .join(format!("{} Audio", p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())),
        None => scratch_dir(),
    }
}

/// `dir/stem.wav`, or `dir/stem (2).wav` … when that file exists: ids restart in every new
/// project and unsaved projects share the scratch folder, so a new take must never
/// overwrite audio another project (or an older save of this one) still points at.
pub(crate) fn fresh_path(dir: &Path, stem: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}.wav"));
    let mut k = 2;
    while p.exists() {
        p = dir.join(format!("{stem} ({k}).wav"));
        k += 1;
    }
    p
}

pub(crate) fn sanitize(s: &str) -> String {
    s.chars().map(|ch| if ch.is_alphanumeric() || ch == ' ' || ch == '-' || ch == '_' { ch } else { '_' }).collect()
}

/// Copies scratch file `src` into `dir` under its own name — or `stem (2).ext` … — creating
/// the destination exclusively, so a file that appears between the name check and the copy
/// is never replaced: two projects save `Take.wav` next to their own `Song Audio` too.
fn copy_no_replace(src: &Path, dir: &Path) -> Result<PathBuf, String> {
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = src.extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
    let mut src_f = std::fs::File::open(src).map_err(|e| e.to_string())?;
    for k in 1..10_000 {
        let name = if k == 1 { format!("{stem}{ext}") } else { format!("{stem} ({k}){ext}") };
        let p = dir.join(name);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
            Ok(mut dst) => {
                if let Err(e) = std::io::copy(&mut src_f, &mut dst) {
                    // a half-written copy (disk full) is not audio: it must not sit in the
                    // project's audio folder claiming the name the next save would use
                    drop(dst);
                    let _ = std::fs::remove_file(&p);
                    return Err(e.to_string());
                }
                return Ok(p);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("no free name for the recording".into())
}
