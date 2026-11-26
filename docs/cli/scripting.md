# Scripting and live coding

Run many commands from one file, or keep a text file open in your editor and let every save rewrite the
song while the loop plays.

## Quick start

```sh
synkopa -p song.synkopa script edits.txt      # many commands, one load and one save
synkopa watch song.txt                        # live coding: every save replaces the song in the app
```

## Run a script file

Why: on a project file, loading and saving happen once instead of once per command, and a failing line
leaves the file untouched.

```sh
synkopa -p song.synkopa script edits.txt      # a file
synkopa -p song.synkopa script -              # read the lines from stdin
synkopa script edits.txt                      # against the running app
```

```text
# edits.txt: one command per line, quoted like a shell ('…' or "…", no escapes)
bpm 128
pattern --id 1 --ch hat --steps "x.x. x.x. x.x5 x.x9"
apply --json '[{"op":"setLoop","loop":{"a":0,"b":16}}]'
render --out preview.wav
```

- **On a file** (`-p`) the project is loaded once and saved once, at the end. The first line that fails
  stops the script before anything is saved, and the error names the failing line and command.
- **Against the running app** the lines run one by one: lines before a failing one stay applied, each as
  its own undo step.
- Renders and ZIP exports write their output when their line runs.
- The reply lists each line's result: `{"results": [{"line", "command", "result"}, …]}`.

**Line rules.**

- `#` starts a comment line; empty lines are skipped. A leading `synkopa` on a line is ignored, so you can
  paste commands as they are.
- Quotes group words: `'…'` or `"…"`, with no escape characters, so Windows paths keep their backslashes
  and JSON inside `'…'` keeps its double quotes.
- Options of the whole run go before `script`, not on its lines: `--project`, `--session`, `--save-as`,
  `--dry-run`, `--force`, `--pretty`. `--dry-run` also reaches every line.
- `script`, `watch`, `new` and `help` cannot run inside a script. On a file, `save --path` is refused in a
  script; use `--save-as` before `script`.
- `export-project` refuses `--dry-run`, so leave it off a script that packages a ZIP.
- Relative paths in a line are relative to the current folder.

## Live coding with `synkopa watch`

Why: write the song as text, like a buffer in Sonic Pi or Strudel, and hear each change while the loop
keeps playing.

```sh
synkopa watch song.txt                                       # the running app: every save replaces the song
synkopa -p song.synkopa watch song.txt --render preview.wav  # no app: every save bounces preview.wav
```

```text
# song.txt — the song, as `synkopa script` lines
bpm 128
pattern --id 1 --ch kick --steps "x... x... x... x..."
pattern --id 1 --ch hat  --steps "..x. ..x. ..x. ..x5"
add-fx --insert 3 --type reverb --preset 2
```

1. Start the loop in Synkopa (or run `synkopa play`).
2. Run `synkopa watch song.txt`.
3. Open the file in your editor (or let an agent write it) and save. The song changes while the loop plays.

**The file is the song.** Each save runs the whole file again on a copy of the project *as it was when the
watch started*, and the result replaces the open project. Delete a line and its effect is gone; a line
that adds something (an effect, a channel) adds it once, not once per save.

- **One undo step per save**, named "synkopa watch: song.txt". A save that changes nothing adds none. The
  transport keeps running, effects keep their tails, plugin instruments keep their sound.
- **A failing line changes nothing.** The open project stays as the last good save, and the error names the
  line. Transport and undo commands (`play`, `undo` …) do not belong in the file: they answer `NEEDS_APP`
  there.
- **Your edits in the app are not overwritten.** After you change the project in Synkopa (or with another
  `synkopa` command), the next save is refused with `REVISION_CONFLICT`. Restart `synkopa watch` to build on
  the project as it is now, or add `--force` to let the file win (what it replaces stays one undo away).
- **With `-p FILE`** the copy is the file's project and it is never saved. Bounce each run with `--render`,
  or save the result with `synkopa -p song.synkopa script song.txt`.
- **`--once`** runs the file once, prints the result and exits. `Ctrl+C` stops watching.

**Output.** One JSON line per save on stdout:

```text
{"event":"applied","file":"…/song.txt","lines":4,"revision":57,"changed":true}
{"event":"failed","file":"…/song.txt","error":"line 3 …"}
```

With `--render` the `applied` line also carries the `render` result.

## Several apps open

Why: with more than one Synkopa window running, the CLI needs to know which one you mean.

```sh
synkopa sessions                       # [{sessionId, pid, project}]
synkopa --session 3f2a context         # a prefix of the id is enough
```

Without `--session`, a command with several apps open is refused (exit code 2) and asks you to pick one.

## Technical notes

- `watch` checks the file every 150 ms and reads it once it has held still for a moment, so an editor that
  writes in several steps is read once. A file that does not exist yet is waited for.
- `watch` needs the `synkopa` launcher the app installs, as `-p` does.
- `synkopa call COMMAND --args JSON` sends a raw request by its internal name; the command list is in
  `synkopa catalog` under `commands`.
- `--pretty` indents the JSON output.

## See also

- [Project files without the app](project-files.md)
- [Working with coding agents](agents.md)
- [Patterns and notes](patterns-and-notes.md)
