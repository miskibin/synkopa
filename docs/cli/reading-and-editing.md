# Reading and editing a project

These are the commands you use first: read what is in the song, then change it, either with a ready-made
command (`bpm`, `markers` …) or with `apply`, which sends a batch of edits as one undo step.

## Quick start

```sh
synkopa context --pretty                 # the map of the song, with every id
synkopa schema                           # every edit `apply` accepts, with an example
synkopa bpm 140                          # one edit = one undo step
synkopa apply --file edit.json --label "Busier hats"   # several edits, one undo step, all or nothing
synkopa undo
```

## Read the project

Why: every other command needs ids, and the reply tells you what is there before you change it.

| Command | What you get |
| --- | --- |
| `synkopa status` | Transport, `revision`, file path, unsaved changes, audio device, CLI launcher path; `mode` is `app` or `file` |
| `synkopa context` | Summary: tracks (index, insert, volume, clip count), inserts with effects and parameters, patterns, channels, sources, markers, notebook pages, `nextId` |
| `synkopa context --full` | The whole project JSON: every clip, note and parameter |
| `synkopa channels` | The Channel Rack (see [Patterns and notes](patterns-and-notes.md)) |
| `synkopa pattern [--id N]` | A pattern as step strings (see [Patterns and notes](patterns-and-notes.md)) |
| `synkopa markers` | Playlist markers in song order |
| `synkopa catalog` | Effect types with parameter ids, ranges, units and presets (the synth under `effects.synth`), Synkopa Instruments (`keys`), the command list |
| `synkopa schema` | Every op `apply` accepts, with units and an example |

## Edit with one command

Why: the common edits have their own command, so you do not need to write JSON.

```sh
synkopa bpm 140
synkopa key A minor                       # the project key, used by scale-aware note tools
synkopa key --off
```

`key` takes a note (`C` … `B`, sharps as `C#`) and a scale: `chromatic`, `major` (the default), `minor`,
`"major pentatonic"` or `"minor pentatonic"` (quote the two-word ones).

Other single-purpose commands live on their topic pages: patterns and channels, mixer effects and
routing, takes, pitch, markers below.

## Edit many things at once with `apply`

Why: related edits should land together. One `apply` batch is atomic (if one op is wrong, none is
applied) and is one undo step.

```sh
synkopa apply --file edit.json --label "Busier hats"
synkopa apply --json '[{"op":"setLoop","loop":{"a":0,"b":16}}]'
```

`edit.json` is an array of ops (`synkopa schema` lists them all):

```json
[
  {"op": "putPattern", "pattern": {"id": 2, "name": "Hats", "color": "#e0a44a", "len": 4, "swing": 0.4,
    "notes": [{"ch": 1002, "step": 0, "len": 1}, {"ch": 1002, "step": 2, "len": 1, "v": 0.6}]}},
  {"op": "setLoop", "loop": {"a": 0, "b": 16}}
]
```

- Every note names its channel with `ch` (ids from `synkopa channels`).
- New ids: count up from `context.nextId`.
- `--label` names the undo step in **Edit → Undo**.
- `apply` also accepts a single op object, or an object with an `ops` array.

### Units

| Value | Unit |
| --- | --- |
| Playlist time (`start`, `len`, loop, markers) | Beats (4 = one bar in 4/4) |
| Pattern note `step` / `len` | 16th notes |
| Fades | Seconds |
| Track and insert `vol` | Fader position (0.7937 = 0 dB) |
| Clip `gain` | dB |
| Audio clip `pitch` | Semitones, ±12 (cents as the fraction; the length stays) |
| Audio clip `stretch` | 0.25–4 |
| Pan | −1..1 |

## Do not overwrite the musician's changes

Why: while a script or an agent works, the musician may change something in the app.

Add `--if-revision N` to any edit, with the `revision` from your last read. If the project changed
meanwhile, the edit is refused with `REVISION_CONFLICT`. Read again and decide; do not blindly retry.

```sh
synkopa bpm 128 --if-revision 57
```

## Mark song sections

Why: markers name the parts of the song (Intro, Hook, Drop) so you and a script can find and loop them.
They work like FL Studio's time markers: labels only, they never change what plays.

A marker's *section* runs to the next marker (the last one: to the song end). Name a marker by its id or
its name (case does not matter). Each edit is one undo step.

```sh
synkopa markers                                   # [{id, name, at, bar}] in song order
synkopa markers --add Intro --bar 1               # --bar N (1-based) or --at B (beats); default: the start position
synkopa markers --add Hook --at 32
synkopa markers --marker Hook --name "Hook 1"     # rename
synkopa markers --marker "Hook 1" --bar 17        # move (rename and move at once: one step)
synkopa markers --loop "Hook 1"                   # set the loop to that section
synkopa markers --remove Intro
```

`bar` in the reply is 1-based and can be fractional: 9.5 is the middle of bar 9.

## Write lyrics into the notebook

Why: the notebook keeps lyrics next to the bars they are sung on.

`synkopa context` lists the notebook pages (`id`, `name`, `startBar`, `lines`); `context --full` includes
their text. Each line of a page is one bar, counted from `startBar` (1-based); an empty line is a bar of
rest, and a line in `[…]` is a header that takes no bar. One op replaces every page (one undo step):

```sh
synkopa apply --json '[{"op":"putNotebook","notebook":{"pages":[{"id":3000,"name":"Chorus","startBar":17,"text":"[Chorus]\nthis is our time\nour night"}]}}]'
```

## Bring in audio and MIDI

Why: start from files you already have.

```sh
synkopa import kick.wav loop.wav                  # add as project sources
synkopa import kick.wav loop.wav --track 3 --beat 16   # and place them on track 3 at beat 16
```

Standard MIDI files (format 0 and 1):

```sh
synkopa midi inspect song.mid                             # parts, channels, instruments, what each becomes
synkopa midi import song.mid --parts 0,2 --tempo --beat 16   # a new pattern on the Playlist (one undo step)
synkopa midi import lead.mid --parts 1 --pattern 3 --ch 1004 --replace   # one part into an existing channel
synkopa midi export --out song.mid                        # the song (pattern clips as placed), with tempo
synkopa midi export --out beat.mid --pattern 3            # one pattern
```

- `midi import` makes one Channel Rack channel per part; General MIDI drums become standard-library
  samplers, one per drum. `--tempo` takes the file's BPM; `--track N` picks the Playlist track.
- With `--pattern P --ch C` one part's notes go into channel C of pattern P (`--ch` is a channel id here).
  `--replace` replaces the notes already there.

## Technical notes

- `synkopa call COMMAND --args JSON` sends a raw request by its internal name (the command list is in
  `synkopa catalog`). Use the named commands where they exist.
- Ops in `apply` for the topics above: `setBpm`, `setKey`, `setLoop`, `putPattern`, `putChannel`,
  `removeChannel`, `putMarker`, `removeMarker`, `putNotebook`, `putAutomation`, `putClips` and more; the
  authoritative list with fields is `synkopa schema`.
- Marker ops: `putMarker {marker: {id, at, name}}` adds, moves or renames by id (`at` 0..2048 beats, names
  up to 80 characters, at most 256 markers, kept sorted); `removeMarker {id}`.

## See also

- [Patterns and notes](patterns-and-notes.md)
- [Mixing](mixing.md)
- [Project files without the app](project-files.md)
