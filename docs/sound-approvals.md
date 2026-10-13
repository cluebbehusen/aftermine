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
