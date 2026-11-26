# EQ

The Parametric EQ shapes the tone of a sound with up to 24 bands, so you can cut mud, add
presence or air, remove a ringing resonance, or make one band dip only when it gets too loud.

## Quick start

1. Add **Parametric EQ** to an insert (mixer, `F9`, then **+ effect**; it is in the **EQ and
   tone** category).
2. Play the track. The graph shows the spectrum before the EQ (grey) and after it (coloured).
3. Drag a numbered point: left and right sets the frequency, up and down the gain. Turn the mouse
   wheel over a point to make it narrower or wider (its *Q*).
4. Double-click empty space on the graph to add a new bell there.

A new EQ starts with seven bands: a low cut (off), a low shelf at 90 Hz, bells at 250 Hz, 1 kHz
and 3.5 kHz, a high shelf at 9 kHz, and a high cut (off). All start flat.

## Cut a resonance fast

When a ringing frequency stands out, the graph highlights the strongest spectrum peak under the
mouse. Drag down from that peak: a new narrow bell (Q 4) appears exactly there and cuts as you
drag. The status line names the frequency and the musical note.

## Clean up the low end

Right-click empty space on the graph and choose **Low cut** to add a high-pass band, or turn on
band 1 (the low cut). Right-click its point to set the **Slope** (12, 24, 36 or 48 dB/oct). The
presets **Low cut 80 Hz** and **Low cut 150 Hz (vocal)** do this in one click.

## Use the band bank

Beside the graph, each band has a strip (as in FL Studio's Parametric EQ 2):

- the shape button at the top: click for the shape menu, drag up or down to step through **off**
  and the shapes;
- a gain fader;
- two small knobs, **FREQ** and **BW** (bandwidth, Q).

Values show in the tooltip and the status line while you move them. Editing a band that is off
turns it back on. The **Output** knob at the left of the bank sets the overall level. Right-click
a fader or knob to automate it; double-click to reset it.

## Treat only the middle or the sides

Right-click a band's point and pick a **Channel**: **ST** (both), **L**, **R**, **M** (mid, what
is in the centre) or **S** (side, what is only in the stereo spread). For example, a low cut on
**S** keeps the bass mono-centred without thinning the middle.

## Make a band react to the level (dynamic EQ)

A *dynamic* band moves its gain only while its frequency range is loud: a harsh 3 kHz that only
stings on loud notes, or a boomy note in a bass line.

1. Right-click a bell, shelf or tilt point and choose **Dynamic: on**.
2. Below the graph, set **THRESH** (the level where it starts to act) and **RANGE** (how far it
   moves, negative to cut, positive to boost).

The graph animates the band's live gain. Once an EQ has a dynamic band, a **Sidechain** box
appears: pick another insert and the dynamic bands dip only when *that* track is loud in their
range, for example a bass that makes room only when the kick hits.

## Band shapes

| Shape | What it does |
|---|---|
| **Bell** | Boosts or cuts around a frequency |
| **Low shelf**, **High shelf** | Boosts or cuts everything below / above |
| **Low cut**, **High cut** | Removes everything below / above (12–48 dB/oct) |
| **Notch** | Removes a narrow band |
| **Band pass** | Keeps only a band |
| **Tilt** | Tilts the whole spectrum around a frequency (brighter one way, darker the other) |

## Mouse reference

| Action | Result |
|---|---|
| Drag a point | Frequency and gain (Shift or Ctrl = fine) |
| Wheel over a point | Q (Shift = fine) |
| Double-click empty space | New bell |
| Drag from a highlighted peak | New narrow bell at that frequency |
| Double-click a point | Turn the band off / on |
| Right-click a point | Shape, slope, channel, dynamic, disable, delete |
| Right-click empty space | New band of a chosen shape at that frequency |

## Presets

Flat, Vocal — presence, Kick — punch, Bass — clean low end, Mud cut, Air / shine,
Band — cut lows and highs, Telephone / lo-fi, Master — smile, Cut < 20 Hz and > 18 kHz,
Low cut 80 Hz, Low cut 150 Hz (vocal), High cut 10 kHz.

## Tips

- Cut narrow, boost wide. A narrow boost tends to sound harsh; a narrow cut is rarely noticed.
- Gain is ±30 dB; the graph shows ±18 dB until a band goes further.

## Technical notes

Code: `Eq` in `crates/engine/src/dsp.rs`; window: `ui/src/views/fx/eq.ts` (graph),
`eqEdit.tsx` (gestures, dynamic band panel), `eqStrips.tsx` (band bank). The response drawn in
the window is a port of the engine's design (`ui/src/lib/eq.ts`), so it matches what you hear.

- Bands live in `Fx.bands` (up to 24), not in numbered parameters; the effect's only parameter
  is `out` (−18 to +18 dB). Band fields: `type` (`peak`, `lowshelf`, `highshelf`, `lowcut`,
  `highcut`, `notch`, `bandpass`, `tilt`), `f` (10 Hz–22 kHz), `g` (±30 dB), `q`, `slope` (12,
  24, 36, 48), `place` (`stereo`, `left`, `right`, `mid`, `side`), `on`, `dynamic`
  (`{range, thr}`).
- Filters: RBJ cookbook biquads (TDF-II). Cuts are cascades of `slope / 12` stages (user Q on the
  first, 0.707 on the rest). Tilt = high shelf at Q 0.5 and gain g, followed by −g/2 dB. No
  latency (minimum-phase IIR).
- Dynamic bands: a band-pass detector at the band's frequency (on its own channel placement, or
  the sidechain), 5 ms attack, 80 ms release (25 ms when keyed, so a bass lets go quickly after
  the kick). Every 32 samples the gain moves towards `range × amount`, where amount ramps from 0
  at 3 dB below `thr` to 1 at 12 dB above it (a soft knee), and the band's coefficients are
  recomputed.
- `tail_samples` adds up the ring-down of the active bands; with a dynamic band there is no fixed
  tail.
- Meters: `[0..24]` each band's current dynamic gain in dB.
- The mix assistant uses a keyed dynamic band to make room for a protected insert
  (`crates/engine/src/masking.rs`).

## See also

- [Filter](filter.md): a moving, resonant filter.
- [De-esser](de-esser.md): for sibilance specifically.
- [All effects](README.md)
