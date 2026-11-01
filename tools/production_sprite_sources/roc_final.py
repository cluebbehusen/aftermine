"""The Roc and its drones.

The Roc is the approved Longbow: a raised narrow cowling between two forward
lift shoulders, two covered aft thrusters, six drone docks on two rails and
two guns on plates bolted outside the lift shoulders. Its drawing code is ported from the approved study,
with the study's clock replaced by the state the shell supplies: fan phase,
each dock's clamp spread, each gun's bearing and shot age, and where each
drone sits. The shell draws it from runtime layers on one 256px canvas (four
tiles), north-facing, pivoting about the canvas centre, in this order:

- the hull's ground shadow, with the yokes' shadow beside it, drawn
  `HULL_SHADOW` pixels off in screen space;
- each gun's ground shadow, `GUN_SHADOW` pixels off;
- the wall, the hull's silhouette dropped toward the tail so the hull reads
  as standing proud of it;
- the fixed gun plates, then each gun on its own canvas about its pivot,
  under the hull;
- the hull, with the fan blades and the docks' clamps cut out;
- each fan's blades at one of `FAN_FRAMES` phases;
- each dock's clamps at one of `CLAMP_STEPS` spreads, and a docked drone;
- the cargo hatch over the spine vent, opening as riders board or leave;
- the aft thrusters' glow, drawn at the engines' heat as the Roc moves.

A destroyed Roc falls as its wall, yokes and guns at their last bearings
under a wreck hull: the hull with its fans still and its docks empty.

A drone's frame (arms and rotors) and its body (gun and accent) are separate
layers on a small canvas, so the frame follows its flight and the body its
gun's bearing.

Every layer is the study's drawing moved `CENTRE` pixels toward the nose,
because the study turns the ship about a point that far behind its canvas
centre.
"""

from __future__ import annotations

import functools
import math
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

from tools import gen_sprites as gen
from tools.production_sprite_sources import maw_final as maw

Registry = dict[str, Image.Image]

SS = 3
SIZE = 256
CENTRE = 7

VOID = (12, 13, 17)
DEEP = (25, 27, 32)
DARK = (48, 47, 53)
IRON = (72, 71, 77)
EDGE = (108, 106, 111)
STEEL = (152, 146, 139)
LIGHT = (199, 187, 166)
RUST = (111, 72, 49)
BRASS = (137, 111, 64)
FLASH = (222, 170, 89)
DRONE_FLASH = (222, 175, 96)
PAINT = {"base": (154, 73, 48), "probe": (40, 139, 158)}

MOUNTS = ((46, 110), (210, 110))
DOCKS = ((102, 147), (102, 172), (102, 197), (154, 197), (154, 172), (154, 147))
FANS = ((76, 120), (180, 120))
EXHAUSTS = ((81, 218), (175, 218))
PARK = (0.0, 0.0)

GUN = 128
DRONE = 32
FAN = 40
CLAMP = 32
FAN_FRAMES = 6
CLAMP_STEPS = 4
ROTOR_FRAMES = 3
ROTOR_STEP = 5
RECOIL_AGES = (0, 1, 2, 4, 6, 8, 10)
HATCH = (32, 56)
HATCH_CENTRE = (128, 165)
HATCH_STEPS = 5
AMBER = (240, 200, 120)
HOLD = (96, 74, 44)
HOLD_LIGHT = (150, 116, 64)
CRADLE_STAGES = 3
HOT = ((214, 128, 52), (236, 158, 70), (250, 214, 150))
KILN = (196, 112, 48)
THRUSTERS = (81, 175)
NO_SHOT = 999
HULL_SHADOW = (5, 12)
GUN_SHADOW = (3, 7)
DRONE_SHADOW = (3, 7)


def smooth(x: float) -> float:
    x = max(0.0, min(1.0, x))
    return x * x * (3 - 2 * x)


def ramp(t: float, a: float, b: float) -> float:
    return smooth((t - a) / (b - a))


def mix(a: float, b: float, u: float) -> float:
    return a + (b - a) * u


def point(a, b, u):
    return (mix(a[0], b[0], u), mix(a[1], b[1], u))


