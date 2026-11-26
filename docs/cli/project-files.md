# Project files without the app

With `-p FILE` every `synkopa` command works on a `.synkopa` file directly: no app, no window and no
audio device. Use it for batch jobs, build servers, or to try an edit without touching the song you have
open.

## Quick start

```sh
synkopa new song.synkopa --template demo                   # create a project file
synkopa -p song.synkopa context                            # read it: nothing is written
synkopa -p song.synkopa pattern --id 1 --ch kick --steps "x... x... x... x..."   # edit: saved back
synkopa -p song.synkopa render --out song.wav              # listen
```

The file is loaded, the command runs, and the file is saved back only if the command changed it.
`-p` is short for `--project`. Projects saved as `.wave` by earlier versions still open.

## Create a project file

```sh
synkopa new song.synkopa                          # the empty template (the default)
synkopa new song.synkopa --template sketch        # a short sketch: beat, hats and a bass line
synkopa new song.synkopa --template demo          # the full demo song
synkopa new song.synkopa --bpm 140                # set the tempo at creation
synkopa new song.synkopa --force                  # replace an existing file
```

A name without an extension gets `.synkopa`. `new` refuses to replace a file that a running Synkopa has
open, even with `--force`.

## Try an edit without keeping it

```sh
synkopa -p song.synkopa bpm 150 --save-as song-150.synkopa   # song.synkopa stays as it was
synkopa -p song.synkopa apply --file ops.json --dry-run      # nothing is saved
synkopa -p song.synkopa export-project --out song.zip        # package it without saving changes to song.synkopa
```

With `--dry-run` the reply has `dryRun.changed` (did the command change the project) and
`dryRun.wouldSave` (the path it would have written).

## Render one pattern or the whole song

```sh
synkopa -p song.synkopa render --out song.wav               # the song
synkopa -p song.synkopa render --pattern 1 --out beat.wav   # pattern 1 alone, as PAT mode plays it
synkopa -p song.synkopa render --stems --out stems/         # one file per mixer insert
```

See [Listening: render and stems](listening.md) for every render option.

## Share an editable project

`export-project` packages a project and its audio sources into a ZIP that someone else can open.

```sh
synkopa export-project --out song.zip                 # the project open in the app, unsaved edits included
synkopa -p song.synkopa export-project --out song.zip # a saved file
```

Unzip the archive and open the `.synkopa` project with the `Audio` folder next to it. Plugin settings
travel in the project; the plugins themselves must be installed separately, at the locations stored in the
project. If an audio source is missing, the export fails and an existing ZIP at `--out` is left as it was.
`export-project` always writes its ZIP, so it refuses `--dry-run`.

## What works differently on a file

- **Sound and undo are refused.** `play`, `stop`, `pattern --play`, `comp --audition`, `undo`, `redo` and
  `open` answer `NEEDS_APP`: they need the audio device or the app's undo history, which a file does not
  have. Listen by rendering. Try edits with `--dry-run` or `--save-as`, and keep the file under version
  control if you want history.
- **A file the app has open is protected.** An edit to a file that a running Synkopa has open is refused
  with `FILE_OPEN`, because the app keeps its own copy and would overwrite your edit on its next save. Edit
  it in the app (drop `-p`), close it there first, or add `--force`. Reading and rendering it are fine.
- **Saving works as in the app.** The previous file is kept as a `(backup)` copy, and recordings are moved
  next to the project. `save` saves even an unchanged file; `save --path FILE` is the same as `--save-as`.
- **Missing audio is a warning, not a failure.** Audio that could not be loaded is printed as a warning on
  stderr and listed under `loadWarnings`; the command still runs and the missing sources play silence, as
  in the app.
- **`--session` and `-p` do not mix.** `--session` picks a running app; `-p` works on a file.

## What is the same

- **Renders and stems** are the same bounce as the app's export. The Synkopa Instruments recordings are
  loaded, or the command fails with `KEYS_MISSING` instead of rendering silence. Pitch-edited clips use the
  installed RMVPE model, as the app does.
- **Plugins** load too: VST3 effects from the scan cache, LV2 effects (Linux) from the installed bundles.
  `plugin-params` and `plugin-set` work, and the plugin's state is saved in the file.
- **Every reply** has `saved`: the path that was written, or `null`.

## Technical notes

- Only the `synkopa` launcher the app installs can open files (it runs the app binary with `--cli`). A
  standalone CLI binary built on its own only talks to a running app and says so.
- `--project`, `--save-as`, `--dry-run` and `--force` apply to the whole call. In a script file they go
  before `script`, not on its lines (see [Scripting](scripting.md)).
- Error codes from file mode: `OPEN_FAILED` (the file could not be read as a project), `SAVE_FAILED`,
  `EXISTS` (`new` without `--force`), `FILE_OPEN`, `NEEDS_APP`.

## See also

- [The command line](../cli.md)
- [Scripting and live coding](scripting.md): many commands, one load and one save
- [Listening: render and stems](listening.md)
