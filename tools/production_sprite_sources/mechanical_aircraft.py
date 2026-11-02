"""Combat and transport aircraft in the accepted quarry-machine materials."""

import math

from tools import gen_sprites as gen
from tools.production_sprite_sources.mechanical_ground import (
    DARK,
    DEEP,
    EDGE,
    IRON,
    LIGHT,
    SS,
    STEEL,
    VOID,
    box,
    canvas,
    circle,
    finish,
    line,
    poly,
    vent,
)

HEAT = (169, 104, 52)
FLASH = (236, 190, 112)
BRASS = (130, 105, 66)
SKYHOOK_DOOR_GAP = (0, 3, 7, 7, 3)
SKYHOOK_ARM_REACH = (0, 5, 10, 6, 2)
SKYHOOK_GRIP_OPEN = (0, 1, 1, 0, 0)


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


def fan(d, x, y, r, phase, paint, heavy=True):
    circle(d, (x - r, y - r, x + r, y + r), VOID)
    circle(d, (x - r + 2, y - r + 2, x + r - 2, y + r - 2), IRON if heavy else DARK)
    circle(d, (x - r + 5, y - r + 5, x + r - 5, y + r - 5), DEEP)
    for i in range(4):
        a = (i * math.pi / 2) + (phase * math.pi / 6)

        def pt(rad, angle):
            return (x + rad * math.cos(angle), y + rad * math.sin(angle))

        poly(
            d,
            [pt(3, a), pt(r - 6, a + 0.2), pt(r - 6, a + 0.48), pt(4, a + 0.65)],
            IRON,
        )
    for side in (-1, 1):
        box(d, (x + side * (r - 2) - 1, y - 3, x + side * (r - 2) + 1, y + 3), paint)
    circle(d, (x - 4, y - 4, x + 4, y + 4), VOID)
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


def gun(d, x, y, length, width, action):
    recoil = (0, 0, 4, 2, 0)[action]
    box(d, (x - width, y + length - 12, x + width, y + length + 6), VOID, 2)
    box(d, (x - width + 2, y + length - 10, x + width - 2, y + length + 3), IRON, 1)
    box(d, (x - width / 2 - 1, y + recoil, x + width / 2 + 1, y + length), VOID, 1)
    box(d, (x - width / 2 + 1, y + recoil + 2, x + width / 2 - 1, y + length - 2), DARK)
    line(
        d,
        [(x - width / 2 + 1, y + recoil + 3), (x - width / 2 + 1, y + length - 4)],
        EDGE,
    )
    box(d, (x - width / 2 - 1, y + recoil, x + width / 2 + 1, y + recoil + 3), IRON)
    if action == 2:
        poly(
            d,
            [
                (x - 3, y + recoil - 1),
                (x - 2, y + recoil - 5),
                (x, y + recoil - 9),
                (x + 2, y + recoil - 5),
                (x + 3, y + recoil - 1),
            ],
            FLASH,
        )
    if action in (1, 3):
        box(d, (x - 2, y + length - 8, x + 2, y + length - 5), HEAT)


def sensor(d, x, y, paint):
    box(d, (x - 5, y - 5, x + 5, y + 4), VOID, 2)
    box(d, (x - 3, y - 3, x + 3, y + 1), paint, 1)
    line(d, [(x - 3, y - 3), (x + 1, y - 3)], EDGE)


def buzzard(variant, move=0, action=0):
    im, d = canvas()
    paint = gen.PALETTES[variant]["dark"]
    for x, y in ((25, 40), (103, 40), (25, 94), (103, 94)):
        strut(d, (64, 64 if y < 64 else 84), (x, y), 7)
        fan(d, x, y, 20, move or action, paint)
    panel(
        d,
        [
            (45, 26),
            (54, 20),
            (74, 20),
            (83, 26),
            (84, 104),
            (73, 113),
            (55, 113),
            (44, 104),
        ],
        DARK,
    )
    for x in (46, 76):
        box(d, (x, 36, x + 7, 89), paint, 2)
        line(d, [(x + 1, 40), (x + 1, 59)], EDGE)
    box(d, (54, 59, 74, 88), VOID, 2)
    for y in (64, 70, 76):
        box(d, (57, y, 71, y + 3), IRON, 1)
    vent(d, 54, 92, 20)
    base = finish(im)
    im, d = canvas()
    gun(d, 64, 12, 48, 10, action)
    base.alpha_composite(finish(im, False))
    return base


