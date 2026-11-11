"""The installer for the quarry environment: the finished ground, scrap and
Extractor frame over the generator's plain ground, then the props.
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image

from tools.production_sprite_sources import quarry_final
from tools.production_sprite_sources.mechanical_final import install_props

Registry = dict[str, Image.Image]


def install_finalized_environment(
    registry: Registry, out: Path, ground_controls: Registry
) -> None:
    """Install every approved environment sprite into the production bank,
    painting the finished ground over the plain `ground_controls`."""

    quarry_final.install_quarry(registry, out, ground_controls)
    install_props(registry, out)
