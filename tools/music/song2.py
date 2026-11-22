"""Match song 2: D Dorian at 84 BPM, the loneliest of the set. The raised
sixth (B natural) gives it a wistful colour; melody entries land just after
the beat. Calm: the melody on a bell with long echoes over a quarter-note
pulse. Fight: an octave-bouncing growl bass, a broad kick pattern and
eighth-note hats, sixteenths in the lift."""

import copy
from collections import defaultdict

from . import match_kit as K
from . import song1
from .instruments import Hit
from .theory import grid, m, melody

BPM = 84.0
SEED = 5202
LAYERS = ["calm", "fight"]

A_CHORDS = ["Dmadd9", "G/D", "Fmaj7", "Cadd9", "Dmadd9", "G/D", "Bbmaj7", "Asus4"]
SECTIONS = [
    ("I", ["Dmadd9", "Dmadd9", "G/D", "G/D", "Dmadd9", "G/D", "Em7/D", "Asus4"]),
    ("A", A_CHORDS),
    ("B", ["Em7", "Am7", "Fmaj7", "Cadd9", "Em7", "Am7", "Gm7", "A"]),
    ("C", ["Bbmaj7", "Cadd9", "Dmadd9", "G", "Bbmaj7", "Cadd9", "Em7", "A7sus4"]),
    ("Br", ["Gm/D", "Dmadd9", "Gm/D", "Dmadd9", "Bbmaj7", "Fmaj7", "Gm7", "Asus4"]),
    ("A2", A_CHORDS),
    ("S", K.SIEGE_CHORDS),
    ("F", A_CHORDS),
]

_A_FRONT = ("r:.5 A4:.5 D5:1 E5:1 F5:1 | E5:1.5 D5:.5 B4:2 | r:.5 A4:.5 C5:1 E5:1 F5:1 | G5:1.5 E5:.5 D5:2 |")
MELODY = {
    "I": "r:4 | r:4 | r:4 | r:4 | r:.5 A4:.5 D5:1 E5:1 F5:1 | E5:1.5 D5:.5 B4:2 | r:4 | r:4",
    "A": _A_FRONT + "r:.5 A4:.5 D5:1 E5:1 F5:1 | G5:1.5 A5:.5 B5:2 | A5:1 F5:1 D5:1.5 C5:.5 | E5:1.5 D5:1.5 r:1",
    "A2": _A_FRONT + "r:.5 D5:.5 F5:1 A5:1 G5:1 | B5:1.5 A5:.5 G5:2 | F5:1 A5:1 D6:1.5 C6:.5 | E5:1 D5:3",
    "B": ("B4:1 D5:1 G5:2 | E5:1.5 C5:.5 A4:2 | A4:1 C5:1 F5:1.5 E5:.5 | D5:3 r:1 |"
          "B4:1 D5:1 G5:1.5 B5:.5 | A5:2 G5:1 E5:1 | F5:1 D5:1 Bb4:1.5 D5:.5 | C#5:3 r:1"),
    "C": ("F5:1.5 D5:.5 F5:1 A5:1 | G5:2 E5:1 D5:1 | F5:1.5 E5:.5 D5:1 A4:1 | B4:3 D5:1 |"
          "F5:1.5 D5:.5 F5:1 A5:1 | C6:2 G5:1 E5:1 | D6:1 B5:1 G5:1 E5:1 | G5:1 E5:1 D5:2"),
    "Br": "D5:2 Bb4:2 | A4:4 | D5:2 G5:2 | F5:3 E5:1 | D5:2 F5:2 | E5:2 C5:2 | D5:1 Bb4:1 G4:2 | A4:4",
}


def build(layer):
    fight = layer == "fight"
    ev = defaultdict(list)
    prev = None
    for s, (name, chords) in enumerate(SECTIONS):
        at = s * 32.0
        if name == "S":
            K.siege(ev, layer, at, prev, "straight", low=34)
            continue
        # the final push is arranged like the lift, over A's chords
        kind = "C" if name == "F" else name.rstrip("2")
        pad_vel = {"I": 0.5, "A": 0.55, "B": 0.56, "C": 0.64, "Br": 0.5}[kind]
        voicings = K.pad_part(ev, chords, at, pad_vel, prev)
        prev = voicings[-1]

        # bass
        low = 33 if kind == "Br" else 34
        if fight:
            if kind == "I":
                K.line(ev, "bass", chords, at, "x...............", 0.25, K.bass_tone(low), vel=0.6, gate=0.95,
                       bars=range(0, 4))
                K.line(ev, "bass", chords, at, "x.o.x.o.x.o.x.o.", 0.25, K.bass_tone(low), vel=0.72, gate=0.85,
                       bars=range(4, 8))
            elif kind == "Br":
                K.line(ev, "bass", chords, at, "x.......x.......", 0.25, K.bass_tone(low), vel=0.62, gate=0.95,
                       bars=range(0, 4))
                K.line(ev, "bass", chords, at, "x.o.x.o.x.o.x.o.", 0.25, K.bass_tone(low), vel=0.75, gate=0.85,
                       bars=range(4, 8))
            else:
                K.line(ev, "bass", chords, at, "x.o.x.o.x.o.x.o.", 0.25, K.bass_tone(low), vel=0.82, gate=0.85,
                       accents={0, 8})
        else:
            K.calm_bass(ev, chords, at, low)

        # pulse: quarters that the dotted echo turns into a lilt; eighths in a fight
        first = 2 if kind == "I" else 0
        bars = range(first, 8)
        if fight and kind == "C":
            K.line(ev, "pulse", chords, at, "xxxxxxxxxxxxxxxx", 0.25, K.pulse_tones, order=(0, 0, 1, 0, 0, 2, 0, 1),
                   vel=0.52, accents={0, 4, 8, 12}, bars=bars)
        elif fight:
            K.line(ev, "pulse", chords, at, "x.x.x.x.x.x.x.x.", 0.25, K.pulse_tones, order=(0, 1, 0, 2),
                   vel=0.55, accents={0, 8}, bars=bars)
        else:
            K.line(ev, "pulse", chords, at, "x...x...x...x...", 0.25, K.pulse_tones, order=(0, 1, 0, 2),
                   vel=0.45, accents={0}, gate=0.5, bars=bars)

        if name == "F":
            K.final_push_melody(ev, layer, at, chords, MELODY["A2"])
            drums_for(ev, layer, kind, at)
            continue
        mel = MELODY[name]
        if fight:
            if kind in ("A", "B", "C"):
                ev["lead"] += melody(mel, at, vel=0.8)
                if kind == "C":
                    ev["lead"] += melody(mel, at, vel=0.5, transpose=-12)
                    ev["bell"] += melody(mel, at, vel=0.4, transpose=12)
            elif kind == "Br":
                ev["whistle"] += melody(mel, at, vel=0.62)
        else:
            if kind == "B" or kind == "Br":
                ev["whistle"] += melody(mel, at, vel=0.62)
            else:
                ev["bell"] += melody(mel, at, vel=0.6)

        drums_for(ev, layer, kind, at)
        if fight and kind in ("I", "Br"):
            ev["riser"].append(Hit(at + 24, "riser", 0.6))
    return ev


