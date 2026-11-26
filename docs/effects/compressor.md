# Compressor

The Compressor evens out the level of a sound by turning down the loud parts, so a vocal sits
steadily in the mix, drums hit with more punch, or a bass ducks under the kick; it has two types:
**Clean** for transparent control and **FET** for the fast, coloured sound of a classic 1176.

![The Compressor window](../../site/img/compressor.png)

## Quick start

1. Add **Compressor** to an insert (mixer, `F9`, then **+ effect**).
2. Play the track. Lower **Thresh** until the **GR** (gain reduction) readout shows a few dB on
   the loud parts.
3. Set **Ratio**: 2:1–4:1 for gentle control, higher for a firm hold.
4. Raise **Makeup** by about the amount of gain reduction, so bypassing compares tone, not
   volume.

## Clean or FET?

The switch next to the effect's name in the window picks the type. Switching loads that type's
default settings.

- **Clean** is a transparent compressor with a threshold, a ratio and a soft knee. Use it for
  leveling, glue on a bus, and sidechain ducking.
- **FET** works like an 1176: the threshold is fixed and you drive the sound into it with
  **Input**. It is very fast and adds a little colour (mostly even harmonics) as you push it. Use
  it to bring a vocal forward or to smash a drum room.

In the effect picker, the **Compressor** tile lists the presets of both types; the FET ones start
with "FET ·".

## Level a vocal (Clean)

Load **Vocal — leveling**: 3:1, a 5 ms attack and a 120 ms release. Lower **Thresh** until the
loudest words show 4–6 dB of reduction. A shorter **Attack** catches more of each consonant; a
longer one lets the start of each word through for more bite.

## Glue a drum bus (Clean)

Load **Drum bus — glue** (2:1, 30 ms attack, 200 ms release). The slow attack keeps the hits'
punch, and 1–3 dB of reduction makes the kit sound like one instrument.

## Duck the bass under the kick (sidechain)

Right-click the kick's mixer strip and choose **Sidechain to** → the bass insert. Synkopa keys
the bass's Compressor to the kick, or adds one set up for ducking if the bass has none, and
opens it. The bass now dips every time the kick plays.

You can also open a Compressor and pick the source in its **Sidechain** box (**Own input** or
another insert). The readout then shows the sidechain's level as **SC**.

## Bring a vocal forward (FET)

1. Switch the type to **FET** (or load **Vocal — classic**).
2. Turn **Input** up until the needle shows 3–7 dB of gain reduction on the loud words.
3. Pull **Output** down to match the level when bypassed.
4. For parallel compression, set **Mix** to 30–50 %.

**Ratio** **All** is the "all buttons in" mode: more aggressive, with a characteristic pumping
and distortion. Use it on drums and rooms rather than on a lead vocal.

Click the meter to switch it between gain reduction (**GR**) and output level (**OUT**).

## Controls: Clean

| Control | What it does | Try |
|---|---|---|
| **Thresh** | Level above which compression starts (−60 to 0 dB) | −18 dB, then adjust |
| **Ratio** | How much it turns down above the threshold (1:1–20:1) | 3:1 on vocals |
| **Attack** | How fast it reacts (0.1–100 ms) | 5–30 ms |
| **Release** | How fast it lets go (10–1000 ms) | 100–200 ms |
| **Makeup** | Gain after compression (0–24 dB) | About the GR amount |
| **Sidechain** | What it listens to: **Own input** or another insert | The kick, for ducking |
| **Detect** | Under **Advanced**: **Peak** follows transients, **RMS** follows the body of the sound | RMS for smooth vocal leveling |

## Controls: FET

| Control | What it does | Try |
|---|---|---|
| **Input** | Drives the sound into the fixed threshold (0–36 dB) | Until you see 3–7 dB of GR |
| **Ratio** | 4, 8, 12, 20 or **All** | 4 on vocals |
| **Attack** | 20–800 µs (very fast) | 300–400 µs |
| **Release** | 50–1100 ms; slows down on its own after long compression | 250 ms |
| **Output** | Output level (−24 to +12 dB) | Match the bypassed level |
| **Mix** | Dry/wet for parallel compression | 100 %, or 30–50 % |

## Presets

Clean: Default, Vocal — leveling, Drum bus — glue, Kick — punch, Bass — control, Gentle, Smash.
FET: Vocal — classic, Vocal — upfront, Bass — control, Parallel compression, All buttons — smash.

## Common problems

- **The sound gets quieter.** Compression lowers the loud parts; add **Makeup** (Clean) or raise
  **Output** (FET).
- **The track pumps.** Lengthen **Release**, or lower **Ratio**. With **FET**, avoid **All** on
  a lead vocal.
- **Switching Clean/FET lost my settings.** Switching loads the new type's defaults; undo
  (`Ctrl+Z`) brings the previous compressor back.

## Technical notes

Code: Clean `Comp` in `crates/engine/src/dsp.rs`; FET `Fet` in `crates/engine/src/vocal.rs`.
Windows: `ui/src/views/fx/dyn.ts` (Clean, transfer curve and level / GR history),
`ui/src/views/fx/fet.ts` (FET needle meter, VU ballistics ≈300 ms, 0 VU = −18 dBFS on the output
reading). Switching type replaces the effect at the same place in the chain
(`swapFxType`, one undo step); the sidechain is not carried over to FET, which takes none.

**Clean** (`comp`). Ids: `thr` (−60–0 dB), `ratio` (1–20), `att` (0.1–100 ms), `rel`
(10–1000 ms), `gain` (Makeup, 0–24 dB), `det` (Peak / RMS). Fixed 6 dB soft knee. The detector
takes the louder of left and right (stereo linked), or the sidechain (`Fx.key`, another insert's
post-fader output, mono). RMS mode uses a short (~5 ms time constant) mean square. Attack and
release are one-pole time constants on the gain reduction in dB. No latency. Meters: `[0]`
detector peak dBFS of the last block, `[1]` 1 while keyed. "Sidechain to" adds a Compressor with
`DUCK_PARAMS` (`app/src/actions.rs`): −24 dB, 6:1, 1 ms attack, 150 ms release.

**FET** (`fet`). Ids: `input` (0–36 dB), `output` (−24–12 dB), `ratio` (index: 4 / 8 / 12 / 20 /
All), `attack` (20–800 µs), `release` (50–1100 ms), `mix` (0–100 %). Fixed threshold −10 dBFS
after the input gain. Soft knee 8 dB at 4:1, 4 dB at higher ratios, 2 dB in All. All: ratio 20,
attack × 0.6, release × 1.4, and an over-compression "plateau" just above the threshold.
Program-dependent release: the release time grows towards 3× while reduction stays above 3 dB.
Output stage: a polynomial adding mostly even harmonics, its amount growing with Input (more in
All), then a DC blocker. Meters: `[0]` GR dB, `[1]` input peak dB, `[2]` output peak dB.

**Command line.** `synkopa sidechain --from 1 --to 3` keys (or adds) the Compressor on insert 3
to insert 1; `--off` sends it back to its own input; `synkopa sidechain --fx ID --from 1` keys a
given Compressor (or Gate, Filter, EQ). See [the CLI guide](../cli.md).

## See also

- [Limiter](limiter.md): the last stage on the master.
- [Gate](gate.md): the opposite job, turning down the quiet parts.
- [De-esser](de-esser.md): compression only on the "s" sounds.
- [All effects](README.md)
