"""Synth voices and drums. Every voice renders a list of notes into a stereo
(n, 2) buffer. Times in notes are beats; `bpm` converts."""

from dataclasses import dataclass

import numpy as np

from . import dsp
from .dsp import SR, add, adsr, midi_hz, perc, secs, to_stereo


@dataclass
class Note:
    beat: float
    dur: float
    midi: float
    vel: float = 0.8
    pan: float = 0.0


def _start(beat, bpm):
    return secs(beat * 60.0 / bpm)


def _len(beats, bpm):
    return beats * 60.0 / bpm


# ------------------------------------------------------------------- tonal


def pad(notes, n, bpm, seed, bright=0.35, attack=0.6, release=1.8, voices=7,
        detune=14.0, res=0.15, width=0.85, lfo=0.09):
    """Detuned saw stack through a slowly breathing 24 dB low-pass."""
    rng = np.random.default_rng(seed)
    out = np.zeros((n, 2))
    for note in notes:
        env = adsr(_len(note.dur, bpm), attack, 1.2, 0.85, release)
        L = env.shape[0]
        t = np.arange(L) / SR
        f0 = float(midi_hz(note.midi))
        mix = np.zeros((L, 2))
        for v in range(voices):
            off = (v - (voices - 1) / 2) / max((voices - 1) / 2, 1)
            cents = off * detune + rng.normal(0, 1.2)
            drift = 1 + 0.0012 * np.sin(2 * np.pi * (0.21 + 0.07 * v) * t + rng.random() * 6.28)
            f = f0 * 2 ** (cents / 1200) * drift
            s = dsp.saw(f, rng.random())
            g = 1.0 if v == (voices - 1) // 2 else 0.75
            mix += to_stereo(s * g, off * width)
        phase = rng.random() * 6.28
        cutoff = (500 + 2600 * bright) * (0.55 + 0.45 * np.clip(env * 1.4, 0, 1))
        cutoff *= 1 + 0.18 * np.sin(2 * np.pi * lfo * (t + note.beat * 60 / bpm) + phase)
        cutoff = np.minimum(cutoff + f0 * 1.5, 14000)
        for c in range(2):
            mix[:, c] = dsp.lp24(mix[:, c], cutoff, res)
        mix *= (env * note.vel / np.sqrt(voices))[:, None]
        add(out, _start(note.beat, bpm), mix)
    return out


def pluck(notes, n, bpm, seed, cutoff=900.0, env_amt=3800.0, decay=0.11, res=0.35,
          detune=7.0, square=0.35, release=0.09, sustain=0.0, amp_decay=0.35, keytrack=0.6):
    """Two detuned saws and a square under a snapping filter envelope."""
    rng = np.random.default_rng(seed)
    out = np.zeros((n, 2))
    for note in notes:
        env = adsr(_len(note.dur, bpm), 0.002, amp_decay, sustain, release)
        L = env.shape[0]
        t = np.arange(L) / SR
        f0 = float(midi_hz(note.midi))
        a = dsp.saw(np.full(L, f0 * 2 ** (detune / 1200)), rng.random())
        b = dsp.saw(np.full(L, f0 * 2 ** (-detune / 1200)), rng.random())
        q = dsp.pulse(np.full(L, f0), np.full(L, 0.5), rng.random())
        s = (a + b) * 0.5 * (1 - square) + q * square
        fc = cutoff * (f0 / 261.6) ** keytrack + env_amt * note.vel * np.exp(-t / decay)
        s = dsp.lp24(s, fc, res)
        s *= env * note.vel
        add(out, _start(note.beat, bpm), to_stereo(s, note.pan))
    return out


