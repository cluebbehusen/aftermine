"""The Condor in shared aircraft materials, keeping its flying-wing silhouette."""

from dataclasses import dataclass

from PIL import Image, ImageDraw

from tools import sprite_kit as kit
from tools.sprite_kit import (
    IRON,
    IRON_DARK,
    IRON_LIGHT,
    SS,
    mirror,
)


@dataclass(frozen=True)
class Palette:
    base: tuple[int, int, int]
    dark: tuple[int, int, int]


PALETTES = {
    "base": Palette((176, 75, 52), (105, 43, 33)),
    "probe": Palette((48, 132, 113), (29, 79, 68)),
}

BLACK = (11, 11, 15)
IRON_DEEP = (24, 25, 31)
EDGE = (115, 114, 123)
CONDOR_SIZE = 128
CONDOR_STATES = ("idle", "crack", "open", "release", "recover")


class ScaledDraw:
    def __init__(self, image):
        self.draw = ImageDraw.Draw(image)

    def __getattr__(self, name):
        def paint(coords, **kwargs):
            if coords and isinstance(coords[0], (tuple, list)):
                scaled = [(round(x * SS), round(y * SS)) for x, y in coords]
            else:
                scaled = tuple(round(v * SS) for v in coords)
            for key in ("width", "radius"):
                if key in kwargs:
                    kwargs[key] = round(kwargs[key] * SS)
            return getattr(self.draw, name)(scaled, **kwargs)

        return paint


def _rgba(color, alpha=255):
    return (*color, alpha)


def _canvas(size=128):
    image = Image.new("RGBA", (size * SS, size * SS), (0, 0, 0, 0))
    return image, ScaledDraw(image)


def _column(x, half, y0, y1):
    return (x - half, y0, x + half, y1)


def condor_wing(draw, side, paint):
    draw.polygon(mirror(((5, 26), (41, 62), (22, 66), (7, 54)), side), fill=_rgba(IRON))
    draw.line(mirror(((8, 26), (39, 58)), side), fill=_rgba(EDGE), width=1)
    draw.line(mirror(((26, 46), (30, 64)), side), fill=_rgba(IRON_DEEP), width=1)
    draw.polygon(
        mirror(((26, 63), (45, 67), (44, 76), (26, 72)), side), fill=_rgba(BLACK)
    )
    draw.polygon(
        mirror(((28, 65), (43, 69), (42, 74), (28, 70)), side), fill=_rgba(paint)
    )
    draw.line(mirror(((28, 65), (43, 69)), side), fill=_rgba(EDGE), width=1)
    draw.polygon(
        mirror(((9, 72), (24, 73), (14, 85), (9, 80)), side), fill=_rgba(IRON_DARK)
    )
    draw.line(mirror(((9, 72), (24, 73)), side), fill=_rgba(IRON), width=1)


def condor_engine(draw, side):
    x = 64 + side * 20
    draw.rounded_rectangle(_column(x, 7, 43, 71), radius=3, fill=_rgba(BLACK))
    draw.rounded_rectangle(_column(x, 5, 45, 69), radius=2, fill=_rgba(IRON_DARK))
    draw.rectangle(_column(x, 4, 46, 51), fill=_rgba(BLACK))
    draw.line([(x - 4, 45), (x + 3, 45)], fill=_rgba(IRON_LIGHT), width=1)
    for y in (55, 58, 61):
        draw.line([(x - 3, y), (x + 3, y)], fill=_rgba(IRON), width=1)
    draw.rounded_rectangle(_column(x, 4, 65, 73), radius=1, fill=_rgba(BLACK))
    draw.rectangle(_column(x, 2, 66, 71), fill=_rgba(IRON_DEEP))


def condor_spine(draw, paint):
    draw.polygon(
        ((56, 30), (64, 22), (72, 30), (73, 72), (64, 80), (55, 72)),
        fill=_rgba(IRON_DARK),
    )
    draw.line([(56, 31), (55, 71)], fill=_rgba(IRON), width=1)
    draw.line([(72, 31), (73, 71)], fill=_rgba(BLACK), width=1)
    draw.rectangle((58, 31, 61, 52), fill=_rgba(paint))
    draw.rectangle((67, 31, 70, 52), fill=_rgba(paint))
    for y in (58, 66):
        draw.line([(57, y), (71, y)], fill=_rgba(IRON_DEEP), width=1)
        draw.line([(57, y + 1), (71, y + 1)], fill=_rgba(IRON), width=1)
    # An uninterrupted keel keeps the back closed: the payload leaves through the nose.
    draw.line((64, 43, 64, 74), fill=_rgba(IRON_DEEP), width=2)


def nose_bay(
    image: Image.Image,
    draw: ImageDraw.ImageDraw,
    state: str,
    paint: tuple[int, int, int],
) -> None:
    spread = {"idle": 0, "crack": 0, "open": 6, "release": 6, "recover": 3}[state]
    # The aperture reaches the leading edge, allowing the separate payload
    # underneath the airframe to emerge instead of appearing on its roof.
    draw.rectangle((56, 19, 72, 36), fill=_rgba(BLACK))
    draw.rectangle((60, 19, 68, 31), fill=(0, 0, 0, 0))
    for side in (-1, 1):
        points = [
            (64 + side * x + side * spread, y)
            for x, y in ((1, 19), (8, 19), (12, 32), (4, 36), (1, 29))
        ]
        draw.polygon(points, fill=_rgba(IRON_DARK))
        draw.line(
            [(64 + side * (x + spread), y) for x, y in ((7, 21), (10, 30))],
            fill=_rgba(IRON_LIGHT),
            width=1,
        )
        draw.line(
            (64 + side * (2 + spread), 21, 64 + side * (2 + spread), 28),
            fill=_rgba(paint),
            width=2,
        )
    if not spread:
        draw.line((64, 19, 64, 30), fill=_rgba(BLACK), width=1)


def render_condor(variant: str, state: str = "idle") -> Image.Image:
    """Preserve the flying wing and route its payload through split nose doors."""
    if variant not in PALETTES:
        raise ValueError(f"unknown variant: {variant}")
    if state not in CONDOR_STATES:
        raise ValueError(f"unknown Condor state: {state}")
    image, draw = _canvas(CONDOR_SIZE)
    palette = PALETTES[variant]
    outer = (
        (56, 19),
        (72, 19),
        (119, 62),
        (112, 82),
        (92, 77),
        (81, 94),
        (64, 83),
        (47, 94),
        (36, 77),
        (16, 82),
        (9, 62),
    )
    inner = (
        (58, 24),
        (70, 24),
        (112, 62),
        (107, 76),
        (89, 71),
        (78, 87),
        (64, 78),
        (50, 87),
        (39, 71),
        (21, 76),
        (16, 62),
    )
    draw.polygon(outer, fill=_rgba(BLACK))
    draw.polygon(inner, fill=_rgba(IRON_DEEP))
    for side in (-1, 1):
        condor_wing(draw, side, palette.dark)
        condor_engine(draw, side)
    condor_spine(draw, palette.dark)
    nose_bay(image, draw, state, palette.dark)
    return kit.finish(image)
