"""Match song 4: D minor in a rolling triplet feel at 76 BPM, the heaviest of
the set. The A section walks a lament bass down from D to E; the lift uses
the falling D-C-Bb-A cadence. Calm: a lilting long-short pulse under a thin
voice. Fight: a triplet shuffle groove, a long-short growl bass with octave
pops, triplet hats."""

import copy
from collections import defaultdict

from . import match_kit as K
from . import song1
from .instruments import Hit
from .theory import grid, m, melody

BPM = 76.0
SEED = 7404
LAYERS = ["calm", "fight"]
T = 1 / 3

A_CHORDS = ["Dm", "Am/C", "Bbmaj7", "F/A", "Gm", "Dm/F", "A/E", "A"]
SECTIONS = [
    ("I", ["Dm", "Dm", "Bb/D", "Bb/D", "Dm", "Dm", "C/D", "Asus4"]),
    ("A", A_CHORDS),
    ("B", ["Bbmaj7", "C", "Am7", "Dm", "Gm7", "C", "Fmaj7", "A"]),
    ("C", ["Dm", "C", "Bbmaj7", "A", "Dm", "C", "Gm", "A"]),
    ("Br", ["Fmaj7", "Fmaj7", "Cadd9/E", "Cadd9/E", "Dm", "Dm", "Bbmaj7", "Asus4"]),
    ("A2", A_CHORDS),
    ("S", K.SIEGE_CHORDS),
    ("F", A_CHORDS),
]

_HOOK = "A4:2/3 D5:1/3 F5:1 E5:2/3 D5:1/3 A4:1 |"
_A_FRONT = _HOOK + "C5:2 E5:2/3 C5:1/3 A4:1 | D5:2/3 F5:1/3 A5:2 G5:2/3 F5:1/3 | F5:2 E5:2/3 F5:1/3 C5:1 |"
MELODY = {
    "I": "r:4 | r:4 | r:4 | r:4 |" + _HOOK + "A4:4 | r:4 | r:4",
    "A": _A_FRONT + ("Bb4:2/3 D5:1/3 G5:2 F5:2/3 D5:1/3 | F5:2 A5:2/3 F5:1/3 D5:1 |"
                     "E5:2/3 C#5:1/3 A4:2 C#5:1 | E5:3 r:1"),
    "A2": _A_FRONT + ("D5:2/3 G5:1/3 Bb5:2 A5:2/3 G5:1/3 | A5:2 F5:2/3 A5:1/3 D6:1 |"
                      "C#6:2/3 A5:1/3 E5:2 G5:1 | E5:1 C#5:3"),
    "B": ("F5:2 D5:2/3 F5:1/3 A5:1 | G5:2 E5:2/3 G5:1/3 C6:1 | C6:2/3 B5:1/3 A5:2 E5:1 | F5:3 r:1 |"
          "D5:2 Bb4:2/3 D5:1/3 G5:1 | E5:2 G5:2/3 E5:1/3 C5:1 | A4:2/3 C5:1/3 F5:2 E5:1 | C#5:3 r:1"),
    "C": ("A5:2/3 D6:1/3 D6:2 C6:2/3 A5:1/3 | G5:2/3 C6:1/3 C6:2 Bb5:2/3 G5:1/3 | F5:2/3 Bb5:1/3 A5:2 F5:1 |"
          "E5:3 C#5:1 | A5:2/3 D6:1/3 D6:2 C6:2/3 A5:1/3 | G5:2/3 C6:1/3 E6:2 C6:2/3 G5:1/3 |"
          "D6:2/3 Bb5:1/3 G5:2 Bb5:1 | A5:3 r:1"),
    "Br": "A5:3 C6:1 | E5:4 | G5:3 D5:1 | E5:4 | F5:3 A5:1 | D5:4 | D5:2 F5:2 | E5:4",
}

SHUFFLE = "x.xx.xx.xx.x"


