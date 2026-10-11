"""Layered quarry slabs using the approved four-rock material treatment.

Rock walls are drawn by the shell from this library: slabs in a few size
classes, each placed by how thick the rock is around it, so thin walls read as
loose stone and a large mass builds up around a few broad slabs.
"""

import math
import random

from tools.production_sprite_sources.quarry_props import DEEP, Surface

TOP = (79, 76, 79)
RIDGE = (94, 89, 87)
EAST = (62, 60, 66)
FACE = (44, 42, 48)


def block(s, top, ridge, depth=9, fractures=(), strata=True):
    s.poly([(x + 1, y + depth + 2) for x, y in top], DEEP)
    for a, b in zip(top, top[1:] + top[:1]):
        if b[0] < a[0]:
            s.poly([a, b, (b[0], b[1] + depth), (a[0], a[1] + depth)], FACE)
            if strata and a[0] - b[0] > 8:
                for f in (0.4, 0.78):
                    s.line(
                        [
                            (a[0] - 1, a[1] + depth * f),
                            (b[0] + 1, b[1] + depth * f + 0.6),
                        ],
                        DEEP,
                        0.9,
                    )
    s.poly(top, TOP)
    s.poly(ridge, RIDGE)
    for points in fractures:
        s.line(points, DEEP, 1.2)


def chip(s, x, y, size=6):
    s.poly(
        [
            (x - size, y + 1),
            (x - 2, y - size),
            (x + size, y - 2),
            (x + size - 1, y + 4),
            (x, y + 6),
        ],
        DEEP,
    )
    s.poly(
        [
            (x - size, y - 1),
            (x - 2, y - size),
            (x + size, y - 2),
            (x + size - 1, y + 2),
            (x, y + 4),
        ],
        (60, 56, 59),
    )


# Slab radius per size class, in tiles. The shell picks a class from the
# local rock thickness; keep the two lists in step with `shell/src/render/boulders.rs`.
SLAB_RADII = (0.32, 0.44, 0.56, 0.70, 0.86, 1.04)
SLAB_VARIANTS = 6
TILE = 64


def slab_canvas(size):
    """Canvas width and height for one size class, centered on the slab."""
    radius = SLAB_RADII[size] * TILE
    half_w = math.ceil(radius * 1.4 + 10)
    half_h = math.ceil(radius * 0.95 + 14)
    return 2 * half_w, 2 * half_h


def slab(size, variant):
    """One slab: an irregular, wide, flat top with a lit ridge, a short
    stratified face, fractures across large tops, and a fallen chip."""
    rng = random.Random(4801 + size * 97 + variant * 13)
    width, height = slab_canvas(size)
    s = Surface(width, height)
    radius = SLAB_RADII[size] * TILE
    cx, cy = width / 2, height / 2
    depth = 9 if radius < 26 else (10 if radius < 44 else 12)
    sides = rng.choice((6, 7, 8, 9))
    base = rng.random() * math.tau
    angles = sorted(
        base + (i + rng.uniform(-0.3, 0.3)) * math.tau / sides for i in range(sides)
    )
    stretch = rng.uniform(1.0, 1.35)
    squash = rng.uniform(0.6, 0.74)
    lift = cy - depth * 0.45
    top = [
        (
            cx + radius * stretch * rng.uniform(0.6, 1.0) * math.cos(a),
            lift + radius * squash * rng.uniform(0.62, 1.0) * math.sin(a),
        )
        for a in angles
    ]
    if sides >= 8 and rng.random() < 0.35:
        # A missing wedge, like the broken crests of the single stones.
        k = rng.randrange(sides)
        top[k] = (cx + (top[k][0] - cx) * 0.45, lift + (top[k][1] - lift) * 0.45)
    anchor_x, anchor_y = cx - radius * 0.16, lift - radius * 0.2
    ridge = [
        (anchor_x + (x - anchor_x) * 0.6, anchor_y + (y - anchor_y) * 0.6)
        for x, y in top
    ]
    fractures = []
    for _ in range(0 if radius < 24 else (1 if radius < 48 else 2)):
        a0 = rng.uniform(0, math.tau)
        fx = cx + radius * 0.5 * stretch * math.cos(a0)
        fy = lift + radius * 0.4 * squash * math.sin(a0)
        path = [(fx, fy)]
        heading = a0 + math.pi + rng.uniform(-0.6, 0.6)
        for _ in range(rng.choice((2, 3))):
            step = radius * rng.uniform(0.2, 0.35)
            heading += rng.uniform(-0.7, 0.7)
            fx += step * math.cos(heading)
            fy += step * 0.7 * math.sin(heading)
            path.append((fx, fy))
        fractures.append(path)
    block(s, top, ridge, depth, fractures)
    # The shaded east facet runs top to bottom down the slab's right side;
    # angles normalized to (-pi, pi] keep its vertices in one sweep.
    east = [
        point
        for _, point in sorted(
            ((a + math.pi) % math.tau - math.pi, point)
            for point, a in zip(top, angles)
            if math.cos(a) > 0.25
        )
    ]
    if len(east) >= 2:
        s.poly(
            [(cx + radius * 0.18, lift - radius * 0.25)]
            + east
            + [(cx + radius * 0.12, lift + radius * 0.3)],
            EAST,
        )
    if rng.random() < 0.5:
        a = rng.uniform(0.2, 2.9)
        chip(
            s,
            cx + radius * 1.05 * math.cos(a),
            cy + radius * 0.85 * math.sin(a),
            rng.choice((3, 4, 5)),
        )
    return s.finish()


def frames():
    for size in range(len(SLAB_RADII)):
        for variant in range(SLAB_VARIANTS):
            yield f"rock_slab_{size}_{variant}", slab(size, variant)
