"""The Maw and its assembly cradle.

The Maw is the approved Kilnjaw: treads on six bogies, a low armored deck, a
horizontal cutter drum under a hood, a conveyor to a kiln. Its drawing code
is ported from the approved study, with the study's clock replaced by the
state the shell supplies: belt travel per side, drum deploy and spin,
conveyor travel and kiln heat. The shell draws it from runtime layers that
share one 256px canvas (four tiles), north-facing, pivoting about the canvas
centre:

- tread cleats, left and right, at one-pixel phases of their 7px pitch;
- the hull, with the moving cleats and conveyor slats cut out;
- conveyor slats at one-pixel phases;
- scrap clumps that ride the belt, heating toward the kiln;
- the kiln, and its glow drawn over it at the kiln's heat;
- the drum's arms at deploy steps;
- the drum and hood at spin steps over half a turn, fully lowered; the
  shell slides them back by `DRUM_STOW` canvas pixels as they stow.

Every layer is the study's drawing moved `CENTRE` pixels toward the nose, so
the hull is centred on the unit and the lowered drum's front edge, body and end
plates alike, meets the hull's collision edge, where the cutter reaches.

The cradle is a 3x3 bed between raised runways. Its site frames fade the
stowed Maw in through the building opacity steps; a bridge crane and its
trolley are separate layers the shell places from assembly progress. A
finished Maw stands up facing east, the heading a new unit takes, so the
whole cradle is drawn north-facing and turned a quarter turn clockwise. Its
crew works from four pads off its north and south faces, each with a lamp on
the runway beside it that lights while that place is worked.
"""

from __future__ import annotations

import functools
import math
import random
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw

from tools import gen_sprites as gen

Registry = dict[str, Image.Image]

SS = 3
SIZE = 256
CRADLE = 192
CRADLE_INSET = (SIZE - CRADLE) // 2

VOID = (12, 13, 17)
DEEP = (25, 27, 32)
DARK = (48, 47, 53)
IRON = (72, 71, 77)
EDGE = (108, 106, 111)
STEEL = (152, 146, 139)
LIGHT = (199, 187, 166)
RUST = (111, 72, 49)
BRASS = (137, 111, 64)
PAINT = {"base": (154, 73, 48), "probe": (40, 139, 158)}

SCRAP = gen.SCRAP
SCRAP_DARK = gen.SCRAP_DARK
SCRAP_LIGHT = gen.SCRAP_LIGHT
BONE = gen.BONE
WRECK = ((58, 58, 68), (44, 44, 52), (86, 64, 40), (72, 72, 84))
HOT = ((214, 128, 52), (236, 158, 70), (250, 214, 150))
KILN_HOT = (196, 112, 48)

TREAD_PITCH = 7
SLAT_PITCH = 7
DRUM_FRAMES = 18
DRUM_PERIOD = math.pi
DRUM_STOW = 8
CENTRE = 10
ARM_STEPS = 4
BELT = (113, 99, 143, 181)
HOOD_EXIT = 99.0
KILN_IN = 176.0
SCRAP_VARIANTS = 6
HEAT_STEPS = 4
CLUMP = 24
HULL_OPACITY = (96, 160, 224)
BOGIES = [(38, y, 63, y + 39) for y in (85, 130, 175)] + [
    (193, y, 218, y + 39) for y in (85, 130, 175)
]


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
        self.d.rounded_rectangle(
            tuple(round(v * SS) for v in b), radius=round(r * SS), fill=c
        )

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
        self.line([(a[0] + nx * 0.65, a[1] + ny * 0.65), (b[0] + nx * 0.65, b[1] + ny * 0.65)], EDGE, 1)

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

    def armor(self, b, p):
        x, y, xx, yy = b
        self.plate(b, IRON, 4)
        self.plate((x + 3, y + 3, xx - 3, yy - 3), DARK, 2)
        self.box((x + 5, y + 5, xx - 5, y + 10), p, 1)
        self.line([(x + 4, y + 12), (x + 4, yy - 6)], EDGE, 1)
        self.line([(x + 5, yy - 5), (xx - 5, yy - 5)], DEEP, 1.5)
        self.line([(x + 7, y + 6), (x + 10, y + 6)], RUST, 1)
        for px, py in [(x + 5, yy - 5), (xx - 5, yy - 5)]:
            self.bolt(px, py, 1)

    def finish(self) -> Image.Image:
        size = self.im.width // SS
        return self.im.resize((size, size), Image.Resampling.LANCZOS)


