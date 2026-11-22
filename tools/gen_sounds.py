# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "numpy==2.5.1",
#   "scipy==1.18.0",
# ]
# ///
"""Generates every sound in assets/sounds/.

Same contract as gen_sprites.py: this script is the source of truth — edit,
run `uv run tools/gen_sounds.py`, commit script and WAVs together. Output is
deterministic (seeded noise, pure synthesis, no timestamps).

The effects use dry, synth-first 8-bit-adjacent gestures, mono 16-bit PCM at
44100 Hz. tools/gen_music.py generates the music in assets/music/.

Pass --out DIRECTORY to write a complete alternate bank for review without
touching the checked-in assets.
"""

import argparse
import hashlib
import io
import json
import math
import wave
from pathlib import Path

import numpy as np
from scipy import signal as sig

SFX_RATE = 44100
OUT = Path(__file__).resolve().parent.parent / "assets" / "sounds"
GENERATED: dict[str, bytes] = {}

SFX_MAX_PEAK = 0.91
SFX_MAX_DC = 0.001
SFX_CATEGORY_BUDGETS = {
    "interface": (0.8, 0.50),
    "economy": (1.2, 0.50),
    "generic-weapon": (0.6, 0.50),
    "signature-weapon": (1.5, 0.50),
    "propulsion-loop": (4.0, 0.50),
    "machine-loop": (4.0, 0.50),
    "destruction": (1.5, 0.50),
    # The Spire's launch, warning, and detonation are rare, map-wide events
    # (one warhead a minute at most), so each may run longer than a weapon
    # report without crowding the mix; the cap still keeps one gesture.
    "strategic": (3.0, 0.50),
}
SFX_ATTACK_AUDIBILITY_NAMES = {
    "attack_breaker",
    "attack_piledriver",
    "demolition_boom",
    "attack_flakhound",
    "attack_flak_turret",
    "attack_bombard",
    "attack_bastion",
    "artillery_boom",
}
SFX_ATTACK_AUDIBLE_FLOOR = 0.45

# This table is shared by generation, validation, the review manifest, and the
# shell's mixer contract. Each row is name, category, level, minimum gap in
# seconds, and paired animation. The level is the clip's loudest moment as the
# mix plays it, in LUFS (see `momentary_peak_lufs`); the manifest's mixer
# volume is the gain that brings the clip there, so a regenerated clip keeps
# its place in the mix. The paired animation is optional: the generic viewer
# matches it when the selected directory tree happens to contain that file.
SFX_METADATA = (
    ("click", "interface", -27.0, 0.05, None),
    ("ack", "interface", -27.0, 0.15, None),
    ("denied", "interface", -23.0, 0.05, None),
    ("alert", "interface", -18.0, 1.50, None),
    ("deposit", "economy", -29.0, 1.20, "02-harvester-bite-cargo.gif"),
    ("train_done", "economy", -24.0, 0.40, "26-foundry-dark-gantry-animated.gif"),
    ("laser", "generic-weapon", -25.0, 0.09, "67-projectile-forge-spot-animated.gif"),
    ("laser2", "generic-weapon", -25.0, 0.09, "28-turret-open-ring-animated.gif"),
    (
        "attack_sentinel",
        "generic-weapon",
        -24.0,
        0.08,
        "18-sentinel-low-casemate-animated.gif",
    ),
    (
        "attack_talon",
        "generic-weapon",
        -24.0,
        0.10,
        "42-talon-compact-interceptor-animated.gif",
    ),
    (
        "attack_buzzard",
        "generic-weapon",
        -22.0,
        0.10,
        "38-buzzard-quad-fan-carriage-animated.gif",
    ),
    ("attack_roc", "generic-weapon", -22.0, 0.10, None),
    ("attack_drone", "generic-weapon", -26.0, 0.08, None),
    ("drone_launch", "signature-weapon", -23.0, 0.50, None),
    ("drone_dock", "signature-weapon", -27.0, 0.12, None),
    (
        "attack_lancer",
        "signature-weapon",
        -22.0,
        0.15,
        "36-lancer-dark-channel-animated.gif",
    ),
    (
        "attack_scuttler",
        "signature-weapon",
        -24.0,
        0.09,
        "20-scuttler-centipede-shear-animated.gif",
    ),
    (
        "attack_flakhound",
        "signature-weapon",
        -19.0,
        0.12,
        "04-flakhound-paired-yoke-carrier.gif",
    ),
    (
        "attack_flak_turret",
        "signature-weapon",
        -19.0,
        0.12,
        "01-flak-turret-paired-yokes.gif",
    ),
    ("attack_warden", "generic-weapon", -22.0, 0.10, None),
    ("attack_breaker", "signature-weapon", -18.0, 0.20, None),
    ("attack_piledriver", "signature-weapon", -12.0, 0.20, None),
    ("piledriver_impact", "signature-weapon", -16.0, 0.20, None),
    ("piledriver_deploy", "signature-weapon", -37.0, 0.15, None),
    ("piledriver_fold", "signature-weapon", -41.0, 0.15, None),
    ("avalanche_launch", "signature-weapon", -15.0, 0.20, None),
    ("avalanche_motor", "propulsion-loop", -20.0, 0.20, None),
    ("rocket_impact", "signature-weapon", -15.0, 0.20, None),
    ("bomb_release", "signature-weapon", -15.0, 0.20, None),
    ("demolition_boom", "destruction", -13.0, 0.20, None),
    ("upgrade_done", "economy", -23.0, 0.30, None),
    ("intercept_zap", "generic-weapon", -24.0, 0.08, None),
    ("spire_launch", "strategic", -13.0, 0.50, None),
    ("incoming_warning", "strategic", -17.0, 1.50, None),
    ("warhead_detonation", "strategic", -10.0, 0.50, None),
    ("interceptor_launch", "signature-weapon", -20.0, 0.20, None),
    ("interception", "signature-weapon", -14.0, 0.20, None),
    ("warhead_flight", "propulsion-loop", -17.0, 0.20, None),
    ("interceptor_flight", "propulsion-loop", -20.0, 0.20, None),
    ("maw_cut", "machine-loop", -20.0, 0.20, None),
    (
        "attack_bombard",
        "signature-weapon",
        -18.0,
        0.20,
        "22-bombard-recoil-spade-animated.gif",
    ),
    (
        "attack_bastion",
        "signature-weapon",
        -18.0,
        0.20,
        "32-bastion-buttressed-turntable-animated.gif",
    ),
    (
        "artillery_launch",
        "signature-weapon",
        -19.0,
        0.20,
        "69-projectile-heated-nose-shell-animated.gif",
    ),
    (
        "artillery_boom",
        "signature-weapon",
        -17.0,
        0.20,
        "69-projectile-heated-nose-shell-animated.gif",
    ),
    ("unit_death", "destruction", -23.0, 0.12, None),
    ("building_boom", "destruction", -16.0, 0.05, None),
)

# The approved bytes of every clip, accepted by ear. They are checked after
# synthesis so a library or recipe change cannot silently replace a clip.
EXPECTED_SFX_SHA256 = {
    "piledriver_fold.wav": "72a6539c1588ef3cec75f449920522d207a65a2f72c48ed44a25702f7edcfd20",
    "piledriver_deploy.wav": "4a4dbc55cb8d23a0cb745d0c99a470e6e87da34203ec359b93e0642b6297b6b3",
    "piledriver_impact.wav": "f4bc67ea38bd6b5cac6de737039b6489a13a4670f696910480e4275fd914e96e",
    "attack_piledriver.wav": "c053a864ebee0c34b74df23ec24e9ea6f44abc5f39ace4cd35a6e1ba64a8ca06",
    "avalanche_motor.wav": "13d76ec852d543be05ebc53a3dee75ba24082f476504c0ccec2be51cdca4a68b",
    "rocket_impact.wav": "037944ae136afdb1c69d34f2f72c6642d1a775663f7ab26fa1c75358af566d0e",
    "ack.wav": "0450d0bb65444c92d60082978685f135c910979446425aa71c75d97842a1081f",
    "alert.wav": "7acef654cf90a5c7df6bc4effd15f533c077c2d7bb5d9067bb863bec32898398",
    "artillery_boom.wav": "698fa419ff73018bfdb638c4a13f740da2ca7b2398ee58a8decad778d5de3597",
    "artillery_launch.wav": "f593a4702d758c908aa973c868a8e61adb46a2b9585be3093f5fd8451f773950",
    "attack_bastion.wav": "6d4198a9e435715cf6f85458fc64820b6efb76579253f375d32248fee418ad5d",
    "attack_bombard.wav": "41a22ab06fb37fb610417ca28d8a008d0f487b6d2ab8ad7c15d61ab5ca648de7",
    "attack_buzzard.wav": "1ca99804b3a0db9dd921f6e7eb3211368622d8d1efcae17bca1e7670ef5cfc73",
    "attack_flak_turret.wav": "7c2ff38fbed0a2a6de489ff6493b468318521ff4c43d4bb36e973a17a9397110",
    "attack_flakhound.wav": "a962ea511f2603a11010abb7a3f2e6067377a3a5cd2d68982637ab1a29b24d62",
    "attack_lancer.wav": "6669626a430c184db5e4349b3aee202577e3bb5daece6f751af311330bbb2099",
    "attack_scuttler.wav": "8ec9947be252a33d3b07955d3d0b76e840f7837cc74204882b0c167c7667f686",
    "attack_sentinel.wav": "2c72d99ac0ee6cca8c9e9a54e34ad0262fc69731e3d00e378f5b0ea93e352df1",
    "attack_talon.wav": "81226ded79cc29a6f5e9edb8ebdcd25cee6fec04877c3e21bac0fb1de55a1428",
    "attack_roc.wav": "3c1b0a6f48788eb4d27cd3ebb609f6149ed242b27ea66040b9663091bb36e497",
    "attack_drone.wav": "64b9d387a2f8d6dffa81ea048cc6289c543da03a41f759a459f69a5a0a7cfcdc",
    "drone_launch.wav": "5c95724f5dc530d351ffe485db2e76fca7098239f599ce6de8c6071bcf2eea94",
    "drone_dock.wav": "fa9550ccb7ac33af39773ec3259038baee5cf866cffeacf674b334c68ec324f0",
    "building_boom.wav": "47445ba8778ef6badf80ae326f55ccf5a3e65dd4931c320978ef620ceaeeef12",
    "click.wav": "8d148979dc943755b924611a63317e8e48830d0f6e0fe2a5277b97dc05301da4",
    "denied.wav": "e77550193b4ce7615f6aae8afe9e817f55c8df84ff58361b5703d6cd0de2714c",
    "deposit.wav": "d92bbc7bfd413943db5fe5bd52ef03fcb318ae550a7eb8291fca80ddf6e0a036",
    "attack_breaker.wav": "ba6243d0e05d418042a83c969d110a5a4b9584fd7d32f6cd8bfce9fc76d9baf8",
    "attack_warden.wav": "9c094a4623eeca1d205db2eba26efbb54c9d559bdb56871eae79e7428739697c",
    "avalanche_launch.wav": "ab8d9a4a2ba9955108ba3e43aaa80b80e86ef8dc947450b53ca8db74aa2b8a85",
    "bomb_release.wav": "d30017fd87f0c0ea6d195fa9b673d7d9fbe653923f5478234959bda226a7c2e8",
    "demolition_boom.wav": "9df7c7933b14acf0c3045597a5ad1d5646b8c150f2b8473434ef2a65f763c5b9",
    "upgrade_done.wav": "f60e6045845b53946224d33ce1cc8dbf96671903893a36fe3a0738c73ea646f0",
    "laser.wav": "252519a8fb00d4587deefb182d1025c2d6c309d63f51d176119e39f24bdf63cd",
    "laser2.wav": "b44d1b4311b750fc6005b22d1a6021db39d1dbdbddabce7dd240d19a0b5ac1b3",
    "train_done.wav": "6a5c193c7c749915cb7356aa3b2b426db50ffd3a017af2466a93dd17c1937827",
    "unit_death.wav": "b875ac45ad46385a8c5acc3f29f150496fb39071dc2c2bcb036b5d28b0538fec",
    "intercept_zap.wav": "4bedf27c34fe11970a0109a9ff81a627eb2c67569eca1b35cc2f623e6269b18a",
    "spire_launch.wav": "8738e26f6daec50f38ff9d381682a42a143521b09de4ac96b9e153cd7c18039c",
    "incoming_warning.wav": "4abc0168ecd66353cbec9a9e2316964f2a696e769671c54370b0807b37cc934d",
    "warhead_detonation.wav": "061059717a4a7b43abf92c4150674d603f97a2f0603cb6a2332faa3a679c4040",
    "interceptor_launch.wav": "5d3c7e6a91c406c81762be260bb38b3169d3d584cc2ff82951c1df543b3c56d6",
    "interception.wav": "d2c2412d0567d65108ce1096880ef576fde7029706ee36fd65ec7e77cb350293",
    "warhead_flight.wav": "b85c23aae2abfb1fc247c7e7e6998dcef0d4b71575057333b604aa7f6334f9c2",
    "interceptor_flight.wav": "077aab9f1523779c6ae787fef587eef67dc7dff11da3ca168eef4528f5d2f1c1",
    "maw_cut.wav": "39815e65e830e5361bcfa9114e32aa625c75d9771ae93666b7da9d96d6c20265",
}
SFX_ROUNDED_PCM = {
    "avalanche_launch",
    "avalanche_motor",
    "rocket_impact",
    "spire_launch",
    "incoming_warning",
    "warhead_detonation",
    "interceptor_launch",
    "interception",
    "warhead_flight",
    "interceptor_flight",
}
SFX_LOOP_NAMES = {
    "avalanche_motor",
    "warhead_flight",
    "interceptor_flight",
    "maw_cut",
}
SFX_META_BY_NAME = {row[0]: row for row in SFX_METADATA}


