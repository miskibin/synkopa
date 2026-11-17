"""Synkopa vs FL Studio: real-world task timings on this machine.

    python perf/realworld/bench.py --app wave|fl --tasks startup,open,roll,fx,drag,dropouts [--runs 5]

Run on an otherwise idle desktop at 1920x1080, nothing over the app (the script takes the
mouse and keyboard). Results go to perf/results/realworld-<app>.json; `table.py` prints the
comparison. What each task measures and how the two apps are set up is in README.md.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

import numpy as np

import rw

ROOT = Path(__file__).resolve().parents[2]
SONG = ROOT / "perf/.cache/song/song.wave"
FL_EXE = Path(os.environ.get("FL_EXE", r"C:\Program Files\Image-Line\FL Studio 20\FL64.exe"))
FL_SONG = FL_EXE.parent / "Data/Demo projects/Demo songs/WiseLabs - Nahash (Original Mix).flp"
WAVE_EXE = Path(os.environ.get("WAVE_EXE", ROOT / "target/release/wave-app.exe"))
WAVE_SETTINGS = Path(os.environ["LOCALAPPDATA"]) / "WAVE/settings.json"

# toasts (bottom right) and the taskbar never count as the app's own changes
SCREEN = (0, 0, 1920, 1030)


def region_without_toasts(r):
    return r


class App:
    image: str
    exe: Path
    song: Path

    def __init__(self, scr: rw.Screen):
        self.scr = scr

    def win(self) -> rw.Win:
        w = rw.main_window(self.image)
        assert w, f"{self.image} has no window"
        return w

    def front(self):
        rw.focus(self.win().hwnd)

    def launch(self, *args) -> subprocess.Popen:
        return subprocess.Popen([str(self.exe), *map(str, args)])

    def prepare_cold(self):
        pass

    def after_open(self):
        pass


class Wave(App):
    image = "wave-app.exe"
    exe = WAVE_EXE
    song = SONG

    def prepare_cold(self):
        # the song exactly as generated (an earlier run may have saved edits into it)
        pristine = SONG.with_suffix(".pristine")
        if not pristine.exists():
            shutil.copyfile(SONG, pristine)
        shutil.copyfile(pristine, SONG)
        # a launch with no project to reopen (like FL Studio starting on its template)
        s = json.loads(WAVE_SETTINGS.read_text(encoding="utf-8"))
        s["lastProject"] = None
        WAVE_SETTINGS.write_text(json.dumps(s, indent=2), encoding="utf-8")

    def after_open(self):
        # the Channel rack window, open on top of the Playlist after a load: F6 closes it
        rw.key("f6")
        time.sleep(0.5)

    def setup_roll(self):
        # select a synth channel in the Channel rack: click its name (opens the synth), close
        # the synth, hide the rack
        rw.key("f6")
        time.sleep(0.8)
        rw.click(175, 151)  # Pad Cieply
        time.sleep(0.8)
        rw.key("esc")
        time.sleep(0.4)
        rw.key("f6")
        time.sleep(0.6)

    def roll_key(self):
        return "f7"

    def play(self):
        rw.click(497, 27)  # the Play button (Space would press the focused SONG button again)

    def song_mode(self):
        rw.click(747, 33)  # SONG (the mode is a saved preference, PAT by default like FL's)
        time.sleep(0.4)

    def fx_prepare(self, k: int):
        rw.key("shift+f9")
        time.sleep(0.8)
        rw.type_text("parametric")
        time.sleep(0.6)

    def fx_commit(self) -> float:
        return rw.key("enter")

    def fx_close(self):
        rw.click(900, 80)  # the effect window's close button (where it opens, maximized)
        time.sleep(0.5)


class Fl(App):
    image = "FL64.exe"
    exe = FL_EXE
    song = FL_SONG

    def after_open(self):
        rw.click(960, 520)  # the demo project's info card
        time.sleep(0.5)
        rw.key("f6")  # the Channel rack, open over the Playlist after a load
        time.sleep(0.5)

    def setup_roll(self):
        # select the Harmor channel in the Channel rack (its selection LED), rack closed after
        rw.key("f6")
        time.sleep(0.6)
        rw.click(652, 949)
        time.sleep(0.3)
        rw.key("f6")
        time.sleep(0.5)

    def roll_key(self):
        return "f7"

    def play(self):
        rw.key("space")

    def song_mode(self):
        pass  # a demo project opens in SONG mode

    SLOTS = [428, 465, 502, 539]

    def fx_prepare(self, k: int):
        rw.click(1720, self.SLOTS[k % 4])  # an empty slot of the master: its menu lists the effects
        time.sleep(0.8)

    def fx_commit(self) -> float:
        return rw.click(1308, 833)  # Fruity Parametric EQ 2

    def fx_close(self):
        rw.key("esc")
        time.sleep(0.5)


# ------------------------------------------------------------------ tasks


def proof(app: App, name: str):
    """What the screen showed after a task, to check the clicks landed where they should."""
    import cv2

    d = ROOT / "perf/results/realworld-proof"
    d.mkdir(parents=True, exist_ok=True)
    cv2.imwrite(str(d / f"{app.image.split(chr(46))[0]}-{name}.png"), app.scr.snapshot())


def park():
    """The pointer over the taskbar, outside the measured area: a tooltip under it would count."""
    rw.move(150, 1060)  # (left of the taskbar icons: hovering an icon pops up its thumbnail)


def settle_window(app: App, since: float, quiet: float, timeout: float, min_px: int, first=None):
    return app.scr.wait_settle(SCREEN, since, quiet=quiet, timeout=timeout, min_px=min_px, first=first)


def task_startup(app: App, runs: int) -> dict:
    out = []
    for i in range(runs):
        kill_both()
        app.prepare_cold()
        rw.clear_desktop()
        time.sleep(2)
        base = app.scr.snapshot()
        t0 = rw.now()
        app.launch()
        w = None
        while rw.now() - t0 < 60 and w is None:
            w = rw.main_window(app.image)
            time.sleep(0.01)
        t_win = rw.now() - t0
        last = app.scr.wait_settle(SCREEN, t0, quiet=2.0, timeout=90, min_px=3000, first=base, hwnd=w.hwnd)
        ready = max(t_win, last or 0)
        ok = rw.responsive(rw.main_window(app.image).hwnd)
        out.append({"window_s": round(t_win, 3), "ready_s": round(ready, 3), "responsive": ok})
        print(f"  startup {i + 1}: window {t_win:.2f} s, ready {ready:.2f} s", flush=True)
    return {"runs": out, "ready_ms": rw.stats([r["ready_s"] * 1000 for r in out])}


def kill_both():
    for im in ("wave-app.exe", "FL64.exe"):
        rw.kill(im)


def task_open(app: App, runs: int) -> dict:
    out = []
    for i in range(runs):
        kill_both()
        app.prepare_cold()
        rw.clear_desktop()
        app.launch()
        time.sleep(1)
        while rw.main_window(app.image) is None:
            time.sleep(0.1)
        time.sleep(8)  # let it finish starting
        app.front()
        rw.maximize(app.win().hwnd)
        time.sleep(2)
        hwnd = app.win().hwnd
        base = app.scr.snapshot()
        t0 = rw.now()
        app.launch(app.song)  # handed to the running instance, as a double-click would
        last = app.scr.wait_settle(SCREEN, t0, quiet=3.0, timeout=120, min_px=3000, first=base, hwnd=hwnd)
        ok = rw.responsive(app.win().hwnd)
        out.append({"loaded_s": round(last or -1, 3), "responsive": ok})
        print(f"  open {i + 1}: {last:.2f} s", flush=True)
    return {"runs": out, "loaded_ms": rw.stats([r["loaded_s"] * 1000 for r in out if r["loaded_s"] > 0])}


def ensure_song_open(app: App):
    """A fresh instance with the song open, maximized, in front: every task starts the same."""
    kill_both()
    app.prepare_cold()
    rw.clear_desktop()
    app.launch(app.song)
    time.sleep(3)
    while rw.main_window(app.image) is None:
        time.sleep(0.2)
    time.sleep(15)
    for k in range(4):
        app.front()
        w = app.win()
        if w.w >= 1900:
            break
        if app.image == "wave-app.exe":
            # Synkopa draws its own title bar: its maximize button (SW_MAXIMIZE does not stick)
            rw.click(w.rect[2] - 70, w.rect[1] + 27)
        else:
            rw.maximize(w.hwnd)
        time.sleep(1.5)
    assert app.win().w >= 1900, "could not maximize the window"
    app.after_open()
    app.front()
    time.sleep(0.5)


def task_roll(app: App, runs: int) -> dict:
    ensure_song_open(app)
    app.setup_roll()
    opens, closes = [], []
    for i in range(runs):
        park()
        time.sleep(1.0)
        base = app.scr.snapshot()
        t0 = rw.key(app.roll_key())
        a = settle_window(app, t0, quiet=0.8, timeout=10, min_px=2000, first=base)
        time.sleep(0.6)
        base = app.scr.snapshot()
        t1 = rw.key(app.roll_key())
        b = settle_window(app, t1, quiet=0.8, timeout=10, min_px=2000, first=base)
        opens.append(a and a * 1000)
        closes.append(b and b * 1000)
        print(f"  piano roll {i + 1}: open {a and a * 1000:.0f} ms, close {b and b * 1000:.0f} ms", flush=True)
        if i == 0:
            rw.key(app.roll_key()); time.sleep(1.5); proof(app, "roll"); rw.key(app.roll_key()); time.sleep(1)
    return {"open_ms": rw.stats(opens), "close_ms": rw.stats(closes)}


def task_fx(app: App, runs: int) -> dict:
    ensure_song_open(app)
    rw.key("f9")
    time.sleep(1.5)
    res = []
    for i in range(runs):
        app.fx_prepare(i)
        if app.image == "wave-app.exe":
            park()  # (FL's pick is a click: the pointer is on the menu)
            time.sleep(0.3)
        base = app.scr.snapshot()
        t0 = app.fx_commit()
        first = app.scr.wait_change(SCREEN, base, t0, timeout=5, min_px=2000)
        done = settle_window(app, t0, quiet=0.8, timeout=10, min_px=2000, first=base)
        res.append({"first_ms": first and first * 1000, "ready_ms": done and done * 1000})
        if i == 0:
            time.sleep(1.0); proof(app, "fx")
        print(f"  fx {i + 1}: first change {first and first * 1000:.0f} ms, window ready {done and done * 1000:.0f} ms", flush=True)
        app.fx_close()
        time.sleep(0.8)
    rw.key("f9")
    return {"first_ms": rw.stats([r["first_ms"] for r in res]), "ready_ms": rw.stats([r["ready_ms"] for r in res])}


def drag_latency(app: App, x: int, y: int, steps: int = 30, dx: int = 48) -> list[float]:
    """Holds the mouse on (x, y) and moves it `dx` px at a time; for each move the time until
    the screen around it changes."""
    reg = (x - 400, y - 120, x + 400, y + 120)
    rw.move(x, y)
    time.sleep(0.3)
    rw.down()
    time.sleep(0.3)
    lat = []
    pos = x
    for k in range(steps):
        d = dx if (k // 5) % 2 == 0 else -dx  # every move crosses a snap line
        pos += d
        base = app.scr.snapshot()
        t = rw.move(pos, y)
        v = app.scr.wait_change(reg, base, t, timeout=0.5, min_px=40)
        lat.append(v * 1000 if v is not None else None)
        time.sleep(0.12)
    rw.up()
    time.sleep(0.3)
    rw.key("ctrl+z")
    return lat


def task_drag(app: App, runs: int, clip: tuple[int, int], note: tuple[int, int] | None) -> dict:
    ensure_song_open(app)
    out = {}
    lat = []
    for _ in range(max(1, runs // 2)):
        lat += drag_latency(app, *clip)
    proof(app, "drag")
    out["clip_ms"] = rw.stats(lat)
    out["clip_missed"] = sum(v is None for v in lat)
    print(f"  clip drag: {out['clip_ms']}", flush=True)
    if note:
        app.setup_roll()
        rw.key(app.roll_key())
        time.sleep(1.5)
        lat = []
        for _ in range(max(1, runs // 2)):
            lat += drag_latency(app, *note)
        rw.key(app.roll_key())
        out["note_ms"] = rw.stats(lat)
        out["note_missed"] = sum(v is None for v in lat)
        print(f"  note drag: {out['note_ms']}", flush=True)
    return out


def task_dropouts(app: App, minutes: float, clip: tuple[int, int]) -> dict:
    """Plays the song while the script edits for `minutes`; counts the gaps in the output."""
    ensure_song_open(app)
    app.setup_roll()
    rec = rw.Loopback()
    rec.start()
    time.sleep(1)
    app.song_mode()
    app.play()
    t_end = rw.now() + minutes * 60
    actions = 0
    while rw.now() < t_end:
        # one round of a producer's editing: piano roll, mixer, a clip moved and undone, scrolling
        rw.key(app.roll_key()); time.sleep(1.2)
        rw.key(app.roll_key()); time.sleep(0.8)
        rw.key("f9"); time.sleep(1.2)
        rw.key("f9"); time.sleep(0.8)
        x, y = clip
        rw.move(x, y); time.sleep(0.1); rw.down()
        for k in range(20):
            rw.move(x + k * 5, y); time.sleep(0.03)
        rw.up(); time.sleep(0.3)
        rw.key("ctrl+z"); time.sleep(0.6)
        rw.wheel(1100, 600, -3); time.sleep(0.4)
        rw.wheel(1100, 600, 3); time.sleep(0.6)
        actions += 1
    app.play()
    time.sleep(0.5)
    audio = rec.stop()
    gaps = rw.find_dropouts(audio, rec.sr)
    loud = float(np.mean(np.abs(audio).max(axis=1) > 1e-4)) if len(audio) else 0
    print(f"  dropouts: {len(gaps)} in {len(audio) / rec.sr:.0f} s of output ({loud:.0%} non-silent), {actions} edit rounds", flush=True)
    return {"dropouts": len(gaps), "gaps": gaps[:50], "seconds": round(len(audio) / rec.sr, 1), "non_silent": round(loud, 3), "edit_rounds": actions}


COORDS = {
    # (clip in the Playlist, note in the Piano roll), screen pixels at 1920x1080, maximized
    "wave": {"clip": (600, 530), "note": None},
    "fl": {"clip": (1300, 376), "note": None}  # the clip's title bar (its body edits the curve),
}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--app", choices=["wave", "fl"], required=True)
    ap.add_argument("--tasks", default="startup,open,roll,fx,drag")
    ap.add_argument("--runs", type=int, default=5)
    ap.add_argument("--minutes", type=float, default=10)
    ap.add_argument("--out", default=None)
    a = ap.parse_args()
    scr = rw.Screen()
    app = Wave(scr) if a.app == "wave" else Fl(scr)
    out_path = Path(a.out or ROOT / f"perf/results/realworld-{a.app}.json")
    res = json.loads(out_path.read_text()) if out_path.exists() else {}
    res["meta"] = {"date": time.strftime("%Y-%m-%d %H:%M"), "exe": str(app.exe), "song": str(app.song)}
    c = COORDS[a.app]
    for t in a.tasks.split(","):
        print(f"[{a.app}] {t}", flush=True)
        if t == "startup":
            res[t] = task_startup(app, a.runs)
        elif t == "open":
            res[t] = task_open(app, a.runs)
        elif t == "roll":
            res[t] = task_roll(app, a.runs)
        elif t == "fx":
            res[t] = task_fx(app, min(a.runs, 3))  # FL: three free master slots line up with the menu
        elif t == "drag":
            res[t] = task_drag(app, a.runs, c["clip"], c["note"])
        elif t == "dropouts":
            res[t] = task_dropouts(app, a.minutes, c["clip"])
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(json.dumps(res, indent=2))
    print(f"wrote {out_path}")


if __name__ == "__main__":
    main()