def talon(variant, move=0, action=0):
    im, d = canvas()
    paint = gen.PALETTES[variant]["dark"]
    for side in (-1, 1):
        panel(
            d,
            [
                (64 + side * 13, 39),
                (64 + side * 47, 58),
                (64 + side * 51, 84),
                (64 + side * 24, 91),
                (64 + side * 12, 73),
            ],
            DARK,
        )
        panel(
            d,
            [
                (64 + side * 21, 49),
                (64 + side * 39, 60),
                (64 + side * 35, 73),
                (64 + side * 18, 65),
            ],
            paint,
        )
        engine(d, 64 + side * 34 - 11, 62, 22, 41, paint, move)
        panel(
            d, [(64 + side * 9, 96), (64 + side * 24, 111), (64 + side * 6, 107)], IRON
        )
    panel(d, [(54, 39), (74, 39), (79, 81), (71, 111), (57, 111), (49, 81)], DARK)
    box(d, (57, 71, 71, 94), paint, 2)
    sensor(d, 64, 63, paint)
    vent(d, 56, 96, 16)
    base = finish(im)
    im, d = canvas()
    for side in (-1, 1):
        x = 64 + side * 12
        box(d, (x - 5, 22, x + 5, 57), VOID, 2)
        box(d, (x - 3, 24, x + 3, 52), IRON, 1)
        box(d, (x - 2, 25, x + 2, 41), paint)
        line(d, [(x - 3, 26), (x - 3, 48)], EDGE)
    gun(d, 64, 19, 34, 5, action)
    base.alpha_composite(finish(im, False))
    return base


def _arc(d, x, y, r, start, end, color, width):
    d.arc(
        tuple(round(v * SS) for v in (x - r, y - r, x + r, y + r)),
        start,
        end,
        fill=(*color, 255),
        width=round(width * SS),
    )


def skyhook_rotor(d, x, y, r, phase, paint, outward):
    circle(d, (x - r, y - r, x + r, y + r), VOID)
    circle(d, (x - r + 2, y - r + 2, x + r - 2, y + r - 2), IRON)
    circle(d, (x - r + 5, y - r + 5, x + r - 5, y + r - 5), DEEP)
    _arc(d, x, y, r - 2, 195, 300, EDGE, 1)
    for i in range(8):
        a = i * math.pi / 4 + math.pi / 8
        line(
            d,
            [
                (x + (r - 5) * math.cos(a), y + (r - 5) * math.sin(a)),
                (x + (r - 2) * math.cos(a), y + (r - 2) * math.sin(a)),
            ],
            VOID,
        )
    center = 0 if outward > 0 else 180
    _arc(d, x, y, r - 2, center - 32, center + 32, paint, 3)
    for i in range(4):
        a = (i * math.pi / 2) + (phase * math.pi / 6)

        def pt(rad, angle):
            return (x + rad * math.cos(angle), y + rad * math.sin(angle))

        poly(
            d,
            [pt(3, a), pt(r - 6, a + 0.2), pt(r - 6, a + 0.48), pt(4, a + 0.65)],
            IRON,
        )
        line(d, [pt(4, a + 0.05), pt(r - 6, a + 0.2)], LIGHT)
    inner = x - outward * (r - 2)
    line(d, [(x, y), (inner, y)], VOID, 5)
    line(d, [(x, y), (inner, y)], DARK, 2)
    circle(d, (x - 4, y - 4, x + 4, y + 4), VOID)
    circle(d, (x - 3, y - 3, x + 3, y + 3), IRON)
    line(d, [(x - 2, y - 2), (x + 1, y - 2)], LIGHT)


def skyhook_drive(d):
    box(d, (41, 30, 87, 38), VOID, 1)
    box(d, (42, 31, 86, 37), IRON)
    line(d, [(42, 31), (86, 31)], EDGE)
    for x in (47, 53, 75, 81):
        line(d, [(x, 31), (x, 37)], VOID, 1.5)
    box(d, (54, 22, 74, 44), VOID, 2)
    box(d, (56, 24, 72, 42), IRON, 1)
    line(d, [(57, 24), (71, 24)], EDGE)
    for y in (29, 33, 37):
        line(d, [(58, y), (70, y)], VOID, 1.5)


