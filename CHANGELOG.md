# Changelog

What changed in each release of Synkopa (called WAVE up to 0.9.21). The newest version comes first; its section is
shown in the app after an update and is published as the release notes (see
`docs/releasing.md`). Write for musicians: what you can do now, not how it works inside.

## Unreleased

### Added
- **A new synth sound bank and a more Serum-like synth.** 50 new factory presets (808s, Reese,
  growl, FM and sync basses, acid, future and progressive plucks, supersaw and creamy mono leads,
  evolving pads, choir, FM keys and bells, organ, strings, flute, arps, lo-fi, risers and impacts).
  Six new wavetables you scan with Position (Basic, Formant, Harmonics, Sync, Growl, Comb), a Warp
  per oscillator (Sync, Bend, Asym, FM from the other oscillator), four Macros named after what they
  drive (nearly every preset puts its main moves on them), eight modulation slots and a Rise per LFO for
  delayed vibrato. The oscillator picture now shows the real wave. Old projects keep their sound;
  the old preset names are gone from the list.
- **Find a command (Ctrl+K).** Type what you want to do; every menu command is there with its
  shortcut, what you have selected comes first, and a question that is not a command goes to Ask AI.
- **Pan per note.** The Piano roll's lower lane switches between Velocity and Pan; drag to pan
  notes left and right, or type it in a note's properties. Plays on sampler, Synth and Synkopa Keys
  channels (not on plugins).
- **Stereo.** Width (0 % mono … 200 %), Spread to widen even a mono synth or vocal without any
  loss when the song is summed to mono, Bass mono to keep everything under 120 Hz (or your
  frequency) in the centre for clubs and phones, gain and balance; polarity and L/R swap under
  Advanced. Its window shows the stereo field as a vectorscope with the correlation, the balance,
  and whether your bass is mono.
- **Multiband (OTT).** Upward and downward compression in three bands at once: Depth, Time,
  Upward and Downward are all you need. Each band's level and how far it is lifted or pulled down
  is drawn live; drag the threshold lines and the crossovers right in the display. Quiet room
  noise is not pulled up into hiss, and Depth blends without the phasey dip a plain dry/wet has.
- **Analyzer.** A meter for the master: short-term, integrated and momentary loudness (LUFS, to
  the EBU R128 standard), loudness range, true peak, the loudness of the last 30 s against a
  target, a spectrum whose bass is sharp enough to tell a 41 Hz kick from a 49 Hz 808 (hover it
  to read the frequency and note), a vectorscope and the correlation. It tells you how much
  streaming would turn the song down instead of telling you to hit a number.
- **Slice mode for the sampler (chop a sample).** A new Slice page in the sampler's window cuts the
  sample at its hits (Transients, with a Sensitivity knob that answers instantly) or into 4, 8, 16
  or 32 equal parts (MPC chop). Click a slice to hear it, drag a marker to move it, double-click to
  add or remove one. Each key from the root plays one slice at the sample's own pitch, and a new
  slice cuts the one sounding. Dump to pattern writes one note per slice in the original timing,
  so the pattern plays the sample back as it was at any tempo: move the notes to rechop it.
- **Ten new Reverb presets:** rap vocal (short room), R&B hall, bright pop plate, snare plate,
  warm keys hall, EDM lead hall, ambient pad haze, lo-fi dark room, an 18-second "cosmos", and a
  100 % wet plate for a send. The old presets keep their places.

### Changed
- **Updates download by themselves.** A new version is fetched in the background while you work;
  when it is ready, "Restart to update" appears under the menus and one click restarts into it
  (after asking about unsaved changes). Turn it off in Settings → General → Updates.
- **A roomier What's new.** After an update that skipped versions, the notes of each one open in
  a larger window with the versions listed beside them; the text is bigger and Added, Changed and
  Fixed are coloured.
- **A calmer look.** Panels, mixer channels, windows and menus are told apart by their tone and a
  soft shadow instead of outlines; floating windows stand a step off the app behind them. Buttons
  and fields are filled blocks, so what can be clicked reads at a glance.
- **Piano roll bar, rebuilt.** Tools as one switch, Snap as a chip (with Auto), the song's key shown
  as a badge with its scale on a small keyboard, and Humanize and the other note tools behind
  "Tools" with icons and sliders. The Playlist bar is built from the same pieces.
- **Playlist.** Track names are shown in full (the mixer insert sits beside the volume). Automation
  clips read as curves. Audio clips show stretch, pitch and gain only when selected; a small dot
  marks a clip that has any. A muted track's M stays lit while the rest of the track fades.
- **Mixer.** A dB scale on the faders, the Master set apart, a grip on effect rows, and a "Route
  selected here" button when Channel rack channels are selected.
- **The effect picker opens in the middle of the app** instead of beside the pointer.
- **Dim labels are easier to read** in the dark theme.
- **Effect windows are tidier.** Each group of controls sits in its own frame, packed closer, and
  the how-to line that took the bottom of every window now sits behind a ? in the title bar.
- **A clearer Reverb window.** The picture now shows what you hear: how long each frequency rings
  (bass, middle and treble decay as one curve), with the tail's own spectrum live behind it and
  the low and high cut shaded where the tail is filtered away. Drag the three dots up and down
  and the cut lines sideways; below, the tail in time with the pre-delay to drag and the output
  after the last hit.

## 0.9.26 — 2026-11-19

### Added
- **Extract stems from an audio clip.** Click the clip's menu → Extract stems…, tick drums, bass,
  vocals and/or the rest, and each stem lands on a new track right below the clip, in the same
  place and with the same length, stretch, pitch, fades and gain; the clip is muted, and one Ctrl+Z
  takes it all back. It runs in the background (about 25 s per minute of audio on a 6-core CPU) with
  progress and Cancel, so you can keep working. The separation model (HTDemucs, 90 MB, MIT licence)
  is downloaded the first time. Also `synkopa stems --clip ID`.
- **An underrun counter and a CPU meter.** Settings → Audio counts, next to Buffer, every time
  the output ran dry — a buffer rendered too late, or a dropout the driver reports — since a larger
  buffer is the cure; Reset starts a fresh count. Turn on "CPU meter in toolbar" there for FL
  Studio's CPU meter: how much of each audio buffer the mix takes to render (near 100 % the output
  crackles); it flashes red on an underrun and keeps the count until you click it.
- **Ducking on the Delay.** A Ducking knob (0–12 dB, as on the Reverb) pulls the echoes under the
  dry sound and lets them bloom back in the gaps; the delay window shows how much it is ducking.
- **AIFF samples.** `.aif` and `.aiff` files load, preview and show in the Library.

### Changed
- **Reverb decay reaches 20 s** (was 6 s). Saved reverbs keep their decay.
- **Export in place keeps effect tails.** The rendered clip runs on until the reverb or echo has
  died away (up to 30 s) and lands on its own new track, so the tail is not cut at the selection's end.
- **Dock guide previews show exactly where the panel lands.** The blue preview now covers the
  area the dropped Channel Rack, Piano roll or Mixer will really take, at its docked size.
- **Dragging a clip keeps its offset from the grid.** Moving or Shift-copying a clip placed between
  grid lines snaps the distance moved, as in FL, instead of jumping the clip onto a line.
- **Unnamed lanes show their clips' icon.** A playlist lane with no icon and a name that says
  nothing ("Track 7") shows the most common icon of its clips; a pattern that plays one channel
  shows that channel's icon.

### Fixed
- A single dropout reported by the audio driver no longer restarts the audio device; it is counted
  as an underrun instead.
- Samples with "Trap" or "Chord" in their name no longer get the vocal icon; Standard library
  samples take the icon of their library group.
- Zooming the Playlist with Ctrl+wheel no longer jumps sideways and back on Linux.
- Mixer effect rows no longer flicker at one exact mixer height.
- A waveform that failed to load is retried instead of staying empty.

## 0.9.25 — 2026-11-18

### Added
- **Trap hats, snares and claps in the Standard library.** Nine closed hats and three open hats
  (Trap Hat Crisp, Tight, Metal…, Trap Open Hat Smooth…), fourteen snares (Trap Snare Punch, Hard,
  Rage, Low, Clap…) and twelve claps (Trap Clap Thick, Warm, Gritty, Click…), chosen to sit with
  the trap kicks and 808s. The library also takes 2 MB less space, with the same sound.
- **A clip light for the Master.** A meter next to Main volume in the toolbar, and a cap above the
  Master strip's meter in the Mixer, turn solid red when the output goes over 0 dBFS and show the
  highest over (CLIP +5.0) until you click them. The tooltip says where it first clipped.
- **Fixed velocity for the MIDI keyboard.** Settings → MIDI → Velocity: "Fixed" plays and
  records every key at one velocity (100 by default, any 1–127) however hard it is pressed —
  for keyboards without velocity or when you do not want it. "From keyboard" is the default.

### Changed
- **What you hear clips where the export does.** A mix over 0 dBFS used to sound clean in the app
  (Windows hid the overs) and distort in the exported MP3. Playback now clips at full scale too,
  so you hear the problem before you export.
