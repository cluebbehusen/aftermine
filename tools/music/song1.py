"""Match song 1: D minor at 92 BPM over dark, pedal-heavy chords. A steady,
muted eighth-note pulse drives both layers; the calm layer carries the
melody on the thin whistle and the bell, the fight layer on a lead with a
growling bass and tight, dry drums. Light saturation, bit-crush and tape
wobble glue the mix."""

from collections import defaultdict

from . import dsp
from . import match_kit as K
from .instruments import Hit, Note
from .theory import bass_note, grid, m, melody

BPM = 92.0
SEED = 4404
LAYERS = ["calm", "fight"]

PEDAL_A = ["Dmadd9", "Bb/D", "F/D", "C/D", "Dmadd9", "Bb/D", "Gm/D", "A"]
DARK_C = ["Bbmaj7", "Am7", "Dmadd9", "Dm", "Bbmaj7", "Gm7", "A7sus4", "A"]
SECTIONS = [
    ("I", ["Dmadd9", "Dmadd9", "Bbmaj7/D", "Bbmaj7/D", "Dmadd9", "Dmadd9", "Bbmaj7/D", "Asus4"]),
    ("A", PEDAL_A),
    ("B", ["Gm", "Dm", "Bbmaj7", "Dm7/F", "Gm", "Dmadd9", "Ebmaj7", "A"]),
    ("C", DARK_C),
    ("Br", ["Dmadd9", "Dm/C", "Bbmaj7", "A", "Dmadd9", "Dm/C", "Gm/Bb", "A"]),
    ("A2", PEDAL_A),
    ("C2", DARK_C),
    ("S", K.SIEGE_CHORDS),
    ("F", PEDAL_A),
]
_MEL_C = ("F5:2 D5:1 F5:1 | G5:2 E5:1 G5:1 | A5:3 G5:.5 F5:.5 | E5:1 F5:1 D5:2 |"
          "F5:2 Bb5:1 A5:1 | G5:2 C6:1 Bb5:1 | A5:2 G5:1 E5:1 | C#5:2 E5:1 A4:1")
MELODY = {
    "I": "r:4 | r:4 | r:4 | r:4 | A4:1.5 D5:.5 F5:1 E5:1 | D5:4 | r:4 | r:4",
    "A": ("A4:1.5 D5:.5 F5:1 E5:1 | D5:3 F5:.5 D5:.5 | C5:1.5 A4:.5 C5:1 F5:1 | E5:3 r:1 |"
          "A4:1.5 D5:.5 F5:1 A5:1 | G5:1 F5:1 D5:2 | Bb4:1 D5:1 G5:1.5 F5:.5 | E5:2 C#5:1 r:1"),
    "B": ("G4:.5 Bb4:.5 D5:1.5 C5:.5 Bb4:1 | A4:2 F4:1 A4:1 | Bb4:.5 D5:.5 F5:1.5 D5:.5 C5:1 | A4:3 r:1 |"
          "G5:1 F5:.5 D5:.5 Bb4:1 D5:1 | F5:1.5 E5:.5 D5:2 | Eb5:1 G5:1 Bb5:1.5 G5:.5 | A5:2 E5:1 C#5:1"),
    "C": _MEL_C,
    "Br": ("A5:1.5 F5:.5 D5:2 | F5:1.5 D5:.5 C5:2 | F5:4 | E5:2 C#5:2 |"
           "A5:1.5 F5:.5 D5:2 | F5:1 E5:1 D5:1 C5:1 | D5:1.5 Bb4:.5 G4:2 | E5:2 C#5:1 A4:1"),
    "A2": ("A4:1.5 D5:.5 F5:1 E5:1 | D5:3 F5:.5 D5:.5 | C5:1.5 A4:.5 C5:1 F5:1 | E5:1.5 F5:.5 G5:1 E5:1 |"
           "A5:1.5 F5:.5 D5:1 A5:1 | G5:1 F5:1 D5:1.5 F5:.5 | G5:1 Bb5:1 A5:1.5 G5:.5 | E5:1 F5:.5 E5:.5 C#5:1 A4:1"),
    "C2": _MEL_C,
}

