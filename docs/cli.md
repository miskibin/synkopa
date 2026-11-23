# `synkopa` — the command line

The CLI works in two ways, with the same commands:

* **The running app** (the default): it edits the project you see, with the same undo history.
* **A project file** (`--project FILE`, or `-p`): no app, no window and no audio device. The
  file is loaded, the command runs, and the file is saved back if the command changed it.
  See [Project files without the app](#project-files-without-the-app).

The old `wave` command still works as an alias.

There is nothing to install: on start the app puts a `synkopa` launcher on your PATH
(`%LOCALAPPDATA%\Microsoft\WindowsApps\wave.cmd` on Windows, `~/.local/bin/wave` elsewhere).
Settings → CLI shows where it went. Open a new terminal after the first start and run:

```sh
synkopa status
synkopa context --pretty
```

stdout is only JSON (`--pretty` to indent), with numbers rounded to at most four decimal places
(0.47, not 0.4699999988); messages go to stderr. Exit codes:
`0` ok, `1` bad usage, `2` Synkopa not running / unreachable, `3` refused, as
`CODE: message` (`BAD_OPS`, `REVISION_CONFLICT`, `NEEDS_APP`, `FILE_OPEN` …).

## Project files without the app

Projects saved as `.wave` by earlier versions still open.

```sh
synkopa new song.synkopa --template demo                  # or --template empty (default) / sketch; --force replaces a file
synkopa -p song.synkopa context                           # any read: nothing is written
synkopa -p song.synkopa pattern --id 1 --ch kick --steps "x... x... x... x..."   # saved back to song.synkopa
synkopa -p song.synkopa render --out song.wav             # or --stems --out stems/
synkopa -p song.synkopa render --pattern 1 --out beat.wav # one pattern alone, as PAT mode plays it
synkopa -p song.synkopa export-project --out song.zip      # without saving changes to song.synkopa
synkopa -p song.synkopa bpm 150 --save-as song-150.synkopa   # song.synkopa stays as it was
synkopa -p song.synkopa apply --file ops.json --dry-run   # try it: the reply has dryRun.changed, nothing is saved
```

Every reply on a file has `saved`: the path written, or `null`. Audio that could not be loaded
is printed as a warning on stderr and listed under `loadWarnings`; the command still runs (the
missing sources play silence, as in the app).

- **What is refused.** `play`, `stop`, `pattern --play`, `comp --audition`, `undo`, `redo` and
  `open` answer `NEEDS_APP`: they need the audio device or the app's undo history, which a file
  does not have. Listen by rendering; try edits with `--dry-run` or `--save-as`.
- **A file the app has open.** An edit to a file that a running Synkopa has open is refused with
  `FILE_OPEN`: the app keeps its own copy and would overwrite the edit on its next save. Edit it
  in the app (drop `--project`), close it there, or pass `--force`. Reading and rendering it are
  fine.
- **Saving.** The save works as it does in the app: a `(backup)` copy of the previous file, and
  recordings moved next to the project. `save` saves even an unchanged file; `save --path` is
  the same as `--save-as`.
- **What is the same.** Renders and stems are the same bounce as the app's export (the Synkopa Keys
  piano is loaded, or the command fails with `KEYS_MISSING` instead of rendering silence;
  pitch-edited clips use the installed RMVPE model as the app does). VST3 effects load from the
  scan cache, LV2 effects (Linux) from the installed bundles. `plugin-params` and `plugin-set`
  work, and the plugin's state is saved in the file.

### Share an editable project

Run `synkopa export-project --out song.zip` to package the current project in the running app,
including unsaved edits. Use `-p song.synkopa` to package a saved file. Unzip the archive and open
the `.synkopa` project with its adjacent `Audio` folder. Plugin settings travel in the project.
Install the plugins separately at the locations stored in the project. A missing audio source
fails the export and any existing output ZIP is preserved.

### Scripts: many commands, one load

```sh
synkopa -p song.synkopa script edits.txt      # or `script -` to read stdin
```

```text
# edits.txt: one command per line, quoted like a shell ('…' or "…", no escapes)
bpm 128
pattern --id 1 --ch hat --steps "x.x. x.x. x.x5 x.x9"
apply --json '[{"op":"setLoop","loop":{"a":0,"b":16}}]'
render --out preview.wav
```

On a file the project is loaded once and saved once, at the end. The first line that fails stops
the script before the project is saved. The error names the failing line and command. Renders and
ZIP exports write their own output when their line runs. For ZIP export, omit `--dry-run`.
The reply lists each line's result. Without `--project`, a script runs against the running app line
by line: lines before a failing one stay applied, each as its own undo step. Options of the whole
run (`--project`, `--session`, `--save-as`, `--dry-run`, `--force`, `--pretty`) go before
`script`, not on its lines; a leading `synkopa` on a line is ignored. Relative paths in a line are
relative to the current folder.

### Live coding: `synkopa watch`

```sh
synkopa watch song.txt                                    # the running app: every save replaces the song
synkopa -p song.synkopa watch song.txt --render preview.wav  # no app: every save bounces preview.wav
```

```text
# song.txt — the song, as `synkopa script` lines
bpm 128
pattern --id 1 --ch kick --steps "x... x... x... x..."
pattern --id 1 --ch hat  --steps "..x. ..x. ..x. ..x5"
add-fx --insert 3 --type reverb --preset 2
```

