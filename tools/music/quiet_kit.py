"""The menu theme and the end-of-match pieces: one layer in song 1's calm
voices, built from a short list of per-section parts."""

import copy
from collections import defaultdict

from . import match_kit as K
from . import song1
from .instruments import Hit
from .theory import grid, m, melody

PULSES = {
    "quarters": "x...x...x...x...",
    "eighths": "x.x.x.x.x.x.x.x.",
}


def build(sections, parts):
    """`parts[name]` lists what plays in that section: ("pad", vel),
    ("bass",), ("pulse", pattern, from_bar), (voice, melody, vel[, transpose]),
    ("harmony", voice, melody, vel), ("kick", pattern, vel), ("hat", pattern, vel),
    ("knell", every_bars, vel), ("boom", bar, vel), ("roll", bar, vel)."""
    ev = defaultdict(list)
    prev = None
    at = 0.0
    for name, chords in sections:
        bars = len(chords)
        for part in parts[name]:
            kind = part[0]
            if kind == "pad":
                prev = K.pad_part(ev, chords, at, part[1], prev)[-1]
            elif kind == "bass":
                K.calm_bass(ev, chords, at, 34)
            elif kind == "pulse":
                K.line(ev, "pulse", chords, at, PULSES[part[1]], 0.25, K.pulse_tones, order=(0, 1, 0, 2),
                       vel=0.42, accents={0}, gate=0.6, bars=range(part[2], bars))
            elif kind in ("whistle", "bell", "lead"):
                ev[kind] += melody(part[1], at, vel=part[2], transpose=part[3] if len(part) > 3 else 0)
            elif kind == "harmony":
                ev[part[1]] += K.harmony(melody(part[2], at, vel=part[3]), chords, at)
            elif kind in ("kick", "hat"):
                for b in range(bars):
                    ev["drums"] += grid(part[1], at + b * 4, kind, part[2], pan=0.25 if kind == "hat" else 0.0)
            elif kind == "knell":
                for b in range(0, bars, part[1]):
                    ev["drums"].append(Hit(at + b * 4, "tom", part[2], 0.0, m("D2")))
            elif kind == "boom":
                ev["drums"].append(Hit(at + part[1] * 4, "boom", part[2]))
            elif kind == "roll":
                t0 = at + part[1] * 4 + 2
                ev["drums"] += [Hit(t0 + k * 0.25, "snare", part[2] * (0.35 + 0.65 * k / 7), 0.0) for k in range(8)]
        at += bars * 4
    return ev


def voices():
    v = copy.deepcopy(song1.VOICES["calm"])
    v["lead"] = ("lead", dict(attack=0.09, cutoff=1500, release=0.6, vib_semis=0.12, saw_mix=0.2), song1.LEAD_FX, -23.0)
    return {"main": v}


def drum_groups():
    return {"main": {
        "perc": (("kick", "hat", "tom"), dict(reverb=(0.3, 2.2, 5000.0, 1.2, 0.01), crush=song1.CRUSH), -29.0),
        "snare": (("snare",), dict(reverb=(0.3, 1.6, 6000.0, 0.9, 0.0), crush=song1.CRUSH), -27.0),
        "boom": (("boom",), dict(reverb=(0.3, 2.6, 3000.0, 1.3, 0.0)), -25.0),
    }}
