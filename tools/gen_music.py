# /// script
# requires-python = "==3.12.*"
# dependencies = [
#   "llvmlite==0.50.0",
#   "numba==0.68.1",
#   "numpy==2.5.4",
#   "pyloudnorm==0.2.0",
#   "scipy==1.18.1",
#   "soundfile==0.14.0",
# ]
# ///
"""Generates every music file in assets/music/.

Same contract as gen_sounds.py. This script and the tools/music/ package are
the source of truth; edit them, run `uv run tools/gen_music.py`, and commit
the code and the Ogg files together.

Each piece renders section by section in memory, and every section of every
layer becomes one stereo Ogg Vorbis file at libsndfile's default quality,
with a fixed stream serial so a rerun on the same machine writes the same
bytes. manifest.json lists each section's kind, its body (where the next
section starts) and its full length with the tail, in frames at 44.1 kHz.

--check renders everything again and requires manifest.json to match exactly.
An Ogg file passes when it decodes to the same 16-bit audio as a fresh
encode, or, because libvorbis and libm round differently across platforms,
to audio of the same length whose difference is at least 25 dB below the
checked-in file's RMS level. The encoder is chaotic: nudging a thousandth of
a file's samples by one step moves its decoded audio to about 33 dB below
the level, so a stricter floor fails on another platform's rounding.

Pass --out DIRECTORY to write a complete alternate bank for review without
touching the checked-in assets.
"""

import argparse
import io
import json
import math
import os
import re
import struct
import sys
import zlib
from concurrent.futures import ProcessPoolExecutor
from multiprocessing import get_context
from pathlib import Path

import numpy as np
import soundfile as sf

if __package__ in (None, ""):
    # Run as a script: make the `tools` package importable.
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from tools.music import render
from tools.music.dsp import SR, secs

OUT = Path(__file__).resolve().parent.parent / "assets" / "music"

# Each piece's id, which is also its module in tools/music/, and its role.
PIECES = (
    ("song1", "match"),
    ("song2", "match"),
    ("song3", "match"),
    ("song4", "match"),
    ("menu", "menu"),
    ("victory", "victory"),
    ("defeat", "defeat"),
    ("draw", "draw"),
)
# Seconds of rendered tail each section's file keeps; its last SLOT_FADE
# seconds fade to silence.
SLOT_TAIL = 6.0
SLOT_FADE = 0.5
CHECK_FLOOR_DB = 25.0


def section_path(piece: str, index: int, name: str, layer: str) -> str:
    return f"{piece}/{index:02}_{name}_{layer}.ogg"


def section_kind(role: str, name: str) -> str:
    """Repeats of a match section, such as A2, play as their first section."""
    return re.sub(r"\d+$", "", name) if role == "match" else name


def section_pcm(clip: np.ndarray, body: int) -> np.ndarray:
    """A section file's audio: the body and SLOT_TAIL of the rendered tail,
    faded out at the end, as 16-bit PCM. Samples round the way libsndfile
    writes floating point to a 16-bit file: to 32 bits, keeping the top 16."""
    frames = body + secs(SLOT_TAIL)
    if len(clip) < frames:
        raise ValueError(f"clip of {len(clip)} frames is shorter than its section's {frames}")
    pcm = clip[:frames].copy()
    fade = secs(SLOT_FADE)
    pcm[-fade:] *= np.linspace(1, 0, fade)[:, None]
    return (np.rint(pcm * 2.0**31).astype(np.int64) >> 16).astype(np.int16)


def manifest_bytes() -> bytes:
    pieces = []
    for piece, role in PIECES:
        mod = render.piece(piece)
        sections = []
        for index, span in enumerate(render.spans(mod)):
            body = span.end - span.start
            sections.append(
                {
                    "name": span.name,
                    "kind": section_kind(role, span.name),
                    "body": body,
                    "frames": body + secs(SLOT_TAIL),
                    "files": {layer: section_path(piece, index, span.name, layer) for layer in mod.LAYERS},
                }
            )
        pieces.append({"id": piece, "role": role, "sections": sections})
    document = {"sample_rate": SR, "pieces": pieces}
    return (json.dumps(document, indent=1) + "\n").encode()


def _ogg_crc_table() -> tuple[int, ...]:
    table = []
    for byte in range(256):
        crc = byte << 24
        for _ in range(8):
            crc = (crc << 1) ^ 0x04C11DB7 if crc & 0x80000000 else crc << 1
        table.append(crc & 0xFFFFFFFF)
    return tuple(table)


OGG_CRC_TABLE = _ogg_crc_table()


def ogg_crc(data: bytes | bytearray) -> int:
    """The Ogg page checksum: CRC-32 with polynomial 0x04C11DB7, not
    reflected, initial value and final XOR zero."""
    crc = 0
    for byte in data:
        crc = ((crc << 8) & 0xFFFFFFFF) ^ OGG_CRC_TABLE[(crc >> 24) ^ byte]
    return crc


