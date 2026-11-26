# Recording and vocals

Comp the best parts of your takes, pull a vocal onto a reference melody, fix single notes, and put a
calibrated vocal chain on a track, all from the terminal.

## Quick start

```sh
synkopa takes --track 5                                    # the takes of a recorded track
synkopa comp --track 5 --take 2003                         # play one whole take
synkopa pitch inspect --clip 412                           # what the vocal sings, note by note
synkopa tune-ref --clip 412 --reference 7                  # pull it onto a guide vocal (source 7)
synkopa auto-vocal --clip 412 --beat-clip 300              # measure it and put the vocal chain on its insert
```

Clip ids are Playlist clips, source ids are project audio sources: both are in `synkopa context`.

## Comp takes

Why: keep the best phrase of each pass without cutting audio by hand.

A track you recorded with a loop set (one take per pass), or recorded over an existing clip, has a
*comp clip*: it plays regions of different takes in turn. Times here are **timeline beats** and crossfades
are **milliseconds**. Add `--clip ID` when a track has several comps.

```sh
synkopa takes --track 5                       # takes, comp regions, crossfades
synkopa comp --track 5 --set '[{"take":2001,"from":32,"to":36},{"take":2002,"from":36,"to":40,"xfadeMs":20}]'
synkopa comp --track 5 --take 2003            # one whole take
synkopa comp --track 5 --xfade 10             # default crossfade at region boundaries
synkopa comp --track 5 --remove-unused        # drop takes the comp does not play
synkopa comp --track 5 --audition 2002        # hear one take instead of the comp (`off` to stop)
synkopa comp --track 5 --flatten              # render to one audio clip, takes removed
```

Each edit is one undo step; undo after `--flatten` brings the takes back. `--audition` is not an edit and
needs the running app.

## Tune a vocal to a reference

Why: pull a take onto another performance of the same melody (a guide vocal, an acapella, a better take)
while keeping the singer's own vibrato and transitions.

The reference must already be a source in the project. Nothing is generated and the take is not
time-stretched: the reference is aligned to the take (content first, then pitch shape), brought to the
take's octave, and its notes become the take's target. Where the alignment is not trusted (another song,
a different part) the take is left as sung.

```sh
synkopa tune-ref --clip 412 --reference 7                       # source 7 = the guide vocal
synkopa tune-ref --clip 412 --reference 7 --correction 80 --vibrato 100 --influence 100 --octave auto
synkopa pitch-target --clip 412            # score, octave, the reference's notes, low-confidence stretches
synkopa pitch-target --clip 412 --full     # plus every curve (source, reference, target, confidence)
synkopa tune-ref --clip 412 --off          # back to manual pitch edits
```

### Use a melody you played as the reference

Why: no guide vocal? Play the melody in the piano roll and tune to that.

The reference is then the notes a channel plays over the clip on the Playlist. It is a copy, so changing
the piano roll later does not change the vocal; run `--melody` again to take the notes again. The take
keeps its own timing, the octave nearest the voice is used unless `--octave` says otherwise, and stretches
where nothing is played stay as sung.

```sh
synkopa melody-channels --clip 412                          # [{channel, name, notes, fit}]; fit = share of the singing it matches
synkopa tune-ref --clip 412 --melody 1003 --speed 40        # follow the Piano channel, 40 ms to reach each note
```

### Options

| Option | What it does |
| --- | --- |
| `--correction 0..100` | How far toward the target (%) |
| `--vibrato 0..100` | How much of the singer's own movement is kept (%) |
| `--influence 0..100` | 100 = the reference's note centres, 0 = the nearest note of `--key`/`--scale` (%) |
| `--speed MS` | Retune speed: how long the voice takes to reach each target note (95 %); 0 or absent = at once |
| `--octave auto\|N` | Octave shift in semitones, or `auto` |
| `--key 0..11`, `--scale 0..4` | Key (0 = C) and scale for `--influence` |

Manual note edits on the clip still apply on top.

### Tune a file without the app

```sh
synkopa tune-file --take take.wav --reference guide.wav --out tuned.wav --json target.json
```

It uses the installed RMVPE model, or `--model rmvpe.onnx`, or `--yin`. It takes `--correction`,
`--vibrato`, `--influence`, `--key` and `--scale` like `tune-ref`; `--octave` is `auto` or a multiple of
12 within ±36. `--speed` and `--melody` do not apply here.

## Choose the pitch detector

Why: pitch detection decides what every pitch tool hears. RMVPE is better on real voices than the
built-in detector.

Synkopa detects pitch with YIN out of the box. The RMVPE model (362 MB, from the RVC project) is better on
real voices. Read its licence first: the RVC code is MIT, but the weights' licence file restricts use to
research. Downloading is your decision.

