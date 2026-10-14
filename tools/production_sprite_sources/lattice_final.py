"""Interceptor battery: two independently swiveling launch pods on one base.

Each pod turns on its own bearing and carries one interceptor, held by a
split clamp that closes once the round is complete and opens to fire. A
single telescopic arm in the central housing rebuilds the pods serially:
it lifts three fixed-size parts out of the housing, turns them upright and
seats them in a pod docked north, nose last, then returns under the cover.

The base, arm and cover share the 128px footprint canvas. Pod layers are
64px canvases centered on their pod's pivot, (32, 62) or (96, 62) on the
footprint canvas; launch layers are 128px canvases centered on the same
pivot. North-facing art is heading zero; positive headings turn clockwise
on screen.
"""

from __future__ import annotations

import functools
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
    SS,
    STEEL,
    VOID,
    box,
    canvas,
    finish,
    line,
    panel,
    vent,
)
from tools.production_sprite_sources.spire_final import (
    brace,
    disk,
    exhaust,
    missile,
    smooth,
)

Registry = dict[str, Image.Image]
Color = tuple[int, int, int]

POD_PIVOTS = ((32, 62), (96, 62))
POD_SIZE = 64
LAUNCH_SIZE = 128
PARTS = 3
PART_BANDS = ((76, 90), (62, 76), (34, 62))
PART_TARGETS = (83, 69, 48)
ARM_STEPS = 4
ARM_RETURN_STEPS = 2
ARM_FRAMES = PARTS * ARM_STEPS + ARM_RETURN_STEPS
CLAMP_STEPS = 4
EXHAUST_FRAMES = 3
ARM_ROOT = (64.0, 104.0)
PART_SOURCE = (64.0, 103.0)


def _foundation(d: ImageDraw.ImageDraw, paint: Color) -> None:
    hull = [(64, 12), (113, 40), (108, 104), (64, 117), (20, 104), (15, 40)]
    panel(d, hull, DARK)
    cx = sum(x for x, _ in hull) / len(hull)
    cy = sum(y for _, y in hull) / len(hull)
    panel(d, [(cx + (x - cx) * 0.73, cy + (y - cy) * 0.73) for x, y in hull], DARK)
    for x, y in ((27, 32), (101, 32), (27, 103), (101, 103)):
        plate(d, (x - 5, y - 6, x + 5, y + 6), IRON, 2)
        bolt(d, x, y)
    for x in (20, 102):
        box(d, (x, 52, x + 6, 82), paint, 1)


def _bearing(d: ImageDraw.ImageDraw, x: float, y: float, r: float) -> None:
    disk(d, x, y, r, VOID)
    disk(d, x, y, r - 2, IRON)
    disk(d, x, y, r - 5, DEEP)
    for angle in range(0, 360, 45):
        a = math.radians(angle)
        bolt(d, x + math.cos(a) * (r - 4), y + math.sin(a) * (r - 4))


def _base(d: ImageDraw.ImageDraw, paint: Color) -> None:
    _foundation(d, paint)
    for x, y in POD_PIVOTS:
        brace(d, (64, 76), (x, y), 8)
        _bearing(d, x, y, 24)


def _pod(d: ImageDraw.ImageDraw, x: float, paint: Color) -> None:
    plate(d, (x - 12, 34, x + 12, 90), VOID, 5)
    plate(d, (x - 9, 37, x + 9, 87), DEEP, 3)
    for sign in (-1, 1):
        xx = x + sign * 10
        line(d, [(xx, 45), (xx, 82)], EDGE, 1.5)
        for yy in (51, 73):
            box(d, (xx - 2, yy - 2, xx + 2, yy + 2), IRON, 1)
    plate(d, (x - 12, 33, x + 12, 43), DARK, 4)
    box(d, (x - 8, 36, x + 8, 39), paint, 1)


def _interceptor(d: ImageDraw.ImageDraw, x: float, paint: Color) -> None:
    missile(d, x, 88, paint, 6, 41)


@functools.lru_cache(maxsize=None)
def _parts(paint: Color) -> tuple[Image.Image, ...]:
    image, d = canvas()
    _interceptor(d, 64, paint)
    return tuple(
        image.crop((46 * SS, top * SS, 82 * SS, bottom * SS))
        for top, bottom in PART_BANDS
    )


