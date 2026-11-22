"""Match song 3: D minor with a Phrygian E-flat at 104 BPM, the most driven
of the set. Everything leans on one 3-3-2 rhythm: the melody's riff, the
pulse, the fight bass and the kick. Calm: a muted sixteenth-note clockwork
under a thin voice. Fight: a 3-3-2 growl bass with octave pops, toms in the
fills, a full sixteenth pulse."""

import copy
from collections import defaultdict

from . import match_kit as K
from . import song1
from .instruments import Hit
from .theory import grid, m, melody

BPM = 104.0
SEED = 6303
LAYERS = ["calm", "fight"]

A_CHORDS = ["Dm", "Ebmaj7/D", "Dm", "C", "Dm", "Ebmaj7/D", "Bb", "Asus4"]
C_CHORDS = ["Dm", "C", "Bb", "C", "Dm", "C", "Bbmaj7", "A"]
SECTIONS = [
    ("I", ["Dm", "Dm", "Ebmaj7/D", "Ebmaj7/D", "Dm", "Dm", "Ebmaj7/D", "Asus4"]),
    ("A", A_CHORDS),
    ("B", ["Gm", "Bb", "C", "Dm", "Gm", "Bb", "C", "Asus4"]),
    ("C", C_CHORDS),
    ("Br", ["Dm", "Dm", "Ebmaj7/D", "Ebmaj7/D", "Gm/D", "Gm/D", "Asus4", "A"]),
    ("A2", A_CHORDS),
    ("C2", C_CHORDS),
    ("S", K.SIEGE_CHORDS),
    ("F", A_CHORDS),
]

_RIFF = "D5:.75 F5:.75 A5:.5 G5:1 F5:1 | G5:.75 F5:.75 Eb5:.5 D5:2 |"
_A_FRONT = _RIFF + "D5:.75 F5:.75 A5:.5 C6:1 A5:1 | G5:1.5 E5:.5 C5:2 |"
_C = ("A5:.75 A5:.75 D6:.5 C6:1 A5:1 | G5:.75 G5:.75 C6:.5 Bb5:1 G5:1 | F5:1.5 Bb5:.5 A5:1 F5:1 | E5:2 G5:2 |"
      "A5:.75 A5:.75 D6:.5 C6:1 A5:1 | G5:.75 G5:.75 C6:.5 E6:1 C6:1 | D6:1.5 C6:.5 A5:1 F5:1 | E5:2 C#5:2")
MELODY = {
    "I": "r:4 | r:4 | r:4 | r:4 |" + _RIFF + "r:4 | r:4",
    "A": _A_FRONT + "D5:.75 F5:.75 A5:.5 G5:1 F5:1 | Bb5:.75 G5:.75 Eb5:.5 D5:2 | F5:1 D5:1 Bb4:1 D5:1 | E5:2 D5:1 A4:1",
    "A2": _A_FRONT + "D6:.75 C6:.75 A5:.5 G5:1 F5:1 | G5:.75 Bb5:.75 G5:.5 D5:2 | F5:1 Bb5:1 D6:1 Bb5:1 | A5:3 r:1",
    "B": ("G4:.75 Bb4:.75 D5:.5 C5:1 Bb4:1 | D5:1.5 F5:.5 D5:2 | E5:.75 G5:.75 C6:.5 Bb5:1 G5:1 | A5:3 r:1 |"
          "G4:.75 Bb4:.75 D5:.5 C5:1 Bb4:1 | F5:1.5 D5:.5 Bb4:2 | C5:.75 E5:.75 G5:.5 F5:1 E5:1 | D5:4"),
    "C": _C,
    "C2": _C,
    "Br": "D5:3 F5:1 | A5:4 | G5:3 Bb5:1 | Bb5:2 G5:2 | D6:3 Bb5:1 | A5:2 G5:2 | D5:4 | C#5:2 E5:2",
}

TRESILLO = "x..x..x.x..x..x."


def build(layer):
    fight = layer == "fight"
    ev = defaultdict(list)
    prev = None
    for s, (name, chords) in enumerate(SECTIONS):
        at = s * 32.0
        if name == "S":
            K.siege(ev, layer, at, prev, "tresillo", low=34)
            continue
        # the final push is arranged like the lift, over A's chords
        kind = "C" if name == "F" else name.rstrip("2")
        pad_vel = {"I": 0.48, "A": 0.52, "B": 0.55, "C": 0.62, "Br": 0.5}[kind]
        voicings = K.pad_part(ev, chords, at, pad_vel, prev)
        prev = voicings[-1]

        low = 34
        if fight:
            if kind == "I":
                K.line(ev, "bass", chords, at, "x..x..x.........", 0.25, K.bass_tone(low), vel=0.66, gate=0.9,
                       bars=range(0, 4))
                K.line(ev, "bass", chords, at, "x..x..o.x..x..o.", 0.25, K.bass_tone(low), vel=0.75, gate=0.8,
                       bars=range(4, 8))
            elif kind == "Br":
                K.line(ev, "bass", chords, at, "x...............", 0.25, K.bass_tone(low), vel=0.62, gate=0.97,
                       bars=range(0, 4))
                K.line(ev, "bass", chords, at, "x..x..o.x..x..o.", 0.25, K.bass_tone(low), vel=0.76, gate=0.8,
                       bars=range(4, 8))
            else:
                K.line(ev, "bass", chords, at, "x..x..o.x..x..o.", 0.25, K.bass_tone(low), vel=0.84, gate=0.8,
                       accents={0, 8})
        else:
            K.calm_bass(ev, chords, at, low)

        first = 2 if kind == "I" else 0
        bars = range(first, 8)
        if fight:
            K.line(ev, "pulse", chords, at, "xxxxxxxxxxxxxxxx", 0.25, K.pulse_tones, order=(0, 0, 1, 0, 0, 2, 0, 1),
                   vel=0.5, accents={0, 3, 6, 8, 11, 14}, accent=0.14, bars=bars)
        else:
            K.line(ev, "pulse", chords, at, TRESILLO, 0.25, K.pulse_tones, order=(0, 1, 0, 2, 0, 1),
                   vel=0.4, accents={0, 8}, gate=0.6, bars=bars)

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
            ev["riser"].append(Hit(at + 24, "riser", 0.62))
    return ev


