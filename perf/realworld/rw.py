"""Real-world timings of a desktop DAW, measured from outside the app (Windows).

Everything here treats the app as a black box, so WAVE and FL Studio are measured by the
same code: input goes in through SendInput, the result is read off the screen with DXGI
Desktop Duplication (a new frame arrives whenever the desktop changes, so a change is
timestamped to within a millisecond plus the display's composition), and the sound comes
back through WASAPI loopback of the default output.
"""

from __future__ import annotations

import ctypes
import ctypes.wintypes as wt
import subprocess
import threading
import time
from dataclasses import dataclass

import numpy as np

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)  # coordinates in physical pixels
except Exception:
    user32.SetProcessDPIAware()

now = time.perf_counter


# ------------------------------------------------------------------ windows


@dataclass
class Win:
    hwnd: int
    pid: int
    title: str
    rect: tuple[int, int, int, int]  # left, top, right, bottom (screen)

    @property
    def w(self) -> int:
        return self.rect[2] - self.rect[0]

    @property
    def h(self) -> int:
        return self.rect[3] - self.rect[1]


def _rect(hwnd: int) -> tuple[int, int, int, int]:
    r = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    return r.left, r.top, r.right, r.bottom


def windows(pids: set[int] | None = None) -> list[Win]:
    out: list[Win] = []
    proto = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)

    def cb(hwnd, _):
        if not user32.IsWindowVisible(hwnd):
            return True
        pid = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        if pids is not None and pid.value not in pids:
            return True
        n = user32.GetWindowTextLengthW(hwnd)
        buf = ctypes.create_unicode_buffer(n + 1)
        user32.GetWindowTextW(hwnd, buf, n + 1)
        out.append(Win(hwnd, pid.value, buf.value, _rect(hwnd)))
        return True

    user32.EnumWindows(proto(cb), 0)
    return out


def process_ids(image: str) -> set[int]:
    r = subprocess.run(["tasklist", "/FI", f"IMAGENAME eq {image}", "/FO", "CSV", "/NH"], capture_output=True, text=True)
    pids = set()
    for line in r.stdout.splitlines():
        parts = [p.strip('"') for p in line.split('","')]
        if len(parts) > 1 and parts[1].isdigit():
            pids.add(int(parts[1]))
    return pids


def main_window(image: str, min_w: int = 800, min_h: int = 500) -> Win | None:
    big = [w for w in windows(process_ids(image)) if w.w >= min_w and w.h >= min_h]
    return max(big, key=lambda w: w.w * w.h) if big else None


def responsive(hwnd: int, timeout_ms: int = 200) -> bool:
    """The window's thread answers a message within `timeout_ms` (not hung, not busy)."""
    res = ctypes.c_size_t()
    ok = user32.SendMessageTimeoutW(hwnd, 0, 0, 0, 0x0002, timeout_ms, ctypes.byref(res))  # WM_NULL, SMTO_ABORTIFHUNG
    return bool(ok)


def focus(hwnd: int) -> None:
    # Alt tap lets SetForegroundWindow work from a background process
    key("alt")
    if user32.IsIconic(hwnd):
        user32.ShowWindow(hwnd, 9)  # SW_RESTORE (only a minimized window: it would un-maximize)
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.15)


def clear_desktop() -> None:
    """Minimizes every window, so an app that starts is on screen (a window opened by a
    background process does not come to the front by itself)."""
    subprocess.run(["powershell", "-NoProfile", "-Command", "(New-Object -ComObject Shell.Application).MinimizeAll()"], capture_output=True)
    time.sleep(1.0)


def maximize(hwnd: int) -> None:
    user32.ShowWindow(hwnd, 3)  # SW_MAXIMIZE
    time.sleep(0.3)


def kill(image: str) -> None:
    subprocess.run(["taskkill", "/F", "/IM", image], capture_output=True)
    t = now()
    while process_ids(image) and now() - t < 10:
        time.sleep(0.1)


