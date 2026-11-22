"""Defeat: the falling A to E-flat, settling on a low D, then a dark,
sparse loop with a distant knell."""

from . import quiet_kit as Q
from . import song1

BPM = 66.0
SEED = 8202
LAYERS = ["main"]

SECTIONS = [
    ("I", ["Dm", "Ebmaj7/D", "Dm", "Dm"]),
    ("L", ["Dm", "Bb/D", "Gm/D", "Dm", "Dm", "Bb/D", "Gm/D", "Asus4"]),
]
PHRASE = "A4:4 | Eb4:4 | D4:4 | r:4"
LOOP = "r:4 | r:4 | D5:2 Bb4:2 | A4:4 | r:4 | r:4 | G4:2 Bb4:2 | A4:4"
PARTS = {
    "I": [("pad", 0.52), ("bass",), ("whistle", PHRASE, 0.64), ("bell", PHRASE, 0.38), ("boom", 0, 0.8),
          ("knell", 2, 0.45)],
    "L": [("pad", 0.46), ("bass",), ("whistle", LOOP, 0.56), ("knell", 2, 0.3)],
}


def build(layer):
    return Q.build(SECTIONS, PARTS)


TARGET = {"main": -19.0}
EXCITE = {"main": song1.EXCITE["calm"]}
MASTER_EQ = song1.MASTER_EQ
WOBBLE = song1.WOBBLE
VOICES = Q.voices()
DRUM_GROUPS = Q.drum_groups()
