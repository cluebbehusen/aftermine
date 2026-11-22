"""Shared parts for the match songs: voice-led pads, sustained calm bass,
pattern-driven pulse and bass lines, and the special sections every song
carries."""

import dataclasses

from .instruments import Hit, Note
from .theory import bass_note, chord, grid, in_range, lead_voices, m, melody


def merged(chords):
    """Runs of one chord as (index, length in bars)."""
    runs = []
    for i, c in enumerate(chords):
        if runs and chords[runs[-1][0]] == c:
            runs[-1][1] += 1
        else:
            runs.append([i, 1])
    return runs


def bass_runs(chords, low):
    """Runs of one bass note as (index, note, length in bars)."""
    runs = []
    for i, c in enumerate(chords):
        b = bass_note(c, low=low)
        if runs and runs[-1][1] == b:
            runs[-1][2] += 1
        else:
            runs.append([i, b, 1])
    return runs


def pulse_tones(symbol, low=45, high=56):
    """The chord's bass note in `low..high`, then its other triad tones above."""
    r, q, b = chord(symbol)
    base = in_range(b, low, high)
    above = sorted({in_range((r + i) % 12, base + 1, base + 12) for i in q[:3] if (r + i) % 12 != b})
    return [base] + above


def pad_part(ev, chords, at, vel, prev, low=52, high=74):
    voicings = lead_voices(chords, low=low, high=high, size=4, start=prev)
    for i, run in merged(chords):
        for p in voicings[i]:
            ev["pad"].append(Note(at + i * 4, run * 4, p, vel))
    return voicings


def calm_bass(ev, chords, at, low=34, vel=0.45):
    for i, note, run in bass_runs(chords, low):
        ev["bass"].append(Note(at + i * 4, run * 4, note, vel))


def line(ev, bus, chords, at, pat, step, tones, order=(0,), vel=0.5, accent=0.1, gate=0.7,
         bars=None, accents=()):
    """One note per non-'.' character of `pat`, repeated each bar. 'x' plays
    the next tone of `order` from `tones(chord)`, 'o' that tone an octave
    up, 'X' and positions in `accents` are accented. A note lasts until the
    next one, times `gate`."""
    per_bar = round(4 / step)
    assert len(pat) == per_bar, (pat, step)
    hits = [i for i, ch in enumerate(pat) if ch != "."]
    for b, c in enumerate(chords):
        if bars is not None and b not in bars:
            continue
        t = tones(c)
        for k, i in enumerate(hits):
            nxt = hits[k + 1] if k + 1 < len(hits) else per_bar
            note = t[order[k % len(order)] % len(t)] + (12 if pat[i] == "o" else 0)
            v = vel + (accent if pat[i] == "X" or i in accents else 0.0)
            ev[bus].append(Note(at + b * 4 + i * step, (nxt - i) * step * gate, note, min(v, 1.0)))


def bass_tone(low=34):
    return lambda c: [bass_note(c, low=low)]


# ------------------------------------------------------- special sections

# Base under siege: the same idea in every song, over a held low D. A
# repeated note leans on the half step above it and climbs; the bar
# before the turn back lands on A.
SIEGE_CHORDS = ["Dm", "Bb/D", "Dm", "Ebmaj7/D", "Dm", "Bb/D", "Gm/D", "A"]
SIEGE_MELODY = {
    "straight": ("A4:.5 A4:.5 A4:.5 Bb4:.5 A4:2 | D5:.5 D5:.5 D5:.5 Eb5:.5 D5:2 | F5:.5 F5:.5 F5:.5 G5:.5 F5:2 |"
                 "G5:.5 G5:.5 G5:.5 Bb5:.5 G5:2 | A5:.5 A5:.5 A5:.5 Bb5:.5 A5:2 | F5:.5 F5:.5 F5:.5 G5:.5 F5:2 |"
                 "G5:.5 G5:.5 Bb5:.5 A5:.5 G5:1 D5:1 | E5:1 F5:1 E5:1 C#5:1"),
    "tresillo": ("A4:.75 A4:.75 Bb4:.5 A4:2 | D5:.75 D5:.75 Eb5:.5 D5:2 | F5:.75 F5:.75 G5:.5 F5:2 |"
                 "G5:.75 G5:.75 Bb5:.5 G5:2 | A5:.75 A5:.75 Bb5:.5 A5:2 | F5:.75 F5:.75 G5:.5 F5:2 |"
                 "G5:.75 Bb5:.75 A5:.5 G5:1 D5:1 | E5:.75 F5:.75 E5:.5 C#5:2"),
    "triplet": ("A4:1/3 A4:1/3 A4:1/3 Bb4:1 A4:2 | D5:1/3 D5:1/3 D5:1/3 Eb5:1 D5:2 | F5:1/3 F5:1/3 F5:1/3 G5:1 F5:2 |"
                "G5:1/3 G5:1/3 G5:1/3 Bb5:1 G5:2 | A5:1/3 A5:1/3 A5:1/3 Bb5:1 A5:2 | F5:1/3 F5:1/3 F5:1/3 G5:1 F5:2 |"
                "G5:2/3 Bb5:1/3 A5:2/3 G5:1/3 D5:2 | E5:2/3 F5:1/3 E5:2/3 C#5:1/3 A4:2"),
}
FEELS = {
    "straight": dict(step=0.25, bass="x.x.x.o.x.x.x.o.", pulse_fight="xxxxxxxxxxxxxxxx", pulse_calm="x.x.x.x.x.x.x.x.",
                     accents={0, 4, 8, 12}, kick="x...x...x...x...", toms="..x...x...x..xx.", snare="....x.......x...",
                     hats="xxxxxxxxxxxxxxxx", heart="x..x............"),
    "tresillo": dict(step=0.25, bass="x..x..o.x..x..o.", pulse_fight="xxxxxxxxxxxxxxxx", pulse_calm="x..x..x.x..x..x.",
                     accents={0, 3, 6, 8, 11, 14}, kick="x..x..x.x..x..x.", toms="..x..x.....x.xx.", snare="....x.......x...",
                     hats="xxxxxxxxxxxxxxxx", heart="x..x............"),
    "triplet": dict(step=1 / 3, bass="x.xo.xx.xo.x", pulse_fight="xxxxxxxxxxxx", pulse_calm="x..x..x..x..",
                    accents={0, 3, 6, 9}, kick="x..x..x..x..", toms="..x..x..x.xx", snare="...x.....x..",
                    hats="xxxxxxxxxxxx", heart="x.x........."),
}


