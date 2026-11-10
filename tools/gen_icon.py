# /// script
# requires-python = ">=3.14"
# dependencies = ["pillow==12.3.0"]
# ///
"""Export the approved Aftermine artwork for desktop, iPad, Mac and Apple TV.

The original artwork and desktop silhouette live in production_icon_sources.
iOS uses the opaque square master; desktop and Mac exports add transparent outer
corners. Apple TV builds its layered icon from a 4096 upscale of the same art,
split into a sky layer and a Scuttler layer, and its Top Shelf banners from a
captured game scene with the wordmark laid over it. Run with
`uv run tools/gen_icon.py`; commit the sources and outputs together.
"""

import json
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "tools" / "production_icon_sources" / "aftermine.png"
DESKTOP_MASK = SOURCE.with_name("desktop_mask.png")
OUT = ROOT / "assets" / "icon"
APP_ICON_OUT = ROOT / "apple" / "Assets.xcassets" / "AppIcon.appiconset"
TV_FRONT = SOURCE.with_name("tv_icon_front.png")
TV_BACK = SOURCE.with_name("tv_icon_back.png")
TOP_SHELF_SCENE = SOURCE.with_name("top_shelf_scene.png")
WORDMARK = ROOT / "assets" / "wordmark.png"
TV_CATALOG_OUT = ROOT / "apple" / "AppleTV" / "Assets.xcassets"
TV_BRAND = "App Icon & Top Shelf Image"
MASTER_SIZE = 1024
# The Mac icon slots, in points; each takes a 1x and a 2x image.
MAC_SLOTS = (16, 32, 128, 256, 512)


def render_master() -> Image.Image:
    with Image.open(SOURCE) as source:
        return source.convert("RGB").resize(
            (MASTER_SIZE, MASTER_SIZE), Image.Resampling.LANCZOS
        )


def desktop_icon(master: Image.Image) -> Image.Image:
    # Preserve the approved desktop silhouette; iOS applies its own corner mask.
    with Image.open(DESKTOP_MASK) as mask:
        alpha = mask.resize(master.size, Image.Resampling.LANCZOS)
    desktop = master.convert("RGBA")
    desktop.putalpha(alpha)
    return desktop


def mac_icon_name(pixels: int) -> str:
    return f"aftermine_mac_{pixels}.png"


def app_icon_catalog() -> dict:
    images = [
        {
            "filename": "aftermine_1024.png",
            "idiom": "universal",
            "platform": "ios",
            "size": "1024x1024",
        }
    ]
    for points in MAC_SLOTS:
        for scale in (1, 2):
            images.append(
                {
                    "filename": mac_icon_name(points * scale),
                    "idiom": "mac",
                    "scale": f"{scale}x",
                    "size": f"{points}x{points}",
                }
            )
    return {"images": images, "info": {"author": "xcode", "version": 1}}


# The TV icon is a 5:3 band of the square art around the Scuttler's body,
# eyes and claws: rows [TV_ICON_TOP, TV_ICON_TOP + width * 3/5) of the master.
TV_ICON_TOP = 960
# Each icon stack: its file name, its slot in points and the scales it ships.
TV_ICON_STACKS = (
    ("App Icon", (400, 240), (1, 2)),
    ("App Icon - App Store", (1280, 768), (1,)),
)
# Each Top Shelf image: its file name, role, size in points, and where the
# wordmark's left edge sits, in points. tvOS shows the focused app's banner
# enlarged about 9% around its centre, which crops its sides, so the wordmark
# sits well inside the crop. The standard image is the wide one's middle, so
# both put the wordmark at the same spot in the scene.
TOP_SHELF_IMAGES = (
    ("Top Shelf Image", "top-shelf-image", (1920, 720), 180),
    ("Top Shelf Image Wide", "top-shelf-image-wide", (2320, 720), 380),
)
WORDMARK_WIDTH = 660
# How dark the scene gets at the left edge, behind the wordmark, fading out
# by the middle of the banner.
TOP_SHELF_SHADE = 0.35
XCODE_INFO = {"author": "xcode", "version": 1}


def write_json(path: Path, data: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + "\n")


def tv_icon_layers() -> list[tuple[str, Image.Image]]:
    """The icon's layers, front first, cropped to the TV icon's band."""
    layers = []
    for name, path, mode in (("Front", TV_FRONT, "RGBA"), ("Back", TV_BACK, "RGB")):
        with Image.open(path) as source:
            width = source.width
            band = (0, TV_ICON_TOP, width, TV_ICON_TOP + round(width * 3 / 5))
            layers.append((name, source.convert(mode).crop(band)))
    return layers