def mix(a, b, u):
    return a + (b - a) * u


def point(a, b, u):
    return (mix(a[0], b[0], u), mix(a[1], b[1], u))


def belt(d: Draw, b, phase: float, p) -> None:
    x, y, xx, yy = b
    layer = Draw()
    layer.box(b, VOID, 7)
    layer.box((x + 2, y + 2, xx - 2, yy - 2), DEEP, 5)
    # Clip wrapped cleats to the belt. No skipped cleat at the wrap boundary.
    cleats = Draw()
    for k in range(-2, math.ceil((yy - y) / 7) + 2):
        z = y + k * 7 + phase % 7
        cleats.box((x + 2, z, xx - 2, z + 4.3), IRON, 1)
        cleats.line([(x + 3, z + 0.7), (xx - 3, z + 0.7)], EDGE, 0.8)
        for px in (x + 5, xx - 5):
            cleats.box((px - 1, z + 1, px + 1, z + 3), DEEP)
    m = Image.new("L", layer.im.size)
    ImageDraw.Draw(m).rounded_rectangle(
        tuple(round(v * SS) for v in (x + 2, y + 2, xx - 2, yy - 2)), radius=5 * SS, fill=255
    )
    cleats.im.putalpha(ImageChops.multiply(cleats.im.getchannel("A"), m))
    layer.im.alpha_composite(cleats.im)
    layer.line([(x + 1, y + 8), (x + 1, yy - 8)], STEEL, 0.9)
    layer.line([(xx - 1, y + 8), (xx - 1, yy - 8)], DEEP, 1)
    # Fixed sprocket guards and load-bearing hubs remain stationary over the belt.
    for z in (y + 8, yy - 8):
        layer.plate((x + 6, z - 5, xx - 6, z + 5), DARK, 2)
        layer.bolt((x + xx) / 2, z, 2)
    layer.box((x + 7, y + 18, xx - 7, yy - 18), DEEP, 2)
    if yy - y > 43:
        layer.box((x + 8, y + 20, xx - 8, y + 26), p, 1)
    d.im.alpha_composite(layer.im)


def engine(d: Draw, b, p, hot=False) -> None:
    x, y, xx, yy = b
    d.plate(b, DARK, 6)
    d.plate((x + 4, y + 4, xx - 4, yy - 4), IRON, 3)
    d.box((x + 6, y + 6, xx - 6, y + 11), p, 1)
    d.vent((x + 7, y + 15, xx - 7, yy - 7))
    for px in (x + 5, xx - 5):
        for py in (y + 5, yy - 5):
            d.bolt(px, py, 1)
    if hot:
        d.box((x + 9, y - 4, xx - 9, y + 2), VOID, 1)
        d.box((x + 11, y - 2, xx - 11, y), (191, 111, 48))
        for px in range(round(x + 13), round(xx - 10), 5):
            d.line([(px, y - 3), (px, y + 1)], DARK, 2)