def bass(notes, n, bpm, seed, cutoff=180.0, env_amt=1100.0, decay=0.09, res=0.25,
         drive=1.8, sub=0.55, release=0.06, sustain=0.75, upper=0.0, attack=0.003):
    """Saw and pulse with a sine sub, a plucked low-pass and tape-ish drive.
    `upper` adds an octave-up saw so small speakers still hear the line."""
    rng = np.random.default_rng(seed)
    out = np.zeros((n, 2))
    for note in notes:
        env = adsr(_len(note.dur, bpm), attack, 0.35, sustain, release)
        L = env.shape[0]
        t = np.arange(L) / SR
        f0 = float(midi_hz(note.midi))
        s = dsp.saw(np.full(L, f0), rng.random()) * 0.7
        s += dsp.pulse(np.full(L, f0 * 1.003), np.full(L, 0.42), rng.random()) * 0.3
        if upper:
            s += dsp.saw(np.full(L, f0 * 2.004), rng.random()) * upper
        fc = cutoff + env_amt * note.vel * np.exp(-t / decay)
        s = dsp.lp24(s, fc, res)
        s += np.sin(2 * np.pi * f0 * t) * sub
        s = dsp.sat(s, drive) * env * note.vel
        add(out, _start(note.beat, bpm), to_stereo(s, 0.0))
    return out


def lead(notes, n, bpm, seed, glide=0.045, vib_semis=0.16, vib_rate=5.3, cutoff=2400.0,
         res=0.18, detune=9.0, attack=0.012, release=0.28, sustain=0.82, saw_mix=0.35):
    """A pulse-width-modulated pair with glide between legato notes and a
    vibrato that blooms on long notes."""
    rng = np.random.default_rng(seed)
    out = np.zeros((n, 2))
    ordered = sorted(notes, key=lambda x: x.beat)
    prev = None
    for note in ordered:
        legato = prev is not None and abs(prev.beat + prev.dur - note.beat) < 0.02
        env = adsr(_len(note.dur, bpm), attack if not legato else 0.004, 0.5, sustain, release)
        L = env.shape[0]
        t = np.arange(L) / SR
        pitch = np.full(L, float(note.midi))
        if legato:
            pitch = note.midi + (prev.midi - note.midi) * np.exp(-t / glide)
        bloom = np.clip((t - 0.28) / 0.35, 0, 1)
        pitch = pitch + vib_semis * bloom * np.sin(2 * np.pi * vib_rate * t)
        f = midi_hz(pitch)
        pw = 0.5 + 0.18 * np.sin(2 * np.pi * 0.7 * t + rng.random() * 6.28)
        s = dsp.pulse(f, pw, rng.random()) * (1 - saw_mix)
        s += dsp.saw(f * 2 ** (detune / 1200), rng.random()) * saw_mix
        s += dsp.pulse(f * 2 ** (-detune / 1200), 1 - pw, rng.random()) * (1 - saw_mix) * 0.6
        fc = cutoff * (1 + 0.9 * np.exp(-t / 0.12)) * (float(midi_hz(note.midi)) / 523.0) ** 0.4
        s = dsp.lp12(s, fc, res)
        s *= env * note.vel
        add(out, _start(note.beat, bpm), to_stereo(s, note.pan))
        prev = note
    return out


def bell(notes, n, bpm, seed, ratio=3.5, index=2.6, decay=2.4, body=0.5):
    """Two-operator FM bell with a soft 1:1 body partial."""
    out = np.zeros((n, 2))
    for note in notes:
        dur = max(decay * 3.2, _len(note.dur, bpm))
        L = secs(dur)
        t = np.arange(L) / SR
        f0 = float(midi_hz(note.midi))
        mod = np.sin(2 * np.pi * f0 * ratio * t) * index * np.exp(-t / (decay * 0.3)) * note.vel
        s = np.sin(2 * np.pi * f0 * t + mod)
        s += body * np.sin(2 * np.pi * f0 * 2.001 * t) * np.exp(-t / (decay * 0.5))
        s *= perc(dur, decay, 0.002) * note.vel
        add(out, _start(note.beat, bpm), to_stereo(s, note.pan))
    return out


