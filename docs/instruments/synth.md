# Synthesizer

The built-in synthesizer is a two-oscillator wavetable synth for basses, 808s, leads, pads, plucks and
keys: pick one of its 50 factory presets and shape it with a few knobs, or build a sound from scratch.

![The synthesizer window](../../site/img/synth.png)

## Quick start

1. In the Channel Rack, click **+ Channel** below the last channel.
2. Under **Synthesizer**, open a category (808, Bass, Pluck, Lead, Pad, Keys, Bells, Brass/Strings, Arp,
   Lo-fi, FX) and click a preset. A new channel appears with that sound.
3. Click the channel's name to open its window.
4. Play it from a MIDI keyboard, or press `F7` to draw notes in the Piano roll.
5. Click **‹** or **›** next to the preset name to step through presets while a loop plays. Turn the
   **Macros** knobs to change the sound without touching the details.

## Find a sound fast

The preset name at the top opens a menu of every preset by category. The arrows on either side step to
the previous or next preset, so you can audition presets against your beat.

Most presets put their main moves (brightness, growl, width, drive) on **Macro 1** and **Macro 2**. Each
macro is named after what it controls, for example "Cutoff". A macro that controls nothing yet says
**Macro 3** and is dimmed. To give it a job, route it in **MOD → Modulation (source → target)**.

Once you turn a knob, the name changes to **Custom sound**, marked **edited**: the sound is yours now and
is saved with the project. Loading a preset is one undo step (`Ctrl+Z`).

## Make an 808 or a bass slide

1. Load a preset from **808** (for example **808 Glide**), or set the voice mode to **Legato**.
2. Turn up **Glide**: it sets how long the slide takes.
3. In the Piano roll, overlap two notes. In **Legato**, the second note slides from the first without
   restarting the envelopes, so it sounds like one sustained note bending.

**Mono** plays one note at a time too, but restarts the envelopes on every note, and with Glide on every
note slides from the previous one, overlapping or not. In **Poly** mode with Glide on, notes struck
together (within 30 ms) are treated as one chord: they all slide from the note played before the chord,
never from each other.

## Make a sound brighter, darker or fatter

- **Brighter or darker:** turn **Cutoff** in the **Filter** card. **Resonance** adds a peak at the
  cutoff. The graph shows the filter curve.
- **Pluck or "wah" on every note:** set the filter **Envelope** amount above 0, then shorten
  **MOD → Mod envelope**'s **Decay**. The cutoff opens on each note and closes again.
- **Wider and thicker:** in **Unison**, choose 3, 5 or 7 copies per oscillator, then raise **Detune**
  and **Width**.
- **More weight:** add **Sub** (one or two octaves below) in **Sub / Noise**.
- **Grit:** raise the filter's **Drive** (on each note) or **FX → Drive** (on the whole synth). Use the
  filter's Drive on chords: it keeps every note clean against the others.

## Add movement

- **Vibrato:** in **MOD → Modulation**, route **LFO 1** to **Pitch** with a small amount. Raise the
  LFO's **Rise** so the vibrato fades in after each note starts, like a singer.
- **Rhythmic wobble:** set an LFO's rate menu from **Hz** to a note value (1/4, 1/8T, 1/8D...) so it
  follows the project tempo, and route it to **Cutoff**.
- **Wavetable sweeps:** pick a wavetable in an oscillator (Basic, Formant, Harmonics, Sync, Growl, Comb).
  Its **Position** knob scans through the table. Route an LFO or the Mod envelope to **Osc 1 shape**.
- **Warp:** each oscillator's warp menu bends its waveform (**Sync**, **Bend**, **Asym**, **FM**), and
  the **Warp** knob sets how much. FM uses the other oscillator as the modulator, even when that
  oscillator's level is 0.

Every knob can be automated: right-click it and create an automation clip.

## Controls

The window has a top bar, the **Macros** row, and three tabs: **OSC**, **MOD**, **FX**.