def _seat(image: Image.Image, index: int, x: float, paint: Color) -> None:
    top, bottom = PART_BANDS[index]
    y = PART_TARGETS[index]
    image.alpha_composite(
        _parts(paint)[index],
        (round((x - 18) * SS), round((y - (bottom - top) / 2) * SS)),
    )


def _clamp(d: ImageDraw.ImageDraw, x: float, closed: float) -> None:
    for side in (-1, 1):
        root = x + side * 10
        jaw = x + side * (8 - 4 * closed)
        line(d, [(root, 66), (jaw, 66)], VOID, 5)
        line(d, [(root, 66), (jaw, 66)], STEEL, 2)
        plate(d, (jaw - 1.5, 61, jaw + 1.5, 71), IRON, 1)
        line(d, [(jaw, 63), (jaw, 69)], EDGE, 1)
        bolt(d, root, 66)


def _lerp(a, b, t: float) -> tuple[float, float]:
    return (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)


def _grip(center, angle: float, side: int) -> tuple[float, float]:
    a = math.radians(angle)
    dx = side * 7
    dy = 7
    return (
        center[0] + math.cos(a) * dx + math.sin(a) * dy,
        center[1] - math.sin(a) * dx + math.cos(a) * dy,
    )


def arm_pose(slot: int, frame: int) -> dict:
    """The arm's hand, the carried part if any, and the parts already
    seated in the served pod, for one arm frame. Frames 1 to
    `PARTS * ARM_STEPS` carry the parts in order; the rest return the
    empty hand to its root under the cover."""
    if slot not in range(2) or frame not in range(1, ARM_FRAMES + 1):
        raise ValueError("invalid Lattice arm frame")
    x = POD_PIVOTS[slot][0]
    side = 1 if slot == 0 else -1
    pickup = _grip(PART_SOURCE, 90, side)
    step = frame - 1
    if step >= PARTS * ARM_STEPS:
        u = smooth((step - PARTS * ARM_STEPS) / ARM_RETURN_STEPS)
        start = _grip((x, PART_TARGETS[-1]), 0, side)
        return dict(x=x, seated=PARTS, hand=_lerp(start, ARM_ROOT, u), moving=None)
    count, j = divmod(step, ARM_STEPS)
    phase = j / ARM_STEPS
    if phase < 0.22:
        start = _grip((x, PART_TARGETS[count - 1]), 0, side) if count else ARM_ROOT
        return dict(
            x=x,
            seated=count,
            hand=_lerp(start, pickup, smooth(phase / 0.22)),
            moving=None,
        )
    travel = smooth((phase - 0.22) / 0.60)
    center = _lerp(PART_SOURCE, (x, PART_TARGETS[count]), travel)
    angle = 90 * (1 - smooth((travel - 0.10) / 0.70))
    return dict(
        x=x,
        seated=count,
        hand=_grip(center, angle, side),
        moving=(count, center, angle),
    )


NOSE_FRAMES = tuple(
    frame
    for frame in range(1, ARM_FRAMES + 1)
    if (moving := arm_pose(0, frame)["moving"]) is not None and moving[0] == PARTS - 1
)


def _telescopic(d: ImageDraw.ImageDraw, a, b, width: float) -> None:
    line(d, [a, b], VOID, width + 3)
    line(d, [a, b], IRON, width)
    sleeve = _lerp(a, b, 0.54)
    line(d, [a, sleeve], DARK, width)
    line(d, [sleeve, b], STEEL, max(1, width - 2))
    for x, y in (a, b):
        disk(d, x, y, 2.4, VOID)
        disk(d, x, y, 1.2, STEEL)


def _carried(image: Image.Image, pose: dict, paint: Color, nose: bool) -> None:
    if pose["moving"] is None:
        return
    index, center, angle = pose["moving"]
    if (index == PARTS - 1) != nose:
        return
    part = _parts(paint)[index].rotate(angle, Image.Resampling.BICUBIC, expand=True)
    image.alpha_composite(
        part,
        (
            round(center[0] * SS - part.width / 2),
            round(center[1] * SS - part.height / 2),
        ),
    )