def close(hwnd: int) -> None:
    user32.PostMessageW(hwnd, 0x0010, 0, 0)  # WM_CLOSE


# ------------------------------------------------------------------ input

INPUT_MOUSE, INPUT_KEYBOARD = 0, 1
KEYEVENTF_KEYUP = 0x0002
MOUSEEVENTF_MOVE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP = 0x0001, 0x0002, 0x0004
MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEEVENTF_ABSOLUTE = 0x0008, 0x0010, 0x0800, 0x8000


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [("dx", wt.LONG), ("dy", wt.LONG), ("mouseData", wt.DWORD), ("dwFlags", wt.DWORD), ("time", wt.DWORD), ("dwExtraInfo", ctypes.c_size_t)]


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [("wVk", wt.WORD), ("wScan", wt.WORD), ("dwFlags", wt.DWORD), ("time", wt.DWORD), ("dwExtraInfo", ctypes.c_size_t)]


class _U(ctypes.Union):
    _fields_ = [("mi", MOUSEINPUT), ("ki", KEYBDINPUT), ("pad", ctypes.c_byte * 32)]


class INPUT(ctypes.Structure):
    _fields_ = [("type", wt.DWORD), ("u", _U)]


def _send(*inputs: INPUT) -> float:
    arr = (INPUT * len(inputs))(*inputs)
    t = now()
    user32.SendInput(len(inputs), arr, ctypes.sizeof(INPUT))
    return t


VK = {
    "alt": 0x12, "ctrl": 0x11, "shift": 0x10, "esc": 0x1B, "enter": 0x0D, "space": 0x20, "tab": 0x09,
    "f1": 0x70, "f2": 0x71, "f3": 0x72, "f4": 0x73, "f5": 0x74, "f6": 0x75, "f7": 0x76, "f8": 0x77, "f9": 0x78,
    "f10": 0x79, "f11": 0x7A, "f12": 0x7B, "left": 0x25, "up": 0x26, "right": 0x27, "down": 0x28, "delete": 0x2E,
    "home": 0x24, "end": 0x23, "pgup": 0x21, "pgdn": 0x22, "backspace": 0x08,
}


def _vk(k: str) -> int:
    return VK[k] if k in VK else ord(k.upper())


def key(combo: str) -> float:
    """Presses e.g. "f7", "shift+f9", "ctrl+z"; returns the time the key went down."""
    keys = [_vk(k) for k in combo.lower().split("+")]
    downs = [INPUT(INPUT_KEYBOARD, _U(ki=KEYBDINPUT(k, 0, 0, 0, 0))) for k in keys]
    ups = [INPUT(INPUT_KEYBOARD, _U(ki=KEYBDINPUT(k, 0, KEYEVENTF_KEYUP, 0, 0))) for k in reversed(keys)]
    t = _send(*downs)
    time.sleep(0.02)
    _send(*ups)
    return t + 0  # the press of the last key is what the app reacts to


def type_text(s: str) -> None:
    for ch in s:
        i = INPUT(INPUT_KEYBOARD, _U(ki=KEYBDINPUT(0, ord(ch), 0x0004, 0, 0)))  # KEYEVENTF_UNICODE
        j = INPUT(INPUT_KEYBOARD, _U(ki=KEYBDINPUT(0, ord(ch), 0x0004 | KEYEVENTF_KEYUP, 0, 0)))
        _send(i, j)
        time.sleep(0.01)


def _abs(x: int, y: int) -> tuple[int, int]:
    sw, sh = user32.GetSystemMetrics(0), user32.GetSystemMetrics(1)
    return int(x * 65535 / (sw - 1)), int(y * 65535 / (sh - 1))


def move(x: int, y: int) -> float:
    ax, ay = _abs(x, y)
    return _send(INPUT(INPUT_MOUSE, _U(mi=MOUSEINPUT(ax, ay, 0, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE, 0, 0))))