| Control | What it does | Try |
| --- | --- | --- |
| Preset name, **‹ ›** | Opens the preset menu; arrows step through presets | Step through Pads over a chord loop |
| **Poly / Mono / Legato** | Chords; one note at a time; one note that slides between overlapping notes without retriggering | Legato for 808 slides |
| **Glide** | Slide time between notes (0–2000 ms) | 40–120 ms for bass |
| **Output** | Synth output level (−30 to +12 dB) | Lower it before it clips the mixer |
| **Macros** | Four knobs that drive whatever the matrix routes to them | Sweep Macro 1 while playing |
| **Oscillator 1 / 2** | Waveform menu, **Level**, **Shape** (or **Position** on a wavetable), **Coarse / st**, **Fine** (osc 2) | Osc 2 a little detuned against osc 1 |
| Warp menu, **Warp** | Bends the oscillator's cycle: Sync, Bend, Asym, FM | Sync with an LFO on it for a lead |
| **Sub / Noise** | **Sub** level at −1 or −2 octaves; **Noise** level and **Colour** | A touch of noise on plucks |
| **Filter** | Type (LP 12, LP 24, BP, HP, Notch), **Cutoff**, **Resonance**, **Drive**, **Envelope**, **Key** | LP 24 with Envelope for a classic bass |
| **Unison** | Copies per oscillator (1, 3, 5, 7), **Detune**, **Width** | 7 copies for a supersaw |
| **Amp envelope** | **Attack**, **Decay**, **Sustain**, **Release** of the volume | Slow attack for pads |
| **MOD → Mod envelope** | A second envelope for the filter and the matrix | Short decay for plucks |
| **MOD → LFO 1 / 2** | Shape, **Rate** (Hz or tempo), **Rise** (fade-in time per note) | 1/8 sync on Cutoff |
| **MOD → Modulation** | 8 slots: source → target with an amount (−100 to 100 %) | Velocity → Cutoff for dynamic playing |
| **FX** | **Drive** (overdrive), **Chorus**, **Delay** (amount, **Feedback**, tempo time), **Reverb** (amount, **Size**) | A little chorus on keys |
| **Volume**, **Pan**, **Root note** | The channel's own level, pan, and the key a Channel Rack step plays | Root note C2 for a bass line drawn in steps |

Modulation sources: Mod env, LFO 1, LFO 2, Velocity, Mod wheel, Random, Key, LFO 1 × wheel, Macro 1–4.
Targets: Pitch, Osc 1/2 pitch, Osc 1/2 shape, Osc 1/2 level, Cutoff, Resonance, Pan, Detune, Drive, Amp,
Noise, Osc 1/2 warp.

## Tips

- Chords are 12 dB quieter than a single note on poly presets, so a six-note chord does not clip. Nothing
  compresses the synth: every note in a chord sounds as it does alone.
- **FX → Drive** works on all notes together, so on a chord it adds harsh intermodulation. Factory poly
  presets get their grit from the filter's Drive instead.
- A preset change or knob move on held notes is smooth; you can turn knobs while playing.

## Technical notes

For scripting, automation and contributors.

### Project format and CLI

A synth channel is `{"kind": "synth", "synth": {"preset": "808 Deep" | null, "params": {"flt_cutoff": 900, ...}}}`.
A missing parameter id plays its default; unknown ids are rejected by `putChannel`. Projects without synth
channels load unchanged (serde defaults).

```sh
synkopa channels --synth "808 Deep"                # a synth channel with a factory preset ("" = blank patch)
synkopa channels --ch ID --preset "Pad Warm"       # load a preset
synkopa channels --ch ID --param flt_cutoff --value 900   # one parameter
synkopa catalog                                    # effects.synth: every parameter id, range and preset
```

Automation target: `{kind: "synth", chanId, param}` with the parameter ids below.

Parameter ids by section (ranges in the catalog): `osc1` (`o1_wave`, `o1_shape`, `o1_level`, `o1_coarse`,
`o1_warp`, `o1_wamt`), `osc2` (the same with `o2_`, plus `o2_fine`), `subnoise` (`sub_level`, `sub_oct`,
`noise_level`, `noise_color`), `unison` (`uni_voices`, `uni_detune`, `uni_width`), `filter` (`flt_type`,
`flt_cutoff`, `flt_res`, `flt_drive`, `flt_key`, `flt_env`), `ampenv` (`amp_a`, `amp_d`, `amp_s`, `amp_r`),
`modenv` (`mod_a` ... `mod_r`), `lfo1`/`lfo2` (`l1_wave`, `l1_rate`, `l1_sync`, `l1_rise`), `matrix`
(`m1_src`, `m1_dst`, `m1_amt` ... `m8_*`), `voice` (`v_mode`, `v_glide`), `fx` (`fx_drive`, `fx_chorus`,
`fx_delay`, `fx_dtime`, `fx_dfb`, `fx_reverb`, `fx_rsize`, `fx_out`), `macros` (`macro1` ... `macro4`).

