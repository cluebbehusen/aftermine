# Sound approvals

Clips approved by ear in the native game. `tools/gen_sounds.py` must reproduce
these bytes exactly, and its `EXPECTED_SFX_SHA256` table checks them along with
every earlier effect. Change an approved clip only when it is reopened for a new
audition.

| Clip                     | Approved   | Plays on                               | Gain        | SHA-256                                                            |
| ------------------------ | ---------- | -------------------------------------- | ----------- | ------------------------------------------------------------------ |
| `avalanche_launch.wav`   | 2026-09-12 | Avalanche rocket launch                | 0.50        | `ab8d9a4a2ba9955108ba3e43aaa80b80e86ef8dc947450b53ca8db74aa2b8a85` |
| `avalanche_motor.wav`    | 2026-09-12 | Avalanche rocket in flight (4 s loop)  | 0.35        | `aa1944a1dba7a0746973873bcffdbf03bf2c09355eef8f1a046028c83fe8a4bd` |
| `rocket_impact.wav`      | 2026-09-12 | Avalanche rocket impact                | 0.50        | `037944ae136afdb1c69d34f2f72c6642d1a775663f7ab26fa1c75358af566d0e` |
| `intercept_zap.wav`      | 2026-10-12 | Mantle shooting down a shell           | 0.24        | `4bedf27c34fe11970a0109a9ff81a627eb2c67569eca1b35cc2f623e6269b18a` |
| `spire_launch.wav`       | 2026-10-13 | Spire launch                           | 0.50        | `8738e26f6daec50f38ff9d381682a42a143521b09de4ac96b9e153cd7c18039c` |
| `incoming_warning.wav`   | 2026-10-13 | First sighting of a hostile warhead    | 0.40        | `4abc0168ecd66353cbec9a9e2316964f2a696e769671c54370b0807b37cc934d` |
| `warhead_detonation.wav` | 2026-10-13 | Warhead detonation                     | 0.65        | `061059717a4a7b43abf92c4150674d603f97a2f0603cb6a2332faa3a679c4040` |
| `interceptor_launch.wav` | 2026-10-13 | Lattice interceptor launch             | 0.35        | `5d3c7e6a91c406c81762be260bb38b3169d3d584cc2ff82951c1df543b3c56d6` |
| `interception.wav`       | 2026-10-13 | Interceptor meeting a warhead          | 0.68        | `d2c2412d0567d65108ce1096880ef576fde7029706ee36fd65ec7e77cb350293` |
| `warhead_flight.wav`     | 2026-10-13 | Warhead in flight (4 s loop)           | 0.35        | `b85c23aae2abfb1fc247c7e7e6998dcef0d4b71575057333b604aa7f6334f9c2` |
| `interceptor_flight.wav` | 2026-10-13 | Interceptor in flight (4 s loop)       | 0.30        | `077aab9f1523779c6ae787fef587eef67dc7dff11da3ca168eef4528f5d2f1c1` |
| `attack_piledriver.wav`  | 2026-10-13 | Piledriver shot                        | 0.55        | `f7a310f69b77e60c64a510245098e916294edb35df253147f515f58e5444740e` |
| `piledriver_impact.wav`  | 2026-10-13 | Piledriver round landing               | 0.26        | `2cde65203180fbbad6eb2ef0e1d118a374546af586c8ef3fb22412a2fa1a585c` |
| `piledriver_deploy.wav`  | 2026-10-13 | Piledriver planting                    | 0.023919583 | `4a4dbc55cb8d23a0cb745d0c99a470e6e87da34203ec359b93e0642b6297b6b3` |
| `piledriver_fold.wav`    | 2026-10-13 | Piledriver folding                     | 0.022660657 | `72a6539c1588ef3cec75f449920522d207a65a2f72c48ed44a25702f7edcfd20` |
| `attack_roc.wav`         | 2026-10-15 | Roc gun volley                         | 0.30        | `3c1b0a6f48788eb4d27cd3ebb609f6149ed242b27ea66040b9663091bb36e497` |
| `attack_drone.wav`       | 2026-10-15 | Drone shot                             | 0.20        | `64b9d387a2f8d6dffa81ea048cc6289c543da03a41f759a459f69a5a0a7cfcdc` |
| `drone_launch.wav`       | 2026-10-15 | Roc bay launching drones               | 0.22        | `5c95724f5dc530d351ffe485db2e76fca7098239f599ce6de8c6071bcf2eea94` |
| `drone_dock.wav`         | 2026-10-15 | Drone docking                          | 0.22        | `fa9550ccb7ac33af39773ec3259038baee5cf866cffeacf674b334c68ec324f0` |
| `maw_cut.wav`            | 2026-10-16 | Maw drum cutting a building (4 s loop) | 0.264       | `39815e65e830e5361bcfa9114e32aa625c75d9771ae93666b7da9d96d6c20265` |
| `maw_kiln.wav`           | 2026-10-16 | Maw kiln eating wreck (4 s loop)       | 0.211       | `53e12faa1a96a3c43fc5e71dadeaf19750a86d4534cef632b0687b6a1bf8c92b` |
