"""Four-point braced rail sled with independent anchors and recoil carriage."""

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
    STEEL,
    VOID,
    box,
    canvas,
    circle,
    finish,
    line,
    panel,
    poly,
    strut,
    vent,
)

PAINT = {"base": (174, 83, 55), "probe": (54, 145, 170)}
LIGHT = (222, 205, 167)
ANCHOR_STEPS = 12
MOUNT_CROP = (32, 0, 104, 112)
ANCHOR_CROP = (12, 41, 116, 113)
RELOAD_FRAMES = 100


def smooth(t):
    t = max(0.0, min(1.0, t))
    return t * t * (3 - 2 * t)


def span(t, a, b):
    return smooth((t - a) / (b - a))


def lerp(a, b, t):
    return a + (b - a) * t


def disk(d, x, y, r, c):
    circle(d, (x - r, y - r, x + r, y + r), c)


def hydraulic(d, a, b, width=5):
    strut(d, a, b, width)
    mid = (lerp(a[0], b[0], 0.58), lerp(a[1], b[1], 0.58))
    line(d, [mid, b], STEEL, max(1, width - 2))
    for x, y in (a, b):
        bolt(d, x, y)


def rail(d, x, y, yy):
    box(d, (x - 3, y, x + 3, yy), VOID, 1)
    line(d, [(x - 1, y + 1), (x - 1, yy - 1)], STEEL, 1)
    line(d, [(x + 1, y + 1), (x + 1, yy - 1)], IRON, 1)


def slug(d, x, y, vertical=True):
    if vertical:
        poly(
            d,
            [
                (x - 3, y + 7),
                (x - 3, y - 4),
                (x, y - 8),
                (x + 3, y - 4),
                (x + 3, y + 7),
            ],
            VOID,
        )
        box(d, (x - 2, y - 3, x + 2, y + 6), BRASS, 1)
        poly(d, [(x - 2, y - 3), (x, y - 7), (x + 2, y - 3)], STEEL)
        line(d, [(x - 1, y), (x - 1, y + 5)], LIGHT, 1)
    else:
        poly(
            d,
            [
                (x + 7, y - 3),
                (x - 4, y - 3),
                (x - 8, y),
                (x - 4, y + 3),
                (x + 7, y + 3),
            ],
            VOID,
        )
        box(d, (x - 3, y - 2, x + 6, y + 2), BRASS, 1)
        poly(d, [(x - 3, y - 2), (x - 7, y), (x - 3, y + 2)], STEEL)


def rigid(points, pivot, angle):
    c, s = math.cos(angle), math.sin(angle)
    x, y = pivot
    return [(x + px * c - py * s, y + px * s + py * c) for px, py in points]


def belt(draw, bounds, distance, paint):
    x0, y0, x1, y1 = bounds
    box(draw, bounds, VOID, 4)
    box(draw, (x0 + 2, y0 + 2, x1 - 2, y1 - 2), DARK, 2)
    box(draw, (x0 + 3, y0 + 4, x1 - 3, y1 - 4), DEEP, 1)
    top, bottom = y0 + 4, y1 - 4
    offset = (-distance) % 8
    for n in range(-1, 12):
        y = top + n * 8 + offset
        a, b = max(top, y), min(bottom, y + 3)
        if b > a:
            box(draw, (x0 + 2, a, x1 - 2, b), IRON)
            line(draw, [(x0 + 3, a), (x1 - 3, a)], EDGE, 0.7)
    line(draw, [(x0 + 1.5, y0 + 6), (x0 + 1.5, y1 - 6)], EDGE)
    for y in (y0 + 7, y1 - 8):
        box(draw, (x0 + 4, y - 2, x1 - 4, y + 2), paint, 1)
    for y in (y0 + 3, y1 - 3):
        bolt(draw, (x0 + x1) / 2, y)


