# Sound approvals

## Avalanche rocket sequence

Approved by Connor after listening in the native game on 2026-09-12. The
launcher uses a compact pressure report, a quiet propulsion hiss sustained
through powered flight, and a separate synthetic energy impact. Reload is
silent.

| Production clip        | Duration | Mixer gain | SHA-256                                                            |
| ---------------------- | -------- | ---------- | ------------------------------------------------------------------ |
| `avalanche_launch.wav` | 0.64 s   | 0.50       | `ab8d9a4a2ba9955108ba3e43aaa80b80e86ef8dc947450b53ca8db74aa2b8a85` |
| `avalanche_motor.wav`  | 4 s loop | 0.35       | `aa1944a1dba7a0746973873bcffdbf03bf2c09355eef8f1a046028c83fe8a4bd` |
| `rocket_impact.wav`    | 1.42 s   | 0.50       | `037944ae136afdb1c69d34f2f72c6642d1a775663f7ab26fa1c75358af566d0e` |

The motor buffer loops independently for each visible missile, beginning after
launcher ejection and stopping at impact. Pausing stops the motors; resuming
reconstructs them from the presented state. Playback speed affects flight
lifetime, while the texture keeps its pitch. The loop file duration does not set
the projectile's audible lifetime.

`tools/gen_sounds.py` reproduces these exact bytes with pinned synthesis
libraries and fixed seeds. These three clips round when quantizing to PCM; the
earlier bank retains its original truncation to preserve its bytes. All other
effect approvals remain frozen in `EXPECTED_SFX_SHA256`.

## Point-defense interception

Approved by Connor by ear on 2026-10-12 from an audition of six zaps and six
mid-air bursts. A shot-down shell plays one thin, fast needle from the Mantle's
emitter, a lighter relative of the Lancer's discharge with no thump. The shell
itself dissolves silently; the burst candidates were dropped because the art
shows it as a puff of smoke.

| Production clip     | Duration | Mixer gain | SHA-256                                                            |
| ------------------- | -------- | ---------- | ------------------------------------------------------------------ |
| `intercept_zap.wav` | 0.14 s   | 0.24       | `4bedf27c34fe11970a0109a9ff81a627eb2c67569eca1b35cc2f623e6269b18a` |

The zap plays at the emitter only where the viewer sees the defender, once for
each shell it stops.

## Spire and Lattice

Approved by Connor by ear on 2026-10-13 from an audition of six options per
voice. The Spire's launch unlatches its deck locks into a pressure blast and a
receding roar, the Avalanche's rocket language at strategic scale. A hostile
warhead's first sighting plays the under-attack alert's falling tritone, doubled
an octave down and repeated, and is protected like that alert. The detonation is
the rocket impact grown to strategic size. A Lattice pod's launch is a tight
pressure pop into a rising hiss. The interception, warhead flight and
interceptor flight followed in a second round (below).

| Production clip          | Duration | Mixer gain | SHA-256                                                            |
| ------------------------ | -------- | ---------- | ------------------------------------------------------------------ |
| `spire_launch.wav`       | 2.20 s   | 0.50       | `8738e26f6daec50f38ff9d381682a42a143521b09de4ac96b9e153cd7c18039c` |
| `incoming_warning.wav`   | 0.95 s   | 0.40       | `4abc0168ecd66353cbec9a9e2316964f2a696e769671c54370b0807b37cc934d` |
| `warhead_detonation.wav` | 2.60 s   | 0.65       | `061059717a4a7b43abf92c4150674d603f97a2f0603cb6a2332faa3a679c4040` |
| `interceptor_launch.wav` | 0.50 s   | 0.35       | `5d3c7e6a91c406c81762be260bb38b3169d3d584cc2ff82951c1df543b3c56d6` |

The launch, warning and detonation sit in a `strategic` category capped at 3 s:
a silo fires at most once a minute, so its cues may outlast a weapon report
without crowding the mix. All four clips round when quantizing to PCM, like the
Avalanche sequence.

### Second round

