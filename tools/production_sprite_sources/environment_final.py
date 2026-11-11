"""The installer for the quarry environment: the finished ground, scrap and
Extractor frame over the generator's plain ground, then the props.
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image

from tools import gen_sprites as gen
from tools.production_sprite_sources import quarry_final

Registry = dict[str, Image.Image]


def install_finalized_environment(registry: Registry, out: Path) -> None:
    """Install every approved environment sprite into the production bank."""

    quarry_final.install_quarry(registry, out, gen.GROUND_CONTROLS)
    from tools.production_sprite_sources.mechanical_final import install_props

    install_props(registry, out)
