"""Extractor surface cutter and Deep Bore hydraulic auger."""

import math

from PIL import Image

from tools import gen_sprites as g
from tools.production_sprite_sources import extractor_reclaimer_final as original
from tools.production_sprite_sources.installed_defenses_final import bolt, plate
from tools.production_sprite_sources.specialists_final import (
    DARK,
    DEEP,
    EDGE,
    IRON,
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

EXTRACTOR_WORK_FRAMES = 16
DEEP_BORE_WORK_FRAMES = 48


def arc(d, b, a, z, c, w):
    d.arc(tuple(round(v * 4) for v in b), a, z, fill=(*c, 255), width=round(w * 4))


def brace(d, a, b, w=8):
    line(d, [a, b], VOID, w + 4)
    line(d, [a, b], IRON, w)
    line(d, [(a[0] - 2, a[1]), (b[0] - 2, b[1])], EDGE, 2)
    for x, y in (a, b):
        bolt(d, x, y)


def hose(d, pts):
    line(d, pts, VOID, 6)
    line(d, pts, IRON, 2)


def render_deep_bore(variant: str, phase: float) -> Image.Image:
    phase %= 4
    im, d = canvas()
    p = g.PALETTES[variant]["dark"]
    plate(d, (5, 13, 123, 124), DEEP, 9)
    # Anchors and braces carry the rig instead of filling its footprint with a solid deck.
    for x in (16, 112):
        for y in (23, 110):
            plate(d, (x - 11, y - 11, x + 11, y + 11), DARK, 4)
            bolt(d, x, y)
    brace(d, (18, 106), (37, 80), 10)
    brace(d, (110, 106), (91, 80), 10)
    hose(d, [(111, 86), (118, 81), (118, 38), (105, 31), (99, 39)])
    plate(d, (100, 44, 121, 88), DARK, 3)
    vent(d, 104, 67, 13)
    box(d, (105, 49, 116, 60), p, 1)
    # Recess and rear lip are below the moving shaft.
    circle(d, (26, 63, 102, 116), VOID)
    circle(d, (27, 64, 101, 113), IRON)
    circle(d, (32, 68, 96, 108), DARK)
    circle(d, (36, 72, 92, 107), VOID)
    arc(d, (29, 65, 99, 111), 185, 355, EDGE, 2)
    for x, y in ((37, 75), (91, 75), (31, 88), (97, 88)):
        bolt(d, x, y)
    # Thick shaft with rounded helical flights. Its lower end is occluded by the front collar.
    box(d, (47, 20, 81, 106), VOID, 5)
    box(d, (51, 21, 77, 106), DARK, 4)
    box(d, (56, 21, 73, 104), IRON, 3)
    line(d, [(57, 24), (57, 103)], EDGE, 2)
    for j in range(-1, 6):
        base = 24 + j * 21 + phase * 5.25
        pts = [
            (64 + 18 * math.cos(t), base + 10 * t / math.pi)
            for t in [i * math.pi / 20 for i in range(21)]
        ]
        pts = [(x, y) for x, y in pts if 21 <= y <= 108]
        if len(pts) > 1:
            line(d, [(x, y + 4) for x, y in pts], VOID, 7)
            line(d, pts, IRON, 6)
            line(d, [(x, y - 1) for x, y in pts], STEEL, 2)
    # Two load-bearing cylinders and a moving crosshead.
    index = int(phase)
    blend = phase - index
    blend = blend * blend * (3 - 2 * blend)
    poses = (0, 2, 5, 5, 0)
    feed = poses[index] + (poses[index + 1] - poses[index]) * blend
    for x in (33, 95):
        box(d, (x - 6, 14, x + 6, 91), VOID, 3)
        box(d, (x - 3, 19, x + 3, 72), STEEL, 1)
        plate(d, (x - 9, 14, x + 9, 43), DARK, 3)
        box(d, (x - 5, 19, x + 5, 32), p, 2)
        plate(d, (x - 9, 73, x + 9, 94), DARK, 3)
        bolt(d, x, 83)
        line(d, [(x - 2, 45), (x - 2, 71)], g.BONE)
    plate(d, (23, 33 + feed, 105, 51 + feed), DARK, 4)
    plate(d, (43, 27 + feed, 85, 58 + feed), IRON, 5)
    box(d, (49, 32 + feed, 79, 43 + feed), p, 2)
    box(d, (51, 46 + feed, 77, 53 + feed), DEEP, 2)
    line(d, [(52, 47 + feed), (76, 47 + feed)], EDGE)
    for x in (30, 98):
        bolt(d, x, 42 + feed)
    plate(d, (43, 5, 85, 21), DARK, 4)
    vent(d, 50, 7, 28)
    # Front collar occludes the drill continuously. Its outer wall has real vertical thickness.
    outer = [
        (64 + 38 * math.cos(t), 89 + 27 * math.sin(t))
        for t in [i * math.pi / 32 for i in range(33)]
    ]
    inner = [
        (64 + 28 * math.cos(t), 87 + 18 * math.sin(t))
        for t in [i * math.pi / 32 for i in range(32, -1, -1)]
    ]
    poly(d, [(x, y + 5) for x, y in outer] + list(reversed(outer)), VOID)
    poly(d, outer + inner, IRON)
    line(d, inner, EDGE, 2)
    line(d, outer, DARK, 2)
    for x, y in ((35, 99), (47, 111), (81, 111), (93, 99)):
        bolt(d, x, y)
    plate(d, (50, 109, 78, 121), DARK, 3)
    box(d, (55, 113, 73, 117), p, 1)
    return finish(im)


def render_extractor(variant: str, phase: float) -> Image.Image:
    phase %= 4
    im, d = canvas()
    p = g.PALETTES[variant]["dark"]
    plate(d, (7, 9, 121, 121), DEEP, 8)
    # Same foundation and processing path, reduced cutter and drive housing.
    for x, y in ((16, 18), (112, 18), (16, 111), (112, 111)):
        plate(d, (x - 6, y - 5, x + 6, y + 6), DARK, 2)
        bolt(d, x, y)
    original._belt(d, (55, 74, 76, 107), phase, variant=variant, step=2)
    for j in range(3):
        yy = 81 + (j * 8 + phase * 2) % 24
        poly(d, [(59, yy), (64, yy - 2), (69, yy + 2), (63, yy + 4)], g.SCRAP_DARK)
    for x in (51, 80):
        line(d, [(x, 80), (x, 108)], IRON, 3)
    circle(d, (39, 37, 93, 90), VOID)
    circle(d, (41, 34, 91, 84), IRON)
    circle(d, (45, 38, 87, 80), DARK)
    circle(d, (49, 42, 83, 76), VOID)
    for n in range(6):
        a = n * math.tau / 6 + phase * math.pi / 12
        ux, uy = math.cos(a), math.sin(a)
        vx, vy = -uy, ux
        pts = [
            (66 + ux * r + vx * w, 59 + uy * r + vy * w)
            for r, w in ((6, -2), (17, -3), (19, 1), (6, 3))
        ]
        poly(d, pts, IRON)
        line(d, [pts[0], pts[1]], EDGE)
        line(d, [(66 + ux * 14, 59 + uy * 14), (66 + ux * 18, 59 + uy * 18)], STEEL, 3)
    for x in (25, 100):
        plate(d, (x - 6, 42, x + 6, 87), DARK, 2)
        box(d, (x - 3, 48, x + 3, 59), p, 1)
        line(d, [(x, 70), (x, 81)], IRON, 2)
    plate(d, (45, 22, 88, 38), DARK, 3)
    box(d, (50, 26, 66, 34), p, 1)
    vent(d, 70, 23, 12)
    plate(d, (49, 37, 83, 53), DARK, 3)
    plate(d, (57, 42, 76, 64), IRON, 3)
    box(d, (60, 45, 73, 53), DEEP, 1)
    circle(d, (60, 57, 73, 70), VOID)
    bolt(d, 66, 63)
    plate(d, (46, 82, 85, 91), DARK, 2)
    box(d, (55, 85, 76, 94), VOID, 1)
    original._hopper(d, (43, 104, 87, 118), variant)
    poly(d, [(53, 113), (59, 110), (65, 113), (73, 111), (78, 115)], g.SCRAP_DARK)
    for x in (26, 93):
        box(d, (x, 111, x + 9, 114), p, 1)
    return finish(im)
