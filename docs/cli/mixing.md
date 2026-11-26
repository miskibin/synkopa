# Mixing

Put effects on mixer inserts, route and sidechain them, let the mix assistant find frequency clashes,
save effect chains as presets, host VST3/LV2 plugins and automate parameters.

An *insert* is a mixer track; its id is in `synkopa context` under the inserts. Effects and plugins on an
insert have their own ids (`fxId`).

## Quick start

```sh
synkopa context --pretty                                    # inserts, their effects and parameters
synkopa add-fx --insert 6 --type reverb --preset 3          # → {"fxId":1203,…}
synkopa set-param --fx 1203 --param mix --value 30
synkopa sidechain --from 1 --to 3                           # the kick (insert 1) ducks the bass (insert 3)
synkopa mix                                                 # frequency collisions, worst first
```

## Add and adjust effects

Why: build an insert's chain without opening the mixer.

```sh
synkopa add-fx --insert 6 --type reverb --preset 3          # the reply has the new fxId
synkopa set-param --fx 1203 --param mix --value 30          # one parameter
synkopa preset --fx 1203 --index 4                          # load a factory preset
```

`--type` takes a built-in effect type such as `eq`, `comp`, `gate`, `filter`, `sat`, `delay`, `half`,
`warp`, `chorus`, `phaser`, `stereo`, `multiband`, `analyzer`, `reverb` or `limit`. `synkopa catalog` lists
every type with its parameter ids, ranges, units and presets.

## Route inserts

Why: build buses (all drums into one compressor) and send effects (a shared reverb).

```sh
synkopa route --from 2 --to 1 --only              # Hats only into the Drums bus, no longer to the master
synkopa route --from 6 --to 8 --gain 0.35         # Vocal also sends to a reverb bus
synkopa unroute --from 6 --to 8                   # remove that send
synkopa route --from 2 --to 0                     # to the master again
synkopa unroute --from 2 --to 0                   # stop going to the master
```

Sends are post-fader. `--gain` is 0..2. Insert `0` is the master.

## Sidechain: make one sound duck another

Why: the classic pump, or a bass that gets out of the kick's way.

```sh
synkopa sidechain --from 1 --to 3                           # the kick (1) ducks the bass (3)
synkopa sidechain --from 1 --to 3 --off                     # the bass's compressor hears itself again
synkopa sidechain --fx 1204 --from 1                        # this effect listens to insert 1
synkopa sidechain --fx 1204 --off
synkopa sidechain --from 1 --to 3 --label "Duck the bass"   # name the undo step (default "Sidechain")
```

- `--from A --to B` keys B's compressor from A, or adds a compressor set up for ducking if B has none.
- `--fx ID --from A` sets what one compressor, gate, filter or EQ (its dynamic bands) listens to.

## Find and fix frequency collisions

Why: two parts that are loud in the same frequencies at the same time mask each other (a kick and an 808
at 50 Hz). The mix assistant finds those pairs and can carve room with a dynamic EQ.

```sh
synkopa mix                           # collisions, worst first (numbered 0, 1, …)
synkopa mix --from 64 --to 128        # only those 16 bars (an offline pass)
synkopa mix fix 0                     # fix collision 0
synkopa mix ignore 1                  # stop reporting pair 1
synkopa mix off                       # the app's live detector (on by default); `mix on` turns it back on
```

**What it reports.** Pairs of mixer inserts that are both loud in the same third-octave bands at the same
time, sustained over bars. Inserts that feed each other (a kick and its drum bus) are never compared. Each
collision names the **masker** (the steadier sound, which should give way), the **protect**ed insert and
the range in Hz. Listing changes nothing.

**Where it listens.** In the app the detector listens while you play. On a project file, or with `--scan`,
it renders the song (or `--from`/`--to`, given together) once offline.

**What `fix` does.** It adds an ordinary EQ to the end of the masker's chain: one bell over the range,
flat until the protected insert gets loud there, then down by up to 6 dB. The EQ is keyed through the
standard sidechain (the protected insert, as `synkopa sidechain --fx ID --from ID` sets it), so it shows in
the chain and on the mixer's SC cables and you edit, bypass or remove it like any EQ. A pair handled this
way, or by a compressor keyed from the other insert, is not reported again. One undo step (named
"Make room").

**`ignore`** keeps the pair out of the report. It is saved in the project and can be undone.

## Save and reuse FX chains