def drums_for(ev, layer, kind, at):
    d = ev["drums"]
    if layer == "calm":
        for b in range(8):
            t0 = at + b * 4
            if kind in ("A", "B", "C"):
                d += grid("x..x..x.........", t0, "kick", 0.4)
            if kind in ("B", "C"):
                d += grid("xxxxxxxxxxxxxxxx", t0, "shaker", 0.11, pan=0.3, accents={0, 3, 6, 8, 11, 14})
        return
    if kind in ("A", "B", "C"):
        d.append(Hit(at, "boom", 0.75))
    for b in range(8):
        t0 = at + b * 4
        last = b == 7
        if kind == "I":
            if b < 4:
                for k, pos in enumerate((0, 3, 6)):
                    d.append(Hit(t0 + pos * 0.25, "tom", 0.7 - 0.08 * k, -0.15 + 0.15 * k, m("D2")))
            elif b < 7:
                d += grid("x..x..x.x..x..x.", t0, "kick", 0.82)
                if b == 6:
                    d += grid("xxxxxxxxxxxxxxxx", t0, "hat", 0.26, pan=0.25, accents={0, 3, 6, 8, 11, 14})
            else:
                d += grid("x..x..x.x..x..x.", t0, "kick", 0.84)
                d += [Hit(t0 + 2 + k * 0.25, "snare", 0.25 + 0.07 * k, 0.0) for k in range(8)]
            continue
        if kind == "Br":
            if b < 4:
                d += grid("x..x............", t0, "kick", 0.8)
                d += grid("........x.......", t0, "snare", 0.7)
            elif b < 7:
                d += grid("x..x..x...x.....", t0, "kick", 0.8)
                d += grid("....x.......x...", t0, "snare", 0.75)
                d += grid("x.x.x.x.x.x.x.x.", t0, "hat", 0.34, pan=0.25)
            else:
                d += grid(TRESILLO, t0, "kick", 0.85)
                d += [Hit(t0 + k * 0.25, "snare", 0.2 + 0.05 * k, 0.0) for k in range(16)]
            continue
        big = kind == "C"
        d += grid(TRESILLO if big else "x..x..x...x..x..", t0, "kick", 0.9)
        d += grid("....x.......x...", t0, "snare", 0.88)
        if last:
            for k, (pos, note) in enumerate(((8, "D3"), (11, "A2"), (14, "D2"))):
                d.append(Hit(t0 + pos * 0.25, "tom", 0.62 + 0.06 * k, 0.3 - 0.3 * k, m(note)))
        elif b % 4 == 3:
            d += [Hit(t0 + 2.5, "tom", 0.5, 0.25, m("A2")), Hit(t0 + 3.25, "tom", 0.55, -0.1, m("D2"))]
        d += grid("xxxxxxxxxxxxxxxx", t0, "hat", 0.24, pan=0.25, accents={0, 3, 6, 8, 11, 14})
        if big:
            d += grid("....x.......x...", t0, "clap", 0.52)


# ------------------------------------------------------------------- the mix

TARGET = song1.TARGET
EXCITE = song1.EXCITE
SIDECHAIN = song1.SIDECHAIN
MASTER_EQ = song1.MASTER_EQ
WOBBLE = song1.WOBBLE

VOICES = copy.deepcopy(song1.VOICES)
VOICES["calm"]["pulse"] = ("pluck", dict(song1.PULSE_VOICE, amp_decay=0.16, cutoff=600), song1.PULSE_FX, -25.5)
VOICES["fight"]["pulse"] = ("pluck", dict(song1.PULSE_VOICE, cutoff=800, env_amt=2600, res=0.36, amp_decay=0.14),
                            song1.PULSE_FX, -24.5)

DRUM_GROUPS = copy.deepcopy(song1.DRUM_GROUPS)
DRUM_GROUPS["calm"]["perc"] = (("kick", "shaker"), dict(reverb=song1.ROOM, crush=song1.CRUSH), -30.0)