def whistle(notes, n, bpm, seed, glide=0.09, vib_semis=0.1, vib_rate=4.4, breath=0.14,
            attack=0.22, release=1.1, drift=0.05, sustain=0.85):
    """A thin, breathy, nearly pure voice that drifts a little off pitch."""
    rng = np.random.default_rng(seed)
    out = np.zeros((n, 2))
    ordered = sorted(notes, key=lambda x: x.beat)
    prev = None
    for note in ordered:
        legato = prev is not None and abs(prev.beat + prev.dur - note.beat) < 0.02
        env = adsr(_len(note.dur, bpm), attack if not legato else 0.03, 0.8, sustain, release)
        L = env.shape[0]
        t = np.arange(L) / SR
        pitch = np.full(L, float(note.midi))
        if legato:
            pitch = note.midi + (prev.midi - note.midi) * np.exp(-t / glide)
        bloom = np.clip((t - 0.4) / 0.6, 0, 1)
        a, b = rng.random(2) * 6.28
        pitch = pitch + vib_semis * bloom * np.sin(2 * np.pi * vib_rate * t + a)
        pitch = pitch + drift * np.sin(2 * np.pi * 0.31 * t + b)
        f = midi_hz(pitch)
        ph = 2 * np.pi * np.cumsum(f) / SR
        tone = np.sin(ph) + 0.1 * np.sin(2 * ph) + 0.04 * np.sin(3 * ph)
        air = dsp.bp(rng.standard_normal(L), f * 1.0, 0.75) * breath
        s = (tone + air) * env * note.vel
        add(out, _start(note.beat, bpm), to_stereo(s, note.pan))
        prev = note
    return out


# -------------------------------------------------------------------- drums


def _noise(rng, L):
    return rng.standard_normal(L)


def kick(rng, vel, tone=1.0):
    L = secs(0.55)
    t = np.arange(L) / SR
    f = 46 + (165 - 46) * np.exp(-t / 0.032)
    body = np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-t / 0.30)
    body = dsp.sat(body * 1.6, 2.2)
    click = dsp.hp12(_noise(rng, L), 1800) * np.exp(-t / 0.0035) * 0.35
    knock = np.sin(2 * np.pi * 190 * t) * np.exp(-t / 0.02) * 0.25 * tone
    env = perc(0.55, 1.0, 0.0005)
    return (body + click + knock) * env * vel


def snare(rng, vel):
    L = secs(0.45)
    t = np.arange(L) / SR
    tone = np.sin(2 * np.pi * 182 * t) * np.exp(-t / 0.08) + 0.5 * np.sin(2 * np.pi * 329 * t) * np.exp(-t / 0.05)
    nz = _noise(rng, L)
    body = dsp.bp(nz, 3200, 0.15) * np.exp(-t / 0.15) * 0.9
    sizzle = dsp.hp12(nz, 6500) * np.exp(-t / 0.07) * 0.5
    return (tone * 0.7 + body + sizzle) * perc(0.45, 1.0, 0.0008) * vel


def clap(rng, vel):
    L = secs(0.4)
    t = np.arange(L) / SR
    nz = dsp.bp(_noise(rng, L), 1400, 0.35)
    env = np.zeros(L)
    for k, at in enumerate((0.0, 0.011, 0.022)):
        i = secs(at)
        env[i:] += np.exp(-(t[: L - i]) / 0.006) * (0.8 + 0.1 * k)
    env += np.where(t > 0.033, np.exp(-(t - 0.033) / 0.11), 0) * 0.9
    return dsp.hp12(nz * env, 700) * vel


_METAL = (205.3, 304.4, 369.6, 522.7, 540.0, 800.0)


def hat(rng, vel, open_=False):
    length = 0.6 if open_ else 0.12
    L = secs(length)
    t = np.arange(L) / SR
    m = np.zeros(L)
    for f in _METAL:
        m += np.sign(np.sin(2 * np.pi * f * 1.9 * t + rng.random() * 6.28))
    m = m / len(_METAL) * 0.6 + _noise(rng, L) * 0.4
    m = dsp.bp(m, 9000, 0.2) + dsp.hp12(m, 8000) * 0.6
    return m * perc(length, 0.22 if open_ else 0.032, 0.0005) * vel


