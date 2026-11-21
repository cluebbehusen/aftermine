"""The menu theme: song 1's melody at 80 BPM on the thin whistle and the
bell over the muted pulse."""

from . import quiet_kit as Q
from . import song1

BPM = 80.0
SEED = 2212
LAYERS = ["main"]

SECTIONS = [
    ("M1", song1.SECTIONS[0][1]),
    ("M2", song1.PEDAL_A),
    ("M3", song1.SECTIONS[4][1]),
    ("M4", song1.DARK_C),
]
PARTS = {
    "M1": [("pad", 0.5), ("bass",), ("pulse", "quarters", 4), ("whistle", song1.MELODY["I"], 0.6)],
    "M2": [("pad", 0.54), ("bass",), ("pulse", "eighths", 0), ("whistle", song1.MELODY["A"], 0.62)],
    "M3": [("pad", 0.5), ("bass",), ("pulse", "quarters", 0), ("bell", song1.MELODY["Br"], 0.56), ("knell", 2, 0.4)],
    "M4": [("pad", 0.6), ("bass",), ("pulse", "eighths", 0), ("whistle", song1.MELODY["C"], 0.62),
           ("harmony", "bell", song1.MELODY["C"], 0.42), ("kick", "x.......x.......", 0.4), ("hat", "..x...x...x...x.", 0.18)],
}


def build(layer):
    return Q.build(SECTIONS, PARTS)


TARGET = {"main": -19.0}
EXCITE = {"main": song1.EXCITE["calm"]}
MASTER_EQ = song1.MASTER_EQ
WOBBLE = song1.WOBBLE
VOICES = Q.voices()
DRUM_GROUPS = Q.drum_groups()
