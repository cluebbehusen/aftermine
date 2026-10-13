"""Strategic missile silo: a fixed foundation under a rotating gantry deck.

The whole upper deck turns to the launch heading: cradle, gantry, service
bay, locks and payload. A five-section warhead is assembled by the gantry
hook, which lifts each fixed-size section out of the covered service bay,
turns it upright and seats it in the long cradle. The lower lock closes
once the second section is seated and the upper lock once the fourth is;
upper releases first, then lower, and the warhead leaves only with both
open.

Every deck layer shares the 128px canvas and rotates about its center,
(64, 64), which is the center of the 2x2 footprint. North-facing art is
heading zero; positive headings turn clockwise on screen.
"""

from __future__ import annotations

import functools
import math
from pathlib import Path

from PIL import Image, ImageDraw

from tools import gen_sprites as gen
from tools.production_sprite_sources.installed_defenses_final import bolt, plate
from tools.production_sprite_sources.specialists_final import (
    BRASS,
    DARK,
    DEEP,
    EDGE,
    IRON,
    SIZE,
    SS,
    STEEL,
    VOID,
    box,
    canvas,
    circle,
    finish,
    line,
    poly,
    strut,
    vent,
)

Registry = dict[str, Image.Image]
Color = tuple[int, int, int]

LIGHT = (225, 205, 163)

SECTIONS = 5
SECTION_BANDS = ((86, 110), (70, 86), (54, 70), (38, 54), (19, 38))
SECTION_CENTERS = tuple((top + bottom) / 2 for top, bottom in SECTION_BANDS)
SECTION_TICKS = 32
GANTRY_STEPS = 8
GANTRY_RETURN_STEPS = 4
GANTRY_FRAMES = SECTIONS * GANTRY_STEPS + GANTRY_RETURN_STEPS
LOCK_STEPS = 4
LOWER_LOCK_Y = 84
UPPER_LOCK_Y = 49
EXHAUST_FRAMES = 3
WARHEAD_SIZE = 160
DECK_PIVOT = (64, 64)

PARK = (100.0, 86.0)


def smooth(t: float) -> float:
    t = max(0.0, min(1.0, t))
    return t * t * (3 - 2 * t)


def disk(d: ImageDraw.ImageDraw, x: float, y: float, r: float, color: Color) -> None:
    circle(d, (x - r, y - r, x + r, y + r), color)


def brace(d: ImageDraw.ImageDraw, a, b, width: float = 5) -> None:
    strut(d, a, b, width)
    for x, y in (a, b):
        bolt(d, x, y)


def shade(paint: Color, amount: float) -> Color:
    return tuple(round(v * amount) for v in paint)


def missile(
    d: ImageDraw.ImageDraw,
    x: float,
    y: float,
    paint: Color,
    width: float,
    length: float,
) -> None:
    """A finished motor, tank courses, fins and a capped payload."""
    top = y - max(8, length)
    w = width
    box(d, (x - w, top, x + w, y - 4), VOID, 3)
    box(d, (x - w + 2, top + 1, x + w - 2, y - 5), IRON, 2)
    for yy in (y - 14, y - 27, y - 40):
        if yy > top:
            line(d, [(x - w + 2, yy), (x + w - 2, yy)], EDGE, 2)
    poly(d, [(x - w, top + 4), (x, top - 12), (x + w, top + 4)], VOID)
    poly(d, [(x - w + 2, top + 3), (x, top - 9), (x + w - 2, top + 3)], paint)
    for sign in (-1, 1):
        poly(
            d,
            [
                (x + sign * (w - 1), y - 19),
                (x + sign * (w + 6), y - 5),
                (x + sign * (w + 6), y + 1),
                (x + sign * (w - 1), y - 6),
            ],
            DARK,
        )
    line(d, [(x - w + 3, top + 3), (x - w + 3, y - 8)], STEEL, 2)
    box(d, (x - 4, y - 5, x + 4, y), DEEP, 1)


def exhaust(d: ImageDraw.ImageDraw, x: float, y: float, flicker: int) -> None:
    """A full-power motor plume; `flicker` steps its length."""
    n = 15 + flicker * 2
    poly(d, [(x - 5, y), (x, y + n + 8), (x + 5, y)], BRASS)
    poly(d, [(x - 3, y), (x, y + n), (x + 3, y)], LIGHT)


def _draw_warhead(d: ImageDraw.ImageDraw, x: float, y: float, paint: Color) -> None:
    missile(d, x, y, paint, 12, 76)
    # Seams mark where the five assembled sections meet.
    for offset in (23, 39, 55, 70):
        seam = y - offset
        line(d, [(x - 9, seam), (x + 9, seam)], DEEP, 2)
        line(d, [(x - 9, seam - 1), (x + 9, seam - 1)], EDGE, 1)


