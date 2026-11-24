# Music Box

A built-in modeled instrument: a cylinder music box whose sound is synthesized from the
physics of a comb tine. Channel kind `musicbox`, like Synkopa Instruments but with no samples and
nothing to install — the DSP is in the program.

A 47 s demo render (Für Elise, two Music Box channels) lives at
[`docs/musicbox-demo.mp3`](musicbox-demo.mp3).

## Sound

A comb tine is a steel bar clamped at one end and plucked by the cylinder's pin, so it rings
at a few flexural modes of a cantilever: an almost pure fundamental with sparse, strongly
inharmonic overtones (1 : 6.267 : 17.55 : 34.39, from the ideal-cantilever mode ratios
(1.875, 4.694, 7.855, 10.996)²) whose decay times are several times shorter than the
fundamental's, plus a short mechanical tick where the pin lets go. The instrument sums four
decaying sines per note (fundamental + 3 overtones), a filtered noise tick, and two fixed
body resonators (185 / 330 Hz) for the wooden box. Velocity brightens the tine: a harder
pluck excites more of the overtone stack, not just the level. Long bass tines ring longest,
thin treble tines die quickest, so decay scales with pitch like on a real comb.

## Knobs

- **Ring** (25–250 %) — scales the natural decay time of the note's fundamental; 100 % plays
  the natural decay (a few seconds on bass tines, ~2 s on treble).
- **Brightness** (0–100 %) — how hard the pluck excites the overtone stack (velocity adds to
  this, so a hard hit both brightens and loudens).
- **Body** (0–100 %) — the box's two resonances mixed under the tines.
- **Mechanical** (0–100 %) — the pin tick at the start of every note.
- **Spread** (0–100 %) — how far the comb's notes sit across the stereo image.
- **Release** (40–3000 ms) — how long a note rings after its key is let go (the tine decays
  naturally while held).

All six are automatable (target kind `musicBox`) and editable from the CLI:
`channels --musicbox`, then `--ch "Music Box" --param ring --value 140`.

## Tests

`crates/engine/tests/musicbox.rs` covers pitch across the keyboard, the brighter/louder hard
pluck, the overtone-dies-first envelope, gate/release behavior, ops validation, serde with a
bare `{"kind":"musicbox"}` (old projects load unchanged), chord linearity, headroom +
determinism, and click-free overlapping rings. The unit tests in `musicbox.rs` pin the
cantilever ratios, the faster overtone decay, velocity→brightness, tick length and knob
ranges.
