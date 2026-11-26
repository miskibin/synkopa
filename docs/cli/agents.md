# Working with coding agents

A coding agent (any assistant that can run terminal commands) can use `synkopa` to read your song, make
edits, render them and check the result, while you keep working in the app and keep the last word through
undo.

## Quick start

1. Start Synkopa once, so the `synkopa` launcher is on your PATH.
2. Start your coding agent in any folder, in a new terminal.
3. Tell it what you want and to begin with `synkopa help` and `synkopa context`.

That is enough: `synkopa help` explains the commands, the output format and the units, and `context`
gives the agent every id it needs.

## Point an agent at Synkopa

Why: a short brief keeps the agent on the safe path.

A brief that works well:

```text
Use the `synkopa` command line to edit the song open in Synkopa.
Run `synkopa help` first, then `synkopa context`.
Use ids from `context`; count new ids up from context.nextId.
Send related edits as one `synkopa apply` batch (see `synkopa schema`).
Pass --if-revision with the revision you last read.
After each change, render the part you changed with `synkopa render` and check the stats.
```

On Windows, agents usually run Git Bash; the app installs a `synkopa` script that works there too.

## A safe edit loop

Why: the agent should see the musician's changes, never silently overwrite them, and prove its own work.

```sh
synkopa context                                     # 1. read: ids and the current revision
synkopa schema                                      #    the ops `apply` accepts
synkopa catalog                                     #    effect parameters and presets
synkopa apply --file edit.json --if-revision 57 --label "Busier hats"   # 2. one batch, one undo step
synkopa render --out check.wav --from 0 --to 16     # 3. listen without speakers
synkopa undo                                        # 4. if it is worse
```

- **Read first.** `context` is the summary; `context --full` is the whole project JSON.
- **One `apply` batch per idea.** It is atomic and is one undo step, so the musician can take back a whole
  idea with one `Ctrl+Z`. `--label` names the step in **Edit → Undo**.
- **Guard with the revision.** With `--if-revision N`, the edit is refused with `REVISION_CONFLICT` if the
  musician changed something since the read. Read again and decide; do not blindly retry.
- **Check the render.** The reply has `peakDbfs`, `rmsDbfs`, `crestDb`, `clippedSamples` and `silent`. Then
  listen to the WAV, or have it checked. See [Listening](listening.md).
- **Generate variants.** Note generators are deterministic in `--seed`: render a few seeds and keep the
  best (see [Patterns and notes](patterns-and-notes.md#generate-and-transform-notes)).
- **Exit codes** tell the agent what happened: 1 is a usage mistake (fix the command), 2 means the app is
  not running, 3 is a refusal with a `CODE: message` on stderr.

Agents can also work headless on a copy of the project with `-p` (see
[Project files without the app](project-files.md)); `--dry-run` and `--save-as` replace undo there.

## Correct a vocal against a reference

Why: an agent can fix the notes that are really off while leaving deliberate variations, slides and
vibrato alone.

The setup: your recording is clip 412; the original singer (another key, range and phrasing) is source 7;
the instrumental is clip 300.

1. **Measure.**

   ```sh
   synkopa analyze --clip 300                                # key and tuning offset of the backing
   synkopa pitch inspect --clip 412 > take.json              # what you sang: kinds, cents, tuning, range
   synkopa pitch compare --clip 412 --source 7 > cmp.json    # related? interval? which notes match?
   synkopa pitch suggest --clip 412 --source 7 --key A --scale minor --max-cents 60 > plan.json
   ```

2. **Decide, note by note.** Accept the plan's edits that make sense, drop the ones that touch a
   deliberate variation, and add your own (`--target`, a split before a scoop you want to keep). Then
   apply them as one edit:

   ```sh
   jq 'del(.op.edit.notes[] | select(.i == 21))' plan.json > edit.json   # e.g. drop note 21's change
   synkopa pitch edit --clip 412 --file edit.json --if-revision "$(jq .revision plan.json)" --max-cents 80
   ```

3. **Check.**

   ```sh
   synkopa pitch render --clip 412 --out "$PWD/after.wav"
   synkopa pitch render --clip 412 --out "$PWD/before.wav" --original
   synkopa pitch inspect --clip 412                          # `out` per note: did it land?
   synkopa pitch edit --clip 412 --reset 7                   # put back a note that sounds worse; or `synkopa undo`
   ```

**What the defaults protect.** A different note, an ornament the reference has and you do not (or the
other way round), a slide or vibrato, and a reference that is not the same song are left as sung. Timing is
never changed. `--max-cents` raises the limit when you know the take is far off; a low-level
`pitch edit` always lets you override the plan.

The commands and every reply field are described in
[Recording and vocals](recording-and-vocals.md#edit-pitch-note-by-note).

## Technical notes

- The CLI and the app share one implementation of every action, so an agent's edit behaves exactly like
  the same edit made with the mouse.
- Numbers in replies are rounded to four decimal places, which keeps values like 0.7937 (0 dB) and triplet
  positions (28.3333) exact enough to send back.
- The pitch correction workflow is measured on real recordings by `crates/tune-bench` (see
  [Recording and vocals](recording-and-vocals.md#benchmarks)).

## See also

- [The command line](../cli.md)
- [Scripting and live coding](scripting.md): let an agent write a `watch` file
- [Reading and editing](reading-and-editing.md)
