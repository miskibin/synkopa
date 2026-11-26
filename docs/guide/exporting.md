# Exporting

How to render your song to WAV or MP3, export stems for mixing elsewhere, bounce parts in place,
and share a project with everything it needs.

## Quick start

1. **File → Export audio…** (`Ctrl+Shift+R`).
2. **Output: Mix**, **Range: Song**.
3. **Format: MP3** at **320** for sharing, or **WAV** for mastering.
4. Click **Export…** and choose where to save.

## The Export dialog

### What to export

| Control | What it does | Try |
| --- | --- | --- |
| **Output** | **Mix**: one file with everything. **Stems**: one file per mixer insert that has sound, named after it, all the same length | Stems to mix or master in another program |
| **Master** (Stems only) | **Include** also writes the whole mix with the Master's effects; the stems themselves skip the Master, like FL Studio | Include it as a reference for the mixing engineer |
| **Range** | **Song**: the whole Playlist. **Pattern**: the current pattern alone, as PAT plays it. **Loop**: the loop drawn on the ruler | Loop to bounce one section |
| **Tail** | How long to keep after the end for reverb and delay to fade: **Cut**, **2 s**, **8 s**, or **Wrap** (the tail goes to the start of a loop, for a seamless loop) | Wrap for a loop you will sell or reuse |

Muted inserts are skipped in a stem export and solo is ignored. A song export ends at the bar where
the last clip ends (see [Arranging](arranging.md#where-the-song-ends)).

### File format

| Control | What it does | Try |
| --- | --- | --- |
| **Format** | **WAV**: uncompressed, for editing or mastering. **MP3**: a smaller file for sharing | MP3 to send a demo |
| **MP3 quality** | 320, 256, 192 or 128 kbps | 320 = best quality |
| **Bit depth** (WAV) | **16** (CD, widest compatibility), **24** (masters and delivery), **32 float** (for further editing; nothing over 0 dBFS clips) | 24 for a mastering engineer |
| **Dither** (16 and 24 bit) | Adds a little noise at the final conversion so fades turn into soft hiss instead of distortion | On for 16-bit |
| **Sample rate** | **48 kHz** or **44.1 kHz** | 48 for video and most streaming, 44.1 for CD |

The line under the settings shows the file name, the length, the format and the expected size.
For stems, the button reads **Pick folder…**: choose a folder and every stem goes there.

While it renders, the dialog shows the elapsed and remaining time and the peak so far. When it is
done the status line names the file, its length, its peak level and any clipped samples.

### If the song is empty

If the Playlist is empty, **Song** would be silence: the dialog says so and offers to export the
current pattern instead.

## Avoid clipping

The toolbar's output meter turns red with **CLIP** when the mix goes over 0 dBFS, and an export
clips at exactly that point (except 32-bit float WAV). New projects have a **Limiter** on the
Master. Main volume and the Master fader come after the Master's effects, so turn them down rather
than up if the meter clips. See [Mixing](mixing.md#check-loudness-and-the-master).

## Bounce in place

- **Export in place** (`Ctrl+Alt+E`, or the clip menu, or **File → Export in place**): the
  selected clips, with their mixer effects, become one audio clip on a new track below; the
  originals are muted. With nothing selected and a loop set, it renders the loop range.
- **File → Export loop range in place:** what plays inside the loop range becomes one audio clip;
  clips crossing its edges are split, so the parts outside keep playing.

A bass ducked by the kick stays ducked in the bounced audio, even when the kick is not part of the
selection.

## Export MIDI

**File → Export song as MIDI…** writes the song as a standard MIDI file, one track per channel. A
pattern alone can be exported as MIDI too.

## Share a project

**File → Export project as ZIP…** bundles the `.synkopa` file with its samples and recorded takes.
The person you send it to unzips the whole archive and keeps the `Audio` folder beside the
`.synkopa` file. Third-party plugins must be installed on their computer; files loaded inside a
plugin are not collected.

If a project opens with audio missing (files that were moved), the **Missing audio** dialog lets
you point at a folder; it relinks everything it finds there and in its subfolders.

## From the command line

`synkopa render --out mix.wav` bounces offline and prints the peak, loudness (LUFS) and clipping
statistics; `synkopa export-in-place` bounces a range. See [Command line](../cli.md).

## See also

- [Mixing](mixing.md) and [Limiter](../effects/limiter.md).
- [Arranging](arranging.md) for loops and the song end.
- [Troubleshooting](troubleshooting.md).