Start the loop in Synkopa (or `synkopa play`), open the file in your editor (or let an agent write it)
and save: the song changes while the loop plays. The file is the song, as a buffer in Sonic Pi
or Strudel: each save runs the whole file again on a copy of the project *as it was when the
watch started*, and the result replaces the open project. Delete a line and its effect is gone;
a line that adds something (an effect, a channel) adds it once, not once per save.

- **One undo step per save** ("synkopa watch: song.txt"); a save that changes nothing adds none.
  The transport is not stopped, effects keep their tails, plugin instruments keep their sound.
- **A failing line changes nothing**: the open project stays as the last good save, and the
  error names the line. Transport and undo commands (`play`, `undo` …) do not belong in the
  file: they answer `NEEDS_APP` there.
- **An edit in the app is not overwritten.** After you change the project in Synkopa (or with
  another `synkopa` command), the next save is refused with `REVISION_CONFLICT`. Restart `synkopa watch`
  to build on the project as it is now, or pass `--force` to let the file win (what it replaces
  stays one undo away).
- **Output**: one JSON line per save on stdout: `{"event":"applied","lines":4,"revision":57,
  "changed":true}` (plus `render` with `--render`) or `{"event":"failed","error":"line 3 …"}`.
  `--once` runs the file once, prints that line and exits. Ctrl+C stops watching.
- **With `-p FILE`** the copy is the file's project: it is never saved (bounce with `--render`,
  or save the result with `synkopa -p song.synkopa script song.txt`).

The file is checked every 150 ms and read once it holds still, so an editor that writes in
steps is read once.

## Reading

