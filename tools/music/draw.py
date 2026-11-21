"""Draw: a phrase that comes to rest on a suspended chord and never
resolves, then a loop that keeps hanging."""

from . import quiet_kit as Q
from . import song1

BPM = 72.0
SEED = 8303
LAYERS = ["main"]

SECTIONS = [
    ("I", ["Dmadd9", "Bbmaj7", "Gm7", "Asus4"]),
    ("L", ["Dsus2", "Dsus2", "Bbmaj7", "Bbmaj7", "Gm7/D", "Gm7/D", "Asus4", "Asus4"]),
]
PHRASE = "A4:2 D5:2 | F5:2 D5:2 | D5:2 Bb4:2 | A4:4"
LOOP = "r:4 | r:4 | D5:2 F5:2 | A5:4 | r:4 | r:4 | G5:2 E5:2 | D5:4"
PARTS = {
    "I": [("pad", 0.54), ("bass",), ("bell", PHRASE, 0.56), ("whistle", PHRASE, 0.5)],
    "L": [("pad", 0.48), ("bass",), ("pulse", "quarters", 0), ("bell", LOOP, 0.5)],
}


def build(layer):
    return Q.build(SECTIONS, PARTS)


TARGET = {"main": -19.0}
EXCITE = {"main": song1.EXCITE["calm"]}
MASTER_EQ = song1.MASTER_EQ
WOBBLE = song1.WOBBLE
VOICES = Q.voices()
DRUM_GROUPS = Q.drum_groups()
