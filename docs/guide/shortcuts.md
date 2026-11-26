# Keyboard and mouse shortcuts

Every key and mouse gesture in Synkopa, grouped by the window that uses it. Most match FL Studio.

Press `?` in the app for the built-in list, or `Ctrl+K` to find any menu command by name. On macOS,
`Cmd` works wherever this page says `Ctrl`.

A key a window uses for itself wins while that window has the keyboard: for example `Ctrl+L` is
legato in the Piano roll but routes channels elsewhere. Click a window (or press `F5` for the
Playlist) to give it the keyboard.

## Everywhere

### Transport

| Key | Action |
| --- | --- |
| Space | Play / stop (PAT: the current pattern, SONG: the song) |
| `L` | Switch between PAT and SONG |
| `R` | Record on / off (while playing: punch in / out) |
| `Ctrl+H` | Stop and cut every ringing tail (panic) |
| `Home` | Back to the start of the song |
| `F` | Follow the playhead on / off |
| `Ctrl+M` | Metronome on / off |
| `Ctrl+T` | Computer keyboard as piano on / off |

### Windows

| Key | Action |
| --- | --- |
| `F5` | Playlist to the front (hides floating windows; again shows them) |
| `F6` | Channel rack |
| `F7` | Piano roll of the selected channel |
| `F8` | Notebook (lyrics) |
| `Alt+F8` | Browser |
| `F9` | Mixer: show / hide |
| `Shift+F9` | Effect picker for the selected mixer insert |
| `F11` | Full screen |
| `F1` | Ask ChatGPT (opens it with a Synkopa starter prompt) |
| `Ctrl+K` | Find a command |
| `?` | Keyboard shortcuts list |
| `Esc` | Close the menu or the window in front; in the maximized mixer, back to arrange; else deselect |

### Patterns

| Key | Action |
| --- | --- |
| `F4` | Next empty pattern (a new one) |
| `F2` | Rename the current pattern (after a click in the Mixer: rename the selected insert) |
| `Num 1`–`Num 9` | Pick pattern 1–9 |
| `Num +` / `Num −` | Next / previous pattern |
| `Shift+Ctrl+C` | Clone the current pattern |
| Wheel over the pattern selector | Previous / next pattern (past the last: a new one) |

### Project and editing

| Key | Action |
| --- | --- |
| `Ctrl+Z` | Undo |
| `Ctrl+Shift+Z`, `Ctrl+Y` | Redo |
| `Ctrl+S` / `Ctrl+Shift+S` | Save / Save as |
| `Ctrl+N` | Save new version (Beat → Beat_2) |
| `Ctrl+Shift+N` | New project |
| `Ctrl+O` | Open |
| `Ctrl+Shift+R` | Export audio |
| `Ctrl+Alt+E` | Export in place (selected clips, or the loop range) |
| `Ctrl+L` | Route the selected Channel rack channels to the selected mixer insert |
| `Shift+Ctrl+L` | Route them one per insert, from the selected insert on |
| `Alt+F4` | Exit |

### Markers

| Key | Action |
| --- | --- |
| `Alt+T` | Add a marker at the song position |
| `Alt+→` / `Alt+←` | Next / previous marker |
| `Alt+Num *` / `Alt+Num /` | Next / previous marker (number pad) |

### Computer keyboard as piano (`Ctrl+T`)

| Key | Action |
| --- | --- |
| `Z S X D C V G B H N J M , L . ; /` | Notes from the base C (C4 by default) |
| `Q 2 W 3 E R 5 T 6 Y 7 U I 9 O 0 P [ = ]` | An octave up |
| `Page Up` / `Page Down` | Octave up / down |
| `Shift` + a note key | Full velocity |

Physical key positions are used, so the layout of your keyboard does not matter. While it is on,
these keys play instead of their shortcuts.

## Playlist

### Tools

| Key | Tool |
| --- | --- |
| `P` | Draw |
| `B` | Paint |
| `D` | Delete |
| `T` | Mute |
| `C` | Slice |
| `E` | Select |

### Clips

