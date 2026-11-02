"""Ground-combat calibration using the accepted machine materials."""

from PIL import Image, ImageDraw

from tools import gen_sprites as gen

SS = 4
SIZE = 128
VOID = (11, 11, 15)
DEEP = (24, 25, 31)
DARK = gen.IRON_DARK
IRON = gen.IRON
LIGHT = gen.IRON_LIGHT
EDGE = (112, 111, 119)
STEEL = (143, 140, 135)


def canvas():
    image = Image.new("RGBA", (SIZE * SS, SIZE * SS))
    return image, ImageDraw.Draw(image)


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


def finish(image, rim=True):
    image = image.resize((SIZE, SIZE), Image.Resampling.LANCZOS)
    return gen.rim_light(image) if rim else image


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


def hull(kind, variant, move=0):
    image, d = canvas()
    paint = gen.PALETTES[variant]["dark"]
    if kind == "sentinel":
        for bounds in ((15, 40, 36, 116), (92, 40, 113, 116)):
            track(d, bounds, move, paint)
        poly(
            d,
            [
                (33, 41),
                (45, 28),
                (83, 28),
                (95, 41),
                (91, 106),
                (80, 113),
                (48, 113),
                (37, 106),
            ],
            VOID,
        )
        poly(d, [(39, 44), (49, 34), (79, 34), (89, 44), (84, 103), (44, 103)], DARK)
        poly(d, [(42, 43), (51, 36), (77, 36), (86, 43), (83, 53), (45, 53)], IRON)
        for x in (39, 79):
            box(d, (x, 59, x + 10, 79), paint, 2)
        box(d, (45, 90, 83, 106), DEEP, 2)
        vent(d, 51, 92, 26)
        circle(d, (43, 42, 85, 84), VOID)
        circle(d, (47, 46, 81, 80), IRON)
        line(d, [(42, 44), (48, 38)], EDGE, 2)
    elif kind == "warden":
        for bounds in (
            (8, 25, 33, 62),
            (8, 76, 33, 115),
            (95, 25, 120, 62),
            (95, 76, 120, 115),
        ):
            track(d, bounds, move, paint)
        for x in (25, 84):
            box(d, (x, 47, x + 19, 92), IRON, 3)
            box(d, (x + 2, 54, x + 17, 86), DEEP, 2)
        poly(
            d,
            [
                (31, 31),
                (44, 19),
                (84, 19),
                (97, 31),
                (94, 104),
                (82, 115),
                (46, 115),
                (34, 104),
            ],
            VOID,
        )
        poly(
            d,
            [
                (38, 36),
                (49, 26),
                (79, 26),
                (90, 36),
                (86, 99),
                (77, 106),
                (51, 106),
                (42, 99),
            ],
            DARK,
        )
        for x in (36, 78):
            poly(
                d,
                [(x, 30), (x + 10, 23), (x + 16, 29), (x + 14, 51), (x + 1, 51)],
                paint,
            )
            line(d, [(x + 2, 31), (x + 9, 26)], EDGE, 2)
        box(d, (43, 88, 85, 107), paint, 3)
        vent(d, 50, 89, 28)
        circle(d, (39, 34, 89, 84), VOID)
        circle(d, (44, 39, 84, 79), IRON)
    else:
        for bounds in ((13, 62, 35, 116), (93, 62, 115, 116)):
            track(d, bounds, move, paint)
        poly(d, [(32, 69), (44, 50), (84, 50), (96, 69), (90, 112), (38, 112)], VOID)
        poly(d, [(39, 72), (48, 57), (80, 57), (89, 72), (84, 105), (44, 105)], DARK)
        for x in (37, 80):
            box(d, (x, 78, x + 11, 100), paint, 2)
        box(d, (48, 100, 80, 112), IRON, 2)
        line(d, [(44, 68), (51, 59)], EDGE, 2)
        circle(d, (42, 45, 86, 89), VOID)
        circle(d, (47, 50, 81, 84), IRON)
    return finish(image)


