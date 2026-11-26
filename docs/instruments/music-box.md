# Music Box

The Music Box is a built-in cylinder music box for lullabies, intros and delicate bell-like melodies.
Its sound is computed from the physics of a plucked steel tine, so there are no samples and nothing to
download.

Listen: [a 47 s demo](music-box-demo.mp3) (Für Elise, two Music Box channels).

## Quick start

1. In the Channel Rack, click **+ Channel**, then **Music Box — modeled tines**.
2. Click the new channel's name to open its window, and play the on-screen keyboard (C3 to C8) or a
   MIDI keyboard.
3. Draw a melody in the Piano roll (`F7`). Music boxes sound best in the upper octaves.
4. Turn **Ring** up for long, overlapping notes, or down for a dry, plucky box.

## Shape the sound

- **A hard hit sounds brighter, not just louder.** Velocity excites more of the tine's overtones. Vary
  velocities in the Piano roll for a more mechanical, real feel.
- **Low notes ring longer than high ones**, like on a real comb. **Ring** scales all of them together.
- **Make it more realistic:** raise **Mechanical** (the pin's tick at the start of each note) and
  **Body** (the wooden box resonating under the tines).
- **Make it cleaner and more bell-like:** lower **Mechanical** and **Body**, raise **Brightness**.
- **Wider:** raise **Spread** to fan the notes across the stereo image.

A note keeps ringing and decaying naturally while its key is held. **Release** sets how long it takes
to fade after you let go.

## Controls

| Control | What it does | Try |
| --- | --- | --- |
| **Release** | How long a note fades after its key is let go (40 ms – 3 s, default 400 ms) | 1.5 s for a dreamy wash |
| **Ring** | Decay time as a share of the tine's natural decay (25–250 %, default 100 %) | 60 % for a tight, staccato box |
| **Brightness** | How hard the pin excites the overtones (0–100 %, default 45 %); velocity adds to it | 70 % for a sparkly top line |
| **Body** | The wooden box's resonance under the tines (0–100 %, default 30 %) | 60 % for a warmer, boxier tone |
| **Mechanical** | The tick of the pin at the start of every note (0–100 %, default 25 %) | 0 % for a pure bell |
| **Spread** | How far the notes sit across the stereo image (0–100 %, default 35 %) | 70 % on a solo intro |
| **Volume**, **Pan** | The channel's own level and pan | |

Double-click a knob to reset it. Right-click it to automate it. Right-click a key on the window's
keyboard to set the channel's root note (the key a Channel Rack step plays).

## Technical notes

For scripting, automation and contributors.

**Model** (`crates/engine/src/musicbox.rs`). A comb tine is a steel bar clamped at one end and plucked
by the cylinder's pin, so it rings at a few flexural modes of a cantilever: an almost pure fundamental
with sparse, strongly inharmonic overtones (1 : 6.267 : 17.55 : 34.39, from the ideal-cantilever mode
ratios (1.875, 4.694, 7.855, 10.996)²) whose decay times are several times shorter than the
fundamental's, plus a short mechanical tick where the pin lets go. The instrument sums four decaying
sines per note (fundamental + 3 overtones), a filtered noise tick, and two fixed body resonators
(185 / 330 Hz) for the wooden box. Velocity brightens the tine: a harder pluck excites more of the
overtone stack, not just the level. Decay scales with pitch: at **Ring** 100 % a few seconds on bass
tines, about 2 s on treble ones.

**Project format, automation and CLI.** Channel kind `musicbox`; a bare `{"kind":"musicbox"}` loads
with every knob at its default. Knob ids: `ring`, `brightness`, `body`, `mechanical`, `spread`,
`release`. All six are automatable (target kind `musicBox`).

```sh
synkopa channels --musicbox
synkopa channels --ch "Music Box" --param ring --value 140
```

**Tests.** `crates/engine/tests/musicbox.rs` covers pitch across the keyboard, the brighter/louder hard
pluck, the overtone-dies-first envelope, gate/release behavior, ops validation, serde with a bare
`{"kind":"musicbox"}` (old projects load unchanged), chord linearity, headroom + determinism, and
click-free overlapping rings. The unit tests in `musicbox.rs` pin the cantilever ratios, the faster
overtone decay, velocity→brightness, tick length and knob ranges.

## See also

- [Choosing an instrument](README.md)
- [Synkopa Instruments](synkopa-instruments.md): the sampled Vibraphone, for a real mallet sound
- [Synthesizer](synth.md): the **Bells** presets
