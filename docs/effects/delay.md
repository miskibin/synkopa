# Delay

Delay repeats the sound as echoes that follow the song tempo, from a short slapback to dub
feedback that keeps growing, with a filter, lo-fi and drive inside the loop so every repeat
changes colour.

The filter, lo-fi and drive sit in the *feedback loop* (the path that feeds each echo back in to
make the next one), so each repeat is shaped again, as on tape and bucket-brigade units. If you
know FL Studio, it works like Fruity Delay 3.

## Quick start

1. Open the mixer (`F9`), click **+ effect** on an insert and pick **Delay**.
2. Leave **Sync** on **Tempo** and turn **Time** to a note value: 1/8, dotted 1/8 or 1/4 are the
   usual choices. The knob snaps to note values.
3. Turn **Feedback** up for more repeats (35 % gives a few, 70 % a long trail).
4. Set **Wet** to taste. Around 20–30 % sits behind a vocal.
5. For a bouncing echo, pick **Ping-pong** under **Model**, or load one of the Ping-pong presets.

## Make the echo follow the tempo

With **Sync** on **Tempo** (the default) the time is set in beats and follows the song tempo
live, tempo automation included. Drag the knob and it snaps to note values (1/128T up to 1/1,
dotted and triplet included). Turn the mouse wheel to step one note value. The status line shows
the note name and the time in ms at the current tempo.

Switch **Sync** to **ms** to set a free time from 1 to 2000 ms, for slapback or doubling that
should not follow the tempo.

## Bounce the echo left and right

**Model** decides how the two sides of the echo are wired:

- **Mono** sums left and right into both echo lines.
- **Stereo** keeps two independent loops.
- **Ping-pong** sends each repeat to the other side: a source on the left echoes first on the
  right, then on the left, and so on.
- **Off** has no echo at all. The loop's filter, lo-fi and drive process the sound directly, so
  the Delay becomes a filter and drive unit.

A centred (mono) vocal fed into a ping-pong delay would bounce symmetrically and sound centred.
**Pan** (under **Advanced**, Ping-pong only) pushes the input to one side first so the bounce is
audible. The Ping-pong presets set it to 70 % left.

In the Mono and Stereo models, **Offset** shifts one side's time: the two sides drift apart with
every repeat (Mono), or one side starts later (Stereo).

## Keep the echo out of the way of the vocal

**Ducking** turns the echoes down while the dry sound plays and lets them bloom back in the gaps
between phrases. Set it to 6–12 dB on a vocal echo and the words stay clear. The repeats keep
running underneath while ducked, so they come back naturally when the singer stops. It is the
same ducking knob as on the [Reverb](reverb.md).

**Tone** (under **Advanced**, in **Feedback loop**) is a final filter on the echoes: left of
centre darkens them, right of centre thins them. The loop's **Cutoff** darkens each repeat a
little more than the one before.

## Use it as a send

To share one delay between several tracks, put it on its own insert, route sends to it, and set
**Dry** to 0 % and **Wet** to 100 %. The preset **Send — 100 % wet** does exactly this.

## Dub, tape and lo-fi echoes

- **Feedback** above 100 % makes the loop grow until the loop drive holds it: controlled
  self-oscillation, the classic dub wind-up. Try the **Dub — self-oscillation** preset.
- **Keep pitch** decides what happens when you change the time. **Repitch** glides to the new
  time and bends the echoes' pitch like tape (**Smoothing** sets how long the glide takes).
  **Keep pitch** crossfades to the new time without bending.
- **Mod time** wobbles the time with an LFO: wow on long echoes, chorus on short ones.
- **Sample rate** and **Bits** degrade every repeat further (lo-fi, bitcrush).
- **Diffusion** smears each repeat into a reverb-like cloud.

## Controls

The first view shows the main controls. Click **Advanced** for the rest.

