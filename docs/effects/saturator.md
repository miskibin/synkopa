# Saturator

The Saturator adds harmonics, warmth and grit, from a subtle tape glow on a vocal to a crushed
drum bus, without making the track louder as you turn it up.

![The Saturator window](../../site/img/saturator.png)

## Quick start

1. Add **Saturator** to an insert (mixer, `F9`, then **+ effect**).
2. Pick a **Style**: **Tape** for smooth warmth, **Triode** for a thick tube sound.
3. Turn **Drive** up. Up to about 50 % it adds warmth; above that it adds grit.
4. Leave **Auto gain** on **Auto**, so bypassing the effect compares the tone, not the volume.
5. Use **Mix** below 100 % to blend the saturated sound with the clean one.

## Choose a character

**Style** picks one of five voicings. Each one reacts differently to level and frequency:

| Style | Modelled on | Sound |
|---|---|---|
| **Tape** | tape preamp | smooth, highs pushed into the curve and rounded off |
| **Console** | solid-state console channel | bright, mostly odd harmonics, squashes transients |
| **Transformer** | transformer input | lows drive it harder, highs softened, big bottom |
| **Triode** | triode tube stage | thick mids, even harmonics that bloom on loud notes |
| **Pentode** | pentode tube stage | hard knee, odd harmonics, cold and aggressive |

Switching style morphs over about 20 ms, so you can switch while the track plays.

## Warm up a vocal

Load **Vocal — warmth** (Tape) or **Vocal — tube** (Triode). Both cut the lows before the stage
with **Low cut**, so breaths and rumble do not distort, and soften the top with **High cut**.
Keep **Drive** under 50 %.

## Add weight to bass and drums

- **Bass — weight** uses Transformer, which saturates the lows harder. **Thump** adds a resonant
  bump at the **Low cut** frequency.
- **Drums — smash** uses Console at high drive, blended in at 70 %.
- **Parallel crush** drives Pentode hard with **Punish +20** and mixes it in at 30 %: the drums
  keep their attack and gain density.

## Make a lo-fi speaker sound

**High cut** around 4–5 kHz with **Steep** on gives the narrow, boxy sound of a small speaker.
The **Lo-fi speaker** preset sets it up with a high **Low cut** as well.

## Controls

| Control | What it does | Try |
|---|---|---|
| **Style** | Tape, Console, Transformer, Triode or Pentode | Tape first |
| **Drive** | How hard the sound hits the stage. Warmth up to ~50 %, grit above | 30–45 % on vocals |
| **Punish** | **Punish +20** adds 20 dB of drive | For parallel crushing |
| **Low cut** | Cuts lows before the stage (10 Hz = off) | 80 Hz on vocals |
| **Thump** | Resonant bump at the low cut | 30–40 % on bass |
| **Tone** | Tilt before the stage: dark to bright | Slightly bright for presence |
| **High cut** | Cuts highs after the stage (20 kHz = off) | 14–16 kHz to tame fizz |
| **Steep** | **Gentle** (6 dB/oct) or **Steep** (30 dB/oct) high cut | Steep for a speaker sound |
| **Auto gain** | **Auto** keeps the output about as loud as the input; **Manual** does not | Auto |
| **Mix** | Blend of clean and saturated sound | 100 %, or 30–70 % for parallel |
| **Output** | Output level (−24 to +12 dB) | 0 dB |
| **Oversampling** | Under **Advanced**: **2×** or **4× HQ** (less aliasing, more CPU) | 2×; 4× HQ for a bright source at high drive |

## Presets

Default, Vocal — warmth, Vocal — tube, Rap — grit, Drums — smash, Bass — weight, Lo-fi speaker,
Parallel crush, Tape — bus glue.

## Tips

- **Auto gain** is calibrated on music at a typical mix level. On a very hot track a heavily
  driven stage comes out a few dB quieter than it went in, on a quiet track louder. Use
  **Output** to trim.
- The window shows the curve the sound runs through, the harmonics the stage adds and a history
  of the added harmonics, so you can see how much it is working.
- The [Voice](README.md) effect's **Drive** knob uses this same saturator (Triode).

## Technical notes

Code: `crates/engine/src/dsp/saturator.rs` (the `sat` effect and the drive of `voice`).
Measurements: `cargo run -p wave-engine --release --example sat_measure`.
Listening demos: `cargo run -p wave-engine --release --example sat_demos -- <vocal.wav 44.1 kHz> <out dir>`.

**Parameter ids** (`catalog.rs` `SAT`, order = `saturator::idx`). `style` is stored as an index; the
catalog's option names are Polish and the English UI translates them (`ui/src/i18n/names-fx.ts`:
Taśma = Tape, Konsola = Console, Transformator = Transformer, Trioda = Triode, Pentoda =
Pentode).

