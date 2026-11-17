//! Makes `synkopa` a command in every new terminal, with no installer step.
//!
//! The app binary doubles as the CLI (`wave-app --cli …`). On start we drop a
//! tiny launcher into a folder that is already on the user's PATH:
//! * Windows: `%LOCALAPPDATA%\Microsoft\WindowsApps\synkopa.cmd` (on PATH by default
//!   since Windows 10, no admin rights, survives app updates because we rewrite it);
//! * macOS/Linux: `~/.local/bin/synkopa` (a `sh` script).
//!
//! The command was called `wave` before the rename, so a second launcher with that name is
//! written next to it: scripts and agents that call `wave` keep working.

use std::path::PathBuf;

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliStatus {
    pub installed: bool,
    pub command: String,
    pub path: Option<String>,
    pub note: Option<String>,
}

fn target_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("WAVE_CLI_DIR") {
        return Some(PathBuf::from(d));
    }
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Microsoft").join("WindowsApps"))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("bin"))
    }
}

fn on_path(dir: &std::path::Path) -> bool {
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|x| x == dir)).unwrap_or(false)
}

fn mark_executable(_file: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(_file, std::fs::Permissions::from_mode(0o755));
    }
}

/// The old `wave` name: best effort, the status reports only the `synkopa` launcher.
fn write_launcher(file: &std::path::Path, body: &str) {
    if std::fs::read_to_string(file).is_ok_and(|t| t == body) {
        return;
    }
    if std::fs::write(file, body).is_ok() {
        mark_executable(file);
    }
}

/// Writes (or refreshes) the launcher. Never fails the app: problems become a note.
pub fn install() -> CliStatus {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            return CliStatus { installed: false, command: "synkopa".into(), path: None, note: Some(format!("cannot locate the app: {e}")) }
        }
    };
    let Some(dir) = target_dir() else {
        return CliStatus { installed: false, command: "synkopa".into(), path: None, note: Some("no home folder to install into".into()) };
    };
    #[cfg(windows)]
    let (file, alias, body) = (dir.join("synkopa.cmd"), dir.join("wave.cmd"), format!("@echo off\r\n\"{}\" --cli %*\r\n", exe.display()));
    #[cfg(not(windows))]
    let (file, alias, body) = (dir.join("synkopa"), dir.join("wave"), format!("#!/bin/sh\nexec \"{}\" --cli \"$@\"\n", exe.display()));
    let unchanged = std::fs::read_to_string(&file).map(|t| t == body).unwrap_or(false);
    if !unchanged {
        if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, &body)) {
            return CliStatus {
                installed: false,
                command: format!("\"{}\" --cli", exe.display()),
                path: None,
                note: Some(format!("could not write {}: {e}", file.display())),
            };
        }
        mark_executable(&file);
    }
    write_launcher(&alias, &body);
    let note = if on_path(&dir) {
        None
    } else {
        Some(format!("{} is not on PATH — add it, or call {} directly", dir.display(), file.display()))
    };
    CliStatus { installed: true, command: "synkopa".into(), path: Some(file.display().to_string()), note }
}
