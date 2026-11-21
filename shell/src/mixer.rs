//! The sound mixer and the one table of what the shell decides per sound.

use crate::assets;
pub(crate) use crate::audio_engine::Bus;
use crate::audio_engine::{PlaySoundParams, Sound, play_sound};
use crate::audio_mix::FrameSound;
use crate::game::SoundKind;
use crate::numeric;
use macroquad::prelude::get_time;

/// How a clip competes for one frame's positional voices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Weight {
    /// Small arms that thin out first when the camera pulls back.
    Detail,
    /// Ordinary reports and cues.
    Standard,
    /// Blasts and heavy guns that keep a voice at any zoom.
    Heavy,
    /// Never dropped from the mix.
    Protected,
    /// The player's own news, such as a unit ready: heard from where it
    /// happened, but never dropped and never so far off that it is missed.
    Notice,
}

/// Everything the shell decides about one sound kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SoundSpec {
    /// The generated clip, which also names its row in the sound manifest.
    /// The basic zap alternates with `laser2`, a clip the manifest gives the
    /// same level and gap.
    pub clip: &'static str,
    pub bus: Bus,
    pub weight: Weight,
    /// Whether it is a blast, heard from beyond sight with its own falloff.
    pub explosion: bool,
}

const fn row(clip: &'static str, bus: Bus, weight: Weight, explosion: bool) -> SoundSpec {
    SoundSpec {
        clip,
        bus,
        weight,
        explosion,
    }
}

/// The per-sound table: a new kind adds exactly one row here.
pub(crate) const fn spec(kind: SoundKind) -> SoundSpec {
    use Bus::{Effects, Ui};
    use Weight::{Detail, Heavy, Notice, Protected, Standard};
    match kind {
        SoundKind::Laser => row("laser", Effects, Detail, false),
        SoundKind::UnitDeath => row("unit_death", Effects, Standard, false),
        SoundKind::BuildingBoom => row("building_boom", Effects, Heavy, true),
        SoundKind::Deposit => row("deposit", Effects, Standard, false),
        SoundKind::TrainDone => row("train_done", Effects, Notice, false),
        SoundKind::Click => row("click", Ui, Standard, false),
        SoundKind::Denied => row("denied", Ui, Standard, false),
        SoundKind::Alert => row("alert", Effects, Protected, false),
        SoundKind::Artillery => row("artillery_boom", Effects, Heavy, true),
        SoundKind::ArtilleryLaunch => row("artillery_launch", Effects, Heavy, false),
        SoundKind::Ack => row("ack", Effects, Standard, false),
        SoundKind::SentinelFire => row("attack_sentinel", Effects, Detail, false),
        SoundKind::ScuttlerFire => row("attack_scuttler", Effects, Detail, false),
        SoundKind::LancerFire => row("attack_lancer", Effects, Standard, false),
        SoundKind::BombardFire => row("attack_bombard", Effects, Heavy, false),
        SoundKind::FlakhoundFire => row("attack_flakhound", Effects, Standard, false),
        SoundKind::BuzzardFire => row("attack_buzzard", Effects, Standard, false),
        SoundKind::TalonFire => row("attack_talon", Effects, Detail, false),
        SoundKind::RocFire => row("attack_roc", Effects, Standard, false),
        SoundKind::DroneFire => row("attack_drone", Effects, Detail, false),
        SoundKind::DroneLaunch => row("drone_launch", Effects, Standard, false),
        SoundKind::DroneDock => row("drone_dock", Effects, Detail, false),
        SoundKind::BastionFire => row("attack_bastion", Effects, Heavy, false),
        SoundKind::FlakTurretFire => row("attack_flak_turret", Effects, Standard, false),
        SoundKind::WardenFire => row("attack_warden", Effects, Standard, false),
        SoundKind::BreakerFire => row("attack_breaker", Effects, Heavy, false),
        SoundKind::PiledriverFire => row("attack_piledriver", Effects, Heavy, false),
        SoundKind::PiledriverImpact => row("piledriver_impact", Effects, Heavy, false),
        SoundKind::PiledriverDeploy => row("piledriver_deploy", Effects, Detail, false),
        SoundKind::PiledriverFold => row("piledriver_fold", Effects, Detail, false),
        SoundKind::AvalancheFire => row("avalanche_launch", Effects, Heavy, false),
        SoundKind::RocketMotor => row("avalanche_motor", Effects, Heavy, false),
        SoundKind::RocketImpact => row("rocket_impact", Effects, Heavy, true),
        SoundKind::BombRelease => row("bomb_release", Effects, Heavy, false),
        SoundKind::DemolitionBoom => row("demolition_boom", Effects, Heavy, true),
        SoundKind::UpgradeDone => row("upgrade_done", Effects, Notice, false),
        SoundKind::InterceptZap => row("intercept_zap", Effects, Detail, false),
        SoundKind::SpireLaunch => row("spire_launch", Effects, Heavy, false),
        SoundKind::IncomingWarning => row("incoming_warning", Effects, Protected, false),
        SoundKind::WarheadDetonation => row("warhead_detonation", Effects, Heavy, true),
        SoundKind::InterceptorLaunch => row("interceptor_launch", Effects, Standard, false),
        SoundKind::Interception => row("interception", Effects, Heavy, true),
        SoundKind::WarheadMotor => row("warhead_flight", Effects, Heavy, false),
        SoundKind::InterceptorMotor => row("interceptor_flight", Effects, Standard, false),
        SoundKind::MawCut => row("maw_cut", Effects, Standard, false),
    }
}

