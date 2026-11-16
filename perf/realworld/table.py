"""Prints the WAVE vs FL Studio table from perf/results/realworld-{wave,fl}.json (Markdown)."""

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def load(app):
    p = ROOT / f"perf/results/realworld-{app}.json"
    return json.loads(p.read_text()) if p.exists() else {}


def ms(v):
    if v is None:
        return "–"
    return f"{v / 1000:.2f} s" if v >= 1000 else f"{v:.0f} ms"


def p50(d, *path):
    for k in path:
        d = (d or {}).get(k)
    return None if not d or not d.get("n") else d["p50"]


def p95(d, *path):
    for k in path:
        d = (d or {}).get(k)
    return None if not d or not d.get("n") else d["p95"]


def row(task, w, f):
    return f"| {task} | {w} | {f} |"


def main():
    W, F = load("wave"), load("fl")
    rows = [
        row("Cold startup (launch to idle UI)", ms(p50(W, "startup", "ready_ms")), ms(p50(F, "startup", "ready_ms"))),
        row("Open a large project", ms(p50(W, "open", "loaded_ms")), ms(p50(F, "open", "loaded_ms"))),
        row("Open piano roll", ms(p50(W, "roll", "open_ms")), ms(p50(F, "roll", "open_ms"))),
        row("Add an effect and open its UI", ms(p50(W, "fx", "ready_ms")), ms(p50(F, "fx", "ready_ms"))),
        row(
            "Move clips (input → screen, median / p95)",
            f"{ms(p50(W, 'drag', 'clip_ms'))} / {ms(p95(W, 'drag', 'clip_ms'))}",
            f"{ms(p50(F, 'drag', 'clip_ms'))} / {ms(p95(F, 'drag', 'clip_ms'))}",
        ),
    ]
    if W.get("dropouts") or F.get("dropouts"):
        d = lambda x: "–" if not x.get("dropouts") else f"{x['dropouts']['dropouts']} in {x['dropouts']['seconds'] / 60:.0f} min"
        rows.append(row("Audio dropouts during 10 min of editing", d(W), d(F)))
    print("| Real-world task | WAVE | FL Studio |\n|---|---:|---:|")
    print("\n".join(rows))


if __name__ == "__main__":
    sys.exit(main())
