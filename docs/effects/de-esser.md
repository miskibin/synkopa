# De-esser

The De-esser turns down harsh "s", "sh" and "t" sounds in a vocal only at the moments they
happen, so the vocal stays bright without stinging.

![The De-esser window](../../site/img/deesser.png)

## Quick start

1. Add **De-esser** to the vocal insert (mixer, `F9`, then **+ effect**), usually after EQ and
   compression.
2. Play a part with strong "s" sounds. Moments the De-esser treats as sibilant are tinted in the
   history, and the reduction hangs from the top in red.
3. Lower **Threshold** (or drag the **THR** line) until the harsh "s" sounds get 3–6 dB of
   reduction and the rest of the vocal gets none.
4. Use **Range** to cap how far an "s" can be turned down.

## Find the right frequency

**Freq** is where sibilance starts: the De-esser listens from there upwards. Higher voices
usually need a higher setting. The presets **Female vocal** (8 kHz) and **Male vocal** (5 kHz)
are good starting points. You can also drag the frequency handle on the spectrum on the right.

Switch **Listen** to **Only S** to hear exactly what is being removed. If you hear parts of
vowels, raise **Threshold** or **Freq**; switch back to **Off** when done.

## Split or Wide

- **Split** (the default) turns down only the highs above **Freq** during an "s". The body of
  the voice is untouched. Use it in most cases.
- **Wide** turns down the whole signal during an "s". It can sound more natural on very bright,
  breathy sibilance, but it dips the level.

## Why it leaves bright vowels alone

The De-esser does not react to every loud high frequency. It also checks how much of the
sound's energy sits in the high band: an "s" is almost all highs, a bright sung vowel or a
cymbal behind the vocal is not. Only sounds that pass both tests are reduced, so you can set the
threshold low without dulling the vocal.

## Controls

| Control | What it does | Try |
|---|---|---|
| **Threshold** | How loud the high band must get before reduction (−60 to 0 dB) | −32 dB, then adjust |
| **Freq** | Start of the band it listens to (2–16 kHz) | 6 kHz; 5 kHz male, 8 kHz female |
| **Mode** | **Split** (only the highs) or **Wide** (everything) | Split |
| **Range** | The most an "s" is turned down (0–24 dB) | 6–8 dB |
| **Lookahead** | How early it sees an "s" (0–15 ms); adds that much latency | 5 ms |
| **Listen** | **Off** or **Only S** (hear what is removed) | Only S while tuning |
| **Detect** | Under **Advanced**: **Absolute** or **Relative** (threshold on the balance of highs to lows, independent of the take's level) | Absolute |

## Presets

Vocal — default, Harsh "s", Gently, Female vocal, Male vocal, Broadband.

## Common problems

- **The vocal sounds lispy or dull.** Too much reduction: raise **Threshold** or lower **Range**.
  Check with **Only S** that only sibilance is removed.
- **Some "s" sounds slip through at the start.** Raise **Lookahead**. It cannot be automated,
  because it changes the latency the mixer compensates.

## Technical notes

Code: `DeEss` in `crates/engine/src/vocal.rs`; window: `ui/src/views/fx/deess.ts` (a 4 s level
history with input, output, the detector against the threshold, sibilant moments tinted and the
gain reduction from the top; on the right an analyser with the detection band).

**Parameter ids:** `thr` (−60–0 dB), `range` (0–24 dB), `freq` (2–16 kHz), `mode` (Split /
Wide), `look` (0–15 ms; sets latency, not automatable), `listen` (Off / S), `detect` (Absolute /
Relative).

**Absolute detection.** The mono sum is high-passed at `freq` (detector). Two energy envelopes,
the high band and the whole signal, give a sibilance ratio; reduction needs both the high band
over the threshold and the high band holding more than about 25–60 % of the energy (`sib` ramps
from 0 to 1 over that range). Target reduction = 0.8 dB per dB over the threshold × `sib`,
capped at `range`; gain attack 0.8 ms, release 60 ms.

**Relative detection.** A 4th-order Butterworth high-pass at `freq` splits the signal; the
threshold is on the high/low band energy ratio in dB (≈5 ms envelopes), so it does not depend on
the take's level; 1 ms attack, 40 ms release; nothing happens below about −80 dBFS.

**Split.** Absolute mode: low band = LR4 low-pass at `freq`, high band = input − low, and only
the high band is turned down. Relative mode uses its own Butterworth split. **Only S** outputs
the removed part (the high band, or the whole signal in Wide, times `1 − gain`).

**Look-ahead.** The audio is delayed by `look`; the detector is not, so the gain is already down
when the "s" arrives. The delay is reported for compensation (`catalog::latency`).

**Meters:** `[0]` GR dB, `[1]` detector level dB (Relative: high/low ratio dB), `[2]` sibilance
0..1, `[3]` threshold dB.

## See also

- [EQ](eq.md): a dynamic band can also tame a single harsh frequency.
- [Compressor](compressor.md)
- [All effects](README.md)
