"""Tracked point-defense carrier with centered charge indicators."""

from pathlib import Path
from PIL import Image
from tools import gen_sprites as gen
from tools.production_sprite_sources.specialists_final import (
    canvas,
    box,
    line,
    poly,
    circle,
    finish,
    track,
    vent,
    VOID,
    DARK,
    IRON,
    EDGE,
    STEEL,
    BRASS,
    DEEP,
)
from tools.production_sprite_sources.installed_defenses_final import plate, bolt

LIGHT = (225, 205, 163)
RECHARGE_STEPS = 16
CHARGE_FRAMES = 3 * RECHARGE_STEPS + 1


def render_mantle(
    variant: str, move: int = 0, charges: int = 3, recharge: float = 0.0
) -> Image.Image:
    if (
        variant not in gen.PALETTES
        or move not in range(3)
        or charges not in range(4)
        or not 0 <= recharge <= 1
    ):
        raise ValueError("invalid Mantle sprite state")
    paint = gen.PALETTES[variant]["base"]
    phase = move
    im, d = canvas()
    muted = tuple((round(c * 0.66) for c in paint))
    for bounds in (
        (10, 24, 34, 61),
        (10, 77, 34, 116),
        (94, 24, 118, 61),
        (94, 77, 118, 116),
    ):
        track(d, bounds, phase, muted)
    for x in (27, 86):
        plate(d, (x, 39, x + 15, 101), IRON, 3)
        for y in (48, 89):
            bolt(d, x + 8, y)
    plate(d, (33, 29, 95, 114), DARK, 10)
    plate(d, (39, 88, 89, 108), IRON, 4)
    vent(d, 45, 91, 38)
    for x in (37, 84):
        box(d, (x, 77, x + 7, 89), muted, 1)
        line(d, [(x + 3, 77), (x + 3, 52)], BRASS, 2)
    for x in (43, 85):
        bolt(d, x, 108)
    circle(d, (33, 33, 95, 95), VOID)
    circle(d, (37, 37, 91, 91), IRON)
    poly(d, [(64, 17), (91, 32), (91, 70), (64, 88), (37, 70), (37, 32)], VOID)
    poly(d, [(64, 21), (87, 34), (87, 68), (64, 82), (41, 68), (41, 34)], DARK)
    poly(d, [(64, 21), (86, 34), (64, 44), (42, 34)], IRON)
    poly(d, [(42, 38), (49, 42), (49, 70), (42, 66)], muted)
    poly(d, [(81, 42), (86, 39), (86, 66), (81, 70)], muted)
    line(d, [(43, 33), (64, 23), (84, 33)], EDGE, 2)
    plate(d, (51, 40, 77, 79), VOID, 3)
    for i, y in enumerate((46, 58, 70)):
        ready = i < charges
        progress = recharge if i == charges else 0.0
        box(d, (54, y - 4, 74, y + 4), DEEP, 1)
        box(d, (56, y - 2, 72, y + 2), IRON, 1)
        if ready or progress > 0:
            fill = 1 if ready else progress
            box(
                d,
                (64 - 8 * fill, y - 2, 64 + 8 * fill, y + 2),
                LIGHT if ready else STEEL,
                1,
            )
    plate(d, (54, 24, 74, 31), DARK, 2)
    for x in (58, 64, 70):
        box(d, (x - 1, 24, x + 1, 27), STEEL, 1)
    return finish(im)


def render_charge_frame(frame: int) -> Image.Image:
    if frame not in range(CHARGE_FRAMES):
        raise ValueError("invalid Mantle charge frame")
    charges, step = divmod(frame, RECHARGE_STEPS)
    full = render_mantle("base", charges=charges, recharge=step / RECHARGE_STEPS)
    layer = Image.new("RGBA", (128, 128))
    bounds = (52, 40, 77, 77)
    layer.paste(full.crop(bounds), bounds[:2])
    return layer


def install_mantle(registry: dict[str, Image.Image], out: Path) -> None:
    for variant in gen.PALETTES:
        for move in range(3):
            image = render_mantle(variant, move)
            suffix = "" if move == 0 else f"_move{move}"
            key = f"mantle{gen.variant_tag(variant)}{suffix}"
            gen.save_sprite(image, out, key)
            registry[key] = image
    for frame in range(CHARGE_FRAMES):
        image = render_charge_frame(frame)
        key = f"mantle_charge{frame}"
        gen.save_sprite(image, out, key)
        registry[key] = image
