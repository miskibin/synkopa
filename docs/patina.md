# Patina

A lo-fi character effect: the colour of old media in one window. Five modules sit in a row,
each with one big knob and a picture of what it is doing right now; **Magnitude** scales them
all at once. Code: `crates/engine/src/dsp/patina.rs`; window: `ui/src/views/fx/patina.ts`.

```text
          ┌─────────── noise (vinyl, shellac, tape, hum) ───────────┐
          │ Pre: through the modules               Post: added clean │
 in ─ + ──┴─ Wobble ─ Drive (band) ─ Digital ─ Wear ─ + ─────────────┴─ low/high cut ─ tone ─ width ─ output
```

## Modules

| Module | Big knob | Details |
|---|---|---|
| **Noise** | level | Type, Route (Post / Pre), Follow (left: duck under the music, right: ride with it), Noise tone (dark … bright) |
| **Wobble** | pitch swing | Wow rate, Flutter (share of the fast 9 Hz flutter), Wobble stereo (linked … independent sides) |
| **Drive** | drive | Style (the Saturator's Tape, Console, Transformer, Triode, Pentode), Band low / Band high |
| **Digital** | mix of the crushed sound | Rate (sample rate), Bits, Smooth (a 4-pole low-pass after the crush) |
| **Wear** | head loss + glue | Tape (cassette, 7.5 ips, 15 ips), Dropouts, Dropouts stereo |

**Magnitude** (0–100 %) scales every module on its own curve: drive and wear come in with the
knob, wobble and digital a little later, noise later still (Magnitude^1.5) and dropouts last
(Magnitude²), so low settings are colour without hiss or tape damage. It also eases the output
section in: at 0 % the low/high cut, tone and width do nothing. **Wander** moves every module at
random (noise level ±35 %, drive ±25 %, sample rate ±¼ octave, wear cutoff ±30 %, and how much
of the wobble is random rather than a steady sine).

The window's header **MIX** is a plain dry/wet. With Wobble up, mixing dry and wet makes a
chorus (two copies a few milliseconds apart); turn Magnitude down instead for less effect.

## How it works

- **Noise** is generated, never a loop, so it never repeats and costs no disk or memory. Hiss is
  pink and white noise shaped per medium (a cassette's 4–7 kHz sizzle and roll-off past 12 kHz,
  a reel's finer and quieter grain) with slow drift and, for tape, a faster breathing of the
  transport. Records add groove swish that swells once per revolution and crackle in three tiers:
  tiny ticks, short clicks in the 0.7–4.5 kHz band and rare low pops, each with its own pitch,
  length, level (heavy-tailed: many faint, few loud) and pan; a few scratches are "pressed" into
  the record and come round every revolution (1.8 s at 33 rpm, 0.77 s at 78). Shellac is a
  narrow (200 Hz – 4.2 kHz, 24 dB/oct), mid-heavy, nearly mono roar. Hum is 50 Hz with its
  harmonics, the rectifier's ripple and a little buzz, over a preamp hiss. White noise is scaled
  with the sample rate, so levels match at 44.1 and 96 kHz.
- **Wobble** reads a delay line with 4-point Hermite interpolation. Depth is a pitch swing (up to
  ±2.5 %, about ±43 cents), so the delay swing is depth / (2π·rate) and the depth sounds the
  same at any rate. The movement is a sine blended with a band-limited random curve; every part
  is bounded and zero-mean, so the pitch centre never drifts.
- **Drive** is the Saturator's core (oversampled 2×, antiderivative antialiasing, auto gain).
  With a band set, Linkwitz-Riley crossovers split the input; only the band is driven, and the
  rest runs through an identical clean core so both paths have the same latency and filtering
  and the bands sum flat. A full-range band skips the split (no phase turn in the bass).
- **Digital** holds the input at a fractional rate: the held value is latched at the exact
  instant the slower clock ticks, so the rate sweeps without beating. Bits are continuous.
  Smooth is a 4-pole low-pass that falls with the rate (no filter at 0 %: raw aliasing).
- **Wear** models playback losses: a low-pass that falls with wear and tape speed, a head bump
  (60 / 80 / 110 Hz), soft compression of peaks above −12 dBFS, slow and fast level wear, and
  dropouts: a Poisson stream of dips (8–30 ms down, a short hold, 30–160 ms back) in level and
  in the high end, as when the tape lifts off the head. Dropouts work at any Wear.

## Latency and CPU

Latency is the Drive's half-band filters plus 2 samples of the wobble line (about 3 samples at
48 kHz), reported for plugin delay compensation. With Noise up the effect never sleeps in
silence: the noise is a sound of its own.

Measured in a release build at 48 kHz, 128-sample blocks (`patina_demos` prints it): about 0.44 µs
per stereo sample (2 % of one core) for a bus preset, 0.86 µs (4 %) with every module on and a
drive band set. Most of it is the saturator core (oversampled, antialiased); a band split runs a
second, clean core so the bands stay in phase, and a full-range band skips it.

## Testing and listening

`cargo test -p wave-engine --lib patina` checks levels at zero settings, Magnitude 0 against the
dry input, noise levels per type at 44.1 and 96 kHz, Follow and Duck, the click rate, the
wobble depth and its centre on a 3150 Hz tone, quantisation and aliasing in Digital, the head
loss in Wear, dropouts, a flat band split, a band-limited drive and extreme settings at block
sizes 1 to 512.

`cargo run -p wave-engine --release --example patina_demos -- <audio> <out dir> [start s] [seconds]`
renders every preset (dry, then wet) and every noise type alone to MP3, and prints how much each
preset changes the music's level.