```sh
synkopa f0-model                     # which detector runs (RMVPE or YIN) and the install state
synkopa f0-model --install           # print the licence note, download, check, switch to RMVPE
synkopa -p song.synkopa f0-model --install   # works without the app too
```

The download takes a few minutes with no live progress; the file is checked against its SHA-256. In the
app, the pitch editor's **Download RMVPE (362 MB)** button does the same after showing the licence.
`WAVE_RMVPE_MODEL=path` points at a copy you already have.

### Better note detection (GAME)

Why: the notes of a vocal (the pitch editor's note blobs, what *Copy notes* pastes, what `pitch inspect`
lists, what Convert to MIDI writes) come from the built-in note tracker. GAME separates a note sung again
on the same pitch and ends notes where the singing ends.

GAME is 46 MB, from openvpi. Its weights are non-commercial (CC BY-NC-SA 4.0), so read the licence before
you install it. In the app, the pitch editor's **Better notes** button downloads it after showing the
licence.

```sh
synkopa note-model             # which note finder runs (GAME or the note tracker) and the install state
synkopa note-model --install   # download GAME and use it
```

`WAVE_GAME_MODEL=dir` points at an unpacked copy you already have. `tune-bench … --game [DIR]` scores any
benchmark suite with it.

## Edit pitch note by note

Why: inspect what a vocal sings, compare it with another performance, get a cautious correction
proposal, and apply note-level edits, as a script or an agent.

Edits are the same `editTune` op the pitch editor sends: one undo step in the app, saved in the project
like a manual edit. Tune to Reference is independent; both can be used on one clip (note edits apply on
top of a reference).

```sh
synkopa pitch inspect --clip 412                     # notes, tuning, range; the `base` fingerprint
synkopa pitch inspect --clip 412 --full --from 10 --to 14   # plus 5 ms frame curves for those 4 s
synkopa pitch compare --clip 412 --source 7          # read-only: against another singer's take
synkopa pitch suggest --clip 412 --source 7          # a proposal with a reason per note; changes nothing
synkopa pitch edit --clip 412 --base 9d0cbbba3b9fc299 --note 3 --cents -25
synkopa pitch edit --clip 412 --file edit.json       # several notes at once; also a `pitch suggest` result or its `op`
synkopa pitch render --clip 412 --out take.wav       # the clip alone with its edits (--original: without)
```

- Times are **seconds of the clip's source**; pitch is **MIDI** (60 = C4, fractional); changes are in
  **cents**.
- Notes are addressed by index `i` in the clip's segmentation: its own notes once it has any edit, else the
  analysis' (the reply's `segmentation` is `edits` or `analysis`).
- `pitch inspect --all` also lists notes outside the clip's region. `pitch compare` and `pitch suggest`
  take `--ref-clip ID` instead of `--source` to compare against another clip.
- With the running app, give `pitch render --out` a full path: a relative path is resolved by the app, not
  by your terminal.

### `pitch suggest`: a cautious proposal

It decides every note and gives a reason; it changes nothing.

- Kept as sung: short, unsure and sliding notes, and notes holding two pitches (`stepCents` ≥ 25).
- An unrelated reference changes nothing at all.
- With a related reference, the intended pitch is the reference's note in the take's register, unless the
  take is further than `--max-cents` (default 60) from it. A semitone or more off yet itself in tune is
  what a deliberate variation looks like, so it is kept.
