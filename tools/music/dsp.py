"""Band-limited oscillators, filters, envelopes, space and dynamics for the
music voices. Audio is float64, mono (n,) or stereo (n, 2), at `SR`."""

import numpy as np
from numba import njit
from scipy.signal import sosfilt

SR = 44100


def secs(n):
    return int(round(n * SR))


# ---------------------------------------------------------------- oscillators


@njit(cache=True)
def _blep(t, dt):
    if t < dt:
        t /= dt
        return t + t - t * t - 1.0
    if t > 1.0 - dt:
        t = (t - 1.0) / dt
        return t * t + t + t + 1.0
    return 0.0


@njit(cache=True)
def saw(freq, phase):
    n = freq.shape[0]
    out = np.empty(n)
    p = phase
    for i in range(n):
        dt = freq[i] / SR
        out[i] = 2.0 * p - 1.0 - _blep(p, dt)
        p += dt
        if p >= 1.0:
            p -= 1.0
    return out


@njit(cache=True)
def pulse(freq, width, phase):
    n = freq.shape[0]
    out = np.empty(n)
    p = phase
    for i in range(n):
        dt = freq[i] / SR
        w = width[i]
        v = 1.0 if p < w else -1.0
        v += _blep(p, dt)
        q = p - w
        if q < 0.0:
            q += 1.0
        v -= _blep(q, dt)
        out[i] = v - (2.0 * w - 1.0)
        p += dt
        if p >= 1.0:
            p -= 1.0
    return out


# -------------------------------------------------------------------- filters


@njit(cache=True)
def svf(x, cutoff, res, mode):
    """Zavalishin TPT state-variable filter. mode 0 low, 1 band, 2 high.
    res 0..0.97 maps damping k from 2 down to 0.06."""
    n = x.shape[0]
    out = np.empty(n)
    ic1 = 0.0
    ic2 = 0.0
    k = 2.0 - 2.0 * res
    for i in range(n):
        fc = cutoff[i]
        if fc > SR * 0.45:
            fc = SR * 0.45
        if fc < 12.0:
            fc = 12.0
        g = np.tan(np.pi * fc / SR)
        a1 = 1.0 / (1.0 + g * (g + k))
        a2 = g * a1
        a3 = g * a2
        v3 = x[i] - ic2
        v1 = a1 * ic1 + a2 * v3
        v2 = ic2 + a2 * ic1 + a3 * v3
        ic1 = 2.0 * v1 - ic1
        ic2 = 2.0 * v2 - ic2
        if mode == 0:
            out[i] = v2
        elif mode == 1:
            out[i] = v1 * k
        else:
            out[i] = x[i] - k * v1 - v2
    return out


def lp24(x, cutoff, res=0.0):
    cutoff = np.broadcast_to(np.asarray(cutoff, dtype=np.float64), x.shape).copy()
    return svf(svf(x, cutoff, 0.0, 0), cutoff, res, 0)


def lp12(x, cutoff, res=0.0):
    cutoff = np.broadcast_to(np.asarray(cutoff, dtype=np.float64), x.shape).copy()
    return svf(x, cutoff, res, 0)


def hp12(x, cutoff, res=0.0):
    cutoff = np.broadcast_to(np.asarray(cutoff, dtype=np.float64), x.shape).copy()
    return svf(x, cutoff, res, 2)


def bp(x, cutoff, res=0.5):
    cutoff = np.broadcast_to(np.asarray(cutoff, dtype=np.float64), x.shape).copy()
    return svf(x, cutoff, res, 1)


def _biquad(b, a):
    return np.array([[b[0] / a[0], b[1] / a[0], b[2] / a[0], 1.0, a[1] / a[0], a[2] / a[0]]])


def peak_sos(f, gain_db, q):
    A = 10 ** (gain_db / 40)
    w = 2 * np.pi * f / SR
    al = np.sin(w) / (2 * q)
    return _biquad(
        [1 + al * A, -2 * np.cos(w), 1 - al * A], [1 + al / A, -2 * np.cos(w), 1 - al / A]
    )


def shelf_sos(f, gain_db, high, s=1.0):
    A = 10 ** (gain_db / 40)
    w = 2 * np.pi * f / SR
    al = np.sin(w) / 2 * np.sqrt((A + 1 / A) * (1 / s - 1) + 2)
    c = np.cos(w)
    sq = 2 * np.sqrt(A) * al
    if high:
        b = [A * ((A + 1) + (A - 1) * c + sq), -2 * A * ((A - 1) + (A + 1) * c), A * ((A + 1) + (A - 1) * c - sq)]
        a = [(A + 1) - (A - 1) * c + sq, 2 * ((A - 1) - (A + 1) * c), (A + 1) - (A - 1) * c - sq]
    else:
        b = [A * ((A + 1) - (A - 1) * c + sq), 2 * A * ((A - 1) - (A + 1) * c), A * ((A + 1) - (A - 1) * c - sq)]
        a = [(A + 1) + (A - 1) * c + sq, -2 * ((A - 1) + (A + 1) * c), (A + 1) + (A - 1) * c - sq]
    return _biquad(b, a)