def _arm(image: Image.Image, pose: dict) -> None:
    d = ImageDraw.Draw(image)
    hand = pose["hand"]
    root = ARM_ROOT
    extension = min(1, math.dist(root, hand) / 40)
    elbow = (
        root[0] + (hand[0] - root[0]) * 0.28,
        root[1] + (hand[1] - root[1]) * 0.46 - 8 * extension,
    )
    _telescopic(d, root, elbow, 5)
    _telescopic(d, elbow, hand, 3)
    line(d, [(hand[0] - 2, hand[1] + 2), hand, (hand[0] + 2, hand[1] - 1)], BRASS, 2)


def _cover(d: ImageDraw.ImageDraw) -> None:
    plate(d, (48, 92, 80, 115), DARK, 4)
    vent(d, 54, 96, 20)
    for x in (28, 100):
        bolt(d, x, 107)


def _pod_image(
    slot: int, load: str, seated: int, clamp: float, paint: Color
) -> Image.Image:
    """One pod at heading zero on the footprint canvas: its structure, its
    loaded round or seated parts, and its clamp."""
    x = POD_PIVOTS[slot][0]
    image, d = canvas()
    _pod(d, x, paint)
    if load == "loaded":
        _interceptor(d, x, paint)
    else:
        for index in range(seated):
            _seat(image, index, x, paint)
    _clamp(d, x, clamp)
    return image


def _rotate(image: Image.Image, heading: float, pivot) -> Image.Image:
    return image.rotate(
        -(heading % 360),
        Image.Resampling.BICUBIC,
        center=(pivot[0] * SS, pivot[1] * SS),
    )


def _paint(variant: str) -> Color:
    if variant not in gen.PALETTES:
        raise ValueError(f"unknown variant: {variant}")
    return gen.PALETTES[variant]["base"]


