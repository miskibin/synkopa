# Patterns and notes

Write beats and melodies from the terminal: add instruments to the Channel Rack, fill patterns with step
strings or piano-roll notes, and generate or transform notes with deterministic tools.

## Quick start

```sh
synkopa channels                                              # ids, instruments, routing
synkopa pattern --new --name Fill --bars 1                    # a new pattern; its id is in the reply
synkopa pattern --id 2 --ch hat --steps "x.x. x.x. x.x5 x.x9" # replace the hat's notes
synkopa pattern --id 3 --ch bass --notes '[{"step":0,"len":3,"p":33},{"step":4,"len":2,"p":40,"v":0.8}]'
synkopa pattern --id 2 --play                                 # loop it on its own in the app
```

## How patterns work

- A *channel* is a Channel Rack row: a sampler, a synth, Synkopa Instruments, the Music Box or a plugin
  instrument. Channels are shared by all patterns; a note plays the channel named by its `ch`.
- Steps are 16th notes.
- A pattern is as long as its notes: the end of the last note, rounded up to a bar, as in FL Studio.
  `--bars N` sets the shortest it may be, for empty bars you keep on purpose (a crash every second bar).
- Swing is 0–100 %: it delays every odd 16th by up to half a step.
- `--ch` takes a channel id or name (`kick`, `Hats 2` …; case does not matter).
- Every edit is one undo step.

## Add and set up channels

Why: a pattern can only play channels that exist in the Channel Rack.

```sh
synkopa channels --add snare-studio                  # a sampler with a standard-library sample
synkopa channels --sample 1001                       # a sampler playing project source 1001
synkopa channels --synth "808 Deep"                  # a synth channel with a factory preset ("" = blank patch)
synkopa channels --keys                              # Synkopa Instruments: the sampled grand piano
synkopa channels --keys --instrument strings         # another Synkopa Instruments instrument
synkopa channels --musicbox                          # the Music Box, the modeled tine instrument
synkopa channels --plugin serum                      # a scanned VST3 instrument
synkopa channels --remove 1004                       # by id or name; also removes its notes
```

