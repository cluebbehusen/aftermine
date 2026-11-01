//! Presentation effects: the visual vocabulary (shots, shells, bursts,
//! pings), the sound kinds, and the event-to-effect mapping that turns
//! sim reports into transient visuals and positional audio. Nothing
//! here is sim-relevant; dropping it all is always safe.

use super::{Presentation, world_vec};
use crate::seat_style::AllegianceCue;
use aftermine_sim::State;

use aftermine_sim::Event;
use macroquad::prelude::Vec2;

#[derive(Clone, Copy)]
pub(crate) struct CollapseBody {
    pub kind: aftermine_sim::BuildingKind,
    pub tier: u8,
    pub player: aftermine_sim::PlayerId,
    pub rotation: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct UnitBody {
    pub kind: aftermine_sim::UnitKind,
    pub player: aftermine_sim::PlayerId,
    pub rotation: f32,
    pub velocity: Vec2,
    /// Each mounted gun's last drawn world bearing.
    pub mounts: [Option<f32>; aftermine_sim::stats::MAX_WEAPONS],
}

impl UnitBody {
    /// A body seen only at its death or its crash: each mounted gun rests at
    /// its arc's centre on a hull turned to `rotation`.
    pub(crate) fn resting(
        kind: aftermine_sim::UnitKind,
        player: aftermine_sim::PlayerId,
        rotation: f32,
    ) -> Self {
        let mut mounts = [None; aftermine_sim::stats::MAX_WEAPONS];
        if let Some(guns) = kind.stats().mounts {
            for (bearing, mount) in mounts.iter_mut().zip(guns.mounts) {
                *bearing =
                    Some(rotation + f32::from(mount.arc_centre) * std::f32::consts::TAU / 256.0);
            }
        }
        Self {
            kind,
            player,
            rotation,
            velocity: Vec2::ZERO,
            mounts,
        }
    }

