# Synkopa Instruments

A sampled-instrument player as a Channel Rack channel (`kind: "keys"`), laid out like a sampler
plugin: the chosen instrument's face (its artwork, knobs and articulations) and a library of every
instrument as cards. Add it from the
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
| Hollow Body Guitar | `hollow-body-guitar` | Gretsch Anniversary, clean: every semitone E2–D6, 3 dynamics × up to 4 round robins, staccato, hand-damping noises | [Black And Green Guitars](https://github.com/sfzinstruments/karoryfer.black-and-green-guitars), Karoryfer Samples (Brian Wood) | CC0 | 306 MB |
| Jazz Club Guitar | `jazz-guitar` | Höfner Club archtop: the same | same | CC0 | 211 MB |
| Vibraphone | `vibraphone` | soft mallets, motor off, 2 dynamics | [VCSL](https://github.com/sgossner/VCSL), Versilian Studios | CC0 | 42 MB |

Only the grand piano ships inside the app. The others are library kits: the instrument's
**Download** button (or Library → the kit) fetches the files once into the sample library, each
checked against the SHA-256 pinned in `assets-manifest.json`. A CC-BY instrument's credit is shown
under its knobs; a song that uses it should credit the recordings.

## The window and the project

The window opens on the instrument's face. **‹ ›** on the plate step to the previous / next
instrument while you play (FL's preset arrows); its name or **Instruments** opens the library:
every instrument as a card with its artwork, category filters (All, Pianos, Electric pianos,
Guitars, Strings, Mallets), and what is on this computer (**in app**, **Ready**, or a **Download**
button with its size). A click on a card makes the channel play it and keeps the library open, so
you can click through instruments with the keyboard below; a double click, Enter on the playing
card, **Back** or Esc returns to its face. Arrow keys move between cards, Enter picks.

Changing the instrument is one undo step. A channel still named after its instrument (a new channel
is named "Grand Piano") takes the new name. The release and the articulation belong to the old
instrument and are dropped; the other knobs stay. Knobs, in four sections:

| section | knob | range | what it does |
| --- | --- | --- | --- |
| Sound | Tone (`tone`) | −100 … +100, default 0 | darker or brighter: a shelf of ±9 dB over the highs above about 1.8 kHz |
| | Width (`width`) | 0 – 150 %, default 100 % | stereo width of the recordings (0 = mono). Dimmed for instruments recorded in mono (the electric pianos, the guitars): use Pan |
| | Tune (`tune`) | ±100 cents | fine tune, e.g. to match a recording |
| Envelope | Attack (`attack`) | 0 – 2 s, default 0 | a fade-in on every note, for swells and pads; it only slows an instrument's own attack |
| | Release (`release`) | Auto, or 40 ms – 3 s | how long a note rings after its key is let go (−60 dB at the end). **Auto** (2× click) = each region's own time as the instrument was mapped: the piano's dampers 0.5 s, its top two octaves (no dampers) 3–4 s, the strings 1.4 s |
| Playing | Dynamics (`dynamics`) | 0 – 100 %, default 100 % | how much the velocity changes the level; the timbre always follows it |
| | Noise (`noise`) | 0 – 100 %, default 100 % | level of the key-release noises (the piano's dampers, fingers leaving guitar strings). Dimmed for instruments without them |
| Channel | Volume, Pan | | the channel's own |

Every knob can be automated (right-click → automation clip). Tone, Width, Tune and Noise glide over
about 10 ms when turned or automated; Attack applies to notes that start after it changes.

### Guitars

The guitars play like a guitar, not like a keyboard. Each note is put on one of six strings in
standard tuning (E2 A2 D3 G3 B3 E4): the highest string that reaches it, so a chord spreads over
the strings the way a guitarist voices it (an open C, a G, an F barre each land one note per
string), and a note whose string is still held moves one string down. A string sounds one note:
a new note on it damps the one it was ringing over 30 ms, so repeated strums and fast lines do not
pile up into a chorus of the same string. Their articulations are **Sustain** (C2) and
**Staccato** (C♯2); on **Auto** a note shorter than 0.15 s plays the staccato recordings. The
mappings level each note against its neighbours (the recordings vary from note to note) and the
staccato against the sustain; `scripts/keys-kits.mjs` measures the files when it writes them.
For a natural strum, spread a chord's notes by a few milliseconds per string (the piano roll's
strum tool), lighter on the upstrokes.

Instruments with articulations (the strings) show them as buttons; the choice is saved with the
channel. The strings default to **Auto**, as modern string libraries do: a note shorter than
0.13 s plays spiccato, a longer one sustain (arco); a note played live, whose length is unknown when
it starts, plays sustain. Their keyswitches (C1–D#1 for the strings) are tinted on the keyboard and switch while
playing: a note on one picks the articulation instead of sounding, until the next edit. Keys outside
an instrument's range are dimmed (they still play, pitched from the nearest recording).

Project format: `{"kind": "keys", "keys": {"instrument": "strings", "dynamics": 100, "articulation": 26, "attack": 0, "tone": 0, "width": 100, "tune": 0, "noise": 100}}`
(projects without the last five play them at their defaults).
`instrument` defaults to `grand-piano`, `release` (ms) is absent for Auto, `articulation` absent for
the instrument's first one. Projects made before there was a choice (`{"release": 350, "dynamics":
100}`) play the grand piano with their saved release, as they did.

```sh
synkopa channels --keys --instrument wurlitzer
synkopa channels --ch "String Section" --articulation pizzicato
synkopa channels --ch "Grand Piano" --param release --value 600    # 0 = Auto
synkopa channels --ch "Grand Piano" --param tone --value -30        # attack, tone, width, tune, noise too
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
importing your own `.sfz` files, filters and LFOs from SFZ, legato transitions, and loops for the
strings: a sustained string note lasts as long as its recording (a few seconds, at most 9 s), not
as long as the note. Range and directional keyswitches (`sw_lolast`, `sw_up`, ...) are counted as
ignored opcodes, not played.
