//! Importing audio files, relinking missing sources, and ops built from library files.

use super::*;

/// A file of an import that did not load: which one, why. The files that did load answer
/// as source ids (and clips, when placed).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportFileError {
    pub path: String,
    pub error: String,
}

/// What an import loads: the new sources, the clips placed for them, the files that did not.
pub type Imported = (Vec<Id>, Vec<Id>, Vec<ImportFileError>);

impl Engine {
    /// Decodes files (slow; call off the UI thread) and adds them as sources, as one undo step.
    /// With `place`, also puts one clip per file on consecutive tracks starting there.
    pub fn import_audio(&self, paths: &[PathBuf], place: Option<(u32, f64)>) -> Result<Imported, EngineError> {
        self.import_audio_with(paths, place, true)
    }

    /// [`Engine::import_audio`] where the caller says whether the import is an undo step of its own.
    /// `step` false is for a file loaded on the way to another edit (a sampler channel made from
    /// it, a channel's sample replaced): that edit is the step, and Ctrl+Z after dropping a sample
    /// on the rack took two presses, one of which changed nothing you could see. A new source still
    /// clears redo, so ids only ever grow along the history.
    pub fn import_audio_with(&self, paths: &[PathBuf], place: Option<(u32, f64)>, step: bool) -> Result<Imported, EngineError> {
        let mut loaded = vec![];
        let mut errors: Vec<ImportFileError> = vec![];
        // A file of the sample library is one source however often it is used (a sampler
        // channel and audio clips share it), stored by its portable `lib:` path.
        let known: HashMap<String, Id> = {
            let c = self.core.lock();
            c.project
                .sources
                .iter()
                .filter(|s| s.path.starts_with(crate::factory::SCHEME) && c.sources.get(s.id).is_some())
                .map(|s| (s.path.clone(), s.id))
                .collect()
        };
        for p in paths {
            let stored = match p.to_str() {
                Some(s) if s.starts_with(crate::factory::SCHEME) => s.to_string(),
                _ => crate::factory::portable(p),
            };
            if known.contains_key(&stored) {
                loaded.push((p.clone(), stored, None));
                continue;
            }
            match Audio::load_path(&stored, Path::new(".")) {
                Ok(a) => loaded.push((p.clone(), stored, Some(a))),
                Err(e) => {
                    // the other files still import; the caller reports this one by path
                    errors.push(ImportFileError { path: p.display().to_string(), error: e });
                }
            }
        }
        if loaded.is_empty() {
            let why = errors.iter().map(|e| e.error.as_str()).collect::<Vec<_>>().join("; ");
            return Err(EngineError::Msg(why));
        }
        // A cache hit from the scan can go stale before the commit (another open replaces
        // the project): such a sample is decoded here, outside the lock, and the commit is
        // retried — the import must never succeed having silently skipped a file.
        let mut last_error = None;
        for _retry in 0..3 {
            let mut c = self.core.lock();
            // phase 1 — pure lookup: is anything the scan cached no longer in the project?
            // Nothing is mutated until every file resolves, so a retry never leaves partial
            // additions (or duplicate ones) behind.
            let mut missing = vec![];
            for (_, stored, audio) in loaded.iter() {
                if audio.is_some() {
                    continue;
                }
                let live = c
                    .project
                    .sources
                    .iter()
                    .find(|s| s.path == *stored && s.path.starts_with(crate::factory::SCHEME))
                    .map(|s| s.id)
                    .is_some_and(|id| c.sources.get(id).is_some());
                if !live {
                    missing.push(stored.clone());
                }
            }
            if !missing.is_empty() && _retry < 2 {
                drop(c);
                for stored in &missing {
                    match Audio::load_path(stored, Path::new(".")) {
                        Ok(a) => {
                            if let Some((_, _, slot)) = loaded.iter_mut().find(|(_, s, _)| s == stored) {
                                *slot = Some(a);
                            }
                        }
                        Err(e) => last_error = Some(e),
                    }
                }
                continue;
            }
            if !missing.is_empty() {
                // still gone after a decode round: report them instead of a hollow success
                drop(c);
                let why = last_error.clone().unwrap_or_else(|| "a sample disappeared while it was imported".into());
                for stored in &missing {
                    errors.push(ImportFileError { path: stored.clone(), error: why.clone() });
                }
                break;
            }
            // phase 2 — the commit: every file resolves, so this pass cannot stop halfway
            let before = c.project.clone();
            let mut src_ids = vec![];
            let mut clip_ids = vec![];
            let bpm = c.project.bpm;
            for (k, (path, stored, audio)) in loaded.iter_mut().enumerate() {
                let reuse = c
                    .project
                    .sources
                    .iter()
                    .find(|s| s.path == *stored && s.path.starts_with(crate::factory::SCHEME))
                    .map(|s| (s.id, s.dur))
                    .filter(|(id, _)| c.sources.get(*id).is_some());
                let audio = audio.take();
                let (id, seconds) = match (reuse, audio) {
                    (Some(r), _) => r,
                    (_, None) => unreachable!("phase 1 resolved every cached sample"),
                    (_, Some(audio)) => {
                        let id = c.project.alloc_id();
                        let factory = crate::factory::embedded(stored);
                        let name = match factory {
                            Some(f) => f.name.to_string(),
                            None => path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Audio".into()),
                        };
                        let colors = ["#69c3d6", "#47b8a8", "#5fbf8a", "#7c8cf8", "#e0a44a", "#9a8cf0", "#d9678f", "#5aa9e6"];
                        let color =
                            factory.map(crate::factory::color).unwrap_or(colors[c.project.sources.len() % colors.len()]).to_string();
                        let seconds = audio.seconds();
                        let src = Source {
                            id,
                            name,
                            color,
                            path: stored.clone(),
                            dur: seconds,
                            sr: audio.sr,
                            ch: audio.channels,
                            peak_db: crate::dsp::gain_to_db(audio.peak()),
                        };
                        c.sources.insert(id, Arc::new(audio));
                        c.project.sources.push(src);
                        (id, seconds)
                    }
                };
                let len_beats = seconds / (60.0 / bpm);
                src_ids.push(id);
                if let Some((track, beat)) = place {
                    let ntracks = c.project.tracks.len() as u32;
                    if ntracks == 0 {
                        continue;
                    }
                    let track = (track + k as u32).min(ntracks - 1);
                    let cid = c.project.alloc_id();
                    let start = beat.max(0.0);
                    c.project.clips.push(Clip {
                        id: cid,
                        track,
                        start,
                        len: len_beats.min(TOTAL_BEATS - start).max(1.0 / 16.0),
                        offset: 0.0,
                        kind: ClipKind::Audio,
                        source: id,
                        muted: false,
                        gain: 0.0,
                        fade_in: 0.0,
                        fade_out: 0.0,
                        pan: 0.0,
                        norm: false,
                        rev: false,
                        tune: None,
                        stretch: 1.0,
                        resample: false,
                        pitch: 0.0,
                        insert: None,
                        root: crate::model::CLIP_ROOT,
                    });
                    clip_ids.push(cid);
                }
            }
            let added = c.project.sources.len() != before.sources.len();
            let fold = if step || !clip_ids.is_empty() {
                push_undo(&mut c, before, Some("Import audio".into()));
                None
            } else if added {
                c.redo.clear();
                Some(before)
            } else {
                None
            };
            c.revision += 1;
            if let Some(before) = fold {
                // the edit that follows (the sampler channel) is the step, and it takes the source with it
                let base = match c.import_base.take() {
                    Some((r, b)) if r + 1 == c.revision => b,
                    _ => before,
                };
                c.import_base = Some((c.revision, base));
            }
            self.sync(&mut c, Dirty { timeline: true, ..Default::default() });
            self.changed(c);
            return Ok((src_ids, clip_ids, std::mem::take(&mut errors)));
        }
        Err(EngineError::Msg(errors.iter().map(|e| e.error.as_str()).collect::<Vec<_>>().join("; ")))
    }

