# Audio clips

Commands that look inside audio on the Playlist: detect its tempo, key and chords, split it into stems,
bounce clips in place, and fix sources whose files went missing.

## Quick start

```sh
synkopa context                                  # find the clip id (Playlist clips) and source ids
synkopa analyze --clip 300                       # tempo and key of a sample or beat
synkopa chords --clip 300 --to-pattern           # its chords as a new pattern you can play over
synkopa stems --clip 300 --only vocals,drums     # pull the vocal and drums out onto their own tracks
```

## Detect tempo and key

Why: match the project to a sample, or tune a vocal to the beat's key.

```sh
synkopa analyze --clip 300            # one audio clip
synkopa analyze                       # the project's audio clips together
synkopa analyze --exclude-fx 1203     # without the tracks routed to that effect's insert
```

Without `--clip`, muted tracks and clips are left out. `--exclude-fx` names an effect (for example a vocal's
pitch correction) and leaves out the tracks routed to its insert, so the key comes from the backing, not
the vocal.

For a clip the reply has the tempo (BPM, with ½× and 2× candidates) and the musical key (top 3
candidates, the 12-note chroma and a tuning offset). For the whole project it
reports the key.

## Detect chords

Why: learn the harmony of a sample or a reference track, or play your own parts over it.

```sh
synkopa chords --install                               # download the ChordMini model once (~17 MB, MIT)
synkopa chords                                         # model install state
synkopa chords --clip 300                              # the chords of an audio clip
synkopa chords --clip 300 --simple                     # major and minor only
synkopa chords --clip 300 --to-pattern                 # as block chords in a new pattern on the Playlist
synkopa chords --clip 300 --to-pattern --from 16 --to 48 --channel 1003
```

- Each chord has `start` and `end` (beats, on the project's grid), `root` (0 = C), `quality` (`maj`, `min`,
  `7`, `maj7`, `m7`, `sus2`, `sus4`, `dim`), `name` (for example "Bbmaj7", spelled for the clip's key) and
  `confidence`. Chords follow the clip's own pitch shift.
- `--to-pattern` writes the whole clip by default, on a new piano channel; `--from`/`--to` (beats) limit
  the range and `--channel ID` picks an existing channel. One undo step.
- The model download is checked against its SHA-256.

## Turn a sung or played melody into MIDI

Why: get a vocal or instrument line into the Piano roll, to double it with a synth or fix its notes.
This is the Playlist's **Convert to MIDI…**: a new pattern under the clip, on a new piano channel (or the
channel you pick), in one undo step.

```sh
synkopa audio-to-midi --clip 412 --preview                         # the notes it would write; nothing changes
synkopa audio-to-midi --clip 412 --quantize 1 --low 48 --high 84   # snap to 1/16, keep keys C3–C6 only
synkopa audio-to-midi --clip 412 --channel 1003 --velocity 0.8     # onto an existing channel, one velocity
```

The notes are the pitch editor's (GAME's when it is installed, see
[Better note detection](recording-and-vocals.md#better-note-detection-game)), with the clip's pitch edits
applied. `--shortest` (seconds, default 0.06) leaves out shorter notes; `--quantize` is in steps (0 = as
sung).

## Extract stems from a clip

Why: take the vocal out of a song, or the drums out of a loop, and mix each part on its own track.

```sh
synkopa stems --install                         # download the HTDemucs model once (~90 MB, MIT)
synkopa stems                                   # model install state
synkopa stems --clip 5012                       # drums, bass, other and vocals
synkopa stems --clip 5012 --only vocals,drums   # only some
```

This does exactly what the Playlist's **Extract stems…** does:

- The region the clip plays, plus 3 s on either side, is separated with HTDemucs.
- Each stem is written next to the project's audio as a WAV (44.1 kHz, 32-bit float), goes on a new track
  right below the clip, and gets a clip at the same place playing the same part with the same stretch,
  pitch, gain, fades and pan.
- The original clip is muted. The whole operation is one undo step.

It takes about 25 s per minute of audio on a 6-core CPU and up to about 2.7 GB of memory while it runs.

## Give a clip its own sample

Why: edit one use of a shared sample without changing the others.

```sh
synkopa make-unique --clip 5012          # or several: --clip 5012,5013
```

FL Studio's *Make unique*. The clips (one sample, one track) get their own copy of the source, with its own
name ("Name #2") and colour, and move to a new track right below theirs. Nothing is written to disk: the
copy shares the file. The new track plays where the old one does; no mixer insert is claimed for it. Other
clips of the sample keep it. One undo step.

## Bounce clips in place

Why: freeze clips with their mixer effects into one audio clip, to save CPU or to edit the result as audio.

```sh
synkopa export-in-place --clip 5012,5013          # these clips, with their mixer effects
synkopa export-in-place --from 8 --to 24          # everything that plays in beats 8..24
synkopa export-in-place --from 8 --to 24 --clip 5012
```

The result is one audio clip on a new track below; the originals are muted (one undo step). With a time
selection, clips crossing its edges are split and only the inside part is muted. `--from` and `--to` go
together.

## Find missing audio files

Why: a project moved to another computer or folder plays silence where a file is missing.

```sh
synkopa missing                                          # sources whose audio file is missing
synkopa relink --folder D:/Backup/Samples                # search there (and below) by name
synkopa relink --source 1010 --path D:/new/pad.wav       # point one source at another file (one undo step)
```

`relink --folder` relinks each unambiguous match of the remembered length and lists the rest.

## Technical notes

- `WAVE_STEMS_MODEL=path` points at an HTDemucs model you already have. Downloaded models live in
  `Models` under Synkopa's data folder (`%LOCALAPPDATA%\WAVE` on Windows).
- Internal command names (for `synkopa call`): `music_analyze`, `chords_analyze`, `chords_to_pattern`,
  `chords_model`, `chords_model_install`, `extract_stems`, `stems_model`, `stems_model_install`,
  `make_unique`, `export_in_place`, `missing`, `relink`.

## See also

- [Recording and vocals](recording-and-vocals.md)
- [Listening: render and stems](listening.md): exporting stems of your own mix
- [Patterns and notes](patterns-and-notes.md)
