"""Install articulated machines and quarry props from their authored geometry."""

from pathlib import Path

from PIL import Image

from tools import sprite_kit as kit
from tools.production_sprite_sources import (
    mechanical_aircraft as aircraft,
)
from tools.production_sprite_sources import (
    mechanical_bombers as bombers,
)
from tools.production_sprite_sources import (
    mechanical_buildings as buildings,
)
from tools.production_sprite_sources import (
    mechanical_buzzard as buzzard,
)
from tools.production_sprite_sources import (
    mechanical_economy as economy,
)
from tools.production_sprite_sources import (
    mechanical_extractor as extractor,
)
from tools.production_sprite_sources import (
    mechanical_ground as ground,
)
from tools.production_sprite_sources import (
    mechanical_workers as workers,
)
from tools.production_sprite_sources import (
    quarry_industrial,
    quarry_props,
    quarry_rocks,
)


def install_machines(registry: dict[str, Image.Image], out: Path) -> None:
    """Install idle, movement, action and independently articulated layers."""

    def put(key, image):
        kit.save_sprite(image, out, key)
        registry[key] = image

    for variant in kit.PALETTES:
        tag = kit.variant_tag(variant)
        for kind in (
            "sentinel",
            "warden",
            "lancer",
            "breaker",
            "avalanche",
        ):
            count = 6 if kind == "lancer" else 4
            rigged = kind in ("sentinel", "warden", "lancer")
            # A rig draws its hull and mount in place of whole-body frames,
            # and a tracked hull holds still while its belts roll.
            actions = (
                [] if rigged else [(f"_action{i}", 0, i) for i in range(1, count + 1)]
            )
            for suffix, move, action in [("", 0, 0)] + actions:
                if kind == "breaker":
                    image = ground.render_breaker(variant, move, action)
                elif kind == "avalanche":
                    image = economy.render_avalanche(variant, move, action)
                else:
                    image = ground.render(kind, variant, move, action)
                put(f"{kind}{tag}{suffix}", image)
            if rigged:
                put(f"rig_{kind}_hull{tag}", ground.hull(kind, variant, 0))
                for action in range(count + 1):
                    suffix = "" if action == 0 else f"_action{action}"
                    put(
                        f"rig_{kind}_mount{tag}{suffix}",
                        ground.mount(kind, variant, action),
                    )

        # Workers draw their body layers in play; the whole-body rest frame
        # is their portrait.
        put(f"harvester{tag}", workers.harvester(variant, 0, 0, 0))
        for cargo in range(6):
            put(
                f"rig_harvester_body{tag}_cargo{cargo}",
                workers.harvester(variant, cargo=cargo, body_only=True),
            )
        # The Excavator's tracks roll beneath a still body.
        for kind, phases in (("excavator", 1), ("scuttler", 3)):
            for phase in range(phases):
                suffix = "" if phase == 0 else f"_move{phase}"
                put(
                    f"rig_{kind}_body{tag}{suffix}",
                    getattr(workers, kind)(variant, move=phase, body_only=True),
                )
            put(f"{kind}{tag}", getattr(workers, kind)(variant, 0, 0))
        for suffix, move, action in [
            ("", 0, 0),
            ("_move1", 1, 0),
            ("_move2", 2, 0),
        ] + [(f"_action{i}", 0, i) for i in range(1, 4)]:
            put(f"sapper{tag}{suffix}", workers.sapper(variant, move, action))
        for suffix, action in [("", 0), ("_move1", 0), ("_move2", 0)] + [
            (f"_action{i}", i) for i in range(1, 5)
        ]:
            put(
                f"condor{tag}{suffix}",
                bombers.render_condor(variant, bombers.CONDOR_STATES[action]),
            )
        for kind in ("buzzard", "talon", "skyhook"):
            frames = [("", 0, 0)]
            if kind == "talon":
                frames += [("_move1", 1, 0), ("_move2", 2, 0)]
                frames += [(f"_action{i}", 0, i) for i in range(1, 5)]
            for suffix, move, action in frames:
                render = (
                    buzzard.buzzard if kind == "buzzard" else getattr(aircraft, kind)
                )
                put(f"{kind}{tag}{suffix}", render(variant, move, action))
        for phase in range(3):
            suffix = "" if phase == 0 else f"_move{phase}"
            put(f"rig_buzzard_hull{tag}{suffix}", buzzard.hull(variant, phase))
        for action in range(5):
            suffix = "" if action == 0 else f"_action{action}"
            put(f"rig_buzzard_mount{tag}{suffix}", buzzard.mount(variant, action))

        for phase in range(3):
            suffix = "" if phase == 0 else f"_move{phase}"
            put(
                f"rig_skyhook_hull{tag}{suffix}",
                aircraft.skyhook(variant, phase, part="hull"),
            )
        for action in range(5):
            suffix = "" if action == 0 else f"_action{action}"
            put(
                f"rig_skyhook_mount{tag}{suffix}",
                aircraft.skyhook(variant, action=action, part="mount"),
            )

        for kind, count in (
            ("foundry", 16),
            ("fabricator", 4),
            ("airworks", 4),
            ("crucible", 4),
            ("repair_bay", 4),
            ("array", 0),
            ("reclaimer", economy.RECLAIMER_WORK_FRAMES),
            ("reclaimer_t1", economy.RECLAIMER_WORK_FRAMES),
        ):
            render = {
                "reclaimer": economy.render_reclaimer,
                "reclaimer_t1": economy.render_refinery,
            }.get(kind)
            if render is None:
                render = getattr(buildings, kind)
            for phase in range(count + 1):
                put(
                    f"{kind}{tag}" + (f"_work{phase}" if phase else ""),
                    render(variant, phase),
                )
        for kind, count, render in (
            ("extractor", extractor.EXTRACTOR_WORK_FRAMES, extractor.render_extractor),
            (
                "extractor_t1",
                extractor.DEEP_BORE_WORK_FRAMES,
                extractor.render_deep_bore,
            ),
        ):
            put(f"{kind}{tag}", render(variant, 0))
            for frame in range(count):
                put(f"{kind}{tag}_work{frame + 1}", render(variant, frame * 4 / count))
        put(f"array_t1{tag}", buildings.array(variant, 0, 1))
        for tier in range(2):
            for part in ("base", "rotor"):
                put(
                    f"rig_array_t{tier}_{part}{tag}",
                    buildings.array(variant, tier=tier, part=part),
                )
    for level in range(5):
        put(f"excavator_cargo{level}", workers.excavator_cargo(level))


def install_props(registry: dict[str, Image.Image], out: Path) -> None:
    """Install static ground blockers and low, passable wreckage."""
    for module in (quarry_props, quarry_industrial, quarry_rocks):
        for key, image in module.frames():
            image.save(out / f"{key}.png")
            registry[key] = image