def anchor(draw, root, length, angle, deploy, sign, style, paint):
    """Fixed beam and shoe swing as an assembly; only the actuator telescopes."""
    x, y = root
    end = rigid([(0, length)], root, angle)[0]
    beam = rigid(
        [
            (-3, -2),
            (3, -2),
            (4, length - 5),
            (2, length + 1),
            (-2, length + 1),
            (-4, length - 5),
        ],
        root,
        angle,
    )
    poly(draw, beam, VOID)
    poly(
        draw,
        rigid(
            [(-1.8, 1), (1.8, 1), (2.5, length - 5), (-2.5, length - 5)], root, angle
        ),
        IRON,
    )
    line(draw, rigid([(-1.5, 2), (-1.5, length - 6)], root, angle), EDGE, 1)
    line(draw, rigid([(1.1, 8), (1.1, length - 9)], root, angle), paint, 1.5)
    piston_end = rigid([(0, length * 0.72)], root, angle)[0]
    hydraulic(draw, (x - sign * 6, y - 9), piston_end, 3)
    # The shoe hinges off the beam's end and settles broadside to recoil.
    shoe_angle = angle * (1 - smooth((deploy - 0.48) / 0.52))
    w, h = ((7, 6), (9, 7), (6, 5))[style]
    shoe = rigid(
        [(-w + 2, -h), (w - 2, -h), (w, -h + 2), (w, h), (-w, h), (-w, -h + 2)],
        end,
        shoe_angle,
    )
    poly(draw, shoe, VOID)
    poly(
        draw,
        rigid(
            [(-w + 2, -h + 2), (w - 2, -h + 2), (w - 1, h - 1), (-w + 1, h - 1)],
            end,
            shoe_angle,
        ),
        DARK,
    )
    poly(
        draw,
        rigid(
            [(-w + 2, 1), (w - 2, 1), (w - 2, h - 1), (-w + 2, h - 1)], end, shoe_angle
        ),
        IRON,
    )
    line(draw, rigid([(-w + 2, -h + 2), (w - 2, -h + 2)], end, shoe_angle), STEEL, 1)
    for px in (-w + 3, 0, w - 3):
        line(draw, rigid([(px, 2), (px, h - 1)], end, shoe_angle), EDGE, 1)
    bolt(draw, *end)
    disk(draw, x, y, 5, VOID)
    disk(draw, x, y, 3, IRON)
    bolt(draw, x, y)


def _hull(variant: str, move: int = 0):
    im, d = canvas()
    paint = PAINT[variant]
    distance = move * 8 / 3
    # Chassis rails carry the hinge loads into the recoil cradle.
    for bounds in ((34, 43, 48, 113), (80, 43, 94, 113)):
        belt(d, bounds, distance, paint)
    panel(d, [(48, 39), (80, 39), (83, 105), (77, 116), (51, 116), (45, 105)], DARK)
    for x in (48, 80):
        line(d, [(x, 43), (x, 106)], IRON, 3)
        line(d, [(x - 1, 43), (x - 1, 105)], EDGE, 1)
    plate(d, (49, 87, 79, 114), IRON, 3)
    plate(d, (52, 89, 76, 111), DARK, 2)
    vent(d, 66, 92, 10)
    box(d, (51, 91, 59, 105), paint, 1)
    for y in (44, 78, 109):
        for x in (49, 79):
            bolt(d, x, y)
    return im


def _anchors(draw, variant: str, deployment: float):
    paint = PAINT[variant]
    for rear, y in ((False, 56), (True, 99)):
        for sign in (-1, 1):
            root = (64 + sign * 20, y)
            plate(draw, (root[0] - 5, y - 5, root[0] + 5, y + 5), DARK, 2)
            amount = smooth((deployment - (0.1 if rear else 0)) / 0.9)
            angle = (
                (-sign * math.radians(76) * amount)
                if not rear
                else -sign * (math.pi - math.radians(106) * amount)
            )
            anchor(draw, root, 20, angle, amount, sign, 2, paint)


def render_body(variant: str, deployment: float = 0.0, move: int = 0):
    image = _hull(variant, move)
    _anchors(ImageDraw.Draw(image), variant, deployment)
    return finish(image)


def render_anchors(variant: str, deployment: float):
    image, draw = canvas()
    _anchors(draw, variant, deployment)
    return finish(image)


