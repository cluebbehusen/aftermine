"""Approved split-dish ground scout, driven by deployment and scanner state."""

import math
from pathlib import Path

from PIL import Image

from tools import gen_sprites as gen
from tools.production_sprite_sources.installed_defenses_final import bolt
from tools.production_sprite_sources.specialists_final import (
    DARK,
    EDGE,
    IRON,
    VOID,
    box,
    canvas,
    circle,
    finish,
    line,
    panel,
    poly,
    strut,
    track,
    vent,
)

RADAR_UNFOLD_FRAMES = 40
RADAR_SCAN_FRAMES = 80
PAINT = {"base": (196, 87, 59), "probe": (53, 157, 179)}
BRASS = (130, 105, 66)
LIGHT = (225, 205, 163)


def smooth(t):
    t = max(0, min(1, t))
    return t * t * (3 - 2 * t)


def disk(d, x, y, r, c):
    circle(d, (x - r, y - r, x + r, y + r), c)


def brace(d, a, b, w=5):
    strut(d, a, b, w)
    for x, y in (a, b):
        bolt(d, x, y)


def hull(d, pts, paint):
    panel(d, pts, DARK)
    cx = sum(x for x, y in pts) / len(pts)
    cy = sum(y for x, y in pts) / len(pts)
    inner = [(cx + (x - cx) * 0.73, cy + (y - cy) * 0.73) for x, y in pts]
    panel(d, inner, paint)


def _chassis(variant: str, move: int) -> Image.Image:
    paint = PAINT[variant]
    im, d = canvas()
    for bounds in ((24, 45, 40, 100), (88, 45, 104, 100), (55, 24, 73, 48)):
        track(d, bounds, move % 3, paint)
    hull(d, [(49, 30), (79, 30), (88, 92), (72, 103), (56, 103), (40, 92)], paint)
    vent(d, 54, 81, 20)
    for x in (47, 81):
        bolt(d, x, 73)
    disk(d, 64, 65, 12, VOID)
    disk(d, 64, 65, 9, IRON)
    brace(d, (64, 70), (64, 48), 7)
    return im


def _hinge(points, pivot, angle):
    if angle == 0:
        return points
    c, s = math.cos(angle), math.sin(angle)
    x, y = pivot
    return [
        (x + (px - x) * c - (py - y) * s, y + (px - x) * s + (py - y) * c)
        for px, py in points
    ]


def _reflector(deploy: float, sweep: int) -> Image.Image:
    rotor, rd = canvas()
    yy, radius, depth = 48, 30, 14
    for sign in (-1, 1):
        opening = smooth((deploy - (0.10 if sign == 1 else 0.0)) / 0.85)
        angle = sign * (1.0 - opening) * math.pi / 2
        pivot = (64 + sign * 2, yy)

        def leaf(points, pivot=pivot, angle=angle):
            return _hinge(points, pivot, angle)

        outer = [
            (64 + sign * radius * math.sin(a), yy + depth * math.cos(a))
            for a in [i * math.pi / 16 for i in range(17)]
        ]
        poly(rd, leaf(outer), VOID)
        inner = [
            (64 + sign * (radius - 3) * math.sin(a), yy + (depth - 3) * math.cos(a))
            for a in [i * math.pi / 16 for i in range(17)]
        ]
        poly(rd, leaf(inner), IRON)
        bowl = [
            (64 + sign * (radius - 8) * math.sin(a), yy + (depth - 7) * math.cos(a) + 2)
            for a in [i * math.pi / 16 for i in range(17)]
        ]
        poly(rd, leaf(bowl), DARK)
        line(rd, leaf(outer[1:9]), EDGE, 1.5)
        for a in (0.5, 1.05, 1.65, 2.25, 2.7):
            edge = (
                64 + sign * (radius - 4) * math.sin(a),
                yy + (depth - 4) * math.cos(a),
            )
            line(rd, leaf([(64 + sign * 2, yy + 3), edge]), EDGE, 1)
        hinge = 64 + sign * 2
        if angle == 0:
            box(rd, (hinge - 1, yy - depth + 4, hinge + 1, yy + depth - 4), IRON, 1)
        else:
            poly(
                rd,
                leaf(
                    [
                        (hinge - 1, yy - depth + 4),
                        (hinge + 1, yy - depth + 4),
                        (hinge + 1, yy + depth - 4),
                        (hinge - 1, yy + depth - 4),
                    ]
                ),
                IRON,
            )
    feed_angle = (1.0 - smooth((deploy - 0.65) / 0.35)) * math.pi * 0.85
    feed_start, feed_end = _hinge([(64, 57), (64, 44)], (64, 57), feed_angle)
    brace(rd, feed_start, feed_end, 2)
    disk(rd, *feed_end, 3, BRASS)
    disk(rd, *feed_end, 1, LIGHT)
    angle = (sweep % RADAR_SCAN_FRAMES) * math.tau / RADAR_SCAN_FRAMES
    return rotor.rotate(
        -math.degrees(angle), Image.Resampling.BICUBIC, center=(256, yy * 4)
    )


def _pack(image: Image.Image, rim: bool = True) -> Image.Image:
    small = finish(image, rim=rim).resize((72, 72), Image.Resampling.LANCZOS)
    out = Image.new("RGBA", (128, 128))
    out.alpha_composite(small, (28, 28))
    return out


def render_surveyor(
    variant: str, move: int = 0, deploy: float = 0.0, sweep: int = 0
) -> Image.Image:
    image = _chassis(variant, move)
    image.alpha_composite(_reflector(deploy, sweep))
    return _pack(image)


def install_surveyor(registry: dict[str, Image.Image], out: Path) -> None:
    """Install the folded drive loop, unfold, and four-second dish rotation."""
    for variant in gen.PALETTES:
        stem = f"surveyor{gen.variant_tag(variant)}"
        rig = "rig_surveyor"
        tag = gen.variant_tag(variant)
        frames = {
            stem: render_surveyor(variant),
            f"{rig}_hull{tag}": _pack(_chassis(variant, 0)),
        }
        for move in (1, 2):
            frames[f"{stem}_move{move}"] = render_surveyor(variant, move=move)
            frames[f"{rig}_hull{tag}_move{move}"] = _pack(_chassis(variant, move))
        for frame in range(1, RADAR_UNFOLD_FRAMES + 1):
            frames[f"{stem}_action{frame}"] = render_surveyor(
                variant, deploy=frame / RADAR_UNFOLD_FRAMES
            )
        for frame in range(RADAR_SCAN_FRAMES):
            frames[f"{stem}_action{RADAR_UNFOLD_FRAMES + frame + 1}"] = render_surveyor(
                variant, deploy=1.0, sweep=frame
            )
        for key, image in frames.items():
            gen.save_sprite(image, out, key)
            registry[key] = image

    mounts = {"": _pack(_reflector(0, 0), rim=False)}
    for frame in range(1, RADAR_UNFOLD_FRAMES + 1):
        mounts[f"_action{frame}"] = _pack(
            _reflector(frame / RADAR_UNFOLD_FRAMES, 0), rim=False
        )
    for frame in range(RADAR_SCAN_FRAMES):
        mounts[f"_action{RADAR_UNFOLD_FRAMES + frame + 1}"] = _pack(
            _reflector(1.0, frame), rim=False
        )
    # The reflector is bare metal; only the chassis carries allegiance paint.
    for suffix, image in mounts.items():
        for tag, layer in (("", image), ("_accent", Image.new("RGBA", image.size))):
            key = f"rig_surveyor_mount{tag}{suffix}"
            gen.save_sprite(layer, out, key)
            registry[key] = layer
