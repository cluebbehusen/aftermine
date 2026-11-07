"""Reproducibility and format contracts for the application icon."""

import hashlib
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
from unittest.mock import patch

from PIL import Image

from tools import gen_icon


class IconGenerationTests(unittest.TestCase):
    def test_generation_reproduces_every_checked_in_icon_artifact(self) -> None:
        committed = Path(__file__).resolve().parent.parent / "assets" / "icon"
        expected_names = {
            "aftermine_1024.png",
            "aftermine_desktop_1024.png",
            "aftermine_256.png",
            "aftermine_16.rgba",
            "aftermine_32.rgba",
            "aftermine_64.rgba",
        }

        with tempfile.TemporaryDirectory(prefix="aftermine-icon-test-") as temp:
            output = Path(temp)
            with (
                patch.object(gen_icon, "OUT", output / "desktop"),
                patch.object(gen_icon, "APP_ICON_OUT", output / "apple"),
                patch.object(gen_icon, "TV_CATALOG_OUT", output / "tv"),
                redirect_stdout(StringIO()),
            ):
                gen_icon.main()

            tv_files = {
                path.relative_to(output / "tv")
                for path in (output / "tv").rglob("*")
                if path.is_file()
            }
            self.assertEqual(
                tv_files,
                {
                    path.relative_to(gen_icon.TV_CATALOG_OUT)
                    for path in gen_icon.TV_CATALOG_OUT.rglob("*")
                    if path.is_file()
                },
                "the generator must neither omit nor invent Apple TV catalog files",
            )
            self.assertEqual(
                {path.name for path in (output / "desktop").iterdir()},
                expected_names,
                "the generator must neither omit nor invent packaged icon files",
            )
            app_icon_names = {path.name for path in gen_icon.APP_ICON_OUT.iterdir()}
            self.assertEqual(
                {path.name for path in (output / "apple").iterdir()},
                app_icon_names,
                "the generator must neither omit nor invent app icon files",
            )
            expected_files = {
                Path("desktop") / name: committed / name for name in expected_names
            }
            for name in app_icon_names:
                expected_files[Path("apple") / name] = gen_icon.APP_ICON_OUT / name
            for name in tv_files:
                expected_files[Path("tv") / name] = gen_icon.TV_CATALOG_OUT / name
            for name, expected in expected_files.items():
                with self.subTest(name=str(name)):
                    actual = output / name
                    if name.suffix == ".png":
                        # PNG compression can differ across platforms; the decoded
                        # format and pixels are the reproducibility contract.
                        with (
                            Image.open(actual) as actual_image,
                            Image.open(expected) as expected_image,
                        ):
                            self.assertEqual(actual_image.mode, expected_image.mode)
                            self.assertEqual(actual_image.size, expected_image.size)
                            self.assertEqual(
                                actual_image.tobytes(),
                                expected_image.tobytes(),
                                f"{name} pixels no longer reproduce from tools/gen_icon.py",
                            )
                    else:
                        self.assertEqual(
                            actual.read_bytes(),
                            expected.read_bytes(),
                            f"{name} no longer reproduces from tools/gen_icon.py",
                        )

    def test_ipad_icon_is_the_approved_opaque_master(self) -> None:
        self.assertEqual(
            hashlib.sha256(gen_icon.SOURCE.read_bytes()).hexdigest(),
            "e020b65d4841ed2237354ea8c27fca6559f142bfe14e98495757743429008561",
            "the original approved artwork must be preserved byte-for-byte",
        )
        catalog = json.loads((gen_icon.APP_ICON_OUT / "Contents.json").read_text())
        ios = [entry for entry in catalog["images"] if entry["idiom"] != "mac"]
        self.assertEqual(len(ios), 1)
        entry = ios[0]
        self.assertEqual(entry["idiom"], "universal")
        self.assertEqual(entry["platform"], "ios")
        self.assertEqual(entry["size"], "1024x1024")
        ios_icon = gen_icon.APP_ICON_OUT / entry["filename"]
        self.assertEqual(
            ios_icon.read_bytes(), (gen_icon.OUT / "aftermine_1024.png").read_bytes()
        )
        with Image.open(ios_icon) as image:
            self.assertEqual(image.mode, "RGB")
            self.assertEqual(image.size, (1024, 1024))
            self.assertEqual(
                hashlib.sha256(image.tobytes()).hexdigest(),
                "dd260fff9b4af93442b74353b9257268953ef99209398fbdea211c9045989b07",
                "the approved icon artwork must remain unchanged across exports",
            )

    def test_tv_sources_are_the_approved_art(self) -> None:
        approved = {
            gen_icon.TV_FRONT: "c27f759d3b066802627b8583bbc3fe89f16c2388c3a035a8aac3b0cca987982e",
            gen_icon.TV_BACK: "5b1c5cf2229954911a91b9512b17f6dadcfb3a8b2f638fbefafd538f179f4959",
            gen_icon.TOP_SHELF_SCENE: "59112737e902e2409df76adad8277c209193344e90ff5719800e5357a3c9ac33",
        }
        for path, digest in approved.items():
            with self.subTest(source=path.name):
                self.assertEqual(
                    hashlib.sha256(path.read_bytes()).hexdigest(),
                    digest,
                    "approved Apple TV art must be preserved byte-for-byte",
                )

    def test_tv_catalog_fills_every_tvos_slot(self) -> None:
        brand = gen_icon.TV_CATALOG_OUT / f"{gen_icon.TV_BRAND}.brandassets"
        assets = json.loads((brand / "Contents.json").read_text())["assets"]
        self.assertEqual(
            sorted((asset["role"], asset["size"]) for asset in assets),
            [
                ("primary-app-icon", "1280x768"),
                ("primary-app-icon", "400x240"),
                ("top-shelf-image", "1920x720"),
                ("top-shelf-image-wide", "2320x720"),
            ],
        )
        for asset in assets:
            width, height = (int(v) for v in asset["size"].split("x"))
            folder = brand / asset["filename"]
            if asset["role"] == "primary-app-icon":
                layers = json.loads((folder / "Contents.json").read_text())["layers"]
                with self.subTest(stack=asset["filename"]):
                    self.assertTrue(
                        2 <= len(layers) <= 5, "tvOS icons take 2 to 5 layers"
                    )
                imagesets = [
                    folder / layer["filename"] / "Content.imageset" for layer in layers
                ]
            else:
                imagesets = [folder]
            for depth, imageset in enumerate(imagesets):
                images = json.loads((imageset / "Contents.json").read_text())["images"]
                for entry in images:
                    scale = int(entry["scale"].removesuffix("x"))
                    with (
                        self.subTest(image=f"{imageset.name}/{entry['filename']}"),
                        Image.open(imageset / entry["filename"]) as image,
                    ):
                        self.assertEqual(image.size, (width * scale, height * scale))
                        if depth == len(imagesets) - 1:
                            # The back layer, and every Top Shelf image, is opaque.
                            self.assertEqual(image.mode, "RGB")

    def test_mac_icon_fills_every_slot_with_the_desktop_silhouette(self) -> None:
        catalog = json.loads((gen_icon.APP_ICON_OUT / "Contents.json").read_text())
        mac = [entry for entry in catalog["images"] if entry["idiom"] == "mac"]
        self.assertEqual(
            sorted((entry["size"], entry["scale"]) for entry in mac),
            sorted(
                (f"{points}x{points}", f"{scale}x")
                for points in (16, 32, 128, 256, 512)
                for scale in (1, 2)
            ),
        )
        with Image.open(gen_icon.OUT / "aftermine_desktop_1024.png") as desktop:
            for entry in mac:
                with self.subTest(size=entry["size"], scale=entry["scale"]):
                    points = int(entry["size"].split("x")[0])
                    pixels = points * int(entry["scale"].removesuffix("x"))
                    with Image.open(gen_icon.APP_ICON_OUT / entry["filename"]) as icon:
                        self.assertEqual(icon.mode, "RGBA")
                        self.assertEqual(icon.size, (pixels, pixels))
                        if pixels == 1024:
                            self.assertEqual(icon.tobytes(), desktop.tobytes())

    def test_desktop_mask_preserves_artwork_and_approved_corners(self) -> None:
        with (
            Image.open(gen_icon.OUT / "aftermine_1024.png") as master,
            Image.open(gen_icon.OUT / "aftermine_desktop_1024.png") as desktop,
        ):
            self.assertEqual(desktop.mode, "RGBA")
            self.assertEqual(desktop.size, master.size)
            self.assertEqual(desktop.convert("RGB").tobytes(), master.tobytes())
            self.assertEqual(
                hashlib.sha256(desktop.getchannel("A").tobytes()).hexdigest(),
                "016346cb3aa77ef1b424525ead9507d067738f3a914482faebdeb3a4f1b912b3",
                "desktop exports must retain the approved corner silhouette",
            )

    def test_raw_window_icons_have_exact_rgba_dimensions(self) -> None:
        with tempfile.TemporaryDirectory(prefix="aftermine-icon-format-") as temp:
            output = Path(temp)
            with (
                patch.object(gen_icon, "OUT", output / "desktop"),
                patch.object(gen_icon, "APP_ICON_OUT", output / "apple"),
                patch.object(gen_icon, "TV_CATALOG_OUT", output / "tv"),
                redirect_stdout(StringIO()),
            ):
                gen_icon.main()

            for size in (16, 32, 64):
                with self.subTest(size=size):
                    raw = (output / "desktop" / f"aftermine_{size}.rgba").read_bytes()
                    self.assertEqual(len(raw), size * size * 4)
                    image = Image.frombytes("RGBA", (size, size), raw)
                    alpha = image.getchannel("A")
                    self.assertEqual(alpha.getpixel((0, 0)), 0)
                    self.assertEqual(alpha.getpixel((size // 2, size // 2)), 255)


if __name__ == "__main__":
    unittest.main()