- Without a reference: the nearest note of the key (`--key C --scale minor`, or the project's key) or the
  nearest semitone, unless the take is near the middle between two notes.
- Within `--min-cents` (15) a note is left alone. `--strength 50` goes half the way. `--tuning 8` puts the
  grid at A = 442 Hz (use the backing track's tuning offset from `synkopa analyze`).
- The result's `op` is a ready `editTune` op you can pass to `pitch edit --file`.

### `pitch edit`: apply edits

One undo step, refused as a whole if any part is invalid.

- `--base` (from `inspect`) refuses the edit with `STALE_NOTES` if the notes were split, merged or
  re-analysed since.
- `--if-revision` refuses it if anything in the project changed.
- `--max-cents C` refuses a result that moves a note further than C cents (`OVER_LIMIT`).

| Flag / JSON field | Meaning |
| --- | --- |
| `--note I --shift C` / `{"i": I, "shift": C}` | The note's pitch move in cents (absolute) |
| `--cents C` / `"shiftBy": C` | Add C cents to its current move |
| `--target M` / `"target": M` | Move the note's centre to MIDI M (turned into `shift`) |
| `--drift %` / `"drift": 0..1` | Flatten the movement inside the note toward its centre |
| `--formant C`, `--gain dB`, `--speed MS\|none` | Formant cents, level, retune speed |
| `--split I@SEC`, `--merge I`, `--reset I,J`, `--clear` | Structure; reset = back to as sung; clear = every note edit and pencil off |
| `"pencil": [[sec, midi], …]`, `"erasePencil": [[from, to]]` | Draw / erase pitch |

All indices in one edit name the notes as they were **before** it, whatever order the parts are in. They
apply in this order: clear, reset, notes, split, merge, erasePencil, pencil. A note cannot be both split
and merged, nor edited and merged into its neighbour, in one edit. The reply has the new `base`.

A worked example, from inspection to a checked result, is in
[Working with coding agents](agents.md#correct-a-vocal-against-a-reference).

## Auto vocal: a tuned chain in one call

Why: get a vocal to a mixed, consistent level and in tune without building the chain by hand. This is the
app's **Auto vocal** button.

It measures the take (loudness, sibilance, key, how much of it is sung), then puts a calibrated chain on
the clip's mixer insert, tuned to that measurement: pitch correction, vocal rider, FET compressor, slow
compressor, de-esser, reverb, limiter. On the Playlist the clip must sit on a track routed to its own
insert; a track that plays into the master is refused (route it in the mixer first).

```sh
synkopa auto-vocal --clip 412                              # measure, and put the chain on the insert
synkopa auto-vocal --clip 412 --style hard                 # rap / hard tune: faster rider, fast retune
synkopa auto-vocal --clip 412 --beat-clip 300              # take the key and level from the beat
synkopa auto-vocal --clip 412 --plan > plan.json           # read-only: the plan, nothing changes
synkopa auto-vocal --clip 412 --label "Tune verse"         # names the undo steps (default "Auto Vocal")
```

- **The rider is baked.** The rider needs a long look-ahead (seconds of delay if it ran live), so it is
  rendered into the clip's audio first. The clip then plays that bake at 0 dB gain; the original source
  stays in the project. A clip baked before is not baked twice. `--no-bake` keeps the rider live on the
  insert instead.
- **Two undo steps:** the bake and the chain.
- **Existing effects.** Effects of the kinds the chain brings are replaced (an old rider counts). Others,
  for example an EQ, stay in front of the chain.
- **The key** comes from `--beat-clip`, or else from the other tracks.
- It is slow: a few seconds per minute of audio. It works on a file too
  (`synkopa -p song.synkopa auto-vocal --clip 412`).

## Technical notes

### `auto-vocal` reply and plan

```jsonc
{
  "revision": 41, "clipId": 412, "insertId": 3, "style": "natural",
  "riderBaked": { "sourceId": 908, "clipId": 412 },    // null with --no-bake and on a re-run
  "fxIds": [911, 912, 913, 914, 915, 916],             // the chain stages, in chain order
  "trackGainDb": 6.4,                                  // null without --beat-clip
  "warnings": []
}
```

`--plan` prints the plan itself: the `chain` with every stage's parameters and a one-line reason (`note`),
`riderBake` when the rider would be baked, the measured `analysis`, the `trackGainDb` level suggestion and
`stages` (what each stage achieved).

### Comp ops

Through `apply`, the ops are `putComp` / `removeComp`, with comp-beat times and crossfades in seconds
(see `synkopa schema`).

### `pitch inspect` reply

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

- `kind`: `stable` (a held pitch), `vibrato` (held, with a vibrato: `[rate Hz, depth ct]`), `glide` (a slide
  through the note's held middle), `short` (< 80 ms: an ornament or transition), `unsure` (breathy, mostly
  unvoiced, or low detector confidence).
- `sung` is the recording; `out` is what it sounds like with the current edits (and Tune to Reference).
- `slopeCents` is the pitch change across the note's central 60 %; `spreadCents` the movement around it;
  `stepCents` the difference between the medians of its second and first half (two pitches sung legato as
  one note: split it).

### `pitch compare` reply

Read-only. It aligns the take with another source (another singer, another key, other phrasing) and
reports, without deciding anything:

- `related`: the reference sings the same melody. The alignment is trusted **and** at least 55 % of the
  confidently matched notes (and at least 3 of them) agree within 50 ct after the interval. `aligned` alone
  means the same material in time (a harmony part aligns but is not related). Only a related reference is
  used by `suggest`.
- `intervals`: the take's interval above the reference, best first: `semitones` (any interval, not only
  octaves) and `cents` (the measured offset, which includes any difference between the two singers'
  tunings), with `support`; `ambiguousInterval` when a second interval is close.
- `notes[]`: per take note, `relation` is `match`, `merged` (one take note over several reference notes:
  the reference ornaments), `split`, `complex` or `extra` (nothing aligned with confidence: an ad-lib, or
  not trusted); plus the reference notes it covers, `expected` (the reference's note in the take's
  register), `diffCents`, `onsetMs`, `confidence`.
- `referenceNotes[]` with `ornament` (short notes inside a run the take sings as one note), `counts`
  (`matched`, `merged`, `split`, `extra`, `missing`), `sameNotes`, `contour`, `coverage`, `unmatched`
  stretches, `timingMs`, `referenceTuningCents`.

### Benchmarks

`crates/tune-bench` measures the pitch pipeline. Exit code 0 means every criterion passed; a run with no
case, no criterion or a failed case is a failure. Add `--rmvpe model.onnx` or `--yin` to pick the detector,
and `--json out.json` for the full results.

| Command | What it measures |
| --- | --- |
| `cargo run --release -p wave-tune-bench -- synth` | Synthetic takes with ±25…400 ct errors and drift against references in other voices, octaves and tempi, plus unrelated references that must be refused |
| `… -- vocalset --root DIR` | Annotated VocalSet: real singers with injected errors, cross-singer references, mismatched references |
| `… -- ab --out DIR`, `… -- ab-score` | A blind A/B listening kit, and scoring of the filled-in ratings |
| `… -- agent-vocalset`, `agent-gtsinger`, `agent-medley`, `estimation`, `listen` | The agent workflow (`pitch suggest` → `editTune` → render) on real recordings, and a three-way listening kit |
| `… -- transcribe` | Note transcription (the pitch editor's notes, and what *Copy notes* pastes), scored note by note with mir_eval's COn / COnP / COnPOff on Annotated VocalSet; `--basic-pitch nmp.onnx` adds Spotify's Basic Pitch as a baseline |
| `… -- transcribe-grid` | VocalSet + GTSinger: tunes the note tracker on some singers and reports the held-out ones |

See [`crates/tune-bench/RESULTS.md`](../../crates/tune-bench/RESULTS.md) for the commands, datasets and the
latest results.

#### vocadito transcription benchmark

Fetch [vocadito](https://doi.org/10.5281/zenodo.5578807) (Rachel Bittner, Katherine Pasalo, Juan José
Bosch, Gabriel Meseguer-Brocal, and David Rubinstein, 2021, CC BY 4.0) and extract `vocadito.zip`. Point
`--vocadito` at the directory containing `Audio/`, `Annotations/Notes/`, and `vocadito_metadata.csv`.

```sh
cargo test --locked -p wave-tune-bench
cargo run --locked -p wave-tune-bench -- transcribe \
  --vocadito /path/to/vocadito --yin --json vocadito-yin.json
```

The command reads every track in the metadata and returns an error naming any missing audio or A1/A2 note
CSV. The report goes to stdout and optionally to `--json FILE`; progress goes to stderr. Use `--yin` to
select YIN, or `--rmvpe MODEL.onnx` to select an explicit RMVPE model. The default loads the installed
RMVPE model when available and otherwise uses YIN. Passing both `--yin` and `--rmvpe` is an error. The
`backendRecordings` counts report the backend that actually analyzed each recording, including YIN
fallbacks. Use `--root DIR` for Annotated VocalSet and its `--limit`, `--bpm`, `--basic-pitch`, and
`--dump` options.

Each recording is analyzed once. Both annotators score the same raw tracker notes in source time. The
report sums note counts across tracks before computing precision, recall and F1. The denominators are
predicted notes for precision, reference notes for recall, and their sum for F1 (with twice the matched
notes as numerator). COnP uses the existing one-to-one match within 50 ms of onset and 50 cents of pitch.
COnPOff also requires the offset to be within the larger of 50 ms and 20 % of the reference duration. A
ratio with a zero denominator is `null`, with counts still present.

A repeat is a consecutive pair of notes on the same rounded MIDI key with a gap from −20 to +120 ms. Both
notes must have distinct COnP matches for the pair to be preserved. A pair is shared when it occurs as a
repeat in both annotations and the A1-to-A2 COnP match links its two corresponding notes. A shared pair is
preserved when the two annotations link each corresponding note to the same predicted note.
Maximum-cardinality matching uses onset, pitch, then offset to order both sides. It visits references in
that order, prefers the first free candidate, and can reassign matches along an augmenting path. Repeat
recall uses preserved pairs over reference pairs for A1, A2, and shared pairs separately.

## See also

- [Working with coding agents](agents.md): the vocal correction workflow step by step
- [Audio clips](audio-clips.md): key detection and stem extraction
- [Mixing](mixing.md)
- [Recording vocals in the app](../guide/recording-vocals.md)
