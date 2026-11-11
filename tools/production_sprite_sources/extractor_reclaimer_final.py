"""Drawing helpers the Extractor and Reclaimer art shares: the sprite canvas,
foundation, hopper and belt that mechanical_extractor and mechanical_economy
build their frames on.
"""

from __future__ import annotations

from PIL import Image, ImageDraw

from tools import sprite_kit as kit

IRON_DEEP = (17, 18, 23)
VOID = (10, 10, 13)
AMBER = (151, 93, 38)


def _box(values: tuple[float, float, float, float]) -> tuple[int, int, int, int]:
    return tuple(kit.s(value) for value in values)  # type: ignore[return-value]


def _points(
    values: tuple[tuple[float, float], ...],
) -> tuple[tuple[int, int], ...]:
    return tuple((kit.s(x), kit.s(y)) for x, y in values)


def _finish(image: Image.Image, size: int) -> Image.Image:
    native = image.resize((size, size), Image.Resampling.LANCZOS)
    return kit.rim_light(native)


def _bolt(draw: ImageDraw.ImageDraw, x: float, y: float, radius: float = 1.7) -> None:
    draw.ellipse(
        _box((x - radius, y - radius, x + radius, y + radius)),
        fill=(*kit.IRON_DARK, 255),
    )
    draw.rectangle(_box((x - 0.6, y - 0.6, x + 0.6, y + 0.6)), fill=(*kit.BONE, 230))


def _foundation(draw: ImageDraw.ImageDraw, size: int, variant: str) -> None:
    palette = kit.PALETTES[variant]
    inset = 7 if size == 128 else 4
    outer = size - inset
    draw.rounded_rectangle(
        _box((inset, inset + 2, outer, outer - 1)),
        radius=kit.s(8 if size == 128 else 6),
        fill=(*kit.IRON_DARK, 255),
    )
    draw.rounded_rectangle(
        _box((inset + 4, inset + 6, outer - 4, outer - 5)),
        radius=kit.s(6 if size == 128 else 4),
        fill=(*IRON_DEEP, 255),
    )
    stripe_y = outer - (10 if size == 128 else 7)
    draw.rectangle(
        _box((inset + 9, stripe_y, outer - 9, stripe_y + 3)),
        fill=(*palette["dark"], 255),
    )
    corners = (
        (inset + 7, inset + 9),
        (outer - 7, inset + 9),
        (inset + 7, outer - 8),
        (outer - 7, outer - 8),
    )
    for x, y in corners:
        _bolt(draw, x, y, 2 if size == 128 else 1.4)


def _hopper(
    draw: ImageDraw.ImageDraw,
    bounds: tuple[float, float, float, float],
    variant: str,
    *,
    split: bool = False,
) -> None:
    x0, y0, x1, y1 = bounds
    palette = kit.PALETTES[variant]
    draw.polygon(
        _points(((x0, y0), (x1, y0), (x1 - 5, y1), (x0 + 5, y1))),
        fill=(*kit.IRON_DARK, 255),
    )
    draw.polygon(
        _points(
            (
                (x0 + 3, y0 + 3),
                (x1 - 3, y0 + 3),
                (x1 - 7, y1 - 3),
                (x0 + 7, y1 - 3),
            )
        ),
        fill=(*VOID, 255),
    )
    draw.line(
        _points(((x0 + 4, y0 + 2), (x1 - 4, y0 + 2))),
        fill=(*palette["light"], 235),
        width=kit.s(2),
    )
    if split:
        draw.line(
            _points((((x0 + x1) / 2, y0 + 3), ((x0 + x1) / 2, y1 - 3))),
            fill=(*kit.IRON_LIGHT, 255),
            width=kit.s(2),
        )


def _belt(
    draw: ImageDraw.ImageDraw,
    bounds: tuple[float, float, float, float],
    phase: int,
    *,
    variant: str,
    step: int = 3,
) -> None:
    x0, y0, x1, y1 = bounds
    palette = kit.PALETTES[variant]
    draw.rounded_rectangle(_box(bounds), radius=kit.s(2), fill=(*kit.IRON_DARK, 255))
    draw.rectangle(_box((x0 + 2, y0 + 2, x1 - 2, y1 - 2)), fill=(*VOID, 255))
    offset = phase * step % 8
    for y in range(round(y0) - 8, round(y1) + 8, 8):
        at = y + offset
        if y0 + 2 <= at <= y1 - 2:
            draw.line(
                _points(((x0 + 2, at), (x1 - 2, at))),
                fill=(*palette["dark"], 255),
                width=kit.s(2),
            )


