"""The palette and drawing helpers the sprite generator and its recipes share.

Nothing here holds state: the registry and the output directory belong to
gen_sprites, which hands them to each recipe it runs. Coordinates are in
sprite space; the helpers scale them by the supersample factor.
"""

from pathlib import Path

from PIL import Image, ImageDraw

SS = 4  # supersample factor
SIZE = 128  # a unit frame's edge

# The Aftermine palette. Keep in sync with kit/src/render.rs and the shell.
GROUND = (35, 35, 41)
GROUND_DARK = (28, 28, 34)
GROUND_LIGHT = (44, 44, 52)
SCRAP = (217, 164, 65)
SCRAP_DARK = (140, 106, 47)
SCRAP_LIGHT = (240, 200, 120)
IRON = (52, 52, 62)
IRON_DARK = (38, 38, 46)
IRON_LIGHT = (72, 72, 84)
BONE = (232, 228, 216)

# Machine materials.
VOID = (11, 11, 15)
DEEP = (24, 25, 31)
DARK = IRON_DARK
EDGE = (112, 111, 119)
STEEL = (143, 140, 135)
BRASS = (130, 105, 66)

# Accent palettes. "base" is the only one that ships. Every accented sprite
# is also rendered in "probe" so accent_masks() can find its tintable
# regions by diffing the two renders; probe renders never reach disk or the
# atlas.
PALETTES = {
    "base": {
        "base": (196, 87, 59),
        "dark": (126, 56, 38),
        "light": (232, 137, 107),
    },
    "probe": {
        "base": (63, 148, 130),
        "dark": (39, 96, 79),
        "light": (119, 196, 176),
    },
}
PROBE_TAG = "_probe"


def variant_tag(variant: str) -> str:
    """The sprite-name token for a palette variant: none for base."""
    if variant not in PALETTES:
        raise ValueError(f"unknown variant: {variant}")
    return "" if variant == "base" else PROBE_TAG


def save_sprite(image: Image.Image, out: Path, name: str) -> None:
    """Writes a shipping sprite; probe renders stay in memory only."""
    if PROBE_TAG not in name:
        image.save(out / f"{name}.png")


def s(v: float) -> int:
    """Scale a sprite-space coordinate by the supersample factor."""
    return round(v * SS)


def canvas(
    px: int = SIZE, color=(0, 0, 0, 0)
) -> tuple[Image.Image, ImageDraw.ImageDraw]:
    img = Image.new("RGBA", (px * SS, px * SS), color)
    return img, ImageDraw.Draw(img)


def rim_light(img: Image.Image) -> Image.Image:
    """A one-pixel warm edge along the top-left silhouette — keeps units
    readable at small zoom against dark ground."""
    from PIL import ImageChops, ImageFilter

    alpha = img.split()[3]
    grown = alpha.filter(ImageFilter.MaxFilter(3))
    edge = ImageChops.subtract(grown, alpha)
    shifted = ImageChops.subtract(
        edge, edge.transform(edge.size, Image.AFFINE, (1, 0, -1, 0, 1, -1))
    )
    rim = Image.new("RGBA", img.size, (255, 244, 224, 0))
    rim.putalpha(shifted.point(lambda v: min(v, 110)))
    out = img.copy()
    out.alpha_composite(rim)
    return out


def finish(image, rim=True):
    """Downsamples a supersampled unit frame, with its rim light."""
    image = image.resize((SIZE, SIZE), Image.Resampling.LANCZOS)
    return rim_light(image) if rim else image


def box(d, bounds, color, radius=0):
    d.rounded_rectangle(
        tuple(round(v * SS) for v in bounds),
        radius=round(radius * SS),
        fill=(*color, 255),
    )


def line(d, points, color, width=1):
    d.line(
        [(round(x * SS), round(y * SS)) for x, y in points],
        fill=(*color, 255),
        width=round(width * SS),
    )


def poly(d, points, color):
    d.polygon([(round(x * SS), round(y * SS)) for x, y in points], fill=(*color, 255))