@functools.lru_cache(maxsize=None)
def _warhead(paint: Color) -> Image.Image:
    image, d = canvas()
    _draw_warhead(d, 64, 108, paint)
    return image


@functools.lru_cache(maxsize=None)
def _sections(paint: Color) -> tuple[Image.Image, ...]:
    warhead = _warhead(paint)
    return tuple(
        warhead.crop((44 * SS, top * SS, 84 * SS, bottom * SS))
        for top, bottom in SECTION_BANDS
    )


def _place_section(
    image: Image.Image,
    index: int,
    center: tuple[float, float],
    angle: float,
    paint: Color,
) -> None:
    part = _sections(paint)[index]
    if angle:
        part = part.rotate(angle, Image.Resampling.BICUBIC, expand=True)
    image.alpha_composite(
        part,
        (
            round(center[0] * SS - part.width / 2),
            round(center[1] * SS - part.height / 2),
        ),
    )


def _foundation(d: ImageDraw.ImageDraw, paint: Color) -> None:
    disk(d, 64, 66, 58, VOID)
    disk(d, 64, 66, 55, DARK)
    disk(d, 64, 64, 53, IRON)
    disk(d, 64, 64, 49, VOID)
    for angle in range(0, 360, 45):
        a = math.radians(angle)
        x = 64 + 51 * math.cos(a)
        y = 64 + 51 * math.sin(a)
        disk(d, x, y, 3, DEEP)
        disk(d, x, y, 1.5, STEEL)
    for x, y in ((25, 27), (103, 27), (25, 104), (103, 104)):
        plate(d, (x - 6, y - 6, x + 6, y + 6), IRON, 3)
        bolt(d, x, y)
    for x in (12, 109):
        box(d, (x, 56, x + 6, 76), paint, 1)


def _deck(d: ImageDraw.ImageDraw, paint: Color) -> None:
    disk(d, 64, 64, 48, EDGE)
    disk(d, 64, 64, 46.5, DARK)
    for angle in (12, 127, 238):
        a = math.radians(angle)
        line(
            d,
            [
                (64 + 35 * math.cos(a), 64 + 35 * math.sin(a)),
                (64 + 46 * math.cos(a), 64 + 46 * math.sin(a)),
            ],
            DEEP,
            2,
        )
        bolt(d, 64 + 42 * math.cos(a), 64 + 42 * math.sin(a))
    vent(d, 26, 67, 11)
    box(d, (29, 51, 34, 60), shade(paint, 0.7), 1)
    # The long cradle runs past the deck at both ends.
    plate(d, (46, 14, 82, 115), DEEP, 4)
    for x in (42, 86):
        plate(d, (x - 3, 12, x + 3, 116), IRON, 3)
        line(d, [(x - 1, 17), (x - 1, 110)], EDGE, 1)
    for y in (35, 62, 90, 111):
        brace(d, (42, y), (86, y), 3)
    brace(d, (37, 27), (108, 27), 5)
    brace(d, (108, 30), (108, 101), 4)


def _stack(image: Image.Image, count: int, paint: Color) -> None:
    for index in range(count):
        _place_section(image, index, (64, SECTION_CENTERS[index]), 0, paint)


def _hook(d: ImageDraw.ImageDraw, x: float, y: float) -> None:
    plate(d, (x - 5, 23, x + 5, 31), DARK, 2)
    line(d, [(x, 31), (x, y)], DEEP, 3)
    line(d, [(x, 31), (x, y)], STEEL, 1)
    line(d, [(x - 2, y), (x, y + 2), (x + 2, y)], BRASS, 2)


