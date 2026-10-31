# /// script
# requires-python = ">=3.14"
# dependencies = ["pillow==12.3.0"]
# ///
"""Export the approved Aftermine artwork for desktop and iOS.

The original artwork and desktop silhouette live in production_icon_sources.
iOS uses the opaque square master; desktop exports add transparent outer corners.
Run with `uv run tools/gen_icon.py`; commit the sources and outputs together.
"""

from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "tools" / "production_icon_sources" / "aftermine.png"
DESKTOP_MASK = SOURCE.with_name("desktop_mask.png")
OUT = ROOT / "assets" / "icon"
IOS_OUT = ROOT / "ios" / "Aftermine" / "Assets.xcassets" / "AppIcon.appiconset"
MASTER_SIZE = 1024


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


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    IOS_OUT.mkdir(parents=True, exist_ok=True)
    master = render_master()
    master.save(OUT / "aftermine_1024.png")
    master.save(IOS_OUT / "aftermine_1024.png")
    desktop = desktop_icon(master)
    desktop.save(OUT / "aftermine_desktop_1024.png")
    desktop.resize((256, 256), Image.Resampling.BOX).save(OUT / "aftermine_256.png")
    for size in (16, 32, 64):
        (OUT / f"aftermine_{size}.rgba").write_bytes(
            desktop.resize((size, size), Image.Resampling.BOX).tobytes()
        )
    print("Generated desktop and iOS icons")


if __name__ == "__main__":
    main()