| Control | What it does | Try |
|---|---|---|
| **Sync** | **Tempo**: time in beats, follows the song. **ms**: free time | Tempo for rhythmic echoes |
| **Time** | Echo time (note value or 1–2000 ms) | 1/8 dotted on vocals |
| **Feedback** | How much of each echo feeds the next (0–125 %) | 30–50 %; over 100 % self-oscillates |
| **Model** | Mono, Stereo, Ping-pong or Off (no echo) | Ping-pong for width |
| **Cutoff** | Loop filter frequency; each repeat gets darker (LP) | 3–6 kHz for a tape feel |
| **Wet** | Level of the echoes | 20–30 % on an insert, 100 % on a send |
| **Dry** | Level of the original sound | 0 % on a send |
| **Ducking** | Turns the echoes down while the input plays (0–12 dB) | 6 dB on vocals |
| **Keep pitch** | Repitch (tape glide) or Keep pitch (crossfade) on a time change | Repitch for dub throws |
| **Smoothing** | Glide length when the time changes (Repitch only) | 0 % for a glitchy jump |
| **Offset** | Mono/Stereo: shifts one side's time | ±20 % for a wider echo |
| **Pan** | Ping-pong: which side the input starts on | −70 % for a centred source |
| **Stereo** | Width of the echoes (0 = mono) | 100 % |
| **Input** | How much sound enters the loop; tails keep ringing | Automate it to echo single words |
| **Filter** | Loop filter type: Off, LP, HP, BP | HP to keep echoes out of the bass |
| **Resonance** | Loop filter peak; high values can ring on their own | Under 75 % |
| **Tone** | Filter on the echo output: left = darker, right = thinner | Slightly left |
| **Distortion** | Loop drive: **Limit** (clips) or **Sat** (soft curve) | Sat |
| **Threshold** | Level where the loop drive starts to bite | −10 dB for warm repeats |
| **Knee** | Sat only: hard (solid state) to soft (valve) | 70 % |
| **Symmetry** | Sat only: adds even harmonics | 30 % |
| **Sample rate**, **Bits** | Lo-fi inside the loop (top of each range = off) | 8 kHz, 8 bit |
| **Diffusion**, **Diff. spread** | Smear each repeat into a cloud | 50–70 % for ambient |
| **LFO rate**, **Mod time**, **Mod cutoff** | LFO wobble on the time and on the cutoff | 0.5 Hz, 15 % |

## Presets

Synced echoes (Eighth 1/8, Dotted 1/8, Quarter 1/4, Triplet 1/8T), three ping-pong presets
(1/8, dotted 1/8, 1/4), Slapback, Send — 100 % wet, Tape — dub, Dub — self-oscillation,
Lo-fi / bitcrush, Smeared echo (ambient), Chorus / doubling, and Multi-FX: filter + drive (no
echo).

## Tips

- An echo that clutters the mix usually needs less low end: set **Filter** to HP, or darken the
  repeats with **Cutoff**, before you lower **Wet**.
- Automate **Input** instead of **Wet** to throw single words into the echo: the tail still rings
  out naturally after the word.
- The window draws where the echoes land on a beat grid (when synced), the read heads moving with
  the LFO, and how hard the loop drive works.

## Common problems

- **The echo keeps growing and does not stop.** **Feedback** is above 100 %, or **Resonance** is
  very high (above about 75 % the loop can ring by itself). Turn either down.
- **Changing the time makes a pitch bend.** That is **Repitch**. Choose **Keep pitch** under
  **Advanced**, or set **Smoothing** to 0 % for an instant jump.

## Technical notes

Code: `crates/engine/src/dsp/delay.rs`; window: `ui/src/views/fx/delay.ts`.

Signal flow:

```text
 in ─ Input ─┬─ model routing (+ offset pre-delay) ─ + ─ ring ─ read ─ loop chain ─┬─ Stereo ─ Tone ─ Wet ─┐
             │                                      └──── × Feedback ─────────────┘                       │
             ├─ envelope (ducking side-chain) ─────────────────────────────────────────────── × Duck ─┤
             └──────────────────────────────── Dry ───────────────────────────────────────────────── + ─ out
 loop chain: filter (LP/HP/BP, LFO on cutoff) → sample-rate hold → Limit / Sat → DC block → bits → diffusion
```

The chain sits between the read head and both the output and the feedback, so the first echo is
already shaped. With model **Off** there is no ring: the chain processes the input directly.

**Parameter ids** (for automation, `synkopa set-param` and `synkopa catalog`; order =
`dsp::delay::idx`):

