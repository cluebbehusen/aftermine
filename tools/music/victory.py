"""Victory: a phrase that rises D-A-D onto a D major chord, then a warm,
settled loop for the results screen."""

from . import quiet_kit as Q
from . import song1

BPM = 72.0
SEED = 8101
LAYERS = ["main"]

SECTIONS = [
    ("I", ["Bbmaj7", "Cadd9", "D", "D"]),
    ("L", ["Dsus2", "C/D", "G/D", "D", "Bm7", "G", "Asus4", "A"]),
]
PHRASE = "D5:2 A5:2 | G5:2 E5:2 | D6:4 | r:4"
LOOP = "r:4 | r:4 | D5:2 A5:2 | F#5:4 | r:4 | r:4 | B4:2 D5:2 | E5:4"
PARTS = {
    "I": [("pad", 0.62), ("bass",), ("whistle", PHRASE, 0.66), ("bell", PHRASE, 0.5),
          ("lead", PHRASE, 0.42, -12), ("kick", "x...x...x...x...", 0.5), ("roll", 1, 0.8), ("boom", 2, 0.85)],
    "L": [("pad", 0.52), ("bass",), ("pulse", "quarters", 0), ("bell", LOOP, 0.52)],
}


def build(layer):
    return Q.build(SECTIONS, PARTS)


TARGET = {"main": -18.0}
EXCITE = {"main": song1.EXCITE["calm"]}
MASTER_EQ = song1.MASTER_EQ
WOBBLE = song1.WOBBLE
VOICES = Q.voices()
DRUM_GROUPS = Q.drum_groups()
