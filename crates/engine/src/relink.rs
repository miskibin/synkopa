//! Finding the files of missing audio sources in a folder the user points at (Ableton's
//! "Search Folder", Cubase's "Resolve Missing Files"): a bounded walk that matches by file
//! name and ranks same-named files by how much of the old path they share.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Bounds of one search, so pointing at a whole drive cannot hang the app.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Directory entries looked at (files and folders).
    pub entries: usize,
    /// Folder depth below the searched folder.
    pub depth: usize,
    /// Candidates kept per source.
    pub per_source: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { entries: 250_000, depth: 24, per_source: 12 }
    }
}

/// A file that may be a missing source's, best first.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub path: String,
    /// The file name is the old one (ignoring case); otherwise only the name without the
    /// extension is (the file was converted, e.g. `.wav` → `.flac`).
    pub exact_name: bool,
    /// Trailing folder names it shares with the old path (`Drums/kick.wav` → 1).
    pub shared_folders: usize,
}

#[derive(Debug, Default)]
pub struct Found {
    /// Source id → candidates, best first (sources with none are absent).
    pub candidates: HashMap<u64, Vec<Candidate>>,
    /// Directory entries looked at.
    pub scanned: usize,
    /// The walk stopped at a limit: a file may be in the part it did not reach.
    pub truncated: bool,
}

fn lower(s: &std::ffi::OsStr) -> String {
    s.to_string_lossy().to_lowercase()
}

fn is_audio(p: &Path) -> bool {
    p.extension().is_some_and(|x| wave_audio_file::AUDIO_EXTENSIONS.contains(&lower(x).as_str()))
}

/// Folder names of a path, last first (lower case), without the file name.
fn folders_rev(p: &Path) -> Vec<String> {
    p.parent().map(|d| d.components().rev().map(|c| lower(c.as_os_str())).collect()).unwrap_or_default()
}

/// Splits a stored path on both separators: a project saved on Windows names its files
/// with `\`, which a Unix `Path` would take as part of one name.
fn portable(path: &str) -> PathBuf {
    path.split(['/', '\\']).filter(|c| !c.is_empty()).collect()
}

/// Looks for the files of `wanted` (source id, old path) under `root`.
pub fn search(root: &Path, wanted: &[(u64, String)], limits: Limits) -> Found {
    struct Want {
        id: u64,
        folders: Vec<String>,
    }
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
    let mut by_stem: HashMap<String, Vec<usize>> = HashMap::new();
    let mut wants = vec![];
    for (id, old) in wanted {
        let p = portable(old);
        let (Some(name), Some(stem)) = (p.file_name(), p.file_stem()) else { continue };
        let k = wants.len();
        wants.push(Want { id: *id, folders: folders_rev(&p) });
        by_name.entry(lower(name)).or_default().push(k);
        by_stem.entry(lower(stem)).or_default().push(k);
    }
    let mut found = Found::default();
    if wants.is_empty() {
        return found;
    }
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    'walk: while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut subdirs = vec![];
        for e in rd.flatten() {
            if found.scanned >= limits.entries {
                found.truncated = true;
                break 'walk;
            }
            found.scanned += 1;
            let name = e.file_name();
            // file_type does not follow links: a linked folder is never entered (no cycles)
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if name.to_string_lossy().starts_with('.') {
                    continue;
                }
                if depth + 1 > limits.depth {
                    found.truncated = true;
                } else {
                    subdirs.push(e.path());
                }
                continue;
            }
            let path = e.path();
            if !is_audio(&path) {
                continue;
            }
            let exact = by_name.get(&lower(&name));
            let stem = path.file_stem().map(lower).unwrap_or_default();
            let (hits, exact_name) = match exact {
                Some(h) => (h, true),
                None => match by_stem.get(&stem) {
                    Some(h) => (h, false),
                    None => continue,
                },
            };
            let have = folders_rev(&path);
            for &k in hits {
                let w = &wants[k];
                let shared = w.folders.iter().zip(&have).take_while(|(a, b)| a == b).count();
                found.candidates.entry(w.id).or_default().push(Candidate {
                    path: path.display().to_string(),
                    exact_name,
                    shared_folders: shared,
                });
            }
        }
        // sorted so the walk (and which part a limit cuts off) does not depend on the OS order
        subdirs.sort();
        stack.extend(subdirs.into_iter().rev().map(|d| (d, depth + 1)));
    }
    for list in found.candidates.values_mut() {
        list.sort_by(|a, b| b.exact_name.cmp(&a.exact_name).then(b.shared_folders.cmp(&a.shared_folders)).then(a.path.cmp(&b.path)));
        list.truncate(limits.per_source);
    }
    found
}