def circle(d, bounds, color):
    d.ellipse(tuple(round(v * SS) for v in bounds), fill=(*color, 255))


def disk(d, x, y, r, c):
    circle(d, (x - r, y - r, x + r, y + r), c)


def mirror(points, side):
    """Places `points`, whose x is an offset from a unit frame's centre line,
    on the right half for `side` 1 and mirrored onto the left for -1."""
    return [(64 + side * x, y) for x, y in points]


def track(d, bounds, phase, paint):
    x0, y0, x1, y1 = bounds
    box(d, bounds, VOID, 5)
    box(d, (x0 + 2, y0 + 2, x1 - 2, y1 - 2), DARK, 3)
    box(d, (x0 + 5, y0 + 4, x1 - 5, y1 - 4), DEEP, 2)
    for y in range(round(y0 + 5), round(y1 - 5), 9):
        offset = (phase * 3) % 9
        if y + offset + 3 < y1 - 3:
            box(d, (x0 + 3, y + offset, x1 - 3, y + offset + 3), IRON, 1)
    line(d, [(x0 + 2, y0 + 6), (x0 + 2, y1 - 7)], EDGE)
    box(d, (x0 + 5, y0 + 6, x1 - 5, y0 + 11), paint, 1)


def vent(d, x, y, width=16):
    box(d, (x, y, x + width, y + 14), VOID, 2)
    for yy in (y + 3, y + 7, y + 11):
        line(d, [(x + 3, yy), (x + width - 3, yy)], IRON)


def panel(d, points, paint):
    poly(d, points, VOID)
    cx = sum(p[0] for p in points) / len(points)
    cy = sum(p[1] for p in points) / len(points)
    inner = [(cx + (x - cx) * 0.88, cy + (y - cy) * 0.88) for x, y in points]
    poly(d, inner, paint)
    line(d, inner[:2], EDGE)


def strut(d, a, b, width=6):
    line(d, [a, b], VOID, width + 3)
    line(d, [a, b], IRON, width)
    for x, y in (a, b):
        circle(d, (x - 2, y - 2, x + 2, y + 2), STEEL)


def engine(d, x, y, width, height, paint, phase):
    box(d, (x, y, x + width, y + height), VOID, 4)
    box(d, (x + 2, y + 2, x + width - 2, y + height - 2), DARK, 3)
    box(d, (x + 5, y + 3, x + width - 5, y + 8), DEEP, 1)
    line(d, [(x + 4, y + 2), (x + width - 4, y + 2)], EDGE)
    box(d, (x + 5, y + 12, x + width - 5, y + height - 13), paint, 2)
    box(d, (x + 4, y + height - 10, x + width - 4, y + height - 3), DEEP, 1)
    for i in range(3):
        xx = x + 5 + i * (width - 10) / 2
        line(
            d,
            [(xx, y + height - 8), (xx, y + height - 5)],
            EDGE if (i + phase) % 2 else IRON,
        )


def plate(d, bounds, fill=DARK, cut=5):
    x, y, r, b = bounds
    pts = [
        (x + cut, y),
        (r - cut, y),
        (r, y + cut),
        (r, b - cut),
        (r - cut, b),
        (x + cut, b),
        (x, b - cut),
        (x, y + cut),
    ]
    poly(d, pts, VOID)
    poly(
        d,
        [
            (x + cut, y + 2),
            (r - cut, y + 2),
            (r - 2, y + cut),
            (r - 2, b - cut),
            (r - cut, b - 2),
            (x + cut, b - 2),
            (x + 2, b - cut),
            (x + 2, y + cut),
        ],
        fill,
    )
    line(
        d,
        [(x + 2, b - cut), (x + 2, y + cut), (x + cut, y + 2), (r - cut, y + 2)],
        EDGE,
    )


def bolt(d, x, y):
    box(d, (x - 2, y - 2, x + 2, y + 2), VOID, 1)
    box(d, (x - 1, y - 1, x + 1, y), STEEL)
