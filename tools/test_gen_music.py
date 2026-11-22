import io
import json
import struct
import tempfile
import unittest
import zlib
from contextlib import redirect_stdout
from pathlib import Path

import numpy as np
import soundfile as sf

from tools import gen_music
from tools.music import render

MUSIC = Path(__file__).resolve().parent.parent / "assets" / "music"


def tone(frames: int, hz: float = 440.0, level: float = 0.3) -> np.ndarray:
    t = np.arange(frames) / gen_music.SR
    mono = level * np.sin(2 * np.pi * hz * t)
    return np.round(np.stack([mono, mono * 0.5], axis=1) * 32767).astype(np.int16)


def raw_ogg(pcm: np.ndarray) -> bytes:
    buffer = io.BytesIO()
    sf.write(buffer, pcm, gen_music.SR, format="OGG", subtype="VORBIS")
    return buffer.getvalue()


def ogg_pages(data: bytes) -> list[tuple[int, int, int, bytes]]:
    """Each page's serial, stored checksum, checksum recomputed with the
    field zeroed, and bytes."""
    pages = []
    offset = 0
    while offset < len(data):
        segments = data[offset + 26]
        end = offset + 27 + segments + sum(data[offset + 27 : offset + 27 + segments])
        page = data[offset:end]
        serial, stored = struct.unpack_from("<I", page, 14)[0], struct.unpack_from("<I", page, 22)[0]
        zeroed = page[:22] + bytes(4) + page[26:]
        pages.append((serial, stored, gen_music.ogg_crc(zeroed), page))
        offset = end
    return pages


class ManifestTests(unittest.TestCase):
    def test_manifest_matches_the_checked_in_bank(self) -> None:
        manifest = gen_music.manifest_bytes()
        self.assertEqual(manifest, (MUSIC / "manifest.json").read_bytes())
        document = json.loads(manifest)
        self.assertEqual(document["sample_rate"], gen_music.SR)
        self.assertEqual(
            [(piece["id"], piece["role"]) for piece in document["pieces"]],
            list(gen_music.PIECES),
        )
        listed = set()
        for piece in document["pieces"]:
            mod = render.piece(piece["id"])
            spans = render.spans(mod)
            self.assertEqual([section["name"] for section in piece["sections"]], [span.name for span in spans])
            for index, (section, span) in enumerate(zip(piece["sections"], spans)):
                self.assertEqual(section["body"], span.end - span.start)
                self.assertEqual(section["frames"], section["body"] + gen_music.secs(gen_music.SLOT_TAIL))
                self.assertEqual(list(section["files"]), mod.LAYERS)
                for layer, path in section["files"].items():
                    self.assertEqual(path, f"{piece['id']}/{index:02}_{span.name}_{layer}.ogg")
                    listed.add(path)
                    info = sf.info(str(MUSIC / path))
                    self.assertEqual((info.samplerate, info.channels), (gen_music.SR, 2))
                    self.assertEqual(info.frames, section["frames"], path)
        self.assertEqual({path.relative_to(MUSIC).as_posix() for path in MUSIC.rglob("*.ogg")}, listed)

    def test_match_repeats_share_their_first_section_kind(self) -> None:
        self.assertEqual(gen_music.section_kind("match", "A2"), "A")
        self.assertEqual(gen_music.section_kind("match", "Br"), "Br")
        self.assertEqual(gen_music.section_kind("menu", "M2"), "M2")


class SectionPcmTests(unittest.TestCase):
    def test_keeps_the_body_and_slot_tail_and_fades_the_end(self) -> None:
        body = 1000
        tail = gen_music.secs(gen_music.SLOT_TAIL)
        fade = gen_music.secs(gen_music.SLOT_FADE)
        clip = np.full((body + gen_music.secs(render.TAIL), 2), 0.5)
        pcm = gen_music.section_pcm(clip, body)
        self.assertEqual(pcm.shape, (body + tail, 2))
        self.assertTrue(np.all(pcm[: body + tail - fade] == 16384))
        self.assertTrue(np.all(np.diff(pcm[-fade:, 0].astype(np.int32)) <= 0))
        self.assertEqual(pcm[-1].tolist(), [0, 0])
        with self.assertRaisesRegex(ValueError, "shorter"):
            gen_music.section_pcm(clip[: body + tail - 1], body)

    def test_rounds_like_libsndfile(self) -> None:
        body = gen_music.secs(2.0)
        rng = np.random.default_rng(7)
        clip = rng.uniform(-0.95, 0.95, (body + gen_music.secs(gen_music.SLOT_TAIL), 2))
        # samples within a hair of a 16-bit step are where truncation, flooring
        # and rounding disagree
        steps = rng.integers(-30000, 30000, 2000) / 32768.0
        clip[:2000, 0] = steps + rng.uniform(-3e-6, 3e-6, 2000) / 32768.0
        clip[:2000, 1] = steps + 0.5 / 32768.0
        wav = io.BytesIO()
        sf.write(wav, clip[:body], gen_music.SR, format="WAV", subtype="PCM_16")
        expected = sf.read(io.BytesIO(wav.getvalue()), dtype="int16")[0]
        np.testing.assert_array_equal(gen_music.section_pcm(clip, body)[:body], expected)


