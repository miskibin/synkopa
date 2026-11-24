"""Reference chords for `crates/chords` (`matches_the_python_reference`): librosa features and the
ONNX model's smoothed argmax, as ChordMini's runner computes them.

    python scripts/chords-reference.py SONG.mp3 OUT_DIR [--model chordnet.onnx] [--seconds 60]
    CHORDS_MODEL=chordnet.onnx CHORDS_REF=OUT_DIR cargo test -p wave-chords --release -- --nocapture

Needs librosa 0.11, numpy and onnxruntime.
"""
import argparse, os
import numpy as np, librosa, onnxruntime as ort

ap = argparse.ArgumentParser()
ap.add_argument("audio"); ap.add_argument("out")
ap.add_argument("--model", default=os.environ.get("CHORDS_MODEL", "chordnet.onnx"))
ap.add_argument("--seconds", type=float, default=60.0)
a = ap.parse_args()
os.makedirs(a.out, exist_ok=True)

y, _ = librosa.load(a.audio, sr=22050, mono=True, duration=a.seconds)
y = y.astype(np.float32)
C = np.abs(librosa.cqt(y, sr=22050, hop_length=2048, fmin=librosa.note_to_hz("C1"), n_bins=144, bins_per_octave=24))
F = np.log(C + 1e-6).T.astype(np.float32)  # [T, 144]

s = ort.InferenceSession(a.model)
T = F.shape[0]
W = -(-T // 108)
pad = np.zeros((((W + 15) // 16) * 16 * 108, 144), np.float32)
pad[:T] = F
X = pad.reshape(-1, 108, 144)
L = np.concatenate([s.run(None, {"features": X[g:g + 16]})[0] for g in range(0, len(X), 16)]).reshape(-1, 170)[:T]
k = np.ones(9) / 9
Ls = np.stack([np.convolve(L[:, c], k, "same") for c in range(170)], 1)

y.tofile(os.path.join(a.out, "audio22050.f32"))
F.tofile(os.path.join(a.out, "feats.f32"))
Ls.argmax(1).astype(np.uint16).tofile(os.path.join(a.out, "argmax.u16"))
print(f"{T} frames written to {a.out}")
