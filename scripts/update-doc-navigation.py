"""Refresh the static sidebar and sitemap after adding or renaming user guides."""

import re
from html import escape
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GUIDES = ROOT / "site" / "docs"
GROUPS = {
    "Start here": [
        ("index", "Overview"),
        ("setup", "Install & set up"),
        ("first-song", "First song"),
        ("projects", "Save & recover projects"),
    ],
    "Build a song": [
        ("patterns", "Write patterns"),
        ("midi", "Set up & record MIDI"),
        ("instruments", "Play instruments"),
        ("arrangement", "Arrange a song"),
        ("audio-editing", "Edit audio clips"),
        ("audio-analysis", "Find chords & stems"),
    ],
    "Record & finish": [
        ("record", "Record & comp"),
        ("vocal-production", "Produce vocals"),
        ("mix", "Mix & route"),
        ("automation", "Automate controls"),
        ("mastering", "Master & measure"),
        ("export", "Export & share"),
    ],
    "Reference": [
        ("plugins", "Plugin guide"),
        ("dynamics", "EQ & dynamics"),
        ("creative-effects", "Creative & repair effects"),
        ("cli", "CLI & coding agents"),
        ("reference", "Shortcuts & help"),
    ],
}
NAV_PATTERN = re.compile(r'<nav aria-label="Guides">.*?</nav>', flags=re.S)
SITE_URL = "https://miskibin.github.io/synkopa/"


def navigation(current_page):
    lines = ['<nav aria-label="Guides">']
    for group, guides in GROUPS.items():
        lines.extend((f"          <p>{escape(group)}</p>", "          <ul>"))
        for page, label in guides:
            current = ' aria-current="page"' if page == current_page else ""
            lines.append(f'            <li><a href="{page}.html"{current}>{escape(label)}</a></li>')
        lines.append("          </ul>")
    lines.append("        </nav>")
    return "\n".join(lines)


def main():
    pages = [page for group in GROUPS.values() for page, _ in group]
    on_disk = {path.stem for path in GUIDES.glob("*.html")}
    if len(pages) != len(set(pages)) or set(pages) != on_disk:
        raise ValueError("Match each HTML page to one sidebar entry.")

    for page in pages:
        path = GUIDES / f"{page}.html"
        content = path.read_text(encoding="utf-8")
        updated, count = NAV_PATTERN.subn(lambda _: navigation(page), content)
        if count != 1:
            raise ValueError(f"Expected one guides sidebar in {path.name}")
        if updated != content:
            path.write_text(updated, encoding="utf-8")

    urls = [SITE_URL] + [f"{SITE_URL}docs/" + ("" if page == "index" else f"{page}.html") for page in pages]
    sitemap = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">',
        *(f"  <url><loc>{url}</loc></url>" for url in urls),
        "</urlset>",
    ]
    (ROOT / "site" / "sitemap.xml").write_text("\n".join(sitemap) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
