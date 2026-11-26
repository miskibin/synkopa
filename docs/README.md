# Synkopa documentation

Synkopa is a free DAW that works the FL Studio way: you build patterns in the Channel Rack, arrange
them on the Playlist and mix them on the Mixer. These pages show you how to do that, from the first
launch to an exported song.

**New to Synkopa?** Read [Getting started](guide/getting-started.md). It takes about ten minutes and
ends with a beat you can listen to as an MP3. Then read [The basics](guide/the-basics.md): one page
that explains how the windows fit together, which makes everything else easier.

**Coming from FL Studio?** Most gestures are the same (right-click deletes, `F6` opens the Channel Rack,
`F7` the Piano roll, `F9` the Mixer, `Ctrl+Q` quantizes). Skim [The basics](guide/the-basics.md) and keep
[Shortcuts](guide/shortcuts.md) open.

## Make music

| I want to… | Read |
| --- | --- |
| Install Synkopa, set up audio and make a first beat | [Getting started](guide/getting-started.md) |
| Understand channels, patterns, the Playlist and the Mixer | [The basics](guide/the-basics.md) |
| Program drums, write melodies, chop samples, use chords | [Making beats](guide/making-beats.md) |
| Arrange a song: clips, audio, stretching, automation, markers | [Arranging](guide/arranging.md) |
| Record and tune vocals | [Recording vocals](guide/recording-vocals.md) |
| Mix: routing, sidechain, effect chains, plugins | [Mixing](guide/mixing.md) |
| Split a song into stems or pull chords out of audio | [Stems and chords](guide/stems-and-chords.md) |
| Export a song, stems or a shareable project | [Exporting](guide/exporting.md) |
| Look up a key or mouse gesture | [Shortcuts](guide/shortcuts.md) |
| Fix sound, device, plugin or download problems | [Troubleshooting](guide/troubleshooting.md) |

## Instruments and effects

- [Instruments](instruments/README.md): which one to pick, then a page for each: the
  [Synth](instruments/synth.md), [Synkopa Instruments](instruments/synkopa-instruments.md) (sampled
  piano, strings, guitars and more), the [Sampler](instruments/sampler.md) and the
  [Music Box](instruments/music-box.md).
- [Effects](effects/README.md): all built-in effects grouped by what they are for, each with a quick
  start and tips.

## Scripts and AI agents

- [Command line](cli.md): the `synkopa` command edits the open project, or a project file without
  the app. Use it from scripts or let a coding agent work on your song; every command is one undo step.

## Contributing

- [Architecture](dev/architecture.md): how the Rust engine, the UI and the CLI fit together, and the
  rules that keep audio glitch-free.
- [Releasing](dev/releasing.md): how a version is built and published.
- [CONTRIBUTING.md](../CONTRIBUTING.md) and [CHANGELOG.md](../CHANGELOG.md).

Something missing or unclear? [Open an issue](https://github.com/miskibin/synkopa/issues) or ask on
[Discord](https://discord.gg/zHMjE3fCY).