def shaker(rng, vel):
    L = secs(0.14)
    t = np.arange(L) / SR
    env = (1 - np.exp(-t / 0.012)) * np.exp(-t / 0.045)
    return dsp.hp12(_noise(rng, L), 5500) * env * vel * 0.7


def tom(rng, vel, midi):
    L = secs(0.7)
    t = np.arange(L) / SR
    f0 = float(midi_hz(midi))
    f = f0 * (1 + 0.6 * np.exp(-t / 0.04))
    s = np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-t / 0.32)
    s = dsp.sat(s * 1.3, 1.6) + dsp.bp(_noise(rng, L), f0 * 4, 0.3) * np.exp(-t / 0.03) * 0.3
    return s * perc(0.7, 1.0, 0.0008) * vel


def boom(rng, vel):
    L = secs(2.5)
    t = np.arange(L) / SR
    f = 38 + 50 * np.exp(-t / 0.08)
    s = np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-t / 0.9)
    s = dsp.sat(s * 1.4, 1.8)
    s += dsp.lp12(_noise(rng, L), 900) * np.exp(-t / 0.35) * 0.5
    return s * perc(2.5, 1.0, 0.002) * vel


DRUMS = {
    "boom": boom,
    "kick": kick,
    "snare": snare,
    "clap": clap,
    "hat": lambda rng, v: hat(rng, v),
    "ohat": lambda rng, v: hat(rng, v, True),
    "shaker": shaker,
}


@dataclass
class Hit:
    beat: float
    kind: str
    vel: float = 0.8
    pan: float = 0.0
    midi: float = 0.0


def drums(hits, n, bpm, seed, jitter_ms=2.0):
    """Renders hits into one stereo bus per drum kind, returned as a dict.
    Every hit draws from one generator in list order, so reordering hits
    changes the sound of all that follow."""
    rng = np.random.default_rng(seed)
    buses = {}
    for h in hits:
        if h.kind == "tom":
            s = tom(rng, h.vel, h.midi)
        else:
            s = DRUMS[h.kind](rng, h.vel)
        start = _start(h.beat, bpm)
        # never early: a hit pulled before its section's first sample would lose its attack
        at = max(start, start + int(rng.normal(0, jitter_ms) * SR / 1000))
        bus = buses.setdefault(h.kind, np.zeros((n, 2)))
        add(bus, at, to_stereo(s, h.pan))
    return buses


# ----------------------------------------------------------------- textures


def riser(n, bpm, beat, beats, vel=0.6, seed=7):
    rng = np.random.default_rng(seed)
    dur = _len(beats, bpm)
    L = secs(dur)
    t = np.arange(L) / SR
    x = t / dur
    nz = rng.standard_normal(L)
    fc = 250 * (8000 / 250) ** (x ** 1.6)
    s = dsp.bp(nz, fc, 0.55) * (x ** 2.2) * vel
    tail = secs(0.01)
    s[-tail:] *= np.linspace(1, 0, tail)
    out = np.zeros((n, 2))
    st = np.stack([s, dsp.bp(rng.standard_normal(L), fc * 1.1, 0.55) * (x ** 2.2) * vel], axis=1)
    add(out, _start(beat, bpm), st)
    return out


def sidechain(n, bpm, beats, depth_db=5.0, release=0.17, attack=0.004):
    """A gain curve that ducks after each beat in `beats`."""
    g = np.zeros(n)
    L = secs(release * 5)
    t = np.arange(L) / SR
    shape = np.exp(-t / release)
    ramp = secs(attack)
    shape[:ramp] = np.linspace(0, 1, ramp)
    for b in beats:
        at = _start(b, bpm)
        end = min(n, at + L)
        if end > at:
            g[at:end] = np.maximum(g[at:end], shape[: end - at])
    depth = 1 - 10 ** (-depth_db / 20)
    return 1 - depth * g