def hp_sos(f, q=0.707):
    w = 2 * np.pi * f / SR
    al = np.sin(w) / (2 * q)
    c = np.cos(w)
    return _biquad([(1 + c) / 2, -(1 + c), (1 + c) / 2], [1 + al, -2 * c, 1 - al])


def lp_sos(f, q=0.707):
    w = 2 * np.pi * f / SR
    al = np.sin(w) / (2 * q)
    c = np.cos(w)
    return _biquad([(1 - c) / 2, 1 - c, (1 - c) / 2], [1 + al, -2 * c, 1 - al])


def eq(x, *sections):
    """Applies biquad sections to mono or stereo (n, 2) audio."""
    sos = np.vstack(sections)
    return sosfilt(sos, x, axis=0)


# ------------------------------------------------------------------ envelopes


def adsr(gate, a, d, s, r, sustain_tail=0.004):
    """Gate length and times in seconds. Exponential decay and release;
    the last few ms of the release ramp to zero so cuts never click."""
    ng = max(1, secs(gate))
    nr = max(1, secs(r))
    t = np.arange(ng) / SR
    a = max(a, 0.001)
    att = np.clip(t / a, 0, 1)
    att = 1 - (1 - att) ** 2
    dec = s + (1 - s) * np.exp(-np.maximum(t - a, 0) / max(d / 4, 1e-4))
    on = np.where(t < a, att, dec)
    level = on[-1]
    tr = np.arange(nr) / SR
    rel = level * np.exp(-tr / max(r / 5, 1e-4))
    tail = min(nr, secs(sustain_tail) + 1)
    rel[-tail:] *= np.linspace(1, 0, tail)
    return np.concatenate([on, rel])


def perc(length, decay, attack=0.001):
    n = secs(length)
    t = np.arange(n) / SR
    env = np.exp(-t / max(decay, 1e-4))
    na = max(1, secs(attack))
    env[:na] *= np.linspace(0, 1, na)
    tail = min(n, secs(0.004))
    env[-tail:] *= np.linspace(1, 0, tail)
    return env


# ---------------------------------------------------------------------- space


@njit(cache=True)
def _allpass(x, delay, g):
    n = x.shape[0]
    buf = np.zeros(delay)
    out = np.empty(n)
    idx = 0
    for i in range(n):
        d = buf[idx]
        v = x[i] + g * d
        out[i] = d - g * v
        buf[idx] = v
        idx += 1
        if idx >= delay:
            idx = 0
    return out