def chassis(d: Draw, tread: tuple[float, float], p) -> None:
    """The six-bogie hull; `tread` is each side's belt phase in canvas px,
    falling as that side rolls forward."""
    for b in BOGIES:
        side = 0 if b[0] < 128 else 1
        cx = (b[0] + b[2]) / 2
        d.beam((128, (b[1] + b[3]) / 2), (cx, (b[1] + b[3]) / 2), 14, IRON)
        belt(d, b, tread[side], p)
    d.plate((65, 77, 191, 215), DARK, 11)
    for x in (71, 157):
        d.plate((x, 108, x + 28, 179), IRON, 5)
        d.box((x + 4, 113, x + 24, 119), p, 1)
        d.vent((x + 5, 138, x + 23, 173))
    engine(d, (86, 180, 170, 219), p, True)
    d.hose([(96, 185), (80, 183), (75, 162)])
    d.hose([(162, 185), (177, 183), (181, 162)])
    # Pale chassis edge only, away from the mechanism.
    for x in (68, 188):
        d.line([(x, 190), (x, 204)], LIGHT, 0.7)
    # Split armored skirts bridge the bogies without hiding their moving belts.
    for side in (-1, 1):
        x = 65 if side < 0 else 180
        for yy in (112, 163):
            d.armor((x, yy, x + 12, yy + 34), p)
        d.hose([(x + 5, 153), (x + 2, 147), (x + 2, 135)])
        d.line([(x + 5, 207), (x + 5, 214)], IRON, 4)
    for x in (99, 152):
        d.box((x - 4, 123, x + 4, 167), VOID, 3)
        d.box((x - 2, 127, x + 2, 163), IRON, 2)
        for yy in (131, 159):
            d.line([(x - 3, yy), (x + 3, yy)], STEEL, 1)
    d.plate((104, 161, 152, 177), IRON, 3)
    d.vent((108, 164, 148, 174))


def conveyor(d: Draw, shift: float, cargo=()) -> None:
    """The belt from the hood to the kiln; `cargo` draws onto the belt's
    clipped layer."""
    x, y, xx, yy = BELT
    d.box((x - 4, y - 3, xx + 4, yy + 3), VOID, 3)
    d.box((x, y, xx, yy), DEEP)
    for i in range(-2, 15):
        z = y + i * 7 + shift % 7
        if y <= z <= yy:
            d.line([(x + 1, z), (xx - 1, z)], IRON, 1)
    layer = Draw()
    for draw in cargo:
        draw(layer)
    mask = Image.new("L", layer.im.size)
    ImageDraw.Draw(mask).rectangle((x * SS, y * SS, xx * SS, yy * SS), fill=255)
    layer.im.putalpha(ImageChops.multiply(layer.im.getchannel("A"), mask))
    d.im.alpha_composite(layer.im)
    for side in (x - 3, xx + 3):
        d.line([(side, y), (side, yy)], EDGE, 1.5)
        for z in (y + 4, yy - 4):
            d.bolt(side, z, 1)


def kiln(d: Draw, heat: float) -> None:
    c = tuple(round(mix(a, b, heat)) for a, b in zip(DEEP, KILN_HOT))
    d.box((96, 175, 160, 183), VOID, 2)
    d.box((99, 177, 157, 181), c)
    for x in range(103, 156, 7):
        d.line([(x, 175), (x, 183)], DARK, 2)
    for x in (105, 151):
        d.box((x - 3, 189, x + 3, 196), VOID, 1)
        d.line([(x - 1, 191), (x + 1, 191)], c, 1)


def drum_row(deploy: float) -> float:
    return 65 + DRUM_STOW * (1 - deploy)


def hood_row(deploy: float) -> float:
    """The hood and side guards sit two rows behind the cutter, where the
    shell's hood and belt constants expect them."""
    return 67 + DRUM_STOW * (1 - deploy)


def arms(d: Draw, deploy: float) -> None:
    cy = drum_row(deploy)
    for x in (76, 180):
        d.beam((x, 96), (x, cy), 15, IRON)
        d.hydraulic((x, 112), (x, cy + 11), 7)