# the pulse: 0 is the bass note an octave up, 1 and 2 the chord's tones above it
PULSE_STEPS = [0, 0, 1, 0, 0, 2, 0, 1]


def build(layer):
    fight = layer == "fight"
    ev = defaultdict(list)
    prev = None
    for s, (name, chords) in enumerate(SECTIONS):
        at = s * 32.0
        if name == "S":
            K.siege(ev, layer, at, prev, "straight")
            continue
        # the final push is arranged like the lift, over A's chords
        kind = "C" if name == "F" else name.rstrip("2")
        pad_vel = {"I": 0.48, "A": 0.55, "B": 0.56, "C": 0.66, "Br": 0.5}[kind]
        voicings = K.pad_part(ev, chords, at, pad_vel, prev)
        prev = voicings[-1]

        low = 33 if kind == "Br" else 34
        if fight:
            for b, c in enumerate(chords):
                t0 = at + b * 4
                root = bass_note(c, low=low)
                if kind == "I" and b < 4:
                    if b in (0, 2):
                        ev["bass"].append(Note(t0, 8, root, 0.55))
                    continue
                for k in range(8):
                    up = 12 if kind == "C" and k == 7 else 0
                    ev["bass"].append(Note(t0 + k * 0.5, 0.42, root + up, 0.85 if k % 2 == 0 else 0.68))
        else:
            K.calm_bass(ev, chords, at, low)

        # the steady pulse
        for b, c in enumerate(chords):
            t0 = at + b * 4
            if kind == "I" and b < (2 if fight else 4):
                continue
            tones = K.pulse_tones(c)
            sixteenths = fight and kind == "C"
            steps = 16 if sixteenths else 8
            for k in range(steps):
                idx = PULSE_STEPS[(k // 2 if sixteenths else k) % len(PULSE_STEPS)] % len(tones)
                on_beat = k % (4 if sixteenths else 2) == 0
                vel = (0.55 if fight else 0.42) + (0.1 if on_beat else 0.0)
                ev["pulse"].append(Note(t0 + k * (4 / steps), (4 / steps) * 0.7, tones[idx], vel))

        if name == "F":
            K.final_push_melody(ev, layer, at, chords, MELODY["A2"])
            drums_for(ev, layer, kind, name, at)
            continue
        mel = MELODY[name]
        if fight:
            if kind in ("A", "B", "C"):
                ev["lead"] += melody(mel, at, vel=0.8)
                if kind == "C":
                    ev["lead"] += melody(mel, at, vel=0.55, transpose=-12)
                    ev["whistle"] += melody(mel, at, vel=0.4)
            elif kind == "Br":
                ev["whistle"] += melody(mel, at, vel=0.62)
        else:
            if kind in ("I", "A", "C"):
                ev["whistle"] += melody(mel, at, vel=0.62)
            elif kind == "Br" or name == "A2":
                ev["bell"] += melody(mel, at, vel=0.55)
            else:
                for b in range(8):
                    ev["bell"].append(Note(at + b * 4, 4, voicings[b][-1] + 12, 0.36))

        drums_for(ev, layer, kind, name, at)
        if fight and kind in ("I", "Br"):
            ev["riser"].append(Hit(at + 24, "riser", 0.65))
    return ev


def drums_for(ev, layer, kind, name, at):
    d = ev["drums"]
    if layer == "calm":
        for b in range(8):
            t0 = at + b * 4
            if kind in ("A", "B", "C"):
                d += grid("x.......x.......", t0, "kick", 0.45)
            if kind in ("B", "C") or name == "A2":
                d += grid("..x...x...x...x.", t0, "hat", 0.22, pan=0.25)
        return
    if kind in ("A", "B", "C"):
        d.append(Hit(at, "boom", 0.75))
    if kind == "C":
        d.append(Hit(at + 16, "boom", 0.6))
    for b in range(8):
        t0 = at + b * 4
        last = b == 7
        if kind == "I":
            if b < 4:
                d += [Hit(t0, "tom", 0.7, -0.1, m("D2")), Hit(t0 + 2, "tom", 0.55, 0.1, m("D2"))]
            elif b < 7:
                d += grid("x...x...x...x...", t0, "kick", 0.8)
                if b == 6:
                    d += grid("x.x.x.x.x.x.x.x.", t0, "hat", 0.42, pan=0.25)
            else:
                d += grid("x...x...x...x...", t0, "kick", 0.82)
                d += [Hit(t0 + 2 + k * 0.25, "snare", 0.25 + 0.07 * k, 0.0) for k in range(8)]
            continue
        if kind == "Br":
            if b < 4:
                d += grid("x...............", t0, "kick", 0.8)
                d += grid("........x.......", t0, "snare", 0.7)
            elif b < 7:
                d += grid("x.......x.......", t0, "kick", 0.8)
                d += grid("....x.......x...", t0, "snare", 0.75)
                d += grid("x.x.x.x.x.x.x.x.", t0, "hat", 0.4, pan=0.25)
            else:
                d += grid("x.x.x.x.x.x.x.x.", t0, "kick", 0.85)
                d += [Hit(t0 + k * 0.25, "snare", 0.2 + 0.05 * k, 0.0) for k in range(16)]
            continue
        big = kind == "C"
        kick = "x.....x.x.....x." if big else ("x.....x.x.x...x." if b % 4 == 3 else "x.....x.x.......")
        d += grid(kick, t0, "kick", 0.9)
        if last and kind == "B":
            d += grid("....x...........", t0, "snare", 0.88)
            for k, note in enumerate(("D3", "D3", "A2", "A2", "F2", "F2", "D2", "D2")):
                d.append(Hit(t0 + 2 + k * 0.25, "tom", 0.55 + 0.05 * k, 0.3 - 0.08 * k, m(note)))
        elif last:
            d += grid("....x.......X.xx", t0, "snare", 0.88)
        else:
            d += grid("....x.......x...", t0, "snare", 0.88)
        # mechanical sixteenths: the same accent shape every bar
        d += grid("xxxxxxxxxxxxxxxx", t0, "hat", 0.26, pan=0.25, accents={0, 4, 8, 12})
        if big:
            d += grid("....x.......x...", t0, "clap", 0.55)
            d += grid("..............x.", t0, "ohat", 0.4, pan=0.25)


# ------------------------------------------------------------------- the mix

TARGET = {"calm": -18.0, "fight": -15.0}
EXCITE = {"calm": dict(freq=1800.0, drive=4.0, mix=0.6)}
SIDECHAIN = {"fight": {"curve": {"depth_db": 4.0, "release": 0.16}, "buses": {"pad", "pulse"}}}
WOBBLE = dict(wow_rate=0.43, wow_ms=0.25, flutter_rate=5.7, flutter_ms=0.03)


def MASTER_EQ(layer):
    if layer == "fight":
        return (dsp.shelf_sos(90, -2.0, False), dsp.peak_sos(230, -1.5, 0.9), dsp.peak_sos(650, -1.5, 1.0),
                dsp.shelf_sos(3500, 2.0, True))
    return (dsp.peak_sos(220, -2.0, 0.9), dsp.shelf_sos(4500, 3.5, True))


CRUSH = (11, 2, 0.4)
PAD_FX = dict(chorus=(0.25, 3.5, 13.0, 0.45), reverb=(0.5, 3.6, 4200.0, 1.5, 0.03), hp=130)
PULSE_FX = dict(crush=CRUSH, delay=(0.12, 0.75, 0.5, 0.25, 2500.0), reverb=(0.12, 1.2, 4500.0, 0.7, 0.0), hp=140)
WHISTLE_FX = dict(delay=(0.18, 0.75, 0.5, 0.3, 3000.0), reverb=(0.5, 4.0, 5000.0, 1.5, 0.06), hp=250)
BELL_FX = dict(delay=(0.2, 0.75, 0.5, 0.35, 3500.0), reverb=(0.5, 3.6, 5500.0, 1.4, 0.03), hp=250)
LEAD_FX = dict(delay=(0.18, 0.75, 0.5, 0.3, 3000.0), reverb=(0.24, 2.4, 5000.0, 1.1, 0.02), hp=150)
ROOM = (0.16, 0.8, 6000.0, 0.55, 0.0)

PULSE_VOICE = dict(cutoff=650, env_amt=2000, decay=0.07, res=0.3, amp_decay=0.22, sustain=0.0,
                   release=0.05, square=0.45, detune=5)

# bus: (voice, voice settings, effects, loudness target in LUFS)
VOICES = {
    "calm": {
        "pad": ("pad", dict(bright=0.36, attack=1.4, release=3.0, detune=11.0, lfo=0.06), PAD_FX, -21.5),
        "pulse": ("pluck", PULSE_VOICE, PULSE_FX, -25.0),
        "bass": ("bass", dict(cutoff=130, env_amt=120, drive=1.3, upper=0.25, sub=0.4, attack=1.0, release=2.0, sustain=0.95),
                 dict(hp=35, lp=1800, crush=CRUSH), -24.0),
        "whistle": ("whistle", dict(breath=0.2), WHISTLE_FX, -21.0),
        "bell": ("bell", dict(ratio=3.5, index=2.3, decay=2.6), BELL_FX, -22.0),
    },
    "fight": {
        "pad": ("pad", dict(bright=0.3, attack=1.0, release=2.5, detune=12.0, lfo=0.06), PAD_FX, -25.0),
        "pulse": ("pluck", dict(PULSE_VOICE, cutoff=850, env_amt=3000, res=0.38), PULSE_FX, -24.0),
        "bass": ("bass", dict(cutoff=120, env_amt=900, decay=0.12, res=0.35, drive=3.0, sub=0.32, upper=0.4,
                              release=0.06, sustain=0.85), dict(hp=35, lp=3800, crush=CRUSH), -20.5),
        "lead": ("lead", dict(cutoff=1900, saw_mix=0.5, release=0.35), LEAD_FX, -19.5),
        "whistle": ("whistle", {}, WHISTLE_FX, -25.0),
        "riser": ("riser", dict(beats=8), dict(reverb=(0.3, 2.5, 6000.0, 1.2, 0.01), lp=6000), -27.0),
        "lead2": ("lead", dict(cutoff=1700, saw_mix=0.5, release=0.35), LEAD_FX, -23.5),
        "lead3": ("lead", dict(cutoff=1500, saw_mix=0.5, release=0.35), LEAD_FX, -25.0),
        "bell": ("bell", dict(ratio=3.5, index=2.0, decay=1.8), BELL_FX, -26.0),
    },
}

# group: (drum kinds, effects, loudness target in LUFS)
DRUM_GROUPS = {
    "calm": {
        "perc": (("kick", "hat"), dict(reverb=ROOM, crush=CRUSH), -30.0),
    },
    "fight": {
        "kick": (("kick",), dict(hp=35, crush=CRUSH), -20.5),
        "snare": (("snare", "clap"), dict(reverb=(0.22, 0.9, 6000.0, 0.55, 0.0), crush=CRUSH), -21.0),
        "hats": (("hat", "ohat"), dict(reverb=ROOM, crush=CRUSH), -28.5),
        "toms": (("tom",), dict(reverb=ROOM, crush=CRUSH), -24.0),
        "boom": (("boom",), dict(reverb=(0.25, 2.0, 3000.0, 1.2, 0.0)), -26.0),
    },
}