def with_serial(data: bytes, serial: int) -> bytes:
    """Rewrites the stream serial number that libsndfile picks at random in
    every page of a single-stream Ogg file, and each page's checksum."""
    out = bytearray(data)
    original = None
    offset = 0
    while offset < len(out):
        if out[offset : offset + 4] != b"OggS" or offset + 27 > len(out):
            raise ValueError(f"no Ogg page at byte {offset}")
        segments = out[offset + 26]
        end = offset + 27 + segments + sum(out[offset + 27 : offset + 27 + segments])
        if end > len(out):
            raise ValueError(f"Ogg page at byte {offset} is truncated")
        page_serial = out[offset + 14 : offset + 18]
        if original is None:
            original = bytes(page_serial)
        elif page_serial != original:
            raise ValueError("Ogg file holds more than one logical stream")
        out[offset + 14 : offset + 18] = struct.pack("<I", serial)
        out[offset + 22 : offset + 26] = bytes(4)
        out[offset + 22 : offset + 26] = struct.pack("<I", ogg_crc(out[offset:end]))
        offset = end
    if original is None:
        raise ValueError("no Ogg pages")
    return bytes(out)


def decode(data: bytes) -> np.ndarray:
    return sf.read(io.BytesIO(data), dtype="int16", always_2d=True)[0]


def encode(pcm: np.ndarray, path: str) -> bytes:
    """Ogg Vorbis at libsndfile's default quality, its serial fixed by `path`."""
    buffer = io.BytesIO()
    sf.write(buffer, pcm, SR, format="OGG", subtype="VORBIS")
    data = with_serial(buffer.getvalue(), zlib.crc32(path.encode()))
    decoded = decode(data)
    if decoded.shape != pcm.shape:
        raise ValueError(f"{path} decodes to {decoded.shape}, expected {pcm.shape}")
    return data


def render_layer(piece: str, layer: str) -> dict[str, bytes]:
    """One layer of a piece, a finished Ogg file per section."""
    mod = render.piece(piece)
    files = {}
    for index, (span, clip) in enumerate(zip(render.spans(mod), render.render(mod, layer))):
        path = section_path(piece, index, span.name, layer)
        files[path] = encode(section_pcm(clip, span.end - span.start), path)
    return files


def generate_bank() -> dict[str, bytes]:
    """Every music file by path. Each layer renders in its own process."""
    jobs = [(piece, layer) for piece, _ in PIECES for layer in render.piece(piece).LAYERS]
    files: dict[str, bytes] = {}
    workers = min(len(jobs), os.cpu_count() or 1)
    with ProcessPoolExecutor(workers, mp_context=get_context("spawn")) as pool:
        for (piece, layer), result in zip(jobs, pool.map(render_layer, *zip(*jobs))):
            files.update(result)
            size = sum(len(data) for data in result.values())
            print(f"  {piece}/{layer}: {len(result)} sections, {size / 1e6:.1f} MB")
    return files


def rms(samples: np.ndarray) -> float:
    return math.sqrt(float(np.mean(np.square(samples, dtype=np.float64)))) if samples.size else 0.0


def audio_difference(expected: np.ndarray, actual: np.ndarray) -> str | None:
    """None when `actual` passes for the checked-in `expected`, else why not."""
    if expected.shape != actual.shape:
        return f"decodes to {actual.shape}, checked in as {expected.shape}"
    if np.array_equal(expected, actual):
        return None
    signal = rms(expected)
    noise = rms(actual.astype(np.float64) - expected)
    if noise <= signal * 10 ** (-CHECK_FLOOR_DB / 20):
        return None
    below = 20 * math.log10(signal / noise) if signal > 0 else -math.inf
    return f"difference is {below:.1f} dB below the checked-in level, not {CHECK_FLOOR_DB:.0f}"


def emit(output: Path, files: dict[str, bytes], check: bool) -> None:
    manifest = manifest_bytes()
    if check:
        failures = []
        manifest_path = output / "manifest.json"
        if not manifest_path.exists() or manifest_path.read_bytes() != manifest:
            failures.append("manifest.json: stale")
        actual = {path.relative_to(output).as_posix() for path in output.rglob("*.ogg")}
        failures += [f"{path}: not generated" for path in sorted(actual - set(files))]
        for path, data in files.items():
            if path not in actual:
                failures.append(f"{path}: missing")
                continue
            problem = audio_difference(decode((output / path).read_bytes()), decode(data))
            if problem:
                failures.append(f"{path}: {problem}")
        if failures:
            raise SystemExit("music bank is stale:\n  " + "\n  ".join(failures))
        print(f"checked {len(files)} Ogg files and manifest")
        return
    for path, data in files.items():
        target = output / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    (output / "manifest.json").write_bytes(manifest)
    print(f"wrote {len(files)} Ogg files and manifest")


def generate(output: Path, check: bool = False) -> None:
    """Generates or checks the complete music bank at `output`."""
    print(f"{'checking' if check else 'writing'} {output}")
    emit(output, generate_bank(), check)
    print("done")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    destination = parser.add_mutually_exclusive_group()
    destination.add_argument(
        "--check",
        action="store_true",
        help="verify the checked-in music bank without rewriting it",
    )
    destination.add_argument(
        "--out",
        type=Path,
        metavar="DIRECTORY",
        help="write the complete music bank to DIRECTORY instead of assets/music",
    )
    args = parser.parse_args()
    generate(args.out if args.out is not None else OUT, check=args.check)


if __name__ == "__main__":
    main()
