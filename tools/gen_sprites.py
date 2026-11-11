# /// script
# requires-python = ">=3.14"
# dependencies = ["pillow==12.3.0"]  # pinned: generator behavior must reproduce
# ///
"""Generates every sprite in assets/sprites/.

This script is the source of truth for the game's art: edit here, run
`uv run tools/gen_sprites.py`, and commit the PNGs together with the script.
Output is deterministic (fixed seeds, no timestamps in the pixels) so a
regenerated sprite only differs when the code does. The default run rewrites
only the files whose pixels or metadata changed, since PNG encoder bytes are
not part of that contract.

Pass --out DIRECTORY to write a complete alternate bank for review without
touching the checked-in assets.

Style: flat top-down geometry, supersampled 4x for clean edges, rust-orange
accents that the shell tints by allegiance. Units face up; the shell rotates
them toward their heading.
"""

import json
import math
import random
import shutil
import sys
import tempfile
from pathlib import Path

from PIL import Image

if __package__ in (None, ""):
    # Run as a script: make the `tools` package importable.
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from tools.production_sprite_sources.construction_final import (
    install_finalized_construction,
)
from tools.production_sprite_sources.environment_final import (
    install_finalized_environment,
)
from tools.production_sprite_sources.finalized import install_finalized_sprites
from tools.production_sprite_sources.maw_final import install_maw
from tools.production_sprite_sources.roc_final import install_roc
from tools.sprite_kit import (
    canvas,
    s,
    save_sprite,
    BONE,
    GROUND,
    GROUND_DARK,
    GROUND_LIGHT,
    IRON,
    IRON_DARK,
    IRON_LIGHT,
    PROBE_TAG,
    SCRAP,
    SCRAP_LIGHT,
    SS,
)

OUT = Path(__file__).resolve().parent.parent / "assets" / "sprites"

class SpriteRegistry(dict):
    """The sprite bank: every finished sprite lands here, so main() can pack
    one atlas the shell renders from. Each key is written once, so a recipe
    whose rows a later pass would replace fails here instead of drawing art
    that never ships."""

    def __setitem__(self, key: str, image: Image.Image) -> None:
        if key in self:
            raise KeyError(f"sprite {key!r} is written twice")
        super().__setitem__(key, image)


REGISTRY: SpriteRegistry = SpriteRegistry()

# The plain ground tiles quarry_final paints its finished ground over. They
# are inputs, not shipped sprites, so they stay out of the registry.
GROUND_CONTROLS: dict[str, Image.Image] = {}

def finish(img: Image.Image, px: int, name: str) -> None:
    img = img.resize((px, px), Image.LANCZOS)
    save_sprite(img, OUT, name)
    REGISTRY[name] = img
    if PROBE_TAG not in name:
        print(f"  {name}.png")