/// The candidate to relink to without asking: the only best-ranked one with the old file name.
pub fn unambiguous(list: &[Candidate]) -> Option<&Candidate> {
    let first = list.first().filter(|c| c.exact_name)?;
    match list.get(1) {
        Some(second) if second.exact_name && second.shared_folders == first.shared_folders => None,
        _ => Some(first),
    }
}

/// The decoded file is the source's audio, as far as its length tells (the project keeps the
/// length it had; a different take of the same name is longer or shorter).
pub fn same_length(expected_seconds: f64, seconds: f64) -> bool {
    expected_seconds <= 0.0 || (seconds - expected_seconds).abs() <= (expected_seconds * 0.005).max(0.02)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    #[test]
    fn finds_by_name_ignoring_case_and_ranks_by_shared_folders() {
        let d = tempfile::tempdir().unwrap();
        touch(&d.path().join("a/Drums/KICK.wav"));
        touch(&d.path().join("b/Other/kick.wav"));
        touch(&d.path().join("c/kick.flac"));
        touch(&d.path().join("c/kick.txt"));
        let f = search(d.path(), &[(7, r"C:\Old\Drums\kick.wav".into())], Limits::default());
        let c = &f.candidates[&7];
        assert_eq!(c.len(), 3);
        assert!(c[0].path.ends_with("KICK.wav") && c[0].exact_name && c[0].shared_folders == 1);
        assert!(c[1].path.ends_with("kick.wav") && c[1].shared_folders == 0);
        assert!(!c[2].exact_name && c[2].path.ends_with("kick.flac"));
        assert_eq!(unambiguous(c).map(|c| c.path.as_str()), Some(c[0].path.as_str()));
        assert!(!f.truncated);
    }

    #[test]
    fn two_equally_good_files_are_ambiguous() {
        let d = tempfile::tempdir().unwrap();
        touch(&d.path().join("x/snare.wav"));
        touch(&d.path().join("y/snare.wav"));
        let f = search(d.path(), &[(1, "/old/place/snare.wav".into())], Limits::default());
        assert_eq!(f.candidates[&1].len(), 2);
        assert!(unambiguous(&f.candidates[&1]).is_none());
    }

    #[test]
    fn only_a_converted_file_is_never_taken_without_asking() {
        let d = tempfile::tempdir().unwrap();
        touch(&d.path().join("pad.mp3"));
        let f = search(d.path(), &[(1, "/old/pad.wav".into())], Limits::default());
        assert!(unambiguous(&f.candidates[&1]).is_none());
    }

    #[test]
    fn hidden_folders_are_skipped_and_limits_are_reported() {
        let d = tempfile::tempdir().unwrap();
        touch(&d.path().join(".git/hat.wav"));
        touch(&d.path().join("deep/1/2/3/hat.wav"));
        let f = search(d.path(), &[(1, "/old/hat.wav".into())], Limits { depth: 2, ..Limits::default() });
        assert!(!f.candidates.contains_key(&1));
        assert!(f.truncated);
        let few = search(d.path(), &[(1, "/old/hat.wav".into())], Limits { entries: 1, ..Limits::default() });
        assert!(few.truncated);
        let all = search(d.path(), &[(1, "/old/hat.wav".into())], Limits::default());
        assert_eq!(all.candidates[&1].len(), 1);
    }

    #[test]
    fn length_check_tolerates_resampling_rounding_only() {
        assert!(same_length(10.0, 10.03));
        assert!(!same_length(10.0, 10.2));
        assert!(same_length(0.1, 0.115));
        assert!(same_length(0.0, 3.0));
    }
}
