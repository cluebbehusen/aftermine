# /// script
# requires-python = ">=3.14"
# dependencies = ["pillow==12.3.0"]
# ///
"""Generates Home's wordmark: bone letters over a rust band, cut in strata.

Run with `uv run tools/gen_wordmark.py`; `--check` compares the committed image.
"""

import argparse
import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw

OUT = Path(__file__).resolve().parent.parent / "assets" / "wordmark.png"
WORD = "AFTERMINE"
GAP = 0.8
# Pixels per letter unit in the output, and how much finer it is drawn.
UNIT = 24
SUPERSAMPLE = 4
HEIGHT = 10
BONE = (232, 226, 210)
RUST = (196, 87, 59)
# Vertical spans in letter units, split by thin cuts like quarry benches.
BANDS = (
    (0.0, 3.15, BONE),
    (3.4, 6.55, BONE),
    (6.8, 10.0, RUST),
)
# Each letter: its width, outline, and holes, in units on a height of ten.
LETTERS = {
    "A": (
        10,
        [(2.5, 0), (7.5, 0), (10, 2.5), (10, 10), (7, 10), (7, 7.3), (3, 7.3), (3, 10), (0, 10), (0, 2.5)],
        [[(3.5, 2.8), (6.5, 2.8), (7, 3.3), (7, 4.7), (3, 4.7), (3, 3.3)]],
    ),
    "F": (
        8.5,
        [(0, 0), (8.5, 0), (8.5, 2.8), (3, 2.8), (3, 3.9), (7.4, 3.9), (7.4, 6.5), (3, 6.5), (3, 10), (0, 10)],
        [],
    ),
    "T": (9, [(0, 0), (9, 0), (9, 2.8), (6, 2.8), (6, 10), (3, 10), (3, 2.8), (0, 2.8)], []),
    "E": (
        8.5,
        [
            (0, 0),
            (8.5, 0),
            (8.5, 2.8),
            (3, 2.8),
            (3, 3.7),
            (7.6, 3.7),
            (7.6, 6.3),
            (3, 6.3),
            (3, 7.2),
            (8.5, 7.2),
            (8.5, 10),
            (0, 10),
        ],
        [],
    ),
    "R": (
        9.5,
        [(0, 0), (7.5, 0), (9.5, 2), (9.5, 5), (8, 6.4), (9.5, 10), (6.4, 10), (5, 6.9), (3, 6.9), (3, 10), (0, 10)],
        [[(3, 2.6), (6.1, 2.6), (6.6, 3.1), (6.6, 3.9), (6.1, 4.4), (3, 4.4)]],
    ),
    "M": (
        12,
        [(0, 0), (3, 0), (6, 4.2), (9, 0), (12, 0), (12, 10), (9, 10), (9, 4.9), (6, 8.8), (3, 4.9), (3, 10), (0, 10)],
        [],
    ),
    "I": (3, [(0, 0), (3, 0), (3, 10), (0, 10)], []),
    "N": (10, [(0, 0), (3, 0), (7, 5.5), (7, 0), (10, 0), (10, 10), (7, 10), (3, 4.5), (3, 10), (0, 10)], []),
}


def render() -> Image.Image:
    scale = UNIT * SUPERSAMPLE
    units = sum(LETTERS[letter][0] for letter in WORD) + GAP * (len(WORD) - 1)
    size = (round(units * scale), HEIGHT * scale)
    letters = Image.new("L", size, 0)
    draw = ImageDraw.Draw(letters)
    x = 0.0
    for letter in WORD:
        width, outline, holes = LETTERS[letter]
        draw.polygon([((x + a) * scale, b * scale) for a, b in outline], fill=255)
        for hole in holes:
            draw.polygon([((x + a) * scale, b * scale) for a, b in hole], fill=0)
        x += width + GAP
    image = Image.new("RGBA", size, (*BONE, 0))
    for top, bottom, color in BANDS:
        band = Image.new("L", size, 0)
        ImageDraw.Draw(band).rectangle(
            (0, round(top * scale), size[0], round(bottom * scale) - 1), fill=255
        )
        image.paste((*color, 255), mask=ImageChops.multiply(letters, band))
    image = image.resize(
        (size[0] // SUPERSAMPLE, size[1] // SUPERSAMPLE), Image.Resampling.LANCZOS
    )
    # Fully clear pixels keep the letters' color, so smooth scaling in the
    # shell blends edges toward bone and not toward black.
    red, green, blue, alpha = image.split()
    opaque = alpha.point(lambda value: 255 if value else 0)
    color = Image.composite(
        Image.merge("RGB", (red, green, blue)), Image.new("RGB", image.size, BONE), opaque
    )
    return Image.merge("RGBA", (*color.split(), alpha))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="compare the committed image")
    args = parser.parse_args()
    image = render()
    if args.check:
        with Image.open(OUT) as committed:
            if committed.convert("RGBA").tobytes() != image.tobytes():
                print(f"{OUT} differs from its generator", file=sys.stderr)
                return 1
        print("reproducible: wordmark matches")
        return 0
    image.save(OUT, optimize=True)
    print(f"wrote {OUT} ({image.width}x{image.height})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
