---
name: synkopa
description: Make and edit music in Synkopa, the open-source DAW with the FL Studio workflow. Use when asked to make a beat, write drums, bass, chords or a melody, arrange a song, add or tune effects, set up sidechain or routing, tune or mix a vocal, separate stems, detect chords, or render and check audio in a Synkopa project (.synkopa file or the open app).
---

# Synkopa

Synkopa is a DAW: Channel Rack (instruments), patterns (notes per channel), Playlist (clips on
tracks), mixer (inserts with effects). You drive it through its command line, which returns JSON.

## How to call it

- **With the Synkopa tools** (`synkopa_context`, `synkopa_run`, `synkopa_apply`, `synkopa_script`):
  use them. `synkopa_run` takes the words of a command as an array: `["bpm", "140"]`.
- **Without them**: run `synkopa <command>` in a shell. Same commands, same JSON on stdout,
  messages on stderr, exit code 0 ok, 1 usage, 2 app not running, 3 refused.

Commands below are written as shell lines; with the tools, each word is one array item.

## Where it acts

- **The open app** (default): edits the project the musician sees. Every command is one undo
  step for them (`Ctrl+Z`). Nothing is saved until they save.
- **A file** (`-p /abs/song.synkopa`, or the tools' `project`): loads the file, runs, saves it
  back. No app needed. `synkopa new /abs/song.synkopa --template empty|sketch|demo` creates one.
  `play`, `stop`, `undo` and `redo` need the app (`NEEDS_APP`); a file the app has open is
  refused (`FILE_OPEN`): edit it through the app instead.

If the app is not running and no file was named, ask which song to work on, or create a file.

## Always

1. **Read first**: `synkopa context`. Note `revision`, `nextId`, the pattern ids, channel ids
   and names, insert ids. Do not guess ids.
2. **Look up, do not invent**: `synkopa help` (every command), `synkopa catalog` (effect types,
   parameter ids and ranges, presets, instruments, synth presets), `synkopa schema` (ops for
   `apply`).
3. **One musical change per step**, named for the musician when you can (`--label "Busier hats"`),
   so their undo history reads well. Related ops go in one `apply` batch: it is atomic.
4. **Listen with numbers**: `synkopa render --out /abs/check.wav` (add `--from B --to B` or
   `--pattern ID`) returns peak, RMS, loudness and clipping. Check it after anything that changes
   level. Clipping or a peak above 0 dBFS means turn something down.
5. **Say what you changed** and that `Ctrl+Z` takes it back. Do not save the project in the app
   unless asked.

`REVISION_CONFLICT` means the musician edited meanwhile: read `context` again and decide; never
retry blindly. Pass `--if-revision N` on edits that depend on what you read.

## Units

- Playlist time: **beats** (4 = one bar). Pattern `step` and `len`: **sixteenths** (16 = one bar).
- Pitch `p`: MIDI key (60 = C4). Velocity `v`: 0..1.
- Track and insert `vol`: fader position, 0.7937 = 0 dB. Clip `gain`: dB. Pan: -1..1.
- New ids count up from `context.nextId`.

## A beat from nothing

A new project already has four drum channels (Kick, Clap, Hat, Snare) and an empty pattern 1.

```sh
synkopa bpm 140
synkopa key A minor                        # scale-aware tools use it
synkopa channels                           # ids and names: use the ones you see
synkopa channels --synth "808 Deep"        # a bass; synth presets are in `catalog`
synkopa channels --add kick-808            # more drums: a library sample by name or its start
synkopa pattern --id 1 --ch Kick --steps "x... ..x. ..x. ...."
synkopa pattern --id 1 --ch Clap --steps ".... x... .... x..."
synkopa pattern --id 1 --ch Hat  --steps "x.x. x.x5 x.x. x7x9"
synkopa pattern --id 1 --ch "808 Deep" --notes '[{"step":0,"len":6,"p":33},{"step":10,"len":4,"p":36}]'
synkopa render --pattern 1 --out /abs/beat.wav
```

Step strings: `x` full velocity, `1`-`9` = 10-90 %, `.` empty; spaces are ignored, so group by
beat. `--steps` writes triggers (drums); `--notes` writes held notes (bass, chords, melody).
`"slide":true` on a note makes an FL-style glide (808s). `--ch` takes a channel id or name.
`pattern --new --name Fill --bars 1` makes another pattern; its id is in the reply.

## Useful moves

- **Variation**: `notes humanize|quantize|euclid|strum|chop|glue|arp --pattern ID …`. All take
  `--seed`, so a variant is reproducible: render a few, keep the best.
- **Harmony**: `notes progression --pattern ID --ch CH --bars 4 --template minloop`,
  `notes chord --pattern ID --type diatonic`, `notes stamp … --root A3 --type min7`.
- **Arrangement**: clips on the Playlist are `apply` ops (see `schema`); name sections with
  `markers --add Hook --bar 17`.
- **Mix**: `add-fx --insert ID --type eq|comp|reverb|… [--preset N]`, `set-param --fx ID --param
  NAME --value V`, `route --from ID --to ID`, `sidechain --from KICK --to BASS`, `mix` (finds
  inserts that mask each other) and `mix fix N`.
- **Audio**: `import file.wav --track N --beat B`, `analyze --clip ID` (tempo and key),
  `stems --clip ID`, `chords --clip ID --to-pattern`, `audio-to-midi --clip ID`.
- **Vocals**: `auto-vocal --clip ID [--beat-clip ID]` builds a tuned chain; `pitch inspect`,
  `pitch suggest` and `pitch edit` work note by note. `--plan` and `suggest` change nothing: read
  them before editing.
- **Deliver**: `render --out mix.wav`, `render --stems --out stems/`, `midi export --out song.mid`,
  `export-project --out song.zip`.

Some commands download a model on first use (`stems --install`, `chords --install`,
`f0-model --install`, `note-model --install`): tens to hundreds of MB, each with its own licence
note. Ask before installing one.

## Taste

A request like "make it hit harder" is a musical question, not a parameter. Make a small,
audible change, render, compare the numbers, and tell the musician what to listen for. Prefer
fewer, deliberate edits over many: they will hear and undo each one. Leave their own parts alone
unless asked to change them.

Full reference: https://github.com/miskibin/synkopa/blob/main/docs/cli.md
