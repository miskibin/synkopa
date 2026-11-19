# Built-in synth

Channel Rack channels of kind `synth` play the engine's own polyphonic synth
(`crates/engine/src/poly`). Project format: `{"kind": "synth", "synth": {"preset": "808 Głęboki" | null,
"params": {"flt_cutoff": 900, ...}}}`; a missing id plays its default, unknown ids are rejected
by `putChannel`. Projects without synth channels load unchanged (serde defaults).

## UI contract (`ui/src/api`, `ui/src/lib/synth.ts`)

- `Catalog.synth: SynthCatalog = { sections, categories, presets }`. Sections in panel order:
  `osc1, osc2, subnoise, unison, filter, ampenv, modenv, lfo1, lfo2, matrix, voice, fx, macros`,
  each `{ id, name, params: ParamDef[] }` (choices carry `options`). Presets carry the complete
  parameter set. `waves[wave][frame][point]` is one cycle (96 points, peak 1) of every frame of
  every oscillator wave, read from the engine's own tables: the panel draws them, with the warp
  applied, instead of an approximation. The browser mock serves the generated copy `ui/src/api/synthCatalog.ts`
  (regenerate: `WAVE_UPDATE_UI=1 cargo test -p wave-engine --test synth`; a test fails when stale).
- New channel: `newSynthChannel(catalog.synth, id, presetName | null)` then the `putChannel` op.
  Knob edit: `withSynthParam(...)` + `putChannel` (fast path, no timeline rebuild, held notes
  follow). Preset: `withSynthPreset(...)` + `putChannel`.
- CLI: `synkopa channels --synth "808 Głęboki"`, `--ch ID --preset NAME`, `--ch ID --param ID --value V`.

## Signal flow

