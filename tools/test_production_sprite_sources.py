import hashlib
import tempfile
import unittest
from itertools import pairwise
from pathlib import Path

from PIL import Image, ImageChops, ImageStat

from tools import gen_sprites as gen
from tools.production_sprite_sources import (
    air_final,
    air_support_final,
    airworks_scouts_final,
    construction_final,
    environment_final,
    excavator_final,
    finalized,
    installed_defenses_final,
    lattice_final,
    mantle_final,
    maw_final,
    mechanical_extractor,
    mechanical_final,
    piledriver_final,
    quarry_final,
    quarry_rocks,
    roc_final,
    skyhook_sapper_crucible_final,
    specialists_final,
    spire_final,
    surveyor_final,
    tier_one_combat_final,
)


def _changed_pixels(left: Image.Image, right: Image.Image) -> int:
    difference = ImageChops.difference(left.convert("RGBA"), right.convert("RGBA"))
    return sum(pixel != (0, 0, 0, 0) for pixel in difference.get_flattened_data())


def _alpha_centroid_y(image: Image.Image, box: tuple[int, int, int, int]) -> float:
    alpha = image.getchannel("A")
    x0, y0, x1, y1 = box
    total = 0
    weighted = 0
    for y in range(y0, y1):
        for x in range(x0, x1):
            value = alpha.getpixel((x, y))
            total += value
            weighted += y * value
    if total == 0:
        raise AssertionError("centroid region is empty")
    return weighted / total


class ProductionSpriteSourceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.temp = tempfile.TemporaryDirectory(prefix="oxide-production-sprites-")
        cls.out = Path(cls.temp.name)
        cls.registry: dict[str, Image.Image] = {}
        old_out = gen.OUT
        old_registry = gen.REGISTRY
        try:
            gen.OUT = cls.out
            gen.REGISTRY = cls.registry
            for phase in range(6):
                gen.ground(phase)
            cls.ground_controls = cls.registry.copy()
            for variant in gen.PALETTES:
                gen.harvester(variant)
                gen.harvester(variant, dig=1)
                gen.harvester(variant, dig=2)
                gen.barricade(variant)
                gen.scuttle_charge(variant)
            finalized.install_finalized_sprites(cls.registry, cls.out)
            construction_final.install_finalized_construction(cls.registry, cls.out)
            environment_final.install_finalized_environment(cls.registry, cls.out)
        finally:
            gen.OUT = old_out
            gen.REGISTRY = old_registry

    @classmethod
    def tearDownClass(cls) -> None:
        cls.temp.cleanup()

    def test_surveyor_preserves_deployed_art_and_scanner_seam(self) -> None:
        digest = hashlib.sha256()
        for variant in gen.PALETTES:
            frames = [surveyor_final.render_surveyor(variant, move=i) for i in range(3)]
            frames += [
                surveyor_final.render_surveyor(variant, deploy=i / 40)
                for i in range(1, 41)
            ]
            frames += [
                surveyor_final.render_surveyor(variant, deploy=1, sweep=i)
                for i in range(80)
            ]
            for frame in frames[43:]:
                digest.update(frame.tobytes())
            self.assertEqual(frames[42].tobytes(), frames[43].tobytes())
            self.assertEqual(
                frames[43].tobytes(),
                surveyor_final.render_surveyor(variant, deploy=1, sweep=80).tobytes(),
            )
            tag = gen.variant_tag(variant)
            self.assertNotIn(f"surveyor{tag}_action1", self.registry)
        mounts = [
            key for key in self.registry if key.startswith("rig_surveyor_mount_action")
        ]
        self.assertEqual(len(mounts), 120)
        self.assertEqual(
            digest.hexdigest(),
            "000e91b69f35066f88c1e67685df75a731d5463892d2cb166986cc9106a7a920",
        )

    def test_installer_is_independent_of_review_files(self) -> None:
        root = Path(finalized.__file__).parent
        production_sources = "\n".join(
            path.read_text() for path in sorted(root.glob("*.py"))
        )
        self.assertNotIn("art-direction-review", production_sources)
        self.assertNotIn("from tools.batch", production_sources)
        self.assertNotIn("from tools import batch", production_sources)
        self.assertNotIn("gen._review", production_sources)
        self.assertNotIn("gen.REVIEW_ROUTE", production_sources)

    def test_piledriver_preserves_approved_body_and_reload_closes(self) -> None:
        digest = hashlib.sha256()
        for variant in gen.PALETTES:
            for deployment in (0, 0.25, 0.5, 0.75, 1):
                digest.update(
                    piledriver_final.render_body(variant, deployment).tobytes()
                )
            self.assertEqual(
                piledriver_final.render_gun(variant).tobytes(),
                piledriver_final.render_gun(variant, 99).tobytes(),
                "the loader finishes at its ready pose",
            )
            layers = [
                (piledriver_final.render_gun(variant, age), piledriver_final.MOUNT_CROP)
                for age in range(piledriver_final.RELOAD_FRAMES)
            ] + [
                (
                    piledriver_final.render_anchors(
                        variant, step / piledriver_final.ANCHOR_STEPS
                    ),
                    piledriver_final.ANCHOR_CROP,
                )
                for step in range(piledriver_final.ANCHOR_STEPS + 1)
            ]
            for image, crop in layers:
                restored = Image.new("RGBA", image.size)
                restored.paste(image.crop(crop), crop[:2])
                self.assertEqual(image.tobytes(), restored.tobytes())
            tag = gen.variant_tag(variant)
            self.assertFalse(
                any(key.startswith(f"piledriver{tag}_action") for key in self.registry)
            )
            for age in range(piledriver_final.RELOAD_FRAMES):
                self.assertEqual(
                    self.registry[
                        f"rig_piledriver_mount{tag}_action{age + 1}"
                    ].tobytes(),
                    piledriver_final.render_gun(variant, age)
                    .crop(piledriver_final.MOUNT_CROP)
                    .tobytes(),
                )
        self.assertEqual(
            digest.hexdigest(),
            "903d5284ca950be6a890bf91865f603e5e4c3176206765f956060ddee1cdffb8",
        )

    def test_articulated_machine_bank_matches_approved_pixels(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            frames = {}
            mechanical_final.install_machines(frames, Path(directory))
        self.assertEqual(len(frames), 629)
        digest = hashlib.sha256()
        for key, image in sorted(frames.items()):
            self.assertEqual(self.registry[key].tobytes(), image.tobytes(), key)
            digest.update(key.encode())
            digest.update(image.tobytes())
        self.assertEqual(
            digest.hexdigest(),
            "94c29027fb67851009bffccdbb831ce143cc7bfa626283de5c84c30fe6ed0c4a",
        )

    def test_extractor_key_poses_keep_the_approved_art(self) -> None:
        digest = hashlib.sha256()
        for render in (
            mechanical_extractor.render_extractor,
            mechanical_extractor.render_deep_bore,
        ):
            for phase in range(4):
                digest.update(render("base", phase).tobytes())
        self.assertEqual(
            digest.hexdigest(),
            "7f24e0a962f69e774773d8a5d5b140552632dd79dfd8b6294e68efe253172729",
        )

    def test_extractor_work_loops_close_without_a_return_jump(self) -> None:
        for stem, count, render in (
            ("extractor", 16, mechanical_extractor.render_extractor),
            ("extractor_t1", 48, mechanical_extractor.render_deep_bore),
        ):
            for variant in gen.PALETTES:
                tag = gen.variant_tag(variant)
                frames = [
                    self.registry[f"{stem}{tag}_work{i + 1}"] for i in range(count)
                ]
                self.assertEqual(
                    frames[0].tobytes(), self.registry[f"{stem}{tag}"].tobytes()
                )
                self.assertEqual(frames[0].tobytes(), render(variant, 4).tobytes())
                self.assertEqual(len({frame.tobytes() for frame in frames}), count)
                steps = [_changed_pixels(a, b) for a, b in pairwise(frames)]
                self.assertLessEqual(_changed_pixels(frames[-1], frames[0]), max(steps))
                # Anchors remain planted throughout the machinery cycle.
                for frame in frames[1:]:
                    self.assertEqual(
                        frame.crop((5, 12, 22, 29)).tobytes(),
                        frames[0].crop((5, 12, 22, 29)).tobytes(),
                    )
        frames = [self.registry[f"extractor_t1_work{i + 1}"] for i in range(48)]
        panel_y = []
        for frame in frames:
            positions = [
                y
                for y in range(28, 53)
                if frame.getpixel((64, y))[:3] == gen.PALETTES["base"]["dark"]
            ]
            self.assertTrue(positions)
            panel_y.append(sum(positions) / len(positions))
        self.assertGreater(max(panel_y) - min(panel_y), 4)
        self.assertLessEqual(
            max(abs(a - b) for a, b in pairwise(panel_y + panel_y[:1])), 1
        )

    def test_skyhook_rotors_keep_three_distinct_poses_during_actions(self) -> None:
        bounds = (6, 13, 42, 52)
        for variant in gen.PALETTES:
            tag = gen.variant_tag(variant)
            rotors = []
            for suffix in ("", "_move1", "_move2"):
                hull = self.registry[f"rig_skyhook_hull{tag}{suffix}"]
                resting = hull.copy()
                resting.alpha_composite(self.registry[f"rig_skyhook_mount{tag}"])
                rotor = resting.crop(bounds).tobytes()
                rotors.append(rotor)
                for action in ("", "_action1", "_action2", "_action3", "_action4"):
                    combined = hull.copy()
                    combined.alpha_composite(
                        self.registry[f"rig_skyhook_mount{tag}{action}"]
                    )
                    self.assertEqual(
                        combined.crop(bounds).tobytes(),
                        rotor,
                        (variant, suffix, action),
                    )
            self.assertEqual(len(set(rotors)), 3, variant)

    def test_sapper_preparation_visibly_advances_through_all_poses(self) -> None:
        for variant in gen.PALETTES:
            poses = [
                self.registry[f"sapper{gen.variant_tag(variant)}_action{phase}"]
                for phase in range(1, 4)
            ]
            self.assertTrue(
                all(
                    _changed_pixels(before, after) > 2
                    for before, after in pairwise(poses)
                )
            )

    def test_reclaimer_rollers_advance_evenly_across_the_loop_seam(self) -> None:
        for stem in ("reclaimer", "reclaimer_t1"):
            for variant in gen.PALETTES:
                frames = [
                    self.registry[f"{stem}{gen.variant_tag(variant)}_work{phase}"]
                    for phase in range(1, 13)
                ]
                for phase, (before, after) in enumerate(
                    zip(frames, frames[1:] + frames[:1], strict=True)
                ):
                    with self.subTest(stem=stem, variant=variant, phase=phase):
                        self.assertEqual(
                            before.crop((54, 54, 73, 59)).tobytes(),
                            after.crop((55, 54, 74, 59)).tobytes(),
                        )
                        self.assertEqual(
                            before.crop((55, 72, 74, 77)).tobytes(),
                            after.crop((54, 72, 73, 77)).tobytes(),
                        )

    def test_promoted_quarry_matches_approved_pixels(self) -> None:
        digest = hashlib.sha256()
        for key, image in sorted(quarry_final.source_frames(self.ground_controls)):
            self.assertEqual(self.registry[key].tobytes(), image.tobytes(), key)
            digest.update(key.encode())
            digest.update(image.tobytes())
        self.assertEqual(
            digest.hexdigest(),
            "70683cfbc918b9c02747e50873e87e89af8a669ffff11fc2d8287819c167c645",
        )

    def test_promoted_specialists_match_approved_pixels(self) -> None:
        digest = hashlib.sha256()
        for key, image in sorted(specialists_final.source_frames()):
            self.assertEqual(self.registry[key].tobytes(), image.tobytes(), key)
            digest.update(key.encode())
            digest.update(image.tobytes())
        self.assertEqual(
            digest.hexdigest(),
            "c3fec01ecaad033daa146b8d2539b549587c22eefbda7266f280b7422645d237",
        )

    def test_promoted_defenses_match_approved_pixels(self) -> None:
        digest = hashlib.sha256()
        for key, image in sorted(installed_defenses_final.source_frames()):
            self.assertEqual(self.registry[key].tobytes(), image.tobytes(), key)
            digest.update(key.encode())
            digest.update(image.tobytes())
        self.assertEqual(
            digest.hexdigest(),
            "8e53ee55b230caaa338de50c5e6d2316a58beb15653ba5b5dacea82bee554f9c",
        )

    def test_citadel_preserves_the_approved_redoubt_sequence(self) -> None:
        digest = hashlib.sha256()
        for phase in range(10):
            suffix = f"_action{phase}" if phase else ""
            for stem in ("bastion_t1", "bastion_mount_t1"):
                digest.update(self.registry[f"{stem}{suffix}"].tobytes())
        self.assertEqual(
            digest.hexdigest(),
            "363e295803d2317d8723ea3f61b6dc4f129a1509f8377005cc468fbc55fcb81a",
        )

    def test_construction_bank_covers_every_building(self) -> None:
        self.assertEqual(
            construction_final.BUILDING_STEMS,
            (
                "foundry",
                "turret",
                "fabricator",
                "flak_turret",
                "bastion",
                "bastion_t1",
                "array",
                "reclaimer",
                "repair_bay",
                "extractor",
                "extractor_t1",
                "airworks",
                "crucible",
                "barricade",
                "scuttle_charge",
                "spire",
                "lattice",
            ),
        )

    def test_finalized_environment_bank_is_complete_and_pixel_stable(self) -> None:
        self.assertEqual(len(environment_final.FIELD_DEBRIS_KEYS), 10)
        self.assertEqual(len(environment_final.GROUND_BLOCKER_KEYS), 9)

        digest = hashlib.sha256()
        for key in environment_final.FIELD_DEBRIS_KEYS:
            image = self.registry[key]
            self.assertIn(image.size, ((32, 32), (64, 64)))
            digest.update(key.encode())
            digest.update(image.tobytes())
        for key, footprint in zip(
            environment_final.GROUND_BLOCKER_KEYS,
            environment_final.GROUND_BLOCKER_FOOTPRINTS,
            strict=True,
        ):
            image = self.registry[key]
            self.assertEqual(image.size, tuple(side * 64 for side in footprint))
            digest.update(key.encode())
            digest.update(image.tobytes())
        self.assertEqual(
            digest.hexdigest(),
            "3c9dbbd2cc09cd516004a5309784db2cb271d4e35b3f602a8d48ae7df4a2b2cd",
        )

    def test_slab_library_covers_every_size_class_without_clipping(self) -> None:
        images = []
        for size in range(len(quarry_rocks.SLAB_RADII)):
            canvas = quarry_rocks.slab_canvas(size)
            for variant in range(quarry_rocks.SLAB_VARIANTS):
                image = self.registry[f"rock_slab_{size}_{variant}"]
                self.assertEqual(image.size, canvas)
                left, top, right, bottom = image.getchannel("A").getbbox()
                # A slab touching its canvas edge would be cut off in game.
                self.assertGreater(left, 0)
                self.assertGreater(top, 0)
                self.assertLess(right, canvas[0])
                self.assertLess(bottom, canvas[1])
                images.append(image.tobytes())
        self.assertEqual(len(set(images)), len(images))
        self.assertEqual(list(quarry_rocks.SLAB_RADII), sorted(quarry_rocks.SLAB_RADII))

    def test_metadata_counts_match_every_generated_action_row(self) -> None:
        for stem, frame_set in finalized.UNIT_ACTIONS.items():
            self.assertEqual(finalized.ACTION_COUNTS[stem], len(frame_set))
            for variant in gen.PALETTES:
                for suffix in frame_set:
                    self.assertIn(
                        f"{stem}{gen.variant_tag(variant)}{suffix}", self.registry
                    )

        for stem, frame_set in finalized.BUILDING_WORK.items():
            self.assertEqual(finalized.ACTION_COUNTS[stem], len(frame_set))
            for variant in gen.PALETTES:
                for suffix in frame_set:
                    self.assertIn(
                        f"{stem}{gen.variant_tag(variant)}{suffix}", self.registry
                    )

        for stem, frame_set in finalized.DEFENSE_ACTIONS.items():
            self.assertEqual(finalized.ACTION_COUNTS[stem], len(frame_set))
            for variant in gen.PALETTES:
                for suffix in frame_set:
                    self.assertIn(
                        f"{stem}{gen.variant_tag(variant)}{suffix}", self.registry
                    )

        for stem, frame_set in finalized.DEFENSE_BASE_ACTIONS.items():
            self.assertEqual(finalized.ACTION_COUNTS[stem], len(frame_set))
            for variant in gen.PALETTES:
                for suffix in frame_set:
                    self.assertIn(
                        f"{stem}{gen.variant_tag(variant)}{suffix}", self.registry
                    )

    def test_movement_metadata_matches_every_generated_row(self) -> None:
        self.assertEqual(set(finalized.UNIT_MOVEMENT), set(finalized.UNIT_ACTIONS))
        for stem, frame_set in finalized.UNIT_MOVEMENT.items():
            for variant in gen.PALETTES:
                tag = gen.variant_tag(variant)
                base = self.registry[f"{stem}{tag}"]
                for suffix in frame_set:
                    frame = self.registry[f"{stem}{tag}{suffix}"]
                    self.assertEqual(frame.size, base.size)
                self.assertTrue(
                    any(
                        _changed_pixels(base, self.registry[f"{stem}{tag}{suffix}"]) > 2
                        for suffix in frame_set
                    ),
                    stem,
                )

    def test_air_support_detail_passes_preserve_the_approved_silhouettes(self) -> None:
        for variant in ("base", "probe"):
            with finalized._variant_palette(variant):
                for base_builder, approved_builder in (
                    (air_final.buzzard_sequence, air_support_final.buzzard_sequence),
                    (air_final.talon_sequence, air_support_final.talon_sequence),
                ):
                    base = base_builder()
                    approved = approved_builder()
                    self.assertEqual(len(base.frames), len(approved.frames))
                    for base_frame, approved_frame in zip(
                        base.frames, approved.frames, strict=True
                    ):
                        self.assertEqual(
                            base_frame.image.getchannel("A").tobytes(),
                            approved_frame.image.getchannel("A").tobytes(),
                        )

    def test_tier_one_combat_art_preserves_motion_and_attack_contracts(self) -> None:
        for builder in (
            tier_one_combat_final.lancer_sequence,
            tier_one_combat_final.bombard_sequence,
            tier_one_combat_final.flakhound_sequence,
        ):
            sequence = builder()
            idle, move1, move2 = (frame.image for frame in sequence.frames[:3])
            self.assertEqual(idle.size, (64, 64))
            self.assertNotEqual(idle.tobytes(), move1.tobytes())
            self.assertNotEqual(move1.tobytes(), move2.tobytes())
            damage_frames = [frame for frame in sequence.frames if frame.logical_damage]
            self.assertEqual(len(damage_frames), 1)
            self.assertGreaterEqual(damage_frames[0].report_count, 1)

    def test_kestrel_sequence_keeps_its_airframe_fixed(self) -> None:
        for variant in ("base", "probe"):
            frames = [
                airworks_scouts_final.render_kestrel(variant, phase)
                for phase in range(3)
            ]
            alpha = frames[0].getchannel("A").tobytes()
            self.assertTrue(
                all(frame.getchannel("A").tobytes() == alpha for frame in frames)
            )
            self.assertTrue(
                all(
                    _changed_pixels(left, right) > 2 for left, right in pairwise(frames)
                )
            )

    def test_airworks_queue_frames_keep_the_doors_closed(self) -> None:
        for variant in ("base", "probe"):
            frames = [
                airworks_scouts_final.render_airworks(variant, stage)
                for stage in range(5)
            ]
            door_box = (29, 44, 100, 106)
            closed = frames[0].crop(door_box).tobytes()
            self.assertEqual(frames[1].crop(door_box).tobytes(), closed)
            self.assertEqual(frames[2].crop(door_box).tobytes(), closed)
            self.assertNotEqual(frames[3].crop(door_box).tobytes(), closed)
            self.assertNotEqual(frames[4].crop(door_box).tobytes(), closed)

    def test_skyhook_rotors_and_sapper_legs_have_real_movement(self) -> None:
        for variant in ("base", "probe"):
            tag = gen.variant_tag(variant)
            for stem in ("skyhook", "sapper"):
                idle = self.registry[f"{stem}{tag}"]
                move1 = self.registry[f"{stem}{tag}_move1"]
                move2 = self.registry[f"{stem}{tag}_move2"]
                self.assertNotEqual(idle.tobytes(), move1.tobytes())
                self.assertNotEqual(move1.tobytes(), move2.tobytes())
            self.assertIsNone(
                skyhook_sapper_crucible_final.render_sapper(variant, action=4)
                .getchannel("A")
                .getbbox()
            )

    def test_crucible_opens_and_closes_its_segmented_lid(self) -> None:
        for variant in gen.PALETTES:
            frames = [
                self.registry[
                    f"crucible{gen.variant_tag(variant)}" + (f"_work{i}" if i else "")
                ]
                for i in range(4)
            ]
            self.assertEqual(frames[1].tobytes(), frames[3].tobytes())
            self.assertEqual(len({frame.tobytes() for frame in frames}), 3)
            for frame in frames[1:]:
                self.assertEqual(
                    frame.crop((0, 0, 128, 40)).tobytes(),
                    frames[0].crop((0, 0, 128, 40)).tobytes(),
                )
                self.assertGreater(
                    _changed_pixels(
                        frames[0].crop((45, 50, 85, 90)), frame.crop((45, 50, 85, 90))
                    ),
                    20,
                )

    def test_crucible_units_animate_treads_without_wobbling_the_hull(self) -> None:
        for stem in ("breaker", "avalanche"):
            for variant in ("base", "probe"):
                tag = gen.variant_tag(variant)
                idle = self.registry[f"{stem}{tag}"]
                move1 = self.registry[f"{stem}{tag}_move1"]
                move2 = self.registry[f"{stem}{tag}_move2"]
                self.assertEqual(
                    idle.getchannel("A").tobytes(), move1.getchannel("A").tobytes()
                )
                self.assertEqual(
                    idle.getchannel("A").tobytes(), move2.getchannel("A").tobytes()
                )
                self.assertNotEqual(idle.tobytes(), move1.tobytes())
                self.assertNotEqual(move1.tobytes(), move2.tobytes())

    def test_heavy_weapon_actions_preserve_distinct_launch_and_recovery_poses(
        self,
    ) -> None:
        for stem in ("breaker", "avalanche"):
            for variant in gen.PALETTES:
                frames = [
                    self.registry[f"{stem}{gen.variant_tag(variant)}_action{i}"]
                    for i in range(1, 5)
                ]
                self.assertGreaterEqual(len({frame.tobytes() for frame in frames}), 3)
                self.assertNotEqual(frames[0].tobytes(), frames[1].tobytes())

    def test_tracked_workers_move_only_their_treads(self) -> None:
        for stem in ("excavator",):
            for variant in gen.PALETTES:
                tag = gen.variant_tag(variant)
                idle = self.registry[f"{stem}{tag}"]
                for phase in (1, 2):
                    frame = self.registry[f"{stem}{tag}_move{phase}"]
                    self.assertEqual(
                        idle.getchannel("A").tobytes(), frame.getchannel("A").tobytes()
                    )
                    self.assertEqual(
                        idle.crop((38, 0, 90, 128)).tobytes(),
                        frame.crop((38, 0, 90, 128)).tobytes(),
                    )
                    for tread in ((10, 36, 38, 117), (90, 36, 118, 117)):
                        self.assertGreater(
                            _changed_pixels(idle.crop(tread), frame.crop(tread)), 8
                        )

    def test_excavator_meter_matches_the_harvesters_five_load_levels(self) -> None:
        frames = [
            self.registry[f"excavator_cargo{level}"]
            for level in range(excavator_final.CARGO_LEVELS)
        ]
        self.assertEqual(len({frame.tobytes() for frame in frames}), 5)
        self.assertIsNone(frames[0].getchannel("A").getbbox())
        areas = [
            sum(value > 0 for value in frame.getchannel("A").get_flattened_data())
            for frame in frames
        ]
        self.assertTrue(all(left < right for left, right in pairwise(areas)))

    def test_warden_moves_without_wobbling_the_hull(self) -> None:
        for variant in ("base", "probe"):
            tag = gen.variant_tag(variant)
            idle = self.registry[f"warden{tag}"]
            phases = [self.registry[f"warden{tag}_move{phase}"] for phase in (1, 2)]
            for frame in phases:
                self.assertEqual(
                    idle.getchannel("A").tobytes(),
                    frame.getchannel("A").tobytes(),
                )
            self.assertTrue(any(idle.tobytes() != frame.tobytes() for frame in phases))
            self.assertNotEqual(phases[0].tobytes(), phases[1].tobytes())

    def test_condor_keeps_its_wing_fixed_while_the_nose_opens(self) -> None:
        for variant in gen.PALETTES:
            tag = gen.variant_tag(variant)
            idle = self.registry[f"condor{tag}"]
            self.assertEqual(idle.size, (128, 128))
            for phase in (1, 2):
                self.assertEqual(
                    idle.tobytes(),
                    self.registry[f"condor{tag}_move{phase}"].tobytes(),
                )
            for action in range(1, 5):
                frame = self.registry[f"condor{tag}_action{action}"]
                self.assertEqual(
                    frame.crop((0, 40, 128, 128)).tobytes(),
                    idle.crop((0, 40, 128, 128)).tobytes(),
                )
            open_nose = self.registry[f"condor{tag}_action2"]
            self.assertLess(open_nose.getpixel((64, 24))[3], idle.getpixel((64, 24))[3])

    def test_probe_renders_share_alpha_but_not_accent_pixels(self) -> None:
        probes = [key for key in self.registry if gen.PROBE_TAG in key]
        self.assertTrue(probes)
        for key in probes:
            base = self.registry[key.replace(gen.PROBE_TAG, "")]
            probe = self.registry[key]
            with self.subTest(key=key):
                self.assertEqual(base.size, probe.size)
                self.assertEqual(
                    base.getchannel("A").tobytes(),
                    probe.getchannel("A").tobytes(),
                )
                self.assertGreater(_changed_pixels(base, probe), 8)

    def test_harvester_keeps_cargo_separate_from_tracks_and_grapple(self) -> None:
        for variant in gen.PALETTES:
            loads = []
            for cargo in range(finalized.HARVESTER_CARGO_LEVELS):
                prefix = f"harvester{gen.variant_tag(variant)}_cargo{cargo}"
                idle = self.registry[prefix]
                loads.append(idle.crop((44, 67, 84, 101)).tobytes())
                for suffix in ("_tread1", "_tread2", "_scoop1", "_scoop2"):
                    frame = self.registry[prefix + suffix]
                    self.assertEqual(
                        idle.crop((44, 67, 84, 101)).tobytes(),
                        frame.crop((44, 67, 84, 101)).tobytes(),
                    )
                    self.assertNotEqual(idle.tobytes(), frame.tobytes())
                for suffix in ("_tread1", "_tread2"):
                    self.assertEqual(
                        idle.crop((0, 0, 128, 32)).tobytes(),
                        self.registry[prefix + suffix].crop((0, 0, 128, 32)).tobytes(),
                    )
                for claw in ((30, 3, 64, 55), (64, 3, 98, 55)):
                    self.assertGreater(
                        _changed_pixels(
                            idle.crop(claw),
                            self.registry[prefix + "_scoop1"].crop(claw),
                        ),
                        20,
                    )
            self.assertEqual(len(set(loads)), 5)

    def test_defense_foundations_and_mounts_are_separate_square_layers(self) -> None:
        pairs = (
            ("turret", "turret_barrel", 128),
            ("turret_t1", "turret_barrel_t1", 128),
            ("turret_t2", "turret_barrel_t2", 128),
            ("flak_turret", "flak_mount", 128),
            ("bastion", "bastion_mount", 128),
        )
        for variant in gen.PALETTES:
            tag = gen.variant_tag(variant)
            for base_stem, mount_stem, side in pairs:
                base = self.registry[f"{base_stem}{tag}"]
                mount = self.registry[f"{mount_stem}{tag}"]
                with self.subTest(variant=variant, mount=mount_stem):
                    self.assertEqual(base.size, (side, side))
                    self.assertEqual(mount.size, (side, side))
                    self.assertIsNotNone(base.getchannel("A").getbbox())
                    self.assertIsNotNone(mount.getchannel("A").getbbox())
                    self.assertGreater(_changed_pixels(base, mount), side)
                for suffix in finalized.DEFENSE_ACTIONS[mount_stem]:
                    action = self.registry[f"{mount_stem}{tag}{suffix}"]
                    self.assertEqual(action.size, (side, side))
                    if mount_stem == "bastion_mount":
                        self.assertLess(
                            action.getchannel("A").getbbox()[3],
                            side,
                            f"{mount_stem}{suffix} must not clip at the canvas edge",
                        )

    def test_action_rows_contain_real_frame_changes(self) -> None:
        for variant in gen.PALETTES:
            tag = gen.variant_tag(variant)
            for stem, frame_set in finalized.UNIT_ACTIONS.items():
                base = self.registry[f"{stem}{tag}"]
                changed = [
                    _changed_pixels(base, self.registry[f"{stem}{tag}{suffix}"])
                    for suffix in frame_set
                ]
                with self.subTest(variant=variant, stem=stem):
                    self.assertGreater(max(changed), 12)
            for stem, frame_set in finalized.BUILDING_WORK.items():
                base = self.registry[f"{stem}{tag}"]
                changed = [
                    _changed_pixels(base, self.registry[f"{stem}{tag}{suffix}"])
                    for suffix in frame_set
                ]
                with self.subTest(variant=variant, stem=stem):
                    self.assertGreater(max(changed), 12)

    def test_construction_keeps_the_complete_hull_visible_from_stage_zero(self) -> None:
        for variant in gen.PALETTES:
            for stem in construction_final.BUILDING_STEMS:
                hull = construction_final.complete_hull(self.registry, stem, variant)
                occupied = [
                    index
                    for index, value in enumerate(
                        hull.getchannel("A").get_flattened_data()
                    )
                    if value
                ]
                self.assertTrue(occupied)
                for stage in range(3):
                    frame_alpha = self.registry[
                        f"{stem}{gen.variant_tag(variant)}_site{stage}_0"
                    ].getchannel("A")
                    with self.subTest(variant=variant, stem=stem, stage=stage):
                        self.assertTrue(
                            all(
                                frame_alpha.getpixel(
                                    (index % hull.width, index // hull.width)
                                )
                                for index in occupied
                            )
                        )

    def test_construction_hull_energy_increases_without_a_reveal_wipe(self) -> None:
        for variant in gen.PALETTES:
            for stem in construction_final.BUILDING_STEMS:
                hull = construction_final.complete_hull(self.registry, stem, variant)
                alpha_totals = [
                    sum(
                        construction_final.dimmed_hull(hull, stage)
                        .getchannel("A")
                        .get_flattened_data()
                    )
                    for stage in range(3)
                ]
                with self.subTest(variant=variant, stem=stem):
                    self.assertLess(alpha_totals[0], alpha_totals[1])
                    self.assertLess(alpha_totals[1], alpha_totals[2])

    def test_construction_cage_is_fixed_and_active_delta_is_local(self) -> None:
        for variant in gen.PALETTES:
            tag = gen.variant_tag(variant)
            for stem in construction_final.BUILDING_STEMS:
                first = self.registry[f"{stem}{tag}_site0_0"]
                scale = first.width / 64
                fixed_points = (
                    (round(6 * scale), round(16 * scale)),
                    (first.width - round(6 * scale), round(16 * scale)),
                    (round(20 * scale), round(9 * scale)),
                    (round(20 * scale), round(32 * scale)),
                    (round(20 * scale), first.height - round(7 * scale)),
                )
                fixed_colors = tuple(first.getpixel(point) for point in fixed_points)
                for stage in range(3):
                    still = self.registry[f"{stem}{tag}_site{stage}_0"]
                    active = self.registry[f"{stem}{tag}_site{stage}_1"]
                    difference = ImageChops.difference(still, active)
                    bbox = difference.getbbox()
                    with self.subTest(variant=variant, stem=stem, stage=stage):
                        self.assertEqual(
                            tuple(still.getpixel(point) for point in fixed_points),
                            fixed_colors,
                        )
                        self.assertIsNotNone(bbox)
                        self.assertLessEqual(bbox[2] - bbox[0], round(9 * scale))
                        self.assertLessEqual(bbox[3] - bbox[1], round(9 * scale))

    def test_defense_sites_include_their_recognizable_mounts(self) -> None:
        for variant in gen.PALETTES:
            tag = gen.variant_tag(variant)
            for stem, mount_stem in construction_final.DEFENSE_MOUNTS.items():
                mount = self.registry[f"{mount_stem}{tag}"].getchannel("A")
                site = self.registry[f"{stem}{tag}_site0_0"].getchannel("A")
                mount_pixels = [
                    index
                    for index, value in enumerate(mount.get_flattened_data())
                    if value
                ]
                with self.subTest(variant=variant, stem=stem):
                    self.assertTrue(mount_pixels)
                    self.assertTrue(
                        all(
                            site.getpixel((index % site.width, index // site.width))
                            for index in mount_pixels
                        )
                    )

    def test_buzzard_matches_the_approved_armored_quad_fan_sequence(self) -> None:
        expected = (
            "1143a769ec064862af3d9fe9837b15434bd60326b45af2415fe267e3c80cb56d",
            "d5d7eb5d4f8a152bdc5a7ddabe52aa8093f1c99333469d9d841a5838021ba971",
            "018ba81c920b462f0a9f9f3fa009d6685c3886d8daf289d6e74155786c62b4d4",
            "1143a769ec064862af3d9fe9837b15434bd60326b45af2415fe267e3c80cb56d",
            "acc78432f812a96dea0e1551c251169d7f24f57174769e87e2a53162b09ed1b7",
            "f945d232516998e8518ba3320ad4e8556ada1131a39ffd072ecf0e28940a9fd7",
            "ed31b38adce84cd193df0e44fd047d95007cccf742b71c8b729c7f1dfea135b8",
            "1143a769ec064862af3d9fe9837b15434bd60326b45af2415fe267e3c80cb56d",
        )
        sequence = finalized._unit_sequences()["buzzard"]()
        actual = tuple(
            hashlib.sha256(frame.image.convert("RGBA").tobytes()).hexdigest()
            for frame in sequence.frames
        )
        self.assertEqual(actual, expected)

    def test_buzzard_attack_flare_does_not_touch_the_canvas_edge(self) -> None:
        for variant in gen.PALETTES:
            frame = self.registry[
                f"buzzard{gen.variant_tag(variant)}_action2"
            ].getchannel("A")
            edge = list(frame.crop((0, 0, frame.width, 1)).get_flattened_data())
            self.assertFalse(any(edge), f"{variant} muzzle flare is clipped")

    def test_foundry_gantry_moves_over_a_fixed_foundation(self) -> None:
        for variant in gen.PALETTES:
            frames = [
                self.registry[
                    f"foundry{gen.variant_tag(variant)}" + (f"_work{i}" if i else "")
                ]
                for i in range(5)
            ]
            self.assertEqual(len({frame.tobytes() for frame in frames}), 5)
            for frame in frames[1:]:
                self.assertEqual(
                    frame.crop((0, 0, 128, 30)).tobytes(),
                    frames[0].crop((0, 0, 128, 30)).tobytes(),
                )
                self.assertGreater(
                    _changed_pixels(
                        frames[0].crop((30, 34, 100, 100)),
                        frame.crop((30, 34, 100, 100)),
                    ),
                    100,
                )

    def test_bastion_ready_and_reload_frames_have_physical_charge_cells(self) -> None:
        centers = [(25, 53 + index * 8) for index in range(5)]
        expected = {
            "": 5,
            "_action1": 1,
            "_action2": 2,
            "_action3": 3,
            "_action4": 4,
            "_action5": 5,
            "_action6": 5,
            "_action7": 0,
            "_action8": 0,
            "_action9": 0,
        }
        for stem, centers, color in (
            ("bastion", centers, installed_defenses_final.BRASS),
            (
                "bastion_t1",
                [(28, 59 + i * 10) for i in range(5)],
                installed_defenses_final.CITADEL_CHARGE,
            ),
        ):
            for variant in gen.PALETTES:
                for suffix, count in expected.items():
                    image = self.registry[f"{stem}{gen.variant_tag(variant)}{suffix}"]
                    lit = sum(
                        sum(
                            (a - b) ** 2
                            for a, b in zip(image.getpixel(center)[:3], color)
                        )
                        < 100
                        for center in centers
                    )
                    with self.subTest(variant=variant, suffix=suffix):
                        self.assertEqual(lit, count)

    def test_bastion_recoils_after_report_then_returns_quickly(self) -> None:
        for stem in ("bastion_mount", "bastion_mount_t1"):
            for variant in gen.PALETTES:
                muzzle_tops = [
                    self.registry[f"{stem}{gen.variant_tag(variant)}{suffix}"]
                    .crop((55, 0, 73, 40))
                    .getchannel("A")
                    .getbbox()[1]
                    for suffix in ("_action5", "_action7", "_action8", "_action9")
                ]
                with self.subTest(variant=variant):
                    ready, recoil, settling, returned = muzzle_tops
                    self.assertEqual(recoil - ready, 7)
                    self.assertEqual(settling - ready, 3)
                    self.assertEqual(returned, ready)

    def test_flakhound_magazines_fill_during_reload(self) -> None:
        expected = {
            "": 4,
            "_tread1": 4,
            "_tread2": 4,
            "_action1": 0,
            "_action2": 1,
            "_action3": 2,
            "_action4": 3,
            "_action5": 4,
            "_action6": 1,
            "_action7": 1,
            "_action8": 1,
            "_action9": 1,
        }
        for variant in gen.PALETTES:
            for suffix, count in expected.items():
                image = self.registry[f"flakhound{gen.variant_tag(variant)}{suffix}"]
                for x in (34, 94):
                    loaded = 0
                    for index in range(4):
                        pixel = image.getpixel((x, 73 + index * 3))[:3]
                        brass = sum(
                            (a - b) ** 2 for a, b in zip(pixel, specialists_final.BRASS)
                        )
                        empty = sum(
                            (a - b) ** 2 for a, b in zip(pixel, specialists_final.DEEP)
                        )
                        loaded += brass < empty
                    self.assertEqual(loaded, count, (variant, suffix, x))

    def test_bombard_spades_and_muzzle_report_fit_their_canvases(self) -> None:
        for phase in range(5):
            image = self.registry[f"bombard_spades_{phase}"]
            left, top, right, bottom = image.getbbox()
            self.assertTrue(0 < left < right < image.width)
            self.assertTrue(0 < top < bottom < image.height)
        for variant in gen.PALETTES:
            report = self.registry[f"bombard{gen.variant_tag(variant)}_action4"]
            flashes = sum(
                pixel[0] > 200 and pixel[1] > 130 and pixel[3] > 128
                for pixel in report.crop((50, 8, 79, 35)).get_flattened_data()
            )
            self.assertGreater(flashes, 0)


class MantleSourceTests(unittest.TestCase):
    def test_charge_key_poses_preserve_the_approved_art(self):
        digests = (
            "e1c1ed2b4cde1c321b433b432c62c0bab7aefaa83df92401010dfe693194436a",
            "8285ba2d7715488e061e726dc3698298f450c16b70128ba74a64e699590f7140",
            "2a7aa359330d69c5004982c96993624d94c4d3e1593967265546eeacf81ef693",
            "9d9a1243ed6da12cffe197f78a969bd59121af2a3ce5afecab84acfcdc3aeba4",
        )
        for charges, expected in enumerate(digests):
            image = mantle_final.render_mantle("base", charges=charges)
            self.assertEqual(hashlib.sha256(image.tobytes()).hexdigest(), expected)

    def test_serial_recharge_composes_without_changing_the_chassis(self):
        for frame in range(mantle_final.CHARGE_FRAMES):
            charges, step = divmod(frame, mantle_final.RECHARGE_STEPS)
            for move in range(3):
                actual = mantle_final.render_mantle("base", move)
                actual.alpha_composite(mantle_final.render_charge_frame(frame))
                expected = mantle_final.render_mantle(
                    "base", move, charges, step / mantle_final.RECHARGE_STEPS
                )
                self.assertEqual(actual.tobytes(), expected.tobytes(), (frame, move))


def _strongly_changed_pixels(left: Image.Image, right: Image.Image) -> int:
    difference = ImageChops.difference(left, right)
    return sum(max(pixel) > 40 for pixel in difference.get_flattened_data())


def _place_on_pivot(image: Image.Image, layer: Image.Image, slot: int) -> None:
    x, y = lattice_final.POD_PIVOTS[slot]
    half = layer.width // 2
    image.alpha_composite(layer, (x - half, y - half))


class StrategicSourceTests(unittest.TestCase):
    def test_spire_key_poses_preserve_the_approved_art(self):
        poses = (
            (
                (0.0, 0, 0, 0.0, 0.0),
                "dbc7e9ed443a2bd2e769377ca0be7995c70047e616f9650ac20da1e100166b3b",
            ),
            (
                (0.0, 2, 21, 1.0, 0.0),
                "a31ea324eeb25080b90b4492250b40f3cff525f2370d3faa4dd86fba5dbdbe15",
            ),
            (
                (0.0, 4, 38, 1.0, 1.0),
                "2f78ddd17bede9d465ae7377fb91404d1d087804e56d24c7bf583322b53ed7e6",
            ),
            (
                (0.0, 5, 0, 1.0, 1.0),
                "747bf87b3487986dbe2b70f7673535ad721a628d85057586a097e2c232a0976d",
            ),
            # A quarter turn: other headings resample between pixels, and
            # bicubic rounding differs across platforms.
            (
                (90.0, 5, 0, 1.0, 1.0),
                "fa656cb5cf4f0ff1cf07c5f3de15eb42d2ff05f6aaa8bff60f4cd08d0b359ed1",
            ),
        )
        for pose, expected in poses:
            image = spire_final.render_spire("base", *pose)
            self.assertEqual(
                hashlib.sha256(image.tobytes()).hexdigest(), expected, pose
            )

    def test_lattice_key_poses_preserve_the_approved_art(self):
        poses = (
            ({}, "c8cc10b86f1f84904a12fdc19d679f84feefe9239c7f17703a5533d6389bae77"),
            (
                {"loads": ("loaded", "loaded"), "clamps": (1.0, 1.0)},
                "5bb47d9f39acaea888643b7f3d8bd613064bef05360d7688daac9d21a0865f14",
            ),
            (
                {"loads": ("partial", "empty"), "seated": (1, 0), "arm": (0, 7)},
                "c0ec7c6c9633cb822dd80829e8906d919fd46df7b4fb2b2857dc260b142116ce",
            ),
            (
                {
                    "headings": (-90.0, 90.0),
                    "loads": ("loaded", "loaded"),
                    "clamps": (1.0, 1.0),
                },
                "cc65c76c87e205e047be1889c94bf60e7355003b65d73f181ae2f96179758c2a",
            ),
        )
        for pose, expected in poses:
            image = lattice_final.render_lattice("base", **pose)
            self.assertEqual(
                hashlib.sha256(image.tobytes()).hexdigest(), expected, pose
            )

    def test_spire_layers_stack_onto_the_single_pass_art(self):
        steps = spire_final.LOCK_STEPS
        for stack, frame, lower, upper in (
            (0, 0, 0, 0),
            (2, 21, steps, 0),
            *((4, frame, steps, steps) for frame in spire_final.NOSE_FRAMES),
            (5, 0, steps, steps),
            (5, spire_final.GANTRY_FRAMES, steps, steps),
        ):
            layered = spire_final.render_foundation("base")
            layered.alpha_composite(spire_final.render_deck("base"))
            if stack == spire_final.SECTIONS:
                layered.alpha_composite(spire_final.render_round("base"))
            elif stack:
                layered.alpha_composite(spire_final.render_stack("base", stack))
            if frame in spire_final.NOSE_FRAMES:
                layered.alpha_composite(spire_final.render_nose("base", frame))
            layered.alpha_composite(spire_final.render_gantry(frame))
            layered.alpha_composite(spire_final.render_cover("base"))
            layered.alpha_composite(
                spire_final.render_lock(spire_final.LOWER_LOCK_Y, lower)
            )
            layered.alpha_composite(
                spire_final.render_lock(spire_final.UPPER_LOCK_Y, upper)
            )
            whole = spire_final.render_spire(
                "base", 0.0, stack, frame, lower / steps, upper / steps
            )
            # Separately filtered layers differ only faintly along shared
            # edges; a layer one pixel off its registration changes dozens.
            self.assertLessEqual(_strongly_changed_pixels(layered, whole), 2, frame)

    def test_lattice_layers_stack_onto_the_single_pass_art(self):
        steps = lattice_final.CLAMP_STEPS
        for loads, seated, clamps, arm in (
            (("empty", "empty"), (0, 0), (0, 0), None),
            (("loaded", "loaded"), (0, 0), (steps, steps), None),
            (("partial", "empty"), (1, 0), (0, 0), (0, 7)),
            (
                ("loaded", "partial"),
                (0, 2),
                (steps, 0),
                (1, lattice_final.NOSE_FRAMES[1]),
            ),
        ):
            layered = lattice_final.render_base()
            for slot in range(2):
                _place_on_pivot(layered, lattice_final.render_pod("base"), slot)
                if loads[slot] == "loaded":
                    _place_on_pivot(
                        layered, lattice_final.render_interceptor("base"), slot
                    )
                elif seated[slot]:
                    _place_on_pivot(
                        layered, lattice_final.render_stack(seated[slot]), slot
                    )
                _place_on_pivot(layered, lattice_final.render_clamp(clamps[slot]), slot)
            if arm is not None:
                if arm[1] in lattice_final.NOSE_FRAMES:
                    layered.alpha_composite(lattice_final.render_nose("base", *arm))
                layered.alpha_composite(lattice_final.render_arm(*arm))
            layered.alpha_composite(lattice_final.render_cover())
            whole = lattice_final.render_lattice(
                "base", (0.0, 0.0), loads, seated, tuple(c / steps for c in clamps), arm
            )
            self.assertLessEqual(_strongly_changed_pixels(layered, whole), 2, loads)

    def test_the_gantry_seats_sections_in_order_and_carries_the_nose_last(self):
        stacks = [
            spire_final.gantry_stack(f) for f in range(spire_final.GANTRY_FRAMES + 1)
        ]
        self.assertEqual(stacks, sorted(stacks))
        self.assertEqual(stacks[-1], spire_final.SECTIONS)
        nose_section = spire_final.SECTIONS - 1
        self.assertTrue(
            all(
                spire_final.gantry_stack(f) == nose_section
                for f in spire_final.NOSE_FRAMES
            )
        )
        for slot in range(2):
            seated = [
                lattice_final.arm_pose(slot, f)["seated"]
                for f in range(1, lattice_final.ARM_FRAMES + 1)
            ]
            self.assertEqual(seated, sorted(seated))
            self.assertEqual(seated[-1], lattice_final.PARTS)
            for frame in lattice_final.NOSE_FRAMES:
                self.assertEqual(
                    lattice_final.arm_pose(slot, frame)["seated"],
                    lattice_final.PARTS - 1,
                )

    def test_rotating_layers_center_on_their_pivots(self):
        with tempfile.TemporaryDirectory() as directory:
            registry: dict[str, Image.Image] = {}
            spire_final.install_spire(registry, Path(directory))
            lattice_final.install_lattice(registry, Path(directory))
        for key, image in registry.items():
            if key.startswith("rig_spire_warhead"):
                size = spire_final.WARHEAD_SIZE
            elif key.startswith(("rig_lattice_pod", "rig_lattice_interceptor")):
                size = lattice_final.POD_SIZE
            elif key.startswith(("rig_lattice_stack", "rig_lattice_clamp")):
                size = lattice_final.POD_SIZE
            elif key.startswith("rig_lattice_launch"):
                size = lattice_final.LAUNCH_SIZE
            else:
                size = specialists_final.SIZE
            self.assertEqual(image.size, (size, size), key)
        # A pod layer's bounds straddle its center, the pivot it turns on.
        left, top, right, bottom = registry["rig_lattice_pod"].getbbox()
        self.assertLess(abs((left + right) / 2 - lattice_final.POD_SIZE / 2), 1)
        self.assertLess(top, lattice_final.POD_SIZE / 2 < bottom)



def _maw_layers(left: int, right: int, slats: int, heat: float, arm: int, spin: int) -> Image.Image:
    """The runtime layers in the shell's draw order, without belt scrap."""
    image = Image.new("RGBA", (maw_final.SIZE, maw_final.SIZE))
    for layer in (
        maw_final.render_tread(0, left),
        maw_final.render_tread(1, right),
        maw_final.render_hull("base"),
        maw_final.render_slats(slats),
        maw_final.render_kiln(),
    ):
        image.alpha_composite(layer)
    glow = maw_final.render_kiln_glow()
    glow.putalpha(glow.getchannel("A").point(lambda value: round(value * heat)))
    image.alpha_composite(glow)
    image.alpha_composite(maw_final.render_arms(arm))
    stow = round(maw_final.DRUM_STOW * (1 - arm / maw_final.ARM_STEPS))
    drum = Image.new("RGBA", image.size)
    drum.paste(maw_final.render_drum("base", spin).crop((0, 0, 256, 256 - stow)), (0, stow))
    image.alpha_composite(drum)
    return image


def _bright_centroid_x(image: Image.Image) -> float:
    """The mean column of bright, opaque pixels: the drum's teeth and arms."""
    total = weighted = 0
    for index, (r, g, b, a) in enumerate(image.get_flattened_data()):
        if a == 255 and (r + g + b) / 3 > 140:
            total += 1
            weighted += index % image.width
    if not total:
        raise AssertionError("no bright pixels")
    return weighted / total


class MawSourceTests(unittest.TestCase):
    def test_maw_key_poses_preserve_the_approved_art(self):
        # Digests of the approved Kilnjaw study's own drawing code at
        # explicit poses: stowed, cutting with the kiln warming, stowing.
        poses = (
            (
                ((0.0, 0.0), 0.0, 0.0, 0.0, 0.0),
                "bd5ee62abd4a67fbac7aed4699e2cab53a330bf27ec8e5a1f8f67f9a0d5c0607",
            ),
            (
                ((-3.0, -3.0), 12.5, 0.4, 1.0, 0.7),
                "6e1c642cef4ca055d80cb2688a1c0949a52e75b366b351c3c2df2122a77a62f2",
            ),
            (
                ((-5.0, -5.0), 33.0, 1.0, 0.5, 2.9),
                "1be2e18edee8ab7763ac1e2e89b8f546c72c1f2c6629c71d33bd62792fd07847",
            ),
        )
        for pose, expected in poses:
            study = maw_final.render_study("base", *pose)
            self.assertEqual(
                hashlib.sha256(study.tobytes()).hexdigest(), expected, pose
            )
            # The game draws the study moved toward the nose, nothing else.
            moved = maw_final.render_maw("base", *pose)
            self.assertEqual(
                moved.crop((0, 0, 256, 256 - maw_final.CENTRE)).tobytes(),
                study.crop((0, maw_final.CENTRE, 256, 256)).tobytes(),
            )

    def test_maw_layers_stack_onto_the_single_pass_art(self):
        for left, right, slats, heat, arm, spin in (
            (0, 0, 0, 0.0, 0, 0),
            (3, 5, 2, 0.0, 4, 7),
            (6, 1, 4, 1.0, 2, 13),
            (2, 2, 6, 0.5, 4, 0),
        ):
            whole = maw_final.render_maw(
                "base",
                (float(left), float(right)),
                float(slats),
                heat,
                arm / maw_final.ARM_STEPS,
                spin * maw_final.DRUM_PERIOD / maw_final.DRUM_FRAMES,
            )
            layered = _maw_layers(left, right, slats, heat, arm, spin)
            # Separately filtered layers differ faintly along shared edges;
            # a misregistered layer changes hundreds of pixels.
            self.assertLessEqual(
                _strongly_changed_pixels(layered, whole), 40, (left, right, spin)
            )

    def test_moving_layers_cover_only_what_moves(self):
        left, right, slats = maw_final._masks()
        for a, b in ((left, right), (left, slats), (right, slats)):
            self.assertIsNone(ImageChops.multiply(a, b).getbbox())
        for side in (0, 1):
            frames = [maw_final.render_tread(side, k) for k in range(maw_final.TREAD_PITCH)]
            for a, b in pairwise(frames):
                self.assertGreater(_changed_pixels(a, b), 0)
        drum = [maw_final.render_drum("base", k) for k in range(maw_final.DRUM_FRAMES)]
        for a, b in pairwise(drum):
            self.assertGreater(_changed_pixels(a, b), 0)
        # Half a turn returns the drum to its first frame.
        self.assertEqual(
            maw_final._centred(
                maw_final._alone(
                    lambda d: maw_final.drum(
                        d, maw_final.DRUM_PERIOD, maw_final.PAINT["base"]
                    )
                )
            ).tobytes(),
            drum[0].tobytes(),
        )

    def test_scrap_heats_toward_the_kiln(self):
        for variant in range(maw_final.SCRAP_VARIANTS):
            steps = [
                maw_final.render_scrap(variant, step)
                for step in range(maw_final.HEAT_STEPS)
            ]
            brightness = [
                sum(ImageStat.Stat(image.convert("RGB"), image.getchannel("A")).mean)
                for image in steps
            ]
            # Pale gold dims a little on its way to orange; the hottest
            # step is still the brightest and every step changes.
            for a, b in pairwise(steps):
                self.assertGreater(_changed_pixels(a, b), 0, variant)
            self.assertEqual(max(brightness), brightness[-1], variant)

    def test_layers_fit_the_atlas(self):
        layers = [
            maw_final.render_hull("base"),
            maw_final.render_tread(0, 0),
            maw_final.render_slats(0),
            maw_final.render_kiln(),
            maw_final.render_kiln_glow(),
            maw_final.render_arms(0),
            maw_final.render_drum("base", 0),
            maw_final.render_scrap(0, 0),
            maw_final.render_cradle("base"),
            maw_final.render_cradle_site("base", 0),
            maw_final.render_bridge("base"),
            maw_final.render_trolley("base", True),
            maw_final.render_pad(),
            maw_final.render_lamp(True),
        ]
        for layer in layers:
            self.assertEqual(layer.width % 8, 0)
            self.assertEqual(layer.height % 8, 0)
        for layer in layers[-6:-2]:
            self.assertEqual(layer.size, (maw_final.CRADLE, maw_final.CRADLE))

    def test_cradle_raises_the_maw_facing_east_in_building_opacity_steps(self):
        empty = maw_final.render_cradle_site("base", 0)
        steps = [
            maw_final.render_cradle_site("base", stage)
            for stage in range(len(maw_final.HULL_OPACITY))
        ]
        bed = maw_final._crop(
            maw_final._alone(lambda d: (maw_final._bed(d), maw_final._runways(d)))
        )
        change = [_changed_pixels(bed, step) for step in steps]
        self.assertTrue(all(count > 0 for count in change))
        energy = [
            sum(ImageChops.difference(bed, step).convert("L").get_flattened_data())
            for step in steps
        ]
        self.assertEqual(energy, sorted(energy))
        self.assertIsNotNone(empty.getbbox())
        # The stood-up Maw takes heading zero, east: the drum end of the
        # assembled hull and of the unit sprite turned east sit east.
        east = maw_final.render_maw("base").transpose(Image.Transpose.ROTATE_270)
        self.assertGreater(_bright_centroid_x(east), east.width * 0.65)
        last = steps[-1]
        self.assertGreater(_bright_centroid_x(last), last.width * 0.65)

    def test_cradle_crane_welds_only_in_its_working_frame(self):
        idle = maw_final.render_trolley("base", False)
        welding = maw_final.render_trolley("base", True)
        self.assertGreater(_changed_pixels(idle, welding), 0)
        self.assertIsNotNone(maw_final.render_bridge("base").getbbox())


def _place(image: Image.Image, layer: Image.Image, centre) -> None:
    x, y = centre
    out = Image.new("RGBA", image.size)
    out.paste(layer, (round(x - layer.width / 2), round(y - layer.height / 2)))
    image.alpha_composite(out)


def _roc_layers(age: float) -> Image.Image:
    """The Roc's runtime layers in the shell's draw order, guns ahead and
    no drones docked."""
    image = Image.new("RGBA", (roc_final.SIZE, roc_final.SIZE))

    def centred(point):
        return point[0], point[1] - roc_final.CENTRE

    centre = roc_final.SIZE // 2
    _place(image, roc_final.render_shadow(), (centre + roc_final.HULL_SHADOW[0], centre + roc_final.HULL_SHADOW[1]))
    for mount in roc_final.MOUNTS:
        x, y = centred(mount)
        _place(image, roc_final.render_gun_shadow(), (x + roc_final.GUN_SHADOW[0], y + roc_final.GUN_SHADOW[1]))
    image.alpha_composite(roc_final.render_wall())
    image.alpha_composite(roc_final.render_yokes())
    for side, mount in enumerate(roc_final.MOUNTS):
        _place(image, roc_final.render_gun("base", age if side == 0 else roc_final.NO_SHOT), centred(mount))
    image.alpha_composite(roc_final.render_hull("base"))
    for side, fan in enumerate(roc_final.FANS):
        _place(image, roc_final.render_fan(side, "base", 0), centred(fan))
    for dock, point in enumerate(roc_final.DOCKS):
        _place(image, roc_final.render_clamps(dock, "base", 0), centred(point))
    _place(image, roc_final.render_hatch("base", 0), centred(roc_final.HATCH_CENTRE))
    return image


class RocSourceTests(unittest.TestCase):
    def test_roc_key_poses_preserve_the_approved_art(self):
        # Digests of the approved Longbow study's own drawing code with the
        # ship at rest and every drone docked: guns parked, then the port
        # gun's first flash.
        poses = (
            (
                (roc_final.NO_SHOT,),
                "5fe4b9a734bbae30b2eea9cd6ff499637cff3f4162577dc4367209e508078a72",
            ),
            (
                (0,),
                "cb1483d901ea4f1561bdda8cde32c9e5a0e601f5300bd24fd5b2268eaa20f38d",
            ),
        )
        for (age,), expected in poses:
            guns = ((roc_final.PARK[0], age), (roc_final.PARK[1], roc_final.NO_SHOT))
            study = roc_final.render_study("base", guns=guns)
            self.assertEqual(hashlib.sha256(study.tobytes()).hexdigest(), expected, age)
        # The portrait is the study without its ground shadow but with the
        # closed cargo hatch, moved so the ship turns about the canvas centre.
        study = roc_final.render_study("base", shadows=False, hatch=0.0)
        self.assertEqual(
            roc_final.render_roc("base").crop((0, 0, 256, 256 - roc_final.CENTRE)).tobytes(),
            study.crop((0, roc_final.CENTRE, 256, 256)).tobytes(),
        )

    def test_roc_layers_stack_onto_the_single_pass_art(self):
        for age in (roc_final.NO_SHOT, 0, 2, 6):
            guns = ((0.0, age), (0.0, roc_final.NO_SHOT))
            whole = roc_final._centred(
                roc_final.render_study("base", guns=guns, drones=[], hatch=0.0)
            )
            # Separately filtered layers differ faintly where the hull's rim
            # meets its wall; a layer a pixel out of place changes hundreds
            # more.
            self.assertLessEqual(_strongly_changed_pixels(_roc_layers(age), whole), 200, age)

    def test_fans_clamps_and_hatch_redraw_only_what_moves(self):
        fans, clamps, hatch = roc_final._masks()
        for a, b in ((fans, clamps), (fans, hatch), (clamps, hatch)):
            self.assertIsNone(ImageChops.multiply(a, b).getbbox())
        for side in (0, 1):
            frames = [roc_final.render_fan(side, "base", k) for k in range(roc_final.FAN_FRAMES)]
            for a, b in pairwise(frames):
                self.assertGreater(_changed_pixels(a, b), 0)
        # A fifth of a turn brings the five blades back to the first frame.
        self.assertEqual(
            roc_final._hull("base", roc_final.FAN_FRAMES).tobytes(),
            roc_final._hull("base", 0).tobytes(),
        )
        for dock in range(len(roc_final.DOCKS)):
            steps = [roc_final.render_clamps(dock, "base", k) for k in range(roc_final.CLAMP_STEPS)]
            for a, b in pairwise(steps):
                self.assertGreater(_changed_pixels(a, b), 0, dock)

    def test_the_wreck_hull_has_still_fans_and_empty_open_docks(self):
        wreck = roc_final.render_wreck("base")
        hull = roc_final.render_hull("base").copy()

        def centred(point):
            return point[0], point[1] - roc_final.CENTRE

        for side, fan in enumerate(roc_final.FANS):
            _place(hull, roc_final.render_fan(side, "base", 0), centred(fan))
        for dock, point in enumerate(roc_final.DOCKS):
            _place(hull, roc_final.render_clamps(dock, "base", roc_final.CLAMP_STEPS - 1), centred(point))
        _place(hull, roc_final.render_hatch("base", 0), centred(roc_final.HATCH_CENTRE))
        self.assertEqual(wreck.tobytes(), hull.tobytes())

    def test_drone_layers_split_the_study_drone(self):
        for rotor in range(0, roc_final.ROTOR_FRAMES * roc_final.ROTOR_STEP, roc_final.ROTOR_STEP):
            whole = roc_final.render_drone("base", rotor)
            layered = roc_final.render_drone_frame("base", rotor)
            layered.alpha_composite(roc_final.render_drone_body("base"))
            self.assertLessEqual(_strongly_changed_pixels(layered, whole), 8, rotor)
        self.assertGreater(
            _changed_pixels(roc_final.render_drone_body("base"), roc_final.render_drone_body("base", True)),
            0,
        )

    def test_the_hatch_and_glow_sit_on_the_hull(self):
        steps = [roc_final.render_hatch("base", k) for k in range(roc_final.HATCH_STEPS)]
        for a, b in pairwise(steps):
            self.assertGreater(_changed_pixels(a, b), 0)
        glow = roc_final.render_glow()
        hull = roc_final._centred(roc_final._hull("base"))
        self.assertGreater(glow.getbbox()[1], 160, "the glow sits on the aft thrusters")
        spill = sum(
            g > 8 and h == 0
            for g, h in zip(
                glow.getchannel("A").get_flattened_data(), hull.getchannel("A").get_flattened_data()
            )
        )
        self.assertEqual(spill, 0, "the glow never spills off the hull")

    def test_roc_layers_fit_the_atlas(self):
        layers = [
            roc_final.render_roc("base"),
            roc_final.render_hull("base"),
            roc_final.render_wreck("base"),
            roc_final.render_wall(),
            roc_final.render_shadow(),
            roc_final.render_yokes(),
            roc_final.render_gun("base"),
            roc_final.render_gun_shadow(),
            roc_final.render_fan(0, "base", 0),
            roc_final.render_clamps(0, "base", 0),
            roc_final.render_drone("base"),
            roc_final.render_drone_shadow(),
            roc_final.render_hatch("base", 0),
            roc_final.render_glow(),
            roc_final.render_cradle("base"),
            roc_final.render_cradle_site("base"),
        ]
        for layer in layers:
            self.assertEqual(layer.width % 8, 0)
            self.assertEqual(layer.height % 8, 0)
        # A gun's canvas is centred on its pivot and holds its flash.
        flash = roc_final.render_gun("base", 0).getbbox()
        self.assertGreater(flash[1], 0)
        self.assertAlmostEqual((flash[0] + flash[2]) / 2, roc_final.GUN / 2, delta=2)


if __name__ == "__main__":
    unittest.main()