Factory presets (`crates/engine/src/poly/presets.rs`), by category:

- **808:** 808 Deep, 808 Distorted, 808 Glide, Sub Clean
- **Bass:** Reese Moving, Growl Bass, FM Bass, Sync Bass, Formant Bass, Acid 303, Warm Bass, Click Bass
- **Pluck:** Pluck Glass, Pluck Future, Pluck Progressive, Pluck Nylon, Pluck Comb, Pluck Vocal
- **Lead:** Supersaw Hymn, Lead Sync, Lead Whistle, Lead Creamy, Lead Hyper, Lead Chip
- **Pad:** Pad Warm, Pad Choir, Pad Motion, Pad Dark, Pad Glass
- **Keys:** EP FM, Keys Soft, Organ, Future Chord, Keys House
- **Bells:** FM Bell, Vibraphone, Marimba, Kalimba
- **Brass/Strings:** Strings, Brass, Flute
- **Arp:** Arp Digital, Arp Trance, Arp Sync
- **Lo-fi:** Lo-fi Keys, Lo-fi Pad
- **FX:** Uplifter, Sub Drop, Wind Noise, Impact

### UI contract (`ui/src/api`, `ui/src/lib/synth.ts`)

- `Catalog.synth: SynthCatalog = { sections, categories, presets }`. Sections in panel order:
  `osc1, osc2, subnoise, unison, filter, ampenv, modenv, lfo1, lfo2, matrix, voice, fx, macros`,
  each `{ id, name, params: ParamDef[] }` (choices carry `options`). Presets carry the complete
  parameter set. `waves[wave][frame][point]` is one cycle (96 points, peak 1) of every frame of
  every oscillator wave, read from the engine's own tables: the panel draws them, with the warp
  applied, instead of an approximation. The browser mock serves the generated copy
  `ui/src/api/synthCatalog.ts` (regenerate: `WAVE_UPDATE_UI=1 cargo test -p wave-engine --test synth`;
  a test fails when it is stale).
- New channel: `newSynthChannel(catalog.synth, id, presetName | null)` then the `putChannel` op.
  Knob edit: `withSynthParam(...)` + `putChannel` (fast path, no timeline rebuild, held notes
  follow). Preset: `withSynthPreset(...)` + `putChannel`.

### Signal flow (`crates/engine/src/poly`)

Per voice: two wavetable oscillators. Eight classic waves (Sine, Triangle, Saw, Square, Hollow, Air,
Digital, Metal) have 2 frames and Shape morphs between them; Square's Shape is PWM, built as the difference
of two phase-offset band-limited saws, so PWM is as alias-free as the tables (no PolyBLEP needed). Six
wavetables have 8 frames and Shape is the position, scanned with neighbouring frames crossfaded:
**Basic** (sine → triangle → saw → square → narrow pulse), **Formant** (vowels A E I O U), **Harmonics**
(a sine gaining harmonics up to a saw), **Sync** (a hard-synced saw, ratio 1 → 8), **Growl** (phase
modulation, index rising) and **Comb** (a saw under a moving comb). They are defined by their spectra,
so sync and PM sweeps played from them are alias-free.

Each oscillator has a **warp** (a phase warp before the table read, amount modulatable): **Sync**
(ratio 1–8 to its own cycle; the reset is softened by a raised-cosine dip at least eight samples wide),
**Bend** (cubic, content pushed to the cycle's ends), **Asym** (two smooth sine phase-distortion stages;
a kinked two-segment PD aliased at 25 dB) and **FM** (linear phase modulation by the other oscillator's
unison copy, ±1 cycle at 100 %; osc 2 runs as the modulator even at level 0). The mip level is chosen
for the warp's fastest read rate, so the table itself never plays above Nyquist.

Unison 1/3/5/7 per oscillator with symmetric detune and stereo width, sub (−1/−2 oct), noise (colour
tilt) → per-voice drive (rational tanh, 2× oversampled) → Simper/Cytomic TPT SVF (LP12, LP24 = two
cascaded, BP, HP, notch; key tracking, env and LFO amounts in octaves; the filter's Envelope amount
follows the mod envelope) → amp ADSR × velocity → pan. Envelopes: exponential (Redmon-style overshoot
targets) amp and mod ADSRs. Two LFOs (sine/tri/saw/square/S&H, Hz or tempo sync) with **Rise**: each
voice fades the LFO's depth in over the rise time from its note's start (delayed vibrato; a legato slide
keeps counting). 8-slot mod matrix (13 sources incl. Macro 1–4, 16 destinations incl. both warp
amounts). Continuous parameter changes glide over about 12 ms on sounding notes.

