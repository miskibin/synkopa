# Warp

Warp replays the sound through a drawn *time map* and *volume map*, locked to the song's bars,
for half-time, stutters, rolls, tape stops, reverses and scratches, and for gates, trance gates
and sidechain-style pumps, without cutting or bouncing audio. It covers the same ground as
Image-Line's Gross Beat.

## Quick start

1. Add **Warp** to an insert (mixer, `F9`, then **+ effect**).
2. Start playback (pattern or song). Warp works only while the transport runs.
3. Click a slot on the **TIME** strip at the top: it starts playing and its map opens below.
   Slot 1 is **No change**; the others hold factory effects such as half-time, stutter and tape
   stop.
4. Switch to the **VOLUME** strip and click a slot for a gate or a pump.
5. Right-click any slot and pick a map from the **Library** to load a different effect into it.

## How the maps work

Each map covers one bar. Warp keeps the last two bars of audio in a buffer and reads from it.

- **Time maps**: the top of the map is "now" (the live input); lower reads further back, up to 8
  beats. The slope of the line is the playback speed:
  - flat = normal speed;
  - following the dashed line = stopped;
  - following the grey guides = reverse at normal speed;
  - a vertical step = a jump (a stutter or a repeat).
- **Volume maps**: the height is the volume.

Hover a time map to read the speed at that point ("×0.50 · −12.0 st", "stopped",
"reverse ×1.00").

A map that reaches further back than Warp has heard since playback started is silent there; the
editor shades that part as "not heard yet". It never plays stale audio.

## Edit a map

- Click empty space to add a point, then drag it. Shift = value only, Ctrl = time only, Alt = no
  snap (the grid is a 1/16 note).
- Drag the round handle between two points to bend that segment.
- Double-click or Alt+click a point to delete it; a handle to straighten its segment.
- Right-click a point to change its segment's shape, and on time maps to set a musical speed
  (×1, ×½, −7 st, ±1 st, stop, reverse ×1).

Each edit is one undo step.

## Switch effects over the song

Clicking a slot switches immediately. To change slots on the beat, set **Trigger sync** to a
note value: the change waits for the next boundary. To switch slots over the song, right-click a
slot and choose **Automate slot changes…**: an automation clip of the slot with hold points.

- **Position**: **Song** plays the slot in step with the bar; **Trigger** plays it from its
  start when you switch.
- **One-shot**: a slot other than 1 plays for one bar, then Warp goes back to slot 1.
- **Hold**: **Previous bar** loops the bar before the current one; **Next bar** records the
  coming bar and loops it.

## Controls