def drums_for(ev, layer, kind, at):
    d = ev["drums"]
    if layer == "calm":
        for b in range(8):
            t0 = at + b * 4
            if kind in ("A", "B", "C"):
                d += grid("x.......x.......", t0, "kick", 0.42)
            if kind in ("B", "C"):
                d += grid("..x...x...x...x.", t0, "shaker", 0.2, pan=0.3)
        return
    if kind in ("A", "B", "C"):
        d.append(Hit(at, "boom", 0.75))
    for b in range(8):
        t0 = at + b * 4
        last = b == 7
        if kind == "I":
            if b < 4:
                d += grid("x...............", t0, "kick", 0.8)
                if b % 2 == 1:
                    d += [Hit(t0 + 2, "tom", 0.6, 0.1, m("A1")), Hit(t0 + 3, "tom", 0.5, -0.1, m("D2"))]
            elif b < 7:
                d += grid("x.......x.x.....", t0, "kick", 0.82)
                d += grid("x.x.x.x.x.x.x.x.", t0, "hat", 0.34 if b < 6 else 0.4, pan=0.25)
            else:
                d += grid("x.......x.x.....", t0, "kick", 0.84)
                d += [Hit(t0 + k * 0.5, "snare", 0.3 + 0.07 * k, 0.0) for k in range(8)]
            continue
        if kind == "Br":
            if b < 4:
                d += grid("x...............", t0, "kick", 0.8)
                d += grid("........x.......", t0, "snare", 0.7)
            elif b < 7:
                d += grid("x.......x.x.....", t0, "kick", 0.8)
                d += grid("....x.......x...", t0, "snare", 0.75)
                d += grid("x.x.x.x.x.x.x.x.", t0, "hat", 0.36, pan=0.25)
            else:
                d += grid("x.......x.x.....", t0, "kick", 0.85)
                d += [Hit(t0 + k * 0.25, "snare", 0.2 + 0.05 * k, 0.0) for k in range(16)]
            continue
        big = kind == "C"
        d += grid("x.....x.x.x....." if big else "x.......x.x.....", t0, "kick", 0.9)
        if last:
            d += grid("....x.......x...", t0, "snare", 0.88)
            for k, note in enumerate(("A2", "A2", "F2", "D2")):
                d.append(Hit(t0 + 2 + k * 0.5, "tom", 0.6 + 0.06 * k, 0.25 - 0.15 * k, m(note)))
        else:
            d += grid("....x.......x...", t0, "snare", 0.88)
        if big:
            d += grid("xxxxxxxxxxxxxxxx", t0, "hat", 0.24, pan=0.25, accents={0, 4, 8, 12})
            d += grid("....x.......x...", t0, "clap", 0.5)
        else:
            d += grid("x.x.x.x.x.x.x.x.", t0, "hat", 0.34, pan=0.25, accents={2, 6, 10})
        if b % 2 == 1 and not last:
            d += grid("..............x.", t0, "ohat", 0.36, pan=0.25)


# ------------------------------------------------------------------- the mix

TARGET = song1.TARGET
EXCITE = song1.EXCITE
SIDECHAIN = song1.SIDECHAIN
MASTER_EQ = song1.MASTER_EQ
WOBBLE = song1.WOBBLE

PULSE_FX = dict(song1.PULSE_FX, delay=(0.32, 0.75, 1.5, 0.4, 2200.0))
BELL_FX = dict(song1.BELL_FX, delay=(0.3, 0.75, 1.5, 0.42, 3200.0), reverb=(0.55, 4.2, 5500.0, 1.6, 0.03))

VOICES = copy.deepcopy(song1.VOICES)
VOICES["calm"]["pulse"] = ("pluck", dict(song1.PULSE_VOICE, amp_decay=0.3), PULSE_FX, -25.0)
VOICES["calm"]["bell"] = ("bell", dict(ratio=3.5, index=2.3, decay=2.8), BELL_FX, -20.5)
VOICES["fight"]["bell"] = ("bell", dict(ratio=3.5, index=2.0, decay=1.8), song1.BELL_FX, -26.0)

DRUM_GROUPS = copy.deepcopy(song1.DRUM_GROUPS)
DRUM_GROUPS["calm"]["perc"] = (("kick", "shaker"), dict(reverb=song1.ROOM, crush=song1.CRUSH), -30.0)