def drum(d: Draw, spin: float, p, deploy: float = 1.0) -> None:
    cy = drum_row(deploy)
    hood = hood_row(deploy)
    spin %= math.tau
    d.box((77, cy - 17, 179, cy + 18), VOID, 5)
    for z in range(-14, 15):
        shade = math.sqrt(max(0, 1 - (z / 16) ** 2))
        c = tuple(round(DEEP[i] + (IRON[i] - DEEP[i]) * shade) for i in range(3))
        d.line([(81, cy + z), (175, cy + z)], c, 1.2)
    # Helical cutting rows; hidden teeth pass around the rear of the cylinder.
    for col in range(10):
        x = 83 + col * 10
        for row in range(6):
            a = spin + row * math.tau / 6 + col * 0.24
            visible = math.cos(a)
            if visible <= 0:
                continue
            z = cy + math.sin(a) * 14
            h = 2 + visible * 3
            d.poly([(x - 3, z + 2), (x - 2, z - h), (x + 2, z - h - 1), (x + 4, z + 1)], DEEP)
            d.poly([(x - 1, z + 1), (x, z - h + 1), (x + 2, z - h), (x + 2, z)], STEEL)
            d.line([(x, z - h + 1), (x + 2, z - h)], LIGHT, 0.7)
    for x in (76, 180):
        d.plate((x - 9, cy - 17, x + 9, cy + 20), IRON, 4)
        d.box((x - 5, cy - 14, x + 5, cy - 8), p, 1)
        d.disk(x, cy, 6, VOID)
        d.disk(x, cy, 4, DARK)
        for a in (spin, spin + math.pi):
            d.line([(x, cy), (x + 3 * math.cos(a), cy + 3 * math.sin(a))], STEEL, 1)
        d.bolt(x, cy, 1)
        for z in (-15, 16):
            d.bolt(x, cy + z, 1)
    # The hood hides the material's route from the cutter to the conveyor.
    d.plate((83, hood + 12, 173, hood + 34), DARK, 6)
    d.box((94, hood + 16, 162, hood + 21), p, 1)
    for x in (94, 109, 147, 162):
        d.bolt(x, hood + 26, 1)
    d.line([(96, hood + 31), (160, hood + 31)], VOID, 3)
    for side in (-1, 1):
        x = 128 + side * 57
        d.poly(
            [(x, hood + 18), (x + side * 13, hood - 9), (x + side * 19, hood - 6), (x + side * 13, hood + 29)],
            VOID,
        )
        d.line([(x + side * 4, hood + 18), (x + side * 14, hood - 5)], STEEL, 4)


def _centred(image: Image.Image) -> Image.Image:
    """The study's drawing moved toward the nose so the hull is centred."""
    out = Image.new("RGBA", image.size)
    out.paste(image.crop((0, CENTRE, image.width, image.height)), (0, 0))
    return out


def render_maw(
    variant: str = "base",
    tread: tuple[float, float] = (0.0, 0.0),
    shift: float = 0.0,
    heat: float = 0.0,
    deploy: float = 0.0,
    spin: float = 0.0,
    cargo=(),
) -> Image.Image:
    """The complete machine as the game draws it."""
    return _centred(render_study(variant, tread, shift, heat, deploy, spin, cargo))


def render_study(
    variant: str = "base",
    tread: tuple[float, float] = (0.0, 0.0),
    shift: float = 0.0,
    heat: float = 0.0,
    deploy: float = 0.0,
    spin: float = 0.0,
    cargo=(),
) -> Image.Image:
    """The approved study's machine, composed in one pass at supersampled
    scale, before centring."""
    p = PAINT[variant]
    d = Draw()
    chassis(d, tread, p)
    conveyor(d, shift, cargo)
    kiln(d, heat)
    arms(d, deploy)
    drum(d, spin, p, deploy)
    return d.finish()


def _alone(draw) -> Image.Image:
    d = Draw()
    draw(d)
    return d.finish()


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


def _base(variant: str, tread=(0.0, 0.0), shift: float = 0.0) -> Image.Image:
    p = PAINT[variant]

    def draw(d):
        chassis(d, tread, p)
        conveyor(d, shift)

    return _alone(draw)


@functools.lru_cache(maxsize=None)
def _tread_frames(side: int) -> tuple[Image.Image, ...]:
    def phase(k):
        tread = [0.0, 0.0]
        tread[side] = float(k)
        return tuple(tread)

    return tuple(_base("base", phase(k)) for k in range(TREAD_PITCH))


@functools.lru_cache(maxsize=None)
def _slat_frames() -> tuple[Image.Image, ...]:
    return tuple(_base("base", shift=float(k)) for k in range(SLAT_PITCH))


@functools.lru_cache(maxsize=None)
def _masks() -> tuple[Image.Image, Image.Image, Image.Image]:
    return (_moving(list(_tread_frames(0))), _moving(list(_tread_frames(1))), _moving(list(_slat_frames())))


