//! Per-user settings that are not part of a song: audio device and library folders.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use wave_audio_io::AudioConfig;
use wave_midi_in::MidiDevice;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub audio: AudioConfig,
    /// The MIDI keyboard to reopen at startup (`None` = no device).
    pub midi_device: Option<MidiDevice>,
    /// The MIDI keyboard plays every key at this velocity (`None` = the keyboard's own).
    pub midi_fixed_velocity: Option<u8>,
    pub folders: Vec<String>,
    pub last_project: Option<String>,
    /// Projects opened or saved lately, newest first, at most [`RECENT_MAX`] (File → Open recent).
    pub recent: Vec<String>,
    /// The app version whose release notes were last shown (`None` = fresh install).
    pub seen_version: Option<String>,
    /// True when the user turned the automatic update check off (the default is on).
    pub no_auto_update: bool,
    /// Downloads skip the certificate check (a company proxy whose root is not installed).
    pub ignore_cert_errors: bool,
    /// The toolbar's main volume (FL Studio's): a fader position, `None` = 0 dB. How loud the
    /// app plays, not part of any project.
    pub program_volume: Option<f32>,
}

/// How many projects File → Open recent keeps.
pub const RECENT_MAX: usize = 10;

/// Two paths name the same file: on Windows case and slash direction do not matter.
fn same_path(a: &str, b: &str) -> bool {
    let norm = |p: &str| {
        let p = p.replace('\\', "/");
        if cfg!(windows) {
            p.to_lowercase()
        } else {
            p
        }
    };
    norm(a) == norm(b)
}

fn path() -> PathBuf {
    wave_engine::engine::scratch_dir().parent().unwrap().join("settings.json")
}

impl Settings {
    /// A project was opened or saved at `path`: it is the last project and the newest recent one.
    pub fn remember(&mut self, path: &str) {
        self.last_project = Some(path.to_owned());
        self.recent.retain(|p| !same_path(p, path));
        self.recent.insert(0, path.to_owned());
        self.recent.truncate(RECENT_MAX);
    }
    /// Takes `path` off the recent list, or every entry with `None`.
    pub fn forget(&mut self, path: Option<&str>) {
        match path {
            Some(p) => self.recent.retain(|q| !same_path(q, p)),
            None => self.recent.clear(),
        }
    }
    pub fn load() -> Self {
        let Ok(t) = std::fs::read_to_string(path()) else { return Self::default() }; // no settings yet
        serde_json::from_str(&t).unwrap_or_else(|e| {
            log::warn!("settings.json did not parse ({e}); starting with defaults");
            Self::default()
        })
    }
    pub fn save(&self) {
        let p = path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let Ok(t) = serde_json::to_string_pretty(self) else {
            log::warn!("could not serialize the settings");
            return;
        };
        // write beside the file and rename: a crash mid-write must not lose the settings
        let tmp = p.with_extension("json.tmp");
        if let Err(e) = std::fs::write(&tmp, t) {
            log::warn!("could not write the settings: {e}");
            return;
        }
        if let Err(e) = std::fs::rename(&tmp, &p) {
            log::warn!("could not replace the settings: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_are_checked_by_default_and_old_files_still_load() {
        let s: Settings = serde_json::from_str(r#"{"folders":["a"],"lastProject":"x.wave"}"#).unwrap();
        assert!(!s.no_auto_update);
        assert_eq!(s.seen_version, None);
        assert!(!s.ignore_cert_errors, "certificates are checked unless the user opts out");
        assert_eq!(s.folders, ["a"]);
    }

    #[test]
    fn recent_projects_move_to_the_top_without_doubles_and_stay_short() {
        let mut s = Settings::default();
        for i in 0..12 {
            s.remember(&format!("C:/songs/{i}.synkopa"));
        }
        assert_eq!(s.recent.len(), RECENT_MAX);
        assert_eq!(s.recent[0], "C:/songs/11.synkopa");
        assert_eq!(s.last_project.as_deref(), Some("C:/songs/11.synkopa"));
        // opened again (another spelling of the same file on Windows): one entry, at the top
        let again = if cfg!(windows) { "c:\\SONGS\\5.synkopa" } else { "C:/songs/5.synkopa" };
        s.remember(again);
        assert_eq!(s.recent[0], again);
        assert_eq!(s.recent.iter().filter(|p| p.ends_with("5.synkopa") || p.ends_with("5.SYNKOPA")).count(), 1);
        s.forget(Some("C:/songs/11.synkopa"));
        assert!(!s.recent.iter().any(|p| p.ends_with("11.synkopa")));
        s.forget(None);
        assert!(s.recent.is_empty());
        // files from before the list load with an empty one
        let old: Settings = serde_json::from_str(r#"{"lastProject":"x.wave"}"#).unwrap();
        assert!(old.recent.is_empty());
    }

    #[test]
    fn update_fields_round_trip_in_camel_case() {
        let s = Settings { seen_version: Some("0.9.3".into()), no_auto_update: true, ..Default::default() };
        let t = serde_json::to_string(&s).unwrap();
        assert!(t.contains("\"seenVersion\":\"0.9.3\"") && t.contains("\"noAutoUpdate\":true"), "{t}");
        let back: Settings = serde_json::from_str(&t).unwrap();
        assert_eq!(back.seen_version.as_deref(), Some("0.9.3"));
        assert!(back.no_auto_update);
    }
}