| id | range | default | meaning |
| --- | --- | --- | --- |
| `style` | 0–4 | 0 (Tape) | voicing |
| `drive` | 0–100 % | 40 | gain into the stage, quadratic: 36·(d/100)² dB (9 dB at 50 %, 36 dB at 100 %); past 50 % the clipper share and the envelope bias also grow |
| `punish` | Off/On | Off | +20 dB into the stage |
| `lowcut` | 10–1000 Hz | 10 (off) | 12 dB/oct high-pass before the curve |
| `thump` | 0–100 % | 0 | resonance of the low cut (Q 0.71 → 4, up to ~+12 dB at the cut) |
| `tone` | −100–100 % | 0 | tilt before the curve (±5 dB shelves at 800 Hz / 1.2 kHz) |
| `highcut` | 500–20 000 Hz | 20 000 (off) | low-pass after the curve |
| `steep` | Off/On | Off | high cut 6 dB/oct (one pole) or 30 dB/oct (5th-order Butterworth) |
| `auto` | Off/On | On | level-matched output |
| `mix` | 0–100 % | 100 | dry/wet, mixed at the oversampled rate (phase-coherent) |
| `out` | −24–12 dB | 0 | output |
| `hq` | 2× / 4× | 2× | oversampling; changes latency, so it cannot be automated |

**Old projects.** `Project::migrate` (and `migrateProject` in the UI) turns the previous `drive`
(0–36 dB) into the knob position with the same gain, sets Tape and auto gain off and lowers
`out` by half the old drive (the old curve divided its output by √drive). A file that already has
`style` is untouched.

### DSP design

Per channel, everything between the up- and downsampler runs at 2× or 4×:

```
in ─ up ─┬─ drive ─ low cut/thump ─ tilt ─ pre-emphasis ─ curve ─ FIR ─ de-emphasis ─ voicing shelf ─ DC block ─ high cut ─ auto gain ─┐
         └─ dry (same ADAA + FIR delay) ─────────────────────────────────────────────────────────────────────────────────── mix ─ down ─ out
```

**Curve.** A blend of three curves with closed-form antiderivatives: `tanh`, an asymmetric tanh
(`tanh(u)` above zero, `tanh(k·u)/k` below, `k` = 1–2), and a cubic soft clipper flat from
|u| = 1.5. Each voicing sets the blend, the headroom (`K`, level at the curve per unit input) and
the rest below; the voicings are morphed over ~20 ms when the style changes.

**Dynamic bias.** The curve's operating point is `bias + benv·env`, where `env` is a peak
follower of the curve input (attack 0.5–5 ms, release 30–150 ms per voicing), and `f(bias)` is
subtracted. Loud notes move the operating point into the asymmetric part of the curve: even
harmonics bloom on the note and collapse after it, as in a tube stage with grid-current bias
shift. The asymmetric DC goes to a 5 Hz DC blocker.

**Memory.** A one-pole low-pass of the curve's error (`f(u) − u`) is fed back to its input
(amount 0.05–0.35, corner 300 Hz–4 kHz). Sustained overdrive pulls the drive down, a transient
passes before the loop catches up: soft "tape" compression, and the console's transient squash
with a fast corner.

**Pre/de-emphasis.** Shelves before the curve with exact inverses after it (RBJ shelves with ±g
cancel), so the linear response is unchanged but chosen bands hit the curve harder: tape +6 dB
at 3 kHz, transformer +10 dB below 150 Hz, pentode +4 dB at 2 kHz and −2 dB at 200 Hz. A final,
non-inverted shelf sets each voicing's top end (−2.5 dB at 7 kHz on the transformer, +0.8 dB at
12 kHz on the console).

**Antialiasing.** First-order ADAA (Parker, Zavalishin & Le Bivic, DAFx 2016; Bilbao, Esqueda,
Parker & Välimäki, IEEE SPL 2017) on the whole curve, in f64, with `F(x₁)` cached and invalidated
when the curve blend changes; near-equal inputs fall back to `f` at the midpoint. ADAA's
`(1 + z⁻¹)/2` response is undone up to 20 kHz by a 3-tap FIR `[−a, 1+2a, −a]` (a = 0.17 at 2×,
0.14 at 4×), and the dry path gets the same `(1 + z⁻¹)/2 · FIR` so Mix stays phase-coherent. The
aliasing ADAA leaves is removed by oversampling: polyphase IIR half-bands (two allpass chains,
elliptic design of Valenzuela & Constantinides as in HIIR): 12 coefficients, transition 0.035
for base ↔ 2× (passband flat to ±0.01 dB at 20 kHz, > 100 dB stopband from 28 kHz at 48 kHz),
plus 4 coefficients for 2× ↔ 4×. IIR half-bands were chosen over linear-phase FIR for minimum
latency (a FIR of similar rejection: ~24 samples at 48 kHz).