@njit(cache=True)
def _fdn(inl, inr, lengths, gains, damp):
    n = inl.shape[0]
    N = lengths.shape[0]
    maxlen = 0
    for j in range(N):
        if lengths[j] > maxlen:
            maxlen = lengths[j]
    bufs = np.zeros((N, maxlen))
    idx = np.zeros(N, dtype=np.int64)
    lp = np.zeros(N)
    outl = np.empty(n)
    outr = np.empty(n)
    taps = np.zeros(N)
    for i in range(n):
        s = 0.0
        for j in range(N):
            v = bufs[j, idx[j]]
            lp[j] = v + damp * (lp[j] - v)
            taps[j] = lp[j] * gains[j]
            s += taps[j]
        s *= 2.0 / N
        l = 0.0
        r = 0.0
        for j in range(N):
            if j % 2 == 0:
                l += taps[j] if (j // 2) % 2 == 0 else -taps[j]
            else:
                r += taps[j] if (j // 2) % 2 == 0 else -taps[j]
            inp = inl[i] if j % 2 == 0 else inr[i]
            bufs[j, idx[j]] = taps[j] - s + inp
            idx[j] += 1
            if idx[j] >= lengths[j]:
                idx[j] = 0
        outl[i] = l
        outr[i] = r
    return outl, outr


def reverb(x, decay=2.2, damp_hz=5000.0, size=1.0, predelay=0.02):
    """Stereo FDN reverb, 100% wet. x is (n, 2)."""
    pad = secs(decay * 1.5)
    inl = np.concatenate([x[:, 0], np.zeros(pad)])
    inr = np.concatenate([x[:, 1], np.zeros(pad)])
    for d, g in ((142, 0.7), (107, 0.7), (379, 0.6), (277, 0.6)):
        inl = _allpass(inl, d, g)
        inr = _allpass(inr, d + 13, g)
    base = np.array([1123, 1291, 1447, 1597, 1777, 1907, 2063, 2251])
    lengths = np.maximum(1, (base * size).astype(np.int64))
    gains = 10 ** (-3 * lengths / (SR * decay))
    damp = float(np.exp(-2 * np.pi * damp_hz / SR))
    l, r = _fdn(inl, inr, lengths, gains, damp)
    out = np.stack([l, r], axis=1) * 0.35
    pre = secs(predelay)
    out = np.concatenate([np.zeros((pre, 2)), out])[: x.shape[0] + pad]
    return out


@njit(cache=True)
def _pingpong(inl, inr, dl, dr, fb, damp):
    n = inl.shape[0]
    bl = np.zeros(dl)
    br = np.zeros(dr)
    il = 0
    ir = 0
    lpl = 0.0
    lpr = 0.0
    outl = np.empty(n)
    outr = np.empty(n)
    for i in range(n):
        vl = bl[il]
        vr = br[ir]
        lpl = vl + damp * (lpl - vl)
        lpr = vr + damp * (lpr - vr)
        outl[i] = vl
        outr[i] = vr
        bl[il] = inl[i] + lpr * fb
        br[ir] = inr[i] + lpl * fb
        il += 1
        if il >= dl:
            il = 0
        ir += 1
        if ir >= dr:
            ir = 0
    return outl, outr


def delay(x, time_l, time_r, feedback=0.35, damp_hz=3500.0, tail=3.0):
    """Cross-feedback stereo delay, 100% wet. x is (n, 2)."""
    pad = secs(tail)
    inl = np.concatenate([x[:, 0], np.zeros(pad)])
    inr = np.concatenate([x[:, 1], np.zeros(pad)])
    damp = float(np.exp(-2 * np.pi * damp_hz / SR))
    l, r = _pingpong(inl, inr, max(1, secs(time_l)), max(1, secs(time_r)), feedback, damp)
    return np.stack([l, r], axis=1)


@njit(cache=True)
def _moddelay(x, base, depth, rate, phase):
    n = x.shape[0]
    size = int(base + depth) + 4
    buf = np.zeros(size)
    out = np.empty(n)
    idx = 0
    for i in range(n):
        buf[idx] = x[i]
        d = base + depth * np.sin(2.0 * np.pi * (rate * i / SR + phase))
        pos = idx - d
        while pos < 0:
            pos += size
        i0 = int(pos)
        frac = pos - i0
        i1 = i0 + 1
        if i1 >= size:
            i1 -= size
        out[i] = buf[i0] * (1.0 - frac) + buf[i1] * frac
        idx += 1
        if idx >= size:
            idx = 0
    return out


def chorus(x, rate=0.35, depth_ms=3.0, base_ms=12.0, mix=0.5):
    """Stereo chorus on (n, 2) audio, LFOs a quarter cycle apart."""
    base = base_ms * SR / 1000
    depth = depth_ms * SR / 1000
    wl = _moddelay(x[:, 0].copy(), base, depth, rate, 0.0)
    wr = _moddelay(x[:, 1].copy(), base, depth, rate * 1.07, 0.25)
    return x * (1 - mix) + np.stack([wl, wr], axis=1) * mix


# ------------------------------------------------------------------- dynamics


def sat(x, drive=1.0):
    return np.tanh(x * drive) / np.tanh(drive)


@njit(cache=True)
def _follow(level, att, rel):
    n = level.shape[0]
    out = np.empty(n)
    e = 0.0
    for i in range(n):
        v = level[i]
        c = att if v > e else rel
        e = v + c * (e - v)
        out[i] = e
    return out


def compress(x, threshold_db=-18.0, ratio=3.0, attack=0.005, release=0.12, knee_db=6.0, makeup_db=0.0):
    level = np.abs(x).max(axis=1) if x.ndim == 2 else np.abs(x)
    att = float(np.exp(-1 / (attack * SR)))
    rel = float(np.exp(-1 / (release * SR)))
    env = _follow(level, att, rel)
    db = 20 * np.log10(np.maximum(env, 1e-7))
    over = db - threshold_db
    slope = 1 / ratio - 1
    red = np.where(
        2 * over <= -knee_db,
        0.0,
        np.where(2 * over >= knee_db, slope * over, slope * (over + knee_db / 2) ** 2 / (2 * knee_db)),
    )
    gain = 10 ** ((red + makeup_db) / 20)
    return x * (gain[:, None] if x.ndim == 2 else gain)


@njit(cache=True)
def _limit_gain(need, look, rel):
    n = need.shape[0]
    held = np.empty(n)
    # sliding minimum over the next `look` samples (inclusive)
    from_q = np.empty(n, dtype=np.int64)
    head = 0
    tail = 0
    for i in range(n + look):
        if i < n:
            while tail > head and need[from_q[tail - 1]] >= need[i]:
                tail -= 1
            from_q[tail] = i
            tail += 1
        j = i - look
        if j >= 0:
            while from_q[head] < j:
                head += 1
            held[j] = need[from_q[head]]
    out = np.empty(n)
    g = 1.0
    acc = 0.0
    win = np.empty(look)
    for k in range(look):
        win[k] = 1.0
    acc = float(look)
    wi = 0
    for i in range(n):
        h = held[i]
        g = h if h < g else h + rel * (g - h)
        acc += g - win[wi]
        win[wi] = g
        wi += 1
        if wi >= look:
            wi = 0
        sm = acc / look
        out[i] = sm if sm < g else g
    return out


def limit(x, ceiling_db=-1.0, look_ms=4.0, release_ms=80.0):
    ceiling = 10 ** (ceiling_db / 20)
    level = np.abs(x).max(axis=1)
    need = np.minimum(1.0, ceiling / np.maximum(level, 1e-9))
    look = max(2, int(look_ms * SR / 1000))
    rel = float(np.exp(-1 / (release_ms * SR / 1000)))
    g = _limit_gain(need, look, rel)
    out = x * g[:, None]
    return np.clip(out, -ceiling, ceiling)


# ------------------------------------------------------------------- mixing


def pan_gains(p):
    a = (np.clip(p, -1, 1) + 1) * np.pi / 4
    return np.cos(a) * np.sqrt(2), np.sin(a) * np.sqrt(2)


def to_stereo(mono, pan=0.0):
    gl, gr = pan_gains(pan)
    return np.stack([mono * gl, mono * gr], axis=1)


def add(buf, at, x):
    at = int(at)
    if at < 0:
        x = x[-at:]
        at = 0
    end = min(buf.shape[0], at + x.shape[0])
    if end > at:
        buf[at:end] += x[: end - at]


def midi_hz(n):
    return 440.0 * 2 ** ((np.asarray(n, dtype=np.float64) - 69) / 12)


@njit(cache=True)
def _vardelay(x, d):
    n = x.shape[0]
    size = 4096
    buf = np.zeros(size)
    out = np.empty(n)
    idx = 0
    for i in range(n):
        buf[idx] = x[i]
        pos = idx - d[i]
        while pos < 0:
            pos += size
        i0 = int(pos)
        frac = pos - i0
        i1 = i0 + 1
        if i1 >= size:
            i1 -= size
        out[i] = buf[i0] * (1.0 - frac) + buf[i1] * frac
        idx += 1
        if idx >= size:
            idx = 0
    return out


def wobble(x, period, wow_rate=0.43, wow_ms=0.45, flutter_rate=5.7, flutter_ms=0.05, seed=5):
    """Tape wow and flutter: one deterministic delay curve for both channels,
    so two layers given the same curve stay in step. The curve repeats every
    `period` seconds from the start, so separately rendered sections of that
    length all begin at the same point of the curve."""
    n = x.shape[0]
    rng = np.random.default_rng(seed)
    p1, p2, p3 = rng.random(3) * 6.28
    slow = wow_rate * 0.37
    wow_rate = max(1, round(wow_rate * period)) / period
    slow = max(1, round(slow * period)) / period
    flutter_rate = max(1, round(flutter_rate * period)) / period
    t = np.arange(n) / SR
    wow = np.sin(2 * np.pi * wow_rate * t + p1) * 0.7 + np.sin(2 * np.pi * slow * t + p2) * 0.3
    d = (wow_ms + wow_ms * wow + flutter_ms * np.sin(2 * np.pi * flutter_rate * t + p3)) * SR / 1000 + 2
    return np.stack([_vardelay(x[:, c].copy(), d) for c in range(2)], axis=1)


def crush(x, bits=11, hold=2, mix=0.4):
    """Parallel sample-hold and bit reduction."""
    y = x
    if hold > 1:
        y = np.repeat(x[::hold], hold, axis=0)[: x.shape[0]]
    q = 2 ** (bits - 1)
    y = np.round(y * q) / q
    return x * (1 - mix) + y * mix


def excite(x, freq=2500.0, drive=3.0, mix=0.25):
    """Adds upper harmonics generated from the content above `freq`."""
    hi = eq(x, hp_sos(freq), hp_sos(freq))
    gen = np.tanh(hi * drive * 4) / 4
    gen = eq(gen, hp_sos(freq * 1.4), hp_sos(freq * 1.4))
    return x + gen * mix
