# Synkopa Keys

A sampled-instrument player as a Channel Rack channel (`kind: "keys"`), laid out like a sampler
plugin: an instrument browser on the left, the chosen instrument on the right. Add it from the
Channel Rack's **+** menu or with `synkopa channels --keys`; it plays from the piano roll, the
typing keyboard and a MIDI keyboard (with the sustain pedal) like every other pitched channel, and
slide notes glide it.

## Instruments

| instrument | id | what it is | recordings | licence | size |
| --- | --- | --- | --- | --- | --- |
| Grand Piano | `grand-piano` | Yamaha C5, 4 dynamics, key-release noises | [Salamander Grand Piano V3](https://github.com/sfzinstruments/SalamanderGrandPiano), Alexander Holm | CC-BY-3.0 | ships in the app (33 MB) |
| Upright Piano | `upright-piano` | 3 dynamics, 2 round robins | [VSCO 2 CE](https://github.com/sgossner/VSCO-2-CE) Upright Nr1, Versilian Studios | CC0 | 155 MB |
| Wurlitzer EP200 | `wurlitzer` | electric piano | [E-Pianos](https://github.com/sfzinstruments/GregSullivan.E-Pianos), Greg Sullivan | CC-BY-3.0 | 21 MB (one kit with the next two) |
| Electric Grand CP80 | `cp80` | stringed electric grand | same | CC-BY-3.0 | |
| Pianet T | `pianet` | plucked-reed piano | same | CC-BY-3.0 | |
| String Section | `strings` | violins, violas, cellos: sustain, tremolo, pizzicato, spiccato | VSCO 2 CE, Versilian Studios | CC0 | 198 MB |
| Solo Violin | `solo-violin` | one violin: arco with vibrato, tremolo, pizzicato, spiccato | VSCO 2 CE, Versilian Studios | CC0 | 135 MB |
| Concert Harp | `harp` | rings out on its own | VSCO 2 CE | CC0 | 35 MB |
| Vibraphone | `vibraphone` | soft mallets, motor off, 2 dynamics | [VCSL](https://github.com/sgossner/VCSL), Versilian Studios | CC0 | 42 MB |

Only the grand piano ships inside the app. The others are library kits: the instrument's
**Download** button (or Library → the kit) fetches the files once into the sample library, each
checked against the SHA-256 pinned in `assets-manifest.json`. A CC-BY instrument's credit is shown
under its knobs; a song that uses it should credit the recordings.

## The window and the project

Clicking an instrument in the browser makes the channel play it (one undo step). A channel still
named after its instrument takes the new name; an articulation and a release set for the old
instrument are dropped. Knobs:

| knob | range | what it does |
| --- | --- | --- |
| Release (`release`) | Auto, or 40 ms – 3 s | how long a note rings after its key is let go (−60 dB at the end). **Auto** (2× click) = each region's own time as the instrument was mapped: the piano's dampers 0.5 s, its top two octaves (no dampers) 3–4 s, the strings 1.4 s |
| Dynamics (`dynamics`) | 0 – 100 %, default 100 % | how much the velocity changes the level; the timbre always follows it |
| Volume, Pan | | the channel's own |

Instruments with articulations (the strings) show them as buttons; the choice is saved with the
channel. The strings default to **Auto**, as modern string libraries do: a note shorter than
0.13 s plays spiccato, a longer one sustain (arco); a note played live, whose length is unknown when
it starts, plays sustain. Their keyswitches (C1–D#1 for the strings) are tinted on the keyboard and switch while
playing: a note on one picks the articulation instead of sounding, until the next edit. Keys outside
an instrument's range are dimmed (they still play, pitched from the nearest recording).

Project format: `{"kind": "keys", "keys": {"instrument": "strings", "dynamics": 100, "articulation": 26}}`.
`instrument` defaults to `grand-piano`, `release` (ms) is absent for Auto, `articulation` absent for
the instrument's first one. Projects made before there was a choice (`{"release": 350, "dynamics":
100}`) play the grand piano with their saved release, as they did.

```sh
synkopa channels --keys --instrument wurlitzer
synkopa channels --ch "String Section" --articulation pizzicato
synkopa channels --ch "Synkopa Keys" --param release --value 600   # 0 = Auto
synkopa catalog                                                       # "keys": the instruments and their articulations
```

## How it works

Every instrument is an SFZ mapping embedded in the program (`crates/engine/instruments/*.sfz`) over
the files of its kit. `crates/engine/src/sfz.rs` parses the part of SFZ the player supports:
`<control>/<global>/<master>/<group>/<region>/<curve>`, `#define` and `#include`; key and velocity
ranges, `pitch_keycenter`/`pitch_keytrack`, `tune`/`transpose`, `volume`/`amplitude`/`pan`
(and `global_/master_/group_volume`), `amp_veltrack` and `amp_velcurve_N`, velocity and key
crossfades, `trigger=release` with `rt_decay`, round robins (`seq_length`/`seq_position`,
`lorand`/`hirand`), the amplitude envelope (`ampeg_*`), `offset`/`end`, loops, `group`/`off_by`,
keyswitches with `sw_label`, and `*_onccN` modulation of those. CCs are not routed into a playing
instrument (except the sustain pedal): a CC modulation is evaluated at load from the CC's default
(`set_cc`/`set_hdcc`), the extended CCs 131 (velocity) and 133 (key) per note. Unknown opcodes are
counted, not fatal; every catalog instrument parses with none.

`crates/engine/src/keys.rs` loads the recordings an instrument names (decoded on a few threads,
cut at 9 s with a 1.5 s fade unless a region loops them) and keeps them as 16-bit PCM — half of
`f32`, far below the recordings' own noise. A note plays every region its key, velocity, round
robin and keyswitch select, resampled with cubic interpolation, under its ADSR; a sequenced note
schedules its key-release regions at its end, a live note plays them at its note-off (or when the
pedal lets it go). Voices come from the engine's shared pool of 128, oldest stolen first. The
bank is an `Arc` built off the audio thread; a voice only reads it. No allocation, lock or I/O in
the callback.

The app loads an instrument the first time a channel plays it and drops it when none does, so
browsing does not pile them up in memory (about 190 MB for the piano, 140 MB the strings, under
40 MB the others). An offline bounce (`synkopa render`, Export) waits for every instrument the
project plays and refuses to start silently when one is missing.

Levels: the same four-bar phrase rendered on every instrument sits at −28 ± 1.5 dBFS RMS
(`cargo run --release -p wave-engine --example keys_demo -- target/keys-kits target/keys-demo`).

## The grand piano's mapping

4 of the Salamander's 16 dynamics (layers 1, 6, 11, 16) at every minor third from A0 to C8, plus its
88 key-release recordings 40 dB down. Inside its velocity band a layer is levelled by
(velocity / the velocity it was played at)^k, with k measured from the level step to the next
layer (RMS of the first 0.5 s, mean over the 30 notes: 1.0, 0.95, 1.83), so the level is continuous
across a layer boundary and only the timbre changes.

The installer carries the recordings prepared, not as downloaded (33 MB instead of 185 MB):
`node scripts/fetch-keys.mjs` downloads the originals into `target/keys-src` (checksummed, reused
on the next run) and runs `wave-keys-prep` (`crates/keys-prep`), which applies the 9 s cut ahead of
time and stores 16-bit FLAC with triangular dither into `app/resources/keys` (not in Git); `tauri
build` ships that folder, and the release workflow runs the script. The tool decodes every file it
writes and stops the build if any sample is more than 1.6 steps of 16-bit from the cut original
(the largest difference on the current files is −86.8 dBFS). A build without the folder (a plain
`cargo build`) offers the download instead.

## Adding an instrument

`node scripts/keys-kits.mjs [kit ...]` builds the downloadable kits: for each it downloads the
recordings it plays (pinned to a commit, cached in `target/keys-kits`), writes their SHA-256 and
size into `assets-manifest.json`, and writes the instrument's SFZ mapping. Add the instrument to
`CATALOG` in `keys.rs`, level it with `keys_demo`, and run
`WAVE_WRITE_FIXTURES=1 cargo test -p wave-engine --test keys_catalog_fixture` so the browser mock
lists it. Only recordings whose licence allows redistribution qualify (CC0, CC-BY, public domain).

Not there (yet): streaming from disk (every recording is in memory while an instrument plays),
importing your own `.sfz` files, filters and LFOs from SFZ, legato transitions.
