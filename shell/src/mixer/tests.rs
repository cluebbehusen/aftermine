use super::*;

#[test]
fn every_sound_kind_mixes_from_its_manifest_row() {
    for kind in SoundKind::ALL {
        let spec = mixer_spec(kind);
        assert!(spec.volume > 0.0 && spec.min_gap > 0.0, "{kind:?}");
    }
    let (laser, laser2) = (MIXER_SPECS["laser"], MIXER_SPECS["laser2"]);
    assert_eq!(laser.min_gap, laser2.min_gap);
    assert!(
        (20.0 * (laser.volume / laser2.volume).log10()).abs() < 1.0,
        "the alternating zaps sit at one level"
    );
}

#[test]
fn piledriver_anchor_cues_remain_quiet() {
    for kind in [SoundKind::PiledriverDeploy, SoundKind::PiledriverFold] {
        assert!(Mixer::base_volume(kind) < Mixer::base_volume(SoundKind::PiledriverFire) / 10.0);
        assert_eq!(spec(kind).weight, Weight::Detail);
        assert!(!spec(kind).explosion);
    }
}
