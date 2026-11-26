# Effects

Synkopa has 21 built-in mixer effects. This page tells you which one to reach for, and how to add
and work with any of them.

## Which effect for what

### EQ and tone

| Effect | Use it when… |
|---|---|
| [Parametric EQ](eq.md) | you want to cut mud, add presence or air, or remove a resonance; up to 24 bands, any of them dynamic |
| [Filter](filter.md) | you want a sweep, a wah, a build-up, pumping or a DJ transition: a resonant filter that moves |

### Dynamics

| Effect | Use it when… |
|---|---|
| [Compressor](compressor.md) | a sound's level jumps around, a bus needs glue, or a bass should duck under the kick (sidechain) |
| [FET Compressor](compressor.md#bring-a-vocal-forward-fet) | you want a fast, coloured 1176-style squeeze; it is the **FET** type of the Compressor |
| Multiband | you want the dense, bright, loud "OTT" sound: upward and downward compression in three bands |
| [Gate](gate.md) | noise or bleed sits between notes and hits, or one sound should open or duck with another |
| [Limiter](limiter.md) | the master must get louder without going over a true-peak ceiling |

### Space

| Effect | Use it when… |
|---|---|
| [Reverb](reverb.md) | a sound needs a room, plate or hall around it, from a short ambience to an 18 s wash |
| [Delay](delay.md) | you want tempo-synced echoes, ping-pong, slapback or dub feedback |

### Character

| Effect | Use it when… |
|---|---|
| [Saturator](saturator.md) | a sound needs warmth, harmonics or grit (tape, console, transformer, tube) |
| Half-Speed | you want half-time in tempo: an octave down per loop window, with the groove kept on the grid |
| [Warp](warp.md) | you want stutters, rolls, tape stops, reverses, scratches, gates or pumps drawn per bar (like Gross Beat) |

### Modulation

| Effect | Use it when… |
|---|---|
| [Chorus](chorus-phaser.md#chorus) | a vocal, synth or guitar should sound thicker and wider, or wobble (Vibrato) |
| [Phaser](chorus-phaser.md#phaser) | a pad or guitar needs sweeping movement, from gentle to jet |

### Vocal

| Effect | Use it when… |
|---|---|
| Pitch Correct | a vocal should be tuned live to a key and scale, from natural to the hard-tune effect |
| Voice | you want to change a voice's pitch or formants, make it robotic, or drive it |
| Vocal Rider | word-to-word loudness should be evened out, like an engineer riding the fader |
| [De-esser](de-esser.md) | harsh "s" and "sh" sounds sting |
| Denoiser | a recording has background noise, hum or room sound to remove |

### Utility

| Effect | Use it when… |
|---|---|
| Stereo | you want to widen or narrow a sound, keep the bass mono, or check the stereo image (vectorscope, correlation) |
| Analyzer | you want to measure loudness (LUFS) and true peak, see a spectrum that names notes, and check the stereo field |

## Add an effect

1. Open the mixer with `F9`.
2. On the insert's strip, click **+ effect**, or click **Add effect** at the bottom of the
   selected insert's effect list. `Shift+F9` opens the same picker for the selected insert.
3. Type to search, or pick a category on the left. Press `Enter` (or click) to add the
   highlighted effect, or pick a preset from the column on the right to add it with that preset.

You can also drag a tile from the picker onto any mixer strip. A new effect starts with its first
preset.

## Work in an effect window

Click an effect in the mixer to open its window. Every built-in effect window works the same way:

- The round button at the top left bypasses the effect (**BYPASS** shows while it is off).
- The preset bar at the top right steps through presets with **‹** and **›**, or lists them all
  when you click the name. A `*` after the name means you have changed it.
- **MIX** in the header blends the dry and processed sound, for effects that have no mix control
  of their own.
- **?** shows a short hint on how to use the window.
- **Advanced** shows the less common controls; **Less** hides them again.
- On a knob: drag to change it, use the wheel for small steps (Shift = finer), double-click or
  Alt+click to reset it, and right-click to automate it.

To save a whole chain of effects for reuse, right-click a mixer strip and use **FX chain preset**,
or the **Presets** button above the effect list.

## Sidechain

Four effects can listen to another insert instead of their own sound: the Compressor, the Gate,
the Filter and the EQ (its dynamic bands). Pick the source in the effect's window. For classic
ducking, right-click the source's mixer strip (the kick) and choose **Sidechain to** → the track
to duck; this keys or adds a Compressor. See [Compressor](compressor.md#duck-the-bass-under-the-kick-sidechain).

## From the command line

`synkopa catalog` lists every effect type with its parameter ids, ranges and presets.
`synkopa add-fx --insert N --type TYPE [--preset I]`, `synkopa set-param` and `synkopa preset`
add and change effects; each command is one undo step. See [the CLI guide](../cli.md).

## See also

- [Mixing](../guide/mixing.md): the mixer, routing, sends and sidechain.
- [Recording vocals](../guide/recording-vocals.md): the vocal effects in context.
