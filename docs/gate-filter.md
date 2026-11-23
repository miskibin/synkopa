# Gate and Filter

Two mixer effects. Both can listen to another insert (sidechain, `Fx.key`), as the Compressor does.

## Gate (`dsp/gate.rs`)

A noise gate, downward expander and ducker. It cleans up noise and bleed between notes and
hits, ducks the music under a voice, and chops a pad in time with a kick or a hi-hat (a trance
gate).

| Parameter | Range | What it does |
| --- | --- | --- |
| `thr` | −80…0 dB | The gate opens when the detector reaches this level. |
| `hyst` | 0–24 dB | The gate closes only once the level is this far below `thr`, so a decaying note does not chatter. |
| `ratio` | 1:1 … ∞:1 | Below the close threshold, the output drops `ratio − 1` dB for every dB the input drops (an expander). At the top of the range (shown as ∞) it is a hard gate. |
| `floor` | −∞…0 dB | The deepest the gate turns the sound down. −14 dB sounds more natural than silence. |
| `att`, `hold`, `rel` | 0.1–50 ms, 0–500 ms, 5–2000 ms | How fast it opens, how long it stays open after the level drops, and how fast it closes. Attack and release are 10–90 % times. |
| `look` | 0 / 1 / 2 / 5 / 10 ms | Look-ahead. The sound is delayed, but the detector is not, so the gate is already open when a drum hit lands. Reported as latency and compensated. |
| `mode` | Gate / Duck | Duck reverses the gate: the sound is turned down to `floor` while the detector hears something. Attack then sets how fast it ducks. |
| `schp`, `sclp` | 20–4000 Hz, 200–20000 Hz | A 24 dB/oct band on what the detector hears (both are off at the ends of their range). |
| `det` | Peak / RMS | Peak reacts at once and lets go over 10 ms. RMS (10 ms) ignores clicks. |
| `listen` | Off / On | The output becomes what the detector hears, so you can tune the band. |

- **Detector.** It hears the insert's own input, with left and right linked (the louder side decides), or the sidechain.
- **Envelope.** A two-pole, critically damped curve with no overshoot and no corner. Opening is smoothed in linear gain, closing in dB. Even a 0.1 ms attack does not click. Measured on a gated 100 Hz tone, high-passed at 4 kHz: edge splatter stays under −65 dBFS at a 2 ms attack and under −50 dBFS at 0.5 ms.
- **States.** Closed → open (at `thr`) → hold (below `thr − hyst`, for `hold` ms) → closing → closed.
- **Cost.** About 25× real time in a debug build and several hundred × in release (10 s at 48 kHz on one core).

Meters: `[0]` the detector's peak in dBFS, `[1]` the gain in dB, `[2]` the state (0 closed, 1 open, 2 hold, 3 closing), `[3]` 1 while it listens to a sidechain.

Presets: Bramka — uniwersalna, Wokal — naturalne czyszczenie, Stopa — krótko i mocno,
Werbel — bez przesłuchów, Tomy — czyste wybrzmienie, Gitara — bez szumu i brumu, Ekspander —
delikatnie, Trance gate (sidechain), Ducking — bas pod stopę, Ducking — muzyka pod lektora,
Gated reverb — werbel lat 80.

## Filter (`dsp/filter.rs`)

A resonant filter whose cutoff moves. You can move it by hand (automation), with an LFO, or with the level of the sound, the insert's own or a sidechain's.

| Parameter | Range | What it does |
| --- | --- | --- |
| `type` | LP / Ladder / HP / BP / Notch / DJ | See below. |
| `cutoff` | 20 Hz – 20 kHz | |
| `res` | 0–100 % | SVF: Q from 0.5 to 64. Ladder: feedback up to 4.15, where it self-oscillates (from about 96 %). |
| `slope` | 12 / 24 dB | The SVF types only. At 24 dB the first stage is Butterworth and only the second resonates, so Resonance feels the same at both slopes. |
| `drive` | 0–24 dB | Saturates (tanh) before the filter, with an automatic trim. It blends in over the first dB, so 0 dB is exactly clean. |
| `dj` | LP 100 % … Off … HP 100 % | DJ type only. The middle ±1 % passes the sound bit for bit. The filter fades in over the next 4 %. Left is low-pass from 20 kHz down to 20 Hz, right is high-pass from 20 Hz up to 20 kHz. |
| `sync`, `rate`, `div`, `shape` | 0.05–20 Hz or 1/16 … 8 bars; Sine, Triangle, Saw up, Saw down, Square, S&H | The LFO. Synced, it follows the song position, and S&H picks the same step at the same bar. |
| `lfo` | ±5 octaves | How far the LFO moves the cutoff. |
| `stereo` | 0–180° | The right side's LFO runs this far behind the left side's. |
| `env`, `eatt`, `erel` | ±6 octaves, 1–200 ms, 10–1000 ms | Envelope follower. It reads the level in dB: −40 dBFS gives 0 and 0 dBFS gives 1, so quiet and loud parts both move the filter. |
| `clip` | Off / On | A smooth limiter on the output. It is linear up to −3 dBFS, then rises towards 0 dBFS. It catches resonant peaks. |

- **SVF types.** Zero-delay-feedback state-variable filter with trapezoidal integration (A. Simper, V. Zavalishin). It stays stable and click-free while the cutoff moves every sample. The cutoff is pre-warped, so it sits at the same frequency at 44.1–192 kHz.
- **Ladder.** Four one-pole stages with tanh in the feedback path. The nonlinearity is linearised around the previous sample (the "cheap" nonlinear ZDF ladder), so it growls when pushed and self-oscillates without running away. The input is raised by half the feedback, so at 85 % resonance the low end loses about 6 dB instead of 11.
- **Modulation.** All cutoff movement is in octaves, smoothed per sample over ~20 ms. LFO jumps (saw, square, S&H) are rounded over 1.5 ms, so they do not click.
- **Latency and oversampling.** There is no latency and no oversampling. With drive, aliasing on a 4.1 kHz tone at +12 dB of drive into a bright resonant ladder measures 44 dB below the fundamental (48 kHz).
- **Cost.** More than 100× real time in a debug build (the heaviest settings, 10 s at 48 kHz).

Meters: `[0]`/`[1]` the cutoff right now, left and right, in Hz; `[2]` the envelope, 0..1; `[3]` the LFO phase; `[4]` DJ: how much of the filter is heard; `[5]` 1 while the soft clip works; `[6]` 1 with a sidechain.

Presets: Ciepły low-pass (ladder), House — zamknięty low-pass, Narastanie — high-pass (4 takty),
Wah — ćwierćnuty, Auto-wah — gitara, bas, Rytmiczny filtr 1/8, Lo-fi — telefon, Dub — rezonans
stereo, Pompowanie (sidechain od stopy), Acid — 303, S&H — losowe kroki 1/16, DJ — przejście.

The English UI shows the preset names translated (`ui/src/i18n/names-fx.ts`).

## Sidechain

Pick the source in the effect's window: the *Sidechain* box on the Gate, the box under
*Envelope* on the Filter. The mixer's *Sidechain to ▸* adds or keys a **Compressor** (ducking).
It leaves a Gate or Filter alone.

From the command line:

```
synkopa add-fx --insert 3 --type gate --preset 7
synkopa sidechain --fx ID --from 1
```

The first command adds a trance gate on the bass. The second makes the kick on insert 1 drive it.
