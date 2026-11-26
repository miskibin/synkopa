# Making beats

How to write drums, bass, chords and melodies in the Channel rack and the Piano roll, and how to
build a kit from samples.

![The Channel rack: one row per channel, sixteen steps per bar](../pr/rack-groups-filter.png)

## Quick start

1. Press `F6` for the **Channel rack**. Click steps to light them, right-click to clear them.
2. Press `L` for **PAT** and Space to loop the pattern while you edit.
3. Add an instrument with **+ Channel**, then press `F7` to write its notes in the **Piano roll**.
4. Press `F4` for a new, empty pattern when you start the next part (a fill, a chorus beat).

## Program drums in the step sequencer

Each channel row has, from left to right: a mute light, pan and volume knobs, the mixer insert box,
the name, and the steps of the current pattern.

- **Click** a step to light it, **right-click** to clear it, **drag** to paint or clear several.
- **Click the light** to mute the channel; `Ctrl`+click or right-click it to solo.
- `Ctrl`+click a channel's **name** to hear it. A plain click opens its window (sample, envelope,
  synth or instrument controls).
- **Right-click a channel** for its menu: **Piano roll**, **Replace** (swap the instrument but keep
  the notes), **Import MIDI into this channel…**, **Fill every 2 / 4 / 8 steps**, **Move left /
  right**, **Clear**, **Rename…**, **Group**, **Clone channel**, **Delete channel**.
- `↑` / `↓` select the channel above or below; `Alt+↑` / `Alt+↓` move it. `Shift+Ctrl+←` / `→`
  shift its steps by one, wrapping around the pattern.
- `Ctrl+C`, `Ctrl+X`, `Ctrl+V` copy, cut and paste the selected channel's steps (also into another
  channel or another pattern).

### Pattern length and swing

A pattern is as long as its last note, rounded up to a whole bar, like FL's automatic pattern
length. The **−** and **+** around the bar count in the rack's title bar remove the last bar or
keep an extra empty one (for a crash every second bar). **Swing** delays every second sixteenth:
around 67 % feels close to triplets, 100 % is the MPC's 75 % swing.

### Velocity, key, pan and shift per step

Press `G` (or the graph button in the title bar) for the **graph editor** of the selected
channel. Its switch picks what you draw:

| Lane | What it does | Try |
| --- | --- | --- |
| **Velocity** | How hard each step hits | Lower every second hat for a groove |
| **Key** | The pitch of each step, around the channel's root | An 808 that follows the chord |
| **Pan** | Places each step left or right | Hats that move across the stereo field |
| **Shift** | Starts a step a little later inside its cell | Pushed hats, flams, hand-made swing |

`Alt`+click a step in the graph to put it back to its default.

### Organise a big kit

Right-click a channel → **Group** to put it in a group (Drums, Bass or a new one). The filter at the
top of the rack shows every channel, one group, or the unsorted ones. Hidden channels keep playing.
The rack grows with its channels; turn that off with **Auto height** in its title-bar menu.

## Add instruments

Click **+ Channel** at the bottom of the rack. The menu lists:

- **Sampler**: every sample of the standard library by group (kicks, snares, claps, hats, cymbals,
  toms, percussion, tonal, orchestra, FX, loops), the project's own audio, and **File from disk…**.
- **Synthesizer**: the built-in synth's presets by category (808, Bass, Pluck, Lead, Pad, Keys,
  Bells, Brass/Strings, Arp, Lo-fi, FX), or **Empty synth**.
- **Sampled instruments**: **Synkopa Instruments** (pianos, strings, guitars and more) and
  **Music Box**.
- **Plugin instruments (VST3)**: the instruments found in the standard VST3 folders, with
  **Rescan plugins** for new ones (Windows and Linux).

You can also drag a sample from the Browser (or a WAV or MP3 from your file manager) onto the rack:
between two rows it makes a new channel, onto a row it replaces that channel's sample. See
[Instruments](../instruments/README.md) for what each instrument is good at.

## Write notes in the piano roll

Select a channel and press `F7`, or right-click it → **Piano roll**. The piano roll shows the
selected channel's notes in the current pattern; the other channels' notes show as faint *ghost
notes* (double right-click one to switch to its channel).

| Tool | Key | What it does |
| --- | --- | --- |
| **Draw** | `P` | Click = a note with the last length used; drag a note to move it, its edge to change its length; right-click deletes |
| **Paint** | `B` | Drag to paint notes along the grid |
| **Erase** | `D` | Click or drag over notes to delete them |
| **Select** | `E` | Drag a rectangle to select; `Shift` adds |
| **Chord stamp** | `C` | Click = the chosen chord rooted at the clicked key |

In every tool: `Ctrl`+drag selects with a box, `Shift`+drag a note clones it, hold `Alt` to place
off the grid, and double-click a note to type its start, length, velocity and pan. **Snap** in the
bar above the grid sets the grid (auto follows the zoom).

### Shape the notes

