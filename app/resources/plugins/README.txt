Plugins that ship inside the installer: Synkopa Demo Synth (crates/demo-synth), a small
polyphonic VST3 instrument. It is not in Git: `node scripts/build-demo-synth.mjs` builds it
and lays out "WAVE Demo Synth.vst3" here; the release workflow runs it before the build.
Synkopa scans this folder with every plugin list (no rescan needed). A build without it still
works: the Channel Rack then lists only the VST3 instruments installed on the computer.