Per voice: 2 wavetable oscillators. Eight classic waves have 2 frames (Shape morphs; Square Shape
= PWM, built as the difference of two phase-offset band-limited saws, so PWM is as alias-free as
the tables — no PolyBLEP needed). Six wavetables have 8 frames and Shape is the position, scanned
with neighbouring frames crossfaded: **Basic** (sine → triangle → saw → square → narrow pulse),
**Formant** (vowels A E I O U), **Harmonics** (a sine gaining harmonics up to a saw), **Sync** (a
hard-synced saw, ratio 1 → 8), **Growl** (phase modulation, index rising) and **Comb** (a saw
under a moving comb). They are defined by their spectra, so sync and PM sweeps played from them
are alias-free. Each oscillator has a **warp** (a phase warp before the table read, amount
modulatable): **Sync** (ratio 1–8 to its own cycle; the reset is softened by a raised-cosine dip at
least eight samples wide), **Bend** (cubic, content pushed to the cycle's ends), **Asym** (two
smooth sine phase-distortion stages; a kinked two-segment PD aliased at 25 dB) and **FM** (linear
phase modulation by the other oscillator's unison copy, ±1 cycle at 100 %; osc 2 runs as the
modulator even at level 0). The mip level is chosen for the warp's fastest read rate, so the
table itself never plays above Nyquist.
unison 1/3/5/7 per oscillator with symmetric detune and stereo width, sub (−1/−2 oct), noise
(colour tilt) → per-voice drive (rational tanh, 2× oversampled) → Simper/Cytomic TPT SVF
(LP12, LP24 = two cascaded, BP, HP, notch; key tracking, env and LFO amounts in octaves) →
amp ADSR × velocity → pan. Envelopes: exponential (Redmon-style overshoot targets) amp and mod
ADSRs. Two LFOs (sine/tri/saw/square/S&H, Hz or tempo sync) with **Rise**: each voice fades the
LFO's depth in over the rise time from its note's start (delayed vibrato; a legato slide keeps
counting). 8-slot mod matrix (sources: mod env, LFO 1/2, velocity, wheel, random, key,
LFO 1 × wheel, Macro 1–4; 16 destinations incl. both warp amounts). **Macros**: four knobs on
every tab, sources in the matrix; the panel names each after what it drives. Nearly every factory
preset puts its main gestures (brightness, growl, width, drive) on Macro 1 and 2.
16 voices plus 4 fade slots: a stolen voice fades over 4 ms (smoothstep curve, like the mono
take-over fade-in: a linear fade's corners tick on loud low notes). Poly / mono / legato with glide
(808 slides). In poly mode, notes struck within 30 ms of each other are one chord: with glide on
they all slide from the note played before the chord, never from each other. Global: drive (the
engine saturator core, triode voicing with auto gain, always running so its latency never moves),
stereo chorus, tempo ping-pong delay, `reverb_v2`, output trim, chord headroom.

**Chords.** Polyphonic patches play 12 dB below their mono level (`POLY_HEADROOM`), so a six-note
chord of a pluck or keys patch (8–13 dB above one note) still lands near full scale. Nothing
limits or compresses the instrument output: every note sounds in a chord exactly as it does
alone (`a_chord_is_the_sum_of_its_notes`). The global drive works on the sum of all notes, so
on a chord it adds intermodulation (−8 to −17 dB residual on a four-note chord at 12–40 %); poly
factory presets therefore get their grit from the per-voice drive instead. Synkopa Keys plays its
recordings 12 dB lower for the same reason (one loud note −11 dBFS, six −5.7, ten −1.8 dBFS on
the Salamander bank).

Wavetables: harmonics capped at 256 per 2048-sample frame, third-octave mip levels crossfaded
by pitch, so nothing above Nyquist is ever played. Real time: tables are built once off the
audio thread, instances are made by the engine and sent through the command ring, old
instances / patches go back through the garbage ring; no allocation, locks or I/O in render;
denormals flushed in every feedback path. Note events are sample-accurate (sorted per block,
note-offs before note-ons at the same sample).

## Measurements (`cargo run --release -p wave-engine --example synth_lab -- alias|clicks|cpu|levels|demos DIR`)

- Alias SNR (1 s tone, harmonics vs everything else), MIDI 24–108, worst: saw 63.3 dB,
  square 65.4 dB, 25 % pulse 63.7 dB (≥ 80 dB from C4 up). Target > 60 dB to C7: met.
- Wavetables and warps (`synth_lab warps`, ≤ 20 kHz, worst over MIDI 36–96): wavetables 56–91 dB
  across their positions (Sync table 56 dB at full position, Comb 62 dB). Warps on a saw: Sync
  ≥ 57 dB to C6 (48 dB at C7), Bend 52 dB to C6, Asym ≥ 60 dB to C6, FM ≥ 81 dB to C6 (55 dB at
  C7). Live warps trade some top-octave purity for movement, like any non-oversampled warp.
- Clicks (spike/step detector on the 2nd difference): 0 on all 32 presets for note on/off,
  retrigger, 20-note steal, legato+glide, mono+glide and preset change on held notes. The
  detector misses a saw cut exactly on its ramp (by design: it must not flag saw edges), and two
  distorted 808 square-ish waves whose edges coincide in poly overlaps read as a spike, so the
  test leaves that one case out.
- CPU, 16 voices × 2 osc × 7 unison, LP24 + drive, every global effect, 48 kHz / 64-sample
  blocks: mean 16.8 % of one core, p99 block 343 µs, worst 705 µs (budget 1333 µs).
- Loudness (one 2 s note at velocity 100, C3 for 808/basses, C4 otherwise; trims set by
  measurement): −19 LUFS (808, bass, pads) / −18 LUFS (others) within ±0.1, true peak ≤ −3 dBTP;
  polyphonic presets 12 dB lower (the chord headroom).
  Factory bank: 50 presets, reviewed by ear over three rounds (an audio model's listening notes
  on rendered demos per category; final round 8.5/10, nothing below 7.5).
  Short, peaky presets (plucks, arps, mallets, FX hits) hit the true-peak ceiling first and
  read lower in LUFS (`synth_lab levels` lists them); integrated loudness of a decaying single
  note understates how loud they sound in a line.