def down(right: bool = False) -> float:
    return _send(INPUT(INPUT_MOUSE, _U(mi=MOUSEINPUT(0, 0, 0, MOUSEEVENTF_RIGHTDOWN if right else MOUSEEVENTF_LEFTDOWN, 0, 0))))


def up(right: bool = False) -> float:
    return _send(INPUT(INPUT_MOUSE, _U(mi=MOUSEINPUT(0, 0, 0, MOUSEEVENTF_RIGHTUP if right else MOUSEEVENTF_LEFTUP, 0, 0))))


def click(x: int, y: int, right: bool = False, double: bool = False) -> float:
    move(x, y)
    time.sleep(0.03)
    t = down(right)
    time.sleep(0.02)
    up(right)
    if double:
        time.sleep(0.05)
        t = down(right)
        time.sleep(0.02)
        up(right)
    return t


def wheel(x: int, y: int, clicks: int) -> float:
    move(x, y)
    return _send(INPUT(INPUT_MOUSE, _U(mi=MOUSEINPUT(0, 0, ctypes.c_ulong(120 * clicks).value, MOUSEEVENTF_WHEEL, 0, 0))))


# ------------------------------------------------------------------ screen


class Screen:
    """Desktop Duplication: `grab` returns a new frame only when the desktop changed."""

    def __init__(self) -> None:
        import dxcam

        self.cam = dxcam.create(output_color="GRAY")
        self.w, self.h = user32.GetSystemMetrics(0), user32.GetSystemMetrics(1)
        self.last = None
        self._frame()
        if self.last is None:
            # a static desktop presents no new frame: start from a GDI copy of the screen
            import mss

            with mss.MSS() as m:
                a = np.asarray(m.grab(m.monitors[1]))
            self.last = (0.114 * a[:, :, 0] + 0.587 * a[:, :, 1] + 0.299 * a[:, :, 2]).astype(np.uint8)

    def _force_frame(self) -> None:
        # nudging the cursor by one pixel makes the compositor present a frame
        p = wt.POINT()
        user32.GetCursorPos(ctypes.byref(p))
        user32.SetCursorPos(p.x + 1, p.y)
        user32.SetCursorPos(p.x, p.y)
        time.sleep(0.02)
        self._frame()

    def _frame(self):
        f = self.cam.grab()
        if f is not None:
            self.last = f[:, :, 0] if f.ndim == 3 else f
            return self.last
        return None

    def snapshot(self) -> np.ndarray:
        self._frame()
        return self.last.copy()

    @staticmethod
    def _crop(f: np.ndarray, r: tuple[int, int, int, int]) -> np.ndarray:
        l, t, rr, b = r
        return f[max(t, 0):b, max(l, 0):rr]

    def changed(self, a: np.ndarray, b: np.ndarray, region, min_px: int) -> int:
        d = np.abs(self._crop(a, region).astype(np.int16) - self._crop(b, region).astype(np.int16)) > 12
        n = int(d.sum())
        return n if n >= min_px else 0

    def wait_change(self, region, base: np.ndarray, since: float, timeout: float = 5.0, min_px: int = 30) -> float | None:
        """Seconds from `since` to the first frame whose `region` differs from `base`."""
        while now() - since < timeout:
            f = self._frame()
            if f is not None and self.changed(base, f, region, min_px):
                return now() - since
            time.sleep(0.0002)
        return None

    def wait_settle(self, region, since: float, quiet: float = 1.0, timeout: float = 60.0, min_px: int = 400, first=None, hwnd=None):
        """Seconds from `since` to the last visible change in `region` before `quiet` seconds
        without one (changes under `min_px` pixels, such as a blinking caret, are ignored).
        With `hwnd`, a window that does not answer messages counts as still changing: an app
        busy loading paints nothing, which is not the same as done."""
        ref = self.snapshot() if first is None else first
        last_change = None
        t_last_frame = now()
        t_ping = 0.0
        while now() - since < timeout:
            f = self._frame()
            t = now()
            if f is not None:
                if self.changed(ref, f, region, min_px):
                    last_change = t
                    ref = f.copy()
                t_last_frame = t
            if hwnd is not None and t - t_ping > 0.1:
                t_ping = t
                if not responsive(hwnd, 50):
                    last_change = now()
                    t_last_frame = last_change
            if last_change is not None and t - last_change >= quiet:
                return last_change - since
            if last_change is None and t - since >= quiet and t - t_last_frame >= quiet:
                return None  # nothing ever changed
            time.sleep(0.001)
        return None


