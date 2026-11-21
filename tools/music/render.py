"""Renders a piece one section at a time so the game can chain sections in
any order. Each section is its body plus the tail that rings past its end;
the next section starts where the body ends while that tail dies away under
it.

Bus balance and the master gain are measured on the sections played in
order, so every section keeps its place in the song's dynamics."""

import dataclasses
import importlib
from typing import NamedTuple

import numpy as np
import pyloudnorm as pyln

from . import dsp
from . import instruments as ins
from .dsp import SR, secs

# Seconds rendered past each section's body for its effects to ring out.
TAIL = 7.0
# Sections the game plays only when it asks for them; they take no part in
# setting the song's levels.
SPECIAL = {"S", "F"}

_meter = pyln.Meter(SR)


class Span(NamedTuple):
    name: str
    beat: float
    beats: float
    start: int
    end: int


def piece(name):
    """The module in this package that composes the piece `name`."""
    return importlib.import_module(f"{__package__}.{name}")


def spans(mod):
    """Each section's name, first beat and length in beats, and its first
    and end frame when the sections play in order."""
    out, at = [], 0.0
    for name, chords in mod.SECTIONS:
        beats = len(chords) * 4.0
        out.append(Span(name, at, beats, secs(at * 60 / mod.BPM), secs((at + beats) * 60 / mod.BPM)))
        at += beats
    return out


def lufs(x):
    if np.abs(x).max() < 1e-6:
        return -120.0
    return _meter.integrated_loudness(x)


def fx(x, bpm, chorus=None, reverb=None, delay=None, hp=None, lp=None, crush=None):
    """reverb=(wet, decay, damp_hz, size, predelay); delay=(wet, l_beats, r_beats, fb, damp)."""
    n = x.shape[0]
    sections = []
    if hp:
        sections.append(dsp.hp_sos(hp))
    if lp:
        sections.append(dsp.lp_sos(lp))
    if sections:
        x = dsp.eq(x, *sections)
    if crush:
        x = dsp.crush(x, *crush)
    if chorus:
        x = dsp.chorus(x, *chorus)
    dry = x
    out = dry.copy()
    if delay:
        wet, lb, rb, fb, damp = delay
        d = dsp.delay(dry, lb * 60 / bpm, rb * 60 / bpm, fb, damp)[:n]
        out += d * wet
        dry = dry + d * wet * 0.5
    if reverb:
        wet, decay, damp, size, pre = reverb
        r = dsp.reverb(dsp.eq(dry, dsp.hp_sos(220)), decay, damp, size, pre)[:n]
        out += r * wet
    return out


def window(ev, a, b):
    """Events that start in [a, b), shifted so the section starts at 0."""
    return {bus: [dataclasses.replace(x, beat=x.beat - a) for x in items if a - 1e-6 <= x.beat < b - 1e-6]
            for bus, items in ev.items()}


def synth(mod, layer, bus, ev, n, seed):
    """One bus of one section, before effects; None when it is silent."""
    voices = mod.VOICES[layer]
    if bus in voices:
        voice, kw, _, _ = voices[bus]
        if voice == "riser":
            if not ev.get("riser"):
                return None
            x = np.zeros((n, 2))
            for h in ev["riser"]:
                x += ins.riser(n, mod.BPM, h.beat, kw.get("beats", 8), h.vel, seed=int(h.beat) + seed)
            return x
        if not ev.get(bus):
            return None
        return getattr(ins, voice)(ev[bus], n, mod.BPM, mod.SEED + sum(map(ord, bus)) + seed, **kw)
    kinds, _, _ = mod.DRUM_GROUPS[layer][bus]
    hits = [h for h in ev.get("drums", []) if h.kind in kinds]
    if not hits:
        return None
    buses = ins.drums(hits, n, mod.BPM, mod.SEED + seed)
    return sum((buses[k] for k in kinds if k in buses), np.zeros((n, 2)))


def master(mod, layer, x, period):
    x = dsp.wobble(x, period, **mod.WOBBLE)
    x = dsp.eq(x, dsp.hp_sos(28), *mod.MASTER_EQ(layer))
    if getattr(mod, "EXCITE", {}).get(layer):
        x = dsp.excite(x, **mod.EXCITE[layer])
    return dsp.compress(x, -16.0, 2.0, 0.02, 0.25, 6.0)


def render(mod, layer):
    """Every section of one layer in `spans(mod)` order, each its body plus
    TAIL seconds, mastered and limited to -1 dBFS."""
    bpm = mod.BPM
    ev = mod.build(layer)
    parts = spans(mod)
    tail = secs(TAIL)
    total = parts[-1].end
    normal_total = max(p.end for p in parts if p.name not in SPECIAL)
    mixes = [np.zeros((p.end - p.start + tail, 2)) for p in parts]
    windows = [window(ev, p.beat, p.beat + p.beats) for p in parts]
    duck = None
    if layer in getattr(mod, "SIDECHAIN", {}):
        curve = mod.SIDECHAIN[layer]["curve"]
        duck = [ins.sidechain(len(mix), bpm, [h.beat for h in w.get("drums", []) if h.kind == "kick"], **curve)
                for mix, w in zip(mixes, windows)]
    chains = [(bus, spec[-2], spec[-1]) for bus, spec in
              list(mod.VOICES[layer].items()) + list(mod.DRUM_GROUPS[layer].items())]
    for bus, effects, target in chains:
        ys = []
        for i, (w, mix) in enumerate(zip(windows, mixes)):
            x = synth(mod, layer, bus, w, len(mix), seed=97 * i)
            ys.append(None if x is None else fx(x, bpm, **effects))
        # the bus's level across the song played in order; a bus heard only
        # in the special sections is set by them instead
        song = np.zeros((total + tail, 2))
        for p, y in zip(parts, ys):
            if y is not None and p.name not in SPECIAL:
                song[p.start:p.start + len(y)] += y
        level = lufs(song[:normal_total])
        if level < -100:
            heard = [y[: p.end - p.start] for p, y in zip(parts, ys) if y is not None and p.name in SPECIAL]
            level = lufs(np.concatenate(heard)) if heard else -120.0
        if level < -100:
            continue
        gain = 10 ** ((target - level) / 20)
        for i, y in enumerate(ys):
            if y is None:
                continue
            y = y * gain
            if duck is not None and bus in mod.SIDECHAIN[layer]["buses"]:
                y *= duck[i][:, None]
            mixes[i] += y
    # every section starts at the same point of the tape wobble's curve
    period = parts[0].beats * 60 / bpm
    clips = [master(mod, layer, mix, period) for mix in mixes]
    song = np.zeros((total + tail, 2))
    for p, c in zip(parts, clips):
        if p.name not in SPECIAL:
            song[p.start:p.start + len(c)] += c
    gain = 10 ** ((mod.TARGET[layer] - lufs(song[:normal_total])) / 20)
    return [dsp.limit(c * gain, -1.0) for c in clips]