class OggSerialTests(unittest.TestCase):
    def test_checksum_matches_libogg(self) -> None:
        pages = ogg_pages(raw_ogg(tone(gen_music.SR)))
        self.assertGreater(len(pages), 2)
        for _, stored, computed, _ in pages:
            self.assertEqual(computed, stored)

    def test_fixed_serial_makes_encoding_repeatable(self) -> None:
        pcm = tone(gen_music.SR // 2)
        first = gen_music.encode(pcm, "song1/00_I_calm.ogg")
        second = gen_music.encode(pcm, "song1/00_I_calm.ogg")
        self.assertEqual(first, second)
        pages = ogg_pages(first)
        self.assertEqual({serial for serial, _, _, _ in pages}, {zlib.crc32(b"song1/00_I_calm.ogg")})
        for _, stored, computed, _ in pages:
            self.assertEqual(computed, stored)
        np.testing.assert_array_equal(gen_music.decode(first), gen_music.decode(raw_ogg(pcm)))
        self.assertNotEqual(first, gen_music.encode(pcm, "song1/00_I_fight.ogg"))

    def test_rejects_data_that_is_not_one_ogg_stream(self) -> None:
        data = raw_ogg(tone(1000))
        with self.assertRaisesRegex(ValueError, "no Ogg page"):
            gen_music.with_serial(b"RIFF" + data, 1)
        with self.assertRaisesRegex(ValueError, "truncated"):
            gen_music.with_serial(data[:-1], 1)
        chained = gen_music.with_serial(data, 1) + gen_music.with_serial(data, 2)
        with self.assertRaisesRegex(ValueError, "more than one logical stream"):
            gen_music.with_serial(chained, 3)


class CheckRuleTests(unittest.TestCase):
    def test_passes_equal_audio_and_differences_25_db_down(self) -> None:
        expected = tone(gen_music.SR)
        self.assertIsNone(gen_music.audio_difference(expected, expected.copy()))
        noise = np.random.default_rng(3).standard_normal(expected.shape)
        noise *= gen_music.rms(expected) / gen_music.rms(noise)
        for below, passes in ((30.0, True), (20.0, False)):
            actual = np.round(expected + noise * 10 ** (-below / 20)).astype(np.int16)
            with self.subTest(below=below):
                self.assertEqual(gen_music.audio_difference(expected, actual) is None, passes)

    def test_fails_other_lengths_other_notes_and_any_change_to_silence(self) -> None:
        expected = tone(gen_music.SR)
        self.assertIn("decodes to", gen_music.audio_difference(expected, expected[:-1]))
        self.assertIn("dB", gen_music.audio_difference(expected, tone(gen_music.SR, hz=466.16)))
        silence = np.zeros_like(expected)
        nudged = silence.copy()
        nudged[100, 0] = 1
        self.assertIsNotNone(gen_music.audio_difference(silence, nudged))

    def test_check_reports_each_failing_file(self) -> None:
        files = {
            "song1/kept.ogg": gen_music.encode(tone(4000), "song1/kept.ogg"),
            "song1/changed.ogg": gen_music.encode(tone(4000), "song1/changed.ogg"),
            "song1/missing.ogg": gen_music.encode(tone(4000), "song1/missing.ogg"),
        }
        with tempfile.TemporaryDirectory(prefix="aftermine music check ") as temp:
            output = Path(temp)
            with redirect_stdout(io.StringIO()):
                gen_music.emit(output, files, check=False)
                gen_music.emit(output, files, check=True)
            (output / "song1/changed.ogg").write_bytes(gen_music.encode(tone(4000, hz=660.0), "x"))
            (output / "song1/missing.ogg").unlink()
            (output / "song1/extra.ogg").write_bytes(files["song1/kept.ogg"])
            (output / "manifest.json").write_bytes(b"{}\n")
            with self.assertRaises(SystemExit) as failure:
                gen_music.emit(output, files, check=True)
        report = str(failure.exception)
        for line in (
            "manifest.json: stale",
            "song1/extra.ogg: not generated",
            "song1/missing.ogg: missing",
            "song1/changed.ogg: difference is",
        ):
            self.assertIn(line, report)
        self.assertNotIn("kept.ogg", report)


if __name__ == "__main__":
    unittest.main()