- `synkopa channels` alone lists the Channel Rack: id, name, what it plays (`sample` with its source and
  sampler settings, `synth` with its preset, `keys` …), volume, pan, mute/solo, mixer insert (`null` = the
  insert of the clip's track), root key and note count.
- `--add` takes a standard-library file name or display name, or the start of one (`kick` picks the first
  kick). The browser's Library tab lists them all.
- `--plugin` takes a name, the start of one, or the class id. Run `synkopa plugins --rescan` first if the
  plugin is new.
- Instrument ids for `--instrument` are in `synkopa catalog` under `keys`; synth presets under
  `effects.synth`.

### Change a channel's sound

```sh
synkopa channels --ch 1005 --preset "Pad Warm"                     # load a synth preset
synkopa channels --ch 1005 --param flt_cutoff --value 900          # one synth knob (ids in `catalog`)
synkopa channels --ch "String Section" --articulation pizzicato     # a Synkopa Instruments articulation
synkopa channels --ch "Grand Piano" --param release --value 600    # a Synkopa Instruments knob
synkopa channels --ch "Music Box" --param ring --value 140         # a Music Box knob
synkopa channels --ch "Studio Snare" --param cut --value 1         # a sampler setting
synkopa channels --ch Kick --group Drums                           # Channel Rack filter group ("" = none)
synkopa plugin-params --fx 1007                                    # a plugin channel's parameters
```

`--param` names by channel type:

| Channel | Parameters |
| --- | --- |
| Sampler | `start`, `end` (0..1 of the file); `reverse`, `norm`, `env` (0 or 1); `semi`, `fine`; `delay`, `attack`, `hold`, `decay`, `release` (ms); `sustain` (0..1); `attackTension`, `decayTension`, `releaseTension` (−1..1); `fadeInMs`, `fadeOutMs` (source milliseconds); `cut` (cut group, 0 = none, 1..8) |
| Synkopa Instruments | `release`, `attack` (ms; `release` 0 = the instrument's own); `dynamics`, `width`, `noise` (%); `tone` (−100..100); `tune` (cents) |
| Music Box | `ring`, `brightness`, `body`, `mechanical`, `spread` (%); `release` (ms) |
| Synth | Any synth parameter id from `synkopa catalog` |

A plugin channel's id works wherever an effect id does: use `plugin-params --fx ID` and
`plugin-set --fx ID` (see [Mixing](mixing.md#use-vst3-and-lv2-plugins)).

## Write a pattern

Why: step strings are the fastest way to write drums; JSON notes give you pitch, length and velocity.

```sh
synkopa pattern --id 2                                         # read: one step string per channel
synkopa pattern --id 2 --ch hat --steps "x.x. x.x. x.x5 x.x9"   # replace the hat's notes
synkopa pattern --id 2 --ch kick --steps "x... x... x... x..." --vel 0.9
synkopa pattern --id 3 --ch bass --notes '[{"step":0,"len":3,"p":33},{"step":4,"len":2,"p":40,"v":0.8}]'
synkopa pattern --id 2 --ch hat --clear                        # remove the hat's notes
synkopa pattern --id 2 --bars 2 --swing 55 --name "Hats B"
synkopa pattern --new --name Fill --bars 1                     # its id is in the reply
```

**Step strings:** `x` is a hit at full velocity (or `--vel`), `1`–`9` are 10–90 % velocity, `.` is empty.
Spaces and `|` are ignored, so write a space every beat. Reading a pattern returns the same strings plus
`notes` for melodic rows, `bars`, the minimum bars and swing %.

**Notes JSON:** `step` and `len` in 16ths, `p` the MIDI key (60 = C4), `v` velocity 0..1.

**Triggers and held notes.** `--steps` writes *triggers*: a sampler trigger rings out even with the
envelope on. `--notes` writes *held notes* by default; on a sampler a held note stops at its length or at
the end of the sample, whichever comes first. Add `"trig":true` to a note to make it a trigger in a mixed
row. When a row has held notes or cannot be written as a step string, the reply's `notes` array carries
`trig` for each event.

### Loop a short channel under a longer pattern

Why: a 3-step shaker under a 16-step kick gives a polyrhythm without writing every hit (FL 20.1's
channel looping).

```sh
synkopa pattern --id 2 --ch shaker --steps "x.x" --loop 3   # its first 3 steps repeat to fill the pattern
synkopa pattern --id 2 --ch shaker --loop off               # clear the loop (0 works too)
```

The loop is anchored to the start of the pattern. Setting it removes that channel's notes from step `N`
on, in the same undo step, so nothing stays hidden. Clearing it keeps what is in the looped steps. A
looped channel shows `"loop": N` in the reply's rows. The pattern's length and its Playlist clips do not
change because of a loop. With `--play`, `--loop A:B` keeps its other meaning: the beat range to preview.

### Slide notes (808 glides)

Why: an 808 that bends up to the next note is a slide, as in FL Studio.

`"slide":true` makes a slide note. It starts no sound: the channel's notes that are sounding when it starts
glide by the distance from the highest of them to its key `p`, over its `len`, and keep that pitch until
they end. Works on sampler, Synkopa Instruments and Synth channels; plugin instruments ignore it, and MIDI
export leaves it out. An 808 jumping an octave up for an eighth note:

```sh
synkopa pattern --id 3 --ch 808 --notes '[{"step":0,"len":8,"p":36},{"step":4,"len":2,"p":48,"slide":true}]'
```

### Hear one pattern

```sh
synkopa pattern --id 2 --play                        # loop it on its own (stops the song)
synkopa pattern --id 2 --play --from 4 --loop 4:8    # from beat 4, looping beats 4–8 of it
synkopa pattern --stop
```

This needs the running app. On a file, render it instead: `render --pattern 2 --out p.wav`.

## Generate and transform notes

Why: humanize, quantize, fill rhythms or write chords in one command. Every verb is one undo step and is
deterministic in `--seed`: the same command on the same pattern gives the same notes, so you (or an
agent) can make variants, render each and keep the favourite.

```sh
synkopa notes humanize --pattern 1 --timing 0.3 --velocity 0.2
synkopa notes quantize --pattern 1 --grid 1 --amount 1 --scale      # --scale: also into the project key
synkopa notes euclid --pattern 1 --ch 1001 --k 5 --n 16 --rotate 2  # E(k,n) rhythm for one channel
synkopa notes strum --pattern 1 --width 0.5 --ramp 0.05
synkopa notes chop --pattern 1 --len 1 --select "0:64@1001"         # FL's chop: pieces a 16th long
synkopa notes glue --pattern 1                                       # FL's glue: touching notes of one key join
synkopa notes arp --pattern 3 --mode upDown --rate 1 --gate 0.9 --octaves 2   # chords become arpeggios
synkopa notes drums --pattern 2 --bars 2 --seed 42 --voice "1001:4:16:0" --voice "1002:2:16:8"
synkopa notes melody --pattern 3 --ch 1003 --bars 2 --seed 7 --density 0.5
```

| Verb | What it does | Options (defaults) |
| --- | --- | --- |
| `humanize` | Random timing and velocity drift | `--timing` 0..1 (0.5), `--velocity` 0..1 (0.4) |
| `quantize` | Snap note starts to the grid; `--scale` also snaps pitches into the project key | `--grid` 1 = 16th, 2 = 8th, 4 = quarter (1); `--amount` 0..1 (1) |
| `euclid` | Spread K hits over N steps for one channel | `--ch`, `--k`, `--n`, `--rotate` (0), `--vel` (0.7) |
| `strum` | Spread notes that start together by pitch, so chords become strums | `--width` steps (0.5), `--ramp` velocity (0.05) |
| `chop` | Cut held notes into pieces `--len` steps long (at least 0.25; the last piece keeps the rest) | `--len` (1) |
| `glue` | Join touching or overlapping notes of one channel and key (undoes `chop`) | — |
| `arp` | Turn each chord (notes starting together on a channel) into a run of its keys every `--rate` steps for as long as it lasts | `--mode` up, down, upDown, random (up); `--rate` (1); `--gate` of a step (0.9); `--octaves` 1–4 (1) |
| `drums` | Euclidean drum voices; each `--voice` is `CH:K:N[:ROT[:VEL]]` (repeatable) | `--bars` (2), `--seed` |
| `melody` | A melody for one channel | `--ch`, `--bars` (2), `--density` 0..1 (0.5), `--repeat` (1), `--seed` |

Step triggers stay as they are under `arp`. One run makes at most 20 000 notes.

**Limit what a verb touches** with `--select "FROM:TO"` (a step range), `--select "FROM:TO@CH"` (also one
channel) or `--indices "1,2,5"` (exact notes).

```sh
synkopa notes humanize --pattern 1 --select "0:16"
```

## Write chords

Why: stamp a chord, harmonize a melody, or generate a progression that stays in key.

```sh
synkopa key A minor                                                  # the key diatonic tools spell from
synkopa notes stamp --pattern 3 --ch 1003 --step 0 --root A3 --type min7 --inv 1
synkopa notes chord --pattern 3 --type diatonic                      # harmonize single notes (A3 → Am)
synkopa notes chord --pattern 3 --select "0:16" --type min7          # spelled by interval, any key
synkopa notes progression --pattern 4 --ch 1003 --bars 4 --template minloop --seed 7
synkopa notes progression --pattern 4 --ch 1003 --degrees "0:5:2:6" --no-voice-lead
```

- **`stamp`** writes one chord at `--step` and replaces what was on that step. `--type` is one of 18
  types: `maj`, `min`, `dim`, `aug`, `sus2`, `sus4`, `7`, `maj7`, `min7`, `9`, `add9`, `maj9`, `min9`, `6`,
  `m6`, `dim7`, `m7b5`, `5`. Optional inversion `--inv` 0..3, `--len` in steps (4). `--root` takes a MIDI key or a note name like `A3`
  (C4 = 60).
- **`chord`** harmonizes the selected single notes. `--type diatonic` (the default) spells the triad from
  the project key and leaves out-of-scale notes alone; named types (`maj`, `min`, `7`, `maj7`, `min7`)
  spell by interval in any key.
- **`progression`** writes one diatonic 7th chord per bar. Templates: `pop`, `pop2`, `minloop`, `trap`,
  `iivI`, `doowop`, `andalusian`, `blues`, `canon`, `chance`. `--bars` 1..32 (4). `--degrees "d:d:d"`
  replaces the template and bar count with your own degree list (at most 32). Inversions are voice-led to
  move the least unless `--no-voice-lead`.

## Technical notes

- Ops through `apply`: `putPattern`, `putChannel`, `removeChannel`, and `transformNotes` for the `notes`
  verbs (see `synkopa schema`).
- When a pattern's length changes (notes added past its end or removed, `--bars`), Playlist clips that
  show exactly one whole pass of it follow. A growing clip stops at the next clip on its track (it never
  overlaps it); a shrinking pattern cuts back clips that would otherwise loop it. Clips you sized on
  purpose keep their length.
- `--seed` is an unsigned 64-bit integer.
- `pattern --color "#e0a44a"` sets a pattern's colour.

## See also

- [Synkopa Instruments](../instruments/synkopa-instruments.md), [Music Box](../instruments/music-box.md),
  [Synth](../instruments/synth.md)
- [Audio clips](audio-clips.md): turn detected chords into a pattern
- [Reading and editing](reading-and-editing.md)
