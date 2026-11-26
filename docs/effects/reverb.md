# Reverb

Reverb puts a sound in a space, from a tight room to an 18-second wash, and is tuned first for
vocals: dense with no metallic ring, and with a low end that does not turn to mud.

## Quick start

1. Add **Reverb** to an insert (mixer, `F9`, then **+ effect**). A new Reverb loads the
   **Vocal Plate** preset.
2. Pick a **Space**: **Plate** for vocals and snares, **Room** for a natural space, **Hall** for
   size, **Ambience** for a short sense of air.
3. Set **Decay**, the time the tail takes to die away. 0.6–1.5 s suits most vocals.
4. Set **Mix**. Start low (10–15 %) and raise it until you hear the space, then back off a little.
5. Add **Pre-delay** (20–40 ms) to keep the words clear of the tail.

## Choose a space

| Space | Sound | Good for |
|---|---|---|
| **Plate** | dense from the first millisecond, no separate early echoes | vocals, snares |
| **Room** | strong short reflections, a small space | drums, a natural vocal |
| **Hall** | wide reflections, a large space | ballads, keys, pads |
| **Ambience** | mostly early reflections, a short soft tail | a sense of space without a tail |

## Keep the vocal clear

- **Pre-delay** holds the reverb back after each word, so the consonants land dry and the tail
  follows.
- **Ducking** (under **Advanced**) turns the reverb down while the singer sings and lets it bloom
  in the gaps. 3–6 dB is enough on most vocals. It is the same control as the
  [Delay](delay.md)'s.
- **Low cut** (under **Advanced**) removes lows before the reverb, the most common cause of a
  muddy mix. The default is 300 Hz.

## Make the tail darker or brighter

**Tone** sets how quickly the high frequencies die compared with the middle: left is dark (the
highs vanish early, like a big soft room), right is bright. The highs always die sooner than the
mids, as in a real space.

**Bass decay** (under **Advanced**) makes the low end ring shorter (below 1×) or longer (above
1×) than the mids. Keep it under 1× on vocals.

## Use it as a send

Put one Reverb on its own insert, send several tracks to it, and set **Mix** to 100 %. The
presets **Send — hall 100 %** and **Send — plate 100 %** are set up for this.

## Shape the reverb on the graph

The window draws how long the tail rings at each frequency (frequency across, decay time up),
with the live spectrum of the tail behind it, and below that the tail over time: pre-delay, early
reflections and the decay down to −60 dB, with the real output traced over it after a hit.

- Drag the three dots up and down: bass decay, **Decay**, and treble decay (**Tone**).
- Drag the **LO** and **HI** lines sideways to set **Low cut** and **High cut**.
- Drag **PRE** to set **Pre-delay**.
- Double-click a handle to reset it. Each drag is one undo step.

## Controls

| Control | What it does | Try |
|---|---|---|
| **Space** | Plate, Room, Hall or Ambience | Plate on vocals |
| **Decay** | Time for the tail to fall 60 dB in the mids (0.2–20 s) | 1 s |
| **Pre-delay** | Gap before the reverb starts (0–150 ms) | 25 ms |
| **Tone** | Dark (highs die early) to bright | Slightly dark on vocals |
| **Mix** | How much reverb you hear. Up to 50 % the dry sound stays at full level | 12–18 % on an insert |
| **Low cut** | Cuts lows going into the reverb (20–1000 Hz) | 250–400 Hz |
| **High cut** | Cuts highs going into the reverb (2–16 kHz) | 7 kHz |
| **Bass decay** | Low-end decay relative to **Decay** (0.5–1.5×) | 0.8× |
| **Early** | Level of the early reflections | 50 % |
| **Width** | Stereo width of the reverb | 100 % |
| **Modulation** | Slow movement inside the reverb that prevents metallic ringing | 50 %; more on pads |
| **Ducking** | Turns the reverb down while the input plays (0–12 dB) | 3–6 dB on vocals |

## Presets

Vocal Plate, Vocal Room, Vocal Hall, Ambience, Ballad — long plate, Drums — room,
Send — hall 100 %, Vocal — rap, short, Vocal — R&B hall, Vocal — pop, bright plate, Snare plate,
Keys — warm hall, Lead — EDM hall, Pad — ambient haze, Lo-fi — dark room, Cosmos — 18 s,
Send — plate 100 %.

## Common problems

- **The mix gets muddy.** Raise **Low cut**, lower **Bass decay**, or shorten **Decay** before
  you lower **Mix**.
- **The vocal sounds far away.** Lower **Mix**, add **Pre-delay**, or add **Ducking**.

## Technical notes

Code: `crates/engine/src/dsp/reverb_v2.rs`; window: `ui/src/views/fx/reverb.ts`,
`ui/src/lib/reverbSpace.ts`. Measurements:
`cargo run -p wave-engine --example reverb_v2_eval --release -- D:\wave-data\reverb_v2_samples`.

```text
dry ─┬─ wet input HPF/LPF ─ pre-delay ─┬─ velvet-noise early reflections ────────────┐
     │                                  └─ 4 allpass diffusers ─ 16-line FDN ─ late ─┤
     │                                                                               ├─ width ─ duck ─ mix
     └── envelope (ducking side-chain) ──────────────────────────────────────────────┘
```

**Parameter ids** (order = `dsp::reverb_v2::idx`): `size` (Decay, 0.2–20 s, log), `pre`
(0–150 ms), `lo` (20–1000 Hz), `hi` (2–16 kHz), `width` (0–100 %), `mix` (0–100 %), `tone`
(−100–100 %), `space` (Plate / Room / Hall / Ambience), `duck` (0–12 dB), `early` (0–100 %),
`bass` (0.5–1.5×), `mod` (0–100 %).