| Key or gesture | Action |
| --- | --- |
| `Ctrl+A` | Select all |
| `Ctrl+D` | Deselect |
| `Ctrl+B` | Duplicate after the selection |
| `Ctrl+C` / `Ctrl+X` / `Ctrl+V` | Copy / cut / paste |
| `Delete` | Delete the selection (or the selected automation points) |
| `Alt+M` | Mute / unmute the selection |
| `Shift+←` / `Shift+→` | Move the selection one grid step |
| `Shift+↑` / `Shift+↓` | Move the selection one track |
| `Shift+M` | Clip edges: trim ↔ stretch audio |
| `Ctrl+↑` / `Ctrl+↓` | Previous / next take of the selected comp section |
| Right-click a clip | Delete it |
| Right-drag | Delete every clip it crosses |
| Right-click a clip's name bar | Clip menu |
| `Ctrl`+drag | Select with a box |
| `Ctrl`+click | Add to / remove from the selection |
| `Shift`+drag a clip | Copy it |
| Hold `Alt` while dragging | Off the grid |
| Double-click a pattern clip | Open the pattern in the Channel rack |
| Double-click an audio clip | Sample editor |
| Double-click a comp clip | Show the take lanes |
| Drag across a take lane | Write that part into the comp |
| Click a take's name | Use the whole take |

### View

| Key or gesture | Action |
| --- | --- |
| `Page Up` / `Page Down` | Zoom in / out at the pointer |
| `Shift+1` / `2` / `3` | Zoom presets |
| `Shift+4` | Whole song |
| `Ctrl+=` / `Ctrl+−` | Zoom in / out |
| `Ctrl`+wheel | Horizontal zoom |
| `Alt`+wheel | Height of all tracks |
| Middle-button drag | Pan the view |
| `Ctrl`+right-drag | Zoom to a range |
| `Ctrl`+right-click | Previous view |
| Drag to the edge of the view | Scroll while dragging |

### Ruler, loop and markers

| Gesture | Action |
| --- | --- |
| Click the ruler | Set the start position |
| Drag on the ruler | Draw a loop |
| Right-drag on the ruler | Draw a loop; with a loop, move its nearer end |
| Drag a loop edge | Resize the loop |
| `Shift`+drag inside the loop | Move the loop |
| `Ctrl`+drag on the ruler | Move the start position |
| Double-click the ruler | Remove the loop |
| Click a marker | Jump there |
| Drag a marker | Move it (`Esc` puts it back) |
| Double-click a marker | Loop its section |
| Right-click a marker | Rename, delete |

### Tracks and automation

| Gesture | Action |
| --- | --- |
| Double-click a track name | Rename |
| Drag a track header | Move the track |
| Drag a track's bottom edge | Height of one track |
| Right-click a track header | Track menu |
| Drag an automation point | Move it (`Shift` locks value, `Ctrl` locks time, `Alt` no snap) |
| Drag the dot between points | Bend the curve |
| Right-click an automation clip | Add a point / point menu |
| Double-click a point / a bend dot | Delete it / straighten it |
| `Ctrl`+drag over an automation clip | Select points |

### Song position slider (toolbar)

| Key | Action |
| --- | --- |
| `←` / `→` | One bar |
| `Shift+←` / `Shift+→` | One beat |
| `Home` / `End` | Start / end |

## Channel rack

| Key or gesture | Action |
| --- | --- |
| Click a step | Light it |
| Right-click a step | Clear it |
| Drag over steps | Paint (right-drag clears) |
| Click a channel name | Open its window |
| `Ctrl`+click a channel name | Preview the channel |
| `Shift`+click names | Select a range of channels |
| `Shift+Ctrl`+click a name | Add / remove one channel |
| Right-click a channel | Channel menu |
| Click the mute light | Mute |
| `Ctrl`+click or right-click the light | Solo |
| Click / double-click the insert box | Pick an insert / give the channel its own insert |
| Wheel over the insert box | Next / previous insert |
| `↑` / `↓` | Select the channel above / below |
| `Alt+↑` / `Alt+↓` | Move the selected channel |
| `Shift+Ctrl+←` / `→` (also `Alt+Shift+←` / `→`) | Shift the channel's steps by one, wrapping |
| `Ctrl+C` / `Ctrl+X` / `Ctrl+V` | Copy / cut / paste the channel's steps |
| `G` | Graph editor (velocity, key, pan, shift per step) |
| `Alt`+click in the graph editor | Reset the step |
| Wheel over the title bar | Previous / next pattern |

## Piano roll

### Tools and editing

| Key | Action |
| --- | --- |
| `P` / `B` / `D` / `E` / `C` | Draw / Paint / Erase / Select / Chord stamp |
| `S` | Slide notes on / off |
| `Ctrl+A` | Select every note of the channel |
| `Shift+I` | Invert the selection |
| `Ctrl+D` | Deselect |
| `Esc` | Deselect |
| `Delete` | Delete the selected notes |
| `Ctrl+C` / `Ctrl+X` / `Ctrl+V` | Copy / cut / paste |
| `Ctrl+B` | Duplicate to the right |
| `Ctrl+Shift+X` | Move the selected notes to a new pattern |
| `Ctrl+Q` | Quantize note starts |
| `Ctrl+L` | Legato |
| `Ctrl+U` / `Alt+U` | Chop at the grid / open the chop tool |
| `Ctrl+G` | Glue touching notes of the same key |
| `Alt+A` | Arpeggiator |
| `Shift+↑` / `Shift+↓` | A semitone up / down |
| `Ctrl+↑` / `Ctrl+↓` | An octave up / down |
| `Shift+←` / `Shift+→` | One grid step |

