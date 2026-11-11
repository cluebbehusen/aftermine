"""Coordinated machine study using the Extractor's open industrial construction."""

from PIL import Image, ImageDraw

from tools import sprite_kit as kit
from tools.production_sprite_sources import extractor_reclaimer_final as metal
from tools.sprite_kit import (
    DARK,
    EDGE,
    IRON,
    SS,
    box,
    line,
    poly,
)

VOID = metal.VOID
DEEP = metal.IRON_DEEP
LIGHT = kit.IRON_LIGHT
RECLAIMER_WORK_FRAMES = 12


def plate(draw, bounds, color=IRON, radius=3):
    x0, y0, x1, y1 = bounds
    box(draw, bounds, DARK, radius)
    box(draw, (x0 + 2, y0 + 2, x1 - 2, y1 - 2), color, max(0, radius - 1))
    line(draw, ((x0 + 3, y0 + 2), (x1 - 3, y0 + 2)), LIGHT)


def finish(image):
    return metal._finish(image, 128)


def foundation(draw, variant):
    metal._foundation(draw, 128, variant)
    for x in (16, 96):
        plate(draw, (x, 22, x + 16, 91))
        box(draw, (x + 4, 34, x + 12, 51), kit.PALETTES[variant]["dark"])


def crusher(image, draw, variant, phase):
    cycle = (phase - 1) % RECLAIMER_WORK_FRAMES if phase else None
    accent = kit.PALETTES[variant]["dark"]
    # Hopper -> opposed corrugated jaws -> open outlet.
    poly(draw, ((33, 20), (95, 20), (86, 49), (42, 49)), DARK)
    poly(draw, ((38, 24), (90, 24), (82, 43), (46, 43)), VOID)
    line(draw, ((37, 22), (91, 22)), accent, 3)
    line(draw, ((38, 25), (45, 43)), LIGHT, 2)
    line(draw, ((89, 25), (83, 43)), IRON, 2)
    # Clip the feed to the hopper opening so a new piece enters from above
    # and disappears inside the crusher before the next piece arrives.
    ore = Image.new("RGBA", (22 * SS, 23 * SS))
    ore_draw = ImageDraw.Draw(ore)
    chunk_y = (30 if cycle is None else 22 + cycle * 3) - 24
    poly(
        ore_draw,
        ((3, chunk_y), (11, chunk_y - 2), (18, chunk_y + 2), (10, chunk_y + 6)),
        kit.SCRAP_DARK,
    )
    line(ore_draw, ((4, chunk_y), (11, chunk_y - 1)), kit.SCRAP, 1)
    image.alpha_composite(ore, (54 * SS, 24 * SS))
    box(draw, (35, 47, 93, 83), VOID, 3)
    for y, direction in ((51, 1), (69, -1)):
        box(draw, (41, y, 87, y + 11), DARK, 4)
        box(draw, (44, y + 2, 84, y + 8), IRON, 2)
        line(draw, ((46, y + 2), (82, y + 2)), LIGHT, 2)
        offset = direction * (cycle or 0)
        for x in (49, 61, 73):
            ridge = 46 + (x - 46 + offset) % 36
            line(draw, ((ridge, y + 3), (ridge + 2, y + 7)), EDGE, 2)
        for x in (36, 87):
            plate(draw, (x, y - 1, x + 5, y + 12), accent, 1)
    # The working gap stays dark; only the material advances through it.
    if cycle is not None and 4 <= cycle <= 8:
        box(draw, (59, 63, 69, 67), kit.SCRAP_DARK, 1)


def render_reclaimer(variant, phase):
    image, draw = kit.canvas()
    foundation(draw, variant)
    crusher(image, draw, variant, phase)
    cycle = (phase - 1) % RECLAIMER_WORK_FRAMES if phase else 0
    metal._belt(draw, (50, 82, 78, 103), cycle, variant=variant, step=2)
    if phase and cycle >= 6:
        y = 83 + (cycle - 6) * 3
        poly(draw, ((59, y), (64, y - 1), (69, y + 2), (62, y + 3)), kit.SCRAP_DARK)
        line(draw, ((60, y), (64, y)), kit.SCRAP)
    metal._hopper(draw, (42, 100, 86, 117), variant)
    poly(draw, ((53, 109), (63, 106), (75, 110), (73, 113), (52, 113)), kit.SCRAP_DARK)
    line(draw, ((56, 108), (63, 107)), kit.SCRAP, 2)
    return finish(image)


def render_refinery(variant, phase):
    image, draw = kit.canvas()
    foundation(draw, variant)
    accent = kit.PALETTES[variant]["dark"]
    plate(draw, (14, 22, 35, 103), accent)
    plate(draw, (93, 67, 115, 103), accent)
    crusher(image, draw, variant, phase)
    # The same crusher feeds a heated finishing press and an ingot tray.
    plate(draw, (32, 80, 96, 109), accent)
    box(draw, (39, 85, 89, 105), DEEP, 2)
    box(draw, (44, 87, 84, 95), VOID)
    box(draw, (49, 89, 79, 93), metal.AMBER)
    cycle = (phase - 1) % RECLAIMER_WORK_FRAMES if phase else 0
    press = (0, 0, 0, 0, 1, 2, 3, 4, 4, 3, 2, 1)[cycle]
    plate(draw, (44, 96 - press, 84, 104 - press), IRON, 1)
    metal._hopper(draw, (43, 105, 85, 119), variant)
    for x in (52, 65):
        box(draw, (x, 111, x + 10, 115), kit.SCRAP_DARK, 1)
        line(draw, ((x + 1, 111), (x + 8, 111)), kit.SCRAP, 1)
    # One side-mounted exhaust identifies the added thermal stage.
    plate(draw, (94, 5, 115, 82), accent)
    box(draw, (96, 9, 113, 34), kit.PALETTES[variant]["dark"], 2)
    box(draw, (99, 12, 110, 29), VOID, 1)
    line(draw, ((99, 10), (110, 10)), LIGHT, 1)
    for y in (51, 58, 65):
        line(draw, ((99, y), (110, y)), DEEP, 2)
    return finish(image)