def write_tv_icon_stack(
    stack: Path,
    layers: list[tuple[str, Image.Image]],
    points: tuple[int, int],
    scales: tuple[int, ...],
) -> None:
    write_json(
        stack / "Contents.json",
        {
            "info": XCODE_INFO,
            "layers": [{"filename": f"{name}.imagestacklayer"} for name, _ in layers],
        },
    )
    for name, layer in layers:
        layer_dir = stack / f"{name}.imagestacklayer"
        imageset = layer_dir / "Content.imageset"
        imageset.mkdir(parents=True, exist_ok=True)
        write_json(layer_dir / "Contents.json", {"info": XCODE_INFO})
        images = []
        for scale in scales:
            filename = f"{name.lower()}@{scale}x.png"
            size = (points[0] * scale, points[1] * scale)
            layer.resize(size, Image.Resampling.LANCZOS).save(imageset / filename)
            images.append({"filename": filename, "idiom": "tv", "scale": f"{scale}x"})
        write_json(imageset / "Contents.json", {"images": images, "info": XCODE_INFO})


def top_shelf(points: tuple[int, int], scale: int, wordmark_left: int) -> Image.Image:
    """The captured scene cropped from its centre to `points`, darkened toward
    the left edge, with the wordmark over the dark side."""
    width, height = points[0] * scale, points[1] * scale
    with Image.open(TOP_SHELF_SCENE) as source:
        scene = source.convert("RGB")
    crop_width = round(scene.height * points[0] / points[1])
    left = (scene.width - crop_width) // 2
    image = scene.crop((left, 0, left + crop_width, scene.height)).resize(
        (width, height), Image.Resampling.LANCZOS
    )
    shade = Image.new("L", (width, 1))
    shade.putdata(
        [
            round(255 * TOP_SHELF_SHADE * max(0.0, 1 - x / (width / 2)))
            for x in range(width)
        ]
    )
    image.paste((10, 10, 15), (0, 0, width, height), shade.resize((width, height)))
    with Image.open(WORDMARK) as source:
        wordmark = source.convert("RGBA")
    mark_width = WORDMARK_WIDTH * scale
    mark = wordmark.resize(
        (mark_width, round(wordmark.height * mark_width / wordmark.width)),
        Image.Resampling.LANCZOS,
    )
    banner = image.convert("RGBA")
    banner.alpha_composite(mark, (wordmark_left * scale, (height - mark.height) // 2))
    return banner.convert("RGB")


def write_tv_catalog(out: Path) -> None:
    write_json(out / "Contents.json", {"info": XCODE_INFO})
    brand = out / f"{TV_BRAND}.brandassets"
    assets = []
    layers = tv_icon_layers()
    for name, points, scales in TV_ICON_STACKS:
        write_tv_icon_stack(brand / f"{name}.imagestack", layers, points, scales)
        assets.append(
            {
                "filename": f"{name}.imagestack",
                "idiom": "tv",
                "role": "primary-app-icon",
                "size": f"{points[0]}x{points[1]}",
            }
        )
    for name, role, points, wordmark_left in TOP_SHELF_IMAGES:
        imageset = brand / f"{name}.imageset"
        imageset.mkdir(parents=True, exist_ok=True)
        images = []
        for scale in (1, 2):
            filename = f"top_shelf_{points[0]}@{scale}x.png"
            top_shelf(points, scale, wordmark_left).save(imageset / filename)
            images.append({"filename": filename, "idiom": "tv", "scale": f"{scale}x"})
        write_json(imageset / "Contents.json", {"images": images, "info": XCODE_INFO})
        assets.append(
            {
                "filename": f"{name}.imageset",
                "idiom": "tv",
                "role": role,
                "size": f"{points[0]}x{points[1]}",
            }
        )
    write_json(brand / "Contents.json", {"assets": assets, "info": XCODE_INFO})


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    APP_ICON_OUT.mkdir(parents=True, exist_ok=True)
    master = render_master()
    master.save(OUT / "aftermine_1024.png")
    master.save(APP_ICON_OUT / "aftermine_1024.png")
    desktop = desktop_icon(master)
    # Xcode has no single-size Mac app icon, so every slot gets its own image.
    for pixels in sorted({points * scale for points in MAC_SLOTS for scale in (1, 2)}):
        image = (
            desktop
            if pixels == MASTER_SIZE
            else desktop.resize((pixels, pixels), Image.Resampling.BOX)
        )
        image.save(APP_ICON_OUT / mac_icon_name(pixels))
    (APP_ICON_OUT / "Contents.json").write_text(
        json.dumps(app_icon_catalog(), indent=2) + "\n"
    )
    desktop.save(OUT / "aftermine_desktop_1024.png")
    desktop.resize((256, 256), Image.Resampling.BOX).save(OUT / "aftermine_256.png")
    for size in (16, 32, 64):
        (OUT / f"aftermine_{size}.rgba").write_bytes(
            desktop.resize((size, size), Image.Resampling.BOX).tobytes()
        )
    write_tv_catalog(TV_CATALOG_OUT)
    print("Generated desktop, iPad, Mac and Apple TV icons")


if __name__ == "__main__":
    main()