# ------------------------------------------------------------------ sound


class Loopback:
    """Records what the default output plays (WASAPI loopback through PyAudioWPatch), as a
    float32 (frames, channels) array at the device rate."""

    def __init__(self) -> None:
        import pyaudiowpatch as pa

        self.pa = pa.PyAudio()
        wasapi = self.pa.get_host_api_info_by_type(pa.paWASAPI)
        spk = self.pa.get_device_info_by_index(wasapi["defaultOutputDevice"])
        self.dev = next(d for d in self.pa.get_loopback_device_info_generator() if spk["name"] in d["name"])
        self.sr = int(self.dev["defaultSampleRate"])
        self.ch = int(self.dev["maxInputChannels"])
        self.chunks: list[np.ndarray] = []
        self.stream = None

    def start(self) -> None:
        import pyaudiowpatch as pa

        def cb(data, n, ti, st):
            self.chunks.append(np.frombuffer(data, np.float32).copy())
            return (None, pa.paContinue)

        self.stream = self.pa.open(format=pa.paFloat32, channels=self.ch, rate=self.sr, input=True,
                                   input_device_index=self.dev["index"], frames_per_buffer=1024, stream_callback=cb)

    def stop(self) -> np.ndarray:
        if self.stream:
            self.stream.stop_stream()
            self.stream.close()
        self.pa.terminate()
        a = np.concatenate(self.chunks) if self.chunks else np.zeros(0, np.float32)
        return a.reshape(-1, self.ch)

def find_dropouts(x: np.ndarray, sr: int = 48000, min_gap_ms: float = 1.5, loud_db: float = -35.0) -> list[dict]:
    """Gaps of exact digital silence inside otherwise loud audio: what a missed audio
    deadline leaves in a shared-mode stream (the engine plays zeros for the late buffer).
    A gap counts when both sides, 30 ms each, are louder than `loud_db` RMS, so the
    music's own rests and the song's start and end are not counted."""
    if len(x) == 0:
        return []
    m = np.abs(x).max(axis=1)
    zero = m < 1e-7
    edges = np.flatnonzero(np.diff(zero.astype(np.int8)))
    runs = []
    start = 0 if zero[0] else None
    for e in edges:
        if zero[e + 1]:
            start = e + 1
        elif start is not None:
            runs.append((start, e + 1))
            start = None
    w = int(0.03 * sr)
    thr = 10 ** (loud_db / 20)
    out = []
    for a, b in runs:
        if (b - a) < min_gap_ms * sr / 1000 or a < w or b + w > len(x):
            continue
        before = np.sqrt(np.mean(x[a - w:a] ** 2))
        after = np.sqrt(np.mean(x[b:b + w] ** 2))
        if before > thr and after > thr:
            out.append({"at_s": round(a / sr, 3), "ms": round((b - a) * 1000 / sr, 2)})
    return out


def stats(v: list[float]) -> dict:
    v = sorted(x for x in v if x is not None)
    if not v:
        return {"n": 0}
    q = lambda p: v[min(len(v) - 1, round((len(v) - 1) * p))]
    return {"n": len(v), "min": round(v[0], 1), "p50": round(q(0.5), 1), "p95": round(q(0.95), 1), "max": round(v[-1], 1)}