Approved by Connor by ear on 2026-10-13 from a second audition: six
interceptions, five warhead motors and five interceptor motors. The interception
is the detonation's small sibling in the sky, its loudness set between the
rocket impact and the artillery boom. The warhead's motor carries the launch's
roar band on as a throbbing loop; the interceptor's is a thin, bright needle
hiss. Both loops keep the Avalanche motor's four-second periodic contract and
compact level. The Spire assembly sound was dropped.

| Production clip          | Duration | Mixer gain | SHA-256                                                            |
| ------------------------ | -------- | ---------- | ------------------------------------------------------------------ |
| `interception.wav`       | 1.00 s   | 0.68       | `d2c2412d0567d65108ce1096880ef576fde7029706ee36fd65ec7e77cb350293` |
| `warhead_flight.wav`     | 4 s loop | 0.35       | `b85c23aae2abfb1fc247c7e7e6998dcef0d4b71575057333b604aa7f6334f9c2` |
| `interceptor_flight.wav` | 4 s loop | 0.30       | `077aab9f1523779c6ae787fef587eef67dc7dff11da3ca168eef4528f5d2f1c1` |

The interception sounds when the interceptor reaches the warhead in the sky, 0.4
s after the simulation stops it, on the same clock the burst draws on. The
warhead motor loops for every warhead the viewer may know of, as drawn: it fades
in over the launch's receding roar and out as the warhead stops burning for its
dive, and carries on through the flight that runs on after an interception until
the burst. The interceptor motor loops through the interceptor's visible climb
from its pod.

## Piledriver pressure discharge

Approved by Connor by ear on 2026-10-13. The Piledriver uses one dry pressure
report and a compact impact. Setup and folding retain very light mechanical
cues, attenuated 18 dB from their initial audition levels. Reload stays silent.

| Production clip         | Duration | Mixer gain  | SHA-256                                                            |
| ----------------------- | -------- | ----------- | ------------------------------------------------------------------ |
| `attack_piledriver.wav` | 0.39 s   | 0.55        | `f7a310f69b77e60c64a510245098e916294edb35df253147f515f58e5444740e` |
| `piledriver_impact.wav` | 0.3 s    | 0.26        | `2cde65203180fbbad6eb2ef0e1d118a374546af586c8ef3fb22412a2fa1a585c` |
| `piledriver_deploy.wav` | 0.82 s   | 0.023919583 | `4a4dbc55cb8d23a0cb745d0c99a470e6e87da34203ec359b93e0642b6297b6b3` |
| `piledriver_fold.wav`   | 0.5 s    | 0.022660657 | `72a6539c1588ef3cec75f449920522d207a65a2f72c48ed44a25702f7edcfd20` |

The report and impact retain their approved PCM bytes. Anchor clips retain their
approved PCM bytes and use the quieter manifest gains. Visible anchor direction
changes trigger setup and folding once; the impact follows the cosmetic round's
arrival on the simulation presentation clock. Seeking discards pending impacts.

## Roc and drones

Approved by Connor by ear on 2026-10-15 from an audition of six options per
voice, each laid over native footage at the ticks its event fires. Both Roc guns
fire on the same tick, so one report stands for the volley: a heavy zap with one
dark echo off the ground below. A drone's shot is a tiny high zap. The bay's
launch is a clamp letting go into a hiss; each dock is a short hiss and a latch.

| Production clip    | Duration | Mixer gain | SHA-256                                                            |
| ------------------ | -------- | ---------- | ------------------------------------------------------------------ |
| `attack_roc.wav`   | 0.31 s   | 0.30       | `3c1b0a6f48788eb4d27cd3ebb609f6149ed242b27ea66040b9663091bb36e497` |
| `attack_drone.wav` | 0.11 s   | 0.20       | `64b9d387a2f8d6dffa81ea048cc6289c543da03a41f759a459f69a5a0a7cfcdc` |
| `drone_launch.wav` | 0.40 s   | 0.22       | `5c95724f5dc530d351ffe485db2e76fca7098239f599ce6de8c6071bcf2eea94` |
| `drone_dock.wav`   | 0.14 s   | 0.22       | `fa9550ccb7ac33af39773ec3259038baee5cf866cffeacf674b334c68ec324f0` |

The bay's cues sound at its host only where the viewer sees it. The launch's
half-second repetition limit gives a bay's launch one cue for all its drones,
and drones that dock together share one.