| command | what you get |
| --- | --- |
| `synkopa status` | transport, revision, file, audio device, CLI path |
| `synkopa context` | summary: tracks (index, insert, volume, clip count), inserts with effects and parameters, patterns, channels, sources, markers, `nextId` |
| `synkopa markers` | the Playlist markers in song order: id, name, `at` (beats) and `bar` (1-based; 9.5 = the middle of bar 9) |
| `synkopa channels` | the Channel Rack: id, name, `sample` (+ source and sampler settings)/`synth` (+ preset)/`keys`, volume, pan, mute/solo, mixer insert (`null` = the clip's track), root key, note count |
| `synkopa pattern [--id N]` | a pattern as one step string per channel (`x` full velocity, `1`–`9` = 10–90 %, `.` empty, a space every beat) plus `notes` for melodic rows, bars, minimum bars, swing % |
| `synkopa context --full` | the whole project JSON (every clip, note, parameter) |
| `synkopa catalog` | effect types, parameter ids/ranges/units, presets, command list |
| `synkopa schema` | every op `apply` accepts, with units and an example |
| `synkopa analyze [--clip ID] [--exclude-fx ID]` | detect tempo (BPM, ½×/2× candidates) and musical key (top 3 candidates, 12-note chroma, tuning offset) |

## Editing

Each command is one undo step. Add `--if-revision N` (the `revision` from your
last read) and the edit is refused with `REVISION_CONFLICT` if the musician
changed something meanwhile — re-read and decide, don't blindly retry.

```sh
synkopa bpm 140
synkopa add-fx --insert 6 --type reverb --preset 3          # → {"fxId":1203,…}
synkopa route --from 2 --to 1 --only                        # Hats only into the Drums bus
synkopa route --from 6 --to 8 --gain 0.35                   # Vocal also sends to a reverb bus
synkopa unroute --from 6 --to 8
synkopa sidechain --from 1 --to 3                           # the kick (1) ducks the bass (3): keys its compressor or adds one
synkopa sidechain --from 1 --to 3 --off                     # the bass's compressor hears itself again
synkopa sidechain --fx 1204 --from 1                        # this compressor listens to insert 1
synkopa set-param --fx 1203 --param mix --value 30
synkopa preset --fx 1203 --index 4
synkopa import kick.wav loop.wav --track 3 --beat 16
synkopa missing                                          # sources whose audio file is missing (they play silence)
synkopa relink --folder D:/Backup/Samples                # find them there by name; relinks unambiguous matches of the same length
synkopa relink --source 1010 --path D:/new/pad.wav       # point one source at another file (one undo step)
synkopa midi inspect song.mid                             # parts, channels, instruments, what each becomes
synkopa midi import song.mid --parts 0,2 --tempo --beat 16   # a new pattern on the Playlist (one undo step)
synkopa midi import lead.mid --parts 1 --pattern 3 --ch 1004 --replace   # one part into a channel
synkopa midi export --out song.mid [--pattern 3]         # the song or one pattern, with tempo
synkopa make-unique --clip 5012                           # own copy of the sample on a new track (same routing)
synkopa export-in-place --clip 5012,5013                 # render them (with mixer effects) to one clip on a new track below
synkopa stems --clip 5012 [--only vocals,drums]           # extract stems: one track per stem below the clip, clip muted
synkopa stems [--install]                                 # stem model state; --install downloads it (90 MB, MIT, SHA-256 checked)
synkopa apply --file edit.json --label "Busier hats"
synkopa undo
```

`edit.json` is an array of ops (see `synkopa schema`), applied atomically:

```json
[
  {"op": "putPattern", "pattern": {"id": 2, "name": "Hats", "color": "#e0a44a", "len": 4, "swing": 0.4,
    "notes": [{"ch": 1002, "step": 0, "len": 1}, {"ch": 1002, "step": 2, "len": 1, "v": 0.6}]}},
  {"op": "setLoop", "loop": {"a": 0, "b": 16}}
]
```

Every note names its channel with `ch` (see `synkopa channels`).

Units: playlist time in **beats**; pattern `step`/`len` in **sixteenths**;
fades in seconds; track/insert `vol` is a fader position (0.7937 = 0 dB);
clip `gain` in dB; audio clip `pitch` in semitones (±12, cents as the fraction; the
length stays) and `stretch` 0.25–4; pan −1..1. New ids: start at `context.nextId`.

## Patterns and channels

Channels (Channel Rack rows) are shared by all patterns; a note plays channel `ch`.
Steps are sixteenths. A pattern is as long as its notes (the end of the last note rounded
up to a bar, like FL Studio); `--bars N` sets the shortest it may be, for empty bars kept on
purpose (a crash every second bar). Swing 0–100 % (delays every odd 16th by up to half a
step). Each edit is one undo step.

```sh
synkopa channels                                     # ids, kinds, routing
synkopa channels --add snare-studio                  # a sampler of a standard-library sample (file or display name, or the start of one: "kick")
synkopa channels --ch "Studio Snare" --param cut --value 1   # one sampler setting: start, end, reverse, norm, semi, fine, env, delay, attack, hold, decay, sustain, release, attackTension, decayTension, releaseTension, fadeInMs, fadeOutMs, cut
synkopa channels --ch "Studio Snare" --param fadeOutMs --value 80   # fadeInMs and fadeOutMs in source milliseconds
synkopa channels --sample 1001                       # a sampler channel playing source 1001
synkopa channels --synth "808 Deep"                # a synth channel with a factory preset ("" = blank patch)
synkopa channels --ch 1005 --preset "Pad Warm"     # load a synth preset (ids/names in `synkopa catalog` → effects.synth)
synkopa channels --ch 1005 --param flt_cutoff --value 900   # one synth knob
synkopa channels --keys                              # Synkopa Keys: the sampled grand piano (see docs/keys.md)
synkopa channels --ch "Synkopa Keys" --param release --value 600   # its knobs: release (ms), dynamics (%)
synkopa channels --musicbox                          # the Music Box: the modeled tine instrument (see docs/musicbox.md)
synkopa channels --ch "Music Box" --param ring --value 140     # its knobs: ring, brightness, body, mechanical, spread (%); release (ms)
synkopa channels --plugin serum                      # a scanned VST3 instrument (name, start of one, or class id; `synkopa plugins --rescan` first)
synkopa plugin-params --fx 1007                      # its parameters: a plugin channel's id works wherever an effect id does
synkopa channels --ch Kick --group Drums             # the Channel Rack filter group ("" = none, unsorted)
synkopa channels --remove 1004                       # (or a name) — also removes its notes
synkopa pattern --id 2                               # step strings per channel
synkopa pattern --id 2 --ch hat --steps "x.x. x.x. x.x5 x.x9"     # replace the hat's notes
synkopa pattern --id 2 --ch kick --steps "x... x... x... x..." --vel 0.9
synkopa pattern --id 3 --ch bass --notes '[{"step":0,"len":3,"p":33},{"step":4,"len":2,"p":40,"v":0.8}]'
synkopa pattern --id 2 --ch hat --clear
synkopa pattern --id 2 --bars 2 --swing 55 --name "Hats B"
synkopa pattern --new --name Fill --bars 1           # → its id in the reply
synkopa pattern --id 2 --play                        # loop it on its own (stops the song); --stop
synkopa pattern --id 2 --play --from 4 --loop 4:8    # from beat 4, looping beats 4–8 of it
```

`--ch` takes a channel id or name (`kick`, `Hats 2` …). With `apply`, the ops are
`putPattern` / `putChannel` / `removeChannel` (see `synkopa schema`). When a pattern's
length changes (notes added past its end or removed, `--bars`), clips that show exactly one
whole pass of it follow; clips sized on purpose keep their length.

`--steps` creates explicit triggers. A sampler trigger rings out even with the envelope on.
`--notes` creates held notes by default. For a sampler, a held note stops at its set length
or the end of the sample, whichever comes first. Include `"trig":true` on any trigger in a mixed row. When a row contains held notes or cannot be represented as a step string, the reply's
`notes` array carries `trig` for each event.

`"slide":true` makes an FL slide note: it starts no sound; the channel's notes sounding when it
starts glide by the distance from the highest of them to its key `p`, over its `len`, and keep
that pitch until they end (sampler, Synkopa Keys and Synth channels; plugin instruments ignore
it; MIDI export leaves it out). An 808 jumping an octave up for an eighth note:

```sh
synkopa pattern --id 3 --ch 808 --notes '[{"step":0,"len":8,"p":36},{"step":4,"len":2,"p":48,"slide":true}]'
```

## Takes and comping

A track you recorded with a loop set (one take per pass), or recorded over
an existing clip, has a comp clip. Times here are **timeline beats**,
crossfades **milliseconds**; add `--clip ID` when a track has several comps.

```sh
synkopa takes --track 5                       # takes, comp regions, crossfades
synkopa comp --track 5 --set '[{"take":2001,"from":32,"to":36},{"take":2002,"from":36,"to":40,"xfadeMs":20}]'
synkopa comp --track 5 --take 2003            # one whole take
synkopa comp --track 5 --xfade 10             # default crossfade at region boundaries
synkopa comp --track 5 --remove-unused        # drop takes the comp does not play
synkopa comp --track 5 --audition 2002        # hear one take instead of the comp (`off` to stop; not an edit)
synkopa comp --track 5 --flatten              # render to one audio clip, takes removed (undo brings them back)
```

Each edit is one undo step. Through `apply`, the ops are `putComp` /
`removeComp` with comp-beat times and crossfades in seconds (see `synkopa schema`).

## Lyrics (the notebook)

`synkopa context` lists the notebook pages (`id`, `name`, `startBar`, `lines`);
`synkopa context --full` includes their text. Each line of a page is one bar from
`startBar`, `[…]` lines are headers. Write them with one op that replaces every
page (one undo step):

```sh
synkopa apply --json '[{"op":"putNotebook","notebook":{"pages":[{"id":3000,"name":"Refren","startBar":17,"text":"[Refren]\nto jest nasz czas\nnasza noc"}]}}]'
```

## Markers (song sections)

Named points on the Playlist timeline (FL Studio's time markers): labels only, they never change
what plays. A marker's *section* runs to the next marker (the last one: to the song end). Name a
marker by its id or its name (case-insensitive). Each edit is one undo step.

```sh
synkopa markers                                   # [{id, name, at, bar}] in song order
synkopa markers --add Intro --bar 1               # --bar N (1-based) or --at B (beats); default: the start position
synkopa markers --add Hook --at 32
synkopa markers --marker Hook --name "Hook 1"     # rename
synkopa markers --marker "Hook 1" --bar 17        # move (both at once: one step)
synkopa markers --loop "Hook 1"                   # the loop = the section from the marker to the next one
synkopa markers --remove Intro
```

Through `apply` the ops are `putMarker {marker: {id, at, name}}` (add, move or rename by id;
`at` 0..2048 beats, names up to 80 characters, at most 256 markers, kept sorted) and
`removeMarker {id}`.

## Listening without speakers

```sh
synkopa render --out bounce.wav --from 0 --to 32
```

renders offline (independent of the audio device, so it works while the
musician keeps playing) and prints peak/RMS/crest/clipping so an agent can
check its work, then listen to the WAV. The same bounce is available in the
app as **Eksport** (`Ctrl+Shift+R`): song or loop, a 0/2/8 s effect tail, and
44.1 or 48 kHz. Stereo WAV defaults to 32-bit float. Select 24-bit PCM with
`--wav-bits 24`, or 16-bit PCM (CD audio) with `--wav-bits 16`, and add `--dither`
for triangular dither of ±1 LSB, applied once at the final conversion. Integer WAV
clips samples beyond full scale. The export stats report those peaks before conversion.

Select **Wrap** when exporting a WAV loop to carry the effect tail into its
start. The result is one loop long. In the CLI, use `--wrap-tail` with the
project loop or specify both `--from` and `--to`. This works for stereo mixes
and stems.

### Stems

```sh
synkopa render --stems --out stems/ --from 0 --to 32     # a folder, created if needed
synkopa render --stems --out stems/ --no-master          # without Master.wav
synkopa render --stems --out stems/ --wav-bits 24 --dither
synkopa render --out cd.wav --wav-bits 16 --dither --rate 44100
synkopa render --stems --out stems/ --format mp3 --kbps 192
```

writes one stereo file per mixer insert that has sound, named after the insert
(`Drums.wav`, `Bass.wav`; a repeated name becomes `Bass (2).wav`). In the app, choose
**Export → Output: Stems**, select WAV or MP3, then pick the folder.

- A stem is what the insert plays after its own effects, fader, pan and width, **before the
  master**. With mixer routing, a bus's stem includes what is routed into it, and a bus fed only
  by sends still gets a stem; a source's stem is its own output, even if it normally goes only
  into a bus. The optional `Master` file contains the normal mix with the master's effects.
  Use `--no-master` to leave it out. Nonlinear effects on the master, such as a limiter,
  can make the mix sound different from the sum of the stems.
- Every bounce uses the same range and effect tail. Use WAV for sample-accurate alignment in
  editors that ignore MP3 encoder delay and padding.
- Muted inserts and inserts with nothing routed into them get no file, and neither does one that
  turns out silent in the range (the reply lists those under `skipped`). Solo is ignored, so a
  soloed insert does not silence the other stems; a muted track or channel stays silent.
- WAV is the default. MP3 supports 128, 192, 256 and 320 kbps (default 320).
  Files are staged during export and moved into place after all stems are encoded.
  Existing files with the same names are replaced.
- The reply lists every file with its level statistics (`files`), so an agent can check each stem.

## Tune to Reference

Pulls a vocal take onto another performance of the same melody (a guide vocal,
an acapella, a better take) that is already a source in the project. Nothing is
generated and the take is not time-stretched: the reference is aligned to the
take (content, then pitch shape), brought to the take's octave, and its notes
become the take's target; the singer's vibrato and transitions are kept.
Where the alignment is not trusted (another song, a different part) the take
is left as sung.

```sh
synkopa tune-ref --clip 412 --reference 7                       # source 7 = the guide vocal
synkopa tune-ref --clip 412 --reference 7 --correction 80 --vibrato 100 --influence 100 --octave auto
synkopa pitch-target --clip 412            # score, octave, the reference's notes, low-confidence stretches
synkopa pitch-target --clip 412 --full     # plus every curve (source, reference, target, confidence)
synkopa tune-ref --clip 412 --off          # back to manual pitch edits
synkopa f0-model                           # which pitch detector runs (RMVPE or YIN) and its install state
```

A melody you played in the piano roll can be the reference instead: the notes a
channel plays over the clip in the playlist (a copy, so changing the piano roll
later does not change the vocal; `--melody` again takes them again). The take
keeps its own timing; the octave nearest the voice is used unless `--octave`
says otherwise; stretches nothing is played under stay as sung.

```sh
synkopa melody-channels --clip 412         # [{channel, name, notes, fit}] — fit = share of the singing it matches
synkopa tune-ref --clip 412 --melody 1003 --speed 40      # follow the Piano channel, 40 ms to reach each note
```

`--speed` is the retune speed in ms: how long the voice takes to reach each
target note (95 %); 0 or absent = at once.

`--correction` (how far toward the target), `--vibrato` (how much of the
singer's own movement is kept) and `--influence` (100 = the reference's note
centres, 0 = nearest note of `--key`/`--scale`) are percentages. Manual note
edits on the clip still apply on top.

Without a running app, a whole file:

```sh
synkopa tune-file --take take.wav --reference guide.wav --out tuned.wav --json target.json
```

uses the installed RMVPE model, or `--model rmvpe.onnx`, or `--yin`.

**Pitch detector.** Synkopa detects pitch with YIN out of the box. The RMVPE
model (362 MB, from the RVC project; read its licence first) is better on
real voices: Pitch Editor → *Pobierz RMVPE* downloads it after showing the
licence, checks its SHA-256 and switches to it — or from the command line:

```sh
synkopa f0-model --install
```

prints the licence note, downloads the model (a few minutes, no live progress),
checks its SHA-256 and switches the detector to it; works without the app too
(`synkopa -p song.synkopa f0-model --install`). `WAVE_RMVPE_MODEL=path` points at a
copy you already have.

**Stems.** `synkopa stems --clip ID` splits an audio clip with HTDemucs into drums, bass, other and vocals
(`--only` picks some) exactly as the Playlist's *Extract stems…* does: the region the clip plays plus 3 s either
side is separated, each stem is written next to the project's audio as a WAV (44.1 kHz, 32-bit float), goes on
a new track right below the clip and gets a clip at the same place playing the same part with the same
stretch, pitch, gain, fades and pan; the clip is muted; one undo step. It takes about 25 s per minute of
audio on a 6-core CPU and up to ~2.7 GB of memory while it runs. The model is downloaded once
(`synkopa stems --install`); `WAVE_STEMS_MODEL=path` points at a copy you already have.

**Benchmark.** `cargo run --release -p wave-tune-bench -- synth` (synthetic
takes with ±25…400 ct errors and drift against references in other voices,
octaves and tempi, plus unrelated references that must be refused),
`… -- vocalset --root DIR` (Annotated VocalSet: real singers with injected
errors, cross-singer references, mismatched references) and `… -- ab --out DIR`
(a blind A/B listening kit; `ab-score` evaluates the filled-in ratings). Add
`--rmvpe model.onnx` or `--yin`, and `--json out.json` for the full results.
Exit code 0 means every criterion passed; a run with no case, no criterion or a
failed case is a failure. The agent workflow (`pitch suggest` → `editTune` →
render) is measured on real recordings by `agent-vocalset`, `agent-gtsinger`,
`agent-medley`, `estimation` and the three-way `listen` kit; see
[`crates/tune-bench/RESULTS.md`](../crates/tune-bench/RESULTS.md) for the commands,
datasets and the latest results. Note transcription (the pitch editor's notes and what *Copy
notes* pastes) is scored note by note (mir_eval's COn / COnP / COnPOff) by `transcribe`
(Annotated VocalSet; `--basic-pitch nmp.onnx` adds Spotify's Basic Pitch as a baseline) and
`transcribe-grid` (VocalSet + GTSinger, tunes the note tracker on some singers and reports the
held-out ones).

### vocadito transcription benchmark

Fetch [vocadito](https://doi.org/10.5281/zenodo.5578807) (Rachel Bittner, Katherine
Pasalo, Juan José Bosch, Gabriel Meseguer-Brocal, and David Rubinstein, 2021,
CC BY 4.0) and extract `vocadito.zip`. Point `--vocadito` at the directory
containing `Audio/`, `Annotations/Notes/`, and `vocadito_metadata.csv`.

```sh
cargo test --locked -p wave-tune-bench
cargo run --locked -p wave-tune-bench -- transcribe \
  --vocadito /path/to/vocadito --yin --json vocadito-yin.json
```

The command reads every track in the metadata and returns an error naming any
missing audio or A1/A2 note CSV. The report goes to stdout and optionally to
`--json FILE`; progress goes to stderr. Use `--yin` to select YIN, or
`--rmvpe MODEL.onnx` to select an explicit RMVPE model. The default loads the
installed RMVPE model when available and otherwise uses YIN. Passing both
`--yin` and `--rmvpe` is an error. The `backendRecordings` counts report the backend that actually analyzed each
recording, including YIN fallbacks. Use `--root DIR` for Annotated VocalSet and
its `--limit`, `--bpm`, `--basic-pitch`, and `--dump` options.

Each recording is analyzed once. Both annotators score the same raw tracker
notes in source time. The report sums note counts across tracks before computing
precision, recall and F1. The denominators are predicted notes for precision,
reference notes for recall, and their sum for F1 (with twice the matched notes
as numerator). COnP uses the existing one-to-one match within 50 ms of onset and
50 cents of pitch. COnPOff also requires the offset to be within the larger of
50 ms and 20% of the reference duration. A ratio with a zero denominator is
`null`, with counts still present.

A repeat is a consecutive pair of notes on the same rounded MIDI key with a
gap from −20 to +120 ms. Both notes must have distinct COnP matches for the
pair to be preserved. A pair is shared when it occurs as a repeat in both
annotations and the A1-to-A2 COnP match links its two corresponding notes. A
shared pair is preserved when the two annotations link each corresponding note
to the same predicted note. Maximum-cardinality matching uses onset, pitch,
then offset to order both sides. It visits references in that order, prefers
the first free candidate, and can reassign matches along an augmenting path.
Repeat recall uses preserved pairs over reference pairs for A1, A2, and shared
pairs separately.

## Pitch editing for agents

The pitch editor's notes, for scripts and agents: what a vocal clip sings, how it relates to
another performance, a cautious correction proposal, and note-level edits. Edits are the same
`editTune` op the pitch editor sends, so they are one undo step in the app and save into the
project like any manual edit. Tune to Reference (`tune-ref`) is unchanged and independent:
both can be used on one clip (note edits apply on top of a reference).

```sh
synkopa pitch inspect --clip 412                     # notes, tuning, range; `base` fingerprint
synkopa pitch inspect --clip 412 --full --from 10 --to 14   # plus 5 ms frame curves for 4 s
synkopa pitch compare --clip 412 --source 7          # read-only: against another singer's take
synkopa pitch suggest --clip 412 --source 7          # a proposal with a reason per note; changes nothing
synkopa pitch edit --clip 412 --base 9d0cbbba3b9fc299 --note 3 --cents -25
synkopa pitch edit --clip 412 --file edit.json       # several notes at once; also a `pitch suggest` result or its `op`
synkopa pitch render --clip 412 --out take.wav       # the clip alone with its edits (--original: without)
```

Times are **seconds of the clip's source**; pitch is **MIDI** (60 = C4, fractional), changes in
**cents**. Notes are addressed by index `i` in the clip's segmentation: its own notes once it has
any edit, else the analysis' (`segmentation`: `edits` / `analysis`).

### `pitch inspect`

```jsonc
{ "clipId": 412, "revision": 31, "base": "9d0cbbba3b9fc299", "segmentation": "analysis",
  "backend": "rmvpe", "rate": 200,
  "clip": { "source": 1008, "start": 0, "len": 65.06, "offset": 0, "stretch": 1, "bpm": 120,
            "region": [0, 32.53], "sourceSeconds": 32.53, "sampleRate": 44100, … },
  "tuning": { "offsetCents": 9.5, "spreadCents": 14.9, "notes": 41 },   // the take vs A440
  "range": { "low": 61.9, "high": 74.1, "lowName": "D4", "highName": "D5" },
  "kinds": { "stable": 24, "vibrato": 17, "glide": 4, "short": 6 },
  "reference": null, "pencil": { "points": 0, "runs": [] },
  "notes": [ { "i": 2, "start": 1.015, "end": 1.835, "beats": [2.03, 3.67], "kind": "vibrato",
               "sung": 74.21, "nearest": "D5", "cents": 21.4, "out": 74.21,
               "shift": 0, "drift": 0, "formant": 0, "gain": 0, "confidence": 0.92, "voiced": 1,
               "levelDb": -18.9, "slopeCents": 22.7, "spreadCents": 44.5, "stepCents": 4.0, "vibrato": [3.2, 37.5] } ],
  "frames": { "rate": 200, "start": 10.0, "f0": [69.02, null, …], "confidence": […], "out": […] }  // --full
}
```

`kind`: `stable` (a held pitch), `vibrato` (held, with a vibrato: `[rate Hz, depth ct]`), `glide`
(a slide through the note's held middle), `short` (< 80 ms: ornament or transition), `unsure`
(breathy, mostly unvoiced or low detector confidence). `sung` is the recording; `out` what it
sounds like with the current edits (and Tune to Reference). `slopeCents` is the pitch change
across the note's central 60 %, `spreadCents` the movement around it, `stepCents` the difference
between the medians of its second and first half (two pitches sung legato as one note: split it).

### `pitch compare`

Read-only. Aligns the take with another source (another singer, another key, other phrasing)
and reports, without deciding anything:

* `related` — the reference sings the same melody: the alignment is trusted **and** at least
  55 % of the confidently matched notes (and at least 3 of them) agree within 50 ct after the
  interval. `aligned` alone
  means the same material in time (a harmony part aligns but is not related). Only a related
  reference is used by `suggest`.
* `intervals` — the take's interval above the reference, best first: `semitones` (any interval,
  not only octaves) and `cents` (the measured offset, which includes a difference between the two
  singers' tunings), with `support`; `ambiguousInterval` when a second interval is close.
* `notes[]` — per take note: `relation` `match` / `merged` (one take note over several reference
  notes: the reference ornaments) / `split` / `complex` / `extra` (nothing aligned with
  confidence: an ad-lib, or not trusted), the reference notes it covers, `expected` (the
  reference's note in the take's register), `diffCents`, `onsetMs`, `confidence`.
* `referenceNotes[]` with `ornament` (short notes inside a run the take sings as one note),
  `counts` (`matched`, `merged`, `split`, `extra`, `missing`), `sameNotes`, `contour`,
  `coverage`, `unmatched` stretches, `timingMs`, `referenceTuningCents`.

### `pitch suggest`

A conservative proposal, with a decision and a reason for every note; it changes nothing.
Rules: short, unsure and sliding notes, and notes holding two pitches (`stepCents` ≥ 25), are kept; an unrelated reference changes nothing at all;
with a related reference the intended pitch is the reference's note in the take's register,
unless the take is further than `--max-cents` (default 60) from it — a semitone or more off yet
itself in tune is what a deliberate variation looks like, so it is kept; without a reference,
the nearest note of the key (`--key C --scale minor`, or the project's key) or the nearest
semitone, unless the take is near the middle between two notes. Within `--min-cents` (15) a note
is left alone. `--strength 50` goes half the way; `--tuning 8` puts the grid at A = 442 Hz (use the
backing track's `synkopa analyze` tuning offset). The result's `op` is a ready `editTune` op.

### `pitch edit`

One undo step, refused as a whole if any part is invalid. `--base` (from `inspect`) refuses
the edit with `STALE_NOTES` if the notes were split, merged or re-analysed since;
`--if-revision` refuses it if anything in the project changed. `--max-cents C` refuses a
result that shifts a note further than C (`OVER_LIMIT`).

| flag / JSON field | meaning |
| --- | --- |
| `--note I --shift C` / `{"i": I, "shift": C}` | the note's pitch move, cents (absolute) |
| `--cents C` / `"shiftBy": C` | add C cents to its current move |
| `--target M` / `"target": M` | move the note's centre to MIDI M (turned into `shift`) |
| `--drift %` / `"drift": 0..1` | flatten the movement inside the note toward its centre |
| `--formant C`, `--gain dB`, `--speed MS\|none` | formant cents, level, retune speed |
| `--split I@SEC`, `--merge I`, `--reset I,J`, `--clear` | structure; reset = back to as sung; clear = every note edit and pencil off |
| `"pencil": [[sec, midi], …]`, `"erasePencil": [[from, to]]` | draw / erase pitch |

All indices in one edit name the notes as they were **before** it, whatever order the parts are
in; they apply as clear, reset, notes, split, merge, erasePencil, pencil. A note cannot be both
split and merged, nor edited and merged into its neighbour, in one edit. The reply has the new
`base`.

### An agent's correction workflow

Your recording is clip 412; the original singer (another key, range and phrasing) is source 7;
the instrumental is clip 300.

```sh
synkopa analyze --clip 300                                # key and tuning offset of the backing
synkopa pitch inspect --clip 412 > take.json              # what you sang: kinds, cents, tuning, range
synkopa pitch compare --clip 412 --source 7 > cmp.json    # related? interval? which notes match?
synkopa pitch suggest --clip 412 --source 7 --key A --scale minor --max-cents 60 > plan.json
```

Then decide, note by note: accept the plan's edits that make sense, drop the ones that touch a
deliberate variation, add your own (`--target`, a split before a scoop you want to keep), and
apply them as one edit:

```sh
jq 'del(.op.edit.notes[] | select(.i == 21))' plan.json > edit.json   # e.g. drop note 21's change
synkopa pitch edit --clip 412 --file edit.json --if-revision "$(jq .revision plan.json)" --max-cents 80
synkopa pitch render --clip 412 --out after.wav && synkopa pitch render --clip 412 --out before.wav --original
synkopa pitch inspect --clip 412                          # `out` per note: did it land?
synkopa pitch edit --clip 412 --reset 7                   # put back a note that sounds worse; or `synkopa undo`
```

What the defaults protect: a different note, an ornament the reference has and you do not (or
the other way round), a slide or vibrato, and a reference that is not the same song are left
as sung. Timing is never changed. `--max-cents` raises the limit when you know the take is far
off; low-level `pitch edit` always lets you override the plan.

## Auto vocal

One call that does what the app's Auto vocal button does: measures the take (loudness,
sibilance, key, how much of it is sung), then puts a calibrated chain on the clip's mixer
insert — pitch correction, vocal rider, FET compressor, slow compressor, de-esser, reverb,
limiter — tuned to that measurement. On the Playlist the clip must sit on a lane routed to its
own insert (a lane that plays into the master is refused; route it in the mixer first).

```sh
synkopa auto-vocal --clip 412                              # measure, and put the chain on the insert
synkopa auto-vocal --clip 412 --style hard                 # rap / hard-tune windows
synkopa auto-vocal --clip 412 --beat-clip 300              # take the key and level from the beat
synkopa auto-vocal --clip 412 --plan > plan.json           # read-only: the plan, nothing changes
```

The rider (a long look-ahead, so seconds of delay if it ran live) is rendered into the clip's
audio first — the clip then plays that bake at 0 dB gain and the original source stays in the
project. A clip that was baked before is not baked twice; `--no-bake` keeps the rider live on
the insert instead. Effects of the kinds the chain brings are replaced (an old rider counts);
others, e.g. an EQ, stay in front of the chain. The reply:

```jsonc
{
  "revision": 41, "clipId": 412, "insertId": 3, "style": "natural",
  "riderBaked": { "sourceId": 908, "clipId": 412 },    // null with --no-bake and on a re-run
  "fxIds": [911, 912, 913, 914, 915, 916],             // the chain stages, in chain order
  "trackGainDb": 6.4,                                  // null without --beat-clip
  "warnings": []
}
```

The bake and the chain are two undo steps. `--plan` prints the plan itself: the `chain` with
every stage's parameters and a one-line reason (`note`), `riderBake` when the rider would be
baked, the measured `analysis`, the `trackGainDb` level suggestion and `stages` (what each
stage achieved). Slow: a few seconds per minute of audio. Works without the app
(`synkopa -p song.synkopa auto-vocal --clip 412`), like every offline command.

## FX chain presets

```sh
synkopa fxchain list
synkopa fxchain save "Vocal chain" --insert 3            # --force replaces a preset of that name
synkopa fxchain load "Vocal chain" --insert 5 --mode replace
synkopa fxchain rename "Vocal chain" --to "Lead vocal"
synkopa fxchain delete "Lead vocal"
```

saves a mixer insert's effects, in order and with their settings and VST3 state, to your
library (`%LOCALAPPDATA%\WAVE\FX Chains`, one `.wavechain` file per preset) and puts them on
any insert of any project. Loading is one undo step and never changes the insert's volume, pan,
name, colour or routing. An insert that already has effects needs `--mode replace` or
`--mode append`. In the app: the **Presets** button above an insert's effects, or the insert's
right-click menu.

## Plugins

```sh
synkopa plugins                    # installed effects: VST3 (cached scan), LV2 on Linux; VST3 instruments under `instruments`; --rescan
                                # {plugins:[{format,path,cid,name,vendor,category,unsupported?}],
                                #  unavailable:{lv2:"why"}}
synkopa plugin-add --insert 2 --path "C:\…\Pro-Q 3.vst3" --cid 72C4DB…            # VST3
synkopa plugin-add --insert 2 --format lv2 --cid http://gareus.org/oss/lv2/dpl#stereo   # LV2: the URI
synkopa plugin-params --fx 1210 --query freq
                # VST3: {id,name,value,display,def}; LV2 also {symbol,real,min,max,unit,
                # steps,toggle,integer,log,options}: id is the control port index
synkopa plugin-set --fx 1210 --id 42 --value 0.3   # normalised 0..1 (LV2: over the port's range)
synkopa plugin-load-anyway --fx 1210               # load a plugin Synkopa's safety check refused (it crashed or hung in a test process)
                # vst3, vst3-add, vst3-params, vst3-set: the older names, VST3 only
synkopa auto-add --fx 1210 --param 42 --points "0:0.2,8:0.8" --name "Reverb decay"
                # automate a hosted plugin parameter: an envelope plus the
                # playlist clip that plays it; ids come from context.nextId
synkopa auto-add --target insertVol --id 3 --points "0:0.2,8:0.8"
synkopa auto-add --target mix --fx 1210 --value 0.4
                # the same for an insert/track/channel fader or pan (--target
                # insertVol|insertPan|trackVol|chanVol|chanPan --id, or mix --fx)
```

## Notes (deterministic transforms and generators)

Every verb is one undo step and deterministic in `--seed`: the same command on the same
pattern produces identical notes, so agents can generate variants, render each and keep
a favourite.

```sh
synkopa notes humanize --pattern 1 --timing 0.3 --velocity 0.2
synkopa notes quantize --pattern 1 --grid 1 --amount 1 --scale      # --scale: into the project key
synkopa notes euclid --pattern 1 --ch 1001 --k 5 --n 16 --rotate 2  # E(k,n) fill for one channel
synkopa notes strum --pattern 1 --width 0.5 --ramp 0.05
synkopa notes chop --pattern 1 --len 1 --select "0:64@1001"         # FL's chop: pieces a 16th long
synkopa notes glue --pattern 1                                       # FL's glue: touching notes of one key join
synkopa notes arp --pattern 3 --mode upDown --rate 1 --gate 0.9 --octaves 2   # chords become arpeggios
synkopa notes drums --pattern 2 --bars 2 --seed 42 --voice "1001:4:16:0" --voice "1002:2:16:8"
synkopa notes melody --pattern 3 --ch 1003 --bars 2 --seed 7 --density 0.5
synkopa notes chord --pattern 3 --type diatonic                      # harmonize single notes (A3 → Am)
synkopa notes chord --pattern 3 --select "0:16" --type min7          # spelled by interval, any key
synkopa notes progression --pattern 4 --ch 1003 --bars 4 --template minloop --seed 7
synkopa notes progression --pattern 4 --ch 1003 --degrees "0:5:2:6" --no-voice-lead
synkopa notes stamp --pattern 3 --ch 1003 --step 0 --root A3 --type min7 --inv 1
synkopa notes humanize --pattern 1 --select "0:16"                  # limit to a step range
synkopa key A minor                                                  # project key for --scale / diatonic spelling
synkopa key --off
```

Chord tools: `stamp` writes one chord (18 types, optional inversion; `--root` takes a MIDI
key or a note name like `A3`) and replaces that step; `chord` harmonizes selected single
notes — `--type diatonic` spells the triad from the project key and leaves out-of-scale
notes alone, named types spell by interval; `progression` generates one diatonic 7th chord
per bar (`--degrees "d:d:d"` replaces the template with an arbitrary degree list, at most
32; inversions are voice-led to move the least unless `--no-voice-lead`).

FL's note tools: `chop` cuts held notes into pieces `--len` steps long (at least 0.25, the
last piece keeps the remainder); `glue` joins touching or overlapping notes of one channel and
key (it undoes `chop`); `arp` turns each chord (notes starting together on a channel) into a run
of its keys every `--rate` steps for as long as it lasts, `--gate` of a step long, over
`--octaves` 1–4, in `--mode` up, down, upDown or random (seeded). Step triggers stay as they are.
One run makes at most 20 000 notes.

`--select "0:16@1001"` also filters one channel; `--indices "1,2,5"` selects exact notes.

## Several apps open

`synkopa sessions` lists them; pass `--session ID` (a prefix is enough).
