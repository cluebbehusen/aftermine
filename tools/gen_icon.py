# /// script
# requires-python = ">=3.14"
# dependencies = ["pillow==12.3.0"]
# ///
"""Export the approved Aftermine artwork for desktop, iPad and Mac.

The original artwork and desktop silhouette live in production_icon_sources.
iOS uses the opaque square master; desktop and Mac exports add transparent outer
corners. Run with `uv run tools/gen_icon.py`; commit the sources and outputs
together.
"""

import json
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "tools" / "production_icon_sources" / "aftermine.png"
DESKTOP_MASK = SOURCE.with_name("desktop_mask.png")
OUT = ROOT / "assets" / "icon"
APP_ICON_OUT = ROOT / "apple" / "Assets.xcassets" / "AppIcon.appiconset"
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
    print("Generated desktop, iPad and Mac icons")


if __name__ == "__main__":
    main()