| id | range | notes |
|---|---|---|
| `input` | 0–100 % | level into the loop |
| `sync` | Tempo / ms | |
| `beats` | 1/48–4 beats | synced time; the knob snaps to note values |
| `time` | 1–2000 ms | free time |
| `keep` | Off / On | Off = Repitch (glide set by `smooth`: 0 % jump, 100 % ≈ 1.5 s); On = two read heads crossfade over 40 ms |
| `smooth` | 0–100 % | |
| `offset` | −100–100 % | Mono: multiplies one side's loop time (left when negative). Stereo: a pre-delay of that share of the time on one side |
| `pan` | −100–100 % | Ping-pong only: balance on the input |
| `model` | Mono / Stereo / Ping-pong / Off | |
| `spread` | 0–100 % | wet width |
| `fb` | 0–125 % | |
| `ftype`, `cutoff`, `res` | Off/LP/HP/BP, 20 Hz–20 kHz, 0–100 % | state-variable loop filter, stable under modulation |
| `srate`, `bits` | 500 Hz–48 kHz, 2–24 bit | sample-and-hold and quantiser; the top of each range is off |
| `dist`, `level`, `knee`, `sym` | Limit/Sat, −36–0 dB, 0–100 %, 0–100 % | Limit clips at the threshold. Sat is linear up to the knee, then a tanh curve that never exceeds the threshold. Symmetry lets the negative half run up to 4× higher before it bends (even harmonics); a DC blocker follows |
| `diff`, `dspread` | 0–100 % | four all-passes blended into each repeat |
| `rate`, `mtime`, `mcut` | 0.05–20 Hz, 0–100 %, 0–100 % | sine LFO on the time (±6 ms at 100 %, at most 45 % of the time) and on the cutoff (±3 octaves) |
| `mix`, `tone`, `dry` | 0–100 %, −100–100 %, 0–100 % | wet level (labelled **Wet**), post-loop tone filter, dry level |
| `duck` | 0–12 dB | see below |

The first delay's ids `time`, `fb` and `mix` are kept, so its automation still applies. `duck` is
the last slot, so projects and automation saved before it keep their indices (it loads as 0 dB).

**Ducking.** The most the wet is pulled down while the dry input plays: input from −45 dBFS up
to −15 dBFS ramps it from 0 to the full amount; 15 ms down; once the phrase ends the wet blooms
back over about a second. It acts on the wet output after the loop. The ducker (`dsp::ducker`)
is shared with the Reverb. On a send, the side-chain is the send's own input.

**Real-time behaviour.**

- No allocation after `prepare`. The ring holds 4 s plus modulation per channel; a synced time
  longer than 4 s at a very slow tempo is clamped.
- Continuous controls glide (20 ms; offset and diffusion spread 80 ms). The loop filter and the
  all-pass lengths are updated every 16 samples.
- The loop is bounded by construction: Sat never exceeds its threshold and Limit clips at it;
  every sample written into the loop is clamped to ±4 and a non-finite value becomes 0.
- Changing the model dips the wet path for 10 ms and switches at the bottom (no click). The ring
  keeps its content.
- `tail_samples`: the repeats until −140 dB from the feedback × filter-resonance bound; when that
  bound is ≥ 1 the loop may sustain forever and the chain is never skipped.
- Zero latency.

**Old projects.** A project saved with the first delay (only `time` 10–1500 ms, `fb` 0–95 %,
`mix`) is migrated on load (`dsp::delay::migrate_params`): free time with the same ms, the same
feedback and wet, no loop filter, a hard-knee Sat at 0 dB, full dry. Automation of its `time`
and `fb` is rescaled to the new ranges.

**Meters** (`Processor::meters`): `[0]`/`[1]` read heads L/R in ms (with the LFO), `[2]` loop
level dBFS, `[3]` level the drive stage takes off (dB ≤ 0), `[4]` LFO −1..1, `[5]` model in use,
`[6]` wet output dBFS, `[7]` the set time in ms at the current tempo, `[8]` ducking gain dB (≤ 0,
lowest of the block; the window shows it next to the time when the wet is ducked).

**Tests.** `dsp::delay::tests`: a synced echo lands on the beat and moves with the tempo; ms
mode; ping-pong alternates sides; stereo offset; 125 % feedback into a resonant band-pass for
20 s of noise stays finite and within ±4; self-oscillation sustains above 100 %; keep pitch holds
the frequency through a time change while repitch bends it; model Off has no delay; bit
reduction; the drive curve; the tail estimate; the old-project migration; ducking (a noise burst
then silence: ~12 dB down during the burst, full level after it; at 0 dB the output is
unchanged). `dsp::ducker::tests`: the shared ducker is bit-identical to the reverb's former inline
one. UI: `ui/src/lib/delay.test.ts` (beat labels, note names, snapping, tap diagram, loop gain).

## See also

- [Reverb](reverb.md): the same Ducking control on a space.
- [Chorus and Phaser](chorus-phaser.md): for short modulated doubling.
- [All effects](README.md)
