# Recording vocals

How to record a voice (or any instrument) over your beat, keep the best parts of several takes,
tune it, and get it sitting in the mix, all in the free build.

![Pitch Correct, the live tuning effect](../../site/img/pitch.png)

## Quick start

1. **File → Settings… → Audio**: pick your **Input** and the **Input channel** your microphone is
   on, then click **Measure latency** once.
2. Drag on the Playlist ruler to set a loop over the part you want to sing.
3. Press `R` to arm recording and Space to start. Each pass of the loop becomes a take.
4. Press Space to stop. Swipe across the takes to build the best version (the *comp*).
5. Right-click the clip → **Auto vocal → Auto vocal — singing** for a ready vocal chain.

## Set up the input

In **File → Settings… → Audio**:

| Setting | What it does | Try |
| --- | --- | --- |
| **Input** | The device you record from | Your audio interface; with ASIO the input is the same driver as the output |
| **Input channel** | The one input that is recorded and monitored, as mono in both ears | Input 1 for a mic in the first socket |
| **Buffer** | How much audio is processed at once | 128–256 samples while recording, larger for mixing |
| **Recording offset** | Shifts every take so it lands on the beat | Set it with **Measure latency** |

**Measure latency** plays a few clicks and records them: hold the microphone near the speaker or
headphone for about five seconds, in a quiet room. Synkopa then knows how far to shift recordings.
If it says the measurement is uncertain, try louder and closer.

Synkopa also measures the delay of the driver during every take, so takes line up even with
plugin delay on the mixer. The offset only covers what the driver does not report.

## Hear yourself and record

- **Monitor** (the headphones switch in the toolbar, or **View → Monitor input**) plays the input
  through the track you record on, with low latency: effects that add delay (Vocal Rider, Pitch
  Correct, Denoiser, Limiter, De-esser) are skipped for monitoring only. The take itself is always
  the raw input.
- **Count-in** (toolbar, or **View → Count-in (1 bar)**) gives one bar of clicks first.
- `Ctrl+M` turns the metronome on or off.
- Right-click the record button to choose what to record: **Audio** (the input becomes takes),
  **Notes** (from a MIDI or computer keyboard), **Automation**.

### Where the recording goes

Nothing has to be armed, as in FL Studio: a recording lands on the **first track that is empty from
the start position on**, or on a new track below when there is none. To record onto a particular
track (and stack more takes on what it holds), right-click its header → **Record into this track**;
a red dot shows it.

**Punch in:** press `R` while the song plays to start recording at the playhead; press it again to
punch out and keep playing.

## Takes and comping

With a loop set, recording cycles over it and each pass becomes a **take**. Recording over an
existing clip on the armed track adds takes instead of overwriting it. The takes play through a
*comp clip*, which you move, trim, fade and mute like any clip.

- Double-click the comp clip (or **Show takes**, or the ▸ in the track header) to open the take
  lanes under it.
- **Drag across a take** to write that part into the comp. **Click a take's name** to use the whole
  take.
- `Ctrl+↑` / `Ctrl+↓` swaps the selected section to the previous or next take.
- Drag a comp boundary to move it; right-click it to set its crossfade. The comp clip's menu sets
  the default **Crossfade**.
- **Delete unused takes** removes takes the comp does not play. **Flatten comp** renders the comp
  into one ordinary audio clip.

Recordings are saved as files in a folder next to the project. Until the first save they live in a
scratch folder and move with the first save.

## Tune the vocal

Synkopa has three ways to fix pitch. Use whichever fits the job.

### Pitch Correct: live tuning

Add **Pitch Correct** to the vocal's mixer insert (category **Vocals and tuning** in the effect
picker). It tunes in real time as the song plays, from natural to the hard robot effect. Set the
**KEY** and **SCALE** (or **Detect key…**), then **RETUNE**: 0 ms is the robot step, longer is more
natural.

### The pitch editor: edit the notes of a take

Right-click an audio clip → **Edit pitch…** (or open the sample editor and click **Pitch editor**).
Synkopa analyses the recording and draws each sung note as a block on a piano grid.

| Tool | Key | What it does |
| --- | --- | --- |
| **Pitch** | `1` | Drag a note up or down; it snaps to the scale (`Alt` = free, in cents) |
| **Drift** | `2` | Flattens the wobble inside a note, 0–100 % |
| **Formant** | `3` | Shifts the formants (the voice's colour) |
| **Volume** | `4` | Note volume in dB |
| **Split** | `5` | Click a note to split it; click a split to merge |
| **Pencil** | `6` | Draw the pitch curve by hand; `Alt`+drag erases the drawing |

To correct everything at once, set the **Key**, pick a **STYLE** (**Natural**, **Pop**, **Tight**,
**Robot**) or set **STRENGTH**, **SPEED** and **FLATTEN** yourself, then click **Correct all** (or
**Correct N selected**). `Delete` resets the selected notes. **Solo** plays the take alone; Space
plays it in the mix. `F` fits the view.

The optional **RMVPE** detector (a 362 MB download, research licence) is more accurate than the
built-in YIN detector; the editor offers it.

### Tune to Reference: make a take follow a melody

Switch the pitch editor's **Mode** to **To reference**. The take is pulled onto a melody instead of
the nearest scale note:

- **A melody from the piano roll:** play the melody in a pattern that runs at the same time as the
  vocal on the Playlist. In the Piano roll, **Vocal reference** opens the pitch editor of the vocal
  under the selected notes. The take keeps its own timing; if you change the melody later, click
  **Update**.
- **Another recording** of the same melody (a guide vocal, an acapella): pick it from
  **Recordings**. Synkopa aligns the two and shows how well they match.

**CORRECTION** sets how strongly to pull, **VIBRATO** how much of the singer's own movement to keep,
**REF. INFLUENCE** whether note centres come from the reference or the nearest scale note, and
**Octave** shifts the reference (Auto picks the nearest). Parts where the match is uncertain are left
as sung.

The other way round, **Copy notes** (`Ctrl+C`) in the pitch editor copies the sung notes for
`Ctrl+V` in the Piano roll, to double a vocal line with an instrument.

## Even out the level

- **Vocal Rider** (effect) evens out word loudness the way an engineer rides the fader.
- **Auto vocal:** right-click a vocal clip (or a comp clip) → **Auto vocal → Auto vocal — singing**
  or **Auto vocal — rap**. Synkopa analyses the recording, levels it, and builds a chain on the
  track's mixer insert: leveling, compression, de-esser, pitch correction in the beat's key, a little
  reverb and a limiter. Rap uses faster leveling and harder tuning. The track must play through its
  own insert, not the Master. One `Ctrl+Z` removes it all.
- **De-esser** tames harsh "s" sounds, **Denoiser** removes noise, hum and room reverb.

## Write the lyrics

Press `F8` for the **Notebook**. Each line is one bar, counted from the page's start bar; the number
beside it shows where it falls on the Playlist. An empty line is a bar of rest, and a line in
brackets such as `[Chorus]` is a header that takes no bar. While the song plays, the current bar is
highlighted. Click a bar number to move the playhead there. **Rhymes** colours line endings that
rhyme, `Ctrl`+wheel changes the font size, `Alt+Z` wraps long lines. The text is saved with the
project and undone like any edit.

## See also

- [Mixing](mixing.md) for effects and routing.
- [Effects](../effects/README.md), including the [De-esser](../effects/de-esser.md),
  [Compressor](../effects/compressor.md) and [Reverb](../effects/reverb.md).
- [Troubleshooting](troubleshooting.md#recording-problems).
- [Command line](../cli.md) for `synkopa auto-vocal` and `synkopa pitch`.
