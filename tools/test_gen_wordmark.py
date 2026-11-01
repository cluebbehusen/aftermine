import unittest

from tools import gen_wordmark


class WordmarkTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.image = gen_wordmark.render()

    def test_is_large_enough_for_a_large_monitor_at_retina_density(self) -> None:
        self.assertGreaterEqual(self.image.width, 1700)

    def test_its_art_touches_every_edge_so_callers_can_align_it(self) -> None:
        bbox = self.image.getchannel("A").getbbox()
        self.assertEqual(bbox, (0, 0, self.image.width, self.image.height))

    def test_bone_sits_over_rust(self) -> None:
        x = round(gen_wordmark.UNIT * 1.5)
        top = self.image.getpixel((x, round(gen_wordmark.UNIT * 4)))
        bottom = self.image.getpixel((x, round(gen_wordmark.UNIT * 9)))
        self.assertEqual(top, (*gen_wordmark.BONE, 255))
        self.assertEqual(bottom, (*gen_wordmark.RUST, 255))


if __name__ == "__main__":
    unittest.main()
