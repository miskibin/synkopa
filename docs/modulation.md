# Chorus and Phaser

Two mixer effects in the **Modulation** category (`crates/engine/src/dsp/modulation.rs`).

## Chorus

Mixes the input with copies of itself read from a delay line whose length an LFO sweeps.

| Mode | Copies a side | What it is for |
| --- | --- | --- |
| Chorus | 2, at `delay` and 1.37 × `delay` | classic thickening and width |
| Ensemble | 3, a third of the LFO cycle apart, plus a fast shallow second LFO | strings, pads, wide synths |
| Vibrato | 1, no dry signal | pitch wobble only (`delay` and `fb` do not apply) |

The right side reads the LFO a quarter cycle later (Ensemble: a sixth), so a mono source turns
stereo and still sums to mono without cancelling.

| Parameter | Range | Notes |
| --- | --- | --- |
| `mode` | Chorus / Ensemble / Vibrato | |
| `sync`, `rate`, `div` | 0.05–10 Hz, or 1/16 … 8 bars | with `sync` = Tempo the LFO follows the song position while playing |
| `depth` | 0–100 % | sweep of ±¼ of `delay` (at most ±6 ms); Vibrato: up to ±6 % pitch |
| `delay` | 2–40 ms | base delay of the copies |
| `fb` | −90…+90 % | copies fed back through a soft clipper; negative sounds hollow |
| `width` | 0–200 % | mid/side width of the copies only; the dry image is untouched |
| `hp` (Safe bass) | 20–1000 Hz | high-pass before the delay line: the low end stays steady (Vibrato passes it dry) |
| `tone` | 1–20 kHz | low-pass on the copies, darker like bucket-brigade chips |
| `mix` | 0–100 % | linear dry/wet; 50 % is the classic sound |

## Phaser

2, 4, 6, 8 or 12 first-order all-pass stages whose break frequency an LFO (triangle) sweeps
exponentially around `center` — ±3 octaves at full `depth`, equal time per octave. Mixed 50/50
with the dry signal, every two stages cut one notch; `fb` (last stage back to the first, through
a soft clipper) turns the notches into resonant peaks, negative feedback moves them. `stereo`
offsets the right side's LFO by up to 180°.

| Parameter | Range |
| --- | --- |
| `stages` | 2 / 4 / 6 / 8 / 12 |
| `sync`, `rate`, `div` | 0.02–10 Hz, or 1/16 … 8 bars |
| `center` | 100 Hz – 8 kHz |
| `depth` | 0–100 % |
| `fb` | −95…+95 % |
| `stereo` | 0–180° |
| `mix` | 0–100 % (50 % = deepest notches) |

## Behaviour

- Continuous parameters glide over ~20 ms, so automation and knob moves do not click.
- No latency; the output does not depend on the block size or the number of render workers.
- The phaser's notches sit at the same frequencies at 44.1, 48 and 96 kHz.
- Cost (10 s of audio at 48 kHz, one core): Ensemble about 150×, a 12-stage phaser about 200×
  faster than real time.

Presets: Chorus — Klasyczny chorus, Wokal — zagęszczenie, Szeroki syntezator, Vintage — dwa BBD,
Czysta gitara lat 80, Bas — stabilny dół, Smyczki — ensemble, Wir z tempem (1 takt), Vibrato.
Phaser — Klasyczny, 4 stopnie, Delikatny, Głębokie przemiatanie, Funk — gitara (1/8),
Odrzutowiec, Wir stereo, Pusty (ujemny feedback), Pad — przemiatanie (2 takty). The English UI
shows them translated (`ui/src/i18n/names-fx.ts`).

From the command line: `synkopa add-fx --insert 3 --type chorus --preset 1`, then `set-param` and
`load-preset` as for any built-in effect.
