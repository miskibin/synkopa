# Chorus and Phaser

Chorus thickens and widens a sound by mixing it with slightly detuned, moving copies of itself;
Phaser sweeps notches through the spectrum for a swirling, jet-like movement. Both are in the
**Modulation** category of the effect picker.

## Quick start

1. Add **Chorus** or **Phaser** to an insert (mixer, `F9`, then **+ effect**).
2. Leave **Mix** at 50 %: that is the classic sound for both.
3. Set **Rate** (how fast it moves) and **Depth** (how far).
4. For movement in time with the song, switch **Rate** from **Hz** to **Tempo** and pick a note
   value.

## Chorus

### Thicken a vocal or a synth

**Mode** picks how many copies are added:

| Mode | Copies per side | Good for |
|---|---|---|
| **Chorus** | 2 | classic thickening and width |
| **Ensemble** | 3, plus a faster shallow wobble | strings, pads, wide synths |
| **Vibrato** | 1, with no dry sound | pitch wobble only |

The right side moves a little later than the left, so a mono source turns stereo but still sums
to mono without cancelling.

### Keep the bass solid

Chorus on a bass or a full mix can smear the low end. **Safe bass** keeps everything below its
frequency out of the effect: set it to 100–200 Hz and the lows stay steady. The **Bass — steady
low end** preset does this.

### Controls

| Control | What it does | Try |
|---|---|---|
| **Mode** | Chorus, Ensemble or Vibrato | Chorus |
| **Rate** | Speed: **Hz** (0.05–10 Hz) or **Tempo** (1/16 to 8 bars) | 0.6 Hz |
| **Depth** | How far the copies move (Vibrato: how much the pitch wobbles) | 40 % |
| **Delay** | Base delay of the copies (2–40 ms; not in Vibrato) | 12 ms |
| **Feedback** | Feeds the copies back (−90 to +90 %); negative sounds hollow (not in Vibrato) | 0 %, or ±30 % for a flanger-like edge |
| **Width** | Stereo width of the copies only; the dry sound is untouched (0–200 %) | 100 % |
| **Safe bass** | Lows below this stay out of the effect (20–1000 Hz) | 150 Hz on bass |
| **Tone** | Darkens the copies, like vintage bucket-brigade chips (1–20 kHz) | 8 kHz for a vintage feel |
| **Mix** | Dry/wet balance | 50 % |

### Presets

Classic chorus, Vocal — thicken, Wide synth, Vintage — dual BBD, Clean guitar, 80s,
Bass — steady low end, Strings — ensemble, Tempo swirl (1 bar), Vibrato.

## Phaser

### Add movement to a pad or guitar

A *phaser* filters the sound with notches (narrow dips) that sweep up and down. **Stages**
decides how many notches: 2 stages give one, 12 give six. **Center** is the middle of the sweep
and **Depth** how far it goes (up to ±3 octaves).

### Make it more intense

Raise **Feedback** to turn the notches into resonant peaks (the "jet" sound). Negative
feedback moves the notches and gives a hollow sound. **Stereo** offsets the right side's sweep
(up to 180°) for a swirl across the speakers.

### Controls

| Control | What it does | Try |
|---|---|---|
| **Stages** | 2, 4, 6, 8 or 12; more stages = more notches | 4 |
| **Center** | Middle of the sweep (100 Hz – 8 kHz) | 800 Hz |
| **Depth** | How far the sweep goes | 70 % |
| **Rate** | Speed: **Hz** (0.02–10 Hz) or **Tempo** (1/16 to 8 bars) | 0.4 Hz, or 2 bars on a pad |
| **Stereo** | Right side's sweep offset (0–180°) | 90° |
| **Feedback** | Hollow (negative) to resonant (positive), −95 to +95 % | 30 % |
| **Mix** | Dry/wet; 50 % gives the deepest notches | 50 % |

### Presets

Classic, 4 stages, Gentle, Deep sweep, Funk — guitar (1/8), Jet, Stereo swirl,
Hollow (negative feedback), Pad sweep (2 bars).

## Tips

- Synced to the tempo, both effects follow the song position while it plays.
- The Chorus window shows where each copy reads right now and the range it sweeps; the Phaser
  window shows the frequency response with the notches moving.

## Technical notes

Code: `crates/engine/src/dsp/modulation.rs`; windows: `ui/src/views/fx/modulation.ts`.

**Chorus.** The input is mixed with copies read from a delay line whose length an LFO sweeps.
Chorus: 2 copies per side, at `delay` and 1.37 × `delay`. Ensemble: 3 copies, a third of the LFO
cycle apart, plus a fast shallow second LFO. Vibrato: 1 copy, no dry signal (`delay` and `fb` do
not apply; the low end below Safe bass passes dry). The right side reads the LFO a quarter cycle
later (Ensemble: a sixth). Depth sweeps ±¼ of `delay` (at most ±6 ms); Vibrato up to ±6 % pitch.
Feedback goes through a soft clipper. `mix` is a linear dry/wet.

Chorus ids (order = `dsp::modulation::chorus_idx`): `mode`, `sync` (Hz / Tempo), `rate`
(0.05–10 Hz), `div` (1/16 … 8 bars), `depth`, `delay` (2–40 ms), `fb` (−90…+90 %), `width`
(0–200 %, mid/side of the copies), `hp` (Safe bass, 20–1000 Hz high-pass before the delay line),
`tone` (1–20 kHz low-pass on the copies), `mix`.

**Phaser.** 2, 4, 6, 8 or 12 first-order all-pass stages whose break frequency a triangle LFO
sweeps exponentially around `center`, ±3 octaves at full `depth`, equal time per octave. Mixed
50/50 with the dry signal, every two stages cut one notch; `fb` (last stage back to the first,
through a soft clipper) turns the notches into resonant peaks, negative feedback moves them.

Phaser ids (order = `dsp::modulation::phaser_idx`): `stages`, `sync`, `rate` (0.02–10 Hz), `div`,
`center` (100 Hz – 8 kHz), `depth`, `fb` (−95…+95 %), `stereo` (0–180°), `mix`.

**Behaviour.**

- Continuous parameters glide over ~20 ms, so automation and knob moves do not click.
- No latency; the output does not depend on the block size or the number of render workers.
- The phaser's notches sit at the same frequencies at 44.1, 48 and 96 kHz.
- Cost (10 s of audio at 48 kHz, one core): Ensemble about 150×, a 12-stage phaser about 200×
  faster than real time.

**Meters.** Chorus: `[0]` LFO phase, `[1..4]` left voices' delay ms, `[4..7]` right voices',
`[7]` voices. Phaser: `[0]`/`[1]` break frequency left/right, `[2]` stages, `[3]` LFO phase.

**Command line.** `synkopa add-fx --insert 3 --type chorus --preset 1`, then `set-param` and
`preset` as for any built-in effect. Preset names are stored in Polish in the engine and shown
translated in the English UI (`ui/src/i18n/names-fx.ts`).

## See also

- [Delay](delay.md): the **Chorus / doubling** preset for a wider, delay-based double.
- [Filter](filter.md): LFO-driven filter sweeps.
- [All effects](README.md)
