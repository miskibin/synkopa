//! Audio files that changed on disk while the project is open (re-bounced, edited in another
//! app): which sources play them, and how a reload went. The watching itself is the app's
//! (`app/src/media_watch.rs`); the decode and the swap are [`crate::Engine::reload_source_file`].

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::model::{Id, Project};

/// How reloading one file went: the sources that play it now play what it holds.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceReload {
    /// The file, as the project names it.
    pub path: String,
    /// Every source that plays the file ("Make unique" copies share one).
    pub source_ids: Vec<Id>,
    /// The first source's name (what the musician knows the file as).
    pub name: String,
    /// The file's length now, seconds.
    pub seconds: f64,
    /// The length differs from the one the source had: clips keep their length, so a
    /// shorter file ends in silence and a longer one is cut where the clip ends.
    pub length_differs: bool,
    /// The source had no audio before (its file was missing when the project opened).
    pub was_missing: bool,
}

/// Whether a source's stored path names `file`. Library (`lib:`) paths never do. On Windows
/// and macOS file names are compared ignoring case and `/` vs `\`, as their file systems do.
pub fn same_file(stored: &str, file: &Path) -> bool {
    if stored.starts_with(crate::factory::SCHEME) {
        return false;
    }
    Path::new(stored) == file || file_key(Path::new(stored)) == file_key(file)
}

/// A file's path in a form that is equal for every spelling of the same file that
/// [`same_file`] accepts (a map key).
pub fn file_key(p: &Path) -> String {
    if cfg!(any(windows, target_os = "macos")) {
        let k = p.to_string_lossy().replace('\\', "/").to_lowercase();
        // a verbatim Windows path (`\\?\C:\…`, what `canonicalize` returns) names the same file
        match k.strip_prefix("//?/") {
            Some(rest) if !rest.starts_with("unc/") => rest.to_string(),
            _ => k,
        }
    } else {
        p.to_string_lossy().into_owned()
    }
}

/// The files a project's sources play (not the sample library's), each once, as the project
/// names them: what a file watcher watches.
pub fn source_files(p: &Project) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    p.sources
        .iter()
        .filter(|s| !s.path.starts_with(crate::factory::SCHEME))
        .map(|s| PathBuf::from(&s.path))
        .filter(|f| seen.insert(file_key(f)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_paths_are_never_a_file() {
        assert!(!same_file("lib:Standard/kick.flac", Path::new("lib:Standard/kick.flac")));
    }

    #[test]
    fn the_same_path_is_the_same_file() {
        assert!(same_file("/music/pad.wav", Path::new("/music/pad.wav")));
        assert!(!same_file("/music/pad.wav", Path::new("/music/pad2.wav")));
    }

    #[cfg(windows)]
    #[test]
    fn windows_ignores_case_and_slashes() {
        assert!(same_file(r"C:\Music\Pad.WAV", Path::new("c:/music/pad.wav")));
        assert!(same_file(r"\\?\C:\Music\pad.wav", Path::new(r"C:\Music\pad.wav")));
        assert_ne!(file_key(Path::new(r"\\?\UNC\server\share\a.wav")), file_key(Path::new(r"C:\a.wav")));
    }
}