- **The splitter between two docked bottom panels trades their sizes.** With the Playlist already
  as small as it goes, dragging the splitter between the docked Channel rack and Piano roll now
  shrinks the panels in between, nearest first, as in FL. A short Piano roll hides its velocity
  lane so the notes keep the room.

## 0.9.24 — 2026-11-18

### Added
- **Trap kicks and an 808 section in the Standard library.** Fourteen new trap kicks (Brick, Clean,
  Clip, Dark, Hard, Heavy, Knock, Phonk, Punch, Rack, Stomp, Tight, Top…) and fifteen new 808s
  (Zay Warm, Opium Growl, Drill Glide, Sub Pure, Plugg Short…) in a Browser section of their own
  between Percussion and Tonal. Every 808 is pitched at its measured root key, so they swap and layer
  on the same notes.
- **Double-click an empty mixer box to give a channel its own insert** (as in FL). On a channel
  with no insert ("—") it routes the channel to the first unused mixer insert, else a new one,
  named and coloured after the channel, in one undo step. An insert you muted, renamed, set up or
  automate is never taken. The channel and sample windows' MIXER list has the same as
  *+ Free insert*.
- **The Playlist's vertical scrollbar zooms the lanes**, as in FL: drag the thumb to scroll, drag
  either end to make every lane taller or shorter, Ctrl+wheel over it to zoom.
- **Play says why there is no sound.** When the audio output failed to open (a headset driver
  error, an unplugged interface), pressing Play or playing a pattern shows a notice with the
  reason, **Try again** (reopens the device) and **Audio settings**, instead of silently not
  starting.
- **Auto vocal from the command line.** `synkopa auto-vocal --clip N` measures a vocal take and puts
  the tuned chain on its mixer insert in one call — the same chain the app's Auto vocal button
  builds (pitch correction, vocal rider rendered into the audio, compressors, de-esser, reverb,
  limiter), with `--style hard` for rap tuning, `--beat-clip` to take key and level from the
  beat, and `--plan` to only print the proposal. It works without the app open
  (`synkopa -p song.synkopa …`), so scripts and agents can tune vocals too. `synkopa f0-model --install`
  downloads the RMVPE pitch model (after printing its licence note) and switches the pitch
  detector to it.
- **Add a playlist sound to the Channel Rack from its clip.** Dragging a sample into the Playlist
  plays it there directly (as before, without a rack entry); the audio clip's menu now offers
  "Add as sampler channel" to put the same sound on the rack and the keyboard in one click. The
  item is disabled when the rack already has a channel playing that sample.

### Changed
- **Lighter dock guides.** Moving a floating Channel Rack, Piano roll or Mixer no longer covers
  the workspace with full-size guides: every dock slot shows a small, faint arrow, and the one
  under the pointer lights up with the area the panel would take. A drop anywhere else keeps it
  floating. The dock button, the title-bar right-click menu and View → Window layout dock as before.
- **Less CPU while nothing plays.** Silent tracks and mixer inserts no longer cost anything in
  each audio block: a stopped project uses almost no CPU, and big songs play with more headroom
  (fewer crackles at small buffer sizes).
- **The loading screen shows the Synkopa logo** above the progress bar.

### Fixed
- **Type in value works on pan and EQ Q.** A centred pan (shown "C") and an EQ band's Q accept a
  typed value instead of greying the item out.
- **A sample dropped on the Channel Rack is undone with its channel.** One Ctrl+Z removes both,
  and the file no longer stays in the project's audio list. A file loaded during a knob drag no
  longer makes Ctrl+Z stop half-way through the drag.
- **Lane heights stay with their track.** Undoing a Move up / down no longer hands a resized
  lane's height to the other track, and undoing a rename keeps the height you gave the lane.
- **Plugin parameter sliders reset on Alt+click**, as their menu says.
- **Paint:** a fast diagonal sweep paints every lane it crosses instead of filling the last one.
- **A folder removed from the Browser** forgets that it was open and is not read again.
- **Zooming no longer fights the playhead.** With Follow on and the song playing, every zoom
  step (Ctrl+wheel, Page Up/Down, the presets, a zoom box, the scrollbar) now pauses following
  instead of letting the view get yanked between the zoom's anchor and the playhead — the
  left-and-right stutter while zooming is gone, in the Playlist and the Piano roll.
- **Closing the pitch editor returns to the sampler.** The pitch editor of an audio clip used to
  close the clip's editor window when it opened, so closing the pitch editor left nothing. The
  editor now stays open underneath, and Esc closes the pitch editor first, then the editor —
  also stopping a solo preview the pitch editor left ringing.

## 0.9.23 — 2026-11-18

### Added
- **Export a pattern on its own, as in FL Studio.** The Export dialog has a Pattern range next to
  Song and Loop: it renders the current pattern the way PAT mode plays it, with an empty
  Playlist too, and leaves the project as it is. It is the default in PAT mode and while the
  Playlist is empty. Choosing Song with an empty Playlist now warns that the file would be
  silence and offers to export the pattern instead (it used to write a silent file). Mix and
  stems both work; the command line has `render --pattern`.
- **Controls work like FL.** Ctrl+drag adjusts finely, Alt+click resets, and a control's
  right-click menu has *Type in value*. Grabbing a knob or fader again right after a drag no
  longer resets it.
- **Mixer: select several tracks** with Ctrl+Shift+click; their faders and mutes move together.
  A right-click (or Ctrl/Alt+click) on a mute switch solos, in the Mixer and on Playlist tracks.
- **Playlist:** a slice dragged up or down cuts every clip it crosses; the paint tool paints
  wherever the pointer goes, also leftwards and on other tracks; tracks can be moved up and down
  (their clips and automation follow).
- **Piano roll:** Shift+Up/Down moves notes a semitone, Ctrl+Up/Down an octave, Shift+Left/Right
  a grid step; Ctrl+V pastes where the notes were copied; with nothing selected, Ctrl+B / Ctrl+C
  and the arrows take every note; a note sounds as long as you hold it, at its velocity;
  right-drag in the velocity lane draws a ramp.
- **FL's shortcuts:** Ctrl+D deselects, Ctrl+M switches the metronome, Alt+M mutes the selected
  clips, Ctrl+N saves a new version beside the project (Beat → Beat_2). New project is
  Ctrl+Shift+N. Export in place is in the File menu (Ctrl+Alt+E).
- **Synkopa remembers where you were:** a saved project reopens on its pattern, channel and start
  marker, and the Browser keeps its open folders and scroll position.

### Changed
- **The autosave never writes over your project.** It used to save a saved project into its own
  file every few minutes, so the version you saved yourself could be lost. Now it keeps a
  recovery copy of the unsaved changes instead. If Synkopa closes unexpectedly, the next start
  offers them back as that project, unsaved: save to keep them, or close without saving to keep
  the file as you left it.
- **A new project's Kick, Clap, Hat and Snare play into mixer inserts 1–4**, named after them,
  like FL Studio's template, so a fader or effect there works on the drum straight away.
- **The keyboard goes where you work.** A click in the Playlist takes the keyboard back from the
  Browser's search box, so Space plays. Delete, Ctrl+X/C/V and the arrows edit clips only while
  you work in the Playlist, never behind the Mixer or an editor.
- **PAT mode plays the pattern through its channels' own inserts**, not through the effects of
  the Playlist track its clip sits on. A press on the Playlist grid switches to SONG.
- **Clicks follow FL:** the Channel Rack's left button lights steps and the right one clears
  them; Up/Down pick the channel; two clicks on a channel name open and close its window. A
  library sample plays on the press, from the top each time. Clicking the Browser or a docked
  panel sends floating windows behind the workspace (F5 brings them back), one at a time.
- **Ruler:** a click sets the start position, a drag draws the loop, Ctrl+drag moves the start
  position.
- **Lane heights are not undo steps** any more, and a right-click on empty Playlist space only
  drops the selection.
- **A new logo:** six staggered bars.

### Fixed
- Clicking the Mixer no longer hides open effect windows or the floating Mixer.
- Piano roll: dragging an off-grid note up or down keeps its timing; changing the grid keeps the
  note length; Delete and Ctrl+X only remove a selection; a click on empty grid deselects; a fast
  velocity drag no longer skips notes. Notes drawn after a Channel Rack step stay steps, and
  triplet hats stay steps in the rack.
- Trimming a clip shorter than the grid no longer makes it longer.
- Dropping samples on the Channel Rack is one undo step.
- A click on the Playlist while playing no longer stops the view from following the playhead;
  zooming no longer makes the view stutter between the zoom point and the playhead.
- The sample editor stays open under its pitch editor; an audio clip's menu can add its sample
  as a sampler channel.
- Touching the sampler's envelope knobs no longer switches the envelope on by itself.

## 0.9.22 — 2026-11-17

### Added
- **One sample editor with a keyboard, as in FL Studio.** The editor of an audio clip and of a
  Channel Rack sampler now look and work the same: both end in a piano keyboard. Click a key
  to hear the sample at that note; right-click a key to make it the root note (the note at
  which the sample plays at its own pitch). The root key is lit and named, and the root shows
  above the keyboard. An audio clip's root belongs to its sample (every clip of it shares it),
  is saved with the project, and a sampler channel made from that sample starts on it. The
  keyboards of Synkopa Keys and plugin channels set their root the same way.