class Draw:
    def __init__(self, size: int = SIZE):
        self.im = Image.new("RGBA", (size * SS, size * SS))
        self.d = ImageDraw.Draw(self.im)

    def poly(self, pts, c):
        self.d.polygon([(round(x * SS), round(y * SS)) for x, y in pts], fill=c)

    def line(self, pts, c, w=1):
        self.d.line(
            [(round(x * SS), round(y * SS)) for x, y in pts],
            fill=c,
            width=max(1, round(w * SS)),
            joint="curve",
        )

    def box(self, b, c, r=0):
        self.d.rounded_rectangle(tuple(round(v * SS) for v in b), radius=round(r * SS), fill=c)

    def disk(self, x, y, r, c):
        self.d.ellipse(tuple(round(v * SS) for v in (x - r, y - r, x + r, y + r)), fill=c)

    def plate(self, b, c=DARK, cut=4):
        x, y, xx, yy = b
        p = [
            (x + cut, y),
            (xx - cut, y),
            (xx, y + cut),
            (xx, yy - cut),
            (xx - cut, yy),
            (x + cut, yy),
            (x, yy - cut),
            (x, y + cut),
        ]
        self.poly(p, VOID)
        self.poly(
            [
                (x + cut, y + 2),
                (xx - cut, y + 2),
                (xx - 2, y + cut),
                (xx - 2, yy - cut),
                (xx - cut, yy - 2),
                (x + cut, yy - 2),
                (x + 2, yy - cut),
                (x + 2, y + cut),
            ],
            c,
        )
        self.line([(x + 2, y + cut), (x + cut, y + 2), (xx - cut, y + 2)], EDGE, 1)
        self.line([(x + cut, yy - 2), (xx - cut, yy - 2), (xx - 2, yy - cut)], DEEP, 1)

    def bolt(self, x, y, r=2):
        self.disk(x, y, r + 1, VOID)
        self.disk(x, y, r, IRON)
        self.line([(x - r * 0.5, y - r * 0.5), (x + r * 0.4, y - r * 0.5)], STEEL, 0.8)

    def beam(self, a, b, w=8, c=DARK):
        dx, dy = b[0] - a[0], b[1] - a[1]
        n = math.hypot(dx, dy)
        nx, ny = -dy / n * w / 2, dx / n * w / 2
        self.poly(
            [
                (a[0] + nx, a[1] + ny),
                (b[0] + nx, b[1] + ny),
                (b[0] - nx, b[1] - ny),
                (a[0] - nx, a[1] - ny),
            ],
            VOID,
        )
        self.line([a, b], c, w - 3)
        self.line(
            [(a[0] + nx * 0.65, a[1] + ny * 0.65), (b[0] + nx * 0.65, b[1] + ny * 0.65)], EDGE, 1
        )

    def hydraulic(self, a, b, w=8):
        self.line([a, b], VOID, w + 2)
        self.line([a, b], STEEL, w - 3)
        mid = point(a, b, 0.58)
        self.beam(a, mid, w + 2, IRON)
        self.line([point(a, b, 0.08), point(a, b, 0.46)], RUST, 2)
        self.bolt(*a)
        self.bolt(*b)

    def hose(self, pts):
        self.line(pts, VOID, 3)
        self.line(pts, RUST, 1)

    def vent(self, b):
        x, y, xx, yy = b
        self.box(b, VOID, 2)
        for i in range(round(y + 3), round(yy - 1), 4):
            self.line([(x + 3, i), (xx - 3, i)], IRON, 1.4)

    def finish(self) -> Image.Image:
        size = self.im.width // SS
        return self.im.resize((size, size), Image.Resampling.LANCZOS)