def skyhook_hold(d, gap):
    box(d, (55, 48, 73, 104), VOID, 2)
    for x in (60, 68):
        line(d, [(x, 51), (x, 101)], DEEP, 2)
    for y in (62, 76, 90):
        line(d, [(58, y), (70, y)], DEEP, 1.5)
    for side, shade in ((-1, IRON), (1, DARK)):
        inner = 64 + side * (1 + gap)
        outer = 64 + side * (9 + gap)
        x0, x1 = sorted((inner, outer))
        lip = LIGHT if side < 0 else IRON
        box(d, (x0, 49, x1, 103), shade, 1)
        line(d, [(x0 + 1, 50), (x1 - 1, 50)], lip)
        for y in (62, 76, 90):
            line(d, [(x0 + 1, y), (x1 - 1, y)], DEEP, 1.5)
            line(d, [(x0 + 1, y + 1.5), (x1 - 1, y + 1.5)], lip)


def skyhook_rails(d, paint):
    # Drawn over the hold so the doors run back underneath them.
    for x, shade in ((46, IRON), (75, DARK)):
        box(d, (x - 1, 44, x + 9, 103), VOID, 2)
        box(d, (x, 45, x + 8, 102), shade, 1)
        line(d, [(x + 1, 46), (x + 1, 101)], EDGE if x == 46 else IRON)
        for y0, y1 in ((47, 61), (73, 87)):
            box(d, (x + 2, y0, x + 7, y1), VOID)
            box(d, (x + 2, y0 + 1, x + 7, y1 - 1), paint)
        for y in (65, 91, 98):
            line(d, [(x + 1, y), (x + 8, y)], VOID, 1.5)
    box(d, (56, 19, 72, 22), paint)


def skyhook_clamp(d, side, y, action):
    root = (64 + side * 20, y)
    rest = (64 + side * 32, y + 8)
    tip = (64 + side * (32 + SKYHOOK_ARM_REACH[action]), y + 8)
    sleeve = (
        root[0] + (rest[0] - root[0]) * 0.62,
        root[1] + (rest[1] - root[1]) * 0.62,
    )
    line(d, [root, tip], VOID, 5)
    line(d, [root, tip], EDGE, 2)
    line(d, [root, sleeve], VOID, 8)
    line(d, [root, sleeve], IRON, 5)
    line(d, [(root[0], root[1] - 1.5), (sleeve[0], sleeve[1] - 1.5)], EDGE)
    box(d, (root[0] - 4, y - 4, root[0] + 4, y + 4), VOID, 1)
    box(d, (root[0] - 3, y - 3, root[0] + 3, y + 3), DARK, 1)
    x, yy = tip
    spread = 2 * SKYHOOK_GRIP_OPEN[action]
    hook = [
        (x + side * spread, yy - 2),
        (x + side * spread, yy + 5),
        (x - side * (5 - 2 * spread), yy + 5),
    ]
    line(d, hook, VOID, 6)
    line(d, hook, STEEL, 3)
    line(d, hook[1:], BRASS, 3)


def skyhook(variant, move=0, action=0, part=None):
    im, d = canvas()
    paint = gen.PALETTES[variant]["dark"]
    for x, outward in ((25, -1), (103, 1)):
        skyhook_rotor(d, x, 34, 22, move, paint, outward)
    if part == "hull":
        return finish(im)
    if part == "mount":
        im, d = canvas()
    panel(
        d,
        [
            (45, 24),
            (55, 17),
            (73, 17),
            (83, 24),
            (85, 96),
            (78, 112),
            (50, 112),
            (43, 96),
        ],
        DARK,
    )
    skyhook_hold(d, SKYHOOK_DOOR_GAP[action])
    skyhook_rails(d, paint)
    skyhook_drive(d)
    base = finish(im)
    im, d = canvas()
    for y in (67, 93):
        for side in (-1, 1):
            skyhook_clamp(d, side, y, action)
    base.alpha_composite(finish(im, False))
    return base
