# Limiter

The Limiter makes a mix louder and guarantees that it never goes over a set peak level, so the
master does not clip on export or on a streaming service.

![The Limiter window](../../site/img/limiter.png)

## Quick start

1. Add **Limiter** as the last effect on the **Master** insert (mixer, `F9`, then **+ effect**).
2. Leave **Ceiling** at −1.00 dBTP, the safe value for streaming.
3. Turn **Gain** up slowly. The window shows the gain reduction (**GR**) the limiter applies.
4. Stop when the mix gets louder without losing punch. A few dB of reduction on the loudest hits
   is usually enough.

## Set the ceiling for streaming

**Ceiling** is the highest level the output can reach, measured as *true peak* (dBTP): the
level of the waveform between the samples, which is what a converter or an MP3/AAC encoder
actually produces. A plain sample-peak limiter can let these in-between peaks go over 0 dB; this
one detects them, so a −1 dBTP ceiling holds after export too.

The **Streaming −1 dBTP** preset adds 4 dB of gain with a −1 dBTP ceiling.

## Get loud without pumping

- **Auto release** (on by default, under **Advanced**) lets go quickly after a single hit and
  more slowly while the limiter works continuously, which keeps bass-heavy and dense mixes from
  pumping or distorting.
- **Lookahead** (under **Advanced**) gives the limiter more time to turn the gain down before a
  peak. Longer is smoother and cleaner; shorter keeps more snap on drums.
- **Stereo link** (under **Advanced**) at 100 % reduces both sides together, so the stereo image
  does not shift. Lower it to let each side be limited on its own.

If the mix starts to sound flat or distorted, you have reached the limit of what limiting can
do: lower **Gain** and get the loudness from the mix itself (EQ, compression, saturation).

## Controls

| Control | What it does | Try |
|---|---|---|
| **Gain** | Drives the mix into the limiter (0–24 dB) | 3–7 dB on a master |
| **Ceiling** | Highest output level, true peak (−12 to 0 dBTP) | −1.00 dBTP |
| **Release** | How fast the gain comes back after a peak (10–1000 ms) | 80 ms |
| **Auto release** | Program-dependent release (up to 4× slower under constant limiting) | On |
| **Lookahead** | How early the limiter sees a peak (1–5 ms); adds that much latency | 3 ms |
| **Stereo link** | 100 % = both sides together, 0 % = each side on its own | 100 % |
| **Output** | Lowers the final level below the ceiling (−12 to 0 dB) | 0 dB |

## Presets

Safe, Streaming −1 dBTP, Loud master, Very loud, Transparent.

## Tips

- The Limiter's latency is compensated automatically, so the mix stays in time. Because changing
  **Lookahead** changes that latency, **Lookahead** cannot be automated.
- To check how loud the result is in LUFS, put the **Analyzer** after the Limiter.

## Technical notes

Code: `crates/engine/src/dsp/limiter_v2.rs`. Window: `ui/src/views/fx/dyn.ts` (transfer curve,
input level and gain-reduction history, shared with the Compressor). Measurements:
`cargo run -p wave-engine --example reverb_v2_eval --release -- D:\wave-data\reverb_v2_samples`
(the same evaluation covers the limiter).

**Parameter ids** (order = `dsp::limiter_v2::idx`): `gain` (0–24 dB), `ceil` (−12–0 dBTP), `rel`
(10–1000 ms), `auto` (Off / On), `look` (1–5 ms; sets latency, not automatable), `link`
(0–100 %), `out` (−12–0 dB, applied after the ceiling).

**Design.**

- Detection: polyphase oversampled true peak per ITU-R BS.1770-4 Annex 2 (4× always, 8× near
  the ceiling, 384-tap Kaiser sinc) per channel, blended between per-channel and linked by
  **Stereo link**.
- Gain computer: required gain `min(1, ceiling / peak)` → sliding minimum over the look-ahead
  window (+2 samples so the neighbours of a peak are covered) → release → moving average over
  the look-ahead window. The average turns the gain change into a linear ramp that lands exactly
  when the peak arrives: no hard knee in the gain (no clicks) and no clipper in the normal path
  (Giannoulis, Massberg & Reiss, "Digital dynamic range compressor design", JAES 2012).
- Release: exponential; with **Auto release** it is program dependent, fast after isolated
  transients, up to 4× slower while the limiter works continuously.
- A last sample clamp at the ceiling only guards against rounding; the tests measure that it does
  not engage on real material.
- Latency = look-ahead + the detector's delay (`latency_samples`), reported for compensation.

**Why v2.** An audit of the old limiter (1 ms look-ahead, instant gain step, 60 ms release,
sample-peak only, hard clip, latency not reported to PDC) found inter-sample overs up to
+4.9 dBTP over a −1 dB ceiling, 1.9 % THD on 50 Hz, and an uncompensated 1 ms delay.

**Measurements** (44.1 kHz):

| limiter (ceiling −1 dBTP) | old max TP | old overs | new max TP | new overs |
|---|---|---|---|---|
| fs/4 sine 45° | +2.10 | 168000 | −1.05 | 0 |
| synthetic drums +9 dB | +4.86 | 560 | −1.04 | 0 |
| 5 MUSDB mixes +8 dB | −0.20 (worst) | 488 | −1.05 (worst) | 0 |

THD at 6 dB GR: 50 Hz 1.86 % → 0.13 %, 100 Hz 0.63 % → 0.00 %. Loudness at equal gain: within
0.1 dB RMS on the mixes. CPU 3 → 18 µs per 128 frames (true-peak detection).

## See also

- [Compressor](compressor.md): control dynamics before the limiter.
- [Saturator](saturator.md): density without more limiting.
- [All effects](README.md)