### Research summary

**Late reverb.** Every current high-end algorithmic reverb is built on a recirculating network
of delay lines; the feedback delay network (FDN) is the general, analysable form.

- Jot & Chaigne (AES 90th Conv., 1991): FDN with a unitary feedback matrix and per-line
  absorption filters `g = 10^(-3·L/T60(f))`; decay is then set per frequency, independent of the
  "colour" of the network.
- Schlecht & Habets, "Accurate reverberation time control in feedback delay networks" (DAFx
  2017) and "Feedback delay networks: echo density and mixing time" (IEEE/ACM TASLP 2020): dense
  unitary matrices (Hadamard, Householder) and more lines give faster echo build-up; mutually
  prime lengths avoid coinciding modes.
- Välimäki, Parker, Savioja, Smith & Abel, "Fifty years of artificial reverberation" (IEEE TASLP
  2012): survey; time-varying (modulated) delays are the standard cure for metallic ringing in
  short-decay or small networks.
- Dattorro, "Effect design part 1" (JAES 1997): the Griesinger/Lexicon plate: input allpass
  diffusers + modulated tank. We take its input diffusion and modulation, not the tank.
- Early reflections: velvet noise, sparse ±1 pulses one per segment (Järveläinen & Karjalainen,
  AES 2007; Välimäki, Holm-Rasmussen, Alary & Lehtonen, "Late reverberation synthesis using
  filtered velvet noise", Applied Sciences 2017). Cheap, smooth, and decorrelated by using
  different pulse patterns per channel.
- Product practice (manuals of FabFilter Pro-R 2, Valhalla VintageVerb/Room, LiquidSonics
  Seventh Heaven): few macro controls (Space/Decay, Brightness/Character, Distance or Pre-delay,
  Mix), per-band decay, wet EQ, and ducking. Users value plates for vocals because the onset is
  dense with no discrete slap and the tail is bright-but-short; a common complaint about cheap
  reverbs is metallic flutter on short decays and a "washy" low end. (Inference from manuals and
  reviews; no listening tests run.)

**Choice for vocals.** 16-line FDN, orthonormal Hadamard mixing (fast WHT, 64 adds/sample),
incommensurate geometric line lengths, slow per-line delay modulation with cubic reads, two
first-order shelves per line for three-band RT60 (bass / mid / treble), 4 input allpass
diffusers per channel, velvet-noise early reflections (none for Plate), wet HPF/LPF, stereo
outputs from two orthogonal Hadamard rows, M/S width, and a dry-keyed ducker (`dsp::ducker`,
shared with the Delay). The macro set stays usable at every position: decay ≤ 20 s (the log
taper keeps 0.2–2 s in the knob's first half), treble decay always shorter than mid, bass decay
0.5–1.5× mid, network size follows the decay, the wet is partly normalised for decay, and the Mix
law keeps the dry at unity up to 50 %.

**Long decays and old projects.** Decay is stored in seconds, so a saved reverb keeps its decay.
Its automation is stored normalised over the knob's range; projects from before the 20 s range
(format < 3) have it rescaled once on load (`reverb_v2::migrate_auto_size`). A 20 s decay stays
stable by construction: every line's absorption gain is `10^(−3·L/T60) < 1` for any finite T60
(also the 30 s bass band at 1.5×), and the network size stops growing at its scale limit (the
delay lines are sized for it); `the_longest_decay_is_stable_and_really_decays` measures the 20 s
mid RT60 and checks the tail is gone by `tail_samples`.

### Display

The window is drawn from the DSP's own numbers. Top: RT60 per frequency (bass below ~250 Hz,
Decay in the mids, Tone setting how soon the highs die; `derive` in `reverbSpace.ts` mirrors
`reverb_v2.rs`), the Low cut / High cut regions shaded, and the live spectrum of the wet signal
behind it. Bottom: pre-delay, the early reflections in the space's real span and slope, the late
tail per band to −60 dB, and the output after the last hit traced over it.
`ui/src/api/reverbSpaces.json` carries the spaces and sample values from the engine
(`tests/reverb_spaces.rs`, regenerate with `WAVE_UPDATE_FIXTURES=1`); `reverbSpace.test.ts`
checks the UI reproduces them.

### Measurements (44.1 kHz; 96 kHz within ±0.05 s / ±0.03)

| reverb | NED ≥ 0.9 at | Wiener flatness | ring peak p99.9 / max | L/R corr | CPU µs / 128 fr |
|---|---|---|---|---|---|
| old (1.0 s) | 139 ms | 0.469 | 12.8 / 15.1 dB | 0.122 | 3.3 |
| new Vocal Plate | 52 ms | 0.488 | 11.7 / 15.2 dB | 0.002 | 33 |
| new Vocal Room | 54 ms | 0.542 | 10.2 / 11.0 dB | 0.046 | 45 |
| new Vocal Hall | 101 ms | 0.490 | 10.1 / 10.9 dB | −0.015 | 44 |
| white noise ref. | – | 0.563 | 10.1 / 11.7 dB | – | – |

RT60 per octave band (measured / designed): Plate 1.0 s within 10 %, Hall 2.0 s within 3 %, Room
0.6 s within 9 %; Ambience 0.4 s within 19 % at 125–250 Hz (band filter ring-down dominates such
short decays). The old reverb's default wet was ~12 dB hotter relative to the dry than the new
Vocal Plate, with a 2.2 s default decay.

## See also

- [Delay](delay.md): echoes with the same Ducking control.
- [Gate](gate.md): the **Gated reverb — 80s snare** preset cuts a reverb tail short.
- [All effects](README.md)