def validate_sfx_wav(name: str, data: bytes) -> dict[str, float]:
    """Checks PCM shape, headroom, duration, DC, and speaker audibility."""
    try:
        _, category, _, _, _ = SFX_META_BY_NAME[name]
    except KeyError as error:
        raise ValueError(f"{name}: no SFX metadata") from error

    with wave.open(io.BytesIO(data), "rb") as source:
        if (
            source.getnchannels(),
            source.getsampwidth(),
            source.getframerate(),
            source.getcomptype(),
        ) != (1, 2, SFX_RATE, "NONE"):
            raise ValueError(
                f"{name}: effects must be mono 16-bit uncompressed PCM at {SFX_RATE} Hz"
            )
        frame_count = source.getnframes()
        frames = source.readframes(frame_count)

    if frame_count == 0:
        raise ValueError(f"{name}: effect is empty")
    pcm = np.frombuffer(frames, dtype="<i2")
    normalized = pcm.astype(np.float64) / 32768.0
    duration = frame_count / SFX_RATE
    peak = float(np.max(np.abs(normalized)))
    dc = abs(float(np.mean(normalized)))
    max_duration, category_floor = SFX_CATEGORY_BUDGETS[category]
    audible_floor = 0.30 if name == "artillery_launch" else category_floor

    spectrum = np.abs(np.fft.rfft(normalized)) ** 2
    frequencies = np.fft.rfftfreq(frame_count, 1.0 / SFX_RATE)
    total_energy = float(np.sum(spectrum))
    audible = (
        float(np.sum(spectrum[frequencies >= 180.0])) / total_energy
        if total_energy > 0.0
        else 0.0
    )
    attack_frames = min(frame_count, round(0.3 * SFX_RATE))
    attack = normalized[:attack_frames]
    attack_spectrum = np.abs(np.fft.rfft(attack)) ** 2
    attack_frequencies = np.fft.rfftfreq(attack_frames, 1.0 / SFX_RATE)
    attack_energy = float(np.sum(attack_spectrum))
    attack_audible = (
        float(np.sum(attack_spectrum[attack_frequencies >= 180.0])) / attack_energy
        if attack_energy > 0.0
        else 0.0
    )
    if int(np.max(np.abs(pcm.astype(np.int32)))) >= 32767 or peak > SFX_MAX_PEAK:
        raise ValueError(f"{name}: peak {peak:.6f} leaves inadequate headroom")
    if dc > SFX_MAX_DC:
        raise ValueError(f"{name}: DC offset {dc:.6f} exceeds {SFX_MAX_DC}")
    if duration > max_duration:
        raise ValueError(
            f"{name}: duration {duration:.3f}s exceeds {max_duration:.3f}s "
            f"for {category}"
        )
    if audible < audible_floor:
        raise ValueError(
            f"{name}: only {audible:.1%} of spectral energy is at or above "
            f"180 Hz; {audible_floor:.0%} required"
        )
    if (
        name in SFX_ATTACK_AUDIBILITY_NAMES
        and attack_audible < SFX_ATTACK_AUDIBLE_FLOOR
    ):
        raise ValueError(
            f"{name}: only {attack_audible:.1%} of its first 300 ms is at or "
            f"above 180 Hz; {SFX_ATTACK_AUDIBLE_FLOOR:.0%} required"
        )
    if name in SFX_LOOP_NAMES:
        if frame_count != 4 * SFX_RATE:
            raise ValueError(
                f"{name}: a loop must contain exactly four seconds"
            )
        if abs(float(normalized[0] - normalized[-1])) > 0.001:
            raise ValueError(f"{name}: loop seam exceeds 0.001")
        rms = float(np.sqrt(np.mean(normalized**2)))
        if rms > 0.16 or peak > 0.5:
            raise ValueError(
                f"{name}: loop level exceeds the compact motor budget"
            )
        if audible < 0.98:
            raise ValueError(f"{name}: loop has excessive low-frequency energy")
    return {
        "duration": duration,
        "peak": peak,
        "dc": dc,
        "audible": audible,
        "attack_audible": attack_audible,
    }


# ----------------------------------------------------------------- SFX kit


def sfx_axis(n: int) -> np.ndarray:
    return np.arange(n) / SFX_RATE


def sfx_decay(n: int, sharp: float = 5.0) -> np.ndarray:
    return np.exp(-sharp * np.arange(n) / n)


def sfx_attack(samples: np.ndarray, milliseconds: float) -> np.ndarray:
    n = min(len(samples), max(1, int(SFX_RATE * milliseconds / 1000)))
    ramp = 0.5 - 0.5 * np.cos(np.linspace(0.0, np.pi, n))
    result = samples.copy()
    result[:n] *= ramp
    return result