- **Velocity and pan:** the lane under the grid switches between **Vel** and **Pan**. Drag in it
  (right-drag draws a ramp), or hold `Alt` and turn the wheel over a note (`Ctrl+Alt` for fine).
- `Ctrl+Q` quantizes the note starts to the grid, `Ctrl+L` makes each note reach the next (legato).
- `Shift+↑` / `↓` moves notes a semitone, `Ctrl+↑` / `↓` an octave, `Shift+←` / `→` one grid step.
  With nothing selected these keys take every note of the channel.
- `Ctrl+B` duplicates the selection to the right; `Ctrl+C` / `Ctrl+V` copy and paste (a paste keeps
  the notes' place in the bar).
- `Ctrl+Shift+X` (or **To new pattern**) moves the selected notes into a new pattern at the same
  place. Every Playlist clip of the old pattern gets a clip of the new one beside it, so the song
  sounds the same.

### Chop, glue, arpeggiate and slide

- `Ctrl+U` chops notes into pieces one snap step long, for rolls and stutters; `Alt+U` opens the chop
  tool with a length of your choice. `Ctrl+G` glues touching notes of the same key back together.
- `Alt+A` (or **Tools → Arpeggiate**) turns chords into runs: **Up**, **Down**, **Up and down** or
  **Random**, with the step, gate and octave range you choose.
- **Slide notes**, as in FL: press `S` (or the glide button) and the notes you draw bend the note
  already playing to their key instead of sounding themselves; their length is the glide time. An
  808 can jump an octave and back in one held note. Plugin instruments ignore slide notes.

### Chords, scales and generators

- The **song key** (in the toolbar and in the piano roll's bar) highlights the scale on the grid in
  every piano roll. With no key set it says *No key* and can detect one from the project's audio.
- **Tools** opens note transforms, each one undo step: **Humanize**, **Quantize**, **Euclidean
  fill**, **Strum**, **Drum generator**, **Melody generator**, **Chord (harmonize)**,
  **Progression** (templates such as Pop I–V–vi–IV, Trap i–iv–VI–v, ii–V–I), **Chop**, **Glue** and
  **Arpeggiate**.
- The **Chord stamp** tool (`C`) writes a whole chord per click; pick the chord type and inversion
  in the bar above the grid.
- To get chords from a recording or a sample, see [Stems and chords](stems-and-chords.md).

## Play notes in live

- **Computer keyboard:** `Ctrl+T` (or **Typing piano** in the toolbar) plays the selected channel
  from two rows of letters: `Z S X D C V…` from C4 and `Q 2 W 3 E R…` an octave up. `Page Up` /
  `Page Down` change the octave. While it is on, those letters play instead of their shortcuts.
- **MIDI keyboard:** pick it in **File → Settings… → MIDI → Device**. **Plays on** chooses which
  channel it plays (by default the one selected in the Channel rack or Piano roll).
- **Record notes:** right-click the record button and tick **Notes** (and **Quantize notes** if you
  want). In PAT mode, Record + Play loops the current pattern and adds what you play to it; in SONG
  mode the notes become a new pattern on the Playlist.

## Build a kit from samples

The Browser's **LIBRARY** tab (`Alt+F8`) holds the standard library that ships with Synkopa (drums,
percussion, tonal one-shots, orchestra hits, FX and loops), **KITS TO DOWNLOAD** (free kits, one
**Install** button each), and **FOLDERS** of your own samples (**Add**).

- Click a sample, or use `↑` / `↓`, to hear it; `Esc` stops the preview.
- `Enter` or **Add as sampler channel** makes a Channel rack channel that plays it.
- Drag it onto the rack for a sampler channel, or onto a Playlist track for an audio clip.

A sampler channel's window has the sample, its start and end, fades, reverse, normalize, pitch, a
volume envelope and cut groups (a closed hat that cuts the open one). See
[Sampler](../instruments/sampler.md).

### Chop a loop

Open a sampler channel's window and its **Slice** page. **Transients** cuts at every hit (raise
**Sensitivity** to catch quieter ones); **Equal** cuts into 4, 8, 16 or 32 parts. Click a slice to
hear it, drag a marker to move it, double-click to add or remove one. Each key from the root plays
one slice. **Dump to pattern** writes one note per slice in the original timing, so the pattern
plays the loop back at any tempo; move the notes to rechop it.

## Import MIDI

**File → Import MIDI file…** brings a `.mid` file in as a pattern (with the file's tempo if you
want it). A channel's menu → **Import MIDI into this channel…** imports one part into that channel.

## See also

- [The basics](the-basics.md) for channels, patterns and inserts.
- [Arranging](arranging.md) to turn patterns into a song.
- [Instruments](../instruments/README.md): [Synth](../instruments/synth.md),
  [Synkopa Instruments](../instruments/synkopa-instruments.md),
  [Music Box](../instruments/music-box.md), [Sampler](../instruments/sampler.md).
- [Shortcuts](shortcuts.md#channel-rack) for every key in the rack and the roll.
