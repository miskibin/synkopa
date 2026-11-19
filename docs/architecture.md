# Synkopa v2 — architecture and engine ⇄ UI contract

```
 ┌──────────── ui/ (SolidJS + canvas) ────────────┐        ┌──── synkopa (CLI) ─┐
 │ playlist · mixer · FX windows · library · ...  │        │ context/apply/undo │
 └───────▲───────────────┬────────────────────────┘        └────────▲───────────┘
   events│ "state"       │ invoke(...)          binary Channel       │ local socket
   ┌─────┴───────────────▼───────────────────────────────────────────┴───────────┐
 │ app/ (Tauri shell): commands, session server, CLI shim, project files        │
 └───────────────────────────┬──────────────────────────────────────────────────┘
                             │ Engine handle (Mutex<Project> + history, rtrb rings)
 ┌───────────────────────────▼──────────────────────────────────────────────────┐
 │ crates/engine: model · ops · history · sequencer · instruments · built-in FX  │
 │                mixer graph · recording · offline render · analysis thread    │
 │ crates/audio-io: cpal (WASAPI / ASIO / CoreAudio / ALSA / JACK) duplex streams│
 │ crates/vst3-host: VST3 effects and instruments, scan in a child process      │
 │ crates/lv2-host:  LV2 effects as `Processor`s (Linux, liblilv at run time)   │
 └──────────────────────────────────────────────────────────────────────────────┘
```

Rules that keep it fast:

* **The audio callback never allocates, locks or does I/O.** Structure changes
  are built off-thread and swapped in through an `rtrb` ring; the old graph is
  sent back to be dropped off-thread. DSP state (reverb tails, delay lines) is
  carried across a swap by effect id.
* **Independent work runs on every core** (`crates/engine/src/pool.rs`). Each block, the
  synth instances render into their own buffers and the non-master insert chains run on a
  pool of audio worker threads (time-critical priority, created with the stream), the audio
  thread taking jobs from the same atomic counter; the results are summed in a fixed order,
  so the output is bit-identical to one thread (`tests/rt_workers.rs`). A dispatch is a few
  atomics: no allocation, no lock. If no worker wakes in time the audio thread does the work
  itself. Chains with a watched effect (an open analyser) stay on the audio thread. With
  mixer routing (below) the inserts run level by level: each level on the pool, its sends
  added into the next level's inputs in a fixed order, the master last.
* **Parameter moves do not rebuild anything.** They are a fixed-size message.
* **The UI never polls JSON at frame rate.** Meters, playhead, recording peaks
  and the analysis of *open* effect windows arrive as one binary frame on a
  Tauri `Channel` (≤60 Hz). FFTs run on an analysis thread in Rust, only for
  effects whose window is open (`watch_fx`).
* **The UI draws with canvas** for anything that moves (playlist clips,
  meters, visualisations) with a single `requestAnimationFrame` loop; static
  layers (grids, curves that depend only on parameters) are cached offscreen.

## Data model

`crates/engine/src/model.rs` is the source of truth; JSON is camelCase. A
project looks like:

```jsonc
{
  "version": 2, "bpm": 128, "nextId": 1200, "loop": null,          // or {"a":0,"b":16}
  "tracks":  [{"id":1,"name":"Drums","color":"#4f8ef7","muted":false,"solo":false,"arm":false,"vol":0.7937,"h":56,"insert":1}],
  "inserts": [{"id":0,"name":"Master","color":"#d5dae2","vol":0.7937,"pan":0,"muted":false,"solo":false,
               "fx":[{"id":900,"type":"limit","bypass":false,"preset":0,"params":{"gain":0,"ceil":-0.3}}]}],
  "channels":[{"id":1001,"name":"Kick","color":"#4f8ef7","kind":"sample","source":1000,"vol":0.7937,"pan":0,
               "mute":false,"solo":false,"insert":null,"root":60,
               "sampler":{"start":0,"end":1,"reverse":false,"norm":false,"semi":0,"fine":0,"env":false,
                          "delay":0,"attack":0,"hold":0,"decay":0,"sustain":1,"release":20,
                          "attackTension":0,"decayTension":0,"releaseTension":0,"cut":0}}],
  "patterns":[{"id":1,"name":"Beat","color":"#4f8ef7","len":4,"minLen":4,"swing":0.5,
               "notes":[{"ch":1001,"step":0,"len":1},{"ch":1001,"step":6,"len":1,"p":55,"v":0.8}]}],
  "sources": [{"id":1000,"name":"808 Kick Short","color":"#4f8ef7","path":"lib:Standard/kick-808-short.flac","dur":0.378,"sr":44100,"ch":1,"peakDb":-1},
              {"id":1010,"name":"Pad Loop","color":"#47b8a8","path":"…/pad.wav","dur":3.75,"sr":48000,"ch":2,"peakDb":-3.1}],
  "clips":   [{"id":5,"track":0,"start":0,"len":16,"offset":0,"kind":"pat","ref":1,"muted":false,"gain":0,
               "fadeIn":0,"fadeOut":0,"pan":0,"norm":false,"rev":false}]
}
```

### Stretched audio clips

An audio clip may carry `"stretch": s` (0.25–4, absent = 1) and `"resample": true`. The clip
plays its audio `s` times longer than it is; `offset` and `len` are beats of the *stretched*
audio, so trimming, slicing and moving work exactly as on an unstretched clip, and a source
second sits at `offset·spb/s`. The Playlist's Trim | Stretch switch (Shift+M) decides whether
dragging an audio clip's edge trims it or changes `s` (offset and length scale with it, the
opposite edge stays). Other clip kinds cannot be stretched (`putClips` rejects it).

By default the pitch is kept: `SourceStore` renders the stretch offline with Signalsmith
Stretch (`crates/stretch`, vendored C++, MIT) on a background thread (`wave-stretch`), caches
it by (source, pre-stretch audio, `s`) within 1 GiB (least recently played dropped first) and
rebuilds the timeline when it lands. Until then — and if the render fails — the clip plays
resampled (`ClipRt::rate = 1/s`, pitch follows speed), and the state's `stretching` /
`stretchErrors` name it so the Playlist can show it. A drag never queues renders (the stretch
is rendered once, on release). Offline bounces render missing stretches themselves, outside
the project lock. `"resample": true` always plays resampled (turntable-style), no render.

### Transposed audio clips

An audio clip may carry `"pitch": p`, semitones within ±12 (cents as the fraction, absent = 0):
the clip sounds `p` semitones higher and keeps its length. It is rendered by the same path as a
stretch, in the same Signalsmith pass (`stretch_shift_stereo`: length × `s`, transposed by `p`),
cached by (source, pre-stretch audio, render ratio, `p`). The render ratio is `s`, or 1 for a
resampled clip — that one reads the render at `1/s`, so speed and pitch move together on top of
the transposition. Until the render lands the clip plays at its own pitch (`stretching` names
it). Other clip kinds cannot be transposed (`putClips` rejects it). The sample window sets it
with PITCH and FINE (whole semitones toward 0, and cents) and fits a clip to 1 or 2 beats or
1–8 bars (`fitClip`: the stretch scales by `beats / len`, the offset with it, the start stays).

## Channels, patterns and notes (Channel Rack and Piano roll)

Channels are **global to the project**, like FL Studio's Channel Rack: one row per
instrument, and every pattern can hold notes for every channel. A note names its
channel with `ch`. Why global and not per pattern: a beat is usually several patterns
(verse / chorus / fill) played by the same kit; per-pattern instruments would duplicate
the kit and its mixer routing in every pattern.