def mount(kind, variant, action=0):
    image, d = canvas()
    paint = gen.PALETTES[variant]["dark"]
    if kind == "sentinel":
        recoil = (0, 0, 7, 4, 1)[action]
        poly(d, [(47, 48), (55, 41), (73, 41), (81, 48), (78, 79), (50, 79)], VOID)
        poly(d, [(51, 51), (57, 46), (71, 46), (77, 51), (74, 74), (54, 74)], IRON)
        box(d, (50, 58, 56, 72), paint, 1)
        box(d, (72, 54, 78, 67), DEEP, 1)
        box(d, (59, 14 + recoil, 69, 60 + recoil), VOID, 2)
        box(d, (62, 17 + recoil, 66, 49 + recoil), STEEL, 1)
        box(d, (56, 48 + recoil, 72, 65 + recoil), DARK, 3)
        box(d, (59, 52 + recoil, 69, 63 + recoil), paint, 1)
        line(d, [(59, 50 + recoil), (68, 50 + recoil)], EDGE)
        box(d, (59, 14 + recoil, 69, 20 + recoil), DARK, 1)
        line(d, [(61, 15 + recoil), (67, 15 + recoil)], LIGHT)
        line(d, [(51, 51), (57, 46), (71, 46)], EDGE)
    elif kind == "warden":
        recoil = (0, 0, 9, 5, 1)[action]
        poly(d, [(44, 45), (54, 36), (74, 36), (84, 45), (81, 81), (47, 81)], VOID)
        poly(d, [(49, 47), (56, 41), (72, 41), (79, 47), (75, 75), (53, 75)], IRON)
        for x in (45, 75):
            box(d, (x, 47, x + 8, 70), DARK, 2)
            box(d, (x + 2, 51, x + 6, 66), STEEL, 1)
        box(d, (56, 11 + recoil, 72, 68 + recoil), VOID, 2)
        box(d, (59, 17 + recoil, 69, 52 + recoil), DARK, 1)
        line(d, [(60, 18 + recoil), (60, 48 + recoil)], STEEL, 2)
        box(d, (53, 48 + recoil, 75, 65 + recoil), DEEP, 3)
        box(d, (56, 52 + recoil, 72, 62 + recoil), paint, 1)
        box(d, (54, 9 + recoil, 74, 19 + recoil), DARK, 2)
        line(d, [(58, 10 + recoil), (70, 10 + recoil)], LIGHT)
    else:
        recoil = (0, 0, 0, 0, 8, 5, 1)[action]
        charge = {0: 0, 1: 0, 2: 1, 3: 2, 4: 0, 5: 0, 6: 0}[action]
        box(d, (45, 55, 83, 84), VOID, 5)
        box(d, (50, 60, 78, 78), IRON, 3)
        for x in (44, 73):
            box(d, (x, 11 + recoil, x + 11, 69 + recoil), VOID, 2)
            box(d, (x + 2, 14 + recoil, x + 9, 63 + recoil), DARK, 1)
            line(d, [(x + 3, 15 + recoil), (x + 3, 59 + recoil)], EDGE, 2)
            box(d, (x - 3, 53, x + 14, 72), DEEP, 2)
            box(d, (x + 1, 57, x + 9, 69), IRON, 1)
        box(d, (54, 53 + recoil, 74, 72 + recoil), DARK, 2)
        box(d, (57, 56 + recoil, 71, 65 + recoil), paint, 1)
        box(d, (51, 79, 77, 99), VOID, 2)
        for i, x in enumerate((54, 66)):
            box(d, (x, 82, x + 8, 96), DEEP, 2)
            box(d, (x + 2, 84, x + 6, 94), (144, 103, 50) if charge > i else IRON, 1)
        if charge == 2:
            line(d, [(60, 49), (60, 31)], (122, 101, 66))
            line(d, [(68, 49), (68, 31)], (122, 101, 66))
    # Report light belongs only to the muzzle on the actual attack pose.
    if action == (4 if kind == "lancer" else 2):
        tip = {"sentinel": 21, "warden": 18, "lancer": 19}[kind]
        poly(
            d,
            [(60, tip), (64, tip - 7), (68, tip), (65, tip + 3), (63, tip + 3)],
            (193, 170, 122),
        )
        line(d, [(64, tip - 3), (64, tip + 2)], gen.BONE, 2)
    return finish(image, rim=False)


