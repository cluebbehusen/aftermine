"""The installer for Aftermine's finalized machine art.

Each bank writes complete gameplay-sized RGBA rows into the sprite
generator's registry. Review cards and their presentation framing are not
runtime inputs.
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image

from tools.production_sprite_sources import (
    installed_defenses_final,
    lattice_final,
    mechanical_final,
    piledriver_final,
    mantle_final,
    specialists_final,
    spire_final,
    surveyor_final,
)

Registry = dict[str, Image.Image]


def install_finalized_sprites(registry: Registry, out: Path) -> None:
    """Install the finalized machine art and its action frames.

    Call this after the ground and environment rows exist and before
    construction frames, allegiance masks and atlas packing. No external
    presentation asset is read.
    """
    out.mkdir(parents=True, exist_ok=True)
    specialists_final.install_specialists(registry, out)
    installed_defenses_final.install_defenses(registry, out)
    mechanical_final.install_machines(registry, out)
    piledriver_final.install_piledriver(registry, out)
    mantle_final.install_mantle(registry, out)
    spire_final.install_spire(registry, out)
    lattice_final.install_lattice(registry, out)
    surveyor_final.install_surveyor(registry, out)
