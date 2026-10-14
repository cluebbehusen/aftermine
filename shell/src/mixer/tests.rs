use super::*;

#[test]
fn every_authored_weapon_report_raises_combat_pressure() {
    for kind in [
        SoundKind::Alert,
        SoundKind::SentinelFire,
        SoundKind::ScuttlerFire,
        SoundKind::LancerFire,
        SoundKind::BombardFire,
        SoundKind::FlakhoundFire,
        SoundKind::BuzzardFire,
        SoundKind::TalonFire,
        SoundKind::BastionFire,
        SoundKind::FlakTurretFire,
        SoundKind::ArtilleryLaunch,
        SoundKind::WardenFire,
        SoundKind::BreakerFire,
        SoundKind::PiledriverFire,
        SoundKind::PiledriverImpact,
        SoundKind::AvalancheFire,
        SoundKind::BombRelease,
        SoundKind::DemolitionBoom,
        SoundKind::SpireLaunch,
        SoundKind::IncomingWarning,
        SoundKind::WarheadDetonation,
        SoundKind::InterceptorLaunch,
    ] {
        assert!(spec(kind).combat, "{kind:?} must pressure the score");
    }
}

#[test]
fn every_sound_kind_mixes_from_its_manifest_row() {
    for kind in SoundKind::ALL {
        let spec = mixer_spec(kind);
        assert!(spec.volume > 0.0 && spec.min_gap > 0.0, "{kind:?}");
    }
    assert_eq!(MIXER_SPECS.get("laser2"), MIXER_SPECS.get("laser"));
}

#[test]
fn piledriver_anchor_cues_remain_quiet_and_do_not_raise_combat_music() {
    let quiet = 10_f32.powf(-18.0 / 20.0);
    for (kind, original) in [
        (SoundKind::PiledriverDeploy, 0.19),
        (SoundKind::PiledriverFold, 0.18),
    ] {
        assert!((Mixer::base_volume(kind) - original * quiet).abs() < 1e-6);
        assert_eq!(spec(kind).weight, Weight::Detail);
        assert!(!spec(kind).combat);
        assert!(!spec(kind).explosion);
    }
    assert_eq!(Mixer::base_volume(SoundKind::PiledriverFire), 0.55);
    assert_eq!(Mixer::base_volume(SoundKind::PiledriverImpact), 0.26);
}
