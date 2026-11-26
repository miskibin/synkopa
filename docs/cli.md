# The `synkopa` command line

`synkopa` is a terminal command that reads and edits a Synkopa project: the song open in the app, or a
`.synkopa` file on disk with no app running.

Reach for it when you want to:

- **Script repetitive edits.** Retune every hat pattern, rename markers, bounce ten variations of a beat.
- **Let a coding agent work on your song.** An agent reads the project as JSON, makes an edit, renders it
  and checks the levels, and every edit lands in your undo history so `Ctrl+Z` takes it back.
- **Edit project files headless.** Change the tempo, render stems or package a project on a machine where
  the app is not open (a build server, a batch job).

You never need it to use Synkopa. Everything it does is also in the app.

## Get it

There is nothing to install. Start the Synkopa app once: it puts a `synkopa` launcher on your PATH.
Then **open a new terminal** (terminals that were already open do not see it yet) and run:

```sh
synkopa status
```

Where the launcher goes:

| System | Launcher |
| --- | --- |
| Windows | `%LOCALAPPDATA%\Microsoft\WindowsApps\synkopa.cmd`, plus an extensionless `synkopa` script for Git Bash / MSYS (the shell coding agents use) |
| macOS, Linux | `~/.local/bin/synkopa` |

**Settings → Audio → Command line** shows where it went, or why it could not be written. The launcher runs
the app binary in CLI mode, so it is always the same version as the app. The old command name `wave` still
works as an alias.

## Five minutes with the CLI

Start Synkopa and choose **File → New project**. The new project has one empty pattern (id 1) and four
drum channels: Kick, Clap, Hat and Snare.

1. **Check that the CLI reaches the app.**

   ```sh
   synkopa status
   ```

   You get the transport state, the project's `revision` (a counter that goes up with every edit), the
   file path, the audio device and where the CLI launcher is. If it says Synkopa is not running, start the
   app (exit code 2).

2. **Read the project.**

   ```sh
   synkopa context --pretty
   ```

   This is the map of the song: tracks, mixer inserts with their effects, patterns, channels, audio
   sources, markers and `nextId`. Every id you pass to other commands comes from here.

3. **Write a beat.** One character per 16th note: `x` is a hit at full velocity, `1`–`9` are 10–90 %
   velocity, `.` is a rest. Spaces are only for reading.

   ```sh
   synkopa pattern --id 1 --ch kick --steps "x... x... x... x..."
   synkopa pattern --id 1 --ch hat  --steps "..x. ..x. ..x. ..x5"
   ```

   The pattern changes in the Channel Rack as you run each line. `--ch` takes a channel id or its name
   (case does not matter).

4. **Listen without speakers.** Render the pattern offline and read the level statistics.

   ```sh
   synkopa render --pattern 1 --out beat.wav
   ```

   The reply has a `stats` object: `peakDbfs`, `rmsDbfs`, `crestDb`, `clippedSamples` and `silent`. A peak
   near 0 dBFS or any clipped samples means the beat is too hot; `silent: true` means nothing played.
   Then open `beat.wav` to hear it.

5. **Take it back.**

   ```sh
   synkopa undo
   ```

   Each command was one undo step, so this removes the hats. `Ctrl+Z` in the app does the same thing.

## How it works

**Two targets, the same commands.**

- **The running app** (the default). The CLI talks to the open Synkopa over a local socket. You see every
  edit at once, and it joins the app's undo history.
- **A project file** (`-p song.synkopa`, long form `--project`). No app, window or audio device: the file is
  loaded, the command runs, and the file is saved back if the command changed it. Commands that need
  sound or undo (`play`, `undo` …) are refused. See [Project files without the app](cli/project-files.md).

**Every edit is one undo step.** A command that changes the project adds one step to **Edit → Undo**
(a few, like `auto-vocal`, say when they add two). A batch of ops sent with `apply` is one step too, and is
all-or-nothing.

**Ids come from `context`.** Channels, patterns, clips, tracks, mixer inserts, effects and audio sources
all have numeric ids. Read them from `synkopa context` (or `channels`, `markers`, `takes` …) instead of
guessing. When you create something with `apply`, count new ids up from `context.nextId`.

**Output is JSON.** stdout carries one JSON value (add `--pretty` to indent it), with numbers rounded to at
most four decimal places (0.47, not 0.4699999988). Messages and warnings go to stderr. The exit code
tells a script what happened:

| Exit code | Meaning |
| --- | --- |
| `0` | OK |
| `1` | Bad usage (unknown command, missing option) |
| `2` | Synkopa is not running or cannot be reached |
| `3` | Refused, printed as `CODE: message`, for example `BAD_OPS`, `REVISION_CONFLICT`, `NEEDS_APP`, `FILE_OPEN` |

`synkopa help` prints the full command list.

## Command groups

| Page | What it covers |
| --- | --- |
| [Project files without the app](cli/project-files.md) | `-p FILE`, `new`, `--dry-run`, `--save-as`, sharing a project as a ZIP |
| [Reading and editing](cli/reading-and-editing.md) | `status`, `context`, `catalog`, `schema`, `apply` and ops, units, `--if-revision`, tempo and key, markers, lyrics, importing audio and MIDI |
| [Patterns and notes](cli/patterns-and-notes.md) | Channel Rack channels and instruments, step strings, piano-roll notes, slides, note generators and chord tools |
| [Audio clips](cli/audio-clips.md) | Tempo and key detection, chord detection, stem extraction, make unique, export in place, missing files |
| [Recording and vocals](cli/recording-and-vocals.md) | Takes and comping, Tune to Reference, pitch editing, Auto vocal |
| [Mixing](cli/mixing.md) | Effects, routing, sidechain, the mix assistant, FX chain presets, VST3/LV2 plugins, automation |
| [Listening: render and stems](cli/listening.md) | Offline bounces, WAV/MP3 options, stem export, level statistics |
| [Scripting and live coding](cli/scripting.md) | Script files, `watch` (the file is the song), several apps open, raw `call` |
| [Working with coding agents](cli/agents.md) | Pointing an agent at Synkopa, a safe edit loop, the vocal correction workflow |

## See also

- [Architecture](dev/architecture.md): how the CLI, the app and the engine share one implementation of
  every action.