16 voices plus 4 fade slots: a stolen voice fades over 4 ms (smoothstep curve, like the mono take-over
fade-in: a linear fade's corners tick on loud low notes). Poly / mono / legato with glide (808 slides).
In poly mode, notes struck within 30 ms of each other are one chord: with glide on they all slide from
the note played before the chord, never from each other. Global: drive (the engine saturator core,
triode voicing with auto gain, always running so its latency never moves), stereo chorus, tempo
ping-pong delay, `reverb_v2`, output trim, chord headroom.

**Chords.** Polyphonic patches play 12 dB below their mono level (`POLY_HEADROOM`), so a six-note chord
of a pluck or keys patch (8–13 dB above one note) still lands near full scale. Nothing limits or
compresses the instrument output: every note sounds in a chord exactly as it does alone
(`a_chord_is_the_sum_of_its_notes`). The global drive works on the sum of all notes, so on a chord it
adds intermodulation (−8 to −17 dB residual on a four-note chord at 12–40 %); poly factory presets
therefore get their grit from the per-voice drive instead. Synkopa Instruments plays its recordings
12 dB lower for the same reason (one loud note −11 dBFS, six −5.7, ten −1.8 dBFS on the Salamander bank).

Wavetables: harmonics capped at 256 per 2048-sample frame, third-octave mip levels crossfaded by pitch,
so nothing above Nyquist is ever played. Real time: tables are built once off the audio thread,
instances are made by the engine and sent through the command ring, old instances / patches go back
through the garbage ring; no allocation, locks or I/O in render; denormals flushed in every feedback
path. Note events are sample-accurate (sorted per block, note-offs before note-ons at the same sample).

### Measurements

Run with `cargo run --release -p wave-engine --example synth_lab -- alias|warps|clicks|cpu|levels|demos DIR`.

- **Alias SNR** (1 s tone, harmonics vs everything else), MIDI 24–108, worst: saw 63.3 dB, square
  65.4 dB, 25 % pulse 63.7 dB (≥ 80 dB from C4 up). Target > 60 dB to C7: met.
- **Wavetables and warps** (`synth_lab warps`, ≤ 20 kHz, worst over MIDI 36–96): wavetables 56–91 dB
  across their positions (Sync table 56 dB at full position, Comb 62 dB). Warps on a saw: Sync ≥ 57 dB
  to C6 (48 dB at C7), Bend 52 dB to C6, Asym ≥ 60 dB to C6, FM ≥ 81 dB to C6 (55 dB at C7). Live warps
  trade some top-octave purity for movement, like any non-oversampled warp.
- **Clicks** (spike/step detector on the 2nd difference): 0 on all 32 presets measured for note on/off,
  retrigger, 20-note steal, legato+glide, mono+glide and preset change on held notes. The detector misses
  a saw cut exactly on its ramp (by design: it must not flag saw edges), and two distorted 808
  square-ish waves whose edges coincide in poly overlaps read as a spike, so the test leaves that one
  case out.
- **CPU**, 16 voices × 2 osc × 7 unison, LP24 + drive, every global effect, 48 kHz / 64-sample blocks:
  mean 16.8 % of one core, p99 block 343 µs, worst 705 µs (budget 1333 µs).
- **Loudness** (one 2 s note at velocity 100, C3 for 808/basses, C4 otherwise; trims set by
  measurement): −19 LUFS (808, bass, pads) / −18 LUFS (others) within ±0.1, true peak ≤ −3 dBTP;
  polyphonic presets 12 dB lower (the chord headroom). Short, peaky presets (plucks, arps, mallets, FX
  hits) hit the true-peak ceiling first and read lower in LUFS (`synth_lab levels` lists them);
  integrated loudness of a decaying single note understates how loud they sound in a line.
- **Factory bank:** 50 presets, reviewed by ear over three rounds (listening notes on rendered demos
  per category; final round 8.5/10, nothing below 7.5).

Tests: `crates/engine/tests/synth.rs`.

## See also

- [Choosing an instrument](README.md)
- [Synkopa Instruments](synkopa-instruments.md): sampled pianos, strings and guitars
- [Sampler](sampler.md)
- [CLI](../cli.md)