def _toms(d, pattern, t0, step, vel, notes=("D2", "A2")):
    k = 0
    for i, ch in enumerate(pattern):
        if ch != ".":
            d.append(Hit(t0 + i * step, "tom", vel, 0.25 - 0.5 * (k % 2), m(notes[k % len(notes)])))
            k += 1


def siege(ev, layer, at, prev, feel, low=34):
    f = FEELS[feel]
    step = f["step"]
    fight = layer == "fight"
    chords = SIEGE_CHORDS
    voicings = pad_part(ev, chords, at, 0.5 if fight else 0.46, prev)
    spec = SIEGE_MELODY[feel]
    d = ev["drums"]
    if fight:
        line(ev, "bass", chords, at, f["bass"], step, bass_tone(low), vel=0.84, gate=0.8, accents={0})
        line(ev, "pulse", chords, at, f["pulse_fight"], step, pulse_tones, order=(0, 0, 1, 0, 0, 2, 0, 1),
             vel=0.52, accents=f["accents"], accent=0.14)
        ev["lead"] += melody(spec, at, vel=0.8)
        ev["lead2"] += melody(spec, at, vel=0.5, transpose=-12)
        d += [Hit(at, "boom", 0.78), Hit(at + 16, "boom", 0.62)]
        per_bar = round(4 / step)
        for b in range(8):
            t0 = at + b * 4
            d += grid(f["kick"], t0, "kick", 0.88, step=step)
            d += grid(f["hats"], t0, "hat", 0.24, pan=0.25, accents=f["accents"], step=step)
            if b == 7:
                d += grid(f["snare"][: per_bar // 2] + "." * (per_bar - per_bar // 2), t0, "snare", 0.86, step=step)
                half = per_bar // 2
                d += [Hit(t0 + (half + k) * step, "snare", 0.3 + 0.6 * k / half, 0.0) for k in range(half)]
            else:
                d += grid(f["snare"], t0, "snare", 0.86, step=step)
                _toms(d, f["toms"], t0, step, 0.55)
    else:
        calm_bass(ev, chords, at, low)
        line(ev, "pulse", chords, at, f["pulse_calm"], step, pulse_tones, order=(0, 1, 0, 2),
             vel=0.42, accents={0}, gate=0.6)
        ev["whistle"] += melody(spec, at, vel=0.55)
        for b in range(8):
            t0 = at + b * 4
            d += grid(f["heart"], t0, "kick", 0.4, step=step)
            if b % 2 == 0:
                _toms(d, "x" + "." * (round(4 / step) - 1), t0, step, 0.35, notes=("D2",))
    return voicings


def harmony(notes, chords, at, below=3):
    """A second line under `notes`: the highest tone of the bar's chord at
    least `below` semitones under each note, so it always fits the chord."""
    out = []
    for n in notes:
        r, q, b = chord(chords[int((n.beat - at + 1e-6) // 4)])
        pcs = {(r + i) % 12 for i in q} | {b}
        h = n.midi - below
        while h % 12 not in pcs:
            h -= 1
        out.append(dataclasses.replace(n, midi=h))
    return out


def final_push_melody(ev, layer, at, chords, spec):
    """The A melody at its fullest: doubled an octave down and an octave up,
    with a second line under it."""
    if layer == "fight":
        ev["lead"] += melody(spec, at, vel=0.85)
        ev["lead2"] += harmony(melody(spec, at, vel=0.52), chords, at)
        ev["lead3"] += melody(spec, at, vel=0.4, transpose=-12)
        ev["bell"] += melody(spec, at, vel=0.32, transpose=12)
    else:
        ev["whistle"] += melody(spec, at, vel=0.64)
        ev["bell"] += harmony(melody(spec, at, vel=0.45), chords, at)