def polygon(d: Draw, pts, c=DARK) -> None:
    d.poly(pts, VOID)
    cx = sum(x for x, y in pts) / len(pts)
    cy = sum(y for x, y in pts) / len(pts)
    inner = [(cx + (x - cx) * 0.965, cy + (y - cy) * 0.965) for x, y in pts]
    d.poly(inner, c)
    d.line(inner[: max(2, len(inner) // 2)], EDGE, 1)


def fan(d: Draw, x, y, r, t, p) -> None:
    """A ducted fan at tick `t` of its thirty-tick turn; under eight pixels,
    a drone's single-bar rotor."""
    if r < 8:
        d.disk(x, y, r, VOID)
        d.disk(x, y, r - 0.7, EDGE)
        d.disk(x, y, r - 1.4, DEEP)
        a = math.tau * (t % 30) / 30
        d.line(
            [
                (x + (r - 1.8) * math.cos(a), y + (r - 1.8) * math.sin(a)),
                (x - (r - 1.8) * math.cos(a), y - (r - 1.8) * math.sin(a)),
            ],
            IRON,
            0.7,
        )
        d.disk(x, y, 0.65, STEEL)
        d.line([(x - r + 1, y - 1), (x - r + 1, y + 1)], p, 0.7)
        return
    d.disk(x, y, r, VOID)
    d.disk(x, y, r - 1.5, IRON)
    d.disk(x, y, r - 4, DEEP)
    phase = math.tau * (t % 30) / 30
    for k in range(5):
        a = phase + k * math.tau / 5
        pts = [
            (x + rad * math.cos(ang), y + rad * math.sin(ang))
            for rad, ang in [(3, a), (r - 5, a + 0.12), (r - 5, a + 0.47), (4, a + 0.8)]
        ]
        d.poly(pts, DARK)
        d.line(pts[:2], EDGE, 0.8)
    d.disk(x, y, 3.4, VOID)
    d.disk(x, y, 2, STEEL)
    for dx, dy in [(0, -r + 2), (0, r - 2), (-r + 2, 0), (r - 2, 0)]:
        if dx:
            d.box((x + dx - 1, y - 3, x + dx + 1, y + 3), p, 1)
        else:
            d.line([(x - 4, y + dy), (x + 4, y + dy)], STEEL, 0.8)


def rim(d: Draw) -> None:
    alpha = d.im.getchannel("A")
    edge = ImageChops.subtract(alpha.filter(ImageFilter.MaxFilter(2 * SS + 1)), alpha)
    shifted = ImageChops.subtract(
        edge, edge.transform(edge.size, Image.Transform.AFFINE, (1, 0, -SS, 0, 1, -SS))
    )
    layer = Image.new("RGBA", d.im.size, (255, 244, 224, 0))
    layer.putalpha(shifted.point(lambda v: min(v, 110)))
    d.im.alpha_composite(layer)


def shadow(im: Image.Image, dx=5, dy=10, opacity=90, blur=2) -> Image.Image:
    alpha = (
        im.getchannel("A")
        .filter(ImageFilter.GaussianBlur(blur * SS))
        .point(lambda n: round(n * opacity / 255))
    )
    layer = Image.new("RGBA", im.size, (0, 0, 0, 0))
    layer.putalpha(alpha)
    out = Image.new("RGBA", im.size)
    out.alpha_composite(layer, (round(dx * SS), round(dy * SS)))
    return out


def transformed(im: Image.Image) -> Image.Image:
    """The study's ship pose at rest: no drift, no heading."""
    im = im.rotate(0, Image.Resampling.BICUBIC, center=(128 * SS, 135 * SS))
    out = Image.new("RGBA", im.size)
    out.alpha_composite(im, (0, 0))
    return out


# Hull: the Longbow's parts, as the study draws them.


def pipe(d: Draw, pts, w=3) -> None:
    d.line(pts, VOID, w + 2)
    d.line(pts, IRON, w)
    d.line([(x - 0.6, y - 0.6) for x, y in pts], STEEL, 0.8)


def reactor(d: Draw, x, y, w=22, h=29) -> None:
    d.plate((x - w / 2 - 3, y - h / 2 - 2, x + w / 2 + 3, y + h / 2 + 2), DEEP, 5)
    d.box((x - w / 2, y - h / 2, x + w / 2, y + h / 2), IRON, 5)
    d.box((x - w / 2 + 3, y - h / 2 + 2, x + w / 2 - 3, y + h / 2 - 2), DARK, 3)
    d.line([(x - w / 2 + 3, y - h / 2 + 5), (x - w / 2 + 3, y + h / 2 - 5)], EDGE, 1)
    for yy in (y - h / 2 + 6, y + h / 2 - 6):
        d.line([(x - w / 2, yy), (x + w / 2, yy)], VOID, 3)
        d.line([(x - w / 2 + 1, yy - 1), (x + w / 2 - 1, yy - 1)], STEEL, 0.8)
    for side in (-1, 1):
        d.box((x + side * (w / 2 + 3) - 1, y - 5, x + side * (w / 2 + 3) + 1, y + 5), RUST, 1)


def nose(d: Draw, x, y, w, h, p, blunt=8) -> None:
    polygon(
        d,
        [
            (x - blunt / 2, y),
            (x + blunt / 2, y),
            (x + w / 2, y + 12),
            (x + w / 2, y + h - 5),
            (x + w / 2 - 5, y + h),
            (x - w / 2 + 5, y + h),
            (x - w / 2, y + h - 5),
            (x - w / 2, y + 12),
        ],
        IRON,
    )
    polygon(
        d,
        [
            (x - blunt / 2 + 1, y + 4),
            (x + blunt / 2 - 1, y + 4),
            (x + w / 2 - 5, y + 14),
            (x + w / 2 - 5, y + h - 7),
            (x - w / 2 + 5, y + h - 7),
            (x - w / 2 + 5, y + 14),
        ],
        DARK,
    )
    d.line([(x - w / 2 + 7, y + 17), (x + w / 2 - 7, y + 17)], p, 5)
    d.vent((x - w / 2 + 7, y + h - 22, x + w / 2 - 7, y + h - 11))
    d.line(
        [
            (x - w / 2 + 6, y + 23),
            (x - w / 2 + 10, y + 27),
            (x + w / 2 - 10, y + 27),
            (x + w / 2 - 6, y + 23),
        ],
        DEEP,
        1,
    )
    for side in (-1, 1):
        polygon(
            d,
            [
                (x + side * (w / 2 - 3), y + 20),
                (x + side * (w / 2 - 7), y + 24),
                (x + side * (w / 2 - 7), y + h - 10),
                (x + side * (w / 2 - 3), y + h - 15),
            ],
            IRON,
        )
        d.bolt(x + side * (w / 2 - 6), y + h - 9, 0.7)
    if h > 60:
        d.plate((x - 6, y + 32, x + 6, y + 43), IRON, 2)
        d.line([(x - 3, y + 35), (x + 3, y + 35)], EDGE, 0.8)
    for side in (-1, 1):
        d.line([(x + side * (w / 2 - 3), y + 19), (x + side * (w / 2 - 3), y + h - 10)], RUST, 1)


def engine(d: Draw, x, y, w, h, t, p, rotor=False) -> None:
    d.plate((x - w / 2, y - h / 2, x + w / 2, y + h / 2), DARK, 7)
    polygon(
        d,
        [
            (x - w / 2 + 3, y - h / 2 + 10),
            (x - w / 2 + 9, y - h / 2 + 3),
            (x + w / 2 - 9, y - h / 2 + 3),
            (x + w / 2 - 3, y - h / 2 + 10),
            (x + w / 2 - 4, y + h / 2 - 8),
            (x - w / 2 + 4, y + h / 2 - 8),
        ],
        IRON,
    )
    if rotor:
        fan(d, x, y - 1, w / 2 - 4, t, p)
        for side in (-1, 1):
            d.line([(x + side * (w / 2 - 2), y - 5), (x + side * (w / 2 - 2), y + 5)], DARK, 2)
    else:
        d.plate((x - w / 2 + 6, y - h / 2 + 6, x + w / 2 - 6, y + h / 2 - 8), DARK, 3)
        d.box((x - w / 2 + 8, y - h / 2 + 9, x + w / 2 - 8, y - h / 2 + 15), p, 1)
        d.vent((x - w / 2 + 8, y - h / 2 + 20, x + w / 2 - 8, y + h / 2 - 12))
    d.box((x - w / 2 + 8, y + h / 2 - 7, x + w / 2 - 8, y + h / 2 - 2), VOID, 1)
    d.line([(x - w / 2 + 9, y + h / 2 - 4), (x + w / 2 - 9, y + h / 2 - 4)], BRASS, 1)
    for xx in (x - w / 2 + 5, x + w / 2 - 5):
        d.bolt(xx, y + h / 2 - 9, 1)


def crossmember(d: Draw, y, left, right, width=12) -> None:
    d.beam((left, y), (right, y), width, IRON)
    for x in range(round(left + 8), round(right - 10), 22):
        d.line([(x, y - width / 2 + 3), (x + 12, y + width / 2 - 3)], STEEL, 0.8)


def docking_rail(d: Draw, x, top, bottom) -> None:
    d.plate((x - 15, top, x + 15, bottom), DEEP, 4)
    for side in (-1, 1):
        xx = x + side * 13
        d.line([(xx, top + 7), (xx, bottom - 7)], IRON, 3)
        d.line([(xx - 0.6, top + 8), (xx - 0.6, bottom - 8)], EDGE, 0.7)


def dock(d: Draw, x, y, spread, p) -> None:
    """A drone dock whose clamps stand `spread` pixels open."""
    d.plate((x - 11, y - 11, x + 11, y + 11), IRON, 2)
    d.box((x - 8, y - 8, x + 8, y + 8), VOID, 1)
    d.line([(x - 5, y + 7), (x + 5, y + 7)], BRASS, 1)
    for side in (-1, 1):
        d.box((x + side * (6 + spread) - 1, y - 5, x + side * (6 + spread) + 1, y + 5), STEEL, 1)
    d.disk(x - 8, y + 9, 0.8, p)


def spine_hatch(d: Draw, p, opening: float) -> None:
    """The cargo hatch over the spine vent, its two leaves `opening` of the
    way apart to show the lit hold."""
    x0, y0, x1, y1 = 118, 146, 138, 182
    d.box((x0 - 1, y0 - 1, x1 + 1, y1 + 1), VOID, 2)
    if opening > 0:
        d.box((x0 + 2, y0 + 2, x1 - 2, y1 - 2), HOLD, 1)
        d.box((124, 150, 132, 178), HOLD_LIGHT, 1)
    gap = 9 * opening
    for side in (-1, 1):
        b = (x0, y0, 128 - gap, y1) if side < 0 else (128 + gap, y0, x1, y1)
        if b[2] - b[0] > 1:
            d.box(b, IRON, 1)
            d.line([(b[0] + 1, y0 + 2), (b[0] + 1, y1 - 2)], EDGE, 0.8)
            for yy in (154, 164, 174):
                d.line([(b[0] + 2, yy), (b[2] - 2, yy)], DARK, 1)
    d.box((126, y1 + 1, 130, y1 + 4), p, 0.6)


def hull(d: Draw, p, fan_tick: int, spreads, hatch: float | None = None) -> None:
    """The Longbow's hull; with `hatch`, the cargo hatch that production
    adds over the spine vent, that far open."""
    for y in (115, 195):
        crossmember(d, y, 76, 180, 12)
        for side in (-1, 1):
            d.beam((128 + side * 13, y - 18), (128 + side * 45, y), 10, IRON)
    polygon(
        d,
        [
            (122, 32),
            (134, 32),
            (146, 54),
            (152, 107),
            (149, 192),
            (140, 221),
            (116, 221),
            (107, 192),
            (104, 107),
            (110, 54),
        ],
        DARK,
    )
    nose(d, 128, 36, 31, 62, p, 10)
    polygon(
        d,
        [
            (115, 92),
            (141, 92),
            (147, 109),
            (144, 185),
            (136, 203),
            (120, 203),
            (112, 185),
            (109, 109),
        ],
        IRON,
    )
    d.plate((116, 98, 140, 187), DARK, 6)
    reactor(d, 128, 124, 15, 35)
    d.vent((120, 150, 136, 178))
    for side in (-1, 1):
        x = 128 + side * 52
        engine(d, x, 121, 38, 55, fan_tick, p, True)
        x = 128 + side * 47
        engine(d, x, 202, 29, 38, fan_tick, p)
        pipe(d, [(128 + side * 18, 109), (128 + side * 20, 145), (128 + side * 18, 177)], 2.4)
    for x in (102, 154):
        docking_rail(d, x, 134, 212)
    d.plate((116, 204, 140, 227), IRON, 5)
    d.vent((121, 209, 135, 221))
    for (x, y), spread in zip(DOCKS, spreads):
        dock(d, x, y, spread, p)
    if hatch is not None:
        spine_hatch(d, p, hatch)


def yoke(d: Draw, side: int) -> None:
    """A gun's plate, bolted outside its fan pod, and the seat it turns on,
    drawn in the port gun's coordinates and mirrored for starboard."""
    x, y = MOUNTS[0]
    mirrored = (lambda v: v) if side == 0 else (lambda v: SIZE - v)
    left, right = sorted((mirrored(40), mirrored(60)))
    d.plate((left, 100, right, 128), DARK, 5)
    d.bolt(mirrored(44), 124, 1)
    d.line([(mirrored(43), 112), (mirrored(43), 120)], IRON, 2)
    left, right = sorted((mirrored(x - 9), mirrored(x + 9)))
    d.plate((left, y - 4, right, y + 13), DEEP, 3)


def recoil(age: float) -> float:
    return 4 * ramp(age, 0, 2) * (1 - ramp(age, 2, 12)) if age < 12 else 0


def gun(d: Draw, x, y, bearing: float, age: float, p) -> None:
    """A twin-barrelled gun on its bearing about pivot (`x`, `y`), `age`
    ticks after its last shot."""
    angle = math.radians(bearing)
    kick = recoil(age)

    def tr(px, py):
        return (
            x + px * math.cos(angle) - py * math.sin(angle),
            y + px * math.sin(angle) + py * math.cos(angle),
        )

    def poly(pts, c):
        d.poly([tr(a, b) for a, b in pts], c)

    def line(pts, c, w=1):
        d.line([tr(a, b) for a, b in pts], c, w)

    d.disk(x, y, 8, IRON)
    d.disk(x, y, 6, DARK)
    for sx in (-9, 9):
        a = tr(sx, -4)
        b = tr(sx, 7)
        d.beam(a, b, 4, IRON)
        d.bolt(*tr(sx, 0), 2)
    d.hose([tr(10, 9), tr(14, 4), tr(13, -5), tr(8, -10)])
    for n in range(5):
        pt = tr(12, 7 - n * 3)
        d.line([(pt[0] - 1, pt[1]), (pt[0] + 1, pt[1])], BRASS, 1.5)
    for bx in (-2.8, 2.8):
        poly(
            [
                (bx - 2.4, -46 + kick),
                (bx + 2.4, -46 + kick),
                (bx + 2.8, -9 + kick),
                (bx - 2.8, -9 + kick),
            ],
            VOID,
        )
        line([(bx - 0.7, -44 + kick), (bx - 0.7, -10 + kick)], STEEL, 2)
        for by in (-38, -29, -20):
            line([(bx - 2.4, by + kick), (bx + 2.4, by + kick)], IRON, 2)
        poly(
            [
                (bx - 3, -48 + kick),
                (bx + 3, -48 + kick),
                (bx + 3, -43 + kick),
                (bx - 3, -43 + kick),
            ],
            IRON,
        )
        line([(bx - 1.8, -47 + kick), (bx + 1.8, -47 + kick)], VOID, 1.2)
        if age < 2:
            poly(
                [
                    (bx - 2, -49 + kick),
                    (bx - 4, -53 + kick),
                    (bx, -59 + kick),
                    (bx + 4, -53 + kick),
                    (bx + 2, -49 + kick),
                ],
                FLASH,
            )
            line([(bx, -50 + kick), (bx, -55 + kick)], LIGHT, 2)
    poly(
        [
            (-7, -18 + kick),
            (7, -18 + kick),
            (9, -10 + kick),
            (7, 8 + kick),
            (-7, 8 + kick),
            (-9, -10 + kick),
        ],
        VOID,
    )
    poly(
        [
            (-5, -16 + kick),
            (5, -16 + kick),
            (6, -9 + kick),
            (5, 5 + kick),
            (-5, 5 + kick),
            (-6, -9 + kick),
        ],
        DARK,
    )
    line([(-4, -14 + kick), (-4, 3 + kick)], EDGE, 1)
    line([(-3, -10 + kick), (3, -10 + kick)], p, 3)
    for bx in (-6, 6):
        line([(bx, -6 + kick), (bx, 3)], STEEL, 1)


# Drones.


def drone_frame(d: Draw, cx, cy, t, p) -> None:
    """A drone's arms and rotors, `t` ticks into the rotors' turn."""
    for dx, dy in ((-6, -6), (6, -6), (-6, 6), (6, 6)):
        d.beam((cx, cy), (cx + dx, cy + dy), 1.8, IRON)
        fan(d, cx + dx, cy + dy, 4.2, t, p)


def drone_body(d: Draw, cx, cy, p, firing=False) -> None:
    polygon(
        d,
        [
            (cx, cy - 6),
            (cx + 2.5, cy - 3),
            (cx + 2.5, cy + 6),
            (cx, cy + 8),
            (cx - 2.5, cy + 6),
            (cx - 2.5, cy - 3),
        ],
        DARK,
    )
    d.box((cx - 1, cy + 2, cx + 1, cy + 5), p, 0.4)
    d.line([(cx, cy - 3), (cx, cy - 9)], STEEL, 1)
    if firing:
        d.line([(cx, cy - 10), (cx, cy - 14)], DRONE_FLASH, 1.6)


def mini_drone(t, p, firing=False) -> Image.Image:
    """The study's whole drone, supersampled and cropped as the study pastes it."""
    d = Draw()
    drone_frame(d, 128, 128, t, p)
    drone_body(d, 128, 128, p, firing)
    return d.im.crop((116 * SS, 114 * SS, 140 * SS, 140 * SS))


# The study, composed in one pass.


def docked(rotor: int = 0) -> list[tuple[float, float, float, float, int, bool]]:
    """Six drones resting in their docks: (x, y, heading, lift, rotor tick,
    firing) each."""
    return [(x, y, 0.0, 0.0, rotor + i * 3, False) for i, (x, y) in enumerate(DOCKS)]


def _top(p, fan_tick: int, spreads, hatch: float | None = None) -> Draw:
    top = Draw()
    hull(top, p, fan_tick, spreads, hatch)
    rim(top)
    return top


def _guns(p, guns) -> Draw:
    d = Draw()
    for side, (bearing, age) in enumerate(guns):
        yoke(d, side)
        x, y = MOUNTS[side]
        gun(d, x, y, bearing, age, p)
    return d


def _wall(top: Image.Image) -> Image.Image:
    wall = top.copy()
    wall.putalpha(top.getchannel("A"))
    wall.paste((*DEEP, 255), (0, 0, wall.width, wall.height), wall.getchannel("A"))
    shifted = Image.new("RGBA", wall.size)
    shifted.alpha_composite(wall, (0, 5 * SS))
    return shifted


def render_study(
    variant: str = "base",
    fan_tick: int = 0,
    spreads=(0.0,) * 6,
    guns=((PARK[0], NO_SHOT), (PARK[1], NO_SHOT)),
    drones=None,
    shadows: bool = True,
    hatch: float | None = None,
) -> Image.Image:
    """The approved study's ship at rest, composed in one pass at
    supersampled scale, before centring. `drones` places each drone as
    (x, y, heading, lift, rotor tick, firing); by default all six are docked.
    `hatch` adds production's cargo hatch, that far open."""
    p = PAINT[variant]
    ground = Draw()
    top = _top(p, fan_tick, spreads, hatch).im
    topworld = transformed(top)
    gunworld = transformed(_guns(p, guns).im)
    if shadows:
        ground.im.alpha_composite(shadow(topworld, *HULL_SHADOW, 82, 2.1))
        ground.im.alpha_composite(shadow(gunworld, *GUN_SHADOW, 70, 1.3))
    ground.im.alpha_composite(transformed(_wall(top)))
    ground.im.alpha_composite(gunworld)
    ground.im.alpha_composite(topworld)
    for x, y, heading, lift, rotor, firing in docked() if drones is None else drones:
        im = mini_drone(rotor, p, firing).rotate(-heading, Image.Resampling.BICUBIC, expand=True)
        canvas = Image.new("RGBA", (SIZE * SS, SIZE * SS))
        canvas.alpha_composite(
            im, (round(x * SS - im.width / 2), round(y * SS - im.height / 2 - lift * SS))
        )
        if lift > 0.05 and shadows:
            ground.im.alpha_composite(shadow(canvas, *DRONE_SHADOW, 70, 1))
        ground.im.alpha_composite(canvas)
    return ground.finish()


def _centred(image: Image.Image) -> Image.Image:
    """The study's drawing moved toward the nose so the ship turns about
    the canvas centre."""
    out = Image.new("RGBA", image.size)
    out.paste(image.crop((0, CENTRE, image.width, image.height)), (0, 0))
    return out


def render_roc(variant: str) -> Image.Image:
    """The whole Roc for portraits: guns parked, drones docked, no ground
    shadow."""
    return _centred(render_study(variant, shadows=False, hatch=0.0))


# Runtime layers.


def _moving(frames: list[Image.Image]) -> Image.Image:
    """Pixels that differ between any frame and the first."""
    first = frames[0]
    mask = Image.new("L", first.size)
    for frame in frames[1:]:
        diff = ImageChops.difference(first, frame).convert("RGB")
        r, g, b = diff.split()
        changed = ImageChops.lighter(ImageChops.lighter(r, g), b).point(lambda v: 255 if v else 0)
        alpha = ImageChops.difference(first.getchannel("A"), frame.getchannel("A")).point(
            lambda v: 255 if v else 0
        )
        mask = ImageChops.lighter(mask, ImageChops.lighter(changed, alpha))
    return mask


def _keep(image: Image.Image, mask: Image.Image) -> Image.Image:
    out = image.copy()
    out.putalpha(ImageChops.multiply(image.getchannel("A"), mask))
    return out


def _without(image: Image.Image, mask: Image.Image) -> Image.Image:
    return _keep(image, ImageChops.invert(mask))


@functools.lru_cache(maxsize=None)
def _hull(variant: str, fan_tick: int = 0, spread: float = 0.0, hatch: float = 0.0) -> Image.Image:
    return _top(PAINT[variant], fan_tick, (spread,) * 6, hatch).im.resize(
        (SIZE, SIZE), Image.Resampling.LANCZOS
    )


def _opening(step: int) -> float:
    return step / (HATCH_STEPS - 1)


@functools.lru_cache(maxsize=None)
def _masks() -> tuple[Image.Image, Image.Image, Image.Image]:
    """The pixels the fan blades, the clamps and the hatch redraw."""
    fans = _moving([_hull("base", k) for k in range(FAN_FRAMES)])
    clamps = _moving([_hull("base", 0, float(s)) for s in range(CLAMP_STEPS)])
    hatch = _moving([_hull("base", 0, 0.0, _opening(k)) for k in range(HATCH_STEPS)])
    return fans, clamps, hatch


def _crop(image: Image.Image, centre, size: int) -> Image.Image:
    x, y = centre
    return image.crop((x - size // 2, y - size // 2, x + size // 2, y + size // 2))


def render_hull(variant: str) -> Image.Image:
    """The hull with the fan blades, clamps and hatch cut out."""
    fans, clamps, hatch = _masks()
    holes = ImageChops.lighter(ImageChops.lighter(fans, clamps), hatch)
    return _centred(_without(_hull(variant), holes))


def render_wreck(variant: str) -> Image.Image:
    """The hull as it falls: fans still, docks empty with their clamps open.
    The shell draws the wall, yokes and guns beneath it at their last
    bearings."""
    return _centred(_hull(variant, 0, float(CLAMP_STEPS - 1)))


def render_fan(side: int, variant: str, frame: int) -> Image.Image:
    """One fan's blades at a phase of their five-fold turn, centred on the
    fan. Each fan and dock keeps its own layer: the blades' and clamps'
    edges blend with whatever lies beneath them."""
    return _crop(_keep(_hull(variant, frame), _masks()[0]), FANS[side], FAN)


def render_clamps(dock: int, variant: str, step: int) -> Image.Image:
    """One dock's clamps `step` pixels open, centred on the dock."""
    return _crop(_keep(_hull(variant, 0, float(step)), _masks()[1]), DOCKS[dock], CLAMP)


def render_hatch(variant: str, step: int) -> Image.Image:
    """The cargo hatch `step` of `HATCH_STEPS - 1` open, centred on it."""
    x, y = HATCH_CENTRE
    w, h = HATCH
    image = _keep(_hull(variant, 0, 0.0, _opening(step)), _masks()[2])
    return image.crop((x - w // 2, y - h // 2, x + w // 2, y + h // 2))


def render_ghost(variant: str) -> Image.Image:
    """The Roc as its cradle assembles it, drawn at the building opacity
    steps: guns at rest, fans still, docks empty and open, no shadow."""
    rest = ((-45.0, NO_SHOT), (45.0, NO_SHOT))
    return _centred(
        render_study(variant, 0, (float(CLAMP_STEPS - 1),) * 6, rest, (), shadows=False, hatch=0.0)
    )


def render_glow() -> Image.Image:
    """The aft thrusters at full heat, drawn over the hull at the engines'
    heat: the outlets glow, the vent slots warm behind them and heat creeps
    up the rear of each housing."""
    d = Draw()
    d.d = ImageDraw.Draw(d.im, "RGBA")
    for x in THRUSTERS:
        # Inside the housing's plate, clear of its cut corners.
        for k in range(6):
            y0 = 190 + k * 4
            d.box((x - 12.5, y0, x + 12.5, y0 + 4), (*KILN, 12 + k * 16))
        d.line([(x - 8.5, 213), (x + 8.5, 213)], (*HOT[0], 170), 1)
        d.box((x - 6.5, 214.5, x + 6.5, 218.5), KILN, 1)
        d.box((x - 5, 215.5, x + 5, 217.5), HOT[1], 0.6)
        d.line([(x - 3.5, 216.5), (x + 3.5, 216.5)], HOT[2], 0.8)
        d.box((x - 6, 203.6, x + 6, 208.4), (*KILN, 150), 1)
    return _centred(d.finish())


# The cradle: the landing pad, drawn north-facing on the hull's canvas between
# the shared crane's runways, cropped to the 3x3 footprint and turned to face
# east as the Maw's is. The shell fades the Roc in over it from the Roc's own
# layers.


def _mast(d: Draw, x, y, p) -> None:
    d.plate((x - 7, y - 7, x + 7, y + 7), IRON, 2)
    d.disk(x, y, 4, VOID)
    d.disk(x, y, 2.6, AMBER)
    for dx, dy in ((-5, 0), (5, 0), (0, -5), (0, 5)):
        d.line([(x + dx * 0.6, y + dy * 0.6), (x + dx, y + dy)], STEEL, 0.8)
    d.box((x - 4, y + 8, x + 4, y + 11), p, 1)


def _pad(d: Draw, p) -> None:
    """A ringed landing circle with hazard chevrons, floodlight masts at the
    corners, and fuel and power lines run in from the tail."""
    d.plate((32, 32, 224, 224), DEEP, 8)
    for x in (44, 212):
        for y in (44, 212):
            d.bolt(x, y, 2)
    d.disk(128, 128, 84, VOID)
    d.disk(128, 128, 82, DARK)
    d.disk(128, 128, 70, DEEP)
    d.disk(128, 128, 58, DARK)
    for k in range(16):
        a = k * math.tau / 16
        x0, y0 = 128 + 72 * math.cos(a), 128 + 72 * math.sin(a)
        x1, y1 = 128 + 80 * math.cos(a + 0.12), 128 + 80 * math.sin(a + 0.12)
        d.line([(x0, y0), (x1, y1)], BRASS if k % 2 else DEEP, 3)
    d.line([(98, 128), (158, 128)], EDGE, 1)
    d.line([(128, 98), (128, 158)], EDGE, 1)
    for x in (106, 150):
        d.hose([(x, 224), (x, 210), (x + (14 if x < 128 else -14), 196)])
        d.plate((x - 5, 214, x + 5, 224), IRON, 2)
    maw._runways(d)
    for x, y in ((50, 50), (206, 50), (50, 206), (206, 206)):
        _mast(d, x, y, p)


def _cradle(variant: str, crane: bool) -> Image.Image:
    p = PAINT[variant]
    d = Draw()
    _pad(d, p)
    if crane:
        maw._bridge(d, p)
        maw._trolley(d, p)
    return maw._crop(d.finish())


def render_cradle(variant: str) -> Image.Image:
    """The cradle with its crane parked, for icons and placement."""
    return _cradle(variant, True)


def render_cradle_site(variant: str) -> Image.Image:
    """The bed under assembly: the crane and the Roc draw over it at run time."""
    return _cradle(variant, False)


def render_wall() -> Image.Image:
    shifted = _wall(_top(PAINT["base"], 0, (0.0,) * 6).im)
    return _centred(shifted.resize((SIZE, SIZE), Image.Resampling.LANCZOS))


def render_shadow() -> Image.Image:
    """The hull's ground shadow with the yokes' beside it, unshifted: the
    shell offsets it by `HULL_SHADOW` in screen space."""
    top = _top(PAINT["base"], 0, (0.0,) * 6).im
    yokes = Draw()
    for side in (0, 1):
        yoke(yokes, side)
    out = shadow(top, 0, 0, 82, 2.1)
    beside = Image.new("RGBA", out.size)
    offset = tuple((g - h) * SS for g, h in zip(GUN_SHADOW, HULL_SHADOW))
    beside.paste(shadow(yokes.im, 0, 0, 70, 1.3), offset)
    out.alpha_composite(beside)
    return _centred(out.resize((SIZE, SIZE), Image.Resampling.LANCZOS))


def render_yokes() -> Image.Image:
    d = Draw()
    for side in (0, 1):
        yoke(d, side)
    return _centred(d.finish())


def _gun_canvas(variant: str, age: float) -> Draw:
    d = Draw(GUN)
    gun(d, GUN // 2, GUN // 2, 0.0, age, PAINT[variant])
    return d


def render_gun(variant: str, age: float = NO_SHOT) -> Image.Image:
    """A gun pointing ahead about its canvas centre, `age` ticks after a shot."""
    return _gun_canvas(variant, age).finish()


def render_gun_shadow() -> Image.Image:
    return shadow(_gun_canvas("base", NO_SHOT).im, 0, 0, 70, 1.3).resize(
        (GUN, GUN), Image.Resampling.LANCZOS
    )


def render_drone(variant: str, rotor: int = 0) -> Image.Image:
    d = Draw(DRONE)
    drone_frame(d, DRONE // 2, DRONE // 2, rotor, PAINT[variant])
    drone_body(d, DRONE // 2, DRONE // 2, PAINT[variant])
    return d.finish()


def render_drone_frame(variant: str, rotor: int) -> Image.Image:
    d = Draw(DRONE)
    drone_frame(d, DRONE // 2, DRONE // 2, rotor, PAINT[variant])
    return d.finish()


def render_drone_body(variant: str, firing: bool = False) -> Image.Image:
    d = Draw(DRONE)
    drone_body(d, DRONE // 2, DRONE // 2, PAINT[variant], firing)
    return d.finish()


def render_drone_shadow() -> Image.Image:
    d = Draw(DRONE)
    drone_frame(d, DRONE // 2, DRONE // 2, 0, PAINT["base"])
    drone_body(d, DRONE // 2, DRONE // 2, PAINT["base"])
    return shadow(d.im, 0, 0, 70, 1).resize((DRONE, DRONE), Image.Resampling.LANCZOS)


def _save(registry: Registry, out: Path, name: str, image: Image.Image) -> None:
    gen.save_sprite(image, out, name)
    registry[name] = image


def install_roc(registry: Registry, out: Path) -> None:
    """Installs the Roc's and the drone's unit sprites and runtime layers.

    Painted layers ship an allegiance-accent row; shadows, the wall and the
    yokes never do."""
    for variant in gen.PALETTES:
        tag = gen.variant_tag(variant)
        roc = render_roc(variant)
        for suffix in ("", "_move1", "_move2"):
            _save(registry, out, f"roc{tag}{suffix}", roc)
        hull = render_hull(variant)
        # The fans turn on their own layers, so the hull's motion rows
        # repeat its rest frame.
        for suffix in ("", "_move1", "_move2"):
            _save(registry, out, f"rig_roc_hull{tag}{suffix}", hull)
        _save(registry, out, f"rig_roc_wreck{tag}", render_wreck(variant))
        for step in range(HATCH_STEPS):
            _save(registry, out, f"rig_roc_hatch{tag}_{step + 1}", render_hatch(variant, step))
        _save(registry, out, f"rig_roc_ghost{tag}", render_ghost(variant))
        _save(registry, out, f"roc_cradle{tag}", render_cradle(variant))
        site = render_cradle_site(variant)
        for stage in range(CRADLE_STAGES):
            for phase in range(2):
                _save(registry, out, f"roc_cradle{tag}_site{stage}_{phase}", site)
        _save(registry, out, f"rig_roc_mount{tag}", render_gun(variant))
        for frame, age in enumerate(RECOIL_AGES):
            _save(registry, out, f"rig_roc_mount{tag}_action{frame + 1}", render_gun(variant, age))
        for move, suffix in enumerate(("", "_move1", "_move2")):
            _save(registry, out, f"drone{tag}{suffix}", render_drone(variant, move * ROTOR_STEP))
            _save(
                registry,
                out,
                f"rig_drone_hull{tag}{suffix}",
                render_drone_frame(variant, move * ROTOR_STEP),
            )
        _save(registry, out, f"rig_drone_mount{tag}", render_drone_body(variant))
        _save(registry, out, f"rig_drone_mount{tag}_action1", render_drone_body(variant, True))
        for side, name in enumerate(("left", "right")):
            for frame in range(FAN_FRAMES):
                _save(
                    registry,
                    out,
                    f"rig_roc_fan_{name}{tag}_{frame + 1}",
                    render_fan(side, variant, frame),
                )
        for dock in range(len(DOCKS)):
            for step in range(CLAMP_STEPS):
                _save(
                    registry,
                    out,
                    f"rig_roc_dock{dock + 1}{tag}_{step + 1}",
                    render_clamps(dock, variant, step),
                )
    _save(registry, out, "rig_roc_wall", render_wall())
    _save(registry, out, "rig_roc_glow", render_glow())
    _save(registry, out, "rig_roc_shadow", render_shadow())
    _save(registry, out, "rig_roc_yoke", render_yokes())
    _save(registry, out, "rig_roc_mount_shadow", render_gun_shadow())
    _save(registry, out, "rig_drone_shadow", render_drone_shadow())
