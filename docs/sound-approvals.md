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