**Auto gain.** A static table per voicing, Punish and drive (5 % steps, interpolated in dB),
measured on pink noise at −18 dBFS RMS and stored in the source (`print_auto_table` regenerates
it). Static on purpose: an adaptive gain would ride the music.

**Real-time.** No allocation after `prepare`; the coefficient tables are computed once.
Continuous parameters glide (gains per sample, filters and voicing per 16-sample block with
coefficients recomputed only when they moved). Oversampling factor and steep slope switch under a
1.5 ms duck. Denormals are flushed in every state; non-finite output is replaced by silence.
Latency is reported to PDC. Silence leaves the exact parked state, so a skipped chain wakes
identically (`tests/silence_skip.rs`).

**Voice.** The drive of the `voice` effect is this core after pitch and formant: Triode, auto
gain on, 6 dB/oct high cut at 14 kHz, 2×, with the same drive curve (Voice's `drive` in dB is the
gain into the stage). Mix rises from 0 to 1 over the first 3 dB so 0 dB stays clean. The
oversampling delay (6 samples) is taken out of Voice's look-ahead, so Voice's reported latency is
unchanged.

### Measurements (48 kHz)

THD, 1 kHz sine at −12 dBFS peak, 2×, auto gain on:

| voicing | drive | THD % | H2 dBc | H3 dBc | H5 dBc |
| --- | --- | --- | --- | --- | --- |
| Tape | 25 / 50 / 100 % | 0.89 / 2.66 / 28.7 | −41.3 / −32.2 / −17.7 | −52.9 / −39.6 / −13.5 | −99.4 / −89.3 / −22.5 |
| Console | 25 / 50 / 100 % | 0.25 / 0.92 / 36.8 | −54.8 / −45.8 / −20.1 | −55.6 / −42.3 / −10.6 | −110 / −83.2 / −17.6 |
| Transformer | 25 / 50 / 100 % | 0.66 / 2.03 / 40.8 | −44.4 / −35.7 / −16.0 | −51.7 / −38.4 / −10.9 | −118 / −77.9 / −18.4 |
| Triode | 25 / 50 / 100 % | 2.54 / 7.42 / 38.2 | −32.0 / −22.8 / −20.3 | −48.3 / −35.9 / −11.2 | −97.3 / −61.7 / −16.9 |
| Pentode | 25 / 50 / 100 % | 0.31 / 1.44 / 30.2 | −73.8 / −60.7 / −36.3 | −50.2 / −36.8 / −12.7 | −99.2 / −73.6 / −17.8 |

Aliasing, 5 kHz sine at −6 dBFS peak, drive 100 %: energy off the harmonic bins relative to the
output (dB):

| voicing | 1× naive | 1× ADAA | 2× + ADAA (normal) | 4× + ADAA (HQ) |
| --- | --- | --- | --- | --- |
| Tape | −11.2 | −20.0 | −36.0 | −51.2 |
| Console | −11.4 | −19.0 | −38.9 | −62.1 |
| Transformer | −12.5 | −20.5 | −40.2 | −61.6 |
| Triode | −11.6 | −18.9 | −35.4 | −50.7 |
| Pentode | −10.3 | −18.1 | −33.2 | −49.3 |

Auto gain, output − input on pink noise, worst case over drive 0–100 % (Punish off):

| voicing | −18 dBFS (calibration) | −12 dBFS | −24 dBFS |
| --- | --- | --- | --- |
| Tape | 0.33 | −5.7 | +5.1 |
| Console | 0.02 | −5.7 | +5.2 |
| Transformer | 0.40 | −5.1 | +5.2 |
| Triode | 0.71 | −5.7 | +5.2 |
| Pentode | 0.04 | −5.9 | +5.8 |

Up to 50 % drive the off-level error stays under ~2.4 dB; the large values are at 80–100 %.

Latency: 2× 5.85 samples (reported 6, 0.125 ms), 4× 6.68 samples (reported 7, 0.146 ms).

CPU per stereo instance, 48 kHz, blocks of 256, release build: about 300 ns/sample at 2×
(1.4–1.5 % of one core) and 520–560 ns/sample at 4× (2.5–2.7 %) on a quiet machine (measured
before the final FIR/bias-cache revision); 0.5–2.5 µs/sample when re-run while the machine was
loaded by parallel builds, so re-measure on an idle machine.

## See also

- [Compressor](compressor.md): the FET mode adds its own colour as you push it.
- [Filter](filter.md): drive into a resonant filter.
- [All effects](README.md)