def render_tread(side: int, phase: int) -> Image.Image:
    """One side's moving cleats at a phase of their pitch; untinted."""
    return _centred(_keep(_tread_frames(side)[phase], _masks()[side]))


def render_slats(phase: int) -> Image.Image:
    return _centred(_keep(_slat_frames()[phase], _masks()[2]))


def render_hull(variant: str) -> Image.Image:
    """Everything the moving layers do not redraw, with their pixels cut out."""
    left, right, slats = _masks()
    holes = ImageChops.lighter(ImageChops.lighter(left, right), slats)
    return _centred(_without(_base(variant), holes))


def _kiln_cold() -> Image.Image:
    return _alone(lambda d: kiln(d, 0.0))


def render_kiln() -> Image.Image:
    return _centred(_kiln_cold())


def render_kiln_glow() -> Image.Image:
    """The kiln at full heat, only where heat changes it; drawn over the
    cold kiln at the kiln's heat as opacity."""
    hot = _alone(lambda d: kiln(d, 1.0))
    return _centred(_keep(hot, _moving([_kiln_cold(), hot])))


def render_arms(step: int) -> Image.Image:
    return _centred(_alone(lambda d: arms(d, step / ARM_STEPS)))


def render_drum(variant: str, frame: int) -> Image.Image:
    """The drum and hood fully lowered at a spin step of half a turn."""
    p = PAINT[variant]
    return _centred(_alone(lambda d: drum(d, frame * DRUM_PERIOD / DRUM_FRAMES, p)))


# Belt cargo: scrap in the scrap node's own shard language, heating toward
# the kiln. Each clump is centred on its own small canvas.
def shard(d: Draw, cx, cy, w, h, dx, tone) -> None:
    d.poly(
        [
            (cx - w / 2 - dx, cy - h / 2),
            (cx + w / 2, cy - h / 2 + dx / 2),
            (cx + w / 2 + dx, cy + h / 2),
            (cx - w / 2, cy + h / 2 - dx / 2),
        ],
        tone,
    )


def heated(base, heat: float):
    target = HOT[min(2, int(heat * 3))]
    return tuple(round(p + (q - p) * heat) for p, q in zip(base, target))


def clump(d: Draw, x, y, rng: random.Random, heat: float, pieces: int, spread: float = 4.0) -> None:
    d.disk(x, y + 1.5, spread + 0.5, (24, 20, 16))
    placed = []
    for _ in range(pieces):
        a = rng.uniform(0, math.tau)
        r = spread * rng.random() ** 0.6
        placed.append((r, x + r * math.cos(a), y + r * math.sin(a) * 0.7))
    placed.sort(key=lambda q: -q[0])
    for r, cx, cy in placed:
        w = rng.uniform(4, 7) * (1.15 - 0.4 * r / spread)
        h = rng.uniform(3, 5) * (1.15 - 0.4 * r / spread)
        crown = r < spread * 0.45
        base = rng.choice((SCRAP, SCRAP_LIGHT)) if crown else rng.choice((SCRAP, SCRAP_DARK))
        shard(d, cx, cy, w, h, rng.uniform(-1.5, 1.5), heated(base, heat))
    if rng.random() < 0.6:
        d.disk(x + rng.uniform(-1.5, 1.5), y - 1 + rng.uniform(-1, 1), 0.9, BONE)


def render_scrap(variant: int, step: int) -> Image.Image:
    rng = random.Random(7240 + variant)
    pieces = rng.randint(3, 5)
    seed = rng.random()
    d = Draw(CLUMP)
    clump(d, CLUMP / 2, CLUMP / 2, random.Random(seed), step / (HEAT_STEPS - 1), pieces)
    return d.finish()


# The cradle: a bed between raised runways, drawn on the hull's canvas and
# cropped to the 3x3 footprint.
RUNWAYS = (38, 218)
POSTS = (40, 128, 216)
BRIDGE_ROW = 128
TROLLEY = (128, 128)


