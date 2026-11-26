# Listening: render and stems

`synkopa render` bounces the song, a range or one pattern to a file offline and replies with level
statistics, so you (or a script) can check a change before listening to it.

The render does not use the audio device, so it works while the musician keeps playing, and on project
files with no app at all.

## Quick start

```sh
synkopa render --out bounce.wav --from 0 --to 32        # bars 1–8 to a WAV
synkopa render --pattern 2 --out hats.wav               # one pattern alone
synkopa render --stems --out stems/ --from 0 --to 32    # one file per mixer insert
```

Then read the `stats` in the reply and open the file.

## Read the statistics

Why: a number tells you at once whether a render clipped or came out silent.

Every render replies with `stats`:

| Field | Meaning |
| --- | --- |
| `peakDbfs` | Highest sample level, dBFS |
| `rmsDbfs` | Average (RMS) level, dBFS |
| `crestDb` | Peak minus RMS: low means squashed, high means spiky |
| `clippedSamples` | Samples at or beyond full scale |
| `silent` | `true` when nothing audible was rendered |
| `seconds`, `sampleRate` | Length and rate of the file |

A stem render lists every file with its own statistics under `files`.

## Render the mix

Why: hear the song, a section or a loop exactly as the export would sound.

```sh
synkopa render --out bounce.wav                         # the whole song
synkopa render --out bounce.wav --from 0 --to 32        # beats 0–32
synkopa render --out bounce.mp3                         # MP3, chosen by the .mp3 extension
synkopa render --out cd.wav --wav-bits 16 --dither --rate 44100
synkopa render --pattern 2 --out hats.wav               # pattern 2 alone, as PAT mode plays it (an empty Playlist is fine)
```

| Option | What it does |
| --- | --- |
| `--from B --to B` | Range in beats |
| `--tail SEC` | Effect tail after the range, in seconds |
| `--rate HZ` | Sample rate, for example 44100 or 48000 |
| `--format wav\|mp3` | Output format (an `.mp3` file name also picks MP3) |
| `--kbps 128\|192\|256\|320` | MP3 bit rate (default 320) |
| `--wav-bits 16\|24\|32` | WAV sample format: 32-bit float (default), 24-bit or 16-bit PCM (CD audio) |
| `--dither` | Triangular dither of ±1 LSB, applied once at the final conversion (16- and 24-bit only) |
| `--wrap-tail` | Carry the effect tail into the start of the loop (WAV only) |
| `--pattern P` | Render pattern P alone |

Integer WAV (16- and 24-bit) clips samples beyond full scale. The statistics report peaks before that
conversion, so a peak above 0 dBFS there means the file clipped.

The same bounce is the app's **Export** (`Ctrl+Shift+R`): song or loop, a 0/2/8 s effect tail, and
44.1 or 48 kHz.

### Make a seamless loop

Why: a loop whose reverb tail cuts off at the end clicks when it repeats.

`--wrap-tail` carries the effect tail into the start, so the file is exactly one loop long. Use it with the
project loop, or give both `--from` and `--to`. It works for stereo mixes and stems, in WAV only, and not
with `--pattern`. In the app this is **Wrap** in the Export dialog.

```sh
synkopa render --out loop.wav --from 0 --to 16 --wrap-tail
```

## Export stems

Why: hand the parts of your mix to someone else, or check each insert on its own.

```sh
synkopa render --stems --out stems/ --from 0 --to 32     # a folder, created if needed
synkopa render --stems --out stems/ --no-master          # without Master.wav
synkopa render --stems --out stems/ --wav-bits 24 --dither
synkopa render --stems --out stems/ --format mp3 --kbps 192
```

You get one stereo file per mixer insert that has sound, named after the insert (`Drums.wav`, `Bass.wav`; a
repeated name becomes `Bass (2).wav`). In the app: **Export → Output: Stems**, choose WAV or MP3, then pick
the folder.

- **What a stem contains.** What the insert plays after its own effects, fader, pan and width, **before the
  master**. With mixer routing, a bus's stem includes what is routed into it, and a bus fed only by sends
  still gets a stem. A source's stem is its own output, even if it normally goes only into a bus.
- **The master file** contains the normal mix with the master's effects. `--no-master` leaves it out.
  Nonlinear effects on the master, such as a limiter, can make the mix sound different from the sum of the
  stems.
- **Same length.** Every file uses the same range and effect tail. Use WAV for sample-accurate alignment:
  some editors ignore MP3 encoder delay and padding.
- **Skipped inserts.** Muted inserts and inserts with nothing routed into them get no file, and neither
  does one that turns out silent in the range (the reply lists those under `skipped`). Solo is ignored, so
  a soloed insert does not silence the other stems; a muted track or channel stays silent.
- **Formats.** WAV is the default. MP3 supports 128, 192, 256 and 320 kbps (default 320).
- **Writing.** Files are staged during export and moved into place after all stems are encoded. Existing
  files with the same names are replaced.

## Technical notes

- The render uses the same bounce as the app's export. Synkopa Instruments recordings must be present, or
  the render fails with `KEYS_MISSING` instead of rendering silence.
- `--out` is resolved against your current folder before it is sent to the app.
- `clippedSamples` counts samples with an absolute value of 0.999 or more.
- `pitch render` (one vocal clip with its pitch edits) is described in
  [Recording and vocals](recording-and-vocals.md#edit-pitch-note-by-note).

## See also

- [Exporting](../guide/exporting.md): the Export dialog in the app
- [Audio clips](audio-clips.md): stem *extraction* from a mixed recording
- [Project files without the app](project-files.md)