/// One clip's mixing, as `tools/gen_sounds.py` records it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MixerSpec {
    volume: f32,
    min_gap: f64,
}

/// The generated sound manifest is the one source for mixer gain and rate
/// limits.
static MIXER_SPECS: std::sync::LazyLock<std::collections::BTreeMap<String, MixerSpec>> =
    std::sync::LazyLock::new(|| {
        #[derive(serde::Deserialize)]
        struct Manifest {
            sounds: Vec<Row>,
        }
        #[derive(serde::Deserialize)]
        struct Row {
            name: String,
            mixer_volume: f64,
            min_gap: f64,
        }
        let manifest: Manifest =
            serde_json::from_str(include_str!("../../assets/sounds/manifest.json"))
                .expect("the generated sound manifest parses");
        manifest
            .sounds
            .into_iter()
            .map(|row| {
                let spec = MixerSpec {
                    volume: numeric::to_f32(row.mixer_volume),
                    min_gap: row.min_gap,
                };
                (row.name, spec)
            })
            .collect()
    });

fn mixer_spec(kind: SoundKind) -> MixerSpec {
    MIXER_SPECS
        .get(spec(kind).clip)
        .copied()
        .expect("every sound kind has a manifest row")
}

/// How long a kind's recent reports keep turning its next one down.
const BURST_WINDOW: f64 = 0.6;
/// The widest pitch spread, either way, on reports that repeat.
const PITCH_SPREAD: f32 = 0.03;

/// Plays queued clips with a per-kind rate limit, so twenty simultaneous
/// weapon reports read as battle, not noise. Within a burst each report of a
/// kind plays quieter than the last, and repeated small reports vary a
/// little in pitch, so a volley does not sound like one clip on repeat.
#[derive(Default)]
pub(crate) struct Mixer {
    pub(crate) rocket_loops: crate::rocket_audio::RocketLoops,
    pub(crate) warhead_loops:
        crate::strike_audio::StrikeLoops<{ crate::strike_audio::WARHEAD_VOICES }>,
    pub(crate) interceptor_loops:
        crate::strike_audio::StrikeLoops<{ crate::strike_audio::INTERCEPTOR_VOICES }>,
    pub(crate) maw_cut_loops: crate::maw_audio::MawLoops,
    recent: std::collections::HashMap<SoundKind, std::collections::VecDeque<f64>>,
    /// Alternates the basic zap between two clips so volleys read as
    /// many guns, not one sample looping.
    laser_flip: bool,
    /// Presentation-only noise for pitch variation.
    noise: u32,
}

impl Mixer {
    /// Seconds a clip waits before it may play again.
    fn min_gap(kind: SoundKind) -> f64 {
        mixer_spec(kind).min_gap
    }

    pub(crate) fn base_volume(kind: SoundKind) -> f32 {
        mixer_spec(kind).volume
    }

    fn clip<'a>(&mut self, sounds: &'a assets::Sounds, kind: SoundKind) -> &'a Sound {
        if kind == SoundKind::Laser {
            self.laser_flip = !self.laser_flip;
            if !self.laser_flip {
                return &sounds.laser2;
            }
        }
        sounds.effect(kind)
    }

    /// A playback rate within the pitch spread for kinds that repeat in
    /// volleys; heavy and protected clips keep their recorded pitch.
    fn rate(&mut self, kind: SoundKind) -> f32 {
        if !matches!(
            spec(kind).weight,
            Weight::Detail | Weight::Standard | Weight::Notice
        ) || spec(kind).bus == Bus::Ui
        {
            return 1.0;
        }
        self.noise = self
            .noise
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        let unit = (self.noise >> 8) as f32 / (1 << 24) as f32;
        1.0 + PITCH_SPREAD * (2.0 * unit - 1.0)
    }

    /// The burst gain for a report of `kind` at `now`: one over the square
    /// root of how many of its kind started in the window, this one included.
    fn burst_gain(&mut self, kind: SoundKind, now: f64) -> f32 {
        let recent = self.recent.entry(kind).or_default();
        while recent.front().is_some_and(|&at| now - at >= BURST_WINDOW) {
            recent.pop_front();
        }
        recent.push_back(now);
        (recent.len() as f32).sqrt().recip()
    }

    pub(crate) fn play(&mut self, sounds: &assets::Sounds, sound: FrameSound) {
        let kind = sound.kind;
        let now = get_time();
        let min_gap = Self::min_gap(kind);
        let last = self
            .recent
            .get(&kind)
            .and_then(|recent| recent.back().copied());
        if last.is_some_and(|last| now - last < min_gap) {
            return;
        }
        let protected = spec(kind).weight == Weight::Protected;
        let burst = if protected {
            1.0
        } else {
            self.burst_gain(kind, now)
        };
        if protected {
            self.recent.entry(kind).or_default().push_back(now);
        }
        let volume = Self::base_volume(kind) * sound.gain * burst;
        if volume <= 0.0 {
            return;
        }
        let rate = self.rate(kind);
        let clip = self.clip(sounds, kind);
        play_sound(
            clip,
            PlaySoundParams {
                pan: sound.pan,
                rate,
                remoteness: sound.remoteness,
                ..PlaySoundParams::new(spec(kind).bus, false, volume)
            },
        );
    }
}

#[cfg(test)]
mod tests;