def sfx_fade_edges(samples: np.ndarray, milliseconds: float = 4.0) -> np.ndarray:
    n = min(len(samples) // 2, max(1, int(SFX_RATE * milliseconds / 1000)))
    result = samples.copy()
    result[:n] *= np.linspace(0.0, 1.0, n)
    result[-n:] *= np.linspace(1.0, 0.0, n)
    return result


def sfx_glide(n: int, start: float, end: float) -> np.ndarray:
    return start * (end / start) ** np.linspace(0.0, 1.0, n)


def sfx_tone(frequencies: np.ndarray, phase: float = 0.0) -> np.ndarray:
    return np.sin(2.0 * np.pi * np.cumsum(frequencies) / SFX_RATE + phase)


def sfx_softsquare(frequencies: np.ndarray, hard: float = 2.5) -> np.ndarray:
    return np.tanh(hard * sfx_tone(frequencies)) / np.tanh(hard)


def sfx_saw(frequencies: np.ndarray) -> np.ndarray:
    phase = np.cumsum(frequencies) / SFX_RATE
    return sig.sawtooth(2.0 * np.pi * phase)


def sfx_bandpass(
    samples: np.ndarray, low: float, high: float, order: int = 4
) -> np.ndarray:
    high = min(high, SFX_RATE / 2 - 100)
    sos = sig.butter(order, [low, high], btype="bandpass", fs=SFX_RATE, output="sos")
    return sig.sosfilt(sos, samples)


def sfx_lowpass(samples: np.ndarray, cutoff: float, order: int = 2) -> np.ndarray:
    sos = sig.butter(
        order,
        min(cutoff, SFX_RATE / 2 - 100),
        btype="lowpass",
        fs=SFX_RATE,
        output="sos",
    )
    return sig.sosfilt(sos, samples)


def sfx_highpass(samples: np.ndarray, cutoff: float, order: int = 2) -> np.ndarray:
    sos = sig.butter(order, cutoff, btype="highpass", fs=SFX_RATE, output="sos")
    return sig.sosfilt(sos, samples)


def sfx_comb(
    samples: np.ndarray, delay: int, gain: float, damping: float = 0.25
) -> np.ndarray:
    delay = max(2, delay)
    denominator = np.zeros(delay + 2)
    denominator[0] = 1.0
    denominator[delay] = -gain * (1.0 - damping)
    denominator[delay + 1] = -gain * damping
    return sig.lfilter([1.0], denominator, samples)


def sfx_allpass(samples: np.ndarray, delay: int, gain: float = 0.5) -> np.ndarray:
    numerator = np.zeros(delay + 1)
    denominator = np.zeros(delay + 1)
    numerator[0], numerator[delay] = -gain, 1.0
    denominator[0], denominator[delay] = 1.0, -gain
    return sig.lfilter(numerator, denominator, samples)


def sfx_space(
    samples: np.ndarray,
    mix: float = 0.16,
    decay_amount: float = 0.74,
    tail: float = 0.5,
) -> np.ndarray:
    """Adds the short dark tail used only by approved interface sounds."""
    dry = np.concatenate([samples, np.zeros(int(tail * SFX_RATE))])
    wet = np.zeros_like(dry)
    for delay in (1687, 1931, 2237, 2617):
        wet += sfx_comb(dry, delay, decay_amount, damping=0.5)
    wet /= 4.0
    wet = sfx_allpass(wet, 347, 0.5)
    wet = sfx_allpass(wet, 113, 0.5)
    wet = sfx_lowpass(wet, 3800)
    predelay = int(0.011 * SFX_RATE)
    wet = np.concatenate([np.zeros(predelay), wet[:-predelay]])
    return dry + mix * wet


def sfx_drive(samples: np.ndarray, amount: float = 1.8) -> np.ndarray:
    return np.tanh(amount * samples) / np.tanh(amount)


def sfx_crush(samples: np.ndarray, bits: int = 10, hold: int = 3) -> np.ndarray:
    levels = float(2 ** (bits - 1))
    result = np.round(samples * levels) / levels
    if hold > 1:
        result = np.repeat(result[::hold], hold)[: len(result)]
    return result


def sfx_ringmod(
    samples: np.ndarray, frequency: float, depth: float = 0.5
) -> np.ndarray:
    return samples * (
        1.0 - depth + depth * np.sin(2.0 * np.pi * frequency * sfx_axis(len(samples)))
    )


def sfx_noise(n: int, seed: int) -> np.ndarray:
    return np.random.default_rng(seed).uniform(-1.0, 1.0, n)


def sfx_ping(
    frequency: float,
    duration: float,
    seed: int,
    damping: float = 0.35,
    sharp: float = 6.0,
) -> np.ndarray:
    n = int(duration * SFX_RATE)
    burst = np.zeros(n)
    burst[:48] = sfx_noise(48, seed)
    result = sfx_comb(burst, int(SFX_RATE / frequency), 0.994, damping=damping)
    result = sfx_bandpass(result, frequency * 0.5, frequency * 4.5)
    return result * sfx_decay(n, sharp)


def sfx_bell(
    n: int,
    frequency: float,
    ratio: float = 3.01,
    index: float = 1.8,
    sharp: float = 5.0,
    detune: float = 0.004,
) -> np.ndarray:
    axis = sfx_axis(n)
    result = np.zeros(n)
    for offset in (1.0 - detune, 1.0 + detune):
        modulation = (
            np.sin(2.0 * np.pi * frequency * ratio * offset * axis)
            * index
            * sfx_decay(n, sharp * 1.4)
        )
        result += np.sin(2.0 * np.pi * frequency * offset * axis + modulation)
    return result / 2.0 * sfx_decay(n, sharp)


def sfx_body(
    frequencies: np.ndarray,
    amplitudes: tuple[float, ...] = (1.0, 0.6, 0.35, 0.18),
) -> np.ndarray:
    result = np.zeros(len(frequencies))
    for harmonic, amplitude in enumerate(amplitudes, start=1):
        result += amplitude * sfx_tone(frequencies * harmonic)
    return result / sum(amplitudes)


class SfxCanvas:
    def __init__(self, duration: float):
        self.buffer = np.zeros(int(duration * SFX_RATE))

    def add(
        self, samples: np.ndarray, at: float = 0.0, gain: float = 1.0
    ) -> "SfxCanvas":
        start = int(at * SFX_RATE)
        required = start + len(samples)
        if required > len(self.buffer):
            self.buffer = np.pad(self.buffer, (0, required - len(self.buffer)))
        self.buffer[start:required] += samples * gain
        return self

    def output(self) -> np.ndarray:
        return self.buffer


def sfx_finish(
    samples: np.ndarray, highpass: float = 70.0, peak: float = 0.88
) -> np.ndarray:
    result = sfx_highpass(samples, highpass)
    result -= np.mean(result)
    result = sfx_fade_edges(result, 4.0)
    maximum = np.max(np.abs(result))
    return result * (peak / maximum) if maximum > 0 else result


def sfx_pwm_sweep(
    start: float,
    end: float,
    duration: float,
    duty_start: float = 0.5,
    duty_end: float = 0.22,
) -> np.ndarray:
    n = int(duration * SFX_RATE)
    phase = np.cumsum(sfx_glide(n, start, end)) / SFX_RATE % 1.0
    duty = np.linspace(duty_start, duty_end, n)
    return np.where(phase < duty, 1.0, -1.0)


def sfx_sync_zap(
    start: float,
    end: float,
    duration: float,
    ratio_start: float = 3.2,
    ratio_end: float = 1.3,
) -> np.ndarray:
    n = int(duration * SFX_RATE)
    fraction = np.cumsum(sfx_glide(n, start, end)) / SFX_RATE % 1.0
    ratio = np.linspace(ratio_start, ratio_end, n)
    return np.sin(2.0 * np.pi * ratio * fraction)


def sfx_zap(
    seed: int,
    start: float,
    end: float,
    duration: float,
    thump: float = 0.45,
    snap: float = 0.3,
    sync_gain: float = 0.35,
    sharp: float = 6.0,
) -> np.ndarray:
    canvas = SfxCanvas(duration + 0.06)
    n = int(duration * SFX_RATE)
    canvas.add(sfx_pwm_sweep(start, end, duration) * sfx_decay(n, sharp), 0.0, 0.75)
    canvas.add(
        sfx_sync_zap(start, end, duration) * sfx_decay(n, sharp * 1.1),
        0.0,
        sync_gain,
    )
    if snap:
        canvas.add(
            sfx_bandpass(sfx_noise(int(0.003 * SFX_RATE), seed), 2000, 7000),
            0.0,
            snap,
        )
    if thump:
        thump_frames = int(min(duration, 0.09) * SFX_RATE)
        canvas.add(
            sfx_drive(
                sfx_tone(sfx_glide(thump_frames, end * 1.6, end * 0.9))
                * sfx_decay(thump_frames, 7.0),
                2.5,
            ),
            0.002,
            thump,
        )
    return canvas.output()


def sfx_swept_lowres(
    samples: np.ndarray,
    start: float,
    end: float,
    resonance: float = 6.0,
    block: int = 64,
) -> np.ndarray:
    cutoffs = sfx_glide(len(samples), start, end)
    result = np.zeros_like(samples)
    state = np.zeros((1, 2))
    for begin in range(0, len(samples), block):
        finish = min(begin + block, len(samples))
        angle = 2.0 * np.pi * min(cutoffs[begin], SFX_RATE / 2 - 200) / SFX_RATE
        alpha = np.sin(angle) / (2.0 * resonance)
        cosine = np.cos(angle)
        denominator = 1.0 + alpha
        sos = np.array(
            [
                [
                    (1 - cosine) / 2 / denominator,
                    (1 - cosine) / denominator,
                    (1 - cosine) / 2 / denominator,
                    1.0,
                    -2 * cosine / denominator,
                    (1 - alpha) / denominator,
                ]
            ]
        )
        result[begin:finish], state = sig.sosfilt(sos, samples[begin:finish], zi=state)
    return result / (np.max(np.abs(result)) + 1e-9)


def sfx_pound(
    seed: int,
    start: float,
    end: float,
    duration: float,
    bloom: float = 0.0,
    knock: float = 0.55,
    drive_amount: float = 3.0,
    sharp: float = 5.0,
) -> np.ndarray:
    n = int(duration * SFX_RATE)
    canvas = SfxCanvas(duration + 0.05)
    core = (
        0.55 * sfx_tone(sfx_glide(n, start, end))
        + sfx_tone(sfx_glide(n, 2 * start, 2 * end))
        + 0.5 * sfx_tone(sfx_glide(n, 3 * start, 3 * end))
    ) / 2.05
    canvas.add(sfx_drive(core * sfx_decay(n, sharp), drive_amount))
    if knock:
        knock_frames = int(min(duration, 0.1) * SFX_RATE)
        canvas.add(
            sfx_drive(
                sfx_tone(sfx_glide(knock_frames, 340.0, 210.0))
                * sfx_decay(knock_frames, 7.5),
                2.5,
            ),
            0.001,
            knock,
        )
    if bloom:
        canvas.add(
            sfx_swept_lowres(sfx_noise(n, seed + 1), 1600.0, 200.0, resonance=8.0)
            * sfx_decay(n, sharp),
            0.004,
            bloom,
        )
    return canvas.output()


def sfx_crack(seed: int, noise_gain: float = 0.8) -> np.ndarray:
    zap_frames = int(0.016 * SFX_RATE)
    zap = sfx_tone(sfx_glide(zap_frames, 3200.0, 380.0)) * sfx_decay(zap_frames, 9.0)
    noise_frames = int(0.01 * SFX_RATE)
    noise_layer = sfx_bandpass(sfx_noise(noise_frames, seed), 900, 8000) * sfx_decay(
        noise_frames, 6.0
    )
    canvas = SfxCanvas(0.03).add(sfx_drive(zap, 2.5)).add(noise_layer, 0.0, noise_gain)
    return sfx_drive(canvas.output(), 2.0)


def sfx_slapback(
    samples: np.ndarray, gap: float = 0.1, times: int = 2, gain: float = 0.3
) -> np.ndarray:
    result = np.zeros(len(samples) + int(gap * times * SFX_RATE) + int(0.05 * SFX_RATE))
    result[: len(samples)] += samples
    echo = samples
    for repeat in range(1, times + 1):
        echo = sfx_lowpass(echo, 2400.0) * gain
        start = int(repeat * gap * SFX_RATE)
        result[start : start + len(echo)] += echo
    return result

def settling_phase(t: np.ndarray, base: float, extra: float, tau: float) -> np.ndarray:
    """Phase of a tone that starts `extra` Hz above `base` and settles."""
    return 2 * np.pi * (base * t + extra * tau * (1 - np.exp(-t / tau)))


def ground_reflection(
    dry: np.ndarray, milliseconds: float, gain: float, cutoff: float
) -> np.ndarray:
    result = dry.copy()
    wet = sfx_lowpass(dry, cutoff)
    delay = round(SFX_RATE * milliseconds / 1000)
    result[delay:] += wet[:-delay] * gain
    return result


def sfx_pad(samples: np.ndarray, seconds: float) -> np.ndarray:
    n = round(seconds * SFX_RATE)
    return np.pad(samples, (0, max(0, n - len(samples))))[:n]


def sfx_burst(seconds: float, seed: int, low: float, high: float, rate: float) -> np.ndarray:
    t = strategic_axis(seconds)
    return rocket_noise(seconds, seed, low, high) * np.exp(-t * rate)


def sfx_snap(
    seed: int, seconds: float = 0.006, low: float = 2500, high: float = 9000
) -> np.ndarray:
    n = round(seconds * SFX_RATE)
    return sfx_bandpass(sfx_noise(n, seed), low, high) * sfx_decay(n, 5)


def sfx_tube(
    seed: int, base: float = 95.0, seconds: float = 0.3, sharp: float = 6.0
) -> np.ndarray:
    """A tube closed at one end rings on odd harmonics only; a comb with
    negative feedback over half a period of its base does the same."""
    n = round(seconds * SFX_RATE)
    excite = np.zeros(n)
    excite[:260] = sfx_noise(260, seed) * np.linspace(1, 0, 260)
    hollow = sfx_comb(excite, round(SFX_RATE / (2 * base)), -0.9, damping=0.3)
    return sfx_bandpass(hollow, 150, 1500) * sfx_decay(n, sharp)


def sfx_puff(seed: int, seconds: float = 0.14, cutoff: float = 1200.0) -> np.ndarray:
    n = round(seconds * SFX_RATE)
    return sfx_attack(sfx_lowpass(sfx_noise(n, seed), cutoff) * sfx_decay(n, 5), 5.0)


def sfx_explosion(seed: int, seconds: float, discharge_gain: float) -> np.ndarray:
    """The rocket impact's blast: fracture, pressure, a low body, and an
    electric discharge riding the bloom."""
    t = strategic_axis(seconds)
    fracture = rocket_noise(seconds, seed, 250, 7100) * np.exp(-t * 42)
    pressure = rocket_noise(seconds, seed + 1, 35, 900) * np.exp(-t * 5.8)
    bloom = (1 - np.exp(-t * 70)) * np.exp(-t * 5.4)
    air = rocket_noise(seconds, seed + 2, 280, 3400)
    phase = settling_phase(t, 73.42, 240, 0.065)
    discharge = np.sin(phase + 2.6 * np.exp(-t * 5) * np.sin(phase * 2.0)) + 0.3 * np.sin(
        phase * 3.0
    )
    textured = air * (0.62 + 0.38 * np.sin(phase * 1.5))
    low = settling_phase(t, 43, 40, 0.055)
    body = (np.sin(low) + 0.25 * np.sin(3 * low)) * np.exp(-t * 7)
    dry = (
        0.19 * fracture
        + 0.62 * pressure
        + 0.32 * body
        + (0.6 * textured + discharge_gain * discharge) * bloom
    )
    return ground_reflection(dry, 43, 0.12, 1800)


def sfx_flak_pair(
    seed: int,
    gap: float,
    pitch: float,
    end: float,
    seconds: float,
    bloom: float,
    turret: bool,
) -> np.ndarray:
    """Two offset barrels: each a crack, a pound and a blast of air."""
    canvas = SfxCanvas(0.5 if turret else 0.42)
    for index, at in enumerate((0.0, gap)):
        canvas.add(sfx_crack(seed + 3 * index), at, (0.9 if turret else 0.85) - 0.1 * index)
        canvas.add(
            sfx_pound(
                seed + 1 + 3 * index,
                pitch * (1.0 + 0.06 * index),
                end,
                seconds,
                bloom=bloom,
            ),
            at + 0.003,
            1.0 - 0.15 * index,
        )
        canvas.add(sfx_burst(0.06, seed + 2 + 3 * index, 150, 2500, 40), at + 0.002, 0.45)
    result = sfx_crush(sfx_drive(canvas.output(), 1.6), 10, 2)
    if turret:
        result = sfx_slapback(result, 0.11, 1, 0.22)
    return sfx_finish(result, highpass=60, peak=0.88)


# -------------------------------------------------------- approved SFX bank


def sfx_click() -> np.ndarray:
    n = int(0.035 * SFX_RATE)
    tick = sfx_bandpass(sfx_noise(n, 101), 2800, 7000) * sfx_decay(n, 22.0)
    tap = sfx_tone(np.full(n, 1150.0)) * sfx_decay(n, 18.0)
    return sfx_finish(0.4 * tick + 0.8 * tap, highpass=200, peak=0.7)


def sfx_ack() -> np.ndarray:
    canvas = SfxCanvas(0.16)
    first = int(0.05 * SFX_RATE)
    second = int(0.06 * SFX_RATE)
    canvas.add(
        sfx_softsquare(np.full(first, 622.0), 2.2) * sfx_decay(first, 8.0),
        0.0,
        0.55,
    )
    canvas.add(
        sfx_softsquare(np.full(second, 932.0), 2.2) * sfx_decay(second, 7.0),
        0.055,
        0.5,
    )
    canvas.add(
        sfx_tone(np.full(second, 2489.0)) * sfx_decay(second, 12.0),
        0.055,
        0.08,
    )
    return sfx_finish(
        sfx_space(sfx_crush(canvas.output(), 11, 2), 0.08, tail=0.12),
        highpass=180,
        peak=0.72,
    )


def sfx_denied() -> np.ndarray:
    n = int(0.2 * SFX_RATE)
    frequencies = sfx_glide(n, 233.0, 218.0)
    buzz = (sfx_saw(frequencies * 0.997) + sfx_saw(frequencies * 1.003)) / 2.0
    buzz = sfx_lowpass(buzz, 1300) * sfx_decay(n, 4.5)
    knock_frames = int(0.1 * SFX_RATE)
    knock = sfx_body(sfx_glide(knock_frames, 150.0, 132.0), (1.0, 0.4)) * sfx_decay(
        knock_frames, 8.0
    )
    canvas = SfxCanvas(0.24).add(buzz, 0.0, 0.8).add(knock, 0.1, 0.55)
    return sfx_finish(
        sfx_space(sfx_crush(canvas.output(), 10, 3), 0.1, tail=0.15),
        highpass=90,
        peak=0.78,
    )


def sfx_alert() -> np.ndarray:
    canvas = SfxCanvas(0.34)
    first = int(0.07 * SFX_RATE)
    second = int(0.09 * SFX_RATE)
    canvas.add(
        sfx_softsquare(np.full(first, 587.33), 2.2) * sfx_decay(first, 7.0),
        0.0,
        0.6,
    )
    canvas.add(
        sfx_softsquare(np.full(second, 415.3), 2.2) * sfx_decay(second, 6.0),
        0.085,
        0.62,
    )
    knock_frames = int(0.09 * SFX_RATE)
    canvas.add(
        sfx_body(sfx_glide(knock_frames, 160.0, 138.0), (1.0, 0.4))
        * sfx_decay(knock_frames, 7.0),
        0.085,
        0.5,
    )
    return sfx_finish(
        sfx_space(sfx_crush(canvas.output(), 11, 2), 0.1, tail=0.15),
        highpass=90,
        peak=0.8,
    )


def sfx_deposit() -> np.ndarray:
    canvas = SfxCanvas(0.42)
    thump_frames = int(0.09 * SFX_RATE)
    canvas.add(
        sfx_body(sfx_glide(thump_frames, 150.0, 126.0), (1.0, 0.5, 0.25))
        * sfx_decay(thump_frames, 7.0),
        0.0,
        0.6,
    )
    chunk_frames = int(0.05 * SFX_RATE)
    chunk = sfx_comb(
        sfx_bandpass(sfx_noise(chunk_frames, 202), 400, 2600),
        int(SFX_RATE / 560),
        0.98,
        damping=0.4,
    )
    canvas.add(chunk * sfx_decay(chunk_frames, 9.0), 0.004, 0.5)
    rng = np.random.default_rng(203)
    for index, frequency in enumerate((1420.0, 1140.0, 930.0, 745.0)):
        frequency *= 1.0 + rng.uniform(-0.02, 0.02)
        canvas.add(
            sfx_ping(frequency, 0.09, 204 + index, sharp=8.0),
            0.085 + 0.068 * index,
            0.42 - 0.07 * index,
        )
    return sfx_finish(
        sfx_space(sfx_crush(canvas.output(), 11, 2), 0.12, tail=0.2),
        highpass=90,
        peak=0.8,
    )


def sfx_train_done() -> np.ndarray:
    canvas = SfxCanvas(0.7)
    vent_frames = int(0.13 * SFX_RATE)
    vent = sfx_bandpass(sfx_noise(vent_frames, 301), 900, 3200) * sfx_decay(
        vent_frames, 6.0
    )
    canvas.add(sfx_attack(vent, 6.0), 0.0, 0.28)
    bell_frames = int(0.6 * SFX_RATE)
    canvas.add(
        sfx_bell(bell_frames, 523.25, ratio=3.01, index=1.6, sharp=5.0),
        0.05,
        0.7,
    )
    canvas.add(
        sfx_bell(bell_frames, 349.23, ratio=2.0, index=0.9, sharp=4.5),
        0.05,
        0.4,
    )
    return sfx_finish(
        sfx_space(sfx_crush(canvas.output(), 11, 2), 0.18, tail=0.35),
        highpass=110,
        peak=0.8,
    )


def sfx_laser() -> np.ndarray:
    return sfx_finish(
        sfx_crush(sfx_zap(401, 1450.0, 320.0, 0.09), 10, 2),
        highpass=130,
        peak=0.82,
    )


def sfx_laser2() -> np.ndarray:
    return sfx_finish(
        sfx_crush(sfx_zap(403, 1150.0, 260.0, 0.1), 10, 2),
        highpass=120,
        peak=0.82,
    )


def sfx_sentinel() -> np.ndarray:
    return sfx_finish(
        sfx_crush(
            sfx_zap(
                501,
                900.0,
                210.0,
                0.13,
                thump=0.6,
                sync_gain=0.4,
                sharp=5.5,
            ),
            10,
            2,
        ),
        highpass=100,
        peak=0.85,
    )


def sfx_talon() -> np.ndarray:
    return sfx_finish(
        sfx_crush(sfx_zap(6311, 1250.0, 420.0, 0.1, thump=0.7, sync_gain=0.45), 10, 2),
        highpass=140,
        peak=0.84,
    )


def sfx_buzzard() -> np.ndarray:
    result = sfx_crush(
        sfx_zap(
            701,
            750.0,
            140.0,
            0.22,
            thump=0.8,
            snap=0.4,
            sync_gain=0.45,
            sharp=4.8,
        ),
        10,
        2,
    )
    return sfx_finish(sfx_slapback(result, 0.09, 1, 0.25), highpass=70, peak=0.88)


def sfx_roc() -> np.ndarray:
    # Both guns fire on the same tick, so one report stands for the volley;
    # its echo comes off the ground below the gunship.
    shot = sfx_zap(
        8241, 760.0, 170.0, 0.13, thump=0.75, snap=0.4, sync_gain=0.45, sharp=5.2
    )
    echoed = sfx_slapback(sfx_crush(shot, 10, 2), 0.07, 1, 0.3)
    return sfx_finish(sfx_crush(echoed, 10, 2), highpass=85, peak=0.87)


def sfx_drone() -> np.ndarray:
    shot = sfx_zap(8411, 2350.0, 880.0, 0.05, thump=0.15, snap=0.2, sync_gain=0.3)
    return sfx_finish(sfx_crush(shot, 10, 2), highpass=200, peak=0.8)


def sfx_clamp(
    pitch: float, seconds: float, sharp: float = 9.0, index: float = 1.6
) -> np.ndarray:
    """A small FM clank, the sound of a clamp closing or letting go."""
    n = int(seconds * SFX_RATE)
    phase = 2 * np.pi * pitch * sfx_axis(n)
    return np.sin(phase + index * np.sin(phase * 2.96)) * sfx_decay(n, sharp)


def sfx_hiss(
    seconds: float, seed: int, low: float, high: float, sharp: float
) -> np.ndarray:
    n = int(seconds * SFX_RATE)
    return sfx_bandpass(sfx_noise(n, seed), low, high) * sfx_decay(n, sharp)


def sfx_drone_launch() -> np.ndarray:
    # The bay's clamps let go and vent once for the whole launch.
    canvas = SfxCanvas(0.4)
    canvas.add(sfx_clamp(440.0, 0.06, sharp=10.0), 0.0, 0.6)
    canvas.add(
        sfx_attack(sfx_hiss(0.32, 8311, 1400.0, 7000.0, sharp=4.0), 3.0), 0.01, 0.75
    )
    return sfx_finish(sfx_crush(canvas.output(), 10, 2), highpass=150, peak=0.8)


def sfx_drone_dock() -> np.ndarray:
    canvas = SfxCanvas(0.14)
    canvas.add(
        sfx_attack(sfx_hiss(0.07, 8312, 1800.0, 7000.0, sharp=6.0), 2.0), 0.0, 0.5
    )
    canvas.add(sfx_clamp(587.33, 0.07, sharp=10.0), 0.045, 0.75)
    return sfx_finish(sfx_crush(canvas.output(), 10, 2), highpass=160, peak=0.78)


def sfx_lancer() -> np.ndarray:
    result = sfx_crush(
        sfx_zap(
            504,
            1650.0,
            230.0,
            0.26,
            thump=0.75,
            snap=0.5,
            sync_gain=0.45,
            sharp=5.0,
        ),
        10,
        2,
    )
    return sfx_finish(sfx_slapback(result, 0.08, 2, 0.28), highpass=85, peak=0.87)


def sfx_scuttler() -> np.ndarray:
    canvas = SfxCanvas(0.32)
    for index, at in enumerate((0.0, 0.115)):
        stroke_frames = int(0.075 * SFX_RATE)
        stroke = sfx_noise(stroke_frames, 951 + index)
        stroke *= sfx_tone(sfx_glide(stroke_frames, 2400.0 - 300.0 * index, 950.0))
        stroke = sfx_bandpass(
            sfx_comb(stroke, int(SFX_RATE / 1650), 0.955, damping=0.3),
            700,
            6500,
        )
        canvas.add(
            sfx_attack(stroke * sfx_decay(stroke_frames, 6.5), 3.0),
            at,
            0.85 - 0.1 * index,
        )
        thump_frames = int(0.05 * SFX_RATE)
        canvas.add(
            sfx_drive(
                sfx_tone(sfx_glide(thump_frames, 320.0, 210.0))
                * sfx_decay(thump_frames, 8.0),
                2.2,
            ),
            at + 0.004,
            0.3,
        )
    canvas.add(sfx_ping(2650.0, 0.05, 955, sharp=10.0), 0.23, 0.28)
    return sfx_finish(sfx_crush(canvas.output(), 9, 2), highpass=200, peak=0.83)


def sfx_flakhound() -> np.ndarray:
    return sfx_flak_pair(6811, 0.115, 130.0, 50.0, 0.2, 0.6, turret=False)


def sfx_flak_turret() -> np.ndarray:
    return sfx_flak_pair(6911, 0.135, 120.0, 46.0, 0.22, 0.65, turret=True)


def sfx_bombard() -> np.ndarray:
    # A mortar: the crack, then the hollow ring of its tube and a puff of
    # gas over a kick that barely falls in pitch.
    canvas = SfxCanvas(0.7)
    canvas.add(sfx_crack(810), 0.0, 0.7)
    canvas.add(sfx_tube(8121), 0.002, 0.9)
    canvas.add(sfx_pound(8122, 110.0, 80.0, 0.25, bloom=0.4, sharp=5.0), 0.004, 0.6)
    canvas.add(sfx_puff(8123), 0.003, 0.5)
    result = sfx_crush(sfx_drive(canvas.output(), 2.0), 10, 2)
    return sfx_finish(sfx_slapback(result, 0.11, 2, 0.24), highpass=55, peak=0.9)


def sfx_bastion() -> np.ndarray:
    # A fortress gun rather than a mortar: the crack, a kick that barely
    # falls in pitch, and a blast under it.
    canvas = SfxCanvas(0.95)
    canvas.add(sfx_crack(820), 0.0, 1.0)
    canvas.add(sfx_pound(8231, 105.0, 72.0, 0.5, bloom=1.0, sharp=4.2), 0.005, 0.9)
    canvas.add(sfx_burst(0.5, 8232, 120, 3500, 14), 0.003, 0.45)
    result = sfx_crush(sfx_drive(canvas.output(), 2.1), 10, 2)
    return sfx_finish(sfx_slapback(result, 0.13, 2, 0.26), highpass=50, peak=0.9)


def sfx_artillery_launch() -> np.ndarray:
    canvas = SfxCanvas(0.55)
    report = sfx_pound(801, 95.0, 48.0, 0.4, bloom=0.4, sharp=4.5)
    canvas.add(sfx_attack(report, 20.0), 0.0, 0.95)
    return sfx_finish(sfx_crush(canvas.output(), 10, 2), highpass=60, peak=0.86)


def sfx_artillery_boom() -> np.ndarray:
    # A landing is an explosion, not a gun: no muzzle crack, a burst that
    # opens out, and a smaller cousin of the rocket impact's electric edge.
    seconds = 1.0
    t = strategic_axis(seconds)
    fracture = rocket_noise(seconds, 1321, 250, 7100) * np.exp(-t * 48)
    pressure = rocket_noise(seconds, 1322, 40, 900) * np.exp(-t * 7.5)
    bloom = (1 - np.exp(-t * 70)) * np.exp(-t * 7)
    air = rocket_noise(seconds, 1323, 280, 3400)
    phase = settling_phase(t, 98.0, 220, 0.05)
    discharge = np.sin(phase + 2.2 * np.exp(-t * 6) * np.sin(phase * 2.0)) + 0.3 * np.sin(
        phase * 3.0
    )
    textured = air * (0.62 + 0.38 * np.sin(phase * 1.5))
    low = settling_phase(t, 48, 40, 0.05)
    body = (np.sin(low) + 0.25 * np.sin(3 * low)) * np.exp(-t * 8)
    dry = (
        0.22 * fracture
        + 0.6 * pressure
        + 0.3 * body
        + (0.55 * textured + 0.35 * discharge) * bloom
    )
    return rocket_finish(ground_reflection(dry, 43, 0.12, 1800), peak=0.88)


def sfx_unit_death() -> np.ndarray:
    n = int(0.4 * SFX_RATE)
    frequencies = sfx_glide(n, 520.0, 138.0)
    vibrato = 1.0 + np.linspace(0.0, 0.055, n) * np.sin(2.0 * np.pi * 9.0 * sfx_axis(n))
    down = (
        sfx_softsquare(frequencies * vibrato * 0.9965, 2.2)
        + sfx_softsquare(frequencies * vibrato * 1.0035, 2.2)
    ) / 2.0
    down = sfx_crush(down * sfx_decay(n, 3.6), 9, 4)
    canvas = SfxCanvas(0.5).add(sfx_attack(down, 8.0), 0.0, 0.85)
    canvas.add(sfx_ping(620.0, 0.07, 901, sharp=9.0), 0.33, 0.22)
    canvas.add(sfx_ping(415.0, 0.08, 902, sharp=9.0), 0.4, 0.18)
    return sfx_finish(
        sfx_space(canvas.output(), 0.12, tail=0.18), highpass=95, peak=0.82
    )


def sfx_building_boom() -> np.ndarray:
    n = int(0.85 * SFX_RATE)
    frequencies = sfx_glide(n, 440.0, 72.0)
    vibrato = 1.0 + np.linspace(0.0, 0.07, n) * np.sin(2.0 * np.pi * 7.0 * sfx_axis(n))
    down = (
        sfx_softsquare(frequencies * vibrato * 0.996, 2.2)
        + sfx_softsquare(frequencies * vibrato * 1.004, 2.2)
    ) / 2.0
    down = sfx_crush(down * sfx_decay(n, 3.2), 9, 4)
    canvas = SfxCanvas(1.0).add(sfx_attack(down, 10.0), 0.0, 0.9)
    canvas.add(sfx_ping(300.0, 0.1, 921, sharp=8.0), 0.68, 0.24)
    canvas.add(sfx_ping(205.0, 0.12, 922, sharp=8.0), 0.8, 0.2)
    return sfx_finish(
        sfx_space(canvas.output(), 0.12, tail=0.25), highpass=60, peak=0.88
    )


def sfx_warden() -> np.ndarray:
    # The Warden's fork cannon: the sentinel zap grown up — lower,
    # longer, with more chest and the same dry report.
    return sfx_finish(
        sfx_crush(
            sfx_zap(731, 640.0, 150.0, 0.17, thump=0.8, sync_gain=0.45, sharp=4.8),
            10,
            2,
        ),
        highpass=85,
        peak=0.87,
    )


def sfx_breaker() -> np.ndarray:
    # A tank's cannon, a step below the Warden: a short crack, a dark blast
    # and a thump that barely falls. A long, low zap sweep sounds like a
    # record scratch here.
    canvas = SfxCanvas(0.7)
    canvas.add(sfx_crack(7481), 0.0, 0.55)
    canvas.add(sfx_burst(0.5, 7482, 100, 1600.0, 14.0), 0.002, 0.9)
    canvas.add(sfx_pound(7483, 115.0, 88.0, 0.25, bloom=0.6, sharp=5.0), 0.004, 1.0)
    result = sfx_crush(sfx_drive(canvas.output(), 2.6), 10, 2)
    return sfx_finish(sfx_slapback(result, 0.09, 1, 0.22), highpass=60, peak=0.88)


def piledriver_finish(samples: np.ndarray, peak: float = 0.87) -> np.ndarray:
    return sfx_finish(sfx_crush(sfx_drive(samples, 1.8), 11, 2), highpass=90, peak=peak)


def sfx_piledriver() -> np.ndarray:
    # The rail shot: a snap, then a deep falling FM sweep with an electric
    # buzz over a short thump.
    seconds = 0.4
    t = strategic_axis(seconds)
    phase = 2 * np.pi * np.cumsum(sfx_glide(len(t), 1400.0, 70.0)) / SFX_RATE
    fm = np.sin(phase + 3.0 * np.exp(-t * 9) * np.sin(phase * 1.41)) * np.exp(-t * 7)
    fm = sfx_ringmod(fm, 55.0, 0.3)
    canvas = SfxCanvas(0.55)
    canvas.add(sfx_snap(9611), 0.0, 0.9)
    canvas.add(fm, 0.0, 1.0)
    canvas.add(sfx_pound(9612, 110.0, 40.0, 0.28, bloom=0.2, knock=0.35), 0.004, 0.5)
    return sfx_finish(
        sfx_crush(sfx_drive(canvas.output(), 2.2), 10, 2), highpass=75, peak=0.88
    )


def sfx_piledriver_impact() -> np.ndarray:
    # A hard hit and a heavy thud, with an electric crackle as the round
    # strikes.
    canvas = SfxCanvas(0.45)
    canvas.add(sfx_burst(0.03, 9711, 300, 8000, 120), 0.0, 0.9)
    canvas.add(sfx_pound(9712, 140.0, 50.0, 0.22, bloom=0.4, knock=0.7), 0.002, 0.85)
    n = round(0.14 * SFX_RATE)
    gate = (np.sin(2 * np.pi * 40 * np.arange(n) / SFX_RATE) > 0).astype(float)
    crackle = sfx_bandpass(sfx_noise(n, 9713), 2000, 6000) * gate * sfx_decay(n, 4)
    canvas.add(crackle, 0.01, 0.45)
    return sfx_finish(
        sfx_crush(sfx_drive(canvas.output(), 2.0), 10, 2), highpass=80, peak=0.86
    )


def piledriver_motor(duration: float, frequency: float) -> np.ndarray:
    n = int(duration * SFX_RATE)
    phase = 2 * np.pi * max(93, frequency * 0.92) * sfx_axis(n)
    core = np.sin(phase * 2 + 0.9 * np.sin(phase * 3.1)) + 0.35 * np.sin(phase * 4)
    texture = sfx_bandpass(sfx_noise(n, 9402 + round(duration * 100)), 250, 1400)
    envelope = np.sin(np.pi * np.linspace(0, 1, n)) ** 0.6
    motor = sfx_lowpass(core * 0.55 + texture * 0.18, 1900) * envelope
    return piledriver_finish(motor, 0.48)


def piledriver_latch(pitch: float) -> np.ndarray:
    n = round(0.11 * SFX_RATE)
    phase = 2 * np.pi * pitch * sfx_axis(n)
    tone = np.sin(phase + 1.7 * np.sin(phase * 2.96))
    return piledriver_finish(tone * sfx_decay(n, 8), 0.56)


def sfx_piledriver_deploy() -> np.ndarray:
    canvas = SfxCanvas(0.82).add(piledriver_motor(0.65, 110), 0)
    canvas.add(piledriver_latch(220), 0.70, 0.75)
    return sfx_finish(canvas.output(), highpass=100, peak=0.58)


def sfx_piledriver_fold() -> np.ndarray:
    canvas = SfxCanvas(0.50).add(piledriver_motor(0.36, 98), 0.08)
    canvas.add(piledriver_latch(246.94), 0.38, 0.7)
    return sfx_finish(canvas.output(), highpass=100, peak=0.54)


def rocket_noise(seconds: float, seed: int, low: float, high: float) -> np.ndarray:
    """Periodic, smoothly band-limited turbulence without a loop splice."""
    count = round(seconds * SFX_RATE)
    rng = np.random.default_rng(seed)
    spectrum = np.fft.rfft(rng.standard_normal(count))
    frequency = np.fft.rfftfreq(count, 1 / SFX_RATE)
    shape = (1 - np.exp(-((frequency / low) ** 4))) / (1 + (frequency / high) ** 6)
    result = np.fft.irfft(spectrum * shape, n=count)
    return result / np.sqrt(np.mean(result**2))


def rocket_finish(
    samples: np.ndarray, peak: float = 0.89, loop: bool = False
) -> np.ndarray:
    samples = np.tanh(samples * 0.85)
    if not loop:
        samples = sig.sosfilt(
            sig.butter(2, 28, "highpass", fs=SFX_RATE, output="sos"), samples
        )
        fade = round(SFX_RATE * 0.025)
        samples[-fade:] *= np.linspace(1, 0, fade) ** 2
        samples[:88] *= np.linspace(0, 1, 88)
        window = np.sin(np.linspace(0, np.pi, len(samples))) ** 2
        samples -= samples.sum() / window.sum() * window
    else:
        samples -= samples.mean()
    return samples * peak / np.max(np.abs(samples))


def sfx_avalanche_launch() -> np.ndarray:
    seconds = 0.64
    t = np.arange(round(seconds * SFX_RATE)) / SFX_RATE
    pressure = rocket_noise(seconds, 91101, 45, 1100) * np.exp(-t * 13)
    crack = rocket_noise(seconds, 91102, 650, 6500) * np.exp(-t * 85)
    exhaust = rocket_noise(seconds, 91103, 100, 2300) * np.exp(-t * 9)
    phase = 2 * np.pi * (65 * t + 35 * 0.045 * (1 - np.exp(-t / 0.045)))
    body = (np.sin(phase) + 0.35 * np.sin(3 * phase)) * np.exp(-t * 17)
    field_phase = 2 * np.pi * (146.83 * t + 140 * 0.045 * (1 - np.exp(-t / 0.045)))
    field = np.sin(field_phase + 1.6 * np.exp(-t * 14) * np.sin(2.03 * field_phase))
    field *= np.exp(-t * 12)
    return rocket_finish(
        0.8 * pressure + 0.21 * crack + 0.35 * exhaust + 0.35 * body + 0.18 * field
    )


def sfx_avalanche_motor() -> np.ndarray:
    # Body and a low hiss with a flutter, swelling slowly so a long flight
    # does not sit on one note. Whole cycles over the loop keep it seamless.
    seconds = 4.0
    t = strategic_axis(seconds)
    body = rocket_noise(seconds, 9821, 220, 650)
    hiss = rocket_noise(seconds, 9822, 380, 2000)
    flutter = 1 + 0.06 * np.tanh(rocket_noise(seconds, 9823, 6, 30))
    swell = 1 + 0.18 * (
        0.6 * np.sin(2 * np.pi * 0.5 * t) + 0.4 * np.sin(2 * np.pi * 1.25 * t + 1.0)
    )
    samples = (0.15 * body + 0.6 * hiss) * flutter * swell
    samples = rocket_finish(samples, peak=0.5, loop=True)
    samples *= min(1.0, 0.13 / np.sqrt(np.mean(samples**2)))
    # Choose a quiet boundary without splicing the periodic waveform.
    boundary = np.argmin(np.abs(samples) + np.abs(np.roll(samples, 1)))
    return np.roll(samples, -boundary)


def sfx_rocket_impact() -> np.ndarray:
    seconds = 1.42
    t = np.arange(round(seconds * SFX_RATE)) / SFX_RATE
    fracture = rocket_noise(seconds, 91121, 250, 7100) * np.exp(-t * 42)
    pressure = rocket_noise(seconds, 91122, 35, 900) * np.exp(-t * 5.8)
    # A fast onset opens into diffuse energy instead of a narrow resonant knock.
    bloom = (1 - np.exp(-t * 70)) * np.exp(-t * 5.4)
    air = rocket_noise(seconds, 91221, 280, 3400)
    phase = 2 * np.pi * (73.42 * t + 240 * 0.065 * (1 - np.exp(-t / 0.065)))
    discharge = np.sin(phase + 2.6 * np.exp(-t * 5) * np.sin(phase * 2.0))
    discharge += 0.3 * np.sin(phase * 3.0)
    textured = air * (0.62 + 0.38 * np.sin(phase * 1.5))
    low_phase = 2 * np.pi * (43 * t + 40 * 0.055 * (1 - np.exp(-t / 0.055)))
    body = (np.sin(low_phase) + 0.25 * np.sin(3 * low_phase)) * np.exp(-t * 7)
    dry = 0.19 * fracture + 0.62 * pressure + 0.32 * body
    dry += (0.6 * textured + 0.58 * discharge) * bloom
    reflection = sig.sosfilt(sig.butter(2, 1800, fs=SFX_RATE, output="sos"), dry)
    delay = round(SFX_RATE * 0.043)
    dry[delay:] += reflection[:-delay] * 0.12
    return rocket_finish(dry)


def sfx_bomb_release() -> np.ndarray:
    # The bay latch lets go with a clunk and the load drops away through
    # rushing air; the landing shell owns the boom.
    clunk = SfxCanvas(0.1)
    clunk.add(sfx_pound(7601, 230.0, 150.0, 0.07, knock=0.6, drive_amount=2.5, sharp=6.0))
    click_frames = round(0.02 * SFX_RATE)
    clunk.add(
        sfx_bandpass(sfx_noise(click_frames, 7602), 600, 2500) * sfx_decay(click_frames, 6),
        0.0,
        0.5,
    )
    air_frames = round(0.4 * SFX_RATE)
    air = sfx_swept_lowres(sfx_noise(air_frames, 7603), 2600.0, 500.0, resonance=1.2)
    canvas = SfxCanvas(0.5)
    canvas.add(clunk.output(), 0.0, 1.0)
    canvas.add(sfx_attack(air * sfx_decay(air_frames, 2.5), 25.0), 0.03, 1.0)
    return sfx_finish(sfx_crush(canvas.output(), 10, 2), highpass=110, peak=0.8)


def sfx_demolition_boom() -> np.ndarray:
    # Half electric charge, half explosion: the rocket impact's blast with
    # an arc crackling through its start. Serves the Scuttle Charge and the
    # Sapper alike.
    seconds = 1.3
    canvas = SfxCanvas(seconds)
    canvas.add(sfx_explosion(7721, seconds, 0.85) / 1.6, 0.0, 1.0)
    n = round(0.25 * SFX_RATE)
    gate = (np.sin(2 * np.pi * 60 * np.arange(n) / SFX_RATE) > 0.2).astype(float)
    arc = sfx_bandpass(sfx_noise(n, 7722), 1800, 7000) * gate * sfx_decay(n, 3.5)
    canvas.add(sfx_crush(arc, 9, 2), 0.0, 0.35)
    canvas.add(sfx_crack(7723, noise_gain=1.0), 0.0, 0.6)
    return rocket_finish(sfx_pad(canvas.output(), seconds), peak=0.89)


def sfx_upgrade_done() -> np.ndarray:
    # The works comes back online a rung higher: the train_done vent
    # into a two-step RISING figure (D up to A — reserved rising motion,
    # never the falling tritone).
    canvas = SfxCanvas(0.8)
    vent_frames = int(0.11 * SFX_RATE)
    vent = sfx_bandpass(sfx_noise(vent_frames, 781), 700, 2800) * sfx_decay(
        vent_frames, 6.0
    )
    canvas.add(sfx_attack(vent, 6.0), 0.0, 0.3)
    bell_frames = int(0.5 * SFX_RATE)
    canvas.add(
        sfx_bell(bell_frames, 293.66, ratio=2.0, index=1.1, sharp=4.5), 0.04, 0.55
    )
    canvas.add(sfx_bell(bell_frames, 440.0, ratio=3.01, index=1.5, sharp=5.0), 0.2, 0.7)
    return sfx_finish(
        sfx_space(sfx_crush(canvas.output(), 11, 2), 0.16, tail=0.3),
        highpass=105,
        peak=0.8,
    )


def sfx_intercept_zap() -> np.ndarray:
    # A point defender's shot: a thin, fast Lancer-family needle with no
    # thump, so it reads as a light beam beside the Lancer's discharge.
    result = sfx_crush(
        sfx_zap(
            1102,
            2800.0,
            900.0,
            0.085,
            thump=0.0,
            snap=0.45,
            sync_gain=0.6,
            sharp=6.5,
        ),
        11,
        2,
    )
    return sfx_finish(result, highpass=260, peak=0.78)


def strategic_axis(seconds: float) -> np.ndarray:
    return np.arange(round(seconds * SFX_RATE)) / SFX_RATE


def sfx_latch(
    seed: int, low: float = 1400, high: float = 4600, ms: float = 45
) -> np.ndarray:
    n = int(ms / 1000 * SFX_RATE)
    return sfx_attack(
        sfx_bandpass(sfx_noise(n, seed), low, high) * sfx_decay(n, 7.5), 2.0
    )


def sfx_voiced(
    samples: np.ndarray,
    highpass: float = 110.0,
    presence: float = 0.45,
    peak: float = 0.9,
) -> np.ndarray:
    """Missing-fundamental voicing for laptop speakers: thin the sub and lift
    the low mids that imply it."""
    lifted = sfx_highpass(samples, highpass, 2) + presence * sfx_bandpass(
        samples, 320.0, 2600.0, 2
    )
    return sfx_finish(lifted, highpass=highpass, peak=peak)


def sfx_spire_launch() -> np.ndarray:
    # The deck locks unlatch, then a pressure blast and a hiss roar that
    # recedes as the warhead climbs: the Avalanche's rocket language at
    # strategic scale, one gesture.
    canvas = SfxCanvas(2.2)
    canvas.add(sfx_latch(5101), 0.0, 0.55).add(sfx_latch(5102), 0.11, 0.6)
    t = strategic_axis(2.0)
    pressure = rocket_noise(2.0, 5103, 45, 1100) * np.exp(-t * 3.2)
    roar = (
        rocket_noise(2.0, 5104, 140, 3200)
        * (1 - np.exp(-t * 30))
        * np.exp(-t * 1.6)
    )
    body = (
        np.sin(2 * np.pi * (70 * t - 12 * t * t))
        + 0.4 * np.sin(2 * np.pi * 3 * (70 * t - 12 * t * t))
    ) * np.exp(-t * 3.0)
    canvas.add(
        0.75 * pressure
        + 0.5 * sfx_swept_lowres(roar, 3200, 700, resonance=1.2)
        + 0.3 * body,
        0.2,
    )
    return rocket_finish(canvas.output())


def sfx_incoming_warning() -> np.ndarray:
    # The under-attack alert's own falling tritone, doubled an octave down
    # and played twice, so a warhead reads as the same kind of bad news.
    canvas = SfxCanvas(0.8)
    for repeat in range(2):
        for frequency, offset, gain in (
            (587.33, 0.0, 0.45),
            (293.66, 0.0, 0.3),
            (415.30, 0.085, 0.45),
            (207.65, 0.085, 0.3),
        ):
            n = int((0.08 if offset else 0.07) * SFX_RATE)
            tone = sfx_softsquare(np.full(n, frequency), 2.2) * sfx_decay(n, 6.5)
            canvas.add(tone, 0.28 * repeat + offset, gain)
    return sfx_finish(
        sfx_space(sfx_crush(canvas.output(), 11, 2), 0.1, tail=0.15),
        highpass=110,
        peak=0.8,
    )


def sfx_warhead_detonation() -> np.ndarray:
    # The approved rocket impact at strategic scale: a diffuse pressure
    # bloom with an FM discharge falling very low, voiced so its depth
    # survives laptop speakers.
    seconds = 2.6
    t = strategic_axis(seconds)
    fracture = rocket_noise(seconds, 6301, 250, 7100) * np.exp(-t * 26)
    pressure = rocket_noise(seconds, 6302, 30, 900) * np.exp(-t * 2.4)
    bloom = (1 - np.exp(-t * 50)) * np.exp(-t * 2.6)
    air = rocket_noise(seconds, 6303, 260, 3000)
    phase = 2 * np.pi * (55.0 * t + 260 * 0.09 * (1 - np.exp(-t / 0.09)))
    discharge = np.sin(phase + 3.0 * np.exp(-t * 3) * np.sin(phase * 2.0)) + 0.3 * np.sin(
        phase * 3.0
    )
    low = 2 * np.pi * (36.7 * t + 45 * 0.07 * (1 - np.exp(-t / 0.07)))
    body = (np.sin(low) + 0.3 * np.sin(3 * low)) * np.exp(-t * 3.5)
    dry = (
        0.22 * fracture
        + 0.65 * pressure
        + 0.35 * body
        + (0.55 * air * (0.6 + 0.4 * np.sin(phase * 1.5)) + 0.55 * discharge) * bloom
    )
    return sfx_voiced(rocket_finish(dry), presence=0.3)


def sfx_interceptor_launch() -> np.ndarray:
    # A Lattice pod fires: a tight pressure pop into a hiss that rises as
    # the round leaves.
    seconds = 0.5
    t = strategic_axis(seconds)
    pop = rocket_noise(seconds, 7101, 300, 4000) * np.exp(-t * 45)
    hiss = (
        rocket_noise(seconds, 7102, 1200, 8000)
        * (1 - np.exp(-t * 60))
        * np.exp(-t * 6)
    )
    hiss = sfx_swept_lowres(hiss, 1500, 7000, resonance=1.5)
    return rocket_finish(0.6 * pop + 0.6 * hiss, peak=0.82)


def sfx_airburst(
    seconds: float,
    seed: int,
    *,
    discharge_hz: float,
    index: float = 3.0,
    fracture: float = 0.3,
    pressure: float = 0.55,
    body_hz: float = 0.0,
    body: float = 0.0,
    air: float = 0.5,
    discharge: float = 0.55,
    speed: float = 2.6,
    air_band: tuple[float, float] = (300.0, 3600.0),
) -> np.ndarray:
    """The warhead detonation's pressure bloom and FM discharge, shortened
    by `speed` and raised for a burst in the sky."""
    t = strategic_axis(seconds)
    frac = rocket_noise(seconds, seed, 250, 7100) * np.exp(-t * 26 * speed)
    press = rocket_noise(seconds, seed + 1, 60, 1100) * np.exp(-t * 2.4 * speed)
    bloom = (1 - np.exp(-t * 60)) * np.exp(-t * 2.6 * speed)
    hiss = rocket_noise(seconds, seed + 2, *air_band)
    sweep = discharge_hz * 4.7
    phase = 2 * np.pi * (discharge_hz * t + sweep * 0.05 * (1 - np.exp(-t / 0.05)))
    fm = np.sin(phase + index * np.exp(-t * 3 * speed) * np.sin(phase * 2.0)) + 0.3 * np.sin(
        phase * 3.0
    )
    dry = (
        fracture * frac
        + pressure * press
        + (air * hiss * (0.6 + 0.4 * np.sin(phase * 1.5)) + discharge * fm) * bloom
    )
    if body:
        low = 2 * np.pi * (body_hz * t + body_hz * 1.2 * 0.04 * (1 - np.exp(-t / 0.04)))
        dry = dry + body * (np.sin(low) + 0.3 * np.sin(3 * low)) * np.exp(-t * 3.5 * speed)
    return rocket_finish(dry)


def sfx_interception() -> np.ndarray:
    # A Lattice interceptor meets its warhead in the sky: the detonation's
    # small sibling, its bloom and discharge an octave and a fifth up and a
    # third as long.
    return sfx_voiced(
        sfx_airburst(1.0, 10101, discharge_hz=146.83, body_hz=73.42, body=0.25),
        presence=0.3,
    )


STRATEGIC_LOOP_SECONDS = 4.0


def strategic_loop(samples: np.ndarray, rms: float = 0.145) -> np.ndarray:
    """A four-second motor loop at the Avalanche motor's compact level,
    rolled to a quiet boundary without splicing the periodic waveform."""
    samples = rocket_finish(samples, peak=0.5, loop=True)
    samples *= min(1.0, rms / np.sqrt(np.mean(samples**2)))
    boundary = np.argmin(np.abs(samples) + np.abs(np.roll(samples, 1)))
    return np.roll(samples, -boundary)


def strategic_flutter(seed: int, depth: float, low: float, high: float) -> np.ndarray:
    return 1 + depth * np.tanh(
        rocket_noise(STRATEGIC_LOOP_SECONDS, seed, low, high)
    )


def sfx_warhead_flight() -> np.ndarray:
    # A warhead under power: the Spire launch's roar band carried on, high-
    # passed for the loop, with a slow combustion throb.
    t = strategic_axis(STRATEGIC_LOOP_SECONDS)
    roar = rocket_noise(STRATEGIC_LOOP_SECONDS, 11201, 200, 3200)
    under = rocket_noise(STRATEGIC_LOOP_SECONDS, 11202, 190, 700)
    throb = 1 + 0.08 * np.sin(2 * np.pi * 6.0 * t)
    return strategic_loop(
        (0.6 * roar + 0.35 * under) * throb * strategic_flutter(11203, 0.03, 8, 30)
    )


def sfx_interceptor_flight() -> np.ndarray:
    # An interceptor climbing to its warhead: a thin, bright needle hiss.
    hiss = rocket_noise(STRATEGIC_LOOP_SECONDS, 12201, 2000, 9000)
    edge = rocket_noise(STRATEGIC_LOOP_SECONDS, 12202, 4500, 10000)
    return strategic_loop(
        (0.8 * hiss + 0.2 * edge) * strategic_flutter(12203, 0.06, 25, 70), rms=0.11
    )


# A machine's working loops repeat every four seconds: every tone, wobble
# and tooth train closes a whole number of cycles in that span, and every
# filter runs to its periodic steady state, so the loop has no seam.
MACHINE_LOOP_SECONDS = 4.0
MACHINE_LOOP_FRAMES = int(MACHINE_LOOP_SECONDS * SFX_RATE)


def machine_hz(frequency: float) -> float:
    """The nearest frequency with a whole number of cycles in the loop."""
    return round(frequency * MACHINE_LOOP_SECONDS) / MACHINE_LOOP_SECONDS


def machine_steady(filter_fn, samples: np.ndarray, *args) -> np.ndarray:
    n = MACHINE_LOOP_FRAMES
    return filter_fn(np.tile(samples, 3), *args)[n : 2 * n]


def machine_wobble(rate: float, depth: float) -> np.ndarray:
    t = sfx_axis(MACHINE_LOOP_FRAMES)
    return 1 + depth * np.sin(2 * np.pi * machine_hz(rate) * t)


def machine_teeth(rate: float, sharp: float, seed: int) -> np.ndarray:
    """A train of tooth strikes of uneven weight."""
    t = sfx_axis(MACHINE_LOOP_FRAMES)
    count = int(round(rate * MACHINE_LOOP_SECONDS))
    phase = (t * rate) % 1.0
    index = np.floor(t * rate).astype(int) % count
    weight = np.random.default_rng(seed).uniform(1 - 0.4, 1.0, count)
    return weight[index] * (1 - np.exp(-phase * 60.0)) * np.exp(-sharp * phase)


def machine_unit(samples: np.ndarray) -> np.ndarray:
    return samples / np.sqrt(np.mean(samples**2))


def machine_loop(
    samples: np.ndarray, highpass: float, bits: int = 10, hold: int = 2
) -> np.ndarray:
    """A loop at the motors' compact level, rolled to a quiet boundary."""
    samples = machine_unit(machine_steady(sfx_highpass, samples, highpass, 2))
    # Rounds off rare peaks so every loop reaches the same compact level.
    samples = 3.2 * np.tanh(samples / 3.2)
    samples = samples / np.max(np.abs(samples)) * 0.9
    samples = sfx_crush(samples, bits, hold)
    samples = machine_steady(sfx_highpass, samples, highpass * 0.8, 1)
    samples = samples - samples.mean()
    samples = samples * 0.5 / np.max(np.abs(samples))
    samples *= min(1.0, 0.145 / np.sqrt(np.mean(samples**2)))
    boundary = np.argmin(np.abs(samples) + np.abs(np.roll(samples, 1)))
    return np.roll(samples, -boundary)


def sfx_maw_cut() -> np.ndarray:
    # The Maw's drum while it cuts: a ring-modulated saw drone on D that
    # sags slightly under load, roughened by teeth far too fast to hear
    # apart.
    n = MACHINE_LOOP_FRAMES
    voice = 0.7 * sfx_saw(np.full(n, 293.75) * machine_wobble(0.5, 0.012))
    voice = voice + 0.3 * sfx_saw(np.full(n, machine_hz(588.5)))
    voice = sfx_ringmod(voice, 587.25, 0.5)
    voice = machine_steady(sfx_lowpass, voice, 3400.0)
    grit = rocket_noise(MACHINE_LOOP_SECONDS, 85201, 900, 4200)
    grit = grit * machine_teeth(22.0, 5.0, 85202)
    return machine_loop(machine_unit(voice) + 0.3 * machine_unit(grit), highpass=190)


SFX_BUILDERS = {
    "piledriver_fold": sfx_piledriver_fold,
    "piledriver_deploy": sfx_piledriver_deploy,
    "piledriver_impact": sfx_piledriver_impact,
    "attack_piledriver": sfx_piledriver,
    "click": sfx_click,
    "ack": sfx_ack,
    "denied": sfx_denied,
    "alert": sfx_alert,
    "deposit": sfx_deposit,
    "train_done": sfx_train_done,
    "laser": sfx_laser,
    "laser2": sfx_laser2,
    "attack_sentinel": sfx_sentinel,
    "attack_talon": sfx_talon,
    "attack_buzzard": sfx_buzzard,
    "attack_roc": sfx_roc,
    "attack_drone": sfx_drone,
    "drone_launch": sfx_drone_launch,
    "drone_dock": sfx_drone_dock,
    "attack_lancer": sfx_lancer,
    "attack_scuttler": sfx_scuttler,
    "attack_flakhound": sfx_flakhound,
    "attack_flak_turret": sfx_flak_turret,
    "attack_warden": sfx_warden,
    "attack_breaker": sfx_breaker,
    "avalanche_launch": sfx_avalanche_launch,
    "avalanche_motor": sfx_avalanche_motor,
    "rocket_impact": sfx_rocket_impact,
    "bomb_release": sfx_bomb_release,
    "demolition_boom": sfx_demolition_boom,
    "upgrade_done": sfx_upgrade_done,
    "intercept_zap": sfx_intercept_zap,
    "spire_launch": sfx_spire_launch,
    "incoming_warning": sfx_incoming_warning,
    "warhead_detonation": sfx_warhead_detonation,
    "interceptor_launch": sfx_interceptor_launch,
    "interception": sfx_interception,
    "warhead_flight": sfx_warhead_flight,
    "interceptor_flight": sfx_interceptor_flight,
    "maw_cut": sfx_maw_cut,
    "attack_bombard": sfx_bombard,
    "attack_bastion": sfx_bastion,
    "artillery_launch": sfx_artillery_launch,
    "artillery_boom": sfx_artillery_boom,
    "unit_death": sfx_unit_death,
    "building_boom": sfx_building_boom,
}


def sfx_wav(samples: np.ndarray, *, rounded: bool = False) -> bytes:
    data = io.BytesIO()
    with wave.open(data, "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(SFX_RATE)
        pcm = np.clip(samples, -1.0, 1.0)
        pcm = pcm * 32767.0
        if rounded:
            pcm = np.rint(pcm)
        output.writeframes(pcm.astype("<i2").tobytes())
    return data.getvalue()


def generate_sfx_bank() -> None:
    if set(SFX_BUILDERS) != set(SFX_META_BY_NAME):
        raise ValueError("SFX builders and metadata differ")
    for name, _, _, _, _ in SFX_METADATA:
        data = sfx_wav(SFX_BUILDERS[name](), rounded=name in SFX_ROUNDED_PCM)
        metrics = validate_sfx_wav(name, data)
        digest = hashlib.sha256(data).hexdigest()
        expected = EXPECTED_SFX_SHA256[f"{name}.wav"]
        if digest != expected:
            raise ValueError(
                f"{name}: approved bytes changed ({digest}, expected {expected})"
            )
        GENERATED[f"{name}.wav"] = data
        print(
            f"  {name}.wav ({metrics['duration']:.2f}s, "
            f"peak {metrics['peak']:.2f}, >=180Hz {metrics['audible']:.0%})"
        )


def k_weighting(rate: int) -> list[tuple[np.ndarray, np.ndarray]]:
    """The two BS.1770 pre-filters, a high shelf and a high pass, at `rate`."""
    filters = []
    for shelf, gain_db, q, hz in ((True, 4.0, 1 / math.sqrt(2), 1500.0), (False, 0.0, 0.5, 38.0)):
        a = 10 ** (gain_db / 40)
        w0 = 2 * math.pi * hz / rate
        alpha = math.sin(w0) / (2 * q)
        cos = math.cos(w0)
        if shelf:
            root = 2 * math.sqrt(a) * alpha
            b = [
                a * ((a + 1) + (a - 1) * cos + root),
                -2 * a * ((a - 1) + (a + 1) * cos),
                a * ((a + 1) + (a - 1) * cos - root),
            ]
            den = [(a + 1) - (a - 1) * cos + root, 2 * ((a - 1) - (a + 1) * cos), (a + 1) - (a - 1) * cos - root]
        else:
            b = [(1 + cos) / 2, -(1 + cos), (1 + cos) / 2]
            den = [1 + alpha, -2 * cos, 1 - alpha]
        filters.append((np.array(b) / den[0], np.array(den) / den[0]))
    return filters


def momentary_peak_lufs(wav: bytes) -> float:
    """The loudest 400 ms of a mono clip played in both speakers, in LUFS:
    BS.1770 momentary loudness, stepped every 100 ms, with silence on both
    sides so a short clip is measured whole."""
    with wave.open(io.BytesIO(wav), "rb") as f:
        rate = f.getframerate()
        samples = np.frombuffer(f.readframes(f.getnframes()), dtype="<i2") / 32768.0
    window, hop = int(0.4 * rate), int(0.1 * rate)
    padded = np.concatenate([np.zeros(window), samples, np.zeros(window)])
    for b, a in k_weighting(rate):
        padded = sig.lfilter(b, a, padded)
    energy = np.concatenate([[0.0], np.cumsum(padded**2)])
    starts = np.arange(0, len(padded) - window + 1, hop)
    loudest = float(np.max(energy[starts + window] - energy[starts])) / window
    return -0.691 + 10 * math.log10(2 * max(loudest, 1e-20))


def clip_bytes(name: str) -> bytes:
    return GENERATED.get(f"{name}.wav") or (OUT / f"{name}.wav").read_bytes()


def mixer_volume(name: str, level: float) -> float:
    return round(10 ** ((level - momentary_peak_lufs(clip_bytes(name))) / 20), 4)


def manifest_bytes() -> bytes:
    sounds = [
        {
            "name": name,
            "filename": f"{name}.wav",
            "category": category,
            "mixer_volume": mixer_volume(name, level),
            "min_gap": min_gap,
            "paired_animation": animation,
            **({"looped": True} if name in SFX_LOOP_NAMES else {}),
        }
        for name, category, level, min_gap, animation in SFX_METADATA
    ]
    document = {
        "format": 1,
        "sample_rate": SFX_RATE,
        "sounds": sounds,
    }
    return (json.dumps(document, indent=2, sort_keys=True) + "\n").encode()


def emit(output: Path, check: bool) -> None:
    expected = set(GENERATED)
    actual = {path.name for path in output.glob("*.wav")}
    manifest = manifest_bytes()
    if check:
        if actual != expected:
            missing = sorted(expected - actual)
            extra = sorted(actual - expected)
            raise SystemExit(f"sound bank differs: missing={missing}, extra={extra}")
        changed = [
            name
            for name, data in GENERATED.items()
            if (output / name).read_bytes() != data
        ]
        if changed:
            raise SystemExit(f"sound bank is stale: {', '.join(sorted(changed))}")
        manifest_path = output / "manifest.json"
        if not manifest_path.exists() or manifest_path.read_bytes() != manifest:
            raise SystemExit("sound manifest is stale")
        print(f"checked {len(GENERATED)} deterministic WAVs and manifest")
        return
    output.mkdir(parents=True, exist_ok=True)
    for name, data in GENERATED.items():
        (output / name).write_bytes(data)
    (output / "manifest.json").write_bytes(manifest)
    print(f"wrote {len(GENERATED)} deterministic WAVs and manifest")


def generate(output: Path, check: bool = False) -> None:
    """Generates or checks the complete sound bank at `output`."""
    GENERATED.clear()
    print(f"{'checking' if check else 'writing'} {output}")
    generate_sfx_bank()
    emit(output, check)
    print("done")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    destination = parser.add_mutually_exclusive_group()
    destination.add_argument(
        "--check",
        action="store_true",
        help="verify the checked-in WAV bank without rewriting it",
    )
    destination.add_argument(
        "--out",
        type=Path,
        metavar="DIRECTORY",
        help="write the complete sound bank to DIRECTORY instead of assets/sounds",
    )
    args = parser.parse_args()
    generate(args.out if args.out is not None else OUT, check=args.check)


if __name__ == "__main__":
    main()