def _gantry_geometry(frame: int):
    """The section the hook carries (index, center, angle) if any, the hook
    point, and the seat being welded if any, for one gantry frame.

    Frame zero is parked. Frames 1 to `SECTIONS * GANTRY_STEPS` carry the
    sections in order, each returning the empty hook from the last seat,
    lifting the next section out of the service bay, turning it upright and
    settling it; the remaining frames return the empty hook to its park."""
    if frame not in range(GANTRY_FRAMES + 1):
        raise ValueError("invalid Spire gantry frame")
    if frame == 0:
        return None, PARK, None
    step = frame - 1
    if step >= SECTIONS * GANTRY_STEPS:
        u = smooth((step - SECTIONS * GANTRY_STEPS) / GANTRY_RETURN_STEPS)
        seat = SECTION_CENTERS[-1] + 3
        return None, (64 + 36 * u, seat + (PARK[1] - seat) * u), None
    index, j = divmod(step, GANTRY_STEPS)
    phase = j / GANTRY_STEPS
    target = SECTION_CENTERS[index]
    if phase < 0.18:
        u = smooth(phase / 0.18)
        previous = SECTION_CENTERS[index - 1] + 3 if index else PARK[1]
        hook = (64 + 36 * u if index else PARK[0], previous + (PARK[1] - previous) * u)
        return None, hook, None
    lift = smooth((phase - 0.18) / 0.25)
    cross = smooth((phase - 0.43) / 0.30)
    settle = smooth((phase - 0.73) / 0.12)
    clearance = max(32, min(78, target - 10))
    x = PARK[0] - 36 * cross
    y = PARK[1] + (clearance - PARK[1]) * lift + (target - clearance) * settle
    angle = 90 * (1 - smooth((phase - 0.22) / 0.46))
    # The seating weld flickers on a four-tick beat of the section's
    # 32-tick carry.
    weld = (
        target if phase > 0.85 and (j * SECTION_TICKS // GANTRY_STEPS) % 4 < 2 else None
    )
    return (index, (x, y), angle), (x, y + 3), weld


def gantry_stack(frame: int) -> int:
    """How many sections stand seated in the cradle during a gantry frame."""
    if frame == 0:
        return 0
    return min(SECTIONS, (frame - 1) // GANTRY_STEPS)


NOSE_FRAMES = tuple(
    frame
    for frame in range(1, GANTRY_FRAMES + 1)
    if (carried := _gantry_geometry(frame)[0]) is not None
    and carried[0] == SECTIONS - 1
)


def _carried(image: Image.Image, frame: int, paint: Color, nose: bool) -> None:
    carried, _, _ = _gantry_geometry(frame)
    if carried is not None and (carried[0] == SECTIONS - 1) == nose:
        _place_section(image, *carried, paint)


def _gantry(d: ImageDraw.ImageDraw, frame: int) -> None:
    _, hook, weld = _gantry_geometry(frame)
    if weld is not None:
        line(d, [(52, weld + 2), (55, weld + 5)], LIGHT, 1)
    _hook(d, *hook)


def _cover(d: ImageDraw.ImageDraw, paint: Color) -> None:
    # Incoming sections start behind this opaque service-bay cover.
    plate(d, (87, 67, 114, 106), DARK, 5)
    vent(d, 92, 75, 16)
    box(d, (93, 95, 108, 99), shade(paint, 0.65), 1)


def _lock(d: ImageDraw.ImageDraw, y: float, closed: float) -> None:
    for side in (-1, 1):
        root = 64 + side * 22
        jaw = 64 + side * (17 - 7 * closed)
        plate(d, (root - 3, y - 5, root + 3, y + 5), DARK, 2)
        line(d, [(root, y), (jaw, y)], VOID, 6)
        line(d, [(root, y), (jaw, y)], STEEL, 2)
        plate(d, (jaw - 2, y - 5, jaw + 2, y + 5), IRON, 1)
        line(d, [(jaw, y - 3), (jaw, y + 3)], EDGE, 1)
        bolt(d, root, y)


def _rotate(image: Image.Image, heading: float) -> Image.Image:
    return image.rotate(
        -(heading % 360),
        Image.Resampling.BICUBIC,
        center=(DECK_PIVOT[0] * SS, DECK_PIVOT[1] * SS),
    )


def _paint(variant: str) -> Color:
    if variant not in gen.PALETTES:
        raise ValueError(f"unknown variant: {variant}")
    return gen.PALETTES[variant]["base"]


def _downsample(image: Image.Image) -> Image.Image:
    return image.resize(
        (image.width // SS, image.height // SS), Image.Resampling.LANCZOS
    )


def _deck_image(
    paint: Color, stack: int, gantry: int, lower: float, upper: float
) -> Image.Image:
    image, d = canvas()
    _deck(d, paint)
    _stack(image, stack, paint)
    _carried(image, gantry, paint, nose=False)
    _carried(image, gantry, paint, nose=True)
    _gantry(d, gantry)
    _cover(d, paint)
    _lock(d, LOWER_LOCK_Y, lower)
    _lock(d, UPPER_LOCK_Y, upper)
    return image


def render_spire(
    variant: str,
    heading: float = 0.0,
    stack: int = 0,
    gantry: int = 0,
    lower: float = 0.0,
    upper: float = 0.0,
) -> Image.Image:
    """The complete silo, composed in one pass at supersampled scale."""
    if stack not in range(SECTIONS + 1) or not 0 <= lower <= 1 or not 0 <= upper <= 1:
        raise ValueError("invalid Spire sprite state")
    _gantry_geometry(gantry)
    paint = _paint(variant)
    foundation, d = canvas()
    _foundation(d, paint)
    out = finish(foundation)
    deck = _rotate(_deck_image(paint, stack, gantry, lower, upper), heading)
    out.alpha_composite(_downsample(deck))
    return out


def _layer(draw) -> Image.Image:
    image, d = canvas()
    draw(image, d)
    return _downsample(image)


def render_foundation(variant: str) -> Image.Image:
    paint = _paint(variant)
    image, d = canvas()
    _foundation(d, paint)
    return finish(image)


def render_deck(variant: str) -> Image.Image:
    paint = _paint(variant)
    return _layer(lambda image, d: _deck(d, paint))


def render_stack(variant: str, count: int) -> Image.Image:
    """Seated sections short of a complete round; they carry no paint."""
    if count not in range(1, SECTIONS):
        raise ValueError("invalid Spire stack")
    paint = _paint(variant)
    return _layer(lambda image, d: _stack(image, count, paint))


def render_round(variant: str) -> Image.Image:
    """The complete loaded warhead seated in the cradle."""
    paint = _paint(variant)
    return _layer(lambda image, d: _stack(image, SECTIONS, paint))


def render_gantry(frame: int) -> Image.Image:
    """The hook, its weld and any plain section it carries."""
    paint = _paint("base")
    return _layer(
        lambda image, d: (_carried(image, frame, paint, nose=False), _gantry(d, frame))
    )


def render_nose(variant: str, frame: int) -> Image.Image:
    """The painted nose section while the hook carries it, drawn beneath
    the gantry frame of the same index."""
    if frame not in NOSE_FRAMES:
        raise ValueError("the gantry does not carry the nose in this frame")
    paint = _paint(variant)
    return _layer(lambda image, d: _carried(image, frame, paint, nose=True))


def render_cover(variant: str) -> Image.Image:
    paint = _paint(variant)
    return _layer(lambda image, d: _cover(d, paint))


def render_lock(y: float, step: int) -> Image.Image:
    if step not in range(LOCK_STEPS + 1):
        raise ValueError("invalid Spire lock step")
    return _layer(lambda image, d: _lock(d, y, step / LOCK_STEPS))


def render_warhead(variant: str, flicker: int | None) -> Image.Image:
    """The launched warhead on a larger deck-centered canvas, so the plume
    is never cut off. `None` draws it without a plume."""
    if flicker is not None and flicker not in range(EXHAUST_FRAMES):
        raise ValueError("invalid Spire exhaust frame")
    paint = _paint(variant)
    margin = (WARHEAD_SIZE - SIZE) // 2
    image = Image.new("RGBA", (WARHEAD_SIZE * SS, WARHEAD_SIZE * SS))
    d = ImageDraw.Draw(image)
    _draw_warhead(d, 64 + margin, 108 + margin, paint)
    if flicker is not None:
        exhaust(d, 64 + margin, 109 + margin, flicker)
    return _downsample(image)


def _save(registry: Registry, out: Path, name: str, image: Image.Image) -> None:
    gen.save_sprite(image, out, name)
    registry[name] = image


def install_spire(registry: Registry, out: Path) -> None:
    """Installs the empty hull and every runtime layer of the Spire.

    Layers that carry paint ship an allegiance-accent row; the hook, the
    plain sections and the locks never do."""
    for variant in gen.PALETTES:
        tag = gen.variant_tag(variant)
        _save(registry, out, f"spire{tag}", render_spire(variant))
        _save(registry, out, f"rig_spire_foundation{tag}", render_foundation(variant))
        _save(registry, out, f"rig_spire_deck{tag}", render_deck(variant))
        _save(registry, out, f"rig_spire_cover{tag}", render_cover(variant))
        _save(registry, out, f"rig_spire_round{tag}", render_round(variant))
        for index, frame in enumerate(NOSE_FRAMES, 1):
            _save(
                registry,
                out,
                f"rig_spire_nose{tag}_{index}",
                render_nose(variant, frame),
            )
        _save(registry, out, f"rig_spire_warhead{tag}", render_warhead(variant, None))
        for flicker in range(EXHAUST_FRAMES):
            _save(
                registry,
                out,
                f"rig_spire_warhead{tag}_{flicker + 1}",
                render_warhead(variant, flicker),
            )
    for count in range(1, SECTIONS):
        _save(registry, out, f"rig_spire_stack_{count}", render_stack("base", count))
    _save(registry, out, "rig_spire_gantry", render_gantry(0))
    for frame in range(1, GANTRY_FRAMES + 1):
        _save(registry, out, f"rig_spire_gantry_{frame}", render_gantry(frame))
    for name, y in (("lower", LOWER_LOCK_Y), ("upper", UPPER_LOCK_Y)):
        _save(registry, out, f"rig_spire_lock_{name}", render_lock(y, 0))
        for step in range(1, LOCK_STEPS + 1):
            _save(registry, out, f"rig_spire_lock_{name}_{step}", render_lock(y, step))