    /// Points sources at other files (slow: decodes them; call off the UI thread), as one undo
    /// step. A file that does not decode leaves its source unchanged and is reported; when no
    /// file decodes nothing changes and it is an error. Library (`lib:`) sources come with
    /// their kit and cannot be relinked. The source keeps its name; its length, rate and
    /// channels become the new file's. Undo brings back the old path and what it played.
    pub fn relink_sources(&self, items: &[Relink]) -> Result<Vec<Relinked>, EngineError> {
        let mut out = vec![];
        let mut loaded = vec![];
        let at_rev = {
            let c = self.core.lock();
            for it in items {
                let Some(s) = c.project.sources.iter().find(|s| s.id == it.source_id) else {
                    return Err(EngineError::Msg(format!("no source {}", it.source_id)));
                };
                if s.path.starts_with(crate::factory::SCHEME) {
                    return Err(EngineError::Msg(format!("{}: a library sample; install its kit instead", s.name)));
                }
            }
            c.revision
        };
        for it in items {
            let p = PathBuf::from(&it.path);
            if !p.is_absolute() {
                out.push(Relinked {
                    source_id: it.source_id,
                    path: it.path.clone(),
                    error: Some(format!("{}: not an absolute path", it.path)),
                    length_differs: false,
                });
                continue;
            }
            match Audio::load(&p) {
                Ok(a) => loaded.push((it.source_id, p.display().to_string(), Arc::new(a))),
                Err(e) => out.push(Relinked { source_id: it.source_id, path: it.path.clone(), error: Some(e), length_differs: false }),
            }
        }
        if loaded.is_empty() {
            let why: Vec<String> = out.iter().filter_map(|r| r.error.clone()).collect();
            return Err(EngineError::Msg(why.join("; ")));
        }
        // the relink belongs to the project it was validated on: an edit or open during the
        // decode refuses it, so it cannot land on same-id sources of a replaced project
        self.commit_relinks(loaded, Some(at_rev)).map(|mut relinked| {
            out.append(&mut relinked);
            out
        })
    }

