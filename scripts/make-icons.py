"""Build Synkopa's icons from the logo's geometry.

The mark is nine rectangles (the same ones as app/icons/synkopa-logo.svg), so
the icons are drawn here instead of being resampled from a bitmap: every size
starts from a 4x supersampled drawing and stays sharp.

Output: app/icons/{icon.png, source.png, 32x32.png, 128x128.png,
        128x128@2x.png, icon.ico, wave-project.ico}, ui/public/favicon.png,
        site/img/icon.png and site/img/social-preview.png (1280x640: the
        repository's social preview on GitHub and the site's link preview).

`wave-project.ico` keeps its name from before the rename: the installer and
the registry entries of existing installs point at that file.

Re-runnable: `python scripts/make-icons.py` (needs Pillow).
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
ICONS = ROOT / "app" / "icons"
FAVICON = ROOT / "ui" / "public" / "favicon.png"
SITE_ICON = ROOT / "site" / "img" / "icon.png"
SOCIAL = ROOT / "site" / "img" / "social-preview.png"
# bold, regular: the first pair found is used (Windows, then common Linux fonts)
FONTS = [
    ("C:/Windows/Fonts/segoeuib.ttf", "C:/Windows/Fonts/segoeui.ttf"),
    ("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
]

AMBER = (253, 180, 0, 255)
GREY = (208, 208, 208, 255)
# The app icon has no tile behind it, so it sits on taskbars and browser tabs of either theme:
# a mid grey reads on both, the logo's own light grey would vanish on a light one.
ICON_GREY = (154, 157, 165, 255)
BACKGROUND = (14, 16, 19, 255)  # the app's window background
PAGE = (19, 22, 27, 255)
PAGE_EDGE = (110, 116, 128, 255)

# x, y, width, height, colour: in the 1254-unit square of the logo source
MARK = [
    (498, 219, 370, 73, AMBER),
    (363, 327, 460, 73, AMBER),
    (316, 435, 246, 71, GREY),
    (468, 533, 83, 181, AMBER),
    (586, 471, 81, 304, AMBER),
    (702, 533, 83, 181, AMBER),
    (692, 747, 247, 71, GREY),
    (421, 850, 470, 73, AMBER),
    (363, 958, 393, 74, AMBER),
]
MARK_BOX = (316, 219, 939, 1032)  # left, top, right, bottom of the rectangles above
SS = 4  # supersampling
ICO_SIZES = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]


def draw_mark(draw: ImageDraw.ImageDraw, cx: float, cy: float, height: float, grey=GREY) -> None:
    """The mark, `height` pixels tall, centred on (cx, cy)."""
    left, top, right, bottom = MARK_BOX
    k = height / (bottom - top)
    ox = cx - (left + right) / 2 * k
    oy = cy - (top + bottom) / 2 * k
    for x, y, w, h, colour in MARK:
        fill = grey if colour == GREY else colour
        draw.rectangle([ox + x * k, oy + y * k, ox + (x + w) * k - 1, oy + (y + h) * k - 1], fill=fill)


def app_icon(size: int) -> Image.Image:
    """The mark alone on a transparent background, filling the square's height."""
    s = size * SS
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    draw_mark(draw, s / 2, s / 2, s * 0.94, ICON_GREY)
    return img.resize((size, size), Image.LANCZOS)


def project_icon(size: int) -> Image.Image:
    """A page with a folded corner carrying the mark: a Synkopa project file."""
    s = size * SS
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    left, top, right, bottom, fold = s * 0.14, s * 0.04, s * 0.86, s * 0.96, s * 0.26
    page = [(left, top), (right - fold, top), (right, top + fold), (right, bottom), (left, bottom)]
    draw.polygon(page, fill=PAGE)
    draw.line(page + [page[0]], fill=PAGE_EDGE, width=max(SS, round(s * 0.025)), joint="curve")
    draw.polygon([(right - fold, top), (right, top + fold), (right - fold, top + fold)], fill=AMBER)
    draw_mark(draw, s / 2, s * 0.6, s * 0.5)
    return img.resize((size, size), Image.LANCZOS)


def social_preview() -> Image.Image | None:
    """The mark, the name and one line about the product on the app's background."""
    pair = next((p for p in FONTS if all(Path(f).is_file() for f in p)), None)
    if pair is None:
        return None
    w, h = 1280 * 2, 640 * 2
    img = Image.new("RGBA", (w, h), BACKGROUND)
    draw = ImageDraw.Draw(img)
    draw_mark(draw, w * 0.19, h / 2, h * 0.56)
    x = w * 0.36
    draw.text((x, h * 0.30), "Synkopa", font=ImageFont.truetype(pair[0], 250), fill=(244, 244, 246, 255))
    regular = ImageFont.truetype(pair[1], 62)
    draw.text((x + 8, h * 0.56), "Open-source DAW with an FL Studio workflow", font=regular, fill=(176, 180, 190, 255))
    draw.text((x + 8, h * 0.64), "Windows · Linux · scriptable by AI agents", font=regular, fill=AMBER)
    return img.resize((1280, 640), Image.LANCZOS).convert("RGB")


def save_ico(path: Path, render) -> None:
    images = [render(w) for w, _ in ICO_SIZES]
    images[-1].save(path, format="ICO", sizes=ICO_SIZES, append_images=images[:-1])


def main() -> None:
    ICONS.mkdir(parents=True, exist_ok=True)
    master = app_icon(1024)
    master.save(ICONS / "icon.png")
    master.save(ICONS / "source.png")
    app_icon(32).save(ICONS / "32x32.png")
    app_icon(128).save(ICONS / "128x128.png")
    app_icon(256).save(ICONS / "128x128@2x.png")
    save_ico(ICONS / "icon.ico", app_icon)
    save_ico(ICONS / "wave-project.ico", project_icon)
    app_icon(64).save(FAVICON)
    app_icon(256).save(SITE_ICON)
    social = social_preview()
    if social is None:
        print("no font found: site/img/social-preview.png was not rebuilt")
    else:
        social.save(SOCIAL, optimize=True)
    print("icons written to", ICONS, FAVICON, "and", SITE_ICON)


if __name__ == "__main__":
    main()
