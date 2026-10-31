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
                patch.object(gen_icon, "IOS_OUT", output / "ios"),
                redirect_stdout(StringIO()),
            ):
                gen_icon.main()

            self.assertEqual(
                {path.name for path in (output / "desktop").iterdir()},
                expected_names,
                "the generator must neither omit nor invent packaged icon files",
            )
            self.assertEqual(
                {path.name for path in (output / "ios").iterdir()},
                {"aftermine_1024.png"},
            )
            expected_files = {
                Path("desktop") / name: committed / name for name in expected_names
            }
            expected_files[Path("ios/aftermine_1024.png")] = (
                gen_icon.IOS_OUT / "aftermine_1024.png"
            )
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

    def test_ios_catalog_uses_the_approved_opaque_master(self) -> None:
        self.assertEqual(
            hashlib.sha256(gen_icon.SOURCE.read_bytes()).hexdigest(),
            "08899d0cf3e97da368e82f2568a7946b79d83228a5691f651099e01abd8ee009",
            "the original approved artwork must be preserved byte-for-byte",
        )
        catalog = json.loads((gen_icon.IOS_OUT / "Contents.json").read_text())
        self.assertEqual(len(catalog["images"]), 1)
        entry = catalog["images"][0]
        self.assertEqual(entry["idiom"], "universal")
        self.assertEqual(entry["platform"], "ios")
        self.assertEqual(entry["size"], "1024x1024")
        ios_icon = gen_icon.IOS_OUT / entry["filename"]
        self.assertEqual(
            ios_icon.read_bytes(), (gen_icon.OUT / "aftermine_1024.png").read_bytes()
        )
        with Image.open(ios_icon) as image:
            self.assertEqual(image.mode, "RGB")
            self.assertEqual(image.size, (1024, 1024))
            self.assertEqual(
                hashlib.sha256(image.tobytes()).hexdigest(),
                "02fdfaee4efd570a80b81e1664061b330facecee57e0bd6cec0f7bbb631cc80d",
                "the approved icon artwork must remain unchanged across exports",
            )

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
                patch.object(gen_icon, "IOS_OUT", output / "ios"),
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