def track(draw, x, phase):
    box(draw, (x, 32, x + 16, 107), VOID, 5)
    box(draw, (x + 2, 35, x + 14, 104), DARK, 3)
    for y in range(37, 100, 9):
        at = 37 + (y - 37 + phase * 3) % 63
        box(draw, (x + 3, at, x + 13, at + 4), IRON, 1)
    line(draw, ((x + 3, 38), (x + 3, 100)), LIGHT)


def rocket(draw, nose, tail):
    # 24 source pixels = 0.375 world tiles at the Avalanche's 2-tile draw size.
    poly(
        draw, ((64, nose), (67, nose + 6), (67, tail), (61, tail), (61, nose + 6)), VOID
    )
    box(draw, (62, nose + 6, 66, tail - 1), (151, 146, 134))
    poly(draw, ((64, nose + 1), (66, nose + 6), (62, nose + 6)), (210, 199, 171))
    line(draw, ((62, nose + 7), (62, tail - 4)), (189, 183, 167))
    poly(draw, ((61, tail - 6), (59, tail), (63, tail - 1)), LIGHT)
    poly(draw, ((67, tail - 6), (69, tail), (65, tail - 1)), DARK)


def render_avalanche(variant, phase=0, action=0):
    image, draw = kit.canvas()
    accent = kit.PALETTES[variant]["dark"]
    track(draw, 23, phase)
    track(draw, 89, phase)
    poly(draw, ((40, 39), (88, 39), (96, 56), (91, 102), (37, 102), (32, 56)), VOID)
    poly(draw, ((42, 42), (86, 42), (92, 56), (87, 99), (41, 99), (36, 56)), DARK)
    line(draw, ((43, 43), (85, 43)), IRON, 2)
    # The hull's corner plates end where the slew ring is drawn over them.
    for side in (-1, 1):
        poly(
            draw,
            [
                (64 + side * x, y)
                for x, y in ((12, 45), (22, 45), (27, 57), (26, 72), (12, 72))
            ],
            VOID,
        )
        poly(
            draw,
            [
                (64 + side * x, y)
                for x, y in ((12, 47), (21, 47), (25, 57), (24, 70), (12, 70))
            ],
            accent,
        )
        line(draw, ((64 + side * 13, 48), (64 + side * 20, 48)), EDGE)
    draw.ellipse(metal._box((42, 50, 86, 94)), fill=(*VOID, 255))
    draw.ellipse(metal._box((44, 52, 84, 92)), fill=(*IRON, 255))
    draw.ellipse(metal._box((48, 56, 80, 88)), fill=(*DEEP, 255))
    for x, y in (
        (46, 72),
        (82, 72),
        (49.5, 60.5),
        (78.5, 60.5),
        (49.5, 83.5),
        (78.5, 83.5),
    ):
        draw.ellipse(metal._box((x - 1, y - 1, x + 1, y + 1)), fill=(*LIGHT, 255))
    box(draw, (44, 92, 84, 99), VOID, 1)
    box(draw, (46, 94, 82, 98), IRON, 1)
    box(draw, (50, 94, 78, 98), accent)
    line(draw, ((47, 94), (81, 94)), EDGE)
    chassis = finish(image)
    image, draw = kit.canvas()
    for side in (-1, 1):
        foot = (64 + side * 21, 90)
        joint = (64 + side * 17, 77)
        lug = (64 + side * 13, 62)
        line(draw, (joint, lug), VOID, 6)
        line(draw, (joint, lug), EDGE, 3)
        line(draw, (foot, joint), VOID, 9)
        line(draw, (foot, joint), IRON, 7)
        line(draw, ((foot[0] - 2, foot[1] - 1), (joint[0] - 2, joint[1] + 1)), LIGHT)
        box(draw, (lug[0] - 4, 58, lug[0] + 4, 65), VOID, 1)
        box(draw, (lug[0] - 3, 59, lug[0] + 3, 64), IRON)
    # A fixed launch track: only the rocket moves.
    plate(draw, (53, 18, 75, 94), DEEP, 3)
    box(draw, (59, 21, 69, 88), VOID, 1)
    for y in range(29, 86, 9):
        line(draw, ((60, y), (68, y)), DARK, 2)
    line(draw, ((56, 22), (56, 84)), LIGHT, 2)
    line(draw, ((72, 22), (72, 84)), IRON, 2)
    plate(draw, (51, 77, 77, 85), accent, 1)
    box(draw, (58, 87, 70, 93), IRON, 1)
    line(draw, ((59, 87), (69, 87)), EDGE)
    if action in (0, 1):
        rocket(draw, 18, 42)
    elif action == 4:
        rocket(draw, 48, 72)
    chassis.alpha_composite(image.resize((128, 128), metal.Image.Resampling.LANCZOS))
    return chassis