def _bed(d: Draw) -> None:
    d.plate((32, 32, 224, 224), DEEP, 8)
    for x in (44, 212):
        for y in (44, 212):
            d.bolt(x, y, 2)
    for x in RUNWAYS:
        for y in POSTS:
            d.box((x - 5, y - 5, x + 5, y + 5), VOID, 1)
            d.box((x - 3, y - 3, x + 3, y + 3), IRON, 1)


def _runways(d: Draw) -> None:
    for x in RUNWAYS:
        d.beam((x, 33), (x, 223), 9, DARK)
        for y in range(36, 222, 12):
            d.line([(x - 2, y), (x + 2, y)], EDGE, 0.8)


def _bridge(d: Draw, p, y: float = BRIDGE_ROW) -> None:
    for x in RUNWAYS:
        d.plate((x - 6, y - 12, x + 6, y + 12), IRON, 3)
        d.box((x - 3.5, y - 9, x + 3.5, y - 5), p, 1)
        for yy in (y - 7, y + 7):
            d.disk(x, yy, 2.2, VOID)
    d.box((44, y - 6, 212, y + 6), VOID, 2)
    d.box((46, y - 4, 210, y + 4), DARK)
    for x in range(50, 206, 12):
        d.line([(x, y - 4), (x + 6, y + 4)], IRON, 1.2)
        d.line([(x + 6, y - 4), (x + 12, y + 4)], IRON, 1.2)
    d.line([(46, y - 4), (210, y - 4)], EDGE, 1)


def _trolley(d: Draw, p, x: float = TROLLEY[0], y: float = TROLLEY[1]) -> None:
    d.plate((x - 10, y - 9, x + 10, y + 9), IRON, 3)
    d.box((x - 6, y - 6, x + 6, y - 3), p, 1)
    d.disk(x, y + 3, 2.5, VOID)
    d.disk(x, y + 3, 1.2, STEEL)


def _weld(d: Draw, x: float = TROLLEY[0], y: float = TROLLEY[1] + 14) -> None:
    rng = random.Random(736)
    for _ in range(5):
        a = rng.uniform(0, math.tau)
        r = rng.uniform(2, 6)
        d.line([(x, y), (x + math.cos(a) * r, y + math.sin(a) * r)], SCRAP_LIGHT, 0.8)
    d.disk(x, y, 1.4, (255, 236, 190))


def _crop(image: Image.Image) -> Image.Image:
    """The footprint, turned to face east."""
    footprint = image.crop(
        (CRADLE_INSET, CRADLE_INSET, CRADLE_INSET + CRADLE, CRADLE_INSET + CRADLE)
    )
    return footprint.transpose(Image.Transpose.ROTATE_270)


def render_cradle(variant: str) -> Image.Image:
    """The cradle with its crane parked, for icons and placement."""
    p = PAINT[variant]

    def draw(d):
        _bed(d)
        _runways(d)
        _bridge(d, p)
        _trolley(d, p)

    return _crop(_alone(draw))