def build(layer):
    fight = layer == "fight"
    ev = defaultdict(list)
    prev = None
    for s, (name, chords) in enumerate(SECTIONS):
        at = s * 32.0
        if name == "S":
            K.siege(ev, layer, at, prev, "triplet", low=33)
            continue
        # the final push is arranged like the lift, over A's chords
        kind = "C" if name == "F" else name.rstrip("2")
        pad_vel = {"I": 0.5, "A": 0.56, "B": 0.56, "C": 0.66, "Br": 0.5}[kind]
        voicings = K.pad_part(ev, chords, at, pad_vel, prev)
        prev = voicings[-1]

        # the lament walks down to A1 and climbs back from G2; elsewhere the usual range
        low = 33
        if fight:
            if kind == "I":
                K.line(ev, "bass", chords, at, "x...........", T, K.bass_tone(low), vel=0.62, gate=0.95,
                       bars=range(0, 4))
                K.line(ev, "bass", chords, at, "x..x..x..x..", T, K.bass_tone(low), vel=0.74, gate=0.85,
                       bars=range(4, 8))
            elif kind == "Br":
                K.line(ev, "bass", chords, at, "x.....x.....", T, K.bass_tone(low), vel=0.64, gate=0.95,
                       bars=range(0, 4))
                K.line(ev, "bass", chords, at, "x.xo.xx.xo.x", T, K.bass_tone(low), vel=0.76, gate=0.8,
                       bars=range(4, 8))
            else:
                K.line(ev, "bass", chords, at, "x.xo.xx.xo.x", T, K.bass_tone(low), vel=0.84, gate=0.8,
                       accents={0, 6})
        else:
            K.calm_bass(ev, chords, at, low)

        first = 2 if kind == "I" else 0
        bars = range(first, 8)
        if fight:
            K.line(ev, "pulse", chords, at, "xxxxxxxxxxxx", T, K.pulse_tones, order=(0, 1, 2, 1, 2, 1),
                   vel=0.5, accents={0, 3, 6, 9}, bars=bars)
        else:
            K.line(ev, "pulse", chords, at, SHUFFLE, T, K.pulse_tones, order=(0, 1, 0, 2),
                   vel=0.42, accents={0, 6}, gate=0.7, bars=bars)

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
                    ev["whistle"] += melody(mel, at, vel=0.4)
            elif kind == "Br":
                ev["whistle"] += melody(mel, at, vel=0.62)
        else:
            if kind in ("B", "Br"):
                ev["bell"] += melody(mel, at, vel=0.55)
            else:
                ev["whistle"] += melody(mel, at, vel=0.62)

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
                d += grid("x.....x.....", t0, "kick", 0.42, step=T)
            if kind in ("B", "C"):
                d += grid("..x..x..x..x", t0, "shaker", 0.18, pan=0.3, step=T)
        return
    if kind in ("A", "B", "C"):
        d.append(Hit(at, "boom", 0.78))
    for b in range(8):
        t0 = at + b * 4
        last = b == 7
        if kind == "I":
            if b < 4:
                d += [Hit(t0, "tom", 0.72, -0.1, m("D2")), Hit(t0 + 5 * T, "tom", 0.5, 0.15, m("A1")),
                      Hit(t0 + 2, "tom", 0.62, 0.1, m("D2"))]
            elif b < 7:
                d += grid("x.....x.....", t0, "kick", 0.82, step=T)
                d += grid("..x..x..x..x", t0, "hat", 0.3 if b < 6 else 0.38, pan=0.25, step=T)
            else:
                d += grid("x.....x.....", t0, "kick", 0.84, step=T)
                d += [Hit(t0 + 2 + k * T, "snare", 0.3 + 0.08 * k, 0.0) for k in range(6)]
            continue
        if kind == "Br":
            if b < 4:
                d += grid("x...........", t0, "kick", 0.8, step=T)
                d += grid("......x.....", t0, "snare", 0.7, step=T)
            elif b < 7:
                d += grid("x....xx.....", t0, "kick", 0.8, step=T)
                d += grid("...x.....x..", t0, "snare", 0.75, step=T)
                d += grid("x.xx.xx.xx.x", t0, "hat", 0.32, pan=0.25, step=T)
            else:
                d += grid("x....xx.....", t0, "kick", 0.85, step=T)
                d += [Hit(t0 + k * T, "snare", 0.22 + 0.06 * k, 0.0) for k in range(12)]
            continue
        big = kind == "C"
        d += grid("x.x..xx.x..x" if big else "x....xx.....", t0, "kick", 0.9, step=T)
        d += grid("...x.....x..", t0, "snare", 0.88, step=T)
        if last:
            for k, note in enumerate(("D3", "C3", "A2", "F2", "D2")):
                d.append(Hit(t0 + (7 + k) * T, "tom", 0.55 + 0.06 * k, 0.3 - 0.15 * k, m(note)))
        d += grid("xxxxxxxxxxxx", t0, "hat", 0.24, pan=0.25, accents={0, 3, 6, 9}, step=T)
        if big:
            d += grid("...x.....x..", t0, "clap", 0.5, step=T)
            if not last:
                d += grid("...........x", t0, "ohat", 0.36, pan=0.25, step=T)


# ------------------------------------------------------------------- the mix

TARGET = song1.TARGET
EXCITE = song1.EXCITE
SIDECHAIN = song1.SIDECHAIN
MASTER_EQ = song1.MASTER_EQ
WOBBLE = song1.WOBBLE

VOICES = copy.deepcopy(song1.VOICES)
VOICES["calm"]["pulse"] = ("pluck", dict(song1.PULSE_VOICE, amp_decay=0.26), song1.PULSE_FX, -25.0)
VOICES["calm"]["bell"] = ("bell", dict(ratio=3.5, index=2.3, decay=2.6), song1.BELL_FX, -21.0)

DRUM_GROUPS = copy.deepcopy(song1.DRUM_GROUPS)
DRUM_GROUPS["calm"]["perc"] = (("kick", "shaker"), dict(reverb=song1.ROOM, crush=song1.CRUSH), -30.0)
