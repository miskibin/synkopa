# Instruments

Every sound in a pattern comes from an instrument on a Channel Rack channel; this page helps you pick
the right one for the part you are making.

## Which instrument for what

| Instrument | Reach for it when you want | Page |
| --- | --- | --- |
| **Sampler** | Drums and one-shots on the step sequencer, a sample played across the keys, or a loop chopped into slices | [Sampler](sampler.md) |
| **Synthesizer** | 808s and basses with glide, leads, pads, plucks, arps, risers: 50 presets you can reshape | [Synthesizer](synth.md) |
| **Synkopa Instruments** | Real recorded pianos, electric pianos, strings, guitars, harp and vibraphone (11 instruments) | [Synkopa Instruments](synkopa-instruments.md) |
| **Music Box** | A delicate cylinder music box for intros and lullabies, with no download | [Music Box](music-box.md) |
| **Plugin instruments (VST3)** | A third-party instrument you already own (Serum, Vital, Kontakt...) | below |

A rough guide:

- **Drums:** samplers, one channel per sound. Start from the standard library's kicks, snares and hats,
  or download the 808 Kit or Studio Drums.
- **Bass:** the synthesizer's **808** and **Bass** presets. For an 808 that slides, see
  [Make an 808 or a bass slide](synth.md#make-an-808-or-a-bass-slide).
- **Chords and melodies that should sound played:** Synkopa Instruments (Grand Piano is ready at once;
  the others download once).
- **Electronic chords, pads and leads:** the synthesizer.
- **Chopping a loop or a vocal:** the sampler's Slice page.

## Add an instrument

1. In the Channel Rack, click **+ Channel** under the last channel.
2. Pick from the menu:
   - **Sampler**: a sample from the standard library by group, **Project audio** (audio already in the
     project), or **File from disk…**.
   - **Synthesizer**: a preset by category, or **Empty synth** for a blank patch.
   - **Sampled instruments**: **Synkopa Instruments — pianos, strings, guitars** or **Music Box —
     modeled tines**.
   - **Plugin instruments (VST3)**: the VST3 instruments installed on this computer.
3. Click the new channel's name to open its window. Press `F7` to open it in the Piano roll.

You can also drag a sample from the Browser's **LIBRARY** tab onto the Channel Rack, or select it and
press `Enter`, to get a sampler channel.

To change what an existing channel plays and keep its notes and mixer routing, right-click the channel
→ **Replace** and pick another instrument from the same menu.

Every instrument plays from the step sequencer, the Piano roll and a MIDI keyboard, and every channel
has its own volume, pan and mixer insert.

## Plugin instruments (VST3)

Synkopa scans the standard VST3 folders for instruments and lists them under **Plugin instruments
(VST3)** in the **+ Channel** menu. When there are more than 12, they are grouped by vendor.

- Clicking a plugin channel's name opens the plugin's own window. Its gear opens Synkopa's channel
  window, with the plugin's parameters, volume, pan and mixer routing.
- Installed a new instrument? Click **Rescan plugins** at the end of the list. A plugin seen before is
  not opened again, so a rescan is quick.
- **No VST3 instruments found** means nothing is installed in the standard VST3 folders. Install the
  instrument there, then rescan.
- LV2 plugins can be used as effects only, not as instruments.

The plugin's state is saved with the project.

## See also

- [Making beats](../guide/making-beats.md): the Channel Rack, step sequencer and Piano roll
- [Sampler](sampler.md)
- [Synthesizer](synth.md)
- [Synkopa Instruments](synkopa-instruments.md)
- [Music Box](music-box.md)
- [CLI](../cli.md): `synkopa channels` adds and edits instruments from the command line