With nothing selected, copy, duplicate, quantize, legato and the arrow keys take every note of the
channel; delete and cut only ever take a selection.

### Mouse

| Gesture | Action |
| --- | --- |
| Click (Draw) | New note, with the last length |
| Drag a note / its edge | Move / change length |
| Right-click a note | Delete it |
| `Ctrl`+drag | Select with a box |
| `Ctrl`+click a note | Add to / remove from the selection |
| `Ctrl`+click a piano key | Select every note of that pitch |
| `Shift`+drag notes | Clone them |
| `Shift` / `Ctrl` while dragging | Move in time only / in pitch only |
| Hold `Alt` | Off the grid |
| Double-click a note | Note properties |
| `Alt`+wheel on a note | Velocity (`Ctrl+Alt` = fine) |
| Drag in the lane under the grid | Velocity or pan; right-drag draws a ramp |
| Double right-click a ghost note | Switch to its channel |

### View

| Key or gesture | Action |
| --- | --- |
| `Page Up` / `Page Down` | Zoom in / out |
| `Shift+1` / `2` / `3` | 1 / 4 / 16 bars in view |
| `Shift+4` | Whole pattern |
| `Ctrl`+wheel | Zoom time |
| `Alt`+wheel on the empty grid | Key height |
| Middle-button drag | Pan |
| `Ctrl`+right-drag / `Ctrl`+right-click | Zoom to a box / previous view |
| Drag an end of the scrollbar | Zoom |

## Pitch editor

| Key or gesture | Action |
| --- | --- |
| `1`–`6` | Pitch, Drift, Formant, Volume, Split, Pencil |
| Space | Play the song (the vocal in its mix) |
| `F` | Fit the view |
| `Ctrl+A` | Select all notes |
| `Ctrl+C` | Copy notes for the Piano roll |
| `Delete` | Reset the selected notes |
| `Esc` | Deselect (again: close) |
| Drag a note (Pitch tool) | Move it; snaps to the scale, `Alt` = free |
| Double-click a note | Split it |
| `Ctrl`+wheel / wheel / `Shift`+wheel | Zoom / up and down / scroll |

The ruler works as in the Playlist.

## Mixer

| Key or gesture | Action |
| --- | --- |
| `F9` | Show / hide |
| Double-click the title bar | Maximize the mixer |
| `F2` | Rename the selected insert |
| `Shift+F9` | Add an effect to the selected insert |
| `Ctrl+L` / `Shift+Ctrl+L` | Route the selected channels here / from here on |
| Double-click a fader | 0 dB |
| Double-click a pan knob | Centre |
| Right-click a fader or knob | Automation menu |
| Right-click **M** or `Ctrl`+click it | Solo |
| Double-click a strip name | Rename |
| Right-click a strip | Strip menu (send, sidechain, FX chains) |
| Drag an effect up / down | Change the order |
| Drag an effect onto another strip | Move it there (`Ctrl`: copy) |
| Drag the dot under a strip (maximized) | Send to another strip |
| Drag or wheel a cable | Send level |

## Knobs and controls

| Gesture | Action |
| --- | --- |
| Drag up / down | Change the value |
| `Ctrl`+drag (`Shift` on some small knobs) | Fine |
| Double-click or `Alt`+click | Default value |
| Wheel | Step the value |
| Right-click | Menu: type a value, automate, reset |

## Browser

| Key | Action |
| --- | --- |
| Click a sample | Preview from the top |
| `↑` / `↓` | Next / previous, with preview |
| `Enter` | Add as a sampler channel |
| `Esc` | Stop the preview (in the search box: clear it) |
| `Ctrl`+click / `Shift`+click a pattern | Add to the selection / select a range |

## Notebook

| Key | Action |
| --- | --- |
| `F8` | Open / close |
| `Esc` | Leave editing |
| `Alt+Z` | Wrap long lines |
| `Ctrl`+wheel | Font size |
| Click a bar number | Move the playhead there |
| `Ctrl+Z` / `Ctrl+S` | Undo / save (the project's, not the text box's) |

## See also

- [The basics](the-basics.md), [Making beats](making-beats.md), [Arranging](arranging.md).