    /// Commits already-decoded relinks as one undo step: the audio handed in is exactly the
    /// audio that gets committed (a search's length check keeps ruling over what a re-decode
    /// would find). `expect_rev`: the project revision the relinks were picked against —
    /// a different project now refuses the whole batch, and the caller starts over.
    fn commit_relinks(&self, loaded: Vec<(Id, String, Arc<Audio>)>, expect_rev: Option<u64>) -> Result<Vec<Relinked>, EngineError> {
        let mut c = self.core.lock();
        for (id, ..) in &loaded {
            let Some(s) = c.project.sources.iter().find(|s| s.id == *id) else {
                return Err(EngineError::Msg(format!("no source {id}")));
            };
            if s.path.starts_with(crate::factory::SCHEME) {
                return Err(EngineError::Msg(format!("{}: a library sample; install its kit instead", s.name)));
            }
        }
        if let Some(r) = expect_rev {
            if r != c.revision {
                return Err(EngineError::Msg(format!("{ERR_REVISION_CONFLICT} project is at revision {}, not {r}", c.revision)));
            }
        }
        let before = c.project.clone();
        let mut out = vec![];
        for (id, path, audio) in loaded {
            // the source may have gone while the files decoded
            let Some(k) = c.project.sources.iter().position(|s| s.id == id) else {
                out.push(Relinked { source_id: id, path, error: Some("the source was removed".into()), length_differs: false });
                continue;
            };
            let seconds = audio.seconds();
            let s = &mut c.project.sources[k];
            let length_differs = !crate::relink::same_length(s.dur, seconds);
            let old = std::mem::replace(&mut s.path, path.clone());
            s.dur = seconds;
            s.sr = audio.sr;
            s.ch = audio.channels;
            s.peak_db = crate::dsp::gain_to_db(audio.peak());
            c.sources.relink(id, &old, &path, audio);
            c.source_errors.remove(&id);
            out.push(Relinked { source_id: id, path, error: None, length_differs });
        }
        push_undo(&mut c, before, Some("Relink audio".into()));
        c.revision += 1;
        self.sync(&mut c, Dirty { timeline: true, ..Default::default() });
        self.changed(c);
        Ok(out)
    }

    /// Looks for the missing sources' files under `folder` (slow: walks it and decodes the
    /// files it takes; call off the UI thread). A source whose file is the only best match by
    /// name and has the remembered length is relinked (all of them as one undo step); the
    /// others are returned as candidates for the user to choose from.
    pub fn relink_search(&self, folder: &Path) -> Result<RelinkSearch, EngineError> {
        if !folder.is_dir() {
            return Err(EngineError::Msg(format!("{}: not a folder", folder.display())));
        }
        // `wanted` and the revision it was scanned at come from one lock hold: the lengths
        // the search validates against belong to exactly the revision the relink commits on
        let (wanted, scan_rev): (Vec<(Id, String, f64)>, u64) = {
            let c = self.core.lock();
            let missing = missing_sources(&c);
            let wanted = c
                .project
                .sources
                .iter()
                .filter(|s| missing.contains_key(&s.id) && !s.path.starts_with(crate::factory::SCHEME))
                .map(|s| (s.id, s.path.clone(), s.dur))
                .collect();
            (wanted, c.revision)
        };
        let names: Vec<(Id, String)> = wanted.iter().map(|w| (w.0, w.1.clone())).collect();
        let found = crate::relink::search(folder, &names, crate::relink::Limits::default());
        let mut candidates = found.candidates;
        // the decode that passed the length check is the decode that gets committed: a file
        // changing under the search cannot smuggle a different length past it
        let mut take: Vec<(Id, String, Arc<Audio>)> = vec![];
        let mut not_found = vec![];
        for (id, _, dur) in &wanted {
            let Some(list) = candidates.get(id) else {
                not_found.push(*id);
                continue;
            };
            let Some(best) = crate::relink::unambiguous(list) else { continue };
            // the length decides; a file that does not decode stays a candidate
            if let Ok(a) = Audio::load(Path::new(&best.path)) {
                if crate::relink::same_length(*dur, a.seconds()) {
                    take.push((*id, best.path.clone(), Arc::new(a)));
                }
            }
        }
        let relinked = if take.is_empty() { vec![] } else { self.commit_relinks(take, Some(scan_rev))? };
        for r in relinked.iter().filter(|r| r.error.is_none()) {
            candidates.remove(&r.source_id);
        }
        Ok(RelinkSearch { relinked, candidates, not_found, scanned: found.scanned, truncated: found.truncated })
    }