def _downsample(image: Image.Image) -> Image.Image:
    return image.resize(
        (image.width // SS, image.height // SS), Image.Resampling.LANCZOS
    )


def _pivot_crop(image: Image.Image, slot: int, size: int) -> Image.Image:
    """A downsampled footprint layer cropped square about a pod pivot."""
    x, y = POD_PIVOTS[slot]
    half = size // 2
    return image.crop((x - half, y - half, x + half, y + half))


def render_lattice(
    variant: str,
    headings: tuple[float, float] = (0.0, 0.0),
    loads: tuple[str, str] = ("empty", "empty"),
    seated: tuple[int, int] = (0, 0),
    clamps: tuple[float, float] = (0.0, 0.0),
    arm: tuple[int, int] | None = None,
) -> Image.Image:
    """The complete battery, composed like the approved review frames.
    `loads` holds "empty", "loaded" or "partial" per pod; `arm` is the
    served slot and arm frame of a rebuild, if one is under way."""
    paint = _paint(variant)
    base, d = canvas()
    _base(d, paint)
    out = finish(base)
    for slot in range(2):
        if loads[slot] not in ("empty", "loaded", "partial"):
            raise ValueError("invalid Lattice pod load")
        pod = _pod_image(slot, loads[slot], seated[slot], clamps[slot], paint)
        out.alpha_composite(_downsample(_rotate(pod, headings[slot], POD_PIVOTS[slot])))
    if arm is not None:
        pose = arm_pose(*arm)
        moving, _ = canvas()
        _carried(moving, pose, paint, nose=False)
        _carried(moving, pose, paint, nose=True)
        _arm(moving, pose)
        out.alpha_composite(_downsample(moving))
    cover, d = canvas()
    _cover(d)
    out.alpha_composite(_downsample(cover))
    return out


def render_base() -> Image.Image:
    """The fixed base; its painted side bars sit under the bearings."""
    image, d = canvas()
    _base(d, _paint("base"))
    return finish(image)


def _pod_layer(variant: str, draw) -> Image.Image:
    paint = _paint(variant)
    image, d = canvas()
    draw(image, d, POD_PIVOTS[0][0], paint)
    return _pivot_crop(_downsample(image), 0, POD_SIZE)


def render_pod(variant: str) -> Image.Image:
    return _pod_layer(variant, lambda image, d, x, paint: _pod(d, x, paint))


def render_interceptor(variant: str) -> Image.Image:
    return _pod_layer(variant, lambda image, d, x, paint: _interceptor(d, x, paint))


def render_stack(count: int) -> Image.Image:
    """Seated parts short of a complete interceptor; they carry no paint."""
    if count not in range(1, PARTS):
        raise ValueError("invalid Lattice stack")

    def draw(image, d, x, paint):
        for index in range(count):
            _seat(image, index, x, paint)

    return _pod_layer("base", draw)


def render_clamp(step: int) -> Image.Image:
    if step not in range(CLAMP_STEPS + 1):
        raise ValueError("invalid Lattice clamp step")
    return _pod_layer(
        "base", lambda image, d, x, paint: _clamp(d, x, step / CLAMP_STEPS)
    )


def render_launch(variant: str, flicker: int | None) -> Image.Image:
    """The launched interceptor and its plume on a pivot-centered canvas;
    `None` draws it without a plume."""
    if flicker is not None and flicker not in range(EXHAUST_FRAMES):
        raise ValueError("invalid Lattice exhaust frame")
    paint = _paint(variant)
    x = POD_PIVOTS[0][0]
    image, d = canvas()
    _interceptor(d, x, paint)
    if flicker is not None:
        exhaust(d, x, 89, flicker)
    return _pivot_crop(_downsample(image), 0, LAUNCH_SIZE)


def render_arm(slot: int, frame: int) -> Image.Image:
    """The telescopic arm and any plain part it carries."""
    pose = arm_pose(slot, frame)
    image, _ = canvas()
    _carried(image, pose, _paint("base"), nose=False)
    _arm(image, pose)
    return _downsample(image)


def render_nose(variant: str, slot: int, frame: int) -> Image.Image:
    """The painted nose part while the arm carries it, drawn beneath the
    arm frame of the same index."""
    if frame not in NOSE_FRAMES:
        raise ValueError("the arm does not carry the nose in this frame")
    image, _ = canvas()
    _carried(image, arm_pose(slot, frame), _paint(variant), nose=True)
    return _downsample(image)


def render_cover() -> Image.Image:
    image, d = canvas()
    _cover(d)
    return _downsample(image)


def _save(registry: Registry, out: Path, name: str, image: Image.Image) -> None:
    gen.save_sprite(image, out, name)
    registry[name] = image


def install_lattice(registry: Registry, out: Path) -> None:
    """Installs the empty hull and every runtime layer of the Lattice.

    Layers that carry paint ship an allegiance-accent row; the base, cover,
    arm, plain parts and clamps never do."""
    sides = ("left", "right")
    for variant in gen.PALETTES:
        tag = gen.variant_tag(variant)
        _save(registry, out, f"lattice{tag}", render_lattice(variant))
        _save(registry, out, f"rig_lattice_pod{tag}", render_pod(variant))
        _save(
            registry, out, f"rig_lattice_interceptor{tag}", render_interceptor(variant)
        )
        _save(registry, out, f"rig_lattice_launch{tag}", render_launch(variant, None))
        for flicker in range(EXHAUST_FRAMES):
            _save(
                registry,
                out,
                f"rig_lattice_launch{tag}_{flicker + 1}",
                render_launch(variant, flicker),
            )
        for slot, side in enumerate(sides):
            for index, frame in enumerate(NOSE_FRAMES, 1):
                _save(
                    registry,
                    out,
                    f"rig_lattice_nose_{side}{tag}_{index}",
                    render_nose(variant, slot, frame),
                )
    _save(registry, out, "rig_lattice_base", render_base())
    _save(registry, out, "rig_lattice_cover", render_cover())
    for count in range(1, PARTS):
        _save(registry, out, f"rig_lattice_stack_{count}", render_stack(count))
    _save(registry, out, "rig_lattice_clamp", render_clamp(0))
    for step in range(1, CLAMP_STEPS + 1):
        _save(registry, out, f"rig_lattice_clamp_{step}", render_clamp(step))
    for slot, side in enumerate(sides):
        for frame in range(1, ARM_FRAMES + 1):
            _save(
                registry,
                out,
                f"rig_lattice_arm_{side}_{frame}",
                render_arm(slot, frame),
            )