def render_gun(variant: str, age: float | None = None):
    im, d = canvas()
    paint = PAINT[variant]
    age = -1 if age is None else age
    recoil = 0 if age < 0 else 11 * span(age, 0, 2) * (1 - span(age, 2, 21))
    opening = span(age, 15, 25) * (1 - span(age, 82, 96)) if age >= 0 else 0
    x = 64
    for xx in (x - 12, x + 12):
        rail(d, xx, 32, 87)
    plate(d, (x - 16, 57, x + 16, 71), IRON, 3)
    # Each gun has one bore; twin visible rods are recoil cylinders, not barrels.
    width = 5
    top = 8 + recoil
    box(d, (x - width - 2, top, x + width + 2, 58 + recoil), VOID, 2)
    box(d, (x - width, top + 2, x + width, 57 + recoil), DARK, 1)
    line(d, [(x - width + 1, top + 4), (x - width + 1, 49 + recoil)], STEEL, 1)
    for yy in (22, 36, 49):
        box(d, (x - width - 2, yy + recoil, x + width + 2, yy + 4 + recoil), IRON, 1)
        line(d, [(x - width, yy + recoil), (x + width, yy + recoil)], EDGE)
    box(d, (x - width - 3, top, x + width + 3, top + 6), IRON, 1)
    box(d, (x - width, top + 1, x + width, top + 3), VOID)
    for xx in (x - 11, x + 11):
        box(d, (xx - 2, 47, xx + 2, 72), VOID, 1)
        line(d, [(xx, 47 + recoil), (xx, 71)], STEEL, 2)
        box(d, (xx - 3, 67, xx + 3, 80), DARK, 1)
    plate(d, (x - 10, 51 + recoil, x + 10, 70 + recoil), DARK, 3)
    box(d, (x - 6, 54 + recoil, x + 6, 60 + recoil), paint, 1)
    # Slide gate opens sideways, leaving the loading tray visible until the slug seats.
    plate(d, (x - 8, 69, x + 8, 100), DEEP, 2)
    for xx in (x - 7, x + 7):
        line(d, [(xx, 75), (xx, 97)], EDGE)
    gate = x + opening * 13
    plate(d, (gate - 8, 68 + recoil, gate + 8, 76 + recoil), IRON, 2)
    box(d, (gate - 5, 70 + recoil, gate + 5, 73 + recoil), DARK, 1)
    # External magazine is a supply hatch; rounds only emerge along its delivery path.
    mx = x - 19
    my = 91
    plate(d, (mx - 6, 79, mx + 6, 109), DARK, 2)
    slug(d, mx, 98)

    # The transfer arm hands the round to the central rammer, then returns home.
    cross = span(age, 25, 48) if age >= 25 else 0
    home = span(age, 49, 75) if age >= 49 else 0
    arm_x = lerp(lerp(mx, x, cross), mx, home)
    arm_y = lerp(lerp(my + 7, 92, cross), my + 7, home)
    hydraulic(d, (mx, 106), (arm_x, arm_y), 2)
    if 25 <= age < 80:
        feed = span(age, 53, 79)
        sx = lerp(mx, x, cross)
        sy = lerp(my, 85, cross) - 22 * feed
        payload, pd = canvas()
        slug(pd, sx, sy)
        # The receiver hides the rigid round progressively as the rammer seats it.
        payload.paste((0, 0, 0, 0), (0, 0, 512, 76 * 4))
        im.alpha_composite(payload)
    ram = (
        101
        if age < 25
        else lerp(101, 78, span(age, 48, 78))
        if age < 81
        else lerp(78, 101, span(age, 81, 96))
    )
    box(d, (x - 2, ram, x + 2, 103), IRON, 1)
    line(d, [(x, ram), (x, 101)], STEEL)
    # Breech housing occludes the round as it crosses into the chamber.
    box(d, (x - 7, 61 + recoil, x + 7, 68 + recoil), DARK, 1)
    if 0 <= age < 3:
        reach = 6 if age != 1 else 8
        poly(
            d,
            [
                (x - width, top),
                (x - 4, top - 5),
                (x, top - reach),
                (x + 4, top - 5),
                (x + width, top),
            ],
            BRASS,
        )
        poly(d, [(x - 3, top), (x, top - reach + 4), (x + 3, top)], LIGHT)
    if 15 <= age < 28:
        u = (age - 15) / 13
        ex = x + 14 + 20 * u
        ey = 70 - 9 * math.sin(u * math.pi)
        casing, cd = canvas()
        box(cd, (ex - 3, ey - 1, ex + 3, ey + 1), BRASS, 1)
        casing.putalpha(
            casing.getchannel("A").point(lambda a: round(a * (1 - span(u, 0.45, 1))))
        )
        im.alpha_composite(casing)
    head = finish(im, False)
    return head


def install_piledriver(registry: dict[str, Image.Image], out: Path) -> None:
    for variant in gen.PALETTES:
        tag = gen.variant_tag(variant)
        frames = {}
        gun = render_gun(variant)
        for move in range(3):
            suffix = f"_move{move}" if move else ""
            image = render_body(variant, 0.0, move)
            image.alpha_composite(gun)
            frames[f"piledriver{tag}{suffix}"] = image
            frames[f"rig_piledriver_hull{tag}{suffix}"] = finish(_hull(variant, move))
        frames[f"rig_piledriver_mount{tag}"] = gun.crop(MOUNT_CROP)
        body = render_body(variant, 1.0)
        for age in range(RELOAD_FRAMES):
            gun = render_gun(variant, age)
            frames[f"rig_piledriver_mount{tag}_action{age + 1}"] = gun.crop(MOUNT_CROP)
            image = body.copy()
            image.alpha_composite(gun)
            frames[f"piledriver{tag}_action{age + 1}"] = image
        for step in range(ANCHOR_STEPS + 1):
            suffix = f"_action{step}" if step else ""
            frames[f"piledriver_anchors{tag}{suffix}"] = render_anchors(
                variant, step / ANCHOR_STEPS
            ).crop(ANCHOR_CROP)
        for key, image in frames.items():
            gen.save_sprite(image, out, key)
            registry[key] = image
