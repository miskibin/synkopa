# Real-world timings: WAVE vs FL Studio

`bench.py` drives the real apps on a Windows desktop and times what a musician waits for. Both
apps go through the same code: input goes in through `SendInput`, results are read off the
screen with DXGI Desktop Duplication (a new frame arrives on every desktop change), and sound
comes back through WASAPI loopback of the default output.

```sh
python -m venv .venv && .venv\Scripts\pip install dxcam opencv-python-headless numpy soundcard
cargo run --release -p wave-perf -- gen song       # the matched project (perf/.cache/song)
cargo build --release -p wave-app --features custom-protocol
.venv\Scripts\python perf/realworld/bench.py --app wave --tasks startup,open,roll,fx,drag,dropouts
.venv\Scripts\python perf/realworld/bench.py --app fl   --tasks startup,open,roll,fx,drag,dropouts
.venv\Scripts\python perf/realworld/table.py
```

The script takes over the mouse and keyboard and kills both apps between tasks. Run it on a
1920×1080 display with nothing else on screen. Screenshots of each task's end state are saved in
`perf/results/realworld-proof/`, so you can check the clicks landed where they should.

## The projects

Both apps open a song of the same size:

* **FL Studio:** the bundled demo *WiseLabs – Nahash*. It has 89 channels, about 82 patterns,
  5,483 notes, 444 playlist items and about 120 native effects. Its instruments are all native
  (Harmor, samplers), so no third-party plugins are involved.
* **WAVE:** `wave-perf gen song`. It has 89 channels (14 built-in synths, 75 samplers),
  82 patterns, 5,545 notes, 442 playlist items, 122 mixer effects on 41 inserts and 23 MB of
  audio.

The content still differs: FL's Harmor patches cost more to load than WAVE's synth presets.
The table compares how long each app makes you wait for a song of this size, not identical
DSP.

## What each row measures

| Row | Start → end |
|---|---|
| Cold startup | process launch → the window shows the app and has not changed for 2 s while answering messages (no project, OS file cache warm) |
| Open a large project | the project is handed to the running app (as a double-click does) → loaded, quiet for 3 s and answering messages |
| Open piano roll | F7 with a synth channel selected → the editor has finished drawing |
| Add an effect and open its UI | the final pick (FL: *Fruity Parametric EQ 2* in a mixer slot's menu; WAVE: Enter in the effect picker on *Parametric EQ*) → the effect window has finished drawing |
| Move clips | each 48 px mouse move while dragging a clip → the first screen frame showing it moved (30 moves, median / p95) |
| Audio dropouts | the song plays while the script edits for 10 minutes (piano roll, mixer, clip drags with undo, scrolling); gaps of exact digital silence of at least 1.5 ms, between loud audio, in the loopback recording |

Every row includes the display's composition (up to one frame at the display's refresh rate),
the same for both apps.

## Audio settings for the dropout row

Both apps run at a 512-sample buffer. WAVE uses WASAPI shared mode at the device rate.
FL Studio uses *FL Studio ASIO* at 512 (its default) with multithreaded generator and mixer
processing on (its defaults). FL's *Primary Sound Driver* at a 4096 buffer would hide dropouts
behind 8× more latency.

## Latest results

2026-11-16, Windows 11, i5-12400F (6 cores / 12 threads), 32 GB, RTX 5060 Ti, 1920×1080 at
144 Hz. WAVE 0.9.17+ (this branch) vs FL Studio 20.7.2. Medians of 5 runs (3 for the effect,
60 moves for the drag).

| Real-world task | WAVE | FL Studio |
|---|---:|---:|
| Cold startup (launch to idle UI) | 1.32 s | 1.64 s |
| Open a large project | 0.44 s | 10.57 s |
| Open piano roll | 39 ms | 639 ms |
| Add an effect and open its UI | 59 ms | 575 ms |
| Move clips (input → screen, median / p95) | 17 / 25 ms | 16 / 18 ms |
| Audio dropouts during 10 min of editing | 0 | 0 |