- **A sampler's volume envelope, as in FL Studio.** A sampler's editor has two pages: Sample
  (the waveform, its part, pitch, level and the sample's own fades) and Envelope, the FL
  Channel settings ▸ Volume page. Switch the envelope on and every note gets delay, attack,
  hold, decay, sustain and release, drawn as a graph: drag a point sideways to change its time,
  the sustain point up and down for the level, and the dot in a curve up or down to bend the
  attack, decay or release. The same values are on knobs, and each drag is one undo step.
  Dragging a point or turning a knob switches the envelope on. With it on, a note fades over
  the release from its note-off: a key let go or the end of a Piano Roll note. Off, the sample
  plays as recorded. A Channel Rack step still has no note-off and holds the sustain.
- **The first start offers the demo project.** A fresh install asks once whether to open
  OBSIDIAN 2093 or start empty. It does not ask after an update, or when Synkopa was started by
  opening a project file.

### Changed
- **WAVE is now called Synkopa**, with a new logo. Nothing of yours moves: settings, the library,
  recordings and presets stay where they are, and the update replaces the WAVE install (its
  shortcuts become Synkopa shortcuts).
- **Projects are saved as `.synkopa`.** Every `.wave` project still opens, by double-click too, and
  keeps its name until you save it under a new one.
- **The command line is `synkopa`.** The old `wave` command keeps working, so existing scripts
  need no change.
- **The sampler's One-shot / Gate switch is now the Envelope switch.** Envelope off plays the
  sample as recorded, as One-shot did, and no longer applies attack, decay and sustain: those
  belong to the envelope. Envelope on works like Gate, and a sized Piano Roll note now fades over
  the release from its end instead of stopping there. Projects keep their sound: a sampler that
  was in Gate mode or had an attack or a sustain below 100 % opens with its envelope on.
- **The demo song OBSIDIAN 2093 has a reworked arrangement and mix**: extra vocal and pad
  layers, an 808 cymbal, a new 808 turnaround, a Warp move on the dark vocal and tighter filter
  and reverb transitions.

## 0.9.21 — 2026-11-17

### Added
- **A new demo song: OBSIDIAN 2093**, a 40-bar modern trap track (136 BPM, D# minor) with a
  punchy kick, a warm 808, melodic hooks and filter sweeps automated on a bus. Open it from
  File → Open demo project, or pick it in Settings → General. Six new free (CC0) sounds come
  with it in the standard library: an 808 spin, a crisp trap clap and snare, and three loops.

### Fixed
- **Linux: WAVE starts on Ubuntu 22.04 and Debian 12.** The packages and the AppImage needed a
  newer system library than those systems have and quit at once; they now run on any
  distribution from Ubuntu 22.04 on.
- **The Playlist no longer goes blank after a scroll.** Scrolling, or dragging a clip past the
  edge so the view scrolls with it, could leave the Playlist empty but for a strip at its edge
  (only fades showed) until something else made it redraw. It now always shows every clip and
  the grid.

## 0.9.20 — 2026-11-17

### Added
- **Mixer routing, as in FL Studio.** A mixer insert can send its sound to other inserts: group
  drums into a bus, or send the vocal and the keys to one shared reverb. In the maximized mixer,
  cables under the strips show every send with its level: drag the dot under a strip onto
  another strip to add one, drag (or scroll) a cable's level to change it, double-click for
  100 %, right-click to remove it. The **M** under a strip switches its output into the Master;
  off, it goes only where it sends. Docked, right-click a strip for *Send to* and *Output to
  Master*. A send that would loop back is refused (the strip turns red). Solo follows the
  routing: soloing a bus keeps what feeds it. Plugin delay compensation lines up every path,
  and stems include what is routed into a bus. In the CLI: `wave route` and `wave unroute`.
- **Effects in the mixer strips.** When the mixer is taller, each strip lists its effects
  above the knobs (icon, name, on/bypass dot; click to open), as many as fit; the strip
  itself keeps its size. In the maximized mixer there is always room for five effects, and the faders take the rest of the height.
- **Arrange the windows the way you work.** The Channel rack, the Piano roll and the Mixer can
  each dock to the left, the right or the bottom of the main window, or float. Drag a window's
  title bar and dock guides appear (as in Visual Studio): drop it on an edge, beside the
  Playlist, or on another docked panel's guide to put the two side by side or one above the
  other. Drag a docked panel's title bar to pull it off. Right-click a title bar, or use
  View → Window layout, to dock it without dragging. Every splitter resizes, the Playlist always
  keeps room, and the layout is remembered. View → Window layout → Reset puts it back.
- **Three new themes**: *Dim*, a mid-grey dark theme, and *Fog* (cool) and *Sand* (warm), two
  mid-grey light themes for when white is too bright.
- **Pin a plugin's window on top.** A plugin editor's header has a pin next to the gear: click
  it and the window stays in front of WAVE's, so an instrument you are playing stays in sight
  while you click in the arrangement. The choice is remembered per plugin, and the pin lights
  up while it holds.
- **When WAVE died, the suspects are named.** If the last session ended without exiting (a
  plugin crashed it, or the machine did), the next start lists the plugins that were loaded
  and offers, per plugin, to keep it out of future scans. A normal exit clears the list on its
  own.
- **A plugin that crashes no longer takes WAVE down when you load it.** The first time you use a
  plugin, WAVE tries it in a separate, throw-away process: it creates it, plays sound (or notes)
  through it, saves and restores its settings and closes it. If that test process crashes or
  hangs, the plugin is not loaded into WAVE: its slot says why, and **Load anyway** loads it if
  you want to take the risk. Plugins that pass load straight away from then on (an updated
  plugin is tried again).
- **Zoom much further into the Playlist**, down to single 16ths spread across the screen, for
  detailed drum and edit work. The automatic snap follows down to 1/16 of a beat, and the snap
  menu has 1/8 and 1/16 beat.

### Fixed
- **Linux: plugin windows stay in front of WAVE.** Under Wayland, clicking anywhere in WAVE sent
  plugin windows (Vitalium, Surge XT, …) behind it. WAVE now runs through XWayland there, as
  other Linux DAWs do, so plugin windows stay above it (`WAVE_WAYLAND=1` keeps native Wayland).
- **Linux: LV2 plugins built with JUCE show their own window** (Surge XT, Dexed and others
  needed the `instance-access` feature; they showed only WAVE's parameter list).
- **Mono VST3 effects load.** Effects with a mono input or output (most ZAM plugins, several
  DISTRHO ones) were refused as "not a stereo effect"; they now take the sum of both sides and
  play on both.
- **A plugin that outputs garbage cannot ruin the mix.** An effect that produces invalid samples
  (NaN or infinity, as ZamEQ2 does) is silenced instead of breaking every effect after it and the
  master.

### Changed
- **The Library's sections collapse.** Standard library, Kits to download and Folders each get
  a chevron: click the heading to fold it away and leave your own folders in sight. A search
  still looks everywhere, and what you folded stays folded after a restart.

- **Redesigned Settings.** A bigger window with an icon sidebar (General, Look, Audio, MIDI and
  a new Updates tab), settings grouped into cards, and each option's explanation shown under its
  name instead of hidden in a tooltip. Themes are shown as small previews of the app, grouped
  into dark and light.
- **New channels get their own colour.** Each channel you add takes the next colour from a
  wider palette (blue, amber, green, pink, violet …), so its steps and piano-roll notes stand
  apart from the other channels' instead of all being the same purple. The colour menu offers
  the new colours too.

## 0.9.19 — 2026-11-16

### Added
- **Triplet grids in the Piano roll.** The Grid list has 1/6, 1/12, 1/24 and 1/48 (quarter,
  eighth, 16th and 32nd triplets) next to the straight values, which now run from a whole bar
  down to 1/64. Drawing, moving, resizing and Ctrl+Q quantize follow it, and the roll draws its
  lines. Picking a grid also sets the next drawn note to one grid step (at most a beat).
- **The Mixer shows what each strip carries.** A docked strip shows its effect count (● 3), and an
  insert you have not named shows, quietly after its number, what is routed to it ("2 Hats").

### Changed
- **The browser has three tabs: Patterns, Audio and Library.** Audio lists your own sounds
  (imports, recordings, audio with clips in the Playlist); the starter kit samples the Channel
  Rack plays stay in the rack, so a new project's Audio tab is empty.
- **Channel Rack steps keep their width.** Resizing the window shows more or fewer steps (the
  grid scrolls) instead of stretching or squeezing them.
- **"Automate on the playlist" uses a track that is free where the clip goes**, not only a track
  empty along the whole song; a new track is added at the bottom only when every track is busy
  there. The Playlist scrolls to the new clip.
- **Clip edges and automation points are easier to grab.** A clip's edge trims from a little
  inside it and from just outside it; automation points are bigger, and the first and last
  point sit whole inside the clip instead of half cut off by its border.
- **A right-click on empty Playlist space drops the selection** (the menu still opens).
- **The Channel Rack and the Piano roll share one window frame.** Each title bar ends with the
  same buttons in the same order: dock or float, maximize (Piano roll), close. The Piano roll's
  maximize was only a double-click on its title bar; it now has a button too.
- **Esc closes the window in front**, whichever kind it is. Before, the sample editor and the
  channel settings closed first even behind a Channel Rack or Piano roll you were working in.
- **F5 (or a click on the Playlist) tucks away every floating window**, the sample editor, the
  channel settings and the pitch editor included; F5 again brings them all back.
- **The Mixer is a window like the Channel Rack and the Piano roll.** It has the same title bar:
  float it as a window or dock it back at the bottom, maximize it over the whole workspace
  (or double-click its title bar), and × hides it. **F9 shows and hides the Mixer**, as in FL
  Studio; floating behind other windows, F9 brings it to the front first. Esc hides it when it
  is the window in front. The View menu has *Dock Mixer at the bottom* and *Maximize mixer*.
  The separate Mix view with its Back button is gone: maximize does the same.

### Fixed
- A channel's window opened from the Channel Rack (a click on its name) appeared behind the
  rack, as did the Piano roll opened by a double-click. Every window now comes to the front
  before what was clicked inside it opens another.

## 0.9.18 — 2026-11-16

### Added
- **Chord tools in the Piano roll.** New **Chord stamp** tool (C): pick a chord type (18, from
  major and minor through 9ths to power chords) and an inversion, then click the grid — the
  chord is written with the clicked key as its root; clicking the same spot again replaces it,
  and right-click removes the whole chord. The roll ghosts the chord under the pointer and its
  status bar names it. In the **Tools** strip, *Chord* harmonizes selected single notes into
  chords (diatonic in the project key, or a named type spelled by interval), and *Progression*
  generates one chord per bar from templates like Pop, the minor loop, Trap, Andalusian and an
  8-bar canon — or a seeded *Chance* walk — with optional **Voice-lead** that picks inversions
  so the chords move as little as possible. The same verbs work headless: `wave notes stamp`,
  `wave notes chord` and `wave notes progression` (with `--degrees "0:5:2:6"` for arbitrary
  chord sequences).
- **Plugin windows open straight away, with WAVE's header on top.** Click a plugin instrument
  in the Channel Rack or a plugin effect in the mixer and its own interface opens at once, like
  in FL Studio: a slim WAVE bar on top shows the name, a gear opens WAVE's settings for it
  (parameters, volume, pan, routing) and × closes it. Click the instrument again to close it.
- **Plugin windows on Linux.** VST3 plugins (such as Vitalium and the DISTRHO and ZAM
  plugins) and LV2 plugins with their own interface (such as x42, ZAM and Dragonfly) now show
  their windows on Linux too, also under Wayland through XWayland. LV2 plugins whose interface
  needs GTK or Qt keep WAVE's parameter list.
- **Pattern previews in the Project tab.** Each pattern in the list shows its notes in
  miniature: drum rows and melodic lines laid out like its clips in the Playlist. Its icon
  comes from the instruments it plays, so the patterns made by *Split by channel* read at a
  glance.
- **Playback uses all your CPU cores.** Synths and mixer effect chains are processed in
  parallel while the song plays, like FL Studio's multithreaded processing. A song the size
  of an FL demo project takes about a third of the time per audio buffer that it took before,
  which leaves room against dropouts while you edit. The sound is bit-for-bit the same.
- **Metronome, count-in, input monitoring and the computer keyboard as piano** are in the
  View menu too.
- **The Channel Rack's mixer box works like the one in the sample editor.** Scroll over it to
  route the channel to the next or previous insert, and double-click it to show that insert in
  the mixer. A single click still opens the list.

### Changed
- **The toolbar fits any window size.** On a smaller window the least needed controls step
  aside one by one: the loop readout, the BPM label, count-in and monitoring, the metronome,
  then the panel buttons. Each is still in the View menu or on its key. Nothing is cut off at
  any width.
- **Recording works like FL Studio: nothing to arm.** Press R and Space: the recording goes to the
  first track that has room from the playhead on, or to a new track when every track is in use
  (undo removes that track with the take). The record button on every track is gone; a track's
  menu still has **Record into this track** for recording more takes over what it holds. While
  Record is on, the toolbar shows where the recording will go.
- **Right-click Record to choose what it records: Audio, Notes or both.** In PAT mode, with Notes
  ticked and a MIDI keyboard connected, Record + Space loops the current pattern and adds what you
  play to it, one undo step per recording. Other recordings run the song, and the PAT/SONG switch
  changes to SONG so it shows what plays. With only Notes ticked, the microphone stays closed.
- **Record notes from the computer keyboard** (Ctrl+T, also in the Record menu), with no MIDI
  keyboard needed, and **quantize recorded notes** to 1/16, 1/8 or 1/4 (Record menu → Quantize
  notes). Starts snap to the grid and lengths stay as played.
- **After a recording, a notice says what landed where**, for example "Recorded Take 3 on Pad",
  with **Delete**, and **Show takes** when the recording added takes.

### Fixed
- **The metronome clicks in PAT mode.** It was silent while a pattern looped and seemed to start
  only after switching modes.
- **PAT/SONG always shows what plays.** In SONG mode with an empty playlist, Space played the
  pattern while SONG stayed lit. The switch now changes to PAT, and a notice offers **Add to
  playlist**, which puts the pattern at bar 1. A quick switch between modes no longer flips back.
- **The sample editor no longer sticks over plugin windows.** Clicking an effect or plugin window
  now brings it in front of the sample editor (and the channel settings window); clicking the
  editor brings it back on top.
- DISTRHO-based plugins (Vitalium and others) can now talk between their engine and their
  interface: knobs and displays stay in step.
- **Browsing big sample folders works again.** In a folder with many groups, opening a group
  further down the list could show only a "search to narrow the list" note instead of its
  files. The library list is now virtualized: it draws only the rows on screen, so any folder
  browses fully whatever its size, and a folder beyond the 5000-file walk limit says so
  instead of ending silently.
- **Starting the song while a pattern loops with Record on** now ends the pattern recording and
  plays the song. Before, the song did not start until you stopped first.
- **Changing the pattern loop range while recording in PAT mode** keeps your notes where you
  played them. Before, notes played after the change could land on the wrong steps.
- **Quantized notes stay on the grid on every pass of the loop**, also when the loop is not a
  whole number of grid steps long.
- A pattern loop can no longer start during a notes-only song recording, which left the
  recording stuck until Stop.
- **Linux and macOS: fewer dropouts with multi-core playback.** The audio worker threads now run
  at real-time priority like the audio callback, where the system allows it.

## 0.9.17 — 2026-11-15

### Added
- **Trap sounds and melodic loops in the library.** 38 new sounds you can use freely: five
  808s tuned to their root, kicks, snares, claps, open and closed hats, percussion, impacts,
  risers and sub drops, and a new Loops group of ten keys, piano, pluck, guitar, pad and flute
  loops with their tempo and key in the name.
- **Drop a sample from anywhere onto the Channel Rack.** Drag a file from Explorer or your web
  browser, or a sample from Browser ▸ Project ▸ Audio: on a channel's edge it becomes a new
  sampler there, on its middle it replaces the sample, below the channels it is added at the
  end. The channel it lands on lights up while you drag.
- **Export in place.** Right-click a clip (or press Ctrl+Alt+E) to render the selected clips,
  with their mixer effects, into one audio clip on a new track right below them; the originals
  are muted, and Ctrl+Z undoes it all.
- **Drag effects between mixer channels.** Drag an effect onto another mixer strip to move it
  there (with Ctrl to copy it), or drag the effect list's header to move the whole chain.
- **Stop twice to silence everything.** A second Stop (or Ctrl+H) cuts reverb and delay tails,
  held notes and the preview at once, without a click.
- **Windows at a click.** Buttons for the Browser, the Channel Rack and the Piano roll sit in the
  toolbar and light up when open. Click the pattern name in the toolbar, or a pattern in the
  Browser, to see it in the Channel Rack; switching patterns shows a closed rack.
- **Double-click to jump to the mixer.** Double-click the header of a sample or channel window,
  or a track's icon, and the mixer opens on its channel.
- **Double-click a sample in Browser ▸ Project ▸ Audio to hear it.**
- **Live coding from a text file: `wave watch song.txt`.** Write the song as `wave` commands
  in a file (tempo, step patterns, effects…), start the loop and save: the song changes while
  it plays, every time you save. The file is the song: remove a line and its effect is gone.
  Each save is one undo step; a mistake in the file changes nothing and the terminal names the
  line; an edit you make in WAVE meanwhile is not overwritten. Without the app,
  `wave -p song.wave watch song.txt --render preview.wav` bounces a preview on every save.

### Changed
- **A sample's sound is shared by all its clips**, as in FL Studio: volume, pan, pitch,
  normalize, reverse and the mixer channel set in the sample window change every clip of that
  sample. "Make unique" splits one off.
- **The mixer channel set in a sample window belongs to that sample**, not to the whole
  playlist track (the track's own routing is still in the track menu).
- **The song loops as soon as the last clip ends**: an audio file a hair longer than its bars
  no longer adds an empty bar.
- Track numbers are gone from the playlist headers.

### Fixed
- **Notes you play are heard at once** while the song is stopped. With a long look-ahead effect
  such as the Vocal Rider in the mix, a plugin instrument could sound up to 1.6 s late.
- **"Open plugin GUI" opens the plugin's window in front of WAVE.** It could open behind it and
  look like nothing happened; the button now also follows the window when you close it, and a
  plugin with no window of its own (WAVE Demo Synth) says so.
- Hosted VST3 plugins can no longer repeat their previous block of audio, and effects no longer
  do extra work on the audio thread.

## 0.9.16 — 2026-11-15

### Added
- **WAVE Demo Synth comes with WAVE.** A small VST3 synth (saw, square or triangle through a
  resonant filter, with an envelope) is in the Channel Rack's "+" menu under "Plugin
  instruments (VST3)" right after installing, so you can try plugin instruments before you own
  any. The mod wheel opens its filter.
- **Play your VST3 instruments in the Channel Rack.** Serum, Analog Lab, Vital and the other
  synths you have installed are listed under "Plugin instruments (VST3)" in the Channel Rack's
  "+" and "Replace" menus. Draw notes for them in the Piano roll or the step sequencer, play
  them from your MIDI keyboard (sustain, pitch bend and mod wheel included), open their own
  window from the channel's settings, and export the song with them. Your sound is saved with
  the project. A plugin that is missing on another computer keeps the channel and its notes
  and is marked in the rack.
- **Ask AI how to do something (F1).** Click Ask next to the menus, press F1 or pick Help → Ask
  AI, and type your question, e.g. "can I reorder the effects on a mixer track?". It opens
  ChatGPT with the question and your WAVE version, and since WAVE is open source it checks the
  code and answers with steps. Prefer DeepWiki? Switch to it: your question is copied, paste it there.
- **Icons in the menus.** Every command in File, Edit, View and Help now has an icon.
- **Use LV2 effects on Linux.** WAVE finds the LV2 plugins installed on your system (for example LSP,
  x42, swh and others) next to your VST3s: pick one from Plugins in the effect picker, edit it in
  WAVE's parameter list and automate it. Its settings are saved with the project. Needs liblilv
  (installed with the .deb and .rpm packages; WAVE tells you what to install when it is missing).
- **Plugin delay is compensated.** VST3 and LV2 effects that report a latency (lookahead
  limiters, linear-phase EQs) now stay in time with the rest of the mix.
- **Dock the Channel rack and the Piano roll.** Click the dock button in a window's title bar,
  or use the View menu, to keep the Channel rack on the left of the Playlist and the Piano roll
  under it, so they never get lost behind other windows. Drag their edges to resize them.
  F6 and F7 show, focus and hide a docked panel, and your layout is kept after a restart.
- **Mix view (F9).** Press F9 (or the toolbar's mixer button) to give the whole window to the
  mixer, with a level readout and what plays through each insert. Press F9 or Esc, or click
  Back to arrange, and your Playlist, docked panels and windows come back exactly as you left
  them. Effect windows stay on top while you mix.
- **Route channels to the mixer like in FL Studio.** Shift+click channel names in the Channel
  rack to select several, pick an insert and press Ctrl+L to send them all there, or Shift+Ctrl+L
  to give each its own insert from that one on (new inserts are added and named after the
  channels). Both are in a strip's right-click menu too, and one Ctrl+Z undoes them.
- **Chorus and Phaser effects.** A new Modulation group in the effect picker: Chorus (Chorus,
  Ensemble and Vibrato modes) and Phaser (2 to 12 stages), both with a rate you can sync to
  the tempo, a live display of what they do and presets to start from.
- **Save an insert's effects as an FX chain preset.** Save the whole chain of a mixer insert
  by name and load it onto any insert of any project, replacing what is there or adding to
  it. Loading is one undo step; rename and delete presets in place.
- **Import and export MIDI files.** File → Import MIDI file… (or drop a `.mid` on the Playlist)
  lists the parts in the file with the WAVE instrument each one becomes: drums become
  samplers, pianos WAVE Keys, everything else the closest synth preset. The parts you pick land
  on the Playlist as one pattern, with the file's tempo if you want it. A channel's menu
  imports one part into that channel. Export writes the whole song or one pattern as a
  standard MIDI file, one track per channel.
- **Find missing audio when a project opens.** When samples or recordings were moved, WAVE
  shows a Missing audio dialog: point it at a folder (subfolders included) and it relinks
  everything it finds, lets you choose when several files match, or locate a file by hand.
  Missing clips are marked FILE MISSING in the Playlist and a toolbar chip reopens the dialog.
  Each relink can be undone.
- **Export 16-bit WAV.** CD-quality WAV for the mix or stems next to 24-bit and 32-bit float,
  dithered by default.
- **Watch an export as it renders.** The Export dialog shows the file in progress with a live
  waveform, the bar being rendered, elapsed and remaining time, the peak level with a clipping
  warning, and which stems are done.
- **Copy, paste and reorder in the Channel rack.** Copy and paste a channel's notes from its
  menu, drag channels to reorder them, and drop a sample from the library between two
  channels to add it exactly there.
- **Switch to WASAPI when your ASIO interface is unplugged (Windows).** If the interface goes
  away, the audio notification and Settings → Audio offer Switch to WASAPI to keep playing
  through the default Windows output, and Retry ASIO once it is back.

### Changed
- The Channel rack and the Piano roll remember where you put them when floating, after a restart too.
- A short Channel rack or Piano roll hides its hint line at the bottom to leave room for the notes.
- The mixer under the Playlist has no title bar any more, so the Playlist gets more room. Drag
  its top edge down to collapse it to a thin handle; click the handle or drag it up to bring
  it back at the same height. View → Collapse docked mixer does the same.
- Mixer strips show one name row: the insert number and its name, with the effect count beside
  it. Inserts you have not named show just their number. The docked mixer keeps only the
  controls you need while arranging; the Mix view shows the rest.
- **Sung notes come out as the notes you sang.** The pitch editor keeps vibrato, drift, scoops
  and slides inside their note instead of chopping them into many short ones, splits a note
  you sing again on the same pitch, and keeps quick notes you actually hold. *Copy notes* now
  pastes a melody into the Piano roll that resembles what was sung, with far fewer stray notes.
  Clips whose notes you already edited keep them.
- *Copy notes* keeps the timing you sang instead of snapping to the 1/16 grid, which moved
  notes and dropped quick ones. The pasted notes stay selected: Quantize snaps them in one step.
- The Export dialog is grouped into "What to export" and "File format" and says what it will
  write: the file name or number of files, length, sample rate, format and estimated size.
  Sample rate and tail are remembered, and Enter starts the export.
- A sampler channel's window and the sample window are now one sample editor with the same
  controls in both places.
- Piano roll notes on a sampler stop at their length (or the end of the sample), in One-shot
  mode too. Step sequencer hits still play the sample to its end.

### Fixed
- WAVE recovers when a Focusrite USB interface is not connected: it waits for Windows to
  detect the interface before opening any ASIO driver instead of probing a missing device.

## 0.9.15 — 2026-11-14

### Added
- **Export stems as MP3.** Choose Stems and MP3 in the Export dialog, pick a bitrate,
  and get one file per mixer insert with sound, plus the optional master mix.
- **Share a project as a ZIP.** File → Export project as ZIP packages the project and its referenced
  audio so it can open from a new folder. Unzip it and open the included `.wave` file.
- **Shape sampler hits with Fade In and Fade Out.** Set fades at the edges of the selected
  sample region. Step sequencer hits ring out while piano roll notes can follow the sampler gate.
- **Edit vocal pitch from the command line.** Use `wave pitch` to inspect vocal notes, preview
  shifts and apply edits that open in WAVE's Pitch Editor.

### Changed
- The Playlist, Piano Roll and Pitch Editor share the same time ruler and overview controls.
- The `wave` command prints numbers with up to four decimal places.

### Fixed
- Ctrl-drag starts a selection rectangle in the Playlist even when you start over a clip.

## 0.9.14 — 2026-11-13

### Added
- **Edit and render projects from the command line without opening WAVE.** `wave new song.wave`
  creates a project, and `wave --project song.wave <command>` (or `-p`) runs any edit or read on
  the file and saves it back: change the tempo, write patterns, add effects, then
  `render --out mix.wav` or `--stems` for batch bounces, build servers and AI agents. `script
  edits.txt` runs many commands with one load and one save, and writes nothing if a line fails.
  `--dry-run` tries an edit without saving, `--save-as` writes a copy. A file the app has open is
  never overwritten behind its back.
- **Wrap effect tails on WAV loops.** Choose *Wrap* to hear the end of the delay and reverb at the start of the exported mix or stems. The file is one loop long.
- **Export 24-bit PCM WAV.** Choose it for the mix or stems and enable dithering if needed. WAV defaults to 32-bit float.
- **Open `.wave` projects from Windows File Explorer.** Projects have a document icon and open on double-click. If WAVE is already running, it brings the window forward and asks about unsaved changes before switching projects.
- **Tune a vocal to the melody you played in the piano roll.** Play a melody, sing along to it,
  then in the Piano roll press **♪ Vocal reference** (only the selected notes, or the whole
  channel) — the vocal's pitch editor opens with that melody as its target. Or open the pitch
  editor, choose *To reference* and pick the melody from *Melodies in the project* (they are
  listed by how well they fit what you sang). The vocal keeps its own timing: singing a little
  early or late is fine, the octave nearest your voice is used (a melody played an octave higher
  just works), in a chord the note nearest your voice is taken, and ad-libs with nothing played
  under them stay as sung. The melody is copied into the vocal, so changing the piano roll later
  does not change it until you press *Update*.
- **Correction you can understand: Strength, Speed and Flatten**, with one-click styles
  (Natural, Pop, Tight, Robot). *Speed* is how fast the voice is pulled onto the note — 0 ms is
  the hard "robot" step, around 50 ms tight pop, 100 ms and more keeps the singer's own scoop. The
  line under the controls says in words what *Correct* will do. A reference has its own Speed too.
- **Play and loop in the pitch editor like in the Playlist.** Click the ruler to move the start
  marker (while playing, playback jumps there), drag it to set the loop, double-click to remove
  the loop; Space or *Play* plays the song so you hear the vocal in the mix, and the view follows
  playback. *Solo* plays the vocal alone, with your edits, from the marker.
- **Pitch Correct one click away.** The pitch editor, the sample window and a sampler channel's
  window have a *Pitch Correct* button that puts the real-time tuner on that sound's mixer insert
  (with the editor's key and scale), or opens it when it is already there. The sample window also
  opens the pitch editor.
- **Main volume knob next to Play and Stop**, like FL Studio: it is the Master fader (drag, scroll,
  double-click for 0 dB, right-click to automate).

- **Reshape and generate notes in the Piano roll.** A *Tools* strip in the header humanizes,
  quantizes (optionally to the project's key), strums, fills a Euclidean rhythm, builds a drum
  groove or writes a melody in the key. Every tool has a *seed*: the same seed always gives the
  same notes, so you can try a few, keep the favourite and come back to it. The same tools are on
  the command line (`wave notes …`, `wave key`).
- **Copy sung notes to the Piano roll.** In the pitch editor, Ctrl+C or *Copy notes* puts the
  selected notes (or all) on the note clipboard; Ctrl+V pastes them into any pattern in time.
- **Full-screen Piano roll**, which remembers where you moved or resized it.

### Changed
- A more compact toolbar and pitch editor header. Playlist lanes show their mixer channel only
  when you routed them to one, and *Make unique* no longer creates a mixer insert.

### Fixed
- VST3 presets restore correctly on systems whose language writes decimals with a comma
  (German, Polish, Danish, French…): settings no longer jump to rounded values.
- Melody, Euclid fill and Drum generators replace the channel's notes: applying again (with a
  new seed or with notes selected) no longer stacks a second melody or doubles hits.
- Humanize and Strum leave very quiet notes alone, and scale-aware quantize no longer fails on
  the highest and lowest MIDI notes.
- The Piano roll's key and scale follow the project after undo or opening another project.
- Double-clicking the project that is already open in File Explorer no longer asks to discard its
  changes; WAVE no longer opens another project while recording; crash recovery is offered also
  when WAVE starts by opening a file.
- Closing WAVE, starting a new project or restarting for an update while a project is still
  opening now says why it waits.
- Pitch editor: *Update* melody appears and disappears correctly after Apply, Update and undo, and
  Space or Delete in its Key, Scale or Reference list no longer starts playback or resets notes.
- Command line: `wave bpm 90 --dry-run -p song.wave` edits the file, not the project open in the
  app (switches no longer swallow the word after them); `save --path` inside a script is refused
  instead of silently writing nothing; `wave new --force` never replaces a project the app has open.
- Loop takes keep their recorded ending when you stop on the boundary. Starting partway through a loop also keeps the start of every following pass.
- The pitch editor's preview now plays your pitch edits (it used to play the original recording).
- The reference list no longer offers drum hits and one-shots: only recordings in the playlist or
  a comp, at least a second long.

## 0.9.13 — 2026-11-12

### Added
- **Make unique for audio clips**, like FL Studio. Open an audio clip's menu (click or right-click
  the icon in its top-left corner) and choose *Make unique*: the clip gets its own copy of the
  sample ("Guitar #2", same file, nothing is copied on disk) on a new track right below, sent to a
  free mixer insert named after it, so you can put effects on that one clip. Selected clips of the
  same sample on that track go along. Other clips of the sample stay as they are; Ctrl+Z undoes it
  in one step. From the command line: `wave make-unique --clip ID`.
- **One sample window for audio clips and sampler channels**, like FL Studio. Double-click an audio
  clip and you get the same window as a Channel Rack sampler: the waveform (click it to listen,
  drag the markers for the part that plays and the corner handles for the fades), the file, then
  Reverse, Normalize and the knobs. The MIXER picker sits in the header of both, next to the close
  button; scroll over it to step through the mixer inserts.
- **Transpose audio clips ±12 semitones without changing their length.** PITCH and FINE in the
  sample window. It is rendered once in the background (the Playlist shows `+5 st …` meanwhile and
  the clip plays at its own pitch until it is ready) and saved with the song.
- **Fit an audio clip to 1 or 2 beats or 1, 2, 4 or 8 bars** with one click (*Fit to* in the sample
  window), like FL Studio's TIME → Beat/Bar. Keep pitch stretches it at the same pitch; Resample
  changes speed and pitch together. Lengths that would need more than ¼×–4× are greyed out.
- **Icons in Project → Audio**: every sample shows the icon its clips show in the Playlist.

- **Drop a library sample on a Channel Rack channel to replace its sample**, like FL Studio. The
  row lights up and the label says what will be replaced; the channel keeps its steps, notes and
  mixer routing (a synth or WAVE Keys channel becomes a sampler). Drop on empty rack space for a
  new channel.
- **The standard library grew from 50 to 148 sounds.** More 808 (long, boomy, deep and punchy
  kicks, four more snares, short and long open hats, deeper and brighter toms, a mid conga) and
  six 808 basses, one of them driven so it carries on small speakers. More acoustic drums
  (snares with and without wires, rimshot, cross stick, rope and field drums, group claps, floor
  tom, cymbal bell, choke) and hand percussion: congas, darbuka, frame drum, agogo, cowbells,
  guiro, vibraslap, sleigh bells, log drums, anvil, brake drum.
- **Orchestra.** Three orchestra hits (open, minor, major) built from strings, brass and
  timpani, a strings stab, a brass stab and pizzicato, all on C so they follow the notes you
  play; single notes of tremolo strings, cello, contrabass, trumpet, horn, flute and harp;
  timpani, concert bass drum, gong, tubular bell and an orchestral crash.
- **More tonal one-shots**, each tuned to its key: vibraphone, xylophone, balafon, kalimba,
  hand chime, harpsichord, upright piano, FM piano and clav, pipe organ, tenor sax.
- **FX.** Cymbal swells (short and long) and a bass-drum swell for build-ups, timpani and snare
  rolls, wind chimes, a bell tree and a zap.
- **Search in the Library.** Type in the field above the list: several words must all match
  the name or the group ("808 bass", "orchestra hit"). It looks in the standard library, your
  installed kits and your folders at once.
- **Browse samples with the keyboard.** ↑ and ↓ move through the list and play each sample;
  Enter adds the one you are on as a sampler channel.

### Changed
- **Play, stop and record come first in the toolbar**, right after the project name.
- **The sample window has no Preview, Trim, Pitch… or Tempo / key… buttons any more.** Click the
  waveform to listen. *Trim to the part that plays*, *Edit pitch…* and *Detect tempo and key…* are
  in the clip's menu (the icon in its corner).
- **PAT / SONG switches what is playing**, like FL Studio: while the song plays, PAT (or L) loops
  the current pattern straight away, and SONG goes back to the song, with no Stop in between.
- **The Piano roll zooms out further**: until the whole pattern and the empty bars after it fit.
- **The Library tab is easier to read.** Groups are rows with their own icon, colour and file
  count that open and close with a click anywhere on the row; the open/closed arrows are
  drawn clearly instead of the tiny grey marks. Every sample carries its group's icon, your
  own folders get icons from their names (kicks, snares, hats, bass, vocals …), and the
  standard library has its own section above the kits you can download.
- **Pitched samples show their key** (G#1, C4 …) in the Library, the key a sampler channel
  made from them is rooted at.
- The installer is about 11 MB larger: the new sounds ship inside WAVE and need no download.

### Fixed
- **A new pattern plays at once.** Making or picking a pattern while a pattern loops could keep
  playing the old one until Stop.
- **808 drums get a drum icon.** "808 Kick", "808 Snare" and the like showed the bass icon.

## 0.9.12 — 2026-11-12

### Added
- **Export stems.** *Export → Output: Stems* writes every mixer track that has sound as its own
  WAV, named after the track, with one folder pick, like FL Studio's *Split mixer tracks*: no more
  soloing and bouncing one track at a time. All files have the same length and start together, so
  they line up in another project or DAW. A stem is the track after its own effects and fader,
  before the master; *Master* adds `Master.wav`, the whole mix with the master effects. Muted
  and empty tracks are skipped, solo is ignored, and a cancelled export leaves no half-written
  files. The same from the command line: `wave render --stems --out folder/`.
- **Stretch audio clips in the Playlist.** Drag an audio clip's edge to make it longer or shorter.
  Trim | Stretch (Shift+M) decides what the edge does: in Stretch mode the clip plays slower or
  faster (¼× to 4×) and keeps its pitch. The stretch is rendered once, in the background, when
  you let go; the Playlist shows which clips are still working.
- **Warp: draw how time and volume move over every bar.** A new effect for stutters, rolls,
  half-time, tape stops and starts, reverses, scratches (time) and gates, trance gates and
  sidechain pumps (volume). Each effect holds 16 time and 16 volume slots, ready-made and
  editable: click a slot to edit and play it, drag points on the map (the slope is the speed,
  the dashed line is "stopped", the grey lines are "reversed"), right-click a point for its
  shape or a musical speed (×½, −7 st, stop, reverse). Change slots over the song with an
  automation clip, or live; Trigger sync waits for the next 1/16 … bar, One-shot plays one bar
  and returns, Hold loops the previous or the next bar. Jumps are de-clicked, and drum hits
  keep their attack.
- **A library of 86 ready-made Warp maps.** Right-click a slot → *Library*: stutters, rolls and
  build-ups, half-time, tape stops and brakes, reverses, scratches, glitches (time), and
  sidechain pumps, gates, trance gates, fades, mutes and tremolos (volume).
- **Three new automation curve shapes:** pulse (square), wave and half sine (tape stop / start).

### Changed
- **The Delay is now a full echo unit.** Its time is set in beats ("1/2 beat", "1 1/2 beats",
  triplets and dotted) and follows the song tempo, even tempo changes; or in ms. Inside the
  feedback loop: a resonant filter (LP/HP/BP), sample-rate and bit reduction, a saturator or
  limiter you can drive into self-oscillation (feedback up to 125 %), diffusion that smears
  repeats into a cloud, and an LFO for tape wow, chorus or filter sweeps. Mono, stereo with an
  offset, ping-pong, or no echo at all (a filter / lo-fi / drive unit). Repitch glides the
  pitch when the time changes, like tape; Keep pitch never bends it. Projects with the old
  Delay sound the same.
- **WAVE opens its window right away.** It no longer waits for your last project to load before
  showing anything: the window appears at once and the project comes in while the interface
  starts, so a big session is ready sooner and there is no more clicking the icon again.
- **A much smaller download.** The installers no longer carry the parts of the piano recordings
  that never play (each note rings at most 9 seconds), so the WAVE Keys piano takes about 23 MB
  instead of 138 MB. It sounds the same.
- **One Windows installer.** Releases have only `setup.exe` now. If you installed WAVE from the
  `.msi`, uninstall it and install the `setup.exe` once; updates then arrive by themselves.

## 0.9.11 — 2026-11-12

### Added
- **Your work survives a crash, even before the first save.** A project you never saved is
  copied aside at every autosave (Settings → General → Autosave, every 2 minutes by default).
  If WAVE closes unexpectedly, the next start offers it back: *Recover*, *Delete copy* or
  *Not now*. It opens as an unsaved project, so Ctrl+S asks where to put it.
- **The previous save is kept.** Every save first keeps the file it replaces as
  "Song (backup).wave" next to your project. It opens like any project.
- **JACK on Linux.** Settings → Audio → Driver → JACK uses JACK or PipeWire's JACK at the rate and
  buffer the server runs at. The ALSA audio thread now asks for real-time priority, so small
  buffers drop out less.

### Changed
- **Songs can be four times longer:** 512 bars instead of 128 (almost 12 minutes at 174 BPM).
- **VST3 on Linux says what it can do.** Plugin windows are not available on Linux yet; the
  effect window shows a note instead of a button that failed, and you edit the plugin with
  its parameter list.

## 0.9.10 — 2026-11-12

### Added
- **Automation you edit right on the clip, with curves.** No more double-click to start
  editing: drag any point straight away (Shift locks the value, Ctrl the time, Alt skips the
  snap). Drag the dot halfway between two points to bend that stretch; right-click a point for
  *Single curve*, *Double curve (S)*, *Hold* or *Stairs*, or to type in an exact value in the
  control's own unit ("-6 dB", "1.2k"). Right-click empty space to add a point, double-click a
  point to delete it, Ctrl+drag to select several and move or delete them together. Values show
  as the knob shows them while you drag. What you see is exactly what plays.
- **Piano roll: To new pattern (Ctrl+Shift+X).** Select notes and move them into a new pattern
  in the same place. The playlist keeps playing them: every clip of the old pattern gets a clip
  of the new one on a free track below. One undo step.
- **Stacked clips are visible.** Where clips on a track overlap, the header is hatched; a clip
  that hides others completely shows "×2", "×3"…, and clicking it says so.
- **Audio comes back by itself.** If the audio device or driver stops (PipeWire/ALSA restart,
  an interface unplugged), WAVE reopens it and tells you, instead of freezing the transport
  until you change the audio settings.

### Changed
- **Picking or making a pattern from the toolbar opens the Channel Rack.** Browsing to a
  shorter pattern no longer shrinks the rack, and it reopens as wide as you left it.
- **A closed hand while you drag.** Clips, notes and windows show the grabbing cursor for the
  whole drag (it used to stay an open hand).

### Fixed
- **Opening a saved project failed** with "undefined is not an object
  (…hasOwnProperty…)". Missing audio files are now listed when a project opens.
- **Cancelling the save dialog no longer loses work.** "Save" in the unsaved-changes question
  followed by Cancel used to go on and discard the project.
- The mixer no longer leaves a grabbing cursor behind after you reorder effects.

## 0.9.9 — 2026-11-11

### Added
- **Move around like in FL Studio.** Middle-button drag pans the playlist and the Piano roll;
  the view scrolls by itself when you drag a clip, a note or a selection box to its edge;
  Page Up / Page Down zoom at the pointer, Shift+1 / 2 / 3 are zoom presets, Shift+4 shows
  everything, Ctrl + right-drag zooms to a box and Ctrl + right-click goes back.
- **Piano roll:** it follows the playhead; Alt + wheel over a note changes its velocity;
  Shift or Ctrl pressed while dragging locks the pitch or the time; Ctrl+Q quantizes and
  Ctrl+L makes notes legato; double-click a note for its properties; Shift+I inverts the
  selection and Ctrl+click on a piano key selects every note of that pitch. New notes take
  the velocity of the last note you touched, like its length.
- **Channel Rack:** Ctrl+click on a channel's light solos it, Alt + Up / Down moves the
  channel, Ctrl+C / X / V copy steps between channels and patterns, and the wheel over the
  title bar switches pattern.

### Changed
- **The playlist deletes with a right-click, like FL** (right-drag deletes what it crosses).
  A clip's menu now opens from the icon in its top-left corner.
- **A beat plays on the first Space.** New installs start in PAT mode, and in SONG mode an
  empty playlist plays the pattern you are editing instead of silence.
- **Notes sound while you draw them** (Settings can switch that off).
- **The Channel Rack sizes itself and has room to breathe:** taller rows, bigger steps and a
  margin around the grid. Its width follows the pattern's length in both directions and only
  scrolls once it spans the screen; it grows when you add a channel.
- **The Piano roll opens over the whole work area**, like FL's. Double-click its title bar to
  switch between that and a smaller window. Both windows reopen where you left them.
- A double-click on a pattern always opens the Channel Rack (it used to open the Piano roll
  instead when only one channel played the pattern).
- **Split by channel adds the tracks it needs** instead of stacking clips on the last track.
- **Playlist tracks in a new project are plain lanes**, as in FL: none is tied to a mixer
  insert any more (they play into the master). Send a track to an insert from its header when
  you want to; existing projects keep their routing.
- Stop works like Space; pressing it again while stopped goes back to the start of the song.
- Paste on the playlist lands right after what you copied, and keeps chaining.
- A project with nothing on the playlist opens on the Channel Rack.
- Tool, snap, zoom, follow and the panel layout are remembered between sessions.
- Selecting a channel selects its mixer insert; mixer strips also name the channels routed to them.
- Slate theme: much higher contrast.

### Fixed
- **Interface scale:** at 110 % and above the bottom of the window (the mixer) was cut off.
- **Autosave never ran** until its setting was changed once.
- **A sample could show another sample's waveform** after opening or creating a project.
- Escape closes Export, Settings and the channel window; a rename is kept when you click
  elsewhere; the delete button of an effect no longer fires on the way to the bypass light.
- Keys typed in the Piano roll no longer switch the playlist's tool behind it, and the typing
  keyboard no longer turns itself back on at start.

## 0.9.8 — 2026-11-11

### Added
- **Menus: File, Edit, View, Help.** New, open, save, export and settings live in File; undo,
  redo and the clipboard in Edit; the browser, mixer, Channel Rack, Piano roll, follow and
  zoom in View; shortcuts and updates in Help.
- **Piano roll scrollbars that zoom.** Drag a bar to scroll, drag either end of it to zoom
  time or key height. The keys are taller by default.
- **Shift+drag clones notes** in the Piano roll: the selection (or the note under the pointer)
  is copied and the copies move. Shift+click still adds a note to the selection.
- **Hear what you click.** Holding a key on the Piano roll keyboard plays it, and sliding
  across the keys follows; a click on the waveform in a sampler channel's window plays the sample.
- **Slate theme**: a lighter dark theme (Settings -> Appearance).
- Icons for every instrument and preset in the add-channel menu.

### Changed
- **The status bar at the bottom is gone**, so the playlist and mixer get its space. Messages
  and the tool hint now sit in the toolbar under the menus; the transport is centred.
- **Bigger Channel Rack steps** with more room between them and a gap after every beat.
- A right-click on the mixer sends the floating windows behind, like a click on the playlist.
- **Patterns in the browser are bigger, coloured blocks** with their own right-click menu:
  open, rename, clone, split by channel, add or remove a bar, colour, delete.
- A right-click anywhere in the Piano roll clears the selection.

### Fixed
- **Piano roll: a note dragged past the end of the pattern no longer jumps around.** The
  pattern still grows to fit, but the view keeps its scale while you drag.
- **Trimming, fading, splitting and clip volume work on the whole selection.** Drag the edge
  or the fade handle of one selected clip and every selected clip follows; *Split here* and the
  clip volume buttons in the clip menu do the same.
- **A new selection box on the playlist replaces the old selection** (hold Shift to add to it).
  With the pencil, Ctrl+drag used to keep adding, so a selection never went away. Ctrl+click
  on a clip toggles it with the select tool too.
- **Short clips can be moved again.** A clip a few pixels wide was all trim zone; now narrow
  clips keep a body to grab, and a plain click on an edge or a fade handle changes nothing.
- **After Space, the keyboard stays in the Piano roll or Channel Rack.** Delete, Ctrl+C / V and
  the tool keys used to go to the playlist after a play / stop and could delete a clip there.
- Piano roll: picking a grid, key or channel from the lists no longer leaves the arrow keys
  and Delete stuck in that list; switching pattern clears the note selection.
- Channel Rack: painting steps into the greyed extra bar no longer shifts the grid under the
  pointer.

## 0.9.7 — 2026-11-11

### Added
- **A standard library of samples, built in.** Fifty drum, percussion and tonal one-shots
  (808 and acoustic kicks, snares, claps, hats, cymbals, toms, congas, shakers, an 808 bass,
  marimba, glockenspiel) are in the Library tab from the first start: no download. Drag one onto
  a track for an audio clip, onto the Channel Rack for a new sampler channel, or onto an open
  sampler to swap its sample. The "+ Channel" menu lists them too.
- **A real sampler in the channel window.** Drag the start and end markers on the waveform to
  pick the part that plays; Reverse and Normalize; transpose and fine tune; a volume envelope
  (attack, decay, sustain, release) drawn over the waveform; One-shot (drums: the sample rings
  out) or Gate (bass and melodies: it stops with the note); and cut groups, so a closed hi-hat
  silences the open one.
- **EQ: a band bank beside the graph.** Each band has a shape button, a gain slider and
  FREQ and BW knobs next to the curve; bands that are switched off no longer clutter the graph.
  New presets that only cut: below 20 Hz and above 18 kHz, low cut 80 Hz, low cut 150 Hz
  (vocal), high cut 10 kHz.

### Changed
- **Kick, Clap, Hat and Bass are samples now.** The built-in synthesised instruments, which had
  no waveform and no settings, are gone. A new project starts with Kick, Clap, Hat and Snare
  samplers from the standard library, and you can see and edit each of them. Songs saved with
  the old instruments open with the closest library samples and sound different.
- Drum samples start exactly as recorded: the sampler no longer fades in the first 2 ms, which
  dulled the attack.
- Projects that use library samples open on any computer: they no longer store where the
  library is installed.
- WAVE starts with an empty project (eight tracks, the starter kit and an empty Pattern 1)
  instead of a demo beat.
- A click on the playlist's time ruler switches from PAT to SONG; a pattern loop that is playing
  carries on as the song from there. PAT is shown amber and SONG green.

### Fixed
- A click on a channel's mini piano roll now brings the Piano roll to the front instead of
  leaving it behind the Channel Rack.

## 0.9.6 — 2026-11-11

### Added
- **Patterns like FL Studio.** The current pattern is picked in the toolbar (menu, mouse
  wheel, Num 1-9, + / -); F4 jumps to the next empty one, F2 renames, Shift+Ctrl+C clones.
- **Channel Rack (F6) and Piano roll (F7) are windows of their own.** The rack has compact step
  buttons, a greyed extra bar (click there to make the pattern longer), a graph editor (G) for
  each step's velocity and key, and *Replace* to swap a channel's instrument while keeping its
  notes and routing. The effect picker moved to Shift+F9.
- **Piano roll for every channel**, drums included. Notes of the pattern's other channels show
  as ghost notes (double right-click one to switch to it), and drawing past the end makes the
  pattern longer: its length follows the notes.

### Changed
- **Much smoother editing in big songs.** Drawing and dragging notes in the Piano roll and the
  Channel Rack, turning a channel's knobs and moving a track fader no longer redraw the whole
  playlist (about 3x the frame rate on a 64-track, 1400-clip song).
- **Zooming the playlist stays fluid** on heavy songs: the view follows the wheel at once and
  sharpens when you stop.
- Resizing or opening the mixer no longer redraws the playlist on every frame.

### Fixed
- **Lower CPU after a phrase ends.** Effects fading out on silence (reverb and delay tails,
  filters) could cost more than the music itself; that part of the load is about a quarter lower.
- Dragging one clip on a big song sometimes redrew the whole playlist on every step.

## 0.9.5 — 2026-11-11

### Fixed
- **Chords from the computer keyboard and MIDI keyboard.** Keys you play live now sound
  through the instrument's own mixer track, exactly like the piano roll. Before, they went
  through the track armed for recording (in the starter project the Vocal track, with its
  compressor), which squashed chords and made them sound unnatural.
- **Shift+click selects notes in the piano roll with any tool.** With the pencil, Shift+click
  used to move the clicked note and drop the rest of the selection, so Ctrl+C copied only one
  note. Shift+click now adds or removes a note, Shift+drag adds a box of notes.

## 0.9.4 — 2026-11-11

### Fixed
- **Chords sound like their notes.** Playing several keys at once on the Synth or WAVE Keys
  no longer changes the sound: every note in a chord now sounds exactly as it does alone.
  Before, each new key pulled the others down and the tone pumped and thinned out.
- **Drive on chords.** Poly synth presets with drive (plucks, Keys Dance, Future Chord, arps,
  Brass and more) now saturate each note on its own, so chords stay clean instead of turning
  into a gritty mush.
- **Glide on chords.** With glide on a poly patch, the notes of a chord no longer slide from
  one another: they all glide from the note you played before the chord.

### Good to know
- Poly synth patches and WAVE Keys now play 12 dB quieter, so a full chord fits without
  clipping. Turn the channel up if a single line feels quiet.
- In older projects, pick a drive preset again to get its new per-note drive.

## 0.9.3 — 2026-11-10

### Added
- **Updates inside the app.** WAVE checks GitHub for a new version a few seconds after it
  starts, then every few hours while it stays open. When one is found, a small
  **Update 0.9.x** button appears in the status bar: it shows what is new, downloads the
  installer in the background (you keep working) and only restarts when you say so.
- **What's new after every update.** The first time you start a new version, its release
  notes open once, including the versions you skipped. Settings → General → Updates opens
  them again at any time.
- Settings → General → **Updates**: current version, **Check for updates**, and a switch for
  the automatic check.

### Good to know
- This is the first version that can update itself: install it by hand once, later versions
  arrive in the app.
- Updates are signed; WAVE refuses an installer whose signature does not match.

## 0.9.2 — 2026-11-10

### Added
- **Automate every knob.** Right-click any knob for *Automate* on the playlist, *Select
  automation clips* and *Reset to default*. New automation targets: channel volume and pan
  (Channel Rack), every synth parameter, the WAVE Keys panel, and an audio clip's own gain
  and pan.

### Fixed
- Changing the song during playback no longer drops automated values for a moment (a full
  0 dB blip on the first block after an edit).

## 0.9.1 — 2026-11-10

### Fixed
- **WAVE Keys and the Synth no longer clip on chords.** Both pass an output limiter at -1 dBFS;
  a single note keeps its level.
- **Smoother playlist drags.** Moving clips, dragging the loop range and rubber-band selection
  only repaint what changes (about 60 fps on a 64-track, 1400-clip song instead of 39).

### Added
- **Half-Speed** effect: a tempo-locked slow-down (1.5x / 2x / 4x per loop window).
- 14 new synth presets, harder basses and wider leads.

## 0.9.0 — 2026-11-10

### Added
- **English interface by default, Polish in Settings.** About 1 280 strings are translated.
- New app icon.
- Windows installer built with ASIO and the WAVE Keys piano included.
