#!/usr/bin/env python3
"""Makes a project saved in WAVE the built-in demo project.

    python scripts/update-demo.py path/to/demo.wave

Save the edited demo with File -> Save (or its zip export, unpacked), then run this. It writes
crates/engine/src/demo_project.json: the project with every sample named by its standard-library
path (`lib:Standard/<file>`), so the demo opens on any computer. A sample that is not in the
standard library is an error: the demo ships no files of its own.

Afterwards regenerate the mock's copy and run the demo test:

    WAVE_UPDATE_FIXTURES=1 cargo test -p wave-engine --test sampler mock_fixture
    cargo test -p wave-engine --test sampler demo
"""
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LIBRARY = ROOT / "crates" / "engine" / "library"
OUT = ROOT / "crates" / "engine" / "src" / "demo_project.json"


def library_file(path: str) -> str:
    """`Audio/12-loop-vox.ogg` (a saved project's copy) or `lib:Standard/loop-vox.ogg` -> `loop-vox.ogg`."""
    name = path.replace("\\", "/").rsplit("/", 1)[-1]
    if not path.startswith("lib:"):
        name = re.sub(r"^\d+-", "", name)  # the number Save puts in front of a collected file
    if not (LIBRARY / name).is_file():
        sys.exit(f"{path}: {name} is not a standard-library sample (crates/engine/library)")
    return name


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    doc = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    if doc.get("format") != "wave-project":
        sys.exit("not a WAVE project file")
    p = doc["project"]
    used = {c["ref"] for c in p["clips"] if c["kind"] == "audio"}
    used |= {ch["source"] for ch in p["channels"] if ch.get("kind") == "sample"}
    dropped = [s["name"] for s in p["sources"] if s["id"] not in used]
    p["sources"] = [s for s in p["sources"] if s["id"] in used]
    for s in p["sources"]:
        s["path"] = "lib:Standard/" + library_file(s["path"])
    OUT.write_text(json.dumps(p, indent=1, ensure_ascii=False, sort_keys=True) + "\n", encoding="utf-8", newline="\n")
    print(f"{OUT.relative_to(ROOT)}: {len(p['tracks'])} tracks, {len(p['clips'])} clips, {len(p['sources'])} samples")
    if dropped:
        print("left out (used by no clip or channel):", ", ".join(dropped))


if __name__ == "__main__":
    main()