def _faded(image: Image.Image, stage: int) -> Image.Image:
    opacity = HULL_OPACITY[stage]
    out = image.copy()
    out.putalpha(image.getchannel("A").point(lambda a: 0 if a == 0 else max(1, a * opacity // 255)))
    return out


def render_cradle_site(variant: str, stage: int) -> Image.Image:
    """The bed, the stowed Maw at a building opacity step, the runways."""
    out = _alone(_bed)
    out.alpha_composite(_faded(render_maw(variant), stage))
    out.alpha_composite(_alone(_runways))
    return _crop(out)


def render_bridge(variant: str) -> Image.Image:
    """The bridge on the footprint's middle column; the shell slides it."""
    p = PAINT[variant]
    return _crop(_alone(lambda d: _bridge(d, p)))


def render_trolley(variant: str, welding: bool) -> Image.Image:
    """The trolley at the footprint centre; the shell places it on the bridge."""
    p = PAINT[variant]

    def draw(d):
        _trolley(d, p)
        if welding:
            _weld(d)

    return _crop(_alone(draw))


PAD = 64
LAMP = 16


def render_pad() -> Image.Image:
    """A crew pad: a bolted deck plate with a worn hazard border, low enough
    to read as ground."""
    d = Draw(PAD)
    d.box((6, 6, 58, 58), VOID, 4)
    d.box((8, 8, 56, 56), DARK, 3)
    for k in range(0, 48, 8):
        for x0, y0, x1, y1 in (
            (8 + k, 8, 12 + k, 12),
            (8 + k, 52, 12 + k, 56),
            (8, 8 + k, 12, 12 + k),
            (52, 8 + k, 56, 12 + k),
        ):
            d.box((x0, y0, x1, y1), BRASS)
    d.box((14, 14, 50, 50), IRON, 2)
    for x, y in ((18, 18), (46, 18), (18, 46), (46, 46)):
        d.bolt(x, y, 1.4)
    d.line([(22, 32), (42, 32)], DEEP, 1.4)
    d.line([(32, 22), (32, 42)], DEEP, 1.4)
    image = d.finish()
    image.putalpha(image.getchannel("A").point(lambda a: a * 3 // 4))
    return image


def render_lamp(lit: bool) -> Image.Image:
    """A crew place's lamp, dark or lit."""
    d = Draw(LAMP)
    d.box((2, 2, 14, 14), VOID, 3)
    d.box((4, 4, 12, 12), IRON, 2)
    if lit:
        d.disk(8, 8, 3.4, SCRAP_LIGHT)
        d.disk(7.4, 7.4, 1.4, BONE)
    else:
        d.disk(8, 8, 3.4, DEEP)
    return d.finish()


def _save(registry: Registry, out: Path, name: str, image: Image.Image) -> None:
    gen.save_sprite(image, out, name)
    registry[name] = image


def install_maw(registry: Registry, out: Path) -> None:
    """Installs the Maw's unit sprite, its runtime layers and its cradle.

    Painted layers ship an allegiance-accent row; treads, slats, scrap,
    kiln, arms and the weld never do."""
    for variant in gen.PALETTES:
        tag = gen.variant_tag(variant)
        _save(registry, out, f"maw{tag}", render_maw(variant))
        for move in (1, 2):
            phase = float(-2 * move)
            _save(registry, out, f"maw{tag}_move{move}", render_maw(variant, (phase, phase)))
        _save(registry, out, f"rig_maw_hull{tag}", render_hull(variant))
        for frame in range(DRUM_FRAMES):
            _save(registry, out, f"rig_maw_drum{tag}_{frame + 1}", render_drum(variant, frame))
        _save(registry, out, f"maw_cradle{tag}", render_cradle(variant))
        for stage in range(len(HULL_OPACITY)):
            site = render_cradle_site(variant, stage)
            for phase in range(2):
                _save(registry, out, f"maw_cradle{tag}_site{stage}_{phase}", site)
        _save(registry, out, f"rig_cradle_bridge{tag}", render_bridge(variant))
        _save(registry, out, f"rig_cradle_trolley{tag}", render_trolley(variant, False))
        _save(registry, out, f"rig_cradle_trolley{tag}_1", render_trolley(variant, True))
    for side, name in enumerate(("left", "right")):
        for phase in range(TREAD_PITCH):
            _save(registry, out, f"rig_maw_tread_{name}_{phase + 1}", render_tread(side, phase))
    for phase in range(SLAT_PITCH):
        _save(registry, out, f"rig_maw_slats_{phase + 1}", render_slats(phase))
    _save(registry, out, "rig_cradle_pad", render_pad())
    _save(registry, out, "rig_cradle_lamp", render_lamp(False))
    _save(registry, out, "rig_cradle_lamp_1", render_lamp(True))
    _save(registry, out, "rig_maw_kiln", render_kiln())
    _save(registry, out, "rig_maw_kiln_glow", render_kiln_glow())
    for step in range(ARM_STEPS + 1):
        _save(registry, out, f"rig_maw_arms_{step + 1}", render_arms(step))
    for variant in range(SCRAP_VARIANTS):
        for step in range(HEAT_STEPS):
            _save(
                registry,
                out,
                f"rig_maw_scrap_{variant * HEAT_STEPS + step + 1}",
                render_scrap(variant, step),
            )