    /// Applies ops built against the project once the standard-library samples `files` (file
    /// names, e.g. `kick-808.flac`) are project sources, as one undo step: `build` gets the
    /// project with them and each file's source id. Nothing changes when `build` or the ops
    /// fail. Library samples are decoded from the program, so this is quick.
    pub fn apply_with_library<F>(&self, files: &[&str], if_revision: Option<u64>, build: F) -> Result<u64, EngineError>
    where
        F: FnOnce(&Project, &HashMap<String, Id>) -> Result<Vec<Op>, String>,
    {
        let have: HashSet<String> = {
            let c = self.core.lock();
            c.project.sources.iter().filter(|s| c.sources.get(s.id).is_some()).map(|s| s.path.clone()).collect()
        };
        let mut decoded = vec![];
        let mut seen = HashSet::new();
        for f in files {
            let path = crate::factory::lib_path(f);
            if !seen.insert(path.clone()) || have.contains(&path) {
                continue;
            }
            let a = Audio::load_path(&path, Path::new(".")).map_err(EngineError::Msg)?;
            decoded.push((path, a));
        }
        // A sample that was loaded at the scan can be gone at commit time (another open
        // replaced the project): it is decoded here, outside the lock, and the commit is
        // retried — `build` must never see a map that lacks a file it asked for.
        let mut last_error = None;
        for _retry in 0..3 {
            let mut c = self.core.lock();
            if let Some(r) = if_revision {
                if r != c.revision {
                    return Err(EngineError::Msg(format!("{ERR_REVISION_CONFLICT} project is at revision {}, not {r}", c.revision)));
                }
            }
            let before = c.project.clone();
            let mut ids: HashMap<String, Id> = HashMap::new();
            for f in files {
                let path = crate::factory::lib_path(f);
                if let Some(s) = c.project.sources.iter().find(|s| s.path == path && c.sources.get(s.id).is_some()) {
                    ids.insert(f.to_string(), s.id);
                }
            }
            // requested by file name and neither resolvable from the live project nor decoded
            let missing: Vec<String> = files
                .iter()
                .map(|f| crate::factory::lib_path(f))
                .filter(|path| match crate::factory::embedded(path).map(|ff| ff.file) {
                    Some(n) if ids.contains_key(n) => false,
                    _ => !decoded.iter().any(|(d, _)| d == path),
                })
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            if !missing.is_empty() && _retry < 2 {
                drop(c);
                for path in &missing {
                    match Audio::load_path(path, Path::new(".")) {
                        Ok(a) => decoded.push((path.clone(), a)),
                        Err(e) => last_error = Some(e),
                    }
                }
                continue;
            }
            if !missing.is_empty() {
                drop(c);
                return Err(EngineError::Msg(last_error.unwrap_or_else(|| "a sample disappeared while it was loaded".into())));
            }
            let mut added = vec![];
            for (path, audio) in decoded {
                let Some(ff) = crate::factory::embedded(&path) else { continue };
                if ids.contains_key(ff.file) {
                    continue;
                }
                let id = c.project.alloc_id();
                c.project.sources.push(Source {
                    id,
                    name: ff.name.to_string(),
                    color: crate::factory::color(ff).to_string(),
                    path: path.clone(),
                    dur: audio.seconds(),
                    sr: audio.sr,
                    ch: audio.channels,
                    peak_db: crate::dsp::gain_to_db(audio.peak()),
                });
                c.sources.insert(id, Arc::new(audio));
                ids.insert(ff.file.to_string(), id);
                added.push(id);
            }
            let result = build(&c.project, &ids).map_err(EngineError::Msg).and_then(|ops| {
                let mut p = c.project.clone();
                ops::apply_all(&mut p, &ops).map(|_| p).map_err(EngineError::from)
            });
            let p = match result {
                Ok(p) => p,
                Err(e) => {
                    c.project = before;
                    for id in added {
                        c.sources.audio.remove(&id);
                    }
                    return Err(e);
                }
            };
            c.project = p;
            push_undo(&mut c, before, Some("Relink audio".into()));
            c.revision += 1;
            self.sync(&mut c, Dirty { timeline: true, mixer: true, params: true });
            let rev = c.revision;
            self.changed(c);
            return Ok(rev);
        }
        unreachable!("the retry loop always returns or errors")
    }
}