| Control | What it does | Try |
|---|---|---|
| **Trigger sync** | When a slot change takes effect: Off, 1/16, 1/8, 1/4, 1/2 or 1 bar | 1/4 for live switching |
| **Position** | **Song** (bar phase) or **Trigger** (from the slot's start) | Song |
| **One-shot** | A slot plays one bar, then slot 1 | On for fills |
| **Hold** | Off, **Previous bar** or **Next bar** loop | Previous bar for a breakdown |
| **Time mix** | How much of the time effect you hear | 100 % |
| **Volume mix** | How much of the volume map you hear | 50 % for a softer pump |
| **Attack**, **Release** | Smooth the volume map's rises and falls | 5 ms, 20 ms against clicks |
| **Tension** | Bends every curve segment of the volume map | |
| **Click reduction** | Under **Advanced**: crossfades jumps so they do not click | On |
| **Attack protect** | Under **Advanced**: a shorter crossfade that keeps attacks | On on drums |
| **DC removal** | Under **Advanced**: removes DC offset | On |

Use **Time mix** and **Volume mix** for blends rather than the header's dry/wet: the dry path is
delayed to match, so a blend has no comb filtering.

## Slot menu

Right-click a slot: **Library** (ready-made maps by category), **Copy slot**, **Paste slot**,
**Clear slot (no change)**, **Restore factory slot**, **Automate slot changes…**. An `*` after a
slot's name marks a factory slot you have changed.

The library holds 51 time maps (slow-down, stutter and repeats, rolls and build-ups, tape,
reverse, scratch, glitch and speed) and 35 volume maps (sidechain, gates, trance gates,
transitions, mutes, tremolo).

## Common problems

- **Nothing happens.** Warp only works while the song or pattern plays. With the transport
  stopped, the sound passes through.
- **A reverse or a long stutter is silent at first.** Warp has not heard that audio yet: it
  needs the music to have played for as long as the map reaches back (up to two bars).

## Technical notes

Code: `crates/engine/src/dsp/warp.rs`; editor: `ui/src/views/fx/warp.ts`.

**Model.** `Fx.maps: { time: WarpSlot[], vol: WarpSlot[] }`, each bank up to 16 slots, a slot =
`{ name, points: AutoPoint[] }` over one bar (`t` 0..4 beats, `v` 0..1, the automation segment
shapes and tension). A new Warp holds the factory slots (`crates/engine/src/warp_factory.json`,
also `fx_catalog().warp` and the UI mock's copy, so the three never drift). `putFx` refuses a
Warp without maps, maps on another effect, more than 16 slots or 256 points, points outside the
bar or 0..1, and points out of time order.

- Time maps: `Y = (1 − v) · 8` beats back; the read position is `now − Y`, so the slope is the
  speed (dashed "safety line" −1 beat per beat = stopped, grey guides −2 = reverse at normal
  speed). A half-sine segment from flat to slope −1 is a tape stop.
- Segment shapes (shared with automation clips, `model::curve_at`, checked against
  `tests/fixtures/curves.json`): curve, double (S), hold, stairs, and for Warp: **pulse** (square,
  1–16 cycles from the tension), **wave** (cosine, an odd number of half cycles so it ends on its
  point), **half sine** (tension −1 ease in = tape stop, +1 ease out = tape start).
- Musical speed for a segment: slope = 2^(n/12) − 1.

**Map library.** `crates/engine/src/warp_library.json` (catalog `warpLibrary`, also read by the
UI mock). Loading a map is one undo step and the editor switches to that slot. The maps are
built from the musical idea: each time map is a list of "play the source from beat s at speed k"
pieces, so it never reads the future.

**Parameter ids** (order = `dsp::warp::idx`):

| id | values | notes |
|---|---|---|
| `tslot`, `vslot` | slot 1–16 | picked on the slot strip (a click edits and plays a slot, one undo step) |
| `trig` | Off, 1/16, 1/8, 1/4, 1/2, 1 bar | a change arriving within 1/32 beat after a boundary counts as on it (automation is evaluated per block) |
| `pos` | Song / Trigger | |
| `oneshot` | Off / On | |
| `hold` | Off / Previous bar / Next bar | the buffer stops writing while it loops; the volume map keeps playing; after a hold the buffer starts empty again |
| `tmix`, `vmix` | 0–100 % | the dry path is delayed by the latency |
| `att`, `rel`, `tns` | 0–500 ms, 0–1000 ms, ±100 % | |
| `click`, `attack`, `dc` | Off / On | de-click jumps; shorter crossfade; DC blocker (5 Hz) |

**Real-time behaviour.**

- The phase comes from `ProcessCtx::beat` per sample, never a free-running counter. The pattern
  loop preview counts as playing. Stopped transport: the input passes through (delayed by the
  latency), nothing is recorded.
- The ring holds 12.5 s per channel (two bars down to 40 bpm), indexed by absolute sample. It
  holds only what was heard since playback started or the playhead last jumped. Validity changes
  fade over 1 ms.
- Hermite interpolation; the read is at least 2 samples behind the write, so the effect reports
  `LATENCY` = 2 samples for compensation.
- Click reduction: a read position more than 4 samples off its own trajectory is a jump. The old
  trajectory keeps playing and crossfades linearly (heads often read the same material, where
  equal power would bump by 3 dB) into the new one over 3 ms, or 0.5 ms with Attack protect. The
  speed carries over a jump.
- Maps reach the audio thread through `Processor::swap_data` (`Cmd::FxData`): built off the
  audio thread, moved in, the old one sent back as garbage. Only changed maps are sent
  (`Core::sent_maps`); fresh processors get theirs in `build_graph`. No allocation in `process`.
- `tail_samples`: the ring length; `None` while a hold loops.

**Meters** (`Processor::meters`): `[0]` bar position 0..4 (−1 stopped), `[1]`/`[2]` playing
time / volume slot, `[3]`/`[4]` queued slot (−1 none), `[5]` time offset in beats (≤ 0), `[6]`
read speed, `[7]` volume gain, `[8]` buffered beats at 120 bpm, `[9]` hold (0 off, 1 recording,
2 looping), `[10]` buffered seconds.

**Command line.** Edits are `putFx` ops (a drag is one undo step), so scripts edit maps the same
way: `synkopa apply` with a `putFx` carrying `maps`.

**Not yet.** Triggering slots from MIDI keys needs MIDI routed to effects; it is a follow-up.
Slot names are not editable.

**Tests.** `dsp::warp::tests`: a flat map is the input delayed by the latency; a half-time slope
plays an octave down (220 Hz from 440); a −2 slope plays the past backwards; the factory stutter
repeats beat 1; reading before the recorded past is silent; the volume map gates in time; click
reduction keeps jump steps near the signal's own (and without it they click); trigger sync waits
for the boundary; Hold loops the previous bar and never parks; stopped transport passes through;
factory slots are valid. `tests/warp_and_delay.rs`: a map edit is heard at once on the running
audio thread; ops refuse bad maps; latency is reported. UI: `ui/src/lib/warp.test.ts`.

## See also

- [Delay](delay.md), [Filter](filter.md)
- [All effects](README.md)