    fn capture(
        game: &Presentation,
        state: &State,
        unit: &aftermine_sim::state::Unit,
        reduced_motion: bool,
    ) -> Self {
        let kind = unit.kind;
        let rotation =
            if super::gun_turns_off_hull(kind) || super::rotor_hull_turn_rate(kind).is_some() {
                game.draw_hull_heading(state, unit.id, 1.0)
            } else if kind.stats().turn_rate > 0
                || kind.ground_turn_rate() > 0
                || kind.stats().cruise_turn_rate > 0
                || kind.stats().turret_turn_rate > 0
            {
                game.draw_heading(unit.id, unit.weapon_heading(), 1.0)
            } else {
                game.aim_units
                    .get(&unit.id.0)
                    .map(|pose| pose.0)
                    .or_else(|| game.facing.get(&unit.id.0).copied())
                    .unwrap_or(0.0)
            };
        let rotation = if super::rotor_hull_turn_rate(kind).is_some() {
            rotation
        } else {
            rotation + game.slide_yaw(unit.id, 1.0, reduced_motion)
        };
        let velocity = game
            .prev_pos
            .get(&unit.id.0)
            .map_or(Vec2::ZERO, |previous| {
                (world_vec(unit.pos) - *previous) / super::TICK_DT
            });
        Self {
            kind,
            player: unit.player,
            rotation,
            velocity: velocity
                .clamp_length_max(kind.stats().speed.to_num::<f32>() / super::TICK_DT),
            mounts: std::array::from_fn(|slot| game.draw_mount_heading(unit, slot, 1.0)),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct BuildingHit {
    pub id: aftermine_sim::BuildingId,
    pub kind: aftermine_sim::BuildingKind,
    pub tier: u8,
    pub anchor: Vec2,
    pub facts: crate::presentation_animation::BuildingAnimationFacts,
}

impl BuildingHit {
    pub(crate) fn capture(state: &State, building: &aftermine_sim::Building) -> Self {
        Self {
            id: building.id,
            kind: building.kind,
            tier: building.tier,
            anchor: Vec2::new(building.anchor.x as f32, building.anchor.y as f32),
            facts: crate::presentation_animation::BuildingAnimationFacts::capture(state, building),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct UnitHit {
    pub id: aftermine_sim::UnitId,
    pub body: UnitBody,
    pub frame: crate::render::UnitSpriteFrame,
    pub center: Vec2,
    pub airborne: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum HitSurface {
    Building(BuildingHit),
    Unit(UnitHit),
}

impl HitSurface {
    pub(crate) fn covers(self, at: Vec2) -> bool {
        match self {
            Self::Building(hit) => {
                let (w, h) = hit.kind.size();
                at.x >= hit.anchor.x
                    && at.y >= hit.anchor.y
                    && at.x <= hit.anchor.x + w as f32
                    && at.y <= hit.anchor.y + h as f32
            }
            Self::Unit(hit) => {
                hit.center.distance(at) <= hit.body.kind.stats().radius.to_num::<f32>()
            }
        }
    }
}

#[derive(Default)]
pub(super) struct PreviousEffects {
    buildings: Vec<(aftermine_sim::BuildingId, Option<CollapseBody>, BuildingHit)>,
    shells: Vec<aftermine_sim::state::Shell>,
    units: Vec<(
        aftermine_sim::UnitId,
        UnitBody,
        Vec2,
        bool,
        crate::render::UnitSpriteFrame,
    )>,
    visible_crash_contacts: Vec<aftermine_sim::UnitId>,
    /// The warheads this seat knew of, so a stopped one can fly on as a
    /// ghost after the simulation removes it.
    warheads: Vec<aftermine_sim::strike::KnownWarhead>,
}

impl PreviousEffects {
    pub(super) fn capture(game: &Presentation, state: &State) -> Self {
        Self {
            visible_crash_contacts: state
                .aircraft_crashes()
                .iter()
                .filter(|crash| {
                    crash.arrival == state.current_tick()
                        && (crash.player == game.human
                            || game.all_seeing()
                            || state
                                .vision(game.human)
                                .visible(chassis::grid::TilePos::containing(crash.impact)))
                })
                .map(|crash| crash.unit)
                .collect(),
            buildings: state
                .buildings()
                .iter()
                .filter(|b| {
                    !b.provisional()
                        && (b.player == game.human
                            || game.all_seeing()
                            || (b.tiles().any(|t| state.vision(game.human).visible(t))
                                && state.building_apparent(game.human, b)))
                })
                .map(|b| {
                    (
                        b.id,
                        b.built().then_some(CollapseBody {
                            kind: b.kind,
                            tier: b.tier,
                            player: b.player,
                            rotation: game.draw_gun_heading(b, 1.0),
                        }),
                        BuildingHit::capture(state, b),
                    )
                })
                .collect(),
            shells: state.shells().to_vec(),
            warheads: if game.all_seeing() {
                state.omniscient_warheads(game.human)
            } else {
                state.known_warheads(game.human)
            },
            units: state
                .units()
                .iter()
                .filter(|unit| {
                    unit.player == game.human
                        || game.all_seeing()
                        || state.vision(game.human).sees_unit(unit)
                })
                .map(|unit| {
                    (
                        unit.id,
                        UnitBody::capture(game, state, unit, crate::render::reduced_motion()),
                        game.draw_pos(unit.id, unit.pos, 1.0),
                        unit.domain() == aftermine_sim::stats::Domain::Air,
                        crate::render::UnitSpriteFrame::capture(
                            unit.kind,
                            crate::render::unit_animation(game, state, unit),
                        ),
                    )
                })
                .collect(),
        }
    }
}

pub struct Effect {
    /// What to draw.
    pub kind: EffectKind,
    /// Wall seconds alive for effects that do not ride the simulation clock.
    pub age: f32,
}

impl Effect {
    /// Age at one simulation-timeline instant. Direct-fire reports follow
    /// sim time so their rounds stay attached to authored muzzle frames at
    /// every game and replay speed. Their wall-age field is only allowed to
    /// drain a terminal battlefield after simulation time has stopped.
    pub(crate) fn age_at(&self, completed_ticks: u64, tick_fraction: f32) -> f32 {
        match self.kind {
            EffectKind::Intercept { completed_tick, .. }
            | EffectKind::DirectShot { completed_tick, .. }
            | EffectKind::SapperDetonation { completed_tick, .. }
            | EffectKind::Impact { completed_tick, .. }
            | EffectKind::Interception { completed_tick, .. }
            | EffectKind::WarheadBurst { completed_tick, .. } => {
                let whole = completed_ticks.saturating_sub(completed_tick) as f32;
                (whole + tick_fraction.clamp(0.0, 1.0)) * super::TICK_DT + self.age
            }
            EffectKind::Falling {
                crash: Some(crash), ..
            } => {
                let whole = completed_ticks.saturating_sub(crash.started + 1) as f32;
                (whole + tick_fraction.clamp(0.0, 1.0)) * super::TICK_DT + self.age
            }
            _ => self.age,
        }
    }
}

chassis::listed_enum! {
    /// A clip the shell should play (queued by sim events, drained per frame).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum SoundKind {
        /// An attack landed somewhere you can see.
        Laser,
        /// A unit died somewhere you can see.
        UnitDeath,
        /// A building fell or a heavy airframe hit the ground.
        BuildingBoom,
        /// Your harvester delivered.
        Deposit,
        /// Your Foundry finished a unit.
        TrainDone,
        /// Menu activation.
        Click,
        /// An order was rejected.
        Denied,
        /// High-priority warning that the local player is under attack.
        Alert,
        /// The match ended in your favor.
        Victory,
        /// It did not.
        Defeat,
        /// An artillery shell landing.
        Artillery,
        /// A hostile artillery launch heard from a visible impact warning.
        ArtilleryLaunch,
        /// An order acknowledged.
        Ack,
        /// A Sentinel's compact cannon report.
        SentinelFire,
        /// A Scuttler's paired mechanical shear.
        ScuttlerFire,
        /// A Lancer's charged rail report.
        LancerFire,
        /// A Bombard's heavy artillery report.
        BombardFire,
        /// A Flakhound's paired anti-air burst.
        FlakhoundFire,
        /// A Buzzard's heavy strike.
        BuzzardFire,
        /// A Talon's interceptor burst.
        TalonFire,
        /// A Roc's volley: both guns fire on the same tick.
        RocFire,
        /// A drone's needle shot.
        DroneFire,
        /// A Roc's bay letting its drones go.
        DroneLaunch,
        /// A drone clamping home in its bay.
        DroneDock,
        /// A Bastion's emplaced artillery report.
        BastionFire,
        /// A Flak Turret's paired-yoke burst.
        FlakTurretFire,
        /// The Warden's fork cannon report.
        WardenFire,
        /// The Breaker's siege mortar.
        BreakerFire,
        /// The Piledriver's pressure discharge.
        PiledriverFire,
        /// Its compact armor impact.
        PiledriverImpact,
        /// Ground anchors deploying.
        PiledriverDeploy,
        /// Ground anchors folding for travel.
        PiledriverFold,
        /// The Avalanche bank launching.
        AvalancheFire,
        /// Missile motor ignition after launcher ejection.
        RocketMotor,
        /// A missile warhead reaching its impact point.
        RocketImpact,
        /// A bomber releasing its load.
        BombRelease,
        /// A buried charge or Sapper detonating.
        DemolitionBoom,
        /// A works coming back online one rung higher.
        UpgradeDone,
        /// A point defender's emitter firing at a shell.
        InterceptZap,
        /// A Spire unlatching and launching its warhead.
        SpireLaunch,
        /// A hostile warhead first known to this seat: protected like the
        /// under-attack alert.
        IncomingWarning,
        /// A warhead bursting on the ground.
        WarheadDetonation,
        /// An interceptor leaving its Lattice pod.
        InterceptorLaunch,
        /// An interceptor meeting its warhead in the sky.
        Interception,
        /// A warhead's motor while it burns.
        WarheadMotor,
        /// An interceptor's motor while it climbs.
        InterceptorMotor,
        /// A Maw's drum while it cuts.
        MawCut,
        /// A Maw's kiln while its belt carries wreck.
        MawKiln,
    }
}

/// What an order-acknowledgment ping means (decides its color).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PingKind {
    /// Move / advance / hunt destination.
    Move,
    /// Attack target.
    Attack,
    /// Harvest node.
    Harvest,
    /// Rally point.
    Rally,
    /// A unit left the Foundry.
    Spawn,
}

/// Delay between the two visible rounds of one logical flak hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlakYokeDelay {
    /// The second yoke fires one simulation tick after the first.
    OneTick,
    /// The second yoke fires halfway through a three-tick report.
    OneAndHalfTicks,
}

impl FlakYokeDelay {
    /// Authored delay in simulation ticks.
    pub(crate) fn ticks(self) -> f32 {
        match self {
            Self::OneTick => 1.0,
            Self::OneAndHalfTicks => 1.5,
        }
    }

    /// Authored delay in seconds on the simulation timeline.
    pub(crate) fn seconds(self) -> f32 {
        self.ticks() * super::TICK_DT
    }
}

/// The visual family of a direct-fire shot, mapped from the exact
/// (shooter kind, weapon slot) the hit event names so every weapon reads
/// as itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShotStyle {
    /// A contact tool: target sparks, never a ranged projectile.
    Contact,
    /// A compact forge-bright orb with no persistent tracer.
    ForgeSpot,
    /// A short metal round, followed by a compact impact burst.
    Kinetic { heavy: bool },
    /// A short cosmetic mortar report with a rupturing contact.
    Mortar,
    /// The Lancer's brief discharge and fading rail trace.
    Rail,
    /// One logical anti-air burst, with one to three rounds from each yoke.
    FlakBurst {
        /// When the second visible yoke reports.
        yoke_delay: FlakYokeDelay,
        /// Barrels firing together on each side.
        rounds_per_yoke: u8,
    },
}

impl ShotStyle {
    /// Seconds the report stays on screen.
    pub fn life(self) -> f32 {
        match self {
            ShotStyle::Contact => 0.12,
            ShotStyle::ForgeSpot => 0.32,
            ShotStyle::Kinetic { heavy: false } => 0.18,
            ShotStyle::Kinetic { heavy: true } => 0.24,
            ShotStyle::Mortar => 0.64,
            ShotStyle::Rail => 0.24,
            ShotStyle::FlakBurst { .. } => 0.30,
        }
    }
}

/// Which report family a unit's weapon slot fires: every slot speaks
/// through the kind's one physical barrel. None for a kind that never draws
/// a direct report.
fn unit_shot_style(kind: aftermine_sim::UnitKind, _weapon: usize) -> Option<ShotStyle> {
    crate::look::unit(kind)
        .weapon
        .and_then(|weapon| weapon.shot)
        .map(|(style, _)| style)
}

fn visual_shot_origin(from: Vec2, to: Vec2, reach: f32) -> Vec2 {
    let direction = to - from;
    if direction.length_squared() <= f32::EPSILON {
        from
    } else {
        from + direction.normalize() * reach
    }
}

fn unit_muzzle_reach(kind: aftermine_sim::UnitKind) -> Option<f32> {
    crate::look::unit(kind)
        .weapon
        .and_then(|weapon| weapon.shot)
        .map(|(_, reach)| reach)
}

fn unit_shot_origin(kind: aftermine_sim::UnitKind, from: Vec2, to: Vec2) -> Vec2 {
    if kind.stats().contact_reach.is_some() {
        return from;
    }
    let Some(reach) = unit_muzzle_reach(kind) else {
        return from;
    };
    let mut origin = visual_shot_origin(from, to, reach);
    if kind.stats().domain == aftermine_sim::stats::Domain::Air {
        origin.y -= crate::render::air_presentation(kind, 1.0).2;
    }
    origin
}

/// The report a unit's weapon makes; an unarmed kind, or one whose work is
/// heard as a loop, makes none.
fn unit_fire_sound(kind: aftermine_sim::UnitKind) -> Option<SoundKind> {
    crate::look::unit(kind)
        .weapon
        .and_then(|weapon| weapon.sound)
}

fn shell_fire_sound(shooter: aftermine_sim::Target) -> SoundKind {
    match shooter {
        aftermine_sim::Target::Unit(_) => SoundKind::BombardFire,
        aftermine_sim::Target::Building(_) => SoundKind::BastionFire,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellSoundAnchor {
    Muzzle,
    Impact,
}

/// A launch heard at the muzzle is the shooting unit's own report; only a
/// shooter without a unit pose falls back to its family's.
fn shell_launch_audio(
    shooter: aftermine_sim::Target,
    shooter_kind: Option<aftermine_sim::UnitKind>,
    allegiance: AllegianceCue,
    muzzle_seen: bool,
    impact_seen: bool,
) -> Option<(SoundKind, ShellSoundAnchor)> {
    if muzzle_seen || allegiance == AllegianceCue::Mine {
        let report = shooter_kind
            .and_then(unit_fire_sound)
            .unwrap_or_else(|| shell_fire_sound(shooter));
        Some((report, ShellSoundAnchor::Muzzle))
    } else if allegiance == AllegianceCue::Hostile && impact_seen {
        Some((SoundKind::ArtilleryLaunch, ShellSoundAnchor::Impact))
    } else {
        None
    }
}

#[derive(Clone, Copy)]
pub(crate) struct InterceptFlight {
    pub launch: Vec2,
    pub impact: Vec2,
    pub shooter: aftermine_sim::Target,
    pub kind: aftermine_sim::ProjectileKind,
    pub pose: Option<super::LaunchPose>,
    pub surface: Option<HitSurface>,
    pub progress: f32,
    pub ticks: f32,
}

pub(crate) fn mantle_emitter(center: Vec2, rotation: f32, slot: u8) -> Vec2 {
    let local = Vec2::new(58.0 + f32::from(slot.min(2)) * 6.0 - 64.0, 25.0 - 64.0) / 128.0
        * crate::look::unit(aftermine_sim::UnitKind::Mantle).scale;
    center + Vec2::from_angle(rotation).rotate(local)
}

/// Seconds a stopped warhead flies on as a ghost while its interceptor
/// climbs to meet it in the sky.
/// Ticks an interceptor climbs from its pod to its warhead.
pub(crate) const INTERCEPT_FLIGHT_TICKS: f32 = 22.0;
/// Ticks a pod's clamp takes to open before its interceptor leaves.
pub(crate) const INTERCEPT_CLAMP_TICKS: f32 = 4.0;
/// Ticks a pod takes to swing onto its interceptor's path.
pub(crate) const INTERCEPT_AIM_TICKS: f32 = 4.0;
/// Ticks both missiles fly on into the burst before it swallows them.
pub(crate) const INTERCEPT_OVERLAP_TICKS: f32 = 2.0;
/// The shortest climb drawn for a stop forecast late.
const INTERCEPT_MIN_FLIGHT_TICKS: f32 = 6.0;
/// Ticks a warhead stopped with no forecast flies on to meet the
/// interceptor that leaves as the stop is reported.
const INTERCEPT_LATE_TICKS: f32 = 6.0;
/// Lead at which a forecast stop starts its pod: aim, clamp, then climb.
/// The simulation's lock-on gives at least this much.
pub(crate) const INTERCEPT_LEAD_TICKS: f32 =
    INTERCEPT_AIM_TICKS + INTERCEPT_CLAMP_TICKS + INTERCEPT_FLIGHT_TICKS;
/// Seconds the sky burst lasts after contact.
const INTERCEPT_AFTERMATH: f32 = 1.8;

/// A forecast stop under way: the pod swinging onto its interceptor's path,
/// its clamp opening, then the interceptor climbing to meet the warhead
/// where the simulation stops it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Prelaunch {
    /// The warhead it will stop.
    pub warhead: aftermine_sim::WarheadId,
    /// The battery and the pod that fires.
    pub battery: aftermine_sim::BuildingId,
    pub pod: u8,
    /// The battery's owner, for the interceptor's identity tint.
    pub battery_player: aftermine_sim::PlayerId,
    /// The warhead's whole flight.
    pub line: aftermine_sim::strike::WarheadLine,
    /// Presentation tick the pod started to swing.
    pub created: f32,
    /// The tick the simulation stops the warhead on.
    pub stop: f32,
    /// The interceptor's climb.
    pub path: crate::render::sky::InterceptorPath,
    /// Where the noses meet, world coords.
    pub contact: Vec2,
    /// Whether its launch has been heard and its pod's shot recorded.
    pub heard: bool,
}

impl Prelaunch {
    /// How far its pod has swung onto the path at presentation `tick`.
    pub(crate) fn aimed(&self, tick: f32) -> f32 {
        ((tick - self.created) / INTERCEPT_AIM_TICKS).clamp(0.0, 1.0)
    }

    /// How far its pod's clamp has opened at `tick`.
    pub(crate) fn opened(&self, tick: f32) -> f32 {
        ((tick - (self.path.launch - INTERCEPT_CLAMP_TICKS)) / INTERCEPT_CLAMP_TICKS)
            .clamp(0.0, 1.0)
    }

    /// Whether the interceptor has left its pod at `tick`.
    pub(crate) fn launched(&self, tick: f32) -> bool {
        tick >= self.path.launch
    }
}

/// Seconds a warhead's ground burst lasts, smoke column included.
pub(crate) const WARHEAD_BURST: f32 = 3.2;

/// Width, in tiles, of the scorch a destroyed building leaves.
const BUILDING_SCORCH: f32 = 2.4;

/// The largest jolt, in logical pixels, a warhead landing at the centre of
/// the view gives the camera; it fades to nothing this many tiles away.
const STRIKE_SHAKE: f32 = 9.0;
const STRIKE_SHAKE_REACH: f32 = 36.0;

/// Effect shapes.
pub enum EffectKind {
    /// A spent point-defense emitter and the interrupted projectile.
    Intercept {
        from: Option<Vec2>,
        at: Vec2,
        flight: Option<InterceptFlight>,
        impact_witnessed: bool,
        completed_tick: u64,
    },
    /// A direct-fire shot, styled by the weapon family that spoke.
    DirectShot {
        /// Visual family (contact, kinetic, rail, or flak).
        style: ShotStyle,
        /// Muzzle, world coords.
        from: Vec2,
        /// Impact, world coords.
        to: Vec2,
        /// Splash radius, if this logical hit has one.
        splash: Option<f32>,
        /// Unit that issued this report, when applicable.
        attacker: Option<aftermine_sim::UnitId>,
        /// Visible target geometry survives a lethal hit for this report.
        surface: Option<HitSurface>,
        /// Consumed when the cosmetic kinetic round reaches its target.
        impact_sound: Option<SoundKind>,
        /// Simulation tick immediately after the hit was reported.
        completed_tick: u64,
    },
    /// A Sapper's decisive action pose, retained after its carrier dies.
    SapperDetonation {
        /// Exact firing position.
        at: Vec2,
        /// Exact center used by the simulation's splash ring.
        blast_at: Vec2,
        /// Direction toward the contacted target.
        rotation: f32,
        /// Owning seat, for the same identity tint the live unit used.
        player: aftermine_sim::PlayerId,
        /// The source position was legitimately known when the report arrived.
        source_witnessed: bool,
        /// The impact position was legitimately known when the report arrived.
        impact_witnessed: bool,
        /// Simulation tick immediately after the hit was reported.
        completed_tick: u64,
    },
    /// An airborne casualty: heavy airframes crash; smaller ones burst in flight.
    Falling {
        at: Vec2,
        body: UnitBody,
        seed: u32,
        crash: Option<aftermine_sim::state::AircraftCrash>,
        impact_witnessed: bool,
    },
    /// A retained structural silhouette collapsing into low wreckage.
    Collapse {
        at: Vec2,
        body: CollapseBody,
        seed: u32,
    },
    /// A payload-specific impact.
    Impact {
        at: Vec2,
        radius: f32,
        payload: aftermine_sim::ProjectileKind,
        from: Vec2,
        surface: Option<HitSurface>,
        completed_tick: u64,
    },
    /// A death pop.
    Puff {
        /// Center, world coords.
        at: Vec2,
    },
    /// An order acknowledgment: a ring collapsing onto the ordered point.
    Ping {
        /// Center, world coords.
        at: Vec2,
        /// Color class.
        kind: PingKind,
        /// Whether the order joined the unit's program rather than
        /// replacing it.
        queued: bool,
    },
    /// A splash detonation blooming over its radius.
    Burst {
        /// Impact center, world coords.
        at: Vec2,
        /// Splash radius, tiles.
        radius: f32,
    },
    /// A stopped warhead and its interceptor flying on into the sky burst
    /// where their noses meet, then the burst, its smoke and its falling
    /// fragments. The simulation removed the warhead when this began;
    /// everything here is presentation.
    Interception {
        /// The warhead's whole flight, when this seat knew it.
        line: Option<aftermine_sim::strike::WarheadLine>,
        /// The warhead's owner when this seat knew it, and whether hostile.
        warhead_player: Option<aftermine_sim::PlayerId>,
        warhead_hostile: bool,
        /// The interceptor's climb, when its battery still stood.
        path: Option<crate::render::sky::InterceptorPath>,
        /// The battery's owner, for the interceptor's identity tint.
        battery_player: aftermine_sim::PlayerId,
        /// Where the noses meet and the burst centres, world coords.
        contact: Vec2,
        /// Presentation tick the noses meet: the stop itself when the
        /// interceptor left ahead of it, a moment later otherwise.
        contact_tick: f32,
        /// Fragment scatter seed (the warhead's id).
        seed: u32,
        /// Simulation tick immediately after the stop was reported.
        completed_tick: u64,
        /// Whether the sky burst has been heard; it sounds at contact.
        burst_heard: bool,
    },
    /// A warhead's ground burst: flash, shockwave, fireball, smoke column.
    WarheadBurst {
        /// Impact centre, world coords.
        at: Vec2,
        /// Smoke scatter seed (the warhead's id).
        seed: u32,
        /// Whether this seat legitimately knows the burst: its team's
        /// warhead, or ground it sees.
        witnessed: bool,
        /// Simulation tick immediately after the landing was reported.
        completed_tick: u64,
    },
    /// A ruptured ground chassis and rigid fragments settling on the floor.
    Debris {
        /// Death point, world coords.
        at: Vec2,
        body: UnitBody,
        /// Deterministic scatter seed (the casualty's id).
        seed: u32,
    },
}

fn push_direct_report(
    effects: &mut Vec<Effect>,
    style: ShotStyle,
    from: Vec2,
    to: Vec2,
    splash: Option<f32>,
    completed_tick: u64,
    surface: Option<HitSurface>,
) -> &mut Effect {
    effects.push(Effect {
        kind: EffectKind::DirectShot {
            style,
            from,
            to,
            splash,
            completed_tick,
            surface,
            attacker: None,
            impact_sound: None,
        },
        age: 0.0,
    });
    effects.last_mut().expect("report was inserted")
}

fn event_target_owner(
    state: &aftermine_sim::State,
    events: &[Event],
    target: aftermine_sim::Target,
) -> Option<aftermine_sim::PlayerId> {
    match target {
        aftermine_sim::Target::Unit(id) => state.unit(id).map(|unit| unit.player).or_else(|| {
            events.iter().find_map(|event| match event {
                Event::UnitDied { unit, player, .. } if *unit == id => Some(*player),
                _ => None,
            })
        }),
        aftermine_sim::Target::Building(id) => state
            .building(id)
            .map(|building| building.player)
            .or_else(|| {
                events.iter().find_map(|event| match event {
                    Event::BuildingDestroyed {
                        building, player, ..
                    } if *building == id => Some(*player),
                    _ => None,
                })
            }),
    }
}

impl Presentation {
    pub(super) fn restore_pending_crashes(&mut self, state: &State) {
        for crash in state.aircraft_crashes().to_vec() {
            self.restore_crash_effect(state, crash, false);
        }
    }

    fn restore_crash_effect(
        &mut self,
        state: &State,
        crash: aftermine_sim::state::AircraftCrash,
        witnessed: bool,
    ) {
        for effect in &mut self.fx {
            if let EffectKind::Falling {
                crash: Some(existing),
                impact_witnessed,
                ..
            } = &mut effect.kind
                && existing.unit == crash.unit
            {
                *impact_witnessed |= witnessed;
                return;
            }
        }
        let visible = witnessed
            || self.all_seeing()
            || crash.player == self.human
            || state
                .vision(self.human)
                .visible(chassis::grid::TilePos::containing(crash.launch))
            || state
                .vision(self.human)
                .visible(chassis::grid::TilePos::containing(crash.impact));
        if !visible {
            return;
        }
        self.fx.push(Effect {
            kind: EffectKind::Falling {
                at: world_vec(crash.launch),
                seed: crash.unit.0,
                crash: Some(crash),
                impact_witnessed: witnessed,
                body: UnitBody::resting(
                    crash.kind,
                    crash.player,
                    f32::from(crash.heading) * std::f32::consts::TAU / 256.0
                        + std::f32::consts::FRAC_PI_2,
                ),
            },
            age: 0.0,
        });
    }

    /// Advances the effect clock and ages and prunes effects, alerts, and
    /// toasts.
    pub fn update_fx(&mut self, state: &State, dt: f32) {
        self.fx_clock += dt;
        for (_, age, _) in &mut self.alerts {
            *age += dt;
        }
        self.alerts.retain(|(_, age, _)| *age < 6.0);
        let terminal = state.result().is_some();
        for fx in &mut self.fx {
            match fx.kind {
                EffectKind::Intercept { .. }
                | EffectKind::DirectShot { .. }
                | EffectKind::SapperDetonation { .. }
                | EffectKind::Impact { .. }
                | EffectKind::Interception { .. }
                | EffectKind::WarheadBurst { .. }
                | EffectKind::Falling { crash: Some(_), .. }
                    if terminal =>
                {
                    fx.age += dt;
                }
                EffectKind::Intercept { .. }
                | EffectKind::DirectShot { .. }
                | EffectKind::SapperDetonation { .. }
                | EffectKind::Impact { .. }
                | EffectKind::Interception { .. }
                | EffectKind::WarheadBurst { .. }
                | EffectKind::Falling { crash: Some(_), .. } => {}
                _ => fx.age += dt,
            }
        }
        let completed_ticks = state.current_tick();
        let tick_fraction = self.tick_fraction();
        // The sky burst sounds as the interceptor reaches its warhead, on
        // the same simulation clock the burst draws on.
        for fx in &mut self.fx {
            let age = fx.age_at(completed_ticks, tick_fraction);
            if let EffectKind::Interception {
                contact,
                contact_tick,
                completed_tick,
                burst_heard,
                ..
            } = &mut fx.kind
                && !*burst_heard
                && completed_tick.saturating_sub(1) as f32 + age / super::TICK_DT >= *contact_tick
            {
                *burst_heard = true;
                self.sounds_pending
                    .push((SoundKind::Interception, Some(*contact)));
            }
        }
        for fx in &mut self.fx {
            let age = fx.age_at(completed_ticks, tick_fraction);
            if let EffectKind::DirectShot {
                style,
                to,
                impact_sound,
                ..
            } = &mut fx.kind
                && age >= style.life() * 0.6
                && let Some(sound) = impact_sound.take()
            {
                self.sounds_pending.push((sound, Some(*to)));
            }
        }
        self.fx.retain(|fx| {
            fx.age_at(completed_ticks, tick_fraction)
                < match fx.kind {
                    EffectKind::Intercept { .. } => 10.0 * super::TICK_DT,
                    EffectKind::DirectShot { style, .. } => style.life(),
                    EffectKind::SapperDetonation { .. } => super::TICK_DT * 2.0,
                    EffectKind::Collapse { .. } => 4.5,
                    EffectKind::Impact { .. } => 1.4,
                    EffectKind::Puff { .. } => 0.4,
                    EffectKind::Falling { .. } => 4.0,
                    EffectKind::Ping { .. } => 0.5,
                    EffectKind::Burst { .. } => 0.35,
                    EffectKind::Interception {
                        contact_tick,
                        completed_tick,
                        ..
                    } => {
                        (contact_tick - completed_tick.saturating_sub(1) as f32).max(0.0)
                            * super::TICK_DT
                            + INTERCEPT_AFTERMATH
                    }
                    EffectKind::WarheadBurst { .. } => WARHEAD_BURST,
                    EffectKind::Debris { .. } => 3.0,
                }
        });
        for toast in &mut self.toasts {
            toast.age += dt;
        }
        self.toasts.retain(|t| t.age < 2.5);
        for (_, age, _) in &mut self.scorches {
            *age += dt;
        }
        self.scorches.retain(|(_, age, _)| *age < 20.0);
    }

    fn building_hit(
        &self,
        state: &State,
        target: Option<aftermine_sim::Target>,
    ) -> Option<BuildingHit> {
        let aftermine_sim::Target::Building(id) = target? else {
            return None;
        };
        state
            .building(id)
            .filter(|b| {
                !b.provisional()
                    && (self.all_seeing()
                        || b.player == self.human
                        || b.tiles().any(|t| state.vision(self.human).visible(t))
                            && state.building_apparent(self.human, b))
            })
            .map(|b| BuildingHit::capture(state, b))
            .or_else(|| {
                self.fx_previous
                    .buildings
                    .iter()
                    .find(|(bid, _, _)| *bid == id)
                    .map(|(_, _, hit)| *hit)
            })
    }

    pub(crate) fn hit_surface(
        &self,
        state: &State,
        target: Option<aftermine_sim::Target>,
    ) -> Option<HitSurface> {
        let target = target?;
        if let aftermine_sim::Target::Building(_) = target {
            return self
                .building_hit(state, Some(target))
                .map(HitSurface::Building);
        }
        let aftermine_sim::Target::Unit(id) = target else {
            return None;
        };
        let (body, mut center, airborne, frame) = if let Some(unit) = state.unit(id) {
            if !self.all_seeing()
                && unit.player != self.human
                && !state.vision(self.human).sees_unit(unit)
            {
                return None;
            }
            (
                UnitBody::capture(self, state, unit, crate::render::reduced_motion()),
                self.draw_pos(id, unit.pos, 1.0),
                unit.domain() == aftermine_sim::stats::Domain::Air,
                crate::render::UnitSpriteFrame::capture(
                    unit.kind,
                    crate::render::unit_animation(self, state, unit),
                ),
            )
        } else {
            let (_, body, center, airborne, frame) =
                self.fx_previous.units.iter().find(|(uid, ..)| *uid == id)?;
            (*body, *center, *airborne, *frame)
        };
        if airborne {
            center.y -= crate::render::air_presentation(body.kind, 1.0).2;
        }
        Some(HitSurface::Unit(UnitHit {
            id,
            body,
            frame,
            center,
            airborne,
        }))
    }

    pub(crate) fn payload_surface(
        &self,
        state: &State,
        target: Option<aftermine_sim::Target>,
        player: aftermine_sim::PlayerId,
        at: Vec2,
        targets: aftermine_sim::stats::DomainMask,
    ) -> Option<HitSurface> {
        let covers = |surface: HitSurface| surface.covers(at);
        let known = self
            .hit_surface(state, target)
            .filter(|surface| covers(*surface));
        if target.is_some() {
            return known;
        }
        known
            .or_else(|| {
                let mut units = state
                    .units()
                    .iter()
                    .filter(|unit| {
                        if !state.hostile(player, unit.player) || !targets.covers(unit.domain()) {
                            return false;
                        }
                        let mut center = self.draw_pos(unit.id, unit.pos, 1.0);
                        if unit.domain() == aftermine_sim::stats::Domain::Air {
                            center.y -= crate::render::air_presentation(unit.kind, 1.0).2;
                        }
                        center.distance(at) <= unit.kind.stats().radius.to_num::<f32>()
                    })
                    .map(|unit| unit.id)
                    .chain(
                        self.fx_previous
                            .units
                            .iter()
                            .filter(|(id, body, _, airborne, _)| {
                                state.unit(*id).is_none()
                                    && state.hostile(player, body.player)
                                    && if *airborne {
                                        targets.air
                                    } else {
                                        targets.ground
                                    }
                            })
                            .map(|(id, ..)| *id),
                    );
                units.find_map(|id| {
                    self.hit_surface(state, Some(aftermine_sim::Target::Unit(id)))
                        .filter(|surface| covers(*surface))
                })
            })
            .or_else(|| {
                if !targets.ground {
                    return None;
                }
                state
                    .buildings()
                    .iter()
                    .filter(|building| state.hostile(player, building.player))
                    .map(|building| building.id)
                    .chain(
                        self.fx_previous
                            .buildings
                            .iter()
                            .filter(|(id, body, _)| {
                                state.building(*id).is_none()
                                    && body.is_some_and(|body| state.hostile(player, body.player))
                            })
                            .map(|(id, ..)| *id),
                    )
                    .find_map(|id| {
                        self.hit_surface(state, Some(aftermine_sim::Target::Building(id)))
                            .filter(|surface| covers(*surface))
                    })
            })
    }

    pub(super) fn queue_brace_sounds(&mut self, state: &State) {
        for unit in state.units().iter().filter(|unit| {
            unit.kind == aftermine_sim::UnitKind::Piledriver
                && state.vision(self.human).sees_unit(unit)
        }) {
            let sound = match self.animations.brace_change(unit) {
                Some(crate::presentation_animation::BraceChange::Deploying) => {
                    SoundKind::PiledriverDeploy
                }
                Some(crate::presentation_animation::BraceChange::Folding) => {
                    SoundKind::PiledriverFold
                }
                None => continue,
            };
            self.sounds_pending.push((sound, Some(world_vec(unit.pos))));
        }
    }

    /// Turns a tick's events into flashes and queued clips. Explosions can be
    /// heard through fog; the camera mixer bounds their audible distance.
    /// Visual effects retain their independent sight rules.
    #[expect(clippy::too_many_lines, reason = "one arm per simulation event")]
    pub(super) fn spawn_fx(&mut self, state: &State, events: &[Event]) {
        let sees = |game: &Self, pos: chassis::fx::Vec2Fx| {
            state
                .vision(game.human)
                .visible(chassis::grid::TilePos::containing(pos))
        };
        let mut remaining = std::collections::HashMap::<aftermine_sim::UnitId, u8>::new();
        for event in events {
            if let Event::ShellIntercepted { defender, .. } = event {
                *remaining.entry(*defender).or_default() += 1;
            }
        }
        for event in events {
            match event {
                Event::DamageTaken { player, pos } if *player == self.human => {
                    self.raise_alert(world_vec(*pos));
                }
                Event::AttackHit {
                    attacker,
                    attacker_kind,
                    weapon,
                    attacker_pos,
                    target,
                    target_pos,
                    ..
                } => {
                    // The shooter turns to its work: aim overrides
                    // movement facing for a beat, and recoil ages off
                    // the same stamp.
                    let d = world_vec(*target_pos) - world_vec(*attacker_pos);
                    if d.length_squared() > 1e-6 {
                        if let Some(target) = target {
                            self.aim_unit_targets.insert(attacker.0, *target);
                        } else {
                            self.aim_unit_targets.remove(&attacker.0);
                        }
                        self.aim_units.insert(
                            attacker.0,
                            (d.y.atan2(d.x) + std::f32::consts::FRAC_PI_2, self.fx_clock),
                        );
                    }
                    let target_owner =
                        target.and_then(|target| event_target_owner(state, events, target));
                    // The kind comes from the event because the attacker
                    // may have died later this same tick. The weapon decides
                    // the report and whether the impact blooms.
                    let sapper_owner = attacker_kind
                        .stats()
                        .demolition
                        .is_some()
                        .then(|| {
                            events.iter().find_map(|event| match event {
                                Event::UnitDied { unit, player, .. } if unit == attacker => {
                                    Some(*player)
                                }
                                _ => None,
                            })
                        })
                        .flatten();
                    let source_witnessed =
                        sees(self, *attacker_pos) || sapper_owner == Some(self.human);
                    let impact_witnessed = sees(self, *target_pos)
                        || target_owner.is_some_and(|player| !state.hostile(self.human, player));
                    let heard = source_witnessed || impact_witnessed;
                    let sound = unit_fire_sound(*attacker_kind);
                    // The burst radius comes from the weapon slot the event
                    // names, so the drawn area matches the damage the sim
                    // deals.
                    let splash = attacker_kind
                        .stats()
                        .weapons
                        .get(*weapon)
                        .and_then(|w| w.splash)
                        .map(|s| s.to_num::<f32>());
                    if let Some(sound) = sound
                        && (heard || crate::mixer::spec(sound).explosion)
                    {
                        let at = if crate::mixer::spec(sound).explosion {
                            *target_pos
                        } else if sees(self, *attacker_pos) {
                            *attacker_pos
                        } else {
                            *target_pos
                        };
                        self.sounds_pending.push((sound, Some(world_vec(at))));
                    }
                    if attacker_kind.stats().demolition.is_some() {
                        if let Some(player) = sapper_owner {
                            let direction = world_vec(*target_pos) - world_vec(*attacker_pos);
                            let rotation = if direction.length_squared() > 1e-6 {
                                direction.y.atan2(direction.x) + std::f32::consts::FRAC_PI_2
                            } else {
                                0.0
                            };
                            self.fx.push(Effect {
                                kind: EffectKind::SapperDetonation {
                                    at: world_vec(*attacker_pos),
                                    blast_at: world_vec(*target_pos),
                                    rotation,
                                    player,
                                    source_witnessed,
                                    impact_witnessed,
                                    completed_tick: state.current_tick(),
                                },
                                age: 0.0,
                            });
                        }
                    } else {
                        let surface = self.hit_surface(state, *target);
                        let mut origin = unit_shot_origin(
                            *attacker_kind,
                            world_vec(*attacker_pos),
                            world_vec(*target_pos),
                        );
                        let airborne = state
                            .unit(*attacker)
                            .map(|unit| unit.domain() == aftermine_sim::stats::Domain::Air)
                            .or_else(|| {
                                self.fx_previous
                                    .units
                                    .iter()
                                    .find(|(id, ..)| id == attacker)
                                    .map(|(_, _, _, airborne, _)| *airborne)
                            })
                            .unwrap_or(
                                attacker_kind.stats().domain == aftermine_sim::stats::Domain::Air,
                            );
                        if attacker_kind.stats().domain == aftermine_sim::stats::Domain::Air
                            && !airborne
                        {
                            origin.y += crate::render::air_presentation(*attacker_kind, 1.0).2;
                        }
                        let Some(style) = unit_shot_style(*attacker_kind, *weapon) else {
                            continue;
                        };
                        let report = push_direct_report(
                            &mut self.fx,
                            style,
                            origin,
                            world_vec(*target_pos),
                            splash,
                            state.current_tick(),
                            surface,
                        );
                        if let EffectKind::DirectShot {
                            attacker: source,
                            impact_sound,
                            ..
                        } = &mut report.kind
                        {
                            *source = Some(*attacker);
                            if *attacker_kind == aftermine_sim::UnitKind::Piledriver
                                && impact_witnessed
                            {
                                *impact_sound = Some(SoundKind::PiledriverImpact);
                            }
                        }
                    }
                }
                Event::TurretFired {
                    kind,
                    tier,
                    turret_pos,
                    target_pos,
                    target,
                    ..
                } => {
                    // The kind comes from the event because the turret may
                    // have been destroyed the tick it fired; its shot still
                    // gets the right report and burst. Only a direct-fire
                    // defense fires this way; shells launch as their own
                    // event.
                    let Some(report) = crate::look::defense(*kind).and_then(|look| look.report)
                    else {
                        continue;
                    };
                    let splash = kind
                        .tier_stats(*tier)
                        .weapons
                        .iter()
                        .find_map(|w| w.splash)
                        .map(|s| s.to_num::<f32>());
                    if sees(self, *turret_pos) || sees(self, *target_pos) {
                        let at = if sees(self, *turret_pos) {
                            *turret_pos
                        } else {
                            *target_pos
                        };
                        self.sounds_pending
                            .push((report.sound, Some(world_vec(at))));
                    }
                    let surface = self.hit_surface(state, *target);
                    push_direct_report(
                        &mut self.fx,
                        report.shot(*tier),
                        visual_shot_origin(
                            world_vec(*turret_pos),
                            world_vec(*target_pos),
                            report.muzzle,
                        ),
                        world_vec(*target_pos),
                        splash,
                        state.current_tick(),
                        surface,
                    );
                }
                Event::BuildingCompleted {
                    building,
                    player,
                    kind,
                } if *player == self.human => {
                    // A completion at a nonzero tier is an upgrade
                    // finishing: its own cue, its own name.
                    let tier = state.building(*building).map_or(0, |b| b.tier);
                    if tier > 0 {
                        self.sounds_pending.push((SoundKind::UpgradeDone, None));
                        self.toast(format!(
                            "{} online",
                            crate::typography::entity_name(kind.tier_name(tier))
                        ));
                    } else {
                        self.sounds_pending.push((SoundKind::TrainDone, None));
                        self.toast(format!(
                            "{} online",
                            crate::typography::entity_name(kind.name())
                        ));
                    }
                }
                Event::BuildCancelled { player, refund, .. } if *player == self.human => {
                    self.toast(format!("Site salvaged (+{refund} scrap)"));
                }
                Event::UnitDied {
                    unit,
                    pos,
                    player,
                    kind,
                    grounded,
                } => {
                    if *player == self.human {
                        self.raise_alert(world_vec(*pos));
                    }
                    let prior = self
                        .fx_previous
                        .units
                        .iter()
                        .find(|(id, ..)| id == unit)
                        .map(|(_, body, at, ..)| (*body, *at));
                    let witnessed = *player == self.human
                        || sees(self, *pos)
                        || self.all_seeing()
                        || prior.is_some();
                    if !witnessed {
                        continue;
                    }
                    self.sounds_pending
                        .push((SoundKind::UnitDeath, Some(world_vec(*pos))));
                    let body = prior.map_or(
                        UnitBody::resting(
                            *kind,
                            *player,
                            self.facing.get(&unit.0).copied().unwrap_or(0.0),
                        ),
                        |(body, _)| body,
                    );
                    let airborne =
                        kind.stats().domain == aftermine_sim::stats::Domain::Air && !grounded;
                    self.fx.push(Effect {
                        kind: if airborne {
                            EffectKind::Falling {
                                at: world_vec(*pos),
                                body,
                                seed: unit.0,
                                impact_witnessed: false,
                                crash: state
                                    .aircraft_crashes()
                                    .iter()
                                    .find(|crash| crash.unit == *unit)
                                    .copied(),
                            }
                        } else {
                            EffectKind::Debris {
                                at: prior.map_or_else(|| world_vec(*pos), |(_, at)| at),
                                body,
                                seed: unit.0,
                            }
                        },
                        age: 0.0,
                    });
                }
                Event::AircraftImpacted { crash } => {
                    let witnessed = crash.player == self.human
                        || sees(self, crash.impact)
                        || self.all_seeing()
                        || self
                            .fx_previous
                            .visible_crash_contacts
                            .contains(&crash.unit);
                    self.restore_crash_effect(state, *crash, witnessed);
                    if state
                        .map()
                        .tile(chassis::grid::TilePos::containing(crash.impact))
                        .is_some_and(|tile| tile.terrain != aftermine_sim::map::Terrain::Pit)
                    {
                        self.sounds_pending
                            .push((SoundKind::BuildingBoom, Some(world_vec(crash.impact))));
                    }
                }
                Event::BuildingDestroyed {
                    building,
                    pos,
                    player,
                    ..
                } => {
                    if *player == self.human {
                        self.raise_alert(world_vec(*pos));
                    }
                    let detonated = events.iter().any(|event| {
                        matches!(event, Event::ChargeDetonated { building: charge, .. } if charge == building)
                    });
                    if !detonated {
                        self.sounds_pending
                            .push((SoundKind::BuildingBoom, Some(world_vec(*pos))));
                    }
                    let body = self
                        .fx_previous
                        .buildings
                        .iter()
                        .find(|(id, _, _)| id == building)
                        .and_then(|(_, body, _)| *body);
                    self.fx.push(Effect {
                        kind: body.map_or(
                            EffectKind::Puff {
                                at: world_vec(*pos),
                            },
                            |body| EffectKind::Collapse {
                                at: world_vec(*pos),
                                body,
                                seed: building.0,
                            },
                        ),
                        age: 0.0,
                    });
                    // A fading scorch decal; capped, oldest dropped first.
                    self.scorches.push((world_vec(*pos), 0.0, BUILDING_SCORCH));
                    if self.scorches.len() > 16 {
                        self.scorches.remove(0);
                    }
                }
                // The cradle was public, so every seat may learn its
                // machine has stood up where it stood.
                Event::UnitAssembled {
                    kind, player, pos, ..
                } if !self.all_seeing() => {
                    let name = crate::typography::entity_name(kind.name());
                    if *player == self.human {
                        self.sounds_pending.push((SoundKind::TrainDone, None));
                        self.toast(format!("{name} assembled"));
                    } else if state.hostile(self.human, *player) {
                        self.raise_cradle_alert(world_vec(*pos), *kind);
                        self.toast(format!("Enemy {name} has rolled out"));
                    }
                }
                // A bay is heard only where its host is seen.
                Event::DroneLaunched { host, .. } | Event::DroneDocked { host, .. } => {
                    let Some(host) = state.unit(*host) else {
                        continue;
                    };
                    if host.player == self.human
                        || self.all_seeing()
                        || state.vision(self.human).sees_unit(host)
                    {
                        let sound = if matches!(event, Event::DroneLaunched { .. }) {
                            SoundKind::DroneLaunch
                        } else {
                            SoundKind::DroneDock
                        };
                        self.sounds_pending.push((sound, Some(world_vec(host.pos))));
                    }
                }
                Event::UnitTrained { unit, player, .. } if *player == self.human => {
                    self.sounds_pending.push((SoundKind::TrainDone, None));
                    if let Some(u) = state.unit(*unit) {
                        self.fx.push(Effect {
                            kind: EffectKind::Ping {
                                at: world_vec(u.pos),
                                kind: PingKind::Spawn,
                                queued: false,
                            },
                            age: 0.0,
                        });
                    }
                }
                Event::ScrapDeposited { player, .. } if *player == self.human => {
                    self.sounds_pending.push((SoundKind::Deposit, None));
                }
                Event::CommandRejected { player, reason } if *player == self.human => {
                    let why: std::borrow::Cow<'static, str> = match reason {
                        aftermine_sim::command::RejectReason::MissingFoundrySupport => {
                            crate::building_actions::foundry_reach_reason().into()
                        }
                        aftermine_sim::command::RejectReason::NotEnoughScrap => {
                            "Not enough scrap".into()
                        }
                        aftermine_sim::command::RejectReason::QueueFull => "Queue is full".into(),
                        aftermine_sim::command::RejectReason::UnreachableGoal => {
                            "Can't reach that".into()
                        }
                        aftermine_sim::command::RejectReason::InvalidTarget => {
                            "Can't target that".into()
                        }
                        aftermine_sim::command::RejectReason::NotANode => {
                            "Nothing to mine there".into()
                        }
                        aftermine_sim::command::RejectReason::NotYourBuilding => {
                            "Not your structure".into()
                        }
                        aftermine_sim::command::RejectReason::CannotProduce => {
                            "That factory can't make those".into()
                        }
                        aftermine_sim::command::RejectReason::BadSite => "Can't build there".into(),
                        aftermine_sim::command::RejectReason::NoValidUnits => {
                            "Nothing selected can do that".into()
                        }
                        aftermine_sim::command::RejectReason::OutOfBounds => {
                            "Outside the map".into()
                        }
                        aftermine_sim::command::RejectReason::Eliminated => {
                            "You are eliminated".into()
                        }
                        aftermine_sim::command::RejectReason::MissingPrerequisite => {
                            "Needs its tech structure first".into()
                        }
                        aftermine_sim::command::RejectReason::NothingLoaded => {
                            "No warhead loaded".into()
                        }
                        aftermine_sim::command::RejectReason::CrewFull => {
                            "Cradle crew is full".into()
                        }
                    };
                    self.toast(why);
                    self.sounds_pending.push((SoundKind::Denied, None));
                }
                Event::ShellLaunched {
                    shooter,
                    player,
                    from,
                    to,
                    unit_pose,
                    ..
                } => {
                    // A firing machine faces its target. A defense's gun
                    // already bears on it in the simulation.
                    let d = world_vec(*to) - world_vec(*from);
                    if let aftermine_sim::Target::Unit(uid) = shooter
                        && d.length_squared() > 1e-6
                    {
                        let angle = d.y.atan2(d.x) + std::f32::consts::FRAC_PI_2;
                        self.aim_units.insert(uid.0, (angle, self.fx_clock));
                    }
                    // No effect spawned: in-flight shells render from
                    // `state.shells()` directly, aged by sim ticks, so pause,
                    // replay loads, and speed changes stay consistent.
                    // Sound follows sight, except that a hostile launch
                    // whose muzzle is fogged plays anchored at its impact:
                    // the same information the sim's incoming-shell sense
                    // grants (impact tile visible), without revealing the
                    // gun.
                    if let Some((sound, anchor)) = shell_launch_audio(
                        *shooter,
                        unit_pose.as_ref().map(|pose| pose.kind),
                        AllegianceCue::of(state, self.human, *player),
                        sees(self, *from),
                        sees(self, *to),
                    ) {
                        let at = match anchor {
                            ShellSoundAnchor::Muzzle => *from,
                            ShellSoundAnchor::Impact => *to,
                        };
                        self.sounds_pending.push((sound, Some(world_vec(at))));
                    }
                }
                Event::ShellIntercepted {
                    defender,
                    defender_player,
                    defender_pos,
                    player,
                    shooter,
                    kind,
                    at,
                    impact,
                    arrival,
                } => {
                    let left = remaining.get_mut(defender).expect("interception counted");
                    *left -= 1;
                    let charges = state
                        .unit(*defender)
                        .and_then(|unit| unit.point_defense)
                        .map_or(0, |held| held.charges);
                    let slot = charges.saturating_add(*left).min(2);
                    let from = (self.all_seeing()
                        || *defender_player == self.human
                        || sees(self, *defender_pos))
                    .then(|| {
                        let rotation = match self
                            .hit_surface(state, Some(aftermine_sim::Target::Unit(*defender)))
                        {
                            Some(HitSurface::Unit(hit)) => hit.body.rotation,
                            _ => 0.0,
                        };
                        mantle_emitter(world_vec(*defender_pos), rotation, slot)
                    });
                    let impact_witnessed =
                        self.all_seeing() || !state.hostile(self.human, *player) || sees(self, *at);
                    if from.is_none() && !impact_witnessed {
                        continue;
                    }
                    let flight = self
                        .fx_previous
                        .shells
                        .iter()
                        .position(|shell| {
                            shell.shooter == *shooter
                                && shell.kind == *kind
                                && shell.player == *player
                                && shell.impact == *impact
                                && shell.arrival == *arrival
                        })
                        .map(|index| {
                            let shell = &self.fx_previous.shells[index];
                            let release = self
                                .projectile_releases
                                .flight(&self.fx_previous.shells, index);
                            let ticks =
                                shell.arrival.saturating_sub(shell.launched_at).max(1) as f32;
                            InterceptFlight {
                                launch: world_vec(shell.launch),
                                impact: world_vec(shell.impact),
                                shooter: shell.shooter,
                                kind: shell.kind,
                                pose: release.and_then(|flight| flight.pose).or_else(|| {
                                    self.projectile_releases.bomber_pose(
                                        state,
                                        &self.fx_previous.shells,
                                        index,
                                    )
                                }),
                                surface: self.payload_surface(
                                    state,
                                    release.and_then(|flight| flight.target),
                                    shell.player,
                                    world_vec(shell.impact),
                                    shell.targets,
                                ),
                                progress: (state
                                    .current_tick()
                                    .saturating_sub(1)
                                    .saturating_sub(shell.launched_at)
                                    as f32
                                    / ticks)
                                    .clamp(0.0, 1.0),
                                ticks,
                            }
                        });
                    // Like gunfire, the zap is heard when either end is seen:
                    // at the emitter when it is, else where the shell
                    // dissolves. The shell makes no sound of its own.
                    if from.is_some() || impact_witnessed {
                        let anchor = if from.is_some() { defender_pos } else { at };
                        self.sounds_pending
                            .push((SoundKind::InterceptZap, Some(world_vec(*anchor))));
                    }
                    self.fx.push(Effect {
                        kind: EffectKind::Intercept {
                            from,
                            at: world_vec(*at),
                            flight,
                            impact_witnessed,
                            completed_tick: state.current_tick(),
                        },
                        age: 0.0,
                    });
                }
                Event::ChargeDetonated { at, .. } => {
                    // A mine going off is loud and unmistakable whoever
                    // owned it.
                    self.fx.push(Effect {
                        kind: EffectKind::Burst {
                            at: world_vec(*at),
                            radius: aftermine_sim::stats::CHARGE_BLAST_RADIUS.to_num::<f32>(),
                        },
                        age: 0.0,
                    });
                    self.sounds_pending
                        .push((SoundKind::DemolitionBoom, Some(world_vec(*at))));
                }
                Event::ShellLanded {
                    player,
                    targets,
                    at,
                    splash,
                } => {
                    // The event names no victim (a shell in flight chooses
                    // nothing), so ask the post-tick world whether the
                    // blast reached anything of ours. Survivors alert here;
                    // the dead alert through their own events.
                    let impact_sound = self.audio_timeline.landed(*player, *at);
                    let reach = splash.map_or(1.0, |r| r.to_num::<f32>().max(1.0));
                    let world = world_vec(*at);
                    let hostile_shell = state.hostile(self.human, *player);
                    // Parenthesized deliberately: && binds tighter than ||,
                    // and without the grouping the building branch would
                    // alert on the player's own artillery.
                    let own_hurt = hostile_shell
                        && (state
                            .units()
                            .iter()
                            .filter(|u| u.player == self.human && targets.covers(u.domain()))
                            .any(|u| world_vec(u.pos).distance(world) <= reach)
                            || (targets.covers(aftermine_sim::stats::Domain::Ground)
                                && state
                                    .buildings()
                                    .iter()
                                    .filter(|b| b.player == self.human)
                                    .any(|b| {
                                        let c = world_vec(b.center());
                                        c.distance(world) <= reach + 1.5
                                    })));
                    if own_hurt {
                        self.raise_alert(world);
                    }
                    self.sounds_pending
                        .push((impact_sound, Some(world_vec(*at))));
                    let arrived = self
                        .fx_previous
                        .shells
                        .iter()
                        .position(|shell| {
                            shell.player == *player
                                && shell.impact == *at
                                && shell.targets == *targets
                                && shell.splash == *splash
                                && shell.arrival < state.current_tick()
                        })
                        .map(|index| {
                            let target = self
                                .projectile_releases
                                .flight(&self.fx_previous.shells, index)
                                .and_then(|flight| flight.target);
                            let shell = self.fx_previous.shells.remove(index);
                            (shell, target)
                        });
                    let payload = arrived
                        .as_ref()
                        .map_or(aftermine_sim::ProjectileKind::Shell, |(shell, _)| {
                            shell.kind
                        });
                    let from = arrived
                        .as_ref()
                        .map_or(world, |(shell, _)| world_vec(shell.launch));
                    let surface = self.payload_surface(
                        state,
                        arrived.and_then(|(_, target)| target),
                        *player,
                        world,
                        *targets,
                    );
                    self.fx.push(Effect {
                        kind: EffectKind::Impact {
                            at: world_vec(*at),
                            radius: splash.map_or(0.8, |r| r.to_num::<f32>()),
                            payload,
                            from,
                            surface,
                            completed_tick: state.current_tick(),
                        },
                        age: 0.0,
                    });
                }
                // A spectator inherits a nominal seat but owns no
                // orders; a bot's private stall feedback must not read
                // as "your order failed" in the replay viewer.
                Event::OrderStalled {
                    unit,
                    player,
                    pos,
                    reason,
                } if *player == self.human && !self.spectating() => {
                    let scout = state
                        .unit(*unit)
                        .is_some_and(|unit| unit.kind.stats().scouts);
                    // Own-state facts only: a stall reason must never reveal
                    // what fog hides.
                    let text = match reason {
                        aftermine_sim::StallReason::NoRoute => "Can't reach that",
                        aftermine_sim::StallReason::NoFiringPosition => {
                            "No ground to fire from there"
                        }
                        aftermine_sim::StallReason::InsufficientScrap => "Out of scrap",
                        aftermine_sim::StallReason::GroundTaken => {
                            "That ground was taken before the founder arrived"
                        }
                        aftermine_sim::StallReason::CrewFull => "Cradle crew is full",
                        aftermine_sim::StallReason::TransportFull => "The transport is full",
                        aftermine_sim::StallReason::NoOpenGround => {
                            "No open ground to unload there"
                        }
                        aftermine_sim::StallReason::DangerHold if scout => {
                            "Scout waiting for a safe way to see"
                        }
                        aftermine_sim::StallReason::DangerHold => {
                            "Worker waiting for a safe route home"
                        }
                        // The scout still has its tile in sight, so the
                        // order did its job; the stop needs no feedback.
                        aftermine_sim::StallReason::SafeVantage => continue,
                        aftermine_sim::StallReason::NoSafeVantage => {
                            "Scout found no safe way to see there"
                        }
                    };
                    self.toast(text);
                    self.fx.push(Effect {
                        kind: EffectKind::Ping {
                            at: world_vec(*pos),
                            kind: PingKind::Attack,
                            queued: false,
                        },
                        age: 0.0,
                    });
                }
                Event::WarheadLaunched { player, from, .. } => {
                    if self.all_seeing() || !state.hostile(self.human, *player) || sees(self, *from)
                    {
                        let from = world_vec(*from);
                        self.sounds_pending
                            .push((SoundKind::SpireLaunch, Some(from)));
                        self.fx.push(Effect {
                            kind: EffectKind::Puff { at: from },
                            age: 0.0,
                        });
                    }
                }
                Event::WarheadIntercepted {
                    warhead,
                    player,
                    battery,
                    battery_player,
                    pod,
                    at,
                    ..
                } => {
                    let early = self
                        .prelaunches
                        .iter()
                        .position(|launch| launch.warhead == *warhead)
                        .map(|index| self.prelaunches.remove(index));
                    if *player == self.human && !self.all_seeing() {
                        self.toast("Warhead intercepted");
                    }
                    let known = self
                        .fx_previous
                        .warheads
                        .iter()
                        .find(|known| known.id == *warhead)
                        .copied();
                    let line = known
                        .and_then(|known| known.line)
                        .or(early.map(|launch| launch.line));
                    let stopped = state.current_tick().saturating_sub(1) as f32;
                    let (path, contact, contact_tick) = if let Some(launch) = early {
                        (Some(launch.path), launch.contact, launch.stop)
                    } else {
                        // No forecast: the warhead flies on a moment to
                        // meet an interceptor leaving now.
                        let meet = stopped + INTERCEPT_LATE_TICKS;
                        let flight = line.map(crate::render::sky::Flight::of);
                        let contact = flight.map_or(world_vec(*at), |flight| {
                            flight.nose(meet)
                                + flight.heading() * crate::render::sky::FUSE_GAP / 2.0
                        });
                        let altitude = flight.map_or(0.0, |flight| flight.altitude(meet));
                        let path = state.building(*battery).map(|b| {
                            let from = crate::render::lattice_pod_pivot(b, *pod);
                            let path = crate::render::sky::InterceptorPath::to(
                                from, contact, stopped, meet, altitude,
                            );
                            let d = path.heading();
                            let angle = d.y.atan2(d.x) + std::f32::consts::FRAC_PI_2;
                            self.pod_shots
                                .insert((battery.0, *pod), (angle, self.fx_clock));
                            let hostile = state.hostile(self.human, *battery_player);
                            if crate::render::missile_seen_by(self, state, hostile, from) {
                                self.sounds_pending
                                    .push((SoundKind::InterceptorLaunch, Some(from)));
                            }
                            path
                        });
                        (path, contact, meet)
                    };
                    self.fx.push(Effect {
                        kind: EffectKind::Interception {
                            line,
                            warhead_player: known.and_then(|known| known.player).or_else(|| {
                                (!state.hostile(self.human, *player)).then_some(*player)
                            }),
                            warhead_hostile: state.hostile(self.human, *player),
                            path,
                            battery_player: *battery_player,
                            contact,
                            contact_tick,
                            seed: warhead.0,
                            completed_tick: state.current_tick(),
                            burst_heard: false,
                        },
                        age: 0.0,
                    });
                }
                Event::WarheadLanded {
                    warhead,
                    player,
                    at,
                } => {
                    let witnessed =
                        self.all_seeing() || !state.hostile(self.human, *player) || sees(self, *at);
                    let at = world_vec(*at);
                    self.sounds_pending
                        .push((SoundKind::WarheadDetonation, Some(at)));
                    self.fx.push(Effect {
                        kind: EffectKind::WarheadBurst {
                            at,
                            seed: warhead.0,
                            witnessed,
                            completed_tick: state.current_tick(),
                        },
                        age: 0.0,
                    });
                    if !witnessed {
                        continue;
                    }
                    // The crater a seat saw form, as wide as full damage.
                    self.scorches.push((
                        at,
                        0.0,
                        2.0 * aftermine_sim::stats::WARHEAD.full_radius.to_num::<f32>(),
                    ));
                    if self.scorches.len() > 16 {
                        self.scorches.remove(0);
                    }
                    if !crate::render::reduced_motion() {
                        let near = 1.0 - at.distance(self.camera.center) / STRIKE_SHAKE_REACH;
                        if near > 0.0 {
                            self.camera.kick(STRIKE_SHAKE * near * near);
                        }
                    }
                }
                Event::GameOver { result } => {
                    let won = matches!(
                        result,
                        aftermine_sim::GameResult::Victory { team }
                            if *team == state.player(self.human).team
                    );
                    self.sounds_pending.push((
                        if won {
                            SoundKind::Victory
                        } else {
                            SoundKind::Defeat
                        },
                        None,
                    ));
                }
                _ => {}
            }
        }
        self.warn_of_warheads(state);
        self.warn_of_cradles(state);
        self.launch_interceptors(state);
    }

    /// Starts each forecast stop its lead ahead: the pod swings onto the
    /// interceptor's path and opens its clamp, and the interceptor climbs to
    /// meet the warhead nose to nose where the simulation stops it.
    /// Presentation only: whoever sees it is decided where it is drawn.
    fn launch_interceptors(&mut self, state: &State) {
        let forecast = state.interception_forecast();
        let present = state.current_tick().saturating_sub(1) as f32;
        // A stop whose forecast lapses before its pod fires is called off;
        // one already in the air flies until the simulation reports it.
        self.prelaunches.retain(|launch| {
            forecast.iter().any(|stop| stop.warhead == launch.warhead)
                || (launch.launched(present) && present <= launch.stop + 1.0)
        });
        for stop in forecast {
            let due = stop.tick as f32;
            let lead = due - present;
            if lead > INTERCEPT_LEAD_TICKS
                || self
                    .prelaunches
                    .iter()
                    .any(|launch| launch.warhead == stop.warhead)
            {
                continue;
            }
            let (Some(battery), Some(warhead)) = (
                state.building(stop.battery),
                state.warheads().iter().find(|w| w.id == stop.warhead),
            ) else {
                continue;
            };
            let line = aftermine_sim::strike::WarheadLine {
                launch: warhead.launch,
                impact: warhead.impact,
                arrival: warhead.arrival,
            };
            let flight = crate::render::sky::Flight::of(line);
            let contact = flight.nose(due) + flight.heading() * crate::render::sky::FUSE_GAP / 2.0;
            let climb = (lead - INTERCEPT_CLAMP_TICKS)
                .clamp(INTERCEPT_MIN_FLIGHT_TICKS, INTERCEPT_FLIGHT_TICKS);
            let path = crate::render::sky::InterceptorPath::to(
                crate::render::lattice_pod_pivot(battery, stop.pod),
                contact,
                (due - climb).max(present),
                due,
                flight.altitude(due),
            );
            self.prelaunches.push(Prelaunch {
                warhead: stop.warhead,
                battery: battery.id,
                pod: stop.pod,
                battery_player: battery.player,
                line,
                created: present,
                stop: due,
                path,
                contact,
                heard: false,
            });
        }
        // Each interceptor is heard, and its pod's shot held, as it leaves.
        let leaving: Vec<Prelaunch> = self
            .prelaunches
            .iter_mut()
            .filter(|launch| !launch.heard && launch.launched(present))
            .map(|launch| {
                launch.heard = true;
                *launch
            })
            .collect();
        for launch in leaving {
            let d = launch.path.heading();
            let angle = d.y.atan2(d.x) + std::f32::consts::FRAC_PI_2;
            self.pod_shots
                .insert((launch.battery.0, launch.pod), (angle, self.fx_clock));
            let hostile = state.hostile(self.human, launch.battery_player);
            if crate::render::missile_seen_by(self, state, hostile, launch.path.from) {
                self.sounds_pending
                    .push((SoundKind::InterceptorLaunch, Some(launch.path.from)));
            }
        }
    }

    /// Announces each hostile warhead once, when this seat first knows of
    /// it: an alert where it will land once tracked, else where it flies.
    fn warn_of_warheads(&mut self, state: &State) {
        if self.all_seeing() {
            return;
        }
        for warhead in state.known_warheads(self.human) {
            if warhead.hostile && self.warned_warheads.insert(warhead.id) {
                let at = warhead.line.map_or(warhead.position, |line| line.impact);
                self.raise_warhead_alert(world_vec(at));
                self.toast("Incoming warhead");
            }
        }
        self.warned_warheads
            .retain(|id| state.warheads().iter().any(|w| w.id == *id));
    }

    /// Announces each hostile assembly cradle once, the moment every seat
    /// learns of it, and forgets cradles every seat has forgotten.
    fn warn_of_cradles(&mut self, state: &State) {
        if self.all_seeing() {
            return;
        }
        let cradles: Vec<_> =
            aftermine_sim::observation::hostile_public_buildings(state, self.human)
                .into_iter()
                .filter(|public| public.kind.assembles().is_some())
                .collect();
        for cradle in &cradles {
            if self
                .warned_cradles
                .insert((cradle.owner.0, cradle.anchor.x, cradle.anchor.y))
            {
                let (w, h) = cradle.kind.size();
                let centre = macroquad::prelude::vec2(
                    cradle.anchor.x as f32 + w as f32 / 2.0,
                    cradle.anchor.y as f32 + h as f32 / 2.0,
                );
                if let Some(machine) = cradle.kind.assembles() {
                    self.raise_cradle_alert(centre, machine);
                    self.toast(format!(
                        "Enemy {} assembling",
                        crate::typography::entity_name(machine.name())
                    ));
                }
            }
        }
        self.warned_cradles.retain(|&(owner, x, y)| {
            cradles
                .iter()
                .any(|c| (c.owner.0, c.anchor.x, c.anchor.y) == (owner, x, y))
        });
    }
}

#[cfg(test)]
mod tests;