def pack_atlas() -> None:
    """Pack exact duplicate frames once into deterministic 4096px texture pages.

    Manifest coordinates address vertically stacked pages. Each sprite keeps
    its full canvas and one-pixel edge extrusion inside a two-pixel gutter.
    """
    pad = 2
    page_size = 4096
    entries = sorted(REGISTRY.items(), key=lambda kv: (-kv[1].height, kv[0]))
    placements: dict[str, tuple[int, int, int, int]] = {}
    unique: dict[tuple[tuple[int, int], bytes], str] = {}
    x, y, shelf_h = pad, pad, 0
    for name, img in entries:
        fingerprint = (img.size, img.tobytes())
        if fingerprint in unique:
            placements[name] = placements[unique[fingerprint]]
            continue
        w, h = img.size
        if max(w, h) + 2 * pad > page_size:
            raise ValueError(f"sprite {name} exceeds the texture page size")
        if x + w + pad > page_size:
            x = pad
            y += shelf_h + 2 * pad
            shelf_h = 0
        if y % page_size + h + pad > page_size:
            x = pad
            y = (y // page_size + 1) * page_size + pad
            shelf_h = 0
        placements[name] = (x, y, w, h)
        unique[fingerprint] = name
        shelf_h = max(shelf_h, h)
        x += w + 2 * pad
    total_height = y + shelf_h + pad
    pages = [
        Image.new("RGBA", (page_size, min(page_size, total_height - offset)))
        for offset in range(0, total_height, page_size)
    ]
    for name in unique.values():
        px, py, w, h = placements[name]
        atlas = pages[py // page_size]
        py %= page_size
        img = REGISTRY[name]
        atlas.paste(img, (px, py))
        atlas.paste(img.crop((0, 0, w, 1)), (px, py - 1))
        atlas.paste(img.crop((0, h - 1, w, h)), (px, py + h))
        atlas.paste(img.crop((0, 0, 1, h)), (px - 1, py))
        atlas.paste(img.crop((w - 1, 0, w, h)), (px + w, py))
    for stale in OUT.glob("atlas_*.png"):
        if stale.stem.removeprefix("atlas_").isdigit():
            stale.unlink()
    for index, page in enumerate(pages):
        page.save(OUT / (f"atlas_{index}.png" if index else "atlas.png"))
    with open(OUT / "atlas.json", "w") as f:
        json.dump(
            {name: list(rect) for name, rect in sorted(placements.items())},
            f,
            indent=1,
            sort_keys=True,
        )
    print(
        f"  atlas: {len(pages)} pages, {len(placements)} sprites, {len(unique)} unique frames"
    )


def accent_masks() -> None:
    """Derives one allegiance-accent mask per probe render, then drops the
    probe renders from the registry.

    A probe render shares its base sprite's silhouette and differs only in
    accent color, so the pixels where the two differ are exactly the region
    an owner tint should cover (the RTS team-color mask, derived instead of
    hand-painted). The mask is luminance-preserving grayscale: the shell
    multiplies it by an allegiance hue, keeping the original shading. Rim
    light and chassis grays cancel in the diff and stay untinted.
    """
    from PIL import ImageChops

    for name in [n for n in sorted(REGISTRY) if PROBE_TAG in n]:
        probe = REGISTRY.pop(name)
        base = REGISTRY[name.replace(PROBE_TAG, "")]
        diff = ImageChops.difference(base.convert("RGB"), probe.convert("RGB"))
        r, g, b = diff.split()
        weight = ImageChops.lighter(ImageChops.lighter(r, g), b).point(
            lambda v: min(255, v * 3)
        )
        alpha = ImageChops.multiply(base.split()[3], weight)
        # Base accent luminance is ~116/255; x1.7 restores full-tint
        # brightness to the palette midpoint without clipping "light".
        lum = base.convert("L").point(lambda v: min(255, round(v * 1.7)))
        mask = Image.merge("RGBA", (lum, lum, lum, alpha))
        out_name = name.replace(PROBE_TAG, "_accent")
        mask.save(OUT / f"{out_name}.png")
        REGISTRY[out_name] = mask
        print(f"  {out_name}.png")


def ground(variant: int) -> None:
    px = 64
    # Variants sweep a subtle brightness range so the field reads as varied
    # terrain instead of a flat wash.
    lift = [-6, -3, 0, 2, 4, 7][variant % 6]
    base = tuple(max(0, c + lift) for c in GROUND)
    img, _ = canvas(px, (*base, 255))
    GROUND_CONTROLS[f"ground_{variant}"] = img.resize((px, px), Image.LANCZOS)


def ground_detail(variant: int) -> None:
    """Grit and a crack drawn apart from the shade, so the shell can vary
    them independently of the shade's diagonal wave."""
    px = 64
    img, d = canvas(px)
    rng = random.Random(1000 + variant)
    # Sparse grit: a few darker and lighter flecks, nothing that tiles loudly.
    for _ in range(26):
        x, y = rng.randrange(2, 62), rng.randrange(2, 62)
        w = rng.choice((1, 1, 2))
        color = rng.choice((GROUND_DARK, GROUND_DARK, GROUND_LIGHT))
        d.rectangle([s(x), s(y), s(x + w), s(y + w)], fill=(*color, 255))
    # One hairline crack.
    x0, y0 = rng.randrange(6, 30), rng.randrange(6, 58)
    points = [(x0, y0)]
    for _ in range(rng.randrange(3, 5)):
        x0 += rng.randrange(4, 12)
        y0 += rng.randrange(-6, 7)
        points.append((x0, y0))
    d.line([(s(x), s(y)) for x, y in points], fill=(*GROUND_DARK, 255), width=SS)
    finish(img, px, f"ground_detail_{variant}")


def scaffold() -> None:
    """Construction lattice drawn over a translucent rising site."""
    px = 64
    img, d = canvas(px)
    beam = (*IRON_LIGHT, 255)
    dark = (*IRON_DARK, 255)
    for x in (4, 56):
        d.rectangle([s(x), s(4), s(x + 4), s(60)], fill=dark)
    for y in (4, 56):
        d.rectangle([s(4), s(y), s(60), s(y + 4)], fill=dark)
    for y in range(8, 54, 26):
        d.line([(s(6), s(y)), (s(58), s(y + 11))], fill=beam, width=s(2))
        d.line([(s(58), s(y)), (s(6), s(y + 11))], fill=beam, width=s(2))
    finish(img, px, "scaffold_sparse")


def debris(variant: int) -> None:
    """A torn hull shard for death scatter."""
    px = 32
    img, d = canvas(px)
    shapes = [
        [(6, 10), (18, 4), (26, 14), (14, 24)],
        [(4, 18), (14, 6), (28, 10), (22, 26), (8, 28)],
        [(8, 8), (24, 6), (28, 20), (12, 26)],
    ]
    pts = [(s(x), s(y)) for x, y in shapes[variant]]
    d.polygon(pts, fill=(*IRON, 255))
    d.line([*pts[:2]], fill=(*IRON_LIGHT, 255), width=s(2))
    d.line([*pts[-2:]], fill=(*IRON_DARK, 255), width=s(2))
    finish(img, px, f"debris_{variant}")


def wreck_pile(
    name: str = "wreck_pile",
    seed: int = 53,
    px: int = 64,
    scale: float = 1.0,
    wide: float = 1.0,
    pieces: int = 11,
    piece_scale: float = 1.0,
    shards: tuple[tuple[float, float], ...] | None = None,
) -> None:
    """Battlefield salvage on open ground: like a scrap heap but in dead
    machine tones with only a little amber — walkable junk, not a node.

    Without `shards`, three overlapping dots make the shipped pile's one
    sliver of amber. Each of `shards` is a (length, width) sliver instead."""
    img, d = canvas(px)
    rng = random.Random(seed)
    mid = px / 2
    rx, ry = 18 * scale * wide, 9 * scale
    d.ellipse(
        [s(mid - rx), s(mid + 5 - ry), s(mid + rx), s(mid + 5 + ry)],
        fill=(24, 20, 16, 110),
    )
    reach = 11 * scale
    for _ in range(pieces):
        angle = rng.uniform(0, 6.28318)
        dist = reach * rng.random() ** 0.6
        cx = mid + wide * dist * math.cos(angle)
        cy = mid + 3 + dist * math.sin(angle) * 0.6 - 2 * scale * (1 - dist / reach)
        w = rng.uniform(5, 10) * piece_scale
        h = rng.uniform(4, 7) * piece_scale
        tone = rng.choice([(58, 58, 68), (44, 44, 52), (86, 64, 40), (72, 72, 84)])
        dx = rng.uniform(-2, 2)
        d.polygon(
            [
                (s(cx - w / 2 - dx), s(cy - h / 2)),
                (s(cx + w / 2), s(cy - h / 2 + dx / 2)),
                (s(cx + w / 2 + dx), s(cy + h / 2)),
                (s(cx - w / 2), s(cy + h / 2 - dx / 2)),
            ],
            fill=(*tone, 255),
        )
    if shards is None:
        for _ in range(3):
            cx, cy = mid + rng.uniform(-6, 6), mid + 2 + rng.uniform(-4, 4)
            d.ellipse(
                [s(cx - 1.5), s(cy - 1.5), s(cx + 1.5), s(cy + 1.5)], fill=(*SCRAP, 255)
            )
    else:
        # Slivers, never round dots or a matched pair: those read as eyes.
        for i, (length, width) in enumerate(shards):
            spread = 1.0 + i * 1.5
            cx = mid + wide * scale * rng.uniform(-5, 5) * spread
            cy = mid + 2 + scale * rng.uniform(-3, 3) * spread
            turn = -math.pi / 3 + rng.uniform(-0.5, 0.5) + i * 1.1
            hl, hw = length / 2, width / 2
            shape = [
                (-hl, -hw * rng.uniform(0.3, 0.7)),
                (-hl * rng.uniform(0.1, 0.4), -hw),
                (hl, -hw * rng.uniform(0.2, 0.6)),
                (hl * rng.uniform(0.5, 0.9), hw * 0.8),
                (-hl * rng.uniform(0.5, 0.9), hw),
            ]
            ca, sa = math.cos(turn), math.sin(turn)
            d.polygon(
                [(s(cx + x * ca - y * sa), s(cy + x * sa + y * ca)) for x, y in shape],
                fill=(*SCRAP, 255),
            )
    finish(img, px, name)


def wreck_piles() -> None:
    """A wreck's pile by the salvage it holds, two shapes to a size: the
    shipped pile's recipe at real size, on a canvas that fits the largest."""
    px = 96
    wreck_pile("wreck_pile_0_0", 71, px, 0.72, 1.0, 6, 0.9, ((4.5, 2.2),))
    wreck_pile("wreck_pile_0_1", 72, px, 0.72, 1.12, 6, 0.9, ((4, 2.2),))
    wreck_pile("wreck_pile_1_0", 53, px)
    wreck_pile("wreck_pile_1_1", 54, px, 1.0, 0.92, 11, 1.0, ((6.5, 3),))
    wreck_pile("wreck_pile_2_0", 81, px, 1.22, 1.0, 15, 1.04, ((7, 3.2),))
    wreck_pile("wreck_pile_2_1", 82, px, 1.22, 1.1, 15, 1.04, ((7.5, 3),))
    wreck_pile("wreck_pile_3_0", 91, px, 1.45, 1.04, 20, 1.08, ((8, 3.4), (4, 1.8)))
    wreck_pile("wreck_pile_3_1", 92, px, 1.45, 0.94, 20, 1.08, ((8.5, 3.2), (4.5, 1.8)))


def air_shadow() -> None:
    """The soft blob a flyer casts on the ground — drawn separately and
    offset by the shell so altitude reads at a glance."""
    px = 64
    img, d = canvas(px)
    for r, alpha in [(22, 40), (16, 60), (10, 80)]:
        d.ellipse(
            [s(32 - r), s(36 - r * 0.6), s(32 + r), s(36 + r * 0.6)],
            fill=(8, 8, 12, alpha),
        )
    finish(img, px, "air_shadow")


def burst() -> None:
    """One frame of splash detonation: bright core, hot ring — the shell
    scales and fades it over the effect's life."""
    px = 64
    img, d = canvas(px)
    d.ellipse([s(10), s(10), s(54), s(54)], fill=(255, 200, 120, 60))
    d.ellipse([s(16), s(16), s(48), s(48)], outline=(255, 220, 150, 200), width=s(3))
    d.ellipse([s(24), s(24), s(40), s(40)], fill=(255, 240, 200, 200))
    d.ellipse([s(28), s(28), s(36), s(36)], fill=(255, 255, 240, 255))
    finish(img, px, "burst")


def rock_skirt() -> None:
    """A soft shadow cast from the top edge; rotated at draw time toward
    whichever neighbor holds the rock."""
    px = 64
    img, d = canvas(px)
    for row in range(20):
        alpha = int(70 * (1.0 - row / 20) ** 1.6)
        d.rectangle([0, s(row), s(64), s(row + 1)], fill=(10, 10, 14, alpha))
    finish(img, px, "rock_skirt")


def theme_prop(theme: str, variant: int) -> None:
    """One-tile flat surface mark for a shipped map theme. None resembles a
    resource, structure, or cover marker, and each can rotate freely.
    """
    px = 64
    img, d = canvas(px)
    rng = random.Random(2000 + sum(theme.encode()) * 17 + variant * 101)

    if theme == "rusted_yard":
        if variant == 0:
            # Rails buried flush in the yard, with faded cross-ties.
            for y in (23, 41):
                d.line(
                    [(s(5), s(y)), (s(59), s(y))], fill=(76, 55, 49, 120), width=s(3)
                )
                d.line(
                    [(s(5), s(y - 1)), (s(59), s(y - 1))],
                    fill=(133, 71, 49, 90),
                    width=SS,
                )
            for x in range(10, 60, 10):
                d.line(
                    [(s(x), s(18)), (s(x), s(46))], fill=(47, 43, 45, 100), width=s(2)
                )
        elif variant == 1:
            # A riveted repair plate, inset into rather than sitting on the floor.
            pts = [(12, 17), (48, 12), (55, 43), (18, 51)]
            d.polygon([(s(x), s(y)) for x, y in pts], fill=(66, 51, 49, 72))
            d.line(
                [(s(x), s(y)) for x, y in [*pts, pts[0]]],
                fill=(137, 70, 48, 105),
                width=s(2),
            )
            for x, y in pts:
                d.ellipse(
                    [s(x - 1), s(y - 1), s(x + 1), s(y + 1)], fill=(173, 102, 70, 130)
                )
        else:
            # Oil soaked into oxidized concrete.
            for _ in range(5):
                cx, cy = rng.randrange(18, 47), rng.randrange(18, 47)
                rx, ry = rng.randrange(6, 14), rng.randrange(3, 9)
                d.ellipse(
                    [s(cx - rx), s(cy - ry), s(cx + rx), s(cy + ry)],
                    fill=(24, 20, 22, 42),
                )
            d.arc(
                [s(15), s(19), s(50), s(48)],
                12,
                196,
                fill=(127, 65, 45, 85),
                width=s(2),
            )
    elif theme == "cold_circuitry":
        if variant == 0:
            # An exposed trace with low, flush contact pads.
            pts = [(7, 42), (22, 42), (22, 19), (43, 19), (43, 51), (58, 51)]
            d.line([(s(x), s(y)) for x, y in pts], fill=(92, 160, 174, 115), width=s(2))
            for x, y in (pts[0], pts[-1], (22, 19), (43, 51)):
                d.rectangle(
                    [s(x - 2), s(y - 2), s(x + 2), s(y + 2)], fill=(138, 193, 202, 125)
                )
        elif variant == 1:
            # A recessed cooling grille.
            d.rounded_rectangle(
                [s(12), s(18), s(52), s(46)],
                radius=s(4),
                fill=(30, 43, 50, 75),
                outline=(96, 133, 144, 105),
                width=s(2),
            )
            for x in range(18, 50, 7):
                d.line([(s(x), s(23)), (s(x), s(41))], fill=(15, 25, 31, 115), width=SS)
        else:
            # Hairline panel seams and cold status marks.
            pts = [(6, 27), (18, 15), (48, 15), (58, 25), (48, 49), (18, 49), (6, 37)]
            d.line([(s(x), s(y)) for x, y in pts], fill=(76, 111, 124, 85), width=SS)
            d.line(
                [(s(18), s(49)), (s(18), s(33)), (s(37), s(33))],
                fill=(114, 180, 190, 95),
                width=SS,
            )
    elif theme == "quarry_dust":
        if variant == 0:
            # Twin vehicle ruts, broken so they do not resemble walls.
            for y in (22, 42):
                for x in range(5, 58, 12):
                    d.line(
                        [(s(x), s(y)), (s(x + 7), s(y))],
                        fill=(104, 88, 67, 92),
                        width=s(3),
                    )
                    d.line(
                        [(s(x), s(y - 1)), (s(x + 7), s(y - 1))],
                        fill=(150, 130, 96, 55),
                        width=SS,
                    )
        elif variant == 1:
            # Saw scoring left in a worked stone floor.
            for offset in (-7, 0, 7):
                d.arc(
                    [s(9 + offset), s(10), s(55 + offset), s(55)],
                    205,
                    320,
                    fill=(126, 108, 82, 92),
                    width=s(2),
                )
        else:
            # Shallow drill-test dimples, not a loose pile.
            for angle in range(0, 360, 45):
                rad = math.radians(angle)
                x, y = 32 + math.cos(rad) * 17, 32 + math.sin(rad) * 12
                d.ellipse(
                    [s(x - 2), s(y - 2), s(x + 2), s(y + 2)], fill=(82, 72, 62, 105)
                )
                d.point((s(x - 0.5), s(y - 0.5)), fill=(173, 151, 112, 85))
    elif theme == "basalt":
        if variant == 0:
            # A branching cooled fracture.
            pts = [(5, 47), (19, 39), (27, 25), (39, 29), (57, 12)]
            d.line([(s(x), s(y)) for x, y in pts], fill=(20, 20, 29, 150), width=s(2))
            d.line([(s(27), s(25)), (s(23), s(11))], fill=(71, 67, 91, 80), width=SS)
            d.line([(s(39), s(29)), (s(50), s(43))], fill=(71, 67, 91, 70), width=SS)
        elif variant == 1:
            # A glassy seam with a faint mineral edge.
            pts = [(3, 38), (15, 31), (25, 34), (38, 24), (60, 29)]
            d.line([(s(x), s(y)) for x, y in pts], fill=(15, 16, 23, 120), width=s(5))
            d.line([(s(x), s(y - 2)) for x, y in pts], fill=(86, 76, 112, 65), width=SS)
        else:
            # Interlocking lava joints: broad geometry, no raised boulders.
            joints = [
                (8, 31),
                (18, 15),
                (38, 13),
                (54, 28),
                (45, 49),
                (23, 52),
                (8, 31),
            ]
            d.line([(s(x), s(y)) for x, y in joints], fill=(70, 65, 91, 82), width=s(2))
            d.line(
                [(s(18), s(15)), (s(27), s(32)), (s(23), s(52))],
                fill=(24, 23, 34, 115),
                width=SS,
            )
            d.line([(s(54), s(28)), (s(27), s(32))], fill=(24, 23, 34, 100), width=SS)
    elif theme == "slag":
        if variant == 0:
            # A thin plate of vitrified slag fused to the floor.
            pts = [(10, 19), (39, 11), (55, 26), (48, 48), (21, 53), (7, 36)]
            d.polygon([(s(x), s(y)) for x, y in pts], fill=(35, 28, 31, 62))
            d.line(
                [(s(x), s(y)) for x, y in [*pts, pts[0]]],
                fill=(120, 63, 62, 75),
                width=SS,
            )
            d.line(
                [(s(18), s(43)), (s(34), s(29)), (s(48), s(35))],
                fill=(157, 72, 61, 58),
                width=SS,
            )
        elif variant == 1:
            # Heat bloom baked into the surface.
            for rx, ry, alpha in ((23, 14, 30), (16, 9, 40), (9, 5, 48)):
                d.ellipse(
                    [s(32 - rx), s(32 - ry), s(32 + rx), s(32 + ry)],
                    outline=(137, 56, 52, alpha),
                    width=s(3),
                )
        else:
            # Parallel runoff channels, recessed and discontinuous.
            for offset in (-9, 0, 9):
                pts = [
                    (7, 18 + offset),
                    (23, 25 + offset),
                    (38, 22 + offset),
                    (57, 34 + offset),
                ]
                d.line(
                    [(s(x), s(y)) for x, y in pts], fill=(45, 31, 34, 105), width=s(2)
                )
    elif theme == "verdigris":
        if variant == 0:
            # Copper panel seams with oxidized edges.
            pts = [(9, 18), (31, 11), (55, 22), (49, 48), (22, 53), (9, 39), (9, 18)]
            d.line([(s(x), s(y)) for x, y in pts], fill=(74, 126, 111, 95), width=s(2))
            for x, y in pts[:-1:2]:
                d.ellipse(
                    [s(x - 1), s(y - 1), s(x + 1), s(y + 1)], fill=(142, 94, 60, 105)
                )
        elif variant == 1:
            # A buried conduit with flush junction caps.
            pts = [(4, 43), (18, 43), (27, 27), (46, 27), (58, 15)]
            d.line([(s(x), s(y)) for x, y in pts], fill=(119, 76, 52, 105), width=s(3))
            d.line(
                [(s(x), s(y - 1)) for x, y in pts], fill=(69, 153, 132, 105), width=SS
            )
            for x, y in ((18, 43), (46, 27)):
                d.ellipse(
                    [s(x - 3), s(y - 3), s(x + 3), s(y + 3)], fill=(48, 106, 94, 105)
                )
        else:
            # Patina leaching through the floor in an irregular bloom.
            for _ in range(7):
                cx, cy = rng.randrange(16, 49), rng.randrange(16, 49)
                rx, ry = rng.randrange(4, 11), rng.randrange(3, 8)
                d.ellipse(
                    [s(cx - rx), s(cy - ry), s(cx + rx), s(cy + ry)],
                    fill=(58, 151, 130, 24 + rng.randrange(0, 25)),
                )
    else:
        raise ValueError(f"unknown theme {theme!r}")

    alpha = img.getchannel("A").point(
        lambda a: min(235, round(a * 1.55 + 12)) if a else 0
    )
    img.putalpha(alpha)
    finish(img, px, f"theme_{theme}_{variant}")


def scorch() -> None:
    px = 128
    img, d = canvas(px)
    rng = random.Random(77)
    for r, alpha in [(56, 60), (44, 90), (30, 120)]:
        d.ellipse(
            [s(64 - r), s(64 - r), s(64 + r), s(64 + r)], fill=(12, 10, 10, alpha)
        )
    for _ in range(14):
        ang = rng.uniform(0, 6.28318)
        import math as _m

        fr = rng.uniform(30, 60)
        x, y = 64 + fr * _m.cos(ang), 64 + fr * _m.sin(ang)
        d.ellipse([s(x - 3), s(y - 3), s(x + 3), s(y + 3)], fill=(16, 14, 12, 90))
    finish(img, px, "scorch")


ICON = 48


def _icon_bar(d, a, b, w: float, color) -> None:
    """A thick line as a polygon — PIL's line joins are ragged at 4x."""
    import math as _m

    (x0, y0), (x1, y1) = a, b
    dx, dy = x1 - x0, y1 - y0
    ln = _m.hypot(dx, dy) or 1.0
    px, py = -dy / ln * w * 0.5, dx / ln * w * 0.5
    d.polygon(
        [
            (s(x0 + px), s(y0 + py)),
            (s(x1 + px), s(y1 + py)),
            (s(x1 - px), s(y1 - py)),
            (s(x0 - px), s(y0 - py)),
        ],
        fill=color,
    )


def icon_stop() -> None:
    """Everything halts: one solid block."""
    img, d = canvas(ICON)
    d.rectangle([s(13), s(13), s(35), s(35)], fill=(*BONE, 255))
    finish(img, ICON, "icon_stop")


def icon_move() -> None:
    """A plain march: arrow up, no opinions."""
    img, d = canvas(ICON)
    d.polygon([(s(24), s(6)), (s(40), s(24)), (s(8), s(24))], fill=(*BONE, 255))
    d.rectangle([s(19), s(24), s(29), s(42)], fill=(*BONE, 255))
    finish(img, ICON, "icon_move")


def icon_run() -> None:
    img, d = canvas(ICON)
    points = [
        (28, 5),
        (40.5, 18),
        (31.5, 18),
        (31.5, 41),
        (24.5, 41),
        (24.5, 18),
        (15.5, 18),
    ]
    d.polygon([(s(x), s(y)) for x, y in points], fill=(*BONE, 255))
    for start, end, y in [(7, 14, 24), (5, 17, 32), (9, 17, 40)]:
        d.line([(s(start), s(y)), (s(end), s(y))], fill=(*BONE, 255), width=s(3))
    finish(img, ICON, "icon_run")


def icon_hunt() -> None:
    img, d = canvas(ICON)
    for points in [
        [(18, 7), (8, 7), (8, 18)],
        [(30, 7), (40, 7), (40, 18)],
        [(8, 30), (8, 40), (18, 40)],
        [(40, 30), (40, 40), (30, 40)],
    ]:
        d.line(
            [(s(x), s(y)) for x, y in points],
            fill=(*BONE, 255),
            width=s(4),
            joint="curve",
        )
    points = [
        (24, 13),
        (33, 27),
        (27, 25),
        (27, 35),
        (21, 35),
        (21, 25),
        (15, 27),
    ]
    d.polygon([(s(x), s(y)) for x, y in points], fill=(*SCRAP_LIGHT, 255))
    finish(img, ICON, "icon_hunt")


def icon_advance() -> None:
    """The fighting march: the move arrow wearing blades."""
    img, d = canvas(ICON)
    d.polygon([(s(24), s(4)), (s(40), s(22)), (s(8), s(22))], fill=(*BONE, 255))
    d.rectangle([s(19), s(22), s(29), s(40)], fill=(*BONE, 255))
    # Blades off the arrowhead.
    d.polygon([(s(4), s(28)), (s(14), s(24)), (s(12), s(33))], fill=(*SCRAP_LIGHT, 255))
    d.polygon(
        [(s(44), s(28)), (s(34), s(24)), (s(36), s(33))], fill=(*SCRAP_LIGHT, 255)
    )
    finish(img, ICON, "icon_advance")


def icon_attack() -> None:
    """A strike burst: eight points of contact."""
    img, d = canvas(ICON)
    cx, cy = 24, 24
    import math as _m

    pts = []
    for i in range(16):
        ang = _m.pi * 2 * i / 16 - _m.pi / 2
        r = 19 if i % 2 == 0 else 8
        pts.append((s(cx + _m.cos(ang) * r), s(cy + _m.sin(ang) * r)))
    d.polygon(pts, fill=(*BONE, 255))
    d.ellipse([s(19), s(19), s(29), s(29)], fill=(*SCRAP, 255))
    finish(img, ICON, "icon_attack")


def _icon_box(d, x0: float, y0: float, x1: float, y1: float, color) -> None:
    """A rectangle whose edges land on whole pixels once downsampled, so
    straight strokes stay as crisp as the solid icons beside them."""
    d.rectangle([s(x0), s(y0), s(x1) - 1, s(y1) - 1], fill=color)


def icon_patrol() -> None:
    """A closed round: the loop with its arrowhead, at the family's
    stroke weight."""
    img, d = canvas(ICON)
    clear = (0, 0, 0, 0)
    _icon_box(d, 6, 6, 42, 42, (*BONE, 255))
    _icon_box(d, 13, 13, 35, 35, clear)
    # The gap the arrowhead leaves in the loop's lower-left corner.
    _icon_box(d, 6, 27, 13, 42, clear)
    _icon_box(d, 6, 35, 21, 42, clear)
    # Arrowhead riding the bottom leg, pointing the way around.
    d.polygon([(s(5), s(38.5)), (s(21), s(30)), (s(21), s(47))], fill=(*BONE, 255))
    finish(img, ICON, "icon_patrol")


def icon_harvest() -> None:
    """The scrap pyramid: what the economy is made of."""
    img, d = canvas(ICON)
    for x, y in [(14, 14), (26, 14), (8, 26), (20, 26), (32, 26)]:
        d.rectangle([s(x), s(y), s(x + 9), s(y + 9)], fill=(*SCRAP, 255))
        d.rectangle([s(x), s(y), s(x + 9), s(y + 3)], fill=(*SCRAP_LIGHT, 255))
    finish(img, ICON, "icon_harvest")


def icon_build() -> None:
    """A tower crane lowering a scrap block into place."""
    img, d = canvas(ICON)
    bone = (*BONE, 255)
    _icon_box(d, 9, 5, 15, 44, bone)  # mast
    _icon_box(d, 4, 40, 20, 44, bone)  # footing
    _icon_box(d, 5, 5, 43, 11, bone)  # jib
    _icon_box(d, 15, 11, 19, 15, bone)  # jib brace
    _icon_box(d, 32, 11, 34, 23, bone)  # hoist line
    _icon_box(d, 25, 23, 41, 36, (*SCRAP, 255))
    _icon_box(d, 25, 23, 41, 26, (*SCRAP_LIGHT, 255))
    finish(img, ICON, "icon_build")


def icon_repair() -> None:
    """The weld: a plus, sparking."""
    img, d = canvas(ICON)
    d.rectangle([s(19), s(9), s(29), s(39)], fill=(*BONE, 255))
    d.rectangle([s(9), s(19), s(39), s(29)], fill=(*BONE, 255))
    d.ellipse([s(33), s(7), s(41), s(15)], fill=(*SCRAP_LIGHT, 255))
    finish(img, ICON, "icon_repair")


def icon_salvage() -> None:
    """Value coming back down: an arrow into the pile."""
    img, d = canvas(ICON)
    d.rectangle([s(19), s(4), s(29), s(20)], fill=(*BONE, 255))
    d.polygon([(s(24), s(32)), (s(38), s(18)), (s(10), s(18))], fill=(*BONE, 255))
    for x in (10, 20, 30):
        d.rectangle([s(x), s(36), s(x + 8), s(44)], fill=(*SCRAP, 255))
    finish(img, ICON, "icon_salvage")


def icon_cancel() -> None:
    """A refusal: the cross."""
    img, d = canvas(ICON)
    _icon_bar(d, (11, 11), (37, 37), 8.0, (*BONE, 255))
    _icon_bar(d, (37, 11), (11, 37), 8.0, (*BONE, 255))
    finish(img, ICON, "icon_cancel")


def icon_rally() -> None:
    """The rally pennant on its pole."""
    img, d = canvas(ICON)
    d.rectangle([s(12), s(6), s(17), s(42)], fill=(*BONE, 255))
    d.polygon([(s(17), s(8)), (s(42), s(15)), (s(17), s(24))], fill=(*SCRAP, 255))
    finish(img, ICON, "icon_rally")


def icon_accompany() -> None:
    """Escort duty: the leader ahead, two escorts keeping pace at its flanks."""
    img, d = canvas(ICON)

    def arrow(
        cx: float,
        top: float,
        half: float,
        head: float,
        stem: float,
        bottom: float,
        color,
    ):
        d.polygon(
            [
                (s(cx), s(top)),
                (s(cx + half), s(top + head)),
                (s(cx - half), s(top + head)),
            ],
            fill=color,
        )
        d.rectangle(
            [s(cx - stem / 2), s(top + head), s(cx + stem / 2), s(bottom)], fill=color
        )

    arrow(24, 4, 9, 11, 6, 30, (*SCRAP, 255))
    arrow(11, 16, 7, 9, 5, 45, (*BONE, 255))
    arrow(37, 16, 7, 9, 5, 45, (*BONE, 255))
    finish(img, ICON, "icon_accompany")


def icon_retire() -> None:
    """Retirement: a gear melting down, its lower half running off as scrap."""
    img, d = canvas(ICON)
    cx, cy = 24, 18
    teeth = []
    for i in range(16):
        ang = math.pi * 2 * i / 16
        r = 14 if i % 2 == 0 else 10
        teeth.append((s(cx + math.cos(ang) * r), s(cy + math.sin(ang) * r)))
    d.polygon(teeth, fill=(*BONE, 255))
    clear = (0, 0, 0, 0)
    d.ellipse([s(19), s(13), s(29), s(23)], fill=clear)
    d.rectangle([s(8), s(22), s(40), s(32)], fill=clear)
    for x, length in ((14, 10), (24, 16), (34, 8)):
        d.rounded_rectangle(
            [s(x - 3), s(24), s(x + 3), s(24 + length)], radius=s(3), fill=(*SCRAP, 255)
        )
    finish(img, ICON, "icon_retire")


def icon_idle() -> None:
    """Standing by: the three-beat wait."""
    img, d = canvas(ICON)
    for i, x in enumerate((8, 20, 32)):
        d.ellipse([s(x), s(21), s(x + 8), s(29)], fill=(*BONE, 200 - i * 30))
    finish(img, ICON, "icon_idle")


# Controller button icons. Hints draw them inline beside text 11 to 18 px
# tall, so each is a solid bone token with a dark label, flat like the verb
# icons, whose outline says what kind of control it is: round face buttons,
# pill shoulders, tab triggers, notched stick clicks and the D-pad's cross
# with its named arms lit. Icons are named by what they show, so families
# that print the same label share one sprite.

# Stroke letters on a unit cap height, as (width, polylines) with y down.
# Corners are cut like the game's typeface rather than rounded.
_PAD_LETTERS: dict[str, tuple[float, tuple[tuple[tuple[float, float], ...], ...]]] = {
    "A": (
        0.78,
        (((0, 1), (0.28, 0), (0.5, 0), (0.78, 1)), ((0.12, 0.64), (0.66, 0.64))),
    ),
    "B": (
        0.7,
        (
            ((0, 0), (0, 1), (0.55, 1), (0.7, 0.85), (0.7, 0.64), (0.56, 0.5), (0, 0.5)),
            ((0, 0), (0.5, 0), (0.64, 0.14), (0.64, 0.36), (0.5, 0.5)),
        ),
    ),
    "X": (0.74, (((0, 0), (0.74, 1)), ((0.74, 0), (0, 1)))),
    "Y": (0.74, (((0, 0), (0.37, 0.5), (0.74, 0)), ((0.37, 0.5), (0.37, 1)))),
    "L": (0.56, (((0, 0), (0, 1), (0.56, 1)),)),
    "R": (
        0.7,
        (
            ((0, 1), (0, 0), (0.55, 0), (0.7, 0.15), (0.7, 0.4), (0.55, 0.55), (0, 0.55)),
            ((0.36, 0.55), (0.7, 1)),
        ),
    ),
    "S": (
        0.68,
        (
            (
                (0.68, 0),
                (0.14, 0),
                (0, 0.14),
                (0, 0.36),
                (0.14, 0.5),
                (0.54, 0.5),
                (0.68, 0.64),
                (0.68, 0.86),
                (0.54, 1),
                (0, 1),
            ),
        ),
    ),
    "T": (0.7, (((0, 0), (0.7, 0)), ((0.35, 0), (0.35, 1)))),
    "Z": (0.66, (((0, 0), (0.66, 0), (0, 1), (0.66, 1)),)),
    "1": (0.36, (((0, 0.22), (0.3, 0), (0.3, 1)),)),
    "2": (
        0.66,
        (
            (
                (0, 0.14),
                (0.14, 0),
                (0.52, 0),
                (0.66, 0.14),
                (0.66, 0.36),
                (0, 1),
                (0.66, 1),
            ),
        ),
    ),
    "3": (
        0.66,
        (
            ((0, 0), (0.52, 0), (0.66, 0.14), (0.66, 0.36), (0.52, 0.5), (0.18, 0.5)),
            ((0.52, 0.5), (0.66, 0.64), (0.66, 0.86), (0.52, 1), (0, 1)),
        ),
    ),
}

# The token, its label and a lit D-pad arm.
_PAD_TOKEN = BONE
_PAD_INK = IRON_DARK
_PAD_LIT = SCRAP


def _pad_stroke(d, points, w: float, color) -> None:
    """A polyline with square ends and filled joints, in icon units."""
    fill = (*color, 255)
    last = len(points) - 1
    for i in range(last):
        (x0, y0), (x1, y1) = points[i], points[i + 1]
        ln = math.hypot(x1 - x0, y1 - y0) or 1.0
        ux, uy = (x1 - x0) / ln, (y1 - y0) / ln
        if i == 0:
            x0, y0 = x0 - ux * w / 2, y0 - uy * w / 2
        if i + 1 == last:
            x1, y1 = x1 + ux * w / 2, y1 + uy * w / 2
        _icon_bar(d, (x0, y0), (x1, y1), w, fill)
    for x, y in points[1:-1]:
        d.ellipse([s(x - w / 2), s(y - w / 2), s(x + w / 2), s(y + w / 2)], fill=fill)


def _pad_text(d, text: str, cx: float, cy: float, h: float, w: float) -> None:
    """Stroke letters centered on (cx, cy), `h` tall between stroke centers."""
    gap = 0.2 * h + w
    widths = [_PAD_LETTERS[ch][0] * h for ch in text]
    x = cx - (sum(widths) + gap * (len(text) - 1)) / 2
    top = cy - h / 2
    for ch, width in zip(text, widths):
        for line in _PAD_LETTERS[ch][1]:
            _pad_stroke(d, [(x + px * h, top + py * h) for px, py in line], w, _PAD_INK)
        x += width + gap


def _unit(ax: float, ay: float, bx: float, by: float) -> tuple[float, float]:
    ln = math.hypot(bx - ax, by - ay)
    return (bx - ax) / ln, (by - ay) / ln


def _pad_polygon(d, corners, radii, color) -> None:
    """Fills a right-angled outline, listed clockwise on screen, with each
    convex corner rounded by its radius."""
    out = []
    for i, (vx, vy) in enumerate(corners):
        ein = _unit(*corners[i - 1], vx, vy)
        eout = _unit(vx, vy, *corners[(i + 1) % len(corners)])
        convex = ein[0] * eout[1] - ein[1] * eout[0] > 0
        r = radii[i] if convex else 0.0
        if r <= 0:
            out.append((vx, vy))
            continue
        ox, oy = vx - ein[0] * r + eout[0] * r, vy - ein[1] * r + eout[1] * r
        start = math.atan2(vy - ein[1] * r - oy, vx - ein[0] * r - ox)
        for k in range(9):
            ang = start + math.pi / 2 * k / 8
            out.append((ox + math.cos(ang) * r, oy + math.sin(ang) * r))
    d.polygon([(s(x), s(y)) for x, y in out], fill=(*color, 255))


def _pad_token(d, kind: str) -> None:
    """The token's outline for each kind of control."""
    if kind in ("face", "stick"):
        d.ellipse([s(3), s(3), s(45), s(45)], fill=(*_PAD_TOKEN, 255))
    elif kind == "shoulder":
        _pad_polygon(d, [(2, 10), (46, 10), (46, 38), (2, 38)], (14, 14, 14, 14), _PAD_TOKEN)
    elif kind == "trigger":
        _pad_polygon(d, [(4, 3), (44, 3), (44, 45), (4, 45)], (16, 16, 4, 4), _PAD_TOKEN)
    else:
        a, lo, hi, c = 8.5, 3.0, 45.0, 24.0
        cross = [
            (c - a, lo),
            (c + a, lo),
            (c + a, c - a),
            (hi, c - a),
            (hi, c + a),
            (c + a, c + a),
            (c + a, hi),
            (c - a, hi),
            (c - a, c + a),
            (lo, c + a),
            (lo, c - a),
            (c - a, c - a),
        ]
        _pad_polygon(d, cross, tuple(5.0 for _ in cross), _PAD_TOKEN)


def _pad_lit_arm(d, arm: str) -> None:
    """Fills one D-pad arm inside the token's edge, short of the hub."""
    lo, hi = 19.0, 29.0
    near, far, r = 17.0, 6.5, 3.0
    corners, radii = {
        "U": ([(lo, far), (hi, far), (hi, near), (lo, near)], (r, r, 0, 0)),
        "D": ([(lo, 48 - near), (hi, 48 - near), (hi, 48 - far), (lo, 48 - far)], (0, 0, r, r)),
        "L": ([(far, lo), (near, lo), (near, hi), (far, hi)], (r, 0, 0, r)),
        "R": ([(48 - near, lo), (48 - far, lo), (48 - far, hi), (48 - near, hi)], (0, r, r, 0)),
    }[arm]
    _pad_polygon(d, corners, radii, _PAD_LIT)


def _pad_knurl(d) -> None:
    """Notches a stick's edge like a thumbstick's grip."""
    for k in range(16):
        ang = math.radians(45 * (k // 2) + (11.25 if k % 2 else -11.25))
        ux, uy = math.cos(ang), math.sin(ang)
        _icon_bar(d, (24 + ux * 19, 24 + uy * 19), (24 + ux * 22, 24 + uy * 22), 2.0, (0, 0, 0, 0))


def _pad_triangle(d, inset: float, color) -> None:
    """PlayStation's triangle, shrunk about its incenter by `inset`."""
    pts = [(24.0, 12.5), (35.5, 32.5), (12.5, 32.5)]
    sides = [math.dist(pts[(i + 1) % 3], pts[(i + 2) % 3]) for i in range(3)]
    perimeter = sum(sides)
    ix = sum(side * x for side, (x, _) in zip(sides, pts)) / perimeter
    iy = sum(side * y for side, (_, y) in zip(sides, pts)) / perimeter
    (x0, y0), (x1, y1), (x2, y2) = pts
    inradius = abs((x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0)) / perimeter
    k = (inradius - inset) / inradius
    d.polygon(
        [(s(ix + (x - ix) * k), s(iy + (y - iy) * k)) for x, y in pts],
        fill=(*color, 255),
    )


def _pad_mark(d, mark: str) -> None:
    """A symbol in place of a label: PlayStation's shapes and the small
    system buttons."""
    ink, under = _PAD_INK, _PAD_TOKEN
    w = 4.5
    if mark == "cross":
        _pad_stroke(d, [(16, 16), (32, 32)], w, ink)
        _pad_stroke(d, [(32, 16), (16, 32)], w, ink)
    elif mark == "circle":
        d.ellipse([s(13.5), s(13.5), s(34.5), s(34.5)], fill=(*ink, 255))
        d.ellipse(
            [s(13.5 + w), s(13.5 + w), s(34.5 - w), s(34.5 - w)], fill=(*under, 255)
        )
    elif mark == "square":
        _icon_box(d, 14.5, 14.5, 33.5, 33.5, (*ink, 255))
        _icon_box(d, 14.5 + w, 14.5 + w, 33.5 - w, 33.5 - w, (*under, 255))
    elif mark == "triangle":
        _pad_triangle(d, 0.0, ink)
        _pad_triangle(d, w, under)
    elif mark == "view":
        for x0, y0 in ((12, 13), (20, 21)):
            _icon_box(d, x0, y0, x0 + 16, y0 + 13, (*ink, 255))
            _icon_box(d, x0 + 3.5, y0 + 3.5, x0 + 12.5, y0 + 9.5, (*under, 255))
    elif mark in ("menu", "options"):
        for y in (15.5, 24, 32.5):
            _pad_stroke(d, [(15.5, y), (32.5, y)], 4.0, ink)
    elif mark == "create":
        _pad_stroke(d, [(14.5, 25), (14.5, 33.5), (33.5, 33.5), (33.5, 25)], 4.0, ink)
        _pad_stroke(d, [(24, 28), (24, 14)], 4.0, ink)
        _pad_stroke(d, [(18, 19.5), (24, 13.5), (30, 19.5)], 4.0, ink)
    elif mark == "minus":
        _pad_stroke(d, [(15, 24), (33, 24)], 5.0, ink)
    elif mark == "plus":
        _pad_stroke(d, [(15, 24), (33, 24)], 5.0, ink)
        _pad_stroke(d, [(24, 15), (24, 33)], 5.0, ink)


_PAD_MARKS = {
    "cross",
    "circle",
    "square",
    "triangle",
    "view",
    "menu",
    "options",
    "create",
    "minus",
    "plus",
}

# (sprite suffix, token, label or mark); D-pad entries list their lit arms.
PAD_ICONS: tuple[tuple[str, str, str], ...] = (
    ("a", "face", "A"),
    ("b", "face", "B"),
    ("x", "face", "X"),
    ("y", "face", "Y"),
    ("cross", "face", "cross"),
    ("circle", "face", "circle"),
    ("square", "face", "square"),
    ("triangle", "face", "triangle"),
    ("lb", "shoulder", "LB"),
    ("rb", "shoulder", "RB"),
    ("l1", "shoulder", "L1"),
    ("r1", "shoulder", "R1"),
    ("l", "shoulder", "L"),
    ("r", "shoulder", "R"),
    ("lt", "trigger", "LT"),
    ("rt", "trigger", "RT"),
    ("l2", "trigger", "L2"),
    ("r2", "trigger", "R2"),
    ("zl", "trigger", "ZL"),
    ("zr", "trigger", "ZR"),
    ("ls", "stick", "LS"),
    ("rs", "stick", "RS"),
    ("l3", "stick", "L3"),
    ("r3", "stick", "R3"),
    ("view", "face", "view"),
    ("menu", "face", "menu"),
    ("create", "face", "create"),
    ("options", "face", "options"),
    ("minus", "face", "minus"),
    ("plus", "face", "plus"),
    ("dpad_up", "dpad", "U"),
    ("dpad_down", "dpad", "D"),
    ("dpad_left", "dpad", "L"),
    ("dpad_right", "dpad", "R"),
    ("dpad_up_down", "dpad", "UD"),
)


def pad_icon(suffix: str, kind: str, label: str) -> None:
    img, d = canvas(ICON)
    _pad_token(d, kind)
    if kind == "dpad":
        for arm in label:
            _pad_lit_arm(d, arm)
    elif label in _PAD_MARKS:
        _pad_mark(d, label)
    elif kind == "face":
        _pad_text(d, label, 24, 24, 17.0, 4.6)
    elif kind == "stick":
        _pad_knurl(d)
        _pad_text(d, label, 24, 24, 12.5, 3.8)
    elif kind == "shoulder":
        _pad_text(d, label, 24, 24, 13.5, 4.0)
    else:
        _pad_text(d, label, 24, 26.5, 13.0, 3.8)
    finish(img, ICON, f"pad_{suffix}")


def pad_icons() -> None:
    for suffix, kind, label in PAD_ICONS:
        pad_icon(suffix, kind, label)


THEMES = (
    "rusted_yard",
    "cold_circuitry",
    "quarry_dust",
    "basalt",
    "slag",
    "verdigris",
)


def generate(output: Path) -> None:
    """Generates the complete sprite directory at `output`."""
    global OUT
    OUT = output
    REGISTRY.clear()
    GROUND_CONTROLS.clear()
    OUT.mkdir(parents=True, exist_ok=True)
    print(f"writing {OUT}")
    for i in range(6):
        ground(i)
    for i in range(12):
        ground_detail(i)
    rock_skirt()
    for theme in THEMES:
        for variant in range(3):
            theme_prop(theme, variant)
    scorch()
    wreck_pile()
    wreck_piles()
    air_shadow()
    burst()
    scaffold()
    for variant in range(3):
        debris(variant)
    install_finalized_sprites(REGISTRY, OUT)
    install_finalized_environment(REGISTRY, OUT, GROUND_CONTROLS)
    install_finalized_construction(REGISTRY, OUT)
    install_maw(REGISTRY, OUT)
    install_roc(REGISTRY, OUT)
    accent_masks()
    icon_stop()
    icon_move()
    icon_run()
    icon_hunt()
    icon_advance()
    icon_attack()
    icon_patrol()
    icon_harvest()
    icon_build()
    icon_repair()
    icon_salvage()
    icon_cancel()
    icon_rally()
    icon_idle()
    icon_accompany()
    icon_retire()
    pad_icons()
    pack_atlas()
    print("done")


def _pngs_match(expected_path: Path, actual_path: Path) -> bool:
    """Compares decoded PNG identity without depending on encoder bytes."""
    try:
        with (
            Image.open(expected_path) as expected,
            Image.open(actual_path) as actual,
        ):
            return (
                expected.mode == actual.mode
                and expected.size == actual.size
                and expected.convert("RGBA").tobytes()
                == actual.convert("RGBA").tobytes()
            )
    except OSError:
        return False


def _sprite_asset_differences(
    expected_dir: Path, actual_dir: Path
) -> tuple[list[str], list[str], list[str]]:
    """Returns missing, extra, and changed generated asset names."""
    expected_files = {
        path.name: path for path in expected_dir.iterdir() if path.is_file()
    }
    actual_files = {path.name: path for path in actual_dir.iterdir() if path.is_file()}
    missing = sorted(expected_files.keys() - actual_files.keys())
    extra = sorted(actual_files.keys() - expected_files.keys())
    changed: list[str] = []
    for name in sorted(expected_files.keys() & actual_files.keys()):
        expected_path = expected_files[name]
        actual_path = actual_files[name]
        matches = (
            _pngs_match(expected_path, actual_path)
            if expected_path.suffix == ".png"
            else expected_path.read_bytes() == actual_path.read_bytes()
        )
        if not matches:
            changed.append(name)
    return missing, extra, changed


def update_committed(committed: Path) -> None:
    """Regenerates out of tree, then writes only the missing or changed
    files into `committed` and removes the ones no longer generated."""
    committed.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="aftermine-sprite-update-") as temp:
        generated = Path(temp) / "sprites"
        generate(generated)
        missing, extra, changed = _sprite_asset_differences(generated, committed)
        for name in missing + changed:
            shutil.copyfile(generated / name, committed / name)
    for name in extra:
        (committed / name).unlink()
    print(
        f"updated {committed}: {len(missing)} added, {len(changed)} changed, "
        f"{len(extra)} removed"
    )


def check_reproducible() -> None:
    """Regenerates out of tree and compares committed pixels and metadata."""
    committed = Path(__file__).resolve().parent.parent / "assets" / "sprites"
    with tempfile.TemporaryDirectory(prefix="aftermine-sprite-check-") as temp:
        generated = Path(temp) / "sprites"
        generate(generated)
        missing, extra, changed = _sprite_asset_differences(committed, generated)
        actual_count = sum(path.is_file() for path in generated.iterdir())
    if missing or extra or changed:
        raise SystemExit(
            "generated sprites differ from the committed source of truth: "
            f"missing={missing}, extra={extra}, changed={changed}"
        )
    print(f"reproducible: {actual_count} files match committed pixels and metadata")


def main() -> None:
    import argparse

    parser = argparse.ArgumentParser(description=__doc__)
    destination = parser.add_mutually_exclusive_group()
    destination.add_argument(
        "--check",
        action="store_true",
        help="regenerate and compare committed pixels and metadata",
    )
    destination.add_argument(
        "--out",
        type=Path,
        metavar="DIRECTORY",
        help="write the complete sprite bank to DIRECTORY instead of assets/sprites",
    )
    args = parser.parse_args()
    if args.check:
        check_reproducible()
    elif args.out is not None:
        generate(args.out)
    else:
        update_committed(Path(__file__).resolve().parent.parent / "assets" / "sprites")


if __name__ == "__main__":
    main()
