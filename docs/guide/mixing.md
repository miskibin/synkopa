# Mixing

How to balance your tracks, add effects, route sound between mixer inserts, duck the bass under the
kick, and use VST3 and LV2 plugins.

![A mixer strip's menu: effects, FX chain presets, routing, Send to and Sidechain to](../pr/sidechain-menu.png)

## Quick start

1. Press `F9` to show the **Mixer**. Each strip is an *insert*; the first one is the **Master**.
2. Make sure each sound has its own insert: in the Channel rack, double-click a channel's insert
   box (between its knobs and its name).
3. Set levels with the faders first, then pan.
4. Click **+ effect** on a strip (or press `Shift+F9`) to add an effect.
5. Watch the toolbar's output meter: if **CLIP** lights red, turn the Master or the main volume down.

## The mixer window

- `F9`, the **Mixer** button or **View → Mixer** shows or hides it. It docks at the bottom by
  default; drag its title bar to float it or dock it elsewhere.
- **Double-click its title bar** (or **View → Maximize mixer**) to cover the whole workspace with
  larger strips, level readouts and the routing lane. **Back to arrange**, `F9` or `Esc` return.
- Each strip has its name, pan, stereo width, **M** (mute) and **S** (solo), meters and a fader with
  a dB scale. Double-click a fader for 0 dB, a pan knob for centre. `Ctrl`+click or right-click
  **M** to solo.
- Double-click a strip's name to rename it (or select the strip and press `F2`). Right-click it for
  its menu: **Rename…**, colour, **Add effect…**, **FX chain preset**, **Reset volume and pan**,
  **Route selected channel here**, **Send to**, **Sidechain to**, **Remove insert**.
- The **+** after the last strip adds a mixer channel.

## Route channels to inserts

FL users know this as "link selected channels". In the Channel rack, `Shift`+click channel names to
select several (`Shift+Ctrl`+click to add or remove one). Then select an insert in the mixer and:

- `Ctrl+L` (or the strip's **Route selected channel here**) sends them all to that insert;
- `Shift+Ctrl+L` gives each its own insert, from the selected one on, creating the missing ones
  named after their channels.

Each is one undo step. A channel can also pick its insert itself, from the insert box in its rack
row or its window.

## Add and arrange effects

- **Add an effect:** click **+ effect** on a strip, **Add effect** at the end of its chain, or press
  `Shift+F9` for the selected insert. The effect picker groups the 21 built-in effects by job:
  **Vocals and tuning**, **Repair**, **Dynamics**, **EQ and tone**, **Time and space** and others,
  plus **Plugins**, **Favourites** (☆ a tile) and **Recent**. Type to search. You can also drag a tile
  onto any strip.
- **Open** an effect by clicking it. Click its dot to bypass it. Drag it up or down to change the
  order; drag it onto another strip to move it there (`Ctrl` to copy).
- **Presets:** `‹` and `›` in an effect window step through its presets.
- **FX chain presets:** **Presets → Save FX chain as preset…** saves an insert's whole chain to your
  library; **Load FX chain preset…** puts one on any insert, appended or replacing what is there.

Every effect draws what it does to the audio: gain reduction, transfer curves, harmonics, reverb
tails. See [Effects](../effects/README.md) for which one to reach for.

## Send to a bus or a shared effect

An insert's output goes to the Master and can also be *sent* to other inserts:

- right-click a strip → **Send to** → the target insert, or
- in the maximized mixer, drag the dot under a strip onto another strip. The cable lane under the
  strips shows every send; drag or wheel a cable to set its level, right-click it for **Level
  100%**, **Only here (not to the Master)** or **Remove send**.

The **M** chip of the output into the Master can be turned off, so an insert goes only where it
sends (a drum bus, for example). Synkopa refuses a send that would make a loop. Plugin delay is
compensated along every path, so parallel sends stay in time.

## Duck the bass under the kick (sidechain)

1. Right-click the **kick's** mixer strip → **Sidechain to** → the **bass** insert.
2. The bass's Compressor now listens to the kick (if it has none, one set up for ducking is added
   and opened). The bass dips on every hit and swells back between them.

In the Compressor window, the **Sidechain** box picks what it listens to: its own input or any
other insert; the window then draws the kick's level against the gain reduction. The kick insert
does not even have to be heard: turn off its output to the Master and it still drives the ducking
(a "ghost kick"). The Gate, the Filter and the EQ (once it has a dynamic band) take a sidechain from
their own windows. The maximized mixer shows each sidechain as a dashed cable marked **SC**.

## Let the mix assistant find clashes

While you play, Synkopa listens for inserts that cover each other: both loud in the same
frequencies at the same time, bar after bar, like a kick and an 808 at 50 Hz. Only then does a small
amber badge appear on the insert that should give way. Click it:

- **Make room for…** adds an ordinary EQ whose one dynamic band dips that range only while the
  other insert hits, so the 808 keeps its weight between kicks.
- **Ignore this pair** stops reporting it (saved with the project).

Nothing changes until you click. Turn the assistant off in **File → Settings… → General → Mix
assistant**.

## Check loudness and the master

A new project has a **Limiter** on the Master. Add the **Analyzer** to the Master to read short-term,
integrated and momentary loudness (LUFS), loudness range, true peak and a spectrum fine enough to
tell a 41 Hz kick from a 49 Hz 808. It tells you how much streaming services would turn the song
down. The toolbar's output meter turns red with **CLIP** when the mix goes over 0 dBFS; click it to
reset. Main volume and the Master fader act after the Master's effects, so a limiter cannot catch
what they add.

## Use VST3 and LV2 plugins

| System | What is hosted |
| --- | --- |
| Windows | VST3 effects and instruments |
| Linux | VST3 effects and instruments, LV2 effects (needs `liblilv`) |
| macOS | Not yet: built-in instruments and effects only |

Synkopa looks for VST3 plugins in the standard folders: on Windows
`C:\Program Files\Common Files\VST3` and `%LOCALAPPDATA%\Programs\Common\VST3`; on Linux
`/usr/lib/vst3`, `/usr/local/lib/vst3` and `~/.vst3`. The `VST3_PATH` environment variable adds more
folders. LV2 plugins are found on the standard LV2 path (or `LV2_PATH`). Scanning runs in a separate
process, so a broken plugin cannot crash the scan.

- **Effects:** the effect picker's **Plugins** category; **Rescan** looks for new or updated ones.
- **Instruments:** Channel rack **+ Channel → Plugin instruments (VST3)**, with **Rescan plugins**.

A plugin opens its own window inside a Synkopa frame. The gear in that frame opens Synkopa's
parameter list for it; the pin keeps it above other windows. The first time a plugin is used,
Synkopa tests it in a throw-away process. If that test crashes, the plugin is not loaded and its
slot says so, with **Load anyway** if you want to risk it. See
[Troubleshooting](troubleshooting.md#plugins).

## See also

- [Effects](../effects/README.md): [EQ](../effects/eq.md), [Compressor](../effects/compressor.md),
  [Reverb](../effects/reverb.md), [Delay](../effects/delay.md), [Limiter](../effects/limiter.md),
  [Saturator](../effects/saturator.md), [Gate](../effects/gate.md), [Filter](../effects/filter.md).
- [Recording vocals](recording-vocals.md) for the vocal chain and Auto vocal.
- [Exporting](exporting.md) for stems.
- [Shortcuts](shortcuts.md#mixer).