* `channel.kind`: `sample` (the sampler: plays project source `source`; the note's key
  repitches it, `root` = the key at which the file plays at its own pitch, default 60 = C5 in
  FL's naming / C4 in this app's labels; `sampler` = how it plays, see below), `keys` (Synkopa
  Keys, see [keys.md](keys.md)), `synth` (the built-in polyphonic synth, `synth: {preset, params}` with
  parameter ids from `fx_catalog().synth`; see [synth.md](synth.md)) or `plugin` (a hosted VST3
  instrument, `plugin: PluginRef`; see [Plugin instruments](#plugin-instruments)). `vol` is a fader position (unity = 0.7937, gain normalised so unity is exactly
  1), `pan` −1..1 (balance law of the mixer), `mute`/`solo` (with any solo, only soloed
  channels play), `insert`: a mixer insert id, or `null` = **the insert of the playlist track
  the clip is on** (how every pattern note behaved before channels existed; the track's fader
  and mute still apply). A channel routed to an insert skips the track's insert but the
  track still mutes it and scales it by its fader.
* `channel.sampler` (kind `sample`; every field has a default, so older files load):
  `start`/`end` = the part of the file that plays, fractions 0..1; `reverse`; `norm` (peak to
  −0.3 dBFS, from the source's `peakDb`); `semi` −24..24 and `fine` −100..100 cents on top of
  the note. `env` switches the **volume envelope** (FL Studio's Channel settings ▸ Volume).
  Off, the sample plays as recorded: live keys, auditions and legacy notes ring to its end and
  sized notes stop at theirs (declicked). On, each note is shaped by `delay` 0..5000 ms of
  silence, `attack` 0..2000 ms up to full, `hold` 0..5000 ms at full, `decay` 0..5000 ms to the
  `sustain` level 0..1 (0 = the decay is the whole sound), and fades to silence over `release`
  1..5000 ms from its note-off: a key let go, the end of a sized note or a legacy note, the end
  of an audition. A step trigger has no note-off, so it holds the sustain. The note-off starts
  the release from whatever level the envelope had, so an early note-off never jumps.
  `attackTension`, `decayTension` and `releaseTension` −1..1 bend those segments (0 = straight,
  positive bows up): progress `x/(x + g(1−x))` with `g = 8^∓tension`, so ±1 reaches 8/9 or 1/9
  half way. The voice computes the level from its own time and its note-off time, so the
  envelope needs no per-voice state beyond the cut level. `cut` 0 = none, 1..8 = a **cut group**: a new note silences every sounding sampler
  note of the group's channels, its own included, when it starts (a closed hat cutting the
  open one). The shared sample editor's Fade In and Fade Out handles and knobs set
  `fadeInMs` and `fadeOutMs` (0–5000 source ms). They apply at the audible edges of the
  selected part, swap sides with Reverse, and are independent of the envelope: a fade
  shapes the sample itself (in source time, heard on every note), the envelope shapes each
  note (in note time, from its note-on and note-off). The editor keeps them on separate pages.
  Each fade is bounded by the region, overlapping fades multiply, and pitch changes the
  audible duration together with the rest of the sample. The automatic 1.5 ms declick
  at trimmed or reversed edges still applies. Both fade settings default to zero.
  `putChannel` refuses values out of range (`SamplerState::check`). Files written before
  `env` existed had `gate` (one-shot or a release after note-off) and always applied attack,
  decay and sustain: `Project::from_json` opens a gated or shaped sampler (attack > 0 or
  sustain < 1) with its envelope on and an untouched one-shot with it off.
* `note`: `step`/`len` in 16ths, `p` MIDI key (absent = the channel's `root`), `v` velocity
  0..1 = linear gain (absent = 1). A Channel Rack step has `trig: true`. It occupies one grid
  cell and plays a sampler without a note-off, even with its envelope on. Synth and Synkopa Keys
  continue to use the grid duration. A sampler note with `trig: false` ends as soon as its
  set duration or the sample runs out; with the envelope on its end starts the release
  instead. Without it, a short fade just before the note boundary prevents clicks. The Piano Roll shows triggers as short marks. Drag their resize grip or use
  **Convert to held note** in note properties to give them a duration.
  After a trigger is clicked, notes drawn in the Piano Roll are triggers too (one grid cell at
  most), so a triplet added among the hats stays steps: the Rack draws an off-grid trigger at its
  place inside the cell, as wide as the gap to the next one. Only held notes and chords (two
  triggers at the same time) turn a row into the mini piano roll. A click on a cell clears every
  trigger starting in it.
  Notes without `trig` keep their original playback and Rack layout. Resizing a legacy note
  in Piano Roll makes it a held note. Replacing a legacy step in the Rack makes it a trigger.
* `pattern.len` in beats is **derived**, like FL Studio's automatic pattern length: the end
  of the last note rounded up to a whole bar, at least `minLen` (whole bars, ≥ 1; the bars
  the user keeps on purpose with "+ bar", e.g. a crash every second bar). The engine sets it
  on every `putPattern` and on load (`Pattern::fit_len`); a `len` sent by a client is
  ignored. Drawing past the end makes a pattern longer, deleting its last notes shorter.
  When `len` changes, pattern clips that show exactly one whole pass (`offset` 0, `len` = the
  old length) take the new length; clips sized on purpose (repeats, offsets) keep theirs. New
  clips use the pattern's length. Notes must end within the song (`TOTAL_BEATS`).
* `pattern.swing` 0..1: a note that starts exactly on an odd 16th (0-based 1, 3, 5 …) is
  delayed by `swing × ½` step. 100 % puts the off-beat 16th at 75 % of its 8th (MPC 75 %),
  ~67 % is close to triplet feel; 0 is straight. Even 16ths and off-grid notes never move.
  Swing is applied when the timeline is built (schedule time), never written into the notes.
* **Chord tools** (piano roll Tools strip + `synkopa notes chord|progression|stamp`): the chord
  theory lives in one place per language — `transform.rs`'s tables (`CHORD_TYPES`,
  `PROGRESSION_TEMPLATES`) and helpers (`chord_pitches`, `chord_name`, `progression`),
  mirrored by `ui/src/lib/transforms.ts`, with golden tests pinning both sides to identical
  voicings and seeds. Stamped notes are concrete pitches (a later key change does not
  re-harmonize); naming is bass-first, so a root-position C6 reads "C6", not "Am7"; the
  `progression` generator replaces its channel's notes and grows the pattern like the other
  generators, bars bounded before allocation. Stamps go through the existing note op — the
  only new engine surface is the two `TransformVerb` kinds.

**The UI, as in FL Studio.** The *current pattern* is global (the toolbar's pattern selector,
numpad 1–9 / + / −, F4 next empty pattern, F2 rename, Shift+Ctrl+C clone); picking a pattern
clip on the playlist or in the browser makes it current, and the playlist paints it. Two
two windows show it: the **Channel Rack** (F6: a row per channel — mute LED,
pan, volume, mixer insert, name, steps — "+ / −" bars, swing, and the graph editor `G` for
per-step velocity and key of the selected channel) and the **Piano roll** (F7: the selected
channel's notes in the current pattern, any channel; the pattern's other channels as ghost
notes, double right-click one to switch). Each one floats in the effect-window layer or is
docked; F5, a Playlist click and a right-click on the Mixer tuck away floating windows only,
Escape never closes a docked panel, and F6 / F7 on a docked panel show it, then give it the
keyboard, then hide it. Docked panels keep the `.win.patwin` frame, so their keys are guarded
exactly like the floating windows'.

**Docking (#90).** The Channel Rack, the Piano roll and the Mixer each float or are docked to the
left, right or bottom edge (`U.dock`, pure model in `lib/dock.ts`, actions in `state/docking.ts`,
layout in `views/PatDock.tsx` `Workspace`). An edge holds *strips* from the Playlist outward;
a strip holds one panel or several that share it (stacked on a side edge, side by side at the
bottom; each panel's `share` is its flex). Left/right strips sit between the Browser and the
Playlist, bottom strips span the whole width under them (so the default — Mixer at the bottom —
looks as before). A strip's thickness is its first shown panel's `U.paneSize[id].w|h` (the rack's
`w` 0 = as wide as the pattern), clamped by `fitStrips` so the Playlist keeps ≥ 320 × 140 px: when
the wants do not fit, every strip gives up the same part of what it has above its minimum
(`PANE_MIN`), and when even the minimums do not fit they shrink alike down to half the room.
Splitters: one on each strip's Playlist side (drag, arrow keys; a rack strip's double-click fits
the pattern) and one between panels sharing a strip (`splitShares`). Dragging a title bar
(`paneHeadDown`) moves the window — a docked panel floats off under the pointer first — and
shows the dock guides (`dockGuides`, Visual Studio's): one at each outer edge (dock outside every
strip there), a compass over the Playlist (dock beside it) and one over each docked panel (along
its strip = share it, across = a new strip on that side). Only a drop on a guide docks; anywhere
else the window stays floating. The title bar's dock button floats a panel or docks it back where
it was (`U.dockHome`); its right-click menu and View → Window layout dock it to an edge without
dragging, and *Reset window layout* restores the first-launch layout. Rows are keyed by strings,
so opening, closing or resizing one panel never rebuilds the others. The docked Mixer alone in a
strip collapses to its title bar (below); sharing a strip it simply leaves while hidden; on a side
edge its effect chain goes under the strips. Dock layout, sizes, homes and the floating places
are per user in `localStorage` (`wave.layout`, validated by `parseLayout`; a layout from before
#90 — `rackDock` / `rollDock` / `mixDock` — is translated), never in the project.

**The mixer: docked, and the Mix view (F9).** The docked mixer (`views/Mixer.tsx` `MixDockPane`)
is a strip like the other panels (at the bottom by default). Alone in its strip, its splitter
dragged below 110 px collapses it to its 28 px title bar (a turned-title column on a side edge) (`U.mixMin`; click, Enter / Space or drag the
handle toward the Playlist to open it again at `U.paneSize.mix`, which the collapse keeps). It
cannot be closed; a layout saved with the old closed or minimised mixer opens collapsed
(`parseLayout`). The collapse math is pure (`lib/layout.ts` `fitMixH`, `mixSplitDrag`,
`mixHandleDrag`). Docked strips are
lean: one name row (number and name, a default "Insert N" is just its number, the effect count
on the right), pan, width, M/S, meters and fader. F9 (the toolbar's mixer button, View → Mix view)
toggles the **Mix view** (`state/mixView.ts`, `MixView`): the mixer covers the whole area under the
toolbar with a "Back to arrange" bar, its strips add the level readout and what plays through
them. The arrange layout under it stays mounted and laid out (`.workcol.away`:
`content-visibility: hidden`, `inert`; the docked mixer keeps its height without building strips;
floating Channel Rack / Piano roll are `visibility: hidden`), so scroll, zoom and sizes are kept,
and the Playlist / rack / roll frame tasks skip drawing while it is on. Entering takes a
detached snapshot of the layout fields (`lib/mixer.ts` `takeArrange`, a deep copy: the dock layout nests); F9, Esc (unless the last
click or the keyboard is in a floating window, which Esc then closes) or the back button restore
it exactly, and asking for a pattern window (F6 / F7, docking) leaves the Mix view first.
Effect windows keep floating above. The Mix view is per session (never saved).
**Routing channels** (FL's "link selected channels"): Shift+click (range) and Shift+Ctrl+click
(toggle) select several Channel Rack channels (`U.chSel`, with `U.selCh` the anchor); Ctrl+L
routes them to the selected insert, Shift+Ctrl+L one per insert from it on, creating the
missing inserts after the last one, named after their channel (`planRoute`; also in a strip's
right-click menu, disabled with the reason while nothing is selected). It is one `commit` of
`putInsert` + `putChannel` ops, so one undo step, in the engine and in the mock alike.
**Mixer routing** (FL's insert routing, `crates/engine/src/routing.rs`, mirrored in
`ui/src/lib/routing.ts`): an insert's post-fader output goes to the master (`Insert.master`,
default true) and/or to other inserts through `Insert.sends` [{to, gain 0..2}]. The routing is
a DAG: `putInsert` refuses a loop, a send to itself or to the master. Rewiring rebuilds the
graph; a send level is a fast-path `MixState` value, smoothed per block. Delay compensation
follows the paths: each insert's own input waits for its slowest source, each send and each
master path has its own delay line (`routing::latency`). Solo follows the routing (a soloed
bus keeps its sources, a soloed source its way out). In the maximized mixer a cable lane under
the strips shows every send (`RouteLane`): drag a strip's dot onto another strip to add one,
drag or wheel a cable's level, right-click it for 100 % / only here / remove; the strip's M
chip is the output into the master. Docked, a strip's right-click menu has *Send to* and
*Output to Master*, and the chain header says where the insert goes.

PAT mode (L) loops the current pattern and follows
it when it changes. The loop plays the pattern itself, not its place in the song: a channel
goes into its own insert, or the master when it has none, never through the insert of the
playlist track its clip is on (those effects belong to the song); a key played while it loops
goes the same way. A click on the playlist's ruler or a press on its grid lights SONG (a loop
that is playing is handed over by the ruler only). The Playlist, the Piano roll and the Pitch editor share one overview bar
on top (`ui/ZoomScrollbar` with a minimap: drag = scroll, drag an end = zoom) and one time
ruler (`ui/timeRuler.ts`, `lib/rulerEdit.ts`: click = start, drag = loop, drag a loop edge =
resize, Shift+drag inside = move, double-click = no loop). The Piano roll's ruler is in
pattern beats: a click in SONG mode, with the pattern in the playlist, moves the song to that
beat of the clip playing it (or the next one); otherwise it sets where the pattern loop starts
(PAT mode). A drag there sets the part of the pattern its loop plays (`pattern_play` /
`pattern_loop` `range`; per pattern, for the session, not an edit) and switches to PAT mode. *To new pattern* (Ctrl+Shift+X, or the button shown while notes are
selected) moves the selected notes to a new pattern at the same steps. Both patterns keep
the source's length, and every playlist clip of the source gets a clip of the new pattern
beside it, on the nearest lane below that is free over its span (a new lane when none is),
so the song sounds the same. It is one undo step (`lib/moveNotes.ts`).

**Automation clips** are edited on the playlist without an edit mode, as in FL. Points and the
tension handles halfway along each segment (shown on the clip under the pointer) are live:
- drag a point (Shift locks the value, Ctrl the time, Alt skips the snap), or drag a handle to
  bend the segment (Ctrl: finer);
- double-click: a point deletes it, a handle straightens it, empty envelope adds a point;
- right-click: empty envelope adds a point that stays under the pointer until release, a point
  opens its menu (type in a value in the control's unit, curve type, delete);
- Ctrl+drag selects points (Shift adds); a selection moves together and Delete removes it.
Each `AutoPoint` may carry `tension` (−1..1) and `shape` (`curve`, `double`, `hold`, `stairs`,
default `curve`) for the segment that ends at it. They are omitted when default, so older
projects load and play unchanged. `curve_at` (model.rs) and `curveAt` (`lib/automation.ts`) are
the same formulas, both checked against `crates/engine/tests/fixtures/curves.json`. The engine
still evaluates automation once per audio block, so a hold or a stair lands on a block boundary.

**Stacked clips.** Clips may overlap on a track, as in FL (layers, crossfades). The
playlist overlay hatches the header where clips of a track play at once, and puts "×N" on a
clip that hides N − 1 clips entirely (`lib/overlaps.ts`). Clicking such a clip says so in
the status line. The overlay is recomputed with the fades on clip edits and tile moves,
never per frame.

**The standard library** (`crates/engine/src/factory.rs`). 179 one-shots in ten groups
(kicks, snares, claps, hats, cymbals, toms, percussion, tonal, orchestra, FX) and 11 melodic
loops (group `Loops`) are embedded in the engine (`crates/engine/library`, 19.0 MB: FLAC
one-shots, Ogg Vorbis loops). All are CC0 but one (the Unlicense); `THIRD_PARTY_NOTICES.md`
credits each file from other authors than Versilian and the TR-808 set, and a test checks that
list against the table. A loop is cut to whole 4/4 bars at the tempo its name gives
(`loop-<name>-<bpm>[-<key>].ogg`, the key also in its display name); the library has no tempo
field, so the user stretches it to the project tempo like any other audio.
`scripts/make-factory-library.py` builds them: it downloads each pick at a pinned commit or URL
(the kits in `assets-manifest.json`, and the files listed under `factory` there, each with its
SHA-256: repository files, and Freesound sounds by their public preview), trims, fades and normalises it,
tunes the pitched ones onto their root key (`pitched` in the table: the browser shows that
key) and mixes the orchestra hits and stabs from tuned layers. A file whose audio did not
change is not rewritten, and existing files keep their names: projects name them by path.
There is nothing synthesised behind a channel any more: a new project's Channel Rack is four
samplers of these files (`STARTER_KIT`: Kick, Clap, Hat, Snare), and the program starts
with that empty project. A source inside the sample library is stored by a portable
path, `lib:<kit>/<file>` (in memory and in the file), never by where the library is on this
computer; `lib:Standard/…` is decoded from the embedded bytes, so it is never missing, other
kits resolve to `<data>/WAVE/Library/<kit>/`. Importing a library file twice gives the same
source: a sampler channel and audio clips of one sample share it. The app also writes the
standard library into `<data>/WAVE/Library/Standard` (`factory::install`) so the browser
lists, previews and drags it like any downloaded kit. `ui/src/api/factory.json` (the file
table, the empty project and the demo project `demo::demo_project`, for the in-browser mock)
is generated from the engine; `tests/sampler.rs` fails when it is stale and says how to
regenerate it.

**Parity by fixtures.** The browser mock is a second implementation of the engine's edit
layer, so what the engine computes is written to files the UI replays instead of trusting
comments. `crates/engine/tests/ui_ops_fixture.rs` runs 108 canonical op batches (every op,
happy and refused) through `ops::apply_all` and writes `ui/src/lib/__fixtures__/ops.json`;
`opsParity.test.ts` replays them through `lib/applyOps.ts` (same resulting project, the
engine's exact error strings — they are user-visible toasts) and `mockParity.test.ts`
through the whole `MockBackend` command path, including the undo/revision rule that an
unchanged batch is not an edit. The four error-string prefixes consumers match on
— views match `EXISTS:` (FX chains) and `NO_EDITOR:` (plugin windows), `synkopa watch`
recognises `REVISION_CONFLICT`, and both adapters throw `INVALID_TARGET:` for a dead
automation target — are named once in
`crates/engine/src/errors.rs`, mirrored by `ui/src/api/errors.ts`, and pinned to
`ui/src/api/__fixtures__/error-prefixes.json` by `app/src/ui_prefixes_tests.rs` ↔
`errors.test.ts`. Every writer re-derives its file on each run and fails when the file on
disk is stale (`WAVE_WRITE_FIXTURES=1` rewrites); the same ritual as
`ui/src/api/factory.json` and the tune-edit fixture.

**Old files.** Files from before the standard library name synthesised instruments:
`{"kind":"inst","inst":"kick"}` channels, or `inst` on notes before channels existed.
`Project::from_json` (used by `open`) turns each into a sampler of the closest library sample
(the bass with its envelope on); they do not sound the same as before. A project without channels
gets the starter kit.

**Real time.** Channels are resolved off-thread into a table in the timeline (`ChanRt`:
voice kind, `Arc` of the sample, gain/pan/on, destination insert). Voices keep a channel
index and id; a new timeline re-maps voices by id (no allocation), and knob / mute / solo
moves go through the mixer fast path (`MixState.chans`) without a timeline rebuild, so
ringing notes follow them. Auditions (`note_audition`) and the pattern loop preview
(`pattern_play`) are commands to the audio thread, not edits.

**Silence skipping.** An insert whose input is exact digital silence for longer than its
chain's tail (each `Processor::tail_samples`, summed, plus the dry-path delays) is not
processed: its effects are settled once (`Processor::park`) and skipped until sound returns;
an effect with no known tail (`None`, e.g. hosted plugins) keeps its insert running. Tune,
Voice and Denoise idle after a second of silence themselves, so a skipped chain and a
processed one give the same output (`crates/engine/tests/silence_skip.rs`, below -120 dB).
Live keyboard events do not take the core lock: they read an atomic snapshot (device live,
target channel, armed insert; refreshed on every sync) and push into a dedicated ring that
the audio thread drains with the command ring. Offline renders may run the insert chains on
worker threads (`Engine::set_render_workers`), bit-identical to one thread.

Effect types: `eq` (7 `bands`), `comp`, `sat` (character saturator, see
`docs/saturator.md`), `delay` (echo with a processing loop, `docs/delay.md`), `half`, `warp`
(time and volume maps, `maps: {time, vol}` slots, `docs/warp.md`), `chorus` (Chorus / Ensemble /
Vibrato, `docs/modulation.md`), `phaser` (2–12 swept all-pass stages, `docs/modulation.md`), `stereo` (width,
mono-safe spread, bass mono, gain, balance, polarity, `dsp/stereo.rs`), `multiband` (three-band upward + downward
compression, the OTT sound, `dsp/multiband.rs`), `analyzer` (BS.1770 loudness, true peak, spectrum and stereo
field; the sound passes unchanged, `dsp/loudness.rs`), `reverb`, `limit`,
`plugin` (a hosted VST3 or LV2 effect: `plugin: {format,path,cid,name,vendor,state}`, see
[Hosted plugins](#hosted-plugins); files from before LV2 support say `vst3`, which still loads
and is written back as `plugin`). Parameter ids, ranges and presets come from `fx_catalog` —
the UI must not hard-code them.

### Hosted plugins

A slot of type `plugin` holds a `PluginRef`: `format` (`vst3` or `lv2`), `path` (VST3: the
module; LV2: the bundle directory, for people), `cid` (VST3: class id in hex; LV2: the plugin
URI, which is what finds it), `name`, `vendor` and `state` (base64 of the plugin's opaque state:
VST3 component + controller streams; LV2: lilv's state as Turtle text, the plugin's own state
extension data plus every control port value). The engine builds processors through the app's
`PluginFactory`, which dispatches on `format` (`app/src/plugins.rs`): `wave_vst3::instantiate`
or `wave_lv2::instantiate`. A plugin that does not load (not installed, refused) becomes an
`fxErrors` entry; its slot and saved state stay in the project and are saved back unchanged.

* **Parameters** are by plugin id: a VST3 `ParamID`, an LV2 control input port index. Values
  are normalized 0..1 everywhere (automation `{kind:"plugin", fxId, paramId}`,
  `plugin_set_param`); for LV2 the host maps them onto the port's range (`lv2:integer`,
  `lv2:toggled`, `lv2:enumeration` snap; `pprops:logarithmic` maps geometrically;
  `lv2:sampleRate` ranges are scaled). On the audio thread a move is an atomic store, picked up
  at the next block.
* **Latency.** `Processor::latency_samples` reports a hosted plugin's delay (VST3
  `getLatencySamples`; LV2 the `lv2:reportsLatency` / `lv2:latency` control output, read after
  every block, with a short silent block run at creation so it is known up front). The engine
  keeps it per effect and uses it like `catalog::latency` in plugin delay compensation (the
  per-insert delays, the dry/wet line, take alignment, offline renders). The housekeeping thread
  asks live plugins again twice a second (`PluginFactory::latency`; VST3 only when the plugin
  sent `kLatencyChanged`) and rebuilds the graph, keeping running instances, when one changed.
* **LV2 host** (`crates/lv2-host`, Linux only; elsewhere every call says LV2 is hosted on Linux).
  liblilv is opened with `dlopen` on first use (`liblilv-0.so.0`), never linked: Synkopa builds
  and starts without it and the scan reports why LV2 is unavailable. The scan reads Turtle only
  (no plugin code runs, so no child process is needed); it lists plugins with audio in and out,
  leaves out instruments and analysers, and gives the reason for plugins that require a feature
  or option Synkopa does not provide. Bundles named in `lv2-blocklist.txt` in Synkopa's data folder
  are skipped. Provided: `urid:map`/`unmap`, `options:options` (sample rate, min/max/nominal
  block), `buf-size:boundedBlockLength`, `worker:schedule`, `state:mapPath`/`makePath`/
  `freePath`, `log:log`. Inputs and outputs always have separate buffers (so
  `lv2:inPlaceBroken` plugins run). Wiring to the stereo slot: stereo directly; 1 in / 1 out as
  two instances with linked parameters; 1 in / 2 out fed the mono sum; 2 in / 1 out to both
  sides; side-chain and CV inputs read silence, atom inputs an empty sequence, extra outputs are
  discarded; `lv2:enabled`, `lv2:freeWheeling` and `time:beatsPerMinute` ports are driven by the
  host. The worker extension gets a thread per instance fed through lock-free byte rings (the
  audio thread never waits; the thread polls every 2 ms), answers are delivered after `run()`.
  Every buffer is allocated at instantiation; `process` does no allocation, locking or system
  call. LV2 fixes the sample rate and block bound at instantiation, so a new device rate
  re-creates the instance from the saved state (the engine does that for every hosted plugin
  when the device changes).
* **VST3 bus shapes.** The main buses are asked for stereo; a plugin that keeps a mono bus is
  wired like LV2's: a mono input is fed the sum of both sides, a mono output plays on both.
* **The safety check** (`app/src/plugin_probe.rs`). Plugins run inside Synkopa, so a plugin that
  crashes closes Synkopa. Before a plugin (effect or instrument) is first loaded, the app starts
  itself as a worker (the scan worker's protocol, `WAVE_SCAN_WORKER=plugin-check`, the request
  as JSON) that creates it, runs a test signal (notes for an instrument) at block sizes from 1
  to 512, saves its state, creates a second instance from that state, runs it and frees both.
  A worker that crashes or does not answer within 45 s means the plugin is not loaded: the
  factory returns an error starting `UNSAFE_PLUGIN: ` (the slot shows it in `fx_errors`) and the
  UI offers **Load anyway** (`plugin_load_anyway {fxId}`: the verdict becomes "trying" and
  `Engine::retry_plugins` loads every slot that failed; after 20 s with Synkopa still running it is
  "trusted", while a "trying" found at the next start means Synkopa died with it, so it is refused
  again and a recovered project does not crash on it a second time). The engine asks the
  factory to `vet` the plugins an edit or a project load brings, before it takes its lock, so
  the checks (up to four at once) never hold up other edits. Verdicts are kept per plugin with the
  plugin file's length and time in `plugin-safety.json` (Synkopa's data folder), so an updated
  plugin is checked again. A plugin that refused to load in the check, or a build without a
  worker, loads as before. This catches crashes on creation, processing, state and teardown
  (Calf Vinyl on Ubuntu 24.04 crashes there, in lilv's `lv2bench` too); it is not a sandbox: a
  crash later (in its editor, at one parameter value) still closes Synkopa.

### Plugin windows

Opening an external plugin (an effect slot, a plugin channel's name in the Channel Rack, adding
one) shows **its own interface** at once, FL Studio's wrapper style: a native window
(`crates/plugin-windows`) whose title bar is Synkopa's header (the channel or effect name, the
format and kind, a gear, a close button; drawn in software with Geist, the app's font) and the
plugin's window embedded below it. The gear emits `plugin-settings` `{id}`, and the UI opens
Synkopa's window for it (the generic parameter list, level, pan, routing; `ui/src/state/pluginWindows.ts`).
A second click on the channel closes the plugin window. A plugin with no interface of its own
(`NO_EDITOR`), one that failed to load, or a session that cannot show plugin windows opens
Synkopa's window instead. Closing Synkopa's window leaves the plugin's open.

* **Windows.** A top-level `HWND` without its caption (`WM_NCCALCSIZE`), the header painted
  with `StretchDIBits` and answering `HTCAPTION` (move, snap) except over its buttons; a
  resizable plugin keeps the system's side and bottom borders. The plugin attaches to a child
  "host" window below the header. Tauri's loop dispatches everything (same thread).
* **Linux (X11; XWayland under Wayland).** Synkopa's own connection to the X server (`x11rb`,
  pure Rust: no libX11, no shared Xlib error handler): an undecorated frame (`_MOTIF_WM_HINTS`)
  holding the header child, the embed child the plugin attaches to (VST3 `X11EmbedWindowID`,
  LV2 `ui:parent`) and, for resizable plugins, input-only border strips. Only the header and the
  strips select pointer events (a frame that did would take an implicit grab and steal drags from
  plugins that read XInput2). Moves and resizes are handed to the window manager
  (`_NET_WM_MOVERESIZE`); minimal XEmbed (embedded notify, mapping on `_XEMBED_INFO`, focus).
  `transient-for` the main window when GTK itself runs on X11, so they stay above it. That is
  why, in a Wayland session with XWayland, the app runs its own window through XWayland too
  (`GDK_BACKEND=x11`, set at start by `plugin_loop::prefer_x11`, as Bitwig, REAPER and Ardour run):
  an X11 window cannot stay above a native Wayland window, and every click on Synkopa sent the
  plugin windows behind it. `WAVE_WAYLAND=1`, or a `GDK_BACKEND` the user set, keeps native
  Wayland. The header's pin keeps a window above every other application's too
  (`_NET_WM_STATE_ABOVE`; `HWND_TOPMOST` on Windows). Scale: `Xft.dpi`. No X display
  (`DISPLAY` unset) makes `app_info.pluginEditors` false with the reason.
* **The UI thread's loop.** Linux has no global event loop, so the app installs GTK's glib loop
  (`app/src/plugin_loop.rs`) as `wave_plugin_windows::event_loop` on the main thread. It reads
  the X connection (a socket watch plus a 16 ms tick that also runs the windows' tick handlers)
  and serves VST3's `Linux::IRunLoop`: plugin file descriptors and timers, offered through the
  editor's `IPlugFrame` and through the host context (so a plugin has one without an editor).
  JUCE-built plugins stop their own message thread once offered a loop, so it is offered only
  where one is installed (never in the CLI's headless renders); every registration belongs to
  its instance and is removed at teardown, so the loop never calls into an unloaded library.
* **VST3 messages.** `IHostApplication::createInstance` makes `IMessage` / `IAttributeList`
  (`crates/vst3-host/src/message.rs`), the channel between a plugin's processor and controller
  over the connected `IConnectionPoint`s (DPF plugins such as Vitalium need it).
* **LV2 interfaces** (`crates/lv2-host/src/ui.rs`). A plugin's first `ui:X11UI` whose required
  features Synkopa provides (`ui:parent`, `ui:resize`, `ui:idleInterface`, `urid:map`/`unmap`,
  `options:options` with `ui:scaleFactor` and `ui:updateRate`, `log:log`, `instance-access`) is
  loaded from its
  library (never unloaded). Control-port writes from the UI (protocol 0) set the same per-port
  value the parameter list and automation set (and mark the state to save); every control port
  that changed, outputs (meters) included, goes back to the UI at each tick; `idle()` runs every
  tick. GTK and Qt interfaces are not embedded (they would need their toolkit in Synkopa's process;
  GTK2 cannot share one with GTK3): those plugins keep the parameter list and
  `editor_unavailable` says why. Atom messages between UI and plugin are not carried yet.
  `instance-access` (JUCE-built LV2 interfaces require it: Surge XT, Dexed, …) hands the UI the
  running instance's `LV2_Handle`. The instance must then outlive the UI: one that stops running
  while such a UI is open (removed, or re-created at a new rate) is parked in the plugin's
  shared state and freed on the UI thread after the UI's `cleanup`; the UI closes at its next
  tick.

### Plugin instruments

A channel of kind `plugin` plays a VST3 instrument, like FL Studio's plugin generators: the
Channel Rack's "+" and "Replace" menus list the scanned instruments (`plugin_scan`'s
`instruments`: classes whose `subCategories` declare `Instrument`) and add the channel with a
`putChannel` whose `plugin` is a `PluginRef` without state. `putChannel` refuses other formats
(LV2 instruments are not hosted) and a ref without path or class id.

* **Instances.** The engine keeps one live instance per plugin channel (`sync_plugs`, built
  through `PluginFactory::create_instrument` off the audio thread, handed over as
  `Cmd::PlugAdd`). It is made when the channel appears or its plugin changes (another class:
  `PluginRef::same_class`); renaming, level, routing or a stale `state` keep it. A removed
  channel releases it; undo brings the channel back with its saved state. Opening another
  project, or a new audio device, makes every instance again from the saved states. A plugin
  that does not load (not installed, not an instrument: no event input, or a main output that
  is not mono or stereo) leaves the channel silent with an `fxErrors` entry **under the channel
  id**, and is not retried until its plugin changes or the device is rebuilt; the channel, its
  notes and its saved state stay in the project.
* **Notes** (`crates/engine/src/plug.rs`). Pattern notes, Channel Rack and piano-roll
  auditions, the MIDI keyboard, sustain, pitch bend and the mod wheel reach the instance as
  sample-accurate events: note-on/off with a note id (two overlapping notes of one key end
  independently), controllers 64 and 1, bend 0..1 (centre 0.5, the keyboard's ±2 semitones at
  the ends). They are queued ahead in a fixed schedule (4096 events; a full one drops new notes,
  never allocates) and delivered per block, at most `MAX_INST_EVENTS` (512) per call. Events of
  one instant keep the order they were queued in. Velocity is the note's; a track fader that a
  routed channel skips scales the output, not the velocity. A transport start or jump sends a
  note-off for every sounding note (its release rings) and drops what was scheduled.
* **Output.** The instance's stereo output plays where its latest note was played (the clip's
  track, or the channel's insert), scaled by the channel's gain and pan like the synth's;
  non-finite samples are dropped. It runs every block, even silent (a plugin may sound by
  itself), so its CPU cost is constant; there is no smart-disable yet.
* **VST3 host** (`wave_vst3::instantiate_instrument`). The main output bus is asked for stereo
  (a mono output is copied to both sides), extra outputs are discarded, audio inputs
  (side-chains) read silence, event input bus 0 is activated. Notes go into a preallocated
  `IEventList` (`kNoteOnEvent` / `kNoteOffEvent` with `sampleOffset`, `noteId`, channel 0);
  controllers and pitch bend become sample-accurate parameter points of whatever the plugin
  maps them to through `IMidiMapping` (looked up once at instantiation; an unmapped controller
  is dropped). The process context carries tempo, position and playing state as for effects.
* **Parameters, editor, state.** A plugin channel's id works wherever an effect id does:
  `plugin_params`, `plugin_set_param`, `plugin_editor`, and `plugin_state` (its current state,
  which the Channel Rack's Clone copies). Clicking the channel shows the plugin's own window
  (see Plugin windows); its gear shows the channel window with the window button and the
  parameter list. Saving stores each live instance's state in `plugin.state` (base64 of the
  component and controller streams, as for effects). Automating an instrument's parameters is
  not supported yet (the parameter menu says so).
* **Offline render.** An export makes its own instance per plugin channel from the saved state,
  activated in VST3's `kOffline` processing mode and fed the same events; the live instance is
  not touched. One that does not load renders silent (logged). Plugin latency of an instrument
  is reported by the plugin but not compensated yet.
* **CLI.** `synkopa channels --plugin NAME|CID` adds a scanned instrument; `channels` lists a
  plugin channel with kind `plugin` and its plugin.

* **Synkopa Demo Synth** (`crates/demo-synth`) ships with the app: a small VST3 instrument of
  Synkopa's own (saw, square or triangle, resonant low-pass opened by the mod wheel, ADSR, 16
  voices; no editor window, so Synkopa's parameter list is its UI; CC 1, CC 64 and pitch bend
  through `IMidiMapping`). `node scripts/build-demo-synth.mjs` builds it into
  `app/resources/plugins/WAVE Demo Synth.vst3`, which the installer ships as the resource
  `plugins/`. The app registers that folder (`wave_vst3::add_bundled_dir`, from
  `plugins::register_bundled`: the Tauri resource dir, or for the CLI on a file the
  executable's folder and the Linux packages' `/usr/lib/Synkopa`); the scan opens its modules
  even without a rescan (only when new or changed, in a child process as always), so the
  instrument is listed on first start.

Test seams: `crates/engine/tests/plugin_instruments.rs` (a stand-in factory: format, instances,
timing, live keys, state, missing plugins), `crates/vst3-host/tests/instrument.rs` (a real
instrument when `WAVE_VST3_TEST_INSTRUMENT` names its bundle; CI builds Synkopa Demo Synth and runs
it on Windows and Linux), `crates/demo-synth` (its DSP: pitch, release, note ids, pedal, bend,
wheel, voice stealing, state).

### Sampler slice mode

A sampler channel may carry `slices` (sorted fractions of its file in 0..1, at most 128, the first
normally 0). While it does, key `root + i` plays slice `i` — from its marker to the next one, the
last to the end of the file — at the file's pitch (plus the channel's transpose and fine tune); keys
outside the slices play nothing, and a new slice note cuts the channel's sounding slice (5 ms, the
cut-group fade). Envelope, reverse, fades and cut groups apply to each slice as to a region. The
engine suggests slice points with `source_onsets` (`slices::onsets`, SuperFlux: log-filtered
spectral flux with a maximum filter, local peaks above the local mean, each moved back to the
start of its attack); the UI keeps the candidates above its Sensitivity threshold, so the knob never
re-analyses. "Dump to pattern" writes one held note per slice at its time in the file at the
project tempo (`ui/src/lib/slicer.ts`).

## Ops (`crates/engine/src/ops.rs`)

All edits are "put" ops, applied atomically per batch:
`putClips{clips}`, `removeClips{ids}`, `putTrack{track}`, `putInsert{insert}`,
`removeInsert{id}`, `putFx{insertId,fx,index?}`, `removeFx{fxId}`,
`moveFx{fxId,to}`, `putPattern{pattern}`, `removePattern{id}` (also removes its clips), `putChannel{channel,index?}`, `removeChannel{id}`
(also removes its notes in every pattern; `removeSource` removes sampler channels of that
source), `putSource{source}`,
`removeSource{id}`, `putAutomation{automation}`, `removeAutomation{id}`,
`transformNotes{pattern, verb, seed, select}` (deterministic note transforms: verb kinds
`humanize|quantize|euclidFill|strum|euclidDrums|melody`; same seed and notes ⇒ identical
notes; `select` limits the notes touched by humanize/quantize/strum — the generators
`euclidFill|euclidDrums|melody` always replace every note of their target channel), `setKey{key, scale}` (project key/scale for the
scale-aware tools),
`editTune{clip, edit}` (note-level pitch edits of an audio clip, see [Pitch edits](#pitch-edits-on-audio-clips)),
`putComp{comp}`, `removeComp{id}`, `putNotebook{notebook}`, `setBpm{bpm}`, `setLoop{loop}`.

Every effect has `mix` (0..1 dry/wet, default 1). Automation:
`project.key` (0 = C, `null` = none) and `project.scale` (0 chromatic, 1 major, 2 minor, 3/4 pentatonic) feed the scale-aware note tools; `project.automations[] = {id, name, color, target, points:[{t, v}]}`; a clip of
kind `auto` with `ref` = automation id plays it (`offset` in beats into it).
Targets: `{kind:"fx", fxId, param}` (catalog id, or `band:<i>:f|g|q` for EQ:
f log 20–20k, g −30..30 dB, q log 0.1–18), `{kind:"mix", fxId}`,
`{kind:"insertVol"|"insertPan", id}`, `{kind:"trackVol", id}`,
`{kind:"chanVol"|"chanPan", id}` (Channel Rack channel), `{kind:"synth", chanId, param}`
(synth catalog id), `{kind:"keys", chanId, param:"release"|"dynamics"}` (release log
40–3000 ms, dynamics 0–100 %), `{kind:"plugin", fxId, paramId}` (a hosted VST3
or LV2 parameter, `paramId` as `plugin_params` reports it; `v` is normalised
0..1; `apply` refuses unknown fx/param pairs), `{kind:"clipGain"|"clipPan", id}` (an audio clip; gain
−24..+12 dB). `v` is 0..1; log parameters are normalised logarithmically, vol is the
fader position, pan maps −1..1 to 0..1, choices snap to an option. While playing, the
engine applies automation every block; channel and clip gain/pan glide across the block,
synth parameters glide like an edit, and an edit during playback keeps the automated value. New ids come from the
client: take them from `project.nextId` and increase locally (the engine bumps
`nextId` past any id it sees).

## Tauri commands

| command | args | returns |
| --- | --- | --- |
| `get_state` | – | `StateDto` |
| `apply_ops` | `{ops, label?, transient?}` | `{revision}`; error string on invalid batch (nothing applied) |
| `begin_gesture` / `end_gesture` | `{label?}` | – . Ops sent with `transient:true` between them become one undo step |
| `undo` / `redo` | – | `StateDto` |
| `transport_play` | `{fromBeat}` | – |
| `transport_stop` | – | – |
| `transport_silence` | – | – — the second Stop / Ctrl+H (FL's "Stop sound"): cuts voices, the built-in effects' tails and latency lines and the preview through a 3 ms ramp (`Cmd::Silence`); hosted plugins only get note-offs |
| `transport_set` | `{metro?, countIn?, rec?, monitor?, startBeat?, recAudio?, recNotes?, recQuantize?}` | `TransportDto` |
| `telemetry_subscribe` | `{channel}` (Tauri `Channel<ArrayBuffer>`) | – |
| `watch_fx` | `{ids: number[]}` | – (effects whose windows are open; ≤ 8) |
| `fx_catalog` | – | `{types:{[type]:{name, params:[{id,label,min,max,def,unit,log}]}}, presets:{[type]:[{name, params?, bands?, beats?}]}}` |
| `import_audio` | `{paths, track?, beat?}` | `{sourceIds, clipIds}` (placed clips only when track/beat given) |
| `source_peaks` | `{id, norm?, rev?}` | `ArrayBuffer` of f32 `[min,max]` pairs, 100 per second of audio |
| `pitch_analyze` | `{clipId}` | `{rate, curve: (midi\|null)[], levels: number[], confidence: number[], notes: [{start,end,pitch,level}], backend: "rmvpe"\|"yin"}` — pitch every 1/rate s (null = unvoiced: breath, sibilant, silence); times in seconds of the source |
| `pitch_target` | `{clipId, reference?: RefTune}` | `PitchTarget` (below) — Tune to Reference for the clip, with `reference` or the clip's own `tune.reference` (preview before applying) |
| `melody_channels` | `{clipId}` | `[{channel, name, notes, fit}]` — channels whose notes the playlist plays over the clip, best `fit` (0..1, share of the sung frames within a semitone of a sounding note) first |
| `melody_reference` | `{clipId, channel, span?: [s, s]}` | `{melody: Melody, fit}` — those notes as a melody reference (a copy, seconds of the source; `span` = only notes overlapping it) |
| `f0_model_status` | – | `{id, name, bytes, installed, backend, licenseNote, licenseUrl, path, error}` |
| `f0_model_install` | – | – ; downloads RMVPE, verifies size and SHA-256, activates it; progress as event `model-progress {id,done,total,error?}` |
| `music_analyze` | `{clipId?, excludeFx?}` | `{tempo: {bpm, confidence, candidates:[{bpm,score}], firstBeat}\|null, key: {root, minor, confidence, candidates:[{root,minor,score}], chroma:number[12], tuningCents}\|null}` — tempo and key of a clip, or of the project's active audio clips (excluding tracks routed to `excludeFx`'s insert) |
| `auto_vocal_plan` | `{clipId, beatClipId?, style?: "natural"\|"hard"}` | `AutoVocalPlan` (`ui/src/api/types.ts`): `{version, style, analysis, chain: [{type, params, mix, enabled, note}], trackGainDb\|null, stages, warnings}` — "Auto wokal": measures the vocal clip's region and calibrates tune → rider → fet → comp → deess → reverb → limit on it (`crates/engine/src/autovocal.rs`; key from `beatClipId`, else from the tracks not routed to the vocal's insert). Read-only, seconds per minute of audio; the UI applies the chain with `apply_ops` |
| `clip_peaks` | `{clipId}` | like `source_peaks`, but of what the clip actually plays (norm/rev/pitch edits applied), in source time: a stretched clip draws them stretched |
| `trim_clip` | `{clipId}` | `{sourceId}` — writes a new file with only the clip region, clip now points at it |
| `missing_sources` | – | `[{sourceId, name, path, error, library, clips, channels}]` — sources whose audio is not loaded (also `StateDto.missing`: source id → why) |
| `relink_sources` | `{args: {items: [{sourceId, path}]}}` | `{relinked: [{sourceId, path, error, lengthDiffers}]}` — decodes the files, then points the sources at them as one undo step (undo/redo also swap the decoded audio back; `SourceStore::relink`/`reconcile`). A file that does not decode leaves its source unchanged; none decoding is an error. `lib:` sources are refused |
| `relink_search` | `{args: {folder}}` | `{search: {relinked, candidates: {[sourceId]: [{path, exactName, sharedFolders}]}, notFound, scanned, truncated}}` — walks the folder (≤ 250 000 entries, depth 24, no hidden or linked folders) for the missing sources' file names (`crates/engine/src/relink.rs`); relinks each one whose only best match has the remembered length, returns the rest ranked |
| `midi` | `{cmd: "midi_inspect", args: {path}}` | `{bpm, tempoChanges, warnings, end, parts: [{index, track, channel (1–16), name, program, gmName, drums, notes, becomes}]}` — a standard MIDI file (format 0/1, `crates/engine/src/midi_file.rs`); `becomes` = `{kind:"drums", drums}` / `{kind:"keys"}` / `{kind:"synth", preset}` |
| `midi` | `{cmd: "midi_import", args: {path, parts, setTempo?, beat?, track?, into?: {pattern, ch, replace}}}` | `{patternId, channelIds, clipId, len, revision, warnings, tempoSet}` — one new pattern (GM drums: a library sampler per drum key, rooted at the key; other parts the closest synth preset or Synkopa Keys) and its clip at `beat` on the first free track; or one part's notes into a channel. One undo step (`Engine::apply_with_library`) |
| `midi` | `{cmd: "midi_export", args: {out, pattern?}}` | `{out, what, tracks, notes, bpm}` — format 1, 960 PPQ, tempo + 4/4, one track per channel (drum samplers on channel 10 as their key); the song = pattern clips as placed (looped, cut at the clip end, muted clips left out) |
| `fx_chain` | `{cmd: "fx_chains", args: {}}` | `{dir, presets: [{name, effects: [{type, name}], plugins, error?}]}` — FX chain presets in the user's library (`app/src/fx_chains.rs`): one `<name>.wavechain` JSON file each in `%LOCALAPPDATA%\WAVE\FX Chains` |
| `fx_chain` | `{cmd: "fx_chain_save", args: {insert, name, overwrite?}}` | `{name, effects}` — the insert's effects in order (parameters, EQ bands, dry/wet, bypass, Warp maps, VST3 state); never its volume, pan, name, colour or routing; an existing name fails with `EXISTS:` |
| `fx_chain` | `{cmd: "fx_chain_load", args: {insert, name, mode?: "replace"\|"append"}}` | `{revision, fxIds, replaced, failed: [{fxId, name, error}]}` — one undo step, fresh ids; an insert with effects needs `mode`; a VST3 that will not load keeps its slot and state |
| `fx_chain` | `{cmd: "fx_chain_rename", args: {name, to}}` / `{cmd: "fx_chain_delete", args: {name}}` | `{name}` / `{deleted}` |
| `make_unique` | `{clipIds}` | `{sourceId, track, insertId}` — FL's "Make unique" for audio clips (one source, one track): a copy of the source entry (same file and decoded audio, name "Name #n"), a new track right below that plays where the old track does (the master unless the user routed it; no insert is claimed), the clips move there. One undo step |
| `export_in_place` | `{clipIds}` | `{sourceId, clipId, track}` or `{cancelled}` — FL's *Export in place*: the clips rendered from the start of their span (through their inserts and effects, not the master's) into a WAV with the project's media, as one audio clip on a new track right below them routed to the master; the originals are muted. Effect tails and releases are kept: the clip runs past the span until the decay stays under -90 dBFS (at most 30 s). One undo step; a silent render is an error. `export_cancel` stops it |
| `stems_model_status` | – | `{id:"htdemucs", name, bytes, installed, licenseNote, licenseUrl, path, stems}` — the stem separation model (`crates/stems`: URL, size, SHA-256, licence as constants) |
| `stems_model_install` | – | the status, or `{cancelled}` after `stems_cancel`; downloads to `<data>/Models/htdemucs.onnx` (`WAVE_STEMS_MODEL` overrides), size and SHA-256 checked; progress as `model-progress {id:"htdemucs",…}` |
| `extract_stems` | `{clipId, stems: ("drums"\|"bass"\|"other"\|"vocals")[]}` | `{sourceIds, clipIds, track, mutedClip, revision}` or `{cancelled}` — FL's *Extract stems*: HTDemucs on the region the audio clip plays (±3 s of context, normalise/reverse as it plays), one WAV per stem with the project's media, one track per stem right below the clip (its track's insert) with a clip at the same place playing the same part the same way (stretch, pitch, gain, fades, pan copied; pitch-editor notes not), the clip muted; one undo step. Progress as `stems-progress {clipId, stage: prepare\|load\|separate\|place, frac}`; `STEM_MODEL_MISSING:` without the model; one job at a time; `stems_cancel` stops it and deletes its files |
| `stems_cancel` | – | – (stops `extract_stems` or `stems_model_install`) |
| `flatten_comp` | `{clipId}` | `{sourceId}` — renders a comp clip to a new file; the clip becomes an `audio` clip, the takes go (one undo step) |
| `comp_audition` | `{compId, takeId?}` | – ; plays one whole take instead of the comp (`null` = the comp). Live only: not saved, not undone, ignored by renders |
| `note_audition` | `{ch, key?, vel?, pattern?}` | – ; plays one note of a channel now (step clicks, piano-roll keys). `pattern`: the edited pattern, so a track-routed channel is heard through that pattern's track. Not an edit |
| `pattern_play` | `{pattern, from?, range?}` (`null` stops) | – ; loops one pattern on its own from `from` beats, within `range` `[a, b]` beats (none = the whole pattern). Starting it stops the song, starting the song stops it (FL's PAT / SONG). Position in telemetry `patBeat` |
| `pattern_loop` | `{range}` | – ; changes the playing pattern loop's range without restarting it (`null` = the whole pattern) |
| `preview_clip` / `preview_file` / `preview_source` / `preview_stop` | `{clipId, from?}` / `{path}` / `{id}` / – | – — `from` (seconds of the source): the take alone with its pitch edits at the source's speed from there to the clip's end (the pitch editor's Solo); rejects when nothing is left |
| `audio_hosts` | – | `[{id, name}]` (ids like `"wasapi"`, `"asio"`, `"alsa"`) |
| `audio_devices` | `{host}` | `{outputs: string[], inputs: string[]}`. ASIO lists registered drivers as outputs, with a shared input. |
| `audio_get_config` / `audio_set_config` | `– / AudioConfig` | `AudioStatus` (a config without `recOffsetMs` keeps the saved one; changing only `recOffsetMs` does not restart the device) |
| `audio_measure_latency` | – | `{roundTripMs, reportedMs, suggestedOffsetMs, confidence}` — plays 8 clicks (~5 s) instead of the mix, records them back (mic at the speaker or a loopback cable), finds them by cross-correlation off the audio thread. Transport must be stopped; opens the input for the measurement. Saves nothing |
| `asio_control_panel` | – | – (opens the driver's own panel, Windows only) |
| `library_list` | – | `{kits:[{id,name,description,bytes,count,installed}], folders:string[]}` |
| `library_install` | `{id}` | – ; progress as event `library-progress {id,done,total,error?}` |
| `library_files` | `{kit?}` or `{folder?}` | `{files: [{name, path, group, root?, pitched?}], truncated}`; kit `Standard` = the standard library (always installed); `root` = the key a sampler made from the file is rooted at, when the library knows it; `pitched` = the file plays a note there (drums and noises do not); `truncated` = the folder was bigger than the walk's 5000-file cap and the list says so |
| `library_add_folder` / `library_remove_folder` | – (native dialog) / `{path}` | updated `folders` |
| `project_new` / `project_open` / `project_save` | `{demo?}` (`true` = the demo project) / `{path?, ifRevision?}` / `{saveAs?}` | `StateDto` / `{state: StateDto, warnings}` or `{cancelled}` / `{path}` or `{cancelled}`. Open checks the approved revision before replacing the project. A save first keeps the file it replaces as `<name> (backup).synkopa` |
| `take_project_open_requests` | – | `string[]`; drains project paths supplied at startup or forwarded to the running Windows instance. The `project-open-requested` event tells the UI to drain again and use its normal guarded open workflow. |
| `recovery_write` | – | `bool`; the autosave tick for a project with no file: writes its crash-recovery copy (`<data>/WAVE/Recovery/<session>.synkopa`), or removes it once the project has a file or nothing unsaved |
| `recovery_list` / `recovery_restore` / `recovery_discard` | – / `{id}` / `{id}` | `[{id, savedMs}]` newest first (copies of sessions that did not exit normally) / `{state, warnings}` (opened as an unsaved project) / – |
| `export_audio` | `{useLoop, tail, sampleRate, format?, mp3Kbps?, wavBits?, dither?, wrapTail?}` | `{out, fromBeat, toBeat, tail, stats}` or `{cancelled:true}`; native save dialog; stereo WAV (32-bit float by default, or 16/24-bit PCM) or MP3; emits `export-progress` |
| `export_stems` | `{useLoop, tail, sampleRate, includeMaster, format?, mp3Kbps?, wavBits?, dither?, wrapTail?}` | `{dir, fromBeat, toBeat, tail, format, files, skipped}` or `{cancelled:true}`; native folder dialog; one WAV or MP3 per insert with sound, optional master mix; emits `export-progress` |
| `export_cancel` | – | – (abort an in-flight `export_audio` before a file is written) |
| `plugin_scan` | `{rescan}` | `{plugins: [{format,path,cid,name,vendor,category,unsupported?}], instruments: [...same], unavailable: {[format]: why}}` — effects: VST3 everywhere, LV2 on Linux; `instruments`: VST3 instruments for the Channel Rack; `unsupported`: why that plugin cannot run here; `unavailable.lv2`: no liblilv |
| `plugin_add` | `{insertId, format?, path?, cid, index?}` | `{fxId, revision}` — a scanned plugin onto an insert (the UI adds with `putFx` instead) |
| `plugin_editor` | `{fxId, open, title?, subtitle?}` | – (the plugin's own window under Synkopa's header showing `title` / `subtitle`; its gear emits `plugin-settings` `{id}`). Rejects with `NO_EDITOR: …` for a plugin without one, with the reason when none can be shown (no X11 display). Here and below `fxId` may also be a plugin channel's id |
| `plugin_params` | `{fxId}` | `[{id,name,value,display,def}]`; LV2 adds `{symbol,real,min,max,unit,steps?,toggle,integer,log,options?:[{value,label}]}` |
| `plugin_set_param` | `{fxId,id,value}` | – (`value` normalised 0..1) |
| `plugin_load_anyway` | `{fxId}` | – (an effect or plugin channel the safety check refused, its `fx_errors` entry containing `UNSAFE_PLUGIN: `, loads from now on; every plugin that failed to load is tried again) |
| `plugin_state` | `{fxId}` | the live instance's current state, base64 (what `PluginRef.state` stores) |
| `vst3_scan` / `vst3_add` / `vst3_editor` / `vst3_params` / `vst3_set_param` | as before | older names: the same, with `vst3_scan` listing only VST3 effects as a plain array |
| `cli_status` | – | `{installed, command, path, note?}` |

**CLI on a file (`synkopa --project FILE`).** The same binary (`Synkopa --cli`) runs
`actions::control` in-process on an engine of its own (`app/src/headless.rs`), against the
`actions::Host` trait that `App` also implements: no window, no audio device, no MIDI, no
session socket. Commands in `actions::LIVE_ONLY` (transport, auditions, undo/redo, open) are
refused with `NEEDS_APP`. Hosted plugins become live instances only when a plugin parameter is
asked for (`create_rt` on an audio half that is pumped by hand after each command, never by a
device), so plain edits and bounces do not pay for them. `crates/cli` owns load → run → save
(`wave_cli::Files`/`Doc`), refuses to write a file a running app reports as open, and runs
`script` files with one load and one save.

**Auto vocal as one session command.** `auto_vocal {clipId, beatClipId?, style?, noBake?}`
(`synkopa auto-vocal`) is the button's whole journey on the shared path: `auto_vocal_plan`, then
`bake_rider` unless `noBake` or the clip's source name already ends " (rider)", then the chain
as `RemoveFx`/`PutFx` ops with the level suggestion folded into the limiter's gain — the same
composition `ui/src/state/actions.ts` `autoVocal()` does for the click, and the insert must not
be the master. The entry checks `if_revision` first (a stale guard refuses before the bake);
the apply's own guard skips the bake's own step. `f0_model_install` is the model download as a
session request too (same `models::install`/`activate`, no progress events — the CLI prints the
licence note and result instead). Both work on a file (`NEEDS_APP` does not apply).

**`synkopa watch FILE` (live coding).** `crates/cli/src/watch.rs` keeps a project of its own (a
headless `Doc`, as for `--project`), reset to the project as it was when the watch started, runs
the script lines on it on every save and sends the result to the app with the `set_project`
action: `Engine::set_project` decodes only the sources whose id/file are new (outside the lock),
swaps the whole project as one undo step without stopping the transport, and syncs like an undo
(effects keep DSP state by id, plugin instruments of the same class keep their instance). The
watch passes the revision of its last apply as `if_revision`, so an edit made in the app
meanwhile is refused (`REVISION_CONFLICT`) rather than overwritten.

Events: `state` (a `StateDto`, after every change from any origin: UI, CLI,
undo, import, recording, file open), `library-progress`, `model-progress`, `stems-progress`, `audio-status`,
`export-progress` (`{done, total, name, stage, frac, beat, peak, wave}` while a bounce runs: the
file in progress, `prepare`/`render`/`write`/`done`, the song position and a live peak waveform).

```ts
type StateDto = { project: Project; revision: number; canUndo: boolean; canRedo: boolean;
                  transport: TransportDto; path: string | null; dirty: boolean;
                  audition: { [compId: string]: number };     // comp → auditioned take
                  missing: { [sourceId: string]: string } };  // sources whose audio is not loaded → why
type TransportDto = { playing: boolean; rec: boolean; monitor: boolean; metro: boolean; countIn: boolean; startBeat: number };
type AudioConfig = { host: string; output: string | null; input: string | null; sampleRate: number | null; bufferSize: number | null;
                     inputChannel?: number | null; recOffsetMs?: number | null };
type AudioStatus = AudioConfig & { running: boolean; actualRate: number; actualBuffer: number; latencyMs: number;
                                   inputChannels: number; error: string | null; note: string | null; asioCompiled: boolean;
                                   unavailableOutput: string | null;
                                   inputUnderruns: number; inputOverruns: number; inputDriftPpm: number };
```

## Telemetry frame (binary, little-endian, ≤ 60 Hz)

```
u32 magic = 0x54564157 ("WAVT")   u32 seq
f64 beat            // playhead in beats as heard (Shared::heard_beat: rendered beat − playback latency); negative during count-in
u8  flags           // bit0 playing, bit1 recording, bit2 previewing
u8  pad, u16 pad
f32 inLevel         // input peak since last frame (linear)
f32 previewPos      // seconds into the previewed file, -1 when not previewing
f32 cpu             // audio callback load 0..1 (peak-held, slow decay)
u32 nIns  × { u32 insertId, f32 peakL, f32 peakR }            // linear peaks since last frame
u32 nFx   × { u32 fxId, f32 inPeak, f32 outPeak, f32 grDb,     // only watched fx (every frame, 60 Hz)
              f32 meters[32],                                   // per-type live readings, see "Effect meters"
              f32 specIn[128], f32 specOut[128],               // dBFS, 128 log bands 20 Hz..20 kHz (reverb: specOut = wet only)
              f32 scopeIn[256], f32 scopeOut[256],             // last samples, mono
              u32 nXy, f32 xy[2*nXy] }                          // stereo + analyzer only: newest output samples, L, R interleaved (≤ 1024 pairs)
u32 rec             // 0, or 1 followed by:
    u32 track, f64 clipStart, f32 seconds, u32 peakOffset, u32 nPeaks, f32 peaks[2*nPeaks]   // new [min,max] pairs at 100/s
f64 patBeat         // beats into the pattern the loop preview plays; −1 = stopped (decoders treat a missing field as −1)
u32 underruns       // output underruns since the engine started: callbacks slower than their period + driver xruns (missing = 0)
```

## Performance budget

* Callback: < 30 % of the buffer period with 8 tracks × 3 built-in effects at 48 kHz / 128 frames.
* UI: playlist pan/zoom at 60 fps with 500 clips; 6 open effect windows (incl. 3 reverbs) at 60 fps.

## Effect meters (`meters[32]` in the telemetry frame)

| type | meters |
| --- | --- |
| `eq` | `[0..24]` current dynamic gain offset of band *k* in dB (0 for static bands) |
| `fet` | `[0]` gain reduction dB, `[1]` input peak dBFS (after the Input knob), `[2]` output peak dBFS |
| `deess` | `[0]` gain reduction dB, `[1]` sidechain (high band) level dB — with Detect = Relative the high/low band energy ratio dB, `[2]` sibilance 0..1, `[3]` threshold dB |
| `rider` | `[0]` gain applied dB, `[1]` word level dB (K-weighted, ~200 ms), `[2]` phrase level dB (~2 s, confident vocal only), `[3]` vocal confidence 0..1, all at the moment being heard. Look-ahead (`look`): Live (default, value 2) adds no latency (trailing windows, forward slew only, gain applied 20 ms behind the analysis); Short ≈ 0.3 s and Long ≈ 1.6 s (the offline research windows exactly) are compensated by PDC |
| `denoise` | `[0..16]` learned noise floor dB per band, `[16..32]` gain applied per band dB (≤ 0); 16 log bands 50 Hz – 16 kHz. Total reduction in `grDb`. Adds `fft_size` (2048 at ≤ 64 kHz) samples of latency, compensated by PDC |
| `tune` | `[0]` detected input Hz (0 = unvoiced), `[1]` target note Hz, `[2]` output Hz, `[3]` confidence 0..1, `[4]` input offset from target in cents, `[5]` correction applied in cents |
| `voice` | `[0]` detected input Hz (0 = unvoiced), `[1]` output Hz, `[2]` confidence 0..1, `[3]` pitch shift applied in semitones. Same latency as `tune`, compensated by PDC; an effect's dry/wet (`fx.mix`) delays its dry path by the effect's latency, so blends line up |
| `half` | `[0]` position in the loop window 0..1, `[1]` 1 = slowing down (0 = stopped, or passing the input through until the first window after a playhead jump), `[2]`/`[3]` gains of read heads A/B (crossfade), `[4]` speed divisor (1.5 / 2 / 4), `[5]` window in beats, `[6]` crossfade ms (negative = before the window start, Percussive/FX), `[7]`/`[8]` band edges Hz (0 = that side not split). Zero latency; follows the song and the pattern loop preview (`ProcessCtx` carries the preview's beat) |
| `chorus` | `[0]` LFO phase 0..1, `[1..4]` left voices' read delay ms, `[4..7]` right voices', `[7]` voices a side (2 Chorus, 3 Ensemble, 1 Vibrato), `[8]` rate Hz. Zero latency |
| `phaser` | `[0]`/`[1]` the stages' break frequency left/right Hz, `[2]` stages, `[3]` LFO phase 0..1. Zero latency |
| `stereo` | `[0]` correlation out, `[1]` balance −1 (left) … +1 (right), `[2]` side/mid dB out, `[3]` correlation below 120 Hz, `[4]` correlation in, `[5]` side/mid dB in, `[6]` bass mono crossover Hz, `[7]` 1 while there is bass to judge. Zero latency; the window also gets X/Y points (`xy`) |
| `multiband` | `[0..3]` band level dB low/mid/high (after the +5.2 dB band drive), `[3..6]` gain dB (+ lifted, − pulled down), `[6..9]` band level out dB, `[9]`/`[10]` crossovers Hz. Zero latency |
| `analyzer` | `[0]` momentary LUFS, `[1]` short-term LUFS, `[2]` integrated LUFS (−200 = not yet), `[3]` loudness range LU (−1 = not yet), `[4]` true-peak hold dBTP, `[5]` true peak of the last 100 ms, `[6]` correlation, `[7]` balance, `[8]` seconds measured, `[9]` side/mid dB. Integrated, range and the hold restart when playback starts. Zero latency; X/Y points and a long FFT (8192) for the bass bands of `specOut` |
| `delay` | see `docs/delay.md`: `[0]`/`[1]` read heads L/R ms, `[2]` loop dBFS, `[3]` drive reduction dB, `[4]` LFO, `[5]` model, `[6]` wet dBFS, `[7]` set time ms. Zero latency |
| `warp` | see `docs/warp.md`: `[0]` bar position (−1 stopped), `[1]`/`[2]` playing slots, `[3]`/`[4]` queued slots, `[5]` offset beats, `[6]` speed, `[7]` gain, `[9]` hold state, `[10]` buffered s. 2 samples latency, compensated by PDC; maps arrive through `Processor::swap_data` (`Cmd::FxData`) |

## Where a recording goes

As in FL Studio, nothing has to be armed (`Engine::rec_target`, mirrored in
`ui/src/lib/recTarget.ts`): a recording goes to the **first track with nothing on
it from the start beat on** (to the loop end when recording loops), and when no
track has room a new track is added below the others. The take lands on that
track, where it was drawn while recording. A track added this way
(`Core::rec_added`) is removed again when nothing landed on it, and is dropped
from the undo snapshots that hold it empty, so undoing the take removes the track
too. "Record into this track" in a track's menu (the `arm` flag, shown as a red
dot in its header) overrides this: recordings then go there, as more takes over
what it holds.

The **recording filter** (right-click on Record; `TransportDto.recAudio` /
`recNotes`, at least one on, kept per computer by the UI) chooses what a
recording captures: the input's audio as takes, keyboard notes as a pattern.
With both on, a take during which the keyboard played keeps only the notes. The
input device opens only when Audio is on (or monitoring). Notes come from a MIDI
keyboard or from the computer keyboard as piano (`Engine::typing_note` feeds the
same recorder, each note on the channel it played); `recQuantize` (beats, 0 = as
played) moves recorded note starts to the grid and keeps their lengths
(`midi::quantize`). In **PAT mode** with Notes on and a MIDI keyboard connected
or the computer keyboard on, Record + Play loops the current pattern
and adds what is played to it (`Engine::begin_pat_take`; notes are counted on
across loop passes and folded back into the loop by `midi::fold_loop`; one undo
step when the loop stops, Record turns off or another pattern starts). Any other
recording runs the song (the UI switches to SONG so the lit mode says so).
Monitoring goes through the armed track, else the track being recorded on, else
the input is heard dry on the master.

## Take lanes and comping

Recording with a loop set cycles over it and makes **one take per pass**;
recording over an existing clip of the armed track adds a take instead of overwriting it. The
takes and the comp built from them are a project-level take group:

```jsonc
"comps": [{ "id": 2000, "name": "Comp 1", "xfade": 0.01,             // default boundary crossfade, s
  "takes":   [{ "id": 2001, "name": "Take 1", "ref": 1000,            // source id
                "start": 0, "len": 16, "offset": 0 }],                // comp beats; offset = beats into the source
  "regions": [{ "take": 2001, "from": 0, "to": 4, "xfade": 0.02 }] }] // sorted, non-overlapping, gaps silent
"clips":   [{ "kind": "comp", "ref": 2000, "start": 32, "len": 16, "offset": 0, … }]
```

**Why a clip kind and not takes on the track.** A comp is played by an
ordinary clip of kind `comp`, exactly like an audio clip plays a source:
comp beat = `clip.offset + (timeline beat − clip.start)`. So moving, trimming,
splitting, muting, gain, pan and fades of the comp reuse the clip machinery
and the existing timeline build; the take group itself never stores timeline
positions. Copies of a comp clip play the same group (like pattern clips).
Projects without `comps` open unchanged (`#[serde(default)]`).

* Takes are never cut. `regions` choose which take plays where; the UI's swipe
  writes a range (replacing what overlapped, merging touching pieces of the
  same take: `Comp::write`, mirrored in `ui/src/lib/comp.ts`).
* Every region boundary is an **equal-power crossfade** (cos/sin) centred on
  the boundary; its length is the region's `xfade` (at its start) or the
  comp's default, clamped so both halves fit. A region followed by silence
  fades out with the same length.
* The comp is resolved off the audio thread into segments (`comp::build`)
  when the timeline is rebuilt; the callback only mixes them (no allocation).
  Offline render and "flatten" use the same code, so they are sample-identical.
* Landing a recording (`comp::land_takes`): over a comp clip → new takes of its
  comp; over plain audio clips → those become the first takes of a new comp;
  elsewhere one pass is a plain audio clip and several passes a new comp. The
  newest take plays over its range (a last pass cut short by Stop only where
  it has audio). A last pass shorter than one beat is dropped. The audio
  thread marks each loop wrap with a NaN frame in the input ring.
* Recordings stay sources (`"Take N"`) next to the project like any audio
  (`<project> Audio/`, or the scratch folder until the first save); a new file
  never overwrites an existing one (`Take 2 1234 (2).wav`).
* **Saving and crash recovery** (`Engine::save`, `app/src/recovery.rs`). A save
  writes a temp file and renames it over the project, after copying the file it
  replaces to `<name> (backup).synkopa` (an ordinary project, so File > Open opens
  it). The autosave timer (Settings, default 2 min) saves a project that has a
  file; one that was never saved goes to `Engine::save_copy` in the Recovery
  folder instead (recordings stay in the scratch folder, so the copy finds them).
  Each running Synkopa holds an exclusive lock on its own `<session>.lock`; a copy
  whose lock nobody holds belongs to a session that crashed or was killed, and
  the UI offers it at the next start (`Engine::open_recovered`: no path, dirty),
  also when that start opened a project from File Explorer (after it opens; the
  question names the project a restore would replace).
  New, Open, Save and a normal exit remove the session's copy.
* **Takes line up with what was heard.** Input captured at a frame was sung
  to playback heard *round trip + PDC* frames earlier. `finalize_take` drops
  that many frames from the front; pass marks stay put, since they already
  sit on the heard loop boundary. At Stop or punch-out the audio callback
  captures the input still in flight for the same number of frames before
  the engine lands the take.
  * *Round trip, per callback* (`wave-audio-io`): output playback − callback
    (cpal) + the age of the input frame played first in this callback. Each
    input packet publishes `origin` = its capture instant − frames pushed
    before it, so input frame *i* was captured at `origin + i/rate`; the
    output callback knows which frame it plays (the jitter buffer's read
    position) and its own instant, so the age needs no ring-backlog guess and
    holds for any packet phase, packet size and a separate input device. Used
    where both streams share the system clock (WASAPI QPC, CoreAudio); other
    hosts fall back to *out + backlog + input callback−capture* (one buffer
    each way when a host reports nothing).
  * *Measured during the take, not at its start.* `Shared::add_take_latency`
    collects one sample per callback while the take captures real input
    (count-in excluded, nothing before the input's first packet), and
    `RecBuf::resolve` aligns the take with their mean. The estimate published
    at the start (`Shared::io_latency`, smoothed) is only the fallback when
    nothing was measured. Before, the start value was used: with monitoring
    off the input opens only when record is armed, so a take started soon
    after (or a punch-in, which used to restart both streams and lose the
    take) was aligned with an unsettled guess — "worse with monitoring off".
    PDC stays frozen at the start (the graph keeps it until the take ends).
  * *Calibration* (`AudioConfig.recOffsetMs`, signed ms, saved with the audio
    config, default 0) is added to every sample: converters, USB and driver
    buffers that timestamps do not report. `audio_measure_latency` measures
    it: the output callback plays a click plan instead of the mix and keeps,
    per output frame, the input frame consumed with it; a click rendered at
    output frame *a* found at *a + lag* means `lag` is exactly the skip a
    take needs, so `suggestedOffsetMs = lag − reported` (reported = mean
    per-callback round trip during the measurement). Normalised
    cross-correlation of a 4 ms windowed noise burst over 8 irregularly
    spaced clicks, lags 0–400 ms; confidence = clicks agreeing within 1 ms ×
    peak prominence over echoes. The job travels to the audio thread and back
    in a preallocated `Box` through lock-free rings; analysis runs on the
    caller's thread.
* **Output watchdog** (`AudioIo::spawn_watched`). A driver error ends a cpal
  stream for good (PipeWire/ALSA restarted, a USB interface unplugged), and a
  panic in the output callback would unwind into the driver (on ALSA it ends
  cpal's thread silently). The callback runs the engine under `catch_unwind`
  (no cost unless one happens) and plays silence after a crash; the audio-io
  thread checks every 500 ms and reopens a dead output with the same config
  and a fresh `Rt` (input errors only drop the microphone). When reopening
  fails it retries after 1, 2, 4 … 30 s until it works or the user changes the
  settings. `AudioStatus.restarts` counts the reopenings, the reason is in
  `note`, and each change is pushed as an `audio-status` event that the UI
  shows in its status line. A callback that *blocks* is not reopened, because
  dropping a stream whose callback is stuck can hang the thread that drops it.
  On Windows, the audio thread also checks the Focusrite USB Plug and Play device
  before ASIO opens and while that interface is active. If it disappears, Synkopa
  finishes the take, stops monitoring and offers the default WASAPI output.
* **Input jitter buffer** (`crates/audio-io/src/jitter.rs`). The input stream
  opens and closes on its own (`AudioIo::set_input` hands the output callback
  a new ring through a lock-free swap ring; the old one is freed off the audio
  thread), so arming record or monitoring never restarts the output. Input
  packets arrive on their own schedule (WASAPI: 480-frame periods against
  512-frame output callbacks) and, with a separate input device, on their own
  clock. Before, the output took what had arrived and zero-padded the rest:
  a packet arriving a moment late put ~477 exact zeros into the take several
  times a second (clicks), and pile-ups beyond two callbacks were dropped.
  Now: *prefill* to two input periods + half a callback + a 2 ms margin (one
  period on ASIO, whose input arrives in step with the output) before
  reading; *drift steering*: after 50 callbacks the backlog it settled at
  becomes the anchor and the read step is nudged (P control, 1 ms deadband,
  ≤ ±0.1 %) through a 32-tap Kaiser-windowed sinc, which is an exact copy at
  a step of 1, so one device is never resampled and a ±200 ppm input clock
  is followed without dropping or repeating samples; *real stalls* fade the
  last sample out over 2 ms, refill with a larger margin (+¼ callback per
  underrun, ≤ 50 ms) and fade back in; *overflow* (output stalled) fades out,
  skips to the target and fades in. Counted in `AudioStatus.inputUnderruns`
  / `inputOverruns`; `inputDriftPpm` is the followed clock difference. The
  cost is a little more input latency (~2 input periods) for monitoring.
* **Latency while recording.** Plugin delay compensation (PDC) delays every
  non-master insert to the slowest one (`pdc`), so one long look-ahead effect
  (the vocal Rider, ≈ 1.6 s on Long) delays all playback. Live input must not
  wait for it: **low-latency monitoring** is on whenever monitoring is on, a
  track is armed or being recorded on, the device delivers input and the track's insert is not the
  master. That insert then (a) skips its effects with latency (look-ahead
  Rider, tuner, denoiser, limiter, de-esser: bypassed for monitoring only, the
  take itself is always the raw input), (b) runs its PDC delay of `pdc` frames
  *before* its chain, on the playback summed into it, and (c) adds the live
  input (× track gain, muted with the track) after that delay. So the voice
  reaches the master with no PDC and no look-ahead, while the insert's own
  clips and patterns still come out `pdc` late like every other insert. The
  master chain's own latency (a few ms of limiter look-ahead) still applies to
  the voice. Switching the monitored insert clears its delay lines (a short
  gap, never stale audio). `playback_latency` does not depend on monitoring,
  so toggling it never moves the playback or the take alignment.
* The **metronome and count-in click** run through a delay line of the
  playback latency (`Graph::with_latency`, allocated with the graph), so they
  sound on the beats of the music; the **playhead** in telemetry is
  `Shared::heard_beat` (rendered beat − playback latency, held at the run's
  start until sound comes out, wrapped back into the loop after a wrap).
* A take's `skip` is fixed at its start, so **mixer changes that would change
  the playback latency during a take are deferred** (`Core::graph_deferred`):
  the audio thread keeps its graph (fader/param changes still apply) and the
  new graph is sent when the take ends. A parameter change that alters an
  effect's latency (e.g. the Rider's look-ahead) rebuilds the graph like a
  bypass toggle, so PDC always matches the parameters.
* **Instant start** (live stream only, `Rt::set_instant_start`): after Play, a
  seek or a pattern play the audio thread renders the playback latency muted
  ahead of the device clock (pre-roll), as many blocks per callback as fit a
  budget of 70 % of the period (its own frames included, at the slowest
  per-frame cost seen), so the start position is heard at once and on the
  timeline instead of after the latency. With a mix costing `c` of real time a
  pre-roll of `L` is heard after about `L · c / 0.7` (plus a callback). A jump
  while sound plays ramps the last output to zero over 3 ms while the new
  music fades in. Not while recording (a take's `skip` assumes the delayed
  start) and not offline (renders drop the latency themselves).
* **Punch-in**: turning `rec` on while playing starts a take at the playhead, on
  the armed track or the first one with room from there (`Cmd::PunchIn`; the audio thread reports the exact beat
  in `Shared::rec_start`); with a loop set it records in passes like loop
  recording. Turning `rec` off punches out and keeps playing.
* The device records **one input channel** (`AudioConfig.inputChannel`,
  0-based, default the first) as mono in both sides, so a mic on input 1 of a
  stereo interface is not left-only. Offline renders drop the PDC delay, so an
  export starts on its first beat and keeps its last `PDC` frames.

## Lyrics notebook ("Notatnik", F8)

```jsonc
"notebook": { "pages": [{ "id": 3000, "name": "Zwrotka 1", "startBar": 1,
                          "text": "[Zwrotka]\nidę przez miasto nocą\n\nświatła mi nie pomogą" }] }
```

Text that belongs to the song, so it is **project data**, not UI state: saved with the
project, undone, visible to the CLI (`synkopa context` lists the pages, `--full` has the
text, `putNotebook` through `apply`). It never touches the audio graph (the op returns an
empty `Dirty`). Old files without `notebook` open with no pages (`#[serde(default)]`).

* **One line = one bar**, counted from the page's `startBar` (1-based, 4 beats per bar,
  like the playlist ruler). An empty line is a bar of rest and still counts; a line whose
  trimmed text is `[…]` is a header and takes no bar. `ui/src/lib/notebook.ts` holds the
  mapping (`lineBars`, `lineOfBar`, `barOfBeat`), the Polish syllable counter and the rhyme
  grouping (ending from the penultimate vowel — Polish stress — with `ó`/`u`, `rz`/`ż`,
  `ch`/`h` and final devoicing folded; an ending needs ≥ 2 lines to get a colour).
* `putNotebook` replaces all pages (tab order = array order); ids are unique, `startBar`
  1..9999, ≤ 64 pages, ≤ 100 kB of text per page.
* **Typing is coalesced in the UI** (`ui/src/state/notebook.ts`): the textarea's text is a
  pending draft sent as one `putNotebook` (one undo step) after 0.9 s without typing, or on
  blur, page switch, undo/redo, save, new/open. Ctrl+Z inside the editor flushes the draft
  and runs the project's undo, never the textarea's own.
* **Draft backup.** Until the text is in a saved file, a copy sits in `localStorage`
  (`wave.notebook.backup:<path>`), together with the hash of the notebook it was based on.
  When that project is loaded again and its notebook is still exactly that base (nothing
  newer was saved), the copy is put back as one undoable edit. Saving drops the copy.
* **The window** is project-level, not part of the effect-window layer: F5 and clicks on
  the playlist never hide it. Geometry, font size (Ctrl+wheel) and the rhyme toggle are
  per-user view settings in `localStorage`. While the transport plays, the bar under the
  playhead (from telemetry) is highlighted on the current page and kept centred, unless the
  user typed or scrolled in the last 2 s. Clicking a bar number moves the playhead there.
  Rendering: a transparent-text `<textarea>` over a mirror of the same text (same font,
  fixed line height, no soft wrap), so gutter, highlight and syllable column stay aligned.

## Pitch edits on audio clips

`clip.tune = { notes: [{start, end, shift, drift, formant, gain, speed?}], pencil: [[sec, midi], …] }`
(seconds into the source; `shift`/`formant` in cents; `speed` ms, see below; `drift` 0..1 flattens pitch
movement inside the note toward its centre; `gain` dB). Start from
`pitch_analyze` notes, edit, send with `putClips`. Rendering is offline and
latency-free; during a gesture the previous render keeps playing and the new
one is made at `end_gesture`.

**Where the analysis' notes come from** (`crates/engine/src/note_track.rs`): a sung curve is
not a keyboard part, so notes are decided over the whole phrase rather than wherever the pitch
leaves a semitone. Viterbi over semitone states reads each frame either as it is (inside a
plateau the voice holds for 70 ms) or averaged over a vibrato cycle (vibrato and drift move
around a note, not to another; the average stops at glides so it never smears a note change);
a change costs a fixed penalty. Short notes join the neighbour nearest in pitch; a short note
with no plateau is a glide and is shared out between its neighbours (a scoop or fall at a phrase
edge joins its note); a note change moves to a spectral onset close to it (a new syllable); and
a held pitch is split where a strong spectral onset shows a re-sung syllable (`PitchAnalysis.
onsets`, 24-band log flux, not sent to the UI). Clips with edits keep their stored notes; only
unedited clips are segmented again. Tune to Reference still segments the *reference* melody
with the plain splitter (`pitch::segment`), which suits its stepwise curve. Parameters and
note-level accuracy (mir_eval's COnP / COnPOff on held-out singers) are in
`crates/tune-bench/RESULTS.md` (`tune-bench transcribe-grid`).

**Note edits are one op.** `editTune {clip, edit}` (`crates/engine/src/tune_edit.rs`) moves,
resets, splits and merges notes and draws pitch, addressed by note index in the clip's current
segmentation (`init` = the analysis' notes when the clip has none yet; `base` = a fingerprint of
the segmentation the indices were read from, refused as `STALE_NOTES` when it changed). The pitch
editor sends it for every note edit (its drags and macros included) and `synkopa pitch edit` builds
the same op, so the two cannot drift apart; `ui/src/lib/pitch.ts` `applyTuneEdit` is the browser
mock's port, checked against a fixture the engine writes. Tune to Reference settings are still a
`putClips` of the clip's `tune.reference`.

**Pitch for agents** (`crates/engine/src/pitch_api.rs`, `app/src/pitch_actions.rs`, `synkopa pitch …`):
read-only measurement — per-note statistics (sung pitch, cents, kind stable/vibrato/glide/short/
unsure, confidence, stability), a comparison with another performance (Tune to Reference's
alignment with a looser timing tolerance for another singer's phrasing, any interval, note
relations match/merged/split/extra, ornaments, `related` only when most matched notes agree) and
a cautious proposal (`suggest`) — kept apart from the decision, which the caller makes and applies
with `editTune`. See [cli.md](cli.md#pitch-editing-for-agents).

**Retune speed.** The correction (edited − sung pitch) follows its target with a one-pole
lag that reaches 95 % of a change in `speed` ms (Auto-Tune's Retune Speed): a note's own
`speed`, else `tune.reference.speed`, else immediate. After an unvoiced gap it starts from no
correction, so the singer's attack is heard and pulled into tune. Below 5 ms (hard tune) the
render smooths the change over ±5 ms instead of ±17.5 ms, keeping the steps. A tune without
speeds renders exactly as before they existed. The editor's Correct macro writes Strength
(`shift` toward the scale note), Speed and Flatten (`drift`) onto the notes; its presets
(Natural 80 %/120 ms/20 %, Pop 100/50/50, Tight 100/15/80, Robot 100/0/100) are UI only.

**Playback in the editor.** The ruler is the Playlist's (`ui/timeRuler.ts`): a click sets the
start marker (playback jumps there), a drag sets the project loop (`setLoop`, one undo step),
dragging a loop edge resizes it, Shift+drag inside moves it, a double-click removes it;
Space and Play run the song from the marker in SONG mode, so the vocal is heard in its mix.
Solo is `preview_clip` with `from`.

### Tune to Reference (`crates/engine/src/reference.rs`)

`clip.tune.reference = {source, correction, vibrato, influence, octave: number|null, key, scale}`
(0..1 amounts; `octave` null = automatic) makes another source the clip's
melody. Offline, deterministic, and the same code for GUI, CLI and tests:

1. **Analysis** — pitch (`F0Estimator`: RMVPE through `ort` when its model is
   installed, else YIN; 200 frames/s, MIDI, NaN = unvoiced) and content
   (MFCC, 100 frames/s; the spectral envelope c1–c6, which survives another
   singer, octave and vowel colour) of take and reference.
2. **Alignment** — coarse subsequence DTW over the whole reference finds where
   the take is; a banded DTW refines it with pitch class and contour shape.
   If that scores low, a content-only path is tried too. A jump-and-stall of
   the path (an implausible tempo map) is bridged by a straight line and
   aligned again narrowly around it, kept only if the content there vouches
   for it.
3. **Confidence** per frame — plausible tempo map (a veto, lifted where the
   content matches far better than chance), content against chance, contour
   correlation, and a gate on the take being more than 7 semitones from the
   reference (exact octave slips of the detector excepted). The median over
   sung frames is the score; below 0.3 the reference is refused and nothing
   is corrected.
4. **Register** — the multiple of 12 bringing the reference closest to the take.
5. **Target** — the reference's notes (segmented on the take's time axis) give
   centres; the take's own notes give vibrato and transitions (movement around
   a drift curve; octave slips of the detector and transitions to neighbours
   excluded from the drift). `target = take + correction·gate(confidence)·(centre + vibrato·movement − take)`,
   the interval folded to ±6 semitones so a detector octave slip still moves by
   the right amount.
6. **Render** — the existing TD-PSOLA with a continuous ratio(t), smoothed
   ±17.5 ms; manual note edits apply on top; a clip whose correction is zero
   everywhere renders bit-identical to its source.

```ts
type PitchTarget = { rate: number; start: number;            // first analysis frame of the region
  sourceF0: (number|null)[]; referenceF0: (number|null)[]; targetF0: (number|null)[];
  alignmentConfidence: number[]; refTime: number[];          // seconds into the reference
  octave: number; notes: Note[]; score: number; reason: string | null };  // reason set = refused
```

### Melody as the reference (`crates/engine/src/melody.rs`)

`reference.melody = {channel, span?, notes: [{start, end, key}]}` (source 0, influence 1): a
copy of the notes `channel` plays over the clip in the playlist (pattern repeats, swing and
clip edges as the sequencer plays them; muted pattern clips left out), in seconds of the take's
source — so it moves with the clip, and a later piano roll edit changes nothing until the
editor's *Update* (shown when `melody_reference` differs from the copy). No alignment is guessed:
`melody::alignment` builds an `Alignment` for `reference::target` directly — per sung note
(the editor's segmentation), stretches between played-note edges take the sounding note nearest
the voice (chords), slivers under 150 ms take their neighbour's (singing early or late keeps its
own timing), a played change inside one sung note moves to the take's steepest pitch change
within 150 ms (melisma); full confidence where a note sounds, none elsewhere (ad-libs stay as
sung); the octave is the multiple of 12 nearest the singing. The Piano roll's *Vocal reference*
button opens the pitch editor of a vocal the pattern plays over with the selected notes (`span`
= the source seconds they cover) or all of the channel's.

The other way round, the pitch editor's *Copy notes* (Ctrl+C) puts the selected sung notes (all
without a selection) on the Piano roll's note clipboard, UI only (`ui/src/lib/vocalNotes.ts`):
source seconds go through the clip (offset, stretch, tempo) to song steps as sung (no grid: the
pasted notes stay selected for the Piano roll's Quantize), the key is the sounding pitch (edits
included, pencil excepted) rounded to a semitone, velocity follows the level relative to the
loudest copied note, a note still sounding when the next starts ends there (the voice is
monophonic), and steps count from the bar of the first note, so Ctrl+V in any pattern keeps the
melody's place in the bar (from the pattern's start; a second paste lands a bar after it).

The RMVPE model is not shipped: `f0_model_install` downloads it (URL,
size, SHA-256 and licence link are constants in `crates/rmvpe`) to
`<data>/Models/rmvpe.onnx`; `WAVE_RMVPE_MODEL` overrides the path.
`crates/tune-bench` measures the whole pipeline (see `docs/cli.md`).

## Stem separation

`crates/stems` splits a stereo mix into drums, bass, other and vocals with Demucs v4 (HTDemucs, MIT
code and weights) on the same ONNX Runtime as RMVPE. `scripts/export-htdemucs-onnx.py` exports the
network without its STFT/iSTFT (ONNX has no complex tensors) and records the source names, rate and
segment as model metadata; `--fp16-weights` stores the weights as fp16 behind Cast nodes (90 MB
instead of 174 MB; ONNX Runtime folds the casts at load, so it computes in fp32 and the stems differ
from the fp32 export by < 1e-4). The crate does the STFT, iSTFT, the 7.8 s segments with 25 %
triangular cross-fades and the mix normalisation exactly as `demucs.apply`/`demucs.separate` do
(outputs match PyTorch to about 7e-5), and streams finished frames to a sink, so memory is one segment
per stem whatever the length. `cargo run -p wave-stems --release --example separate -- model.onnx
mix.wav out/` is the headless entry point.

The model is never committed or bundled: `stems_model_install` (`app/src/models.rs` `fetch`, shared
with RMVPE) downloads the pinned release asset (tag `models-htdemucs-v1`, a pre-release, so the
updater, which reads `releases/latest`, never sees it) and checks its size and SHA-256.

*Extract stems* (`app/src/stems_actions.rs`, one function for the dialog, the session socket and
`synkopa stems`): `Engine::stem_region` copies the region the clip plays (±3 s of context, with
normalise/reverse applied) under the lock and nothing else; off the lock it is resampled to 44.1 kHz
if needed, separated with `default_threads()` (cores − 2, 2..8: the audio engine keeps playing) and
streamed to one 32-bit float WAV per picked stem in the project's audio folder (the scratch folder
of an unsaved project, which the first save moves). The separator is dropped before the stems are
decoded back, so peak memory is the model's (~2.7 GB on a 4-minute song), not the model's plus the
stems'. `Engine::place_stems` then adds them as one undo step and checks first that the clip still
plays the same source the same way inside the separated region (else an error and the files go).
The stems stay at 44.1 kHz whatever the source's rate (any rate plays; converting back would cost a
second filter). Stretch, pitch, gain, fades, pan, the clip's insert and root are copied to the stem
clips, so they play exactly what the clip played (tested by rendering: stems that sum to the region
render like the muted clip); pitch-editor notes are not (they belong to the analysis of the mix).
Regions over 30 minutes are refused (`Audio::load` would cut the stems on reopening).

## Updates and release notes

The app updates itself from GitHub Releases (public repository: no token). See
[releasing.md](releasing.md) for how a release is made and signed.

* `app/src/updater.rs` wraps `tauri-plugin-updater`: `update_check` (finds a newer
  `latest.json`), `update_download` (progress on the `update-progress` event, signature
  verified, nothing restarts) and `update_install` (detaches plugin editors, stops audio, runs
  the installer, restarts; on failure audio comes back and the old version keeps running).
  `app_info` (also `pluginEditors`: plugin windows can be shown in this session, and `pluginEditorsWhy` when not), `update_set_auto` and `update_mark_seen` read and write `settings.json`
  (`seenVersion`, `noAutoUpdate`). Views reach them only through `backend.ts`
  (`appInfo`, `updateCheck`, …); `api/mockUpdates.ts` is the browser mock with a fake newer release.
* `ui/src/state/updateCtl.ts` is the state machine
  (`idle → available → downloading → ready → installing`); `state/updates.ts` is its signal.
  An automatic check (8 s after start, then every 6 h, can be switched off in Settings) never
  opens a dialog, only a status-bar button; downloading happens in the background; the restart
  needs a click and the unsaved-changes question.
* **What's new** reads the `CHANGELOG.md` bundled into the UI (`lib/changelog.ts`) and opens
  once at the first start of a new version, for every version since `seenVersion`. A fresh
  install shows nothing; it can be reopened from Settings → General → Updates.
* The updater is app-shell behaviour, not a project action, so it has no `synkopa` CLI command.

### Audio clips share their sample's sound; per-sample routing

Like FL's audio clip channel, every audio clip of one source shares its *sound*: volume, pan, pitch,
normalize, reverse and the mixer insert (`ui/src/lib/clipOps.ts` `SHARED_SOUND`). The clip editor
writes an edit to all of them in one `putClips` op, and a new clip of the source starts with it.
The region, the fades, the stretch and pitch-editor notes stay each clip's own; *Make unique* splits a
clip off with its own copy of the source. `Clip.insert` (audio clips only) routes the clip past its
track's insert, like a routed channel: the track still mutes it and scales it with its fader
(`rt.rs` mixes it straight into that insert's input). Removing the insert sends the clip back to its
track. `Project::clip_insert` resolves where a clip is heard.

### Song end

SONG playback wraps, and a song export stops, at the bar the last clip ends in
(`Project::song_end`, `ui/src/lib/export.ts` `songEnd`). A clip running into the next bar by less
than a 16th (`SONG_END_SLACK`: an audio file a hair longer than its bars) does not add that bar; a
clip that starts in a bar always keeps it.
