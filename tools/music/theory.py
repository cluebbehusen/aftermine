"""Note names, chords, voicings and a small melody notation."""

from fractions import Fraction

from .instruments import Hit, Note

PC = {"C": 0, "C#": 1, "Db": 1, "D": 2, "D#": 3, "Eb": 3, "E": 4, "F": 5, "F#": 6,
      "Gb": 6, "G": 7, "G#": 8, "Ab": 8, "A": 9, "A#": 10, "Bb": 10, "B": 11}


def m(name):
    pitch, octave = name[:-1], int(name[-1])
    return 12 * (octave + 1) + PC[pitch]


def chord(symbol):
    """Chord symbol -> (root pitch class, intervals, bass pitch class)."""
    bass = None
    if "/" in symbol:
        symbol, b = symbol.split("/")
        bass = PC[b]
    quality = (0, 4, 7)
    root = symbol
    if symbol.endswith("madd9"):
        root, quality = symbol[:-5], (0, 3, 7, 14)
    elif symbol.endswith("add9"):
        root, quality = symbol[:-4], (0, 4, 7, 14)
    elif symbol.endswith("7sus4"):
        root, quality = symbol[:-5], (0, 5, 7, 10)
    elif symbol.endswith("maj7"):
        root, quality = symbol[:-4], (0, 4, 7, 11)
    elif symbol.endswith("m7"):
        root, quality = symbol[:-2], (0, 3, 7, 10)
    elif symbol.endswith("sus2"):
        root, quality = symbol[:-4], (0, 2, 7)
    elif symbol.endswith("sus4"):
        root, quality = symbol[:-4], (0, 5, 7)
    elif symbol.endswith("m"):
        root, quality = symbol[:-1], (0, 3, 7)
    r = PC[root]
    return r, quality, bass if bass is not None else r


def in_range(pc, low, high):
    """The lowest midi note of pitch class `pc` at or above `low`, folded
    under `high`."""
    n = low + (pc - low) % 12
    while n > high:
        n -= 12
    return n


def bass_note(symbol, low=34, high=45):
    _, _, b = chord(symbol)
    return in_range(b, low, high)


def voicings(symbol, low, high, size):
    r, q, _ = chord(symbol)
    pcs = [(r + i) % 12 for i in q]
    notes = sorted(n for n in range(low, high + 1) if n % 12 in pcs)
    out = []

    def rec(start, chosen):
        if len(chosen) == size:
            if {c % 12 for c in chosen} >= set(pcs[: min(size, len(pcs))]) and chosen[-1] - chosen[0] <= 14:
                out.append(tuple(chosen))
            return
        for i in range(start, len(notes)):
            rec(i + 1, chosen + [notes[i]])

    rec(0, [])
    return out


def lead_voices(symbols, low=53, high=76, size=4, start=None):
    """Picks a voicing per chord with the least total movement."""
    prev = start
    result = []
    for s in symbols:
        cands = voicings(s, low, high, size)
        if prev is None:
            center = (low + high) / 2
            best = min(cands, key=lambda v: abs(sum(v) / len(v) - center))
        else:
            best = min(cands, key=lambda v: sum(abs(a - b) for a, b in zip(v, prev)) + 0.1 * abs(sum(v) - sum(prev)))
        result.append(best)
        prev = best
    return result


def melody(spec, start, vel=0.8, transpose=0, accent_downbeats=0.08):
    """'A4:1.5 D5:.5 | ...' -> notes. Durations may be fractions such as
    2/3 for triplets. Each bar must total four beats."""
    notes = []
    beat = start
    for b, bar in enumerate(spec.split("|")):
        total = 0.0
        for tok in bar.split():
            name, dur = tok.split(":")
            d = float(Fraction(dur))
            if name != "r":
                frac = (beat - start) % 1
                on_beat = min(frac, 1 - frac) < 1e-6
                v = vel + (accent_downbeats if on_beat else 0)
                notes.append(Note(beat, d, m(name) + transpose, min(v, 1.0)))
            beat += d
            total += d
        assert abs(total - 4) < 1e-6, f"bar {b + 1} of {spec[:30]}... has {total} beats"
    return notes


def grid(pattern, start, kind, vel=0.8, pan=0.0, accents=None, step=0.25):
    """'x...x...' style 16th grid; 'X' is accented."""
    hits = []
    for i, ch in enumerate(pattern):
        if ch == ".":
            continue
        v = vel
        if ch == "X":
            v = min(1.0, vel * 1.25)
        if accents and i in accents:
            v = min(1.0, v * 1.2)
        hits.append(Hit(start + i * step, kind, v, pan))
    return hits