def render(kind, variant, move=0, action=0):
    image = hull(kind, variant, move)
    image.alpha_composite(mount(kind, variant, action))
    return image


def render_breaker(variant, move=0, action=0):
    image, d = canvas()
    paint = gen.PALETTES[variant]["dark"]
    # Continuous running gear carries a fixed, braced gun cradle.
    for bounds in ((20, 29, 41, 112), (87, 29, 108, 112)):
        track(d, bounds, move, paint)
    poly(d, [(38, 35), (49, 24), (79, 24), (90, 35), (86, 107), (42, 107)], VOID)
    poly(d, [(44, 39), (52, 30), (76, 30), (84, 39), (80, 99), (48, 99)], DARK)
    for side in (-1, 1):
        poly(
            d,
            [(64 + side * x, y) for x, y in ((9, 32), (20, 39), (19, 63), (11, 67))],
            VOID,
        )
        poly(
            d,
            [(64 + side * x, y) for x, y in ((10, 35), (18, 40), (18, 60), (11, 63))],
            paint,
        )
        line(d, [(64 + side * 11, 37), (64 + side * 17, 41)], EDGE)
        line(d, [(64 + side * 11, 52), (64 + side * 18, 52)], VOID, 1.5)
        for y in (45, 57):
            x = 64 + side * 15
            circle(d, (x - 1, y - 1, x + 1, y + 1), IRON)
    box(d, (47, 64, 81, 87), DEEP, 2)
    for x in (47, 77):
        box(d, (x, 58, x + 4, 87), IRON, 1)
        line(d, [(x + 1, 59), (x + 1, 85)], EDGE)
    for side in (-1, 1):
        line(d, [(64 + side * 17, 66), (64 + side * 18, 90)], VOID, 2)
    box(d, (46, 87, 82, 106), VOID, 3)
    box(d, (48, 89, 80, 104), IRON, 2)
    box(d, (48, 89, 80, 94), paint, 1)
    vent(d, 53, 93, 22)
    chassis = finish(image)
    image, d = canvas()
    recoil = (0, 0, 10, 6, 2)[action]
    # The cylinders stay on the cradle while the tube and breech recoil.
    for x in (50, 74):
        box(d, (x, 40, x + 4, 62), VOID, 1)
        box(d, (x + 1, 41, x + 3, 61), IRON, 0)
        line(d, [(x + 1, 42), (x + 1, 60)], EDGE)
    box(d, (56, 19 + recoil, 72, 66 + recoil), VOID, 2)
    box(d, (59, 23 + recoil, 69, 64 + recoil), DARK, 1)
    line(d, [(60, 24 + recoil), (60, 60 + recoil)], STEEL, 2)
    box(d, (54, 14 + recoil, 74, 24 + recoil), VOID, 2)
    box(d, (56, 16 + recoil, 72, 22 + recoil), IRON, 1)
    line(d, [(57, 16 + recoil), (71, 16 + recoil)], LIGHT)
    box(d, (50, 58 + recoil, 78, 74 + recoil), VOID, 3)
    box(d, (53, 61 + recoil, 75, 71 + recoil), IRON, 2)
    line(d, [(54, 61 + recoil), (74, 61 + recoil)], EDGE)
    for x, y in ((56, 64), (72, 64), (56, 68), (72, 68)):
        circle(d, (x - 1, y + recoil - 1, x + 1, y + recoil + 1), LIGHT)
    if action == 2:
        poly(d, [(59, 18), (64, 10), (69, 18), (65, 22), (63, 22)], (193, 170, 122))
        line(d, [(64, 15), (64, 19)], gen.BONE, 2)
    chassis.alpha_composite(finish(image, rim=False))
    return chassis
