# Troubleshooting

Fixes for the problems people actually run into: no sound, crackles, recording out of time, plugins
that do not show up, downloads that fail, and the first launch on macOS.

## Quick checks

1. **File → Settings… → Audio**: the **Status** line should say **● running**.
2. The toolbar's CPU meter: near 100 % the output crackles. Click it to open the audio settings.
3. The status line under the menus: Synkopa reports what just went wrong there.
4. Still stuck? [Open an issue](https://github.com/miskibin/synkopa/issues) or ask on
   [Discord](https://discord.gg/zHMjE3fCY).

## No sound, or the device is unavailable

- **Status says stopped, or "Cannot play — the audio output did not open".** Open **File →
  Settings… → Audio** and pick another **Driver** or **Output**, or click **Try again** in the
  notification. On Windows, **WASAPI** with **Output: Default** works with any sound card.
- **"Windows has no default audio output."** Connect speakers or headphones, or set a default
  output in Windows, and try again.
- **"The audio device stopped and was reopened."** A driver restarted (PipeWire or ALSA on Linux)
  or an interface was unplugged. Synkopa reopens the output by itself and keeps trying every few
  seconds until it works or you change the settings.
- **"ASIO is not compiled into this version"** in Settings: this build has no ASIO. Use WASAPI.
- **Space does nothing:** a text box may have the keyboard. Click the Playlist (or press `F5`)
  and try again.
- **Only one channel is silent:** check its mute light in the Channel rack, the **M** and **S** of
  its mixer insert, and whether any other channel or insert is soloed.

### ASIO and Focusrite interfaces (Windows)

If a Focusrite USB interface is not connected, Synkopa does not open its ASIO driver (it waits for
Windows to detect the interface first) and says **Focusrite USB ASIO is unavailable.** Click
**Switch to WASAPI**, in the notification or in **Settings → Audio**, to keep playing through the
default Windows output. When the interface is back, choose **ASIO** again or click **Retry ASIO**.

The same buttons appear for any ASIO device that goes away. If it disappears while you record,
Synkopa finishes the take and stops monitoring first, so nothing is lost.

### JACK on Linux

With **Driver: JACK**, the sample rate and buffer size are set by JACK or PipeWire, not by Synkopa;
change them there. The status line shows what JACK uses.

## Crackles, clicks and dropouts

The output ran dry: a buffer took longer to render than it lasts. Synkopa counts each one as an
*underrun*.

1. Look at **Settings → Audio → Underruns** (and the CPU meter in the toolbar, which flashes red on
   each one).
2. Choose a larger **Buffer** (512 or 1024 samples while mixing). Smaller buffers only matter while
   recording or playing live.
3. Close other heavy programs. Heavy plugins and many instances of them cost the most.
4. Click **Reset** next to the counter and play again to see whether it helped.

## Recording problems

- **"No recording input is open: choose an input in Settings."** Pick an **Input** in **Settings →
  Audio**. The input opens only while recording or monitoring, so the **Input channel** list fills
  in once one of them is on.
- **"Recording input could not open … Playback works."** Another program may hold the input, or the
  input and output are different devices that do not run together. Try the same device for both.
- **The mic is only on one side, or you record the wrong input:** set **Input channel** to the
  socket your microphone is plugged into. Synkopa records that one channel as mono in both ears.
- **Takes land late or early:** click **Measure latency** in **Settings → Audio** with the mic near
  the speaker or headphone, in a quiet room. If it reports an uncertain measurement, make it louder
  and closer and try again.
- **Input dropouts** (shown under the recording offset): increase the buffer, or use the same device
  for input and output.
- **You hear yourself late while monitoring:** lower the buffer. Effects with look-ahead on your
  track are skipped for monitoring already.

See [Recording vocals](recording-vocals.md).

## Plugins

### A plugin does not show up

- **macOS:** VST3 plugins are not hosted on macOS yet. Use the built-in instruments and effects.
- **CLAP, AU and LV2 instruments** are not supported. VST3 instruments and effects work on Windows and
  Linux; LV2 effects on Linux.
- **Is it in a standard folder?** Synkopa scans `C:\Program Files\Common Files\VST3` and
  `%LOCALAPPDATA%\Programs\Common\VST3` on Windows, `/usr/lib/vst3`, `/usr/local/lib/vst3` and
  `~/.vst3` on Linux. Set the `VST3_PATH` environment variable to add another folder.
- **Rescan:** effects: the **Rescan** button in the effect picker's **Plugins** category.
  Instruments: Channel rack **+ Channel → Plugin instruments (VST3) → Rescan plugins**. A rescan
  opens only plugins it has not seen or that changed.
- **"Cannot run in Synkopa"** next to a plugin: it needs something Synkopa does not provide (for
  LV2, for example a fixed block size). The reason is shown on the tile.

### LV2 on Linux: "LV2 is unavailable"

LV2 needs the lilv library, which Synkopa loads when it first scans. The `.deb` and `.rpm` packages
install it; the AppImage uses it if it is on the system. Install it and restart Synkopa:

| Distribution | Package |
| --- | --- |
| Debian, Ubuntu | `liblilv-0-0` |
| Fedora | `lilv-libs` |
| Arch | `lilv` |

LV2 plugins with a GTK or Qt interface open Synkopa's parameter list instead of their own window.

### A plugin is "not loaded" or crashed a check

The first time a plugin is used, Synkopa tries it in a separate, throw-away process: it creates it,
plays sound or notes through it, saves and restores its settings, and closes it. If that test
crashes, the plugin is not loaded (an effect lets the audio pass through, an instrument stays
silent) and its slot says so. The slot and its settings stay in the project.

- **Update the plugin** if you can; an updated plugin is checked again.
- **Load anyway** loads it from now on, in every project. If it then crashes, Synkopa closes,
  your unsaved work comes back from the autosave, and the plugin is not loaded again until you
  choose **Load anyway** once more.

### Synkopa closed while a plugin was loaded

The next start shows **Synkopa did not exit cleanly last time** with the plugins that were loaded.
**Block** keeps a plugin out of future scans.

### A project opens with a plugin missing

A plugin that is not installed on this computer keeps its slot and its saved settings, so the
project opens and saves unchanged; it plays again once the plugin is installed. Loading an FX chain
preset says which plugins were **not loaded (plugin missing)**.

## Downloads fail: certificate errors

Library kits, Synkopa Instruments and the stem, chord and pitch models are downloaded on demand. On
a work computer whose proxy re-signs HTTPS, a download can fail with **invalid peer certificate:
UnknownIssuer**.

Synkopa already trusts the operating system's certificate store, as your browser does. If that is
not enough, turn on **File → Settings… → General → Network → Ignore certificate errors**. Every
download is still checked against its SHA-256 checksum, so a tampered file is still refused. The
setting also applies to the `synkopa` command line.

## macOS: "Synkopa cannot be opened"

The app is not notarized by Apple yet. Start it once, then open **System Settings → Privacy &
Security** and click **Open Anyway**. Synkopa runs on Apple Silicon Macs with macOS 13.4 or newer;
there is no Intel build.

## Linux display issues

- **The Playlist flickered or showed blank strips while zooming** in older versions. Synkopa now
  starts its web view without the renderer that caused it. To get that renderer back, start Synkopa
  with `WEBKIT_DISABLE_DMABUF_RENDERER=0`.
- **Plugin windows under Wayland:** Synkopa runs its window through XWayland so plugin windows stay
  above it. To keep native Wayland, start it with `WAVE_WAYLAND=1` (plugin windows may then fall
  behind the main window).
- **Synkopa quits at once:** the packages need Ubuntu 22.04, Debian 12 or a newer distribution.

## Projects

- **Missing audio:** when samples or recordings were moved, a **Missing audio** dialog opens. Point
  it at a folder and it relinks everything it finds there (subfolders included). Missing clips are
  marked in the Playlist until relinked.
- **Synkopa closed unexpectedly:** the next start offers the autosaved copy: **Recover**, **Delete
  copy** or **Not now**. It opens with the changes unsaved; your project file still holds your last
  save. The previous save is also kept as `<name> (backup).synkopa`.
- **A panel shows "stopped because of an error":** click **Try again**. Only that panel is
  affected: the project, the audio and `Ctrl+S` keep working.

## The `synkopa` command is not found

The command line is installed when the app runs for the first time. Open a new terminal after that.
**Settings → Audio → Command line** shows whether it is in your `PATH`. See
[Command line](../cli.md).

## See also

- [Getting started](getting-started.md), [Recording vocals](recording-vocals.md),
  [Mixing](mixing.md#use-vst3-and-lv2-plugins).