Why: your vocal chain, your drum bus chain: build it once and load it on any insert of any project.

```sh
synkopa fxchain list                                     # names and the effects in each
synkopa fxchain save "Vocal chain" --insert 3            # --force replaces a preset of that name
synkopa fxchain load "Vocal chain" --insert 5 --mode replace
synkopa fxchain rename "Vocal chain" --to "Lead vocal"
synkopa fxchain delete "Lead vocal"
```

A preset holds an insert's effects in order, with their settings and VST3 state. Loading is one undo step
and never changes the insert's volume, pan, name, colour or routing. An insert that already has effects
needs `--mode replace` or `--mode append`. In the app: the **Presets** button above an insert's effects,
or the insert's right-click menu.

## Use VST3 and LV2 plugins

Why: host the plugins you already own. VST3 works everywhere, LV2 on Linux.

```sh
synkopa plugins                     # installed effects (VST3 from the cached scan, LV2 on Linux) and VST3 instruments
synkopa plugins --rescan            # scan again
synkopa plugin-add --insert 2 --path "C:\…\Pro-Q 3.vst3" --cid 72C4DB…                  # VST3
synkopa plugin-add --insert 2 --format lv2 --cid http://gareus.org/oss/lv2/dpl#stereo   # LV2: the URI
synkopa plugin-params --fx 1210 --query freq
synkopa plugin-set --fx 1210 --id 42 --value 0.3
synkopa plugin-load-anyway --fx 1210
```

- `plugins` replies `{plugins: [{format, path, cid, name, vendor, category, unsupported?}], instruments,
  unavailable: {lv2: "why"}}`.
- A VST3 plugin needs its `--path` and class id `--cid`; an LV2 plugin needs only its URI as `--cid`.
- `plugin-params` lists `{id, name, value, display, def}` for VST3. LV2 adds `symbol`, `real`, `min`,
  `max`, `unit`, `steps`, `toggle`, `integer`, `log` and `options`; there `id` is the control port index.
- `plugin-set` values are normalised 0..1 (for LV2, over the port's range).
- A plugin channel's id works wherever an effect id does (see
  [Patterns and notes](patterns-and-notes.md#add-and-set-up-channels)).
- **`plugin-load-anyway`** loads a plugin that Synkopa's safety check refused because it crashed or hung
  when tried in a separate process. The choice holds for every project from then on.
- The older names `vst3`, `vst3-add`, `vst3-params` and `vst3-set` still work, for VST3 only.

## Automate a parameter

Why: draw a filter sweep, a volume ride or a reverb throw from a script.

```sh
synkopa auto-add --fx 1210 --param 42 --points "0:0.2,8:0.8" --name "Reverb decay"   # a plugin parameter
synkopa auto-add --target insertVol --id 3 --points "0:0.2,8:0.8"                    # an insert fader
synkopa auto-add --target mix --fx 1210 --value 0.4                                  # an effect's mix, one value
```

Each call adds an automation envelope plus the Playlist clip that plays it, as one undo step.

- `--points "t:v,…"`: time in beats, value 0..1. `--value V` instead sets one constant value.
- Targets: a plugin parameter (`--fx ID --param ID`, the default), an effect's mix (`--target mix --fx ID`),
  or a fader or pan (`--target insertVol|insertPan|trackVol|chanVol|chanPan --id ID`).
- `--track N` picks the Playlist track for the clip (default 0); `--name` names the envelope
  ("Automation").

## Technical notes

- FX chain presets are stored one `.wavechain` file per preset in `%LOCALAPPDATA%\WAVE\FX Chains`
  (`~/Library/Application Support/WAVE/FX Chains` on macOS, `~/.local/share/WAVE/FX Chains` on Linux).
- `mix ignore` is stored in the project as `project.maskIgnore`. The fix EQ's key is `fx.key` = the
  protected insert.
- `auto-add` takes the new ids from `context.nextId` and writes `putAutomation` and `putClips` ops.
- Effects and plugins also work on project files with `-p`: VST3 effects load from the scan cache, LV2
  effects from the installed bundles, and plugin state is saved in the file.

## See also

- [Recording and vocals](recording-and-vocals.md#auto-vocal-a-tuned-chain-in-one-call): Auto vocal builds a
  whole vocal chain
- [Listening: render and stems](listening.md): check the result
- [Effects](../effects/README.md): what each built-in effect does
- [Mixing in the app](../guide/mixing.md)
