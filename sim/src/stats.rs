//! Unit and building kinds, their stats, and global tuning constants.
//!
//! Unit and building stats are `const` tables here, alongside the global
//! tuning constants. Changing a number changes sim behavior, so expect
//! regression hashes to move (see AGENTS.md).
//!
//! Combat is a weapons matrix: every kind carries a (possibly empty) list
//! of weapons, each declaring which movement domains it can hit, whether it
//! splashes, and whether it fires indirect (arcing over terrain cover).
//! Units also carry a movement domain: ground units path and collide on the
//! terrain grid, air units fly above it.

use chassis::fx::Fx;
use serde::{Deserialize, Serialize};

chassis::listed_enum! {
    /// Every trainable unit type.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum UnitKind {
        /// Gathers scrap from nodes and hauls it to a Foundry.
        Harvester,
        /// The line combat unit: short-ranged, sturdy, expendable.
        Sentinel,
        /// Fast, cheap, fragile raider: a contact-range shredder that eats
        /// harvest lines and dies to anything that fights back in time.
        Scuttler,
        /// Slow long-range artillery: outranges everything it can see,
        /// melts to anything that reaches it.
        Lancer,
        /// Heavy siege piece: arcing splash shells that reach beyond its own
        /// eyes — someone else must hold sight on the target.
        Bombard,
        /// Anti-air crawler: tanky flak platform, blind to ground.
        Flakhound,
        /// Ground-attack flyer: slow, heavy strikes, no answer to air.
        Buzzard,
        /// Air-superiority flyer: sees far, hits only other flyers.
        Talon,
        /// Tier-two line brawler: an upgunned sentinel-class hull.
        Warden,
        /// Tier-two super-harvester: digs faster, hauls triple, and builds
        /// at twice the pace.
        Excavator,
        /// Scout flyer: fast, unarmed, far-sighted.
        Kestrel,
        /// Strategic bomber: one enormous bomb per pass, flown on a
        /// committed attack run — it cannot stop and strafe.
        Condor,
        /// Tier-three assault walker: slow and heavily armored.
        Breaker,
        /// Tier-three rocket battery: extreme-reach indirect saturation with
        /// a blind ring at its feet.
        Avalanche,
        /// Air transport: an unarmed lifter with a four-point sling rack.
        /// Cargo rides sealed — it fights nothing, sees nothing, and dies
        /// with the airframe.
        Skyhook,
        /// A walking demolition charge: presses to its ordered target and
        /// detonates — enormous against structures, modest splash against
        /// machines, always fatal to itself.
        Sapper,
        /// Unarmed tracked point-defense carrier: shoots down hostile projectiles
        /// aimed near it, while gunfire passes through.
        Mantle,
        /// Tier-three anti-heavy gun: braces, then fires one enormous
        /// instant slug that breaks heavy machines and is wasted on light
        /// ones.
        Piledriver,
        /// Ground scout: fast, unarmed, fragile, and far-sighted; it spots
        /// buried charges like a Kestrel.
        Surveyor,
    }
}

chassis::listed_enum! {
    /// Every building type.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum BuildingKind {
        /// HQ, unit factory, and scrap drop-off. Lose all of them, lose the game.
        Foundry,
        /// Static defense: fires on its own at anything in range and line of
        /// sight.
        Turret,
        /// Second factory: trains the advanced roster and gates further tech.
        Fabricator,
        /// Anti-air emplacement: flak bursts that only ever look up.
        FlakTurret,
        /// Artillery emplacement: arcing splash shells beyond its own sight;
        /// needs a spotter at full reach. Upgrades to the longer-ranged
        /// Citadel.
        Bastion,
        /// Radar: a ring of true sight, and a wider ring of blips (contacts
        /// without identity whose contact tracks attacks may target).
        Array,
        /// Grinds ambient debris into a scrap trickle, so a match can outlive
        /// its scrap patches.
        Reclaimer,
        /// Field workshop: an unarmed aura that repairs own wounded machines
        /// and completed structures inside its ring, billed per hp from the
        /// owner's bank at repair pricing.
        RepairBay,
        /// A restored strip-mining machine, rebuilt only on a map-authored
        /// derelict frame. It provides durable income, raised by a nearby own
        /// Foundry. The frame survives destruction, so the site remains
        /// contestable.
        Extractor,
        /// Air production hall: every flyer trains here.
        Airworks,
        /// The tier-three works: trains the heaviest machines, gates the
        /// deepest upgrades, and smelts nearby wreck into scrap.
        Crucible,
        /// A cheap standing wall segment: blocks ground movement and
        /// nothing else.
        Barricade,
        /// A buried demolition charge and the game's only stealth. Invisible
        /// to enemies until a scout comes close or an Array's detection ring
        /// covers it; detonates under hostile ground machines.
        ScuttleCharge,
    }
}

/// A movement medium. Ground units path and collide on the terrain grid;
/// air units fly over everything but peaks and collide only with each
/// other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    /// Bound to passable terrain.
    Ground,
    /// Above the grid: only peaks block it.
    Air,
}

/// Which movement domains a weapon can hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DomainMask {
    /// Can hit ground units and buildings.
    pub ground: bool,
    /// Can hit air units.
    pub air: bool,
}

impl DomainMask {
    /// Hits ground only.
    pub const GROUND: DomainMask = DomainMask {
        ground: true,
        air: false,
    };
    /// Hits air only.
    pub const AIR: DomainMask = DomainMask {
        ground: false,
        air: true,
    };
    /// Hits everything.
    pub const BOTH: DomainMask = DomainMask {
        ground: true,
        air: true,
    };

    /// Whether this mask covers the given domain.
    pub const fn covers(self, domain: Domain) -> bool {
        match domain {
            Domain::Ground => self.ground,
            Domain::Air => self.air,
        }
    }
}

/// One weapon: its numbers and its firing rules.
#[derive(Debug, Clone, Copy)]
pub struct WeaponStats {
    /// Hit points removed per hit.
    pub damage: u32,
    /// Maximum engagement distance, in tiles (center to closest point).
    pub range: Fx,
    /// Closest engagement distance. Targets inside this radius cannot be
    /// selected, giving long-range emplacements an explicit close-pressure
    /// counter.
    pub minimum_range: Fx,
    /// Ticks between hits.
    pub cooldown_ticks: u32,
    /// Which movement domains this weapon can hit. Buildings count as
    /// ground.
    pub targets: DomainMask,
    /// Area damage radius around the impact point. Splash hits enemy
    /// *units* in the weapon's target domains; buildings only ever take
    /// the direct hit.
    pub splash: Option<Fx>,
    /// Indirect fire arcs over terrain: the line-of-sight trace that lets
    /// rock block direct shots is skipped.
    pub indirect: bool,
    /// The shot is a real projectile: a Shell entity travels to a fixed
    /// fire-time aim point and resolves on arrival. Artillery may lead an
    /// existing path before launch, but the shell is never guided and a
    /// later course change can dodge it. Hitscan when `None`.
    pub projectile: Option<ProjectileStats>,
}

/// What a projectile weapon launches and how fast it flies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectileStats {
    /// The payload's physical identity, which only presentation reads.
    pub payload: ProjectileKind,
    /// Flight speed in tiles per tick.
    pub speed: Fx,
}

/// Physical identity of an in-flight payload, independent of its damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectileKind {
    /// An artillery shell.
    Shell,
    /// An Avalanche missile.
    Missile,
    /// An air-dropped bomb.
    Bomb,
}

/// Gathering parameters for units that can harvest.
#[derive(Debug, Clone, Copy)]
pub struct HarvestStats {
    /// Scrap carried before a delivery trip is forced.
    pub capacity: u32,
    /// Ticks of standing at a node to extract one scrap.
    pub ticks_per_scrap: u32,
}

/// Static parameters of a unit kind.
#[derive(Debug, Clone, Copy)]
pub struct UnitStats {
    /// Tool reach beyond the chassis for direct-contact weapons.
    pub contact_reach: Option<Fx>,
    /// Hit points at spawn.
    pub max_hp: u32,
    /// Movement speed in tiles per tick.
    pub speed: Fx,
    /// Collision radius in tiles (separation only; units never hard-block).
    pub radius: Fx,
    /// Scrap price.
    pub cost: u32,
    /// Factory queue time.
    pub train_ticks: u32,
    /// The medium this unit moves through.
    pub domain: Domain,
    /// Every weapon this unit carries (empty for pacifists). The first
    /// weapon that can cover an ordered target engages it; weapons that
    /// cannot pick their own nearest hostile in their domain.
    pub weapons: &'static [WeaponStats],
    /// Idle units acquire targets inside this radius on their own. Zero
    /// for units with no weapons.
    pub aggro_range: Fx,
    /// Present iff the unit is a worker: it gathers, builds and welds.
    pub harvest: Option<HarvestStats>,
    /// Fog-of-war reveal radius, in tiles.
    pub vision: i32,
    /// Building kinds the owner must have completed before training this
    /// unit; identical for humans and bots. Empty means the producer alone
    /// decides.
    pub requires: &'static [BuildingKind],
    /// Construction work applied per adjacent tick (1 for everyone but
    /// the Excavator).
    pub build_rate: u32,
    /// The machine is its own warhead: an ordered attack ends with the unit
    /// pressing to contact and detonating. Grants attack legality without
    /// weapons.
    pub demolition: Option<DemolitionStats>,
    /// Room this machine occupies aboard a transport. 0 means it can
    /// never be carried — every flyer, and the transport itself.
    pub transport_size: u8,
    /// Total cargo room this machine offers as a carrier. 0 for
    /// everything that is not a transport.
    pub transport_capacity: u8,
    /// Maximum compass steps (of 256) a committed airframe turns per tick.
    /// 0 selects ordinary movement or [`Self::cruise_turn_rate`]. A
    /// nonzero rate makes the unit fly heading-first: it steers on a
    /// bounded arc, attacks on passes, and releases bombs only into its
    /// forward cone.
    pub turn_rate: u8,
    /// Travel and fixed-gun traverse, in compass steps per tick, for
    /// aircraft that bank in cruise and hover at rest. 0 for everything
    /// else, including committed bombers and rotorcraft.
    pub cruise_turn_rate: u8,
    /// Independent turret traverse in compass steps per tick. 0 when the
    /// gun turns with the hull.
    pub turret_turn_rate: u8,
    /// Hull turn speed in compass steps per tick for a ground unit that
    /// does not scale its turning with its speed.
    pub hull_turn_rate: Option<u8>,
    /// Recoil spades the gun must deploy, stationary and aligned, before
    /// it fires.
    pub brace: Option<BraceStats>,
    /// Which enemy machine it picks when it chooses its own target.
    pub target_priority: TargetPriority,
    /// The ground blast a large airframe makes when it crashes.
    pub crash: Option<CrashProfile>,
    /// Reveals hostile Scuttle Charges within
    /// [`CHARGE_SCOUT_DETECT_RADIUS`] tiles.
    pub charge_detection: bool,
    /// Charges that shoot down hostile projectiles aimed near the machine.
    pub point_defense: Option<PointDefenseStats>,
}

/// How a unit picks an enemy machine when nobody ordered a target. It ranks
/// only machines it could engage now, so a unit with nothing better in reach
/// still answers what it can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetPriority {
    /// The nearest.
    Nearest,
    /// The one with the most full health, then the nearest: a gun whose one
    /// slug a light machine would mostly waste.
    Heaviest,
}

/// Recoil spades a gun deploys before it may fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BraceStats {
    /// Stationary, aligned ticks to deploy the spades fully; the gun
    /// fires only when they are.
    pub deploy_ticks: u8,
    /// Ticks after a shot before the spades may start retracting.
    pub recoil_ticks: u32,
    /// Deployment lost per tick while retracting.
    pub retract_per_tick: u8,
}

/// Point defense: each charge destroys one hostile projectile in flight
/// whose fixed aim point and current flight position both lie within
/// `radius`. Hitscan fire is untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointDefenseStats {
    /// Charges held when full.
    pub charges: u8,
    /// Reach from the machine's center, applied to both the aim point and
    /// the projectile's flight position.
    pub radius: Fx,
    /// Ticks to restore one spent charge; charges return one at a time.
    pub recharge_ticks: u16,
}

/// A machine's own warhead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DemolitionStats {
    /// How close the machine presses to its target before the charge
    /// fires, measured to the target's closest point.
    pub contact_range: Fx,
    /// Damage dealt directly to the building target.
    pub structure_damage: u32,
    /// Damage dealt to every hostile ground machine in the blast ring.
    pub splash_damage: u32,
    /// The blast ring's radius.
    pub blast_radius: Fx,
}

impl UnitStats {
    /// The radius of the tightest circle a turn-limited flier can fly:
    /// `speed * 256 / (2*pi*turn_rate)`, with `256/(2*pi)` as the literal
    /// `40.75`. Only meaningful when `turn_rate > 0`.
    pub fn turn_radius(&self) -> Fx {
        debug_assert!(self.turn_rate > 0);
        self.speed * const { Fx::lit("40.75") } / Fx::from_num(i64::from(self.turn_rate))
    }

    /// The ring inside which a turn-limited flier accepts a waypoint or
    /// goal: [`Self::turn_radius`] plus [`BOMBER_ACCEPT_SLACK`]. Anything
    /// smaller is an orbit the aircraft can fly forever without crossing the
    /// ring. Only meaningful when `turn_rate > 0`.
    pub fn turn_acceptance(&self) -> Fx {
        self.turn_radius() + BOMBER_ACCEPT_SLACK
    }
}

impl UnitStats {
    /// Whether this kind can set down and park. Only an airframe that turns
    /// as it flies lands; the rest hover.
    pub const fn can_land(&self) -> bool {
        self.turn_rate > 0
    }

    /// Whether this kind carries any weapon at all.
    pub const fn can_fight(&self) -> bool {
        !self.weapons.is_empty() || self.demolition.is_some()
    }

    /// Whether any weapon covers the given domain.
    pub fn can_target(&self, domain: Domain) -> bool {
        self.weapons.iter().any(|w| w.targets.covers(domain))
    }

    /// The longest reach of any weapon covering the given domain.
    pub fn max_range_vs(&self, domain: Domain) -> Option<Fx> {
        self.weapons
            .iter()
            .filter(|w| w.targets.covers(domain))
            .map(|w| w.range)
            .max()
    }
}

/// Static parameters of a building kind.
#[derive(Debug, Clone, Copy)]
pub struct BuildingStats {
    /// Hit points when fully built.
    pub max_hp: u32,
    /// Fog-of-war reveal radius, in tiles (from each footprint tile).
    pub vision: i32,
    /// What this building can train. Empty for non-producers.
    pub produces: &'static [UnitKind],
    /// Every weapon this building fires on its own; buildings have no aggro
    /// range separate from weapon range. Empty for unarmed buildings.
    pub weapons: &'static [WeaponStats],
    /// Price, build time, and prerequisites for placing this kind. Every
    /// current kind has one; for an upgrade tier it prices the upgrade.
    pub construction: Option<ConstructionStats>,
}

impl BuildingKind {
    /// Footprint in tiles (width, height), anchored top-left. Every tier
    /// of a kind shares it.
    pub const fn size(self) -> (i32, i32) {
        match self {
            BuildingKind::Turret
            | BuildingKind::FlakTurret
            | BuildingKind::Array
            | BuildingKind::Reclaimer
            | BuildingKind::Barricade
            | BuildingKind::ScuttleCharge => (1, 1),
            BuildingKind::Foundry
            | BuildingKind::Fabricator
            | BuildingKind::Bastion
            | BuildingKind::RepairBay
            | BuildingKind::Airworks
            | BuildingKind::Crucible
            | BuildingKind::Extractor => (2, 2),
        }
    }

    /// The tier ladder for this kind: index by a building's `tier`.
    /// Kinds without upgrades ladder alone at tier zero.
    pub const fn tiers(self) -> &'static [&'static BuildingStats] {
        match self {
            BuildingKind::Turret => &[&TURRET, &HEAVY_TURRET, &BULWARK],
            BuildingKind::FlakTurret => &[&FLAK_TURRET, &BURST_FLAK],
            BuildingKind::Reclaimer => &[&RECLAIMER, &REFINERY],
            BuildingKind::Array => &[&ARRAY, &DEEP_ARRAY],
            BuildingKind::Foundry => &[&FOUNDRY],
            BuildingKind::Fabricator => &[&FABRICATOR],
            BuildingKind::Bastion => &[&BASTION, &CITADEL],
            BuildingKind::RepairBay => &[&REPAIR_BAY],
            BuildingKind::Extractor => &[&EXTRACTOR, &DEEP_BORE],
            BuildingKind::Airworks => &[&AIRWORKS],
            BuildingKind::Crucible => &[&CRUCIBLE],
            BuildingKind::Barricade => &[&BARRICADE],
            BuildingKind::ScuttleCharge => &[&SCUTTLE_CHARGE],
        }
    }

    /// Stats at `tier`, clamped to the ladder's top so a forged tier
    /// can never index past the table (the validator refuses it first).
    pub fn tier_stats(self, tier: u8) -> &'static BuildingStats {
        let tiers = self.tiers();
        tiers[(tier as usize).min(tiers.len() - 1)]
    }

    /// Scrap paid to build this kind and upgrade it through `tier`.
    pub fn invested_cost(self, tier: u8) -> u32 {
        self.tiers()
            .iter()
            .take(usize::from(tier) + 1)
            .filter_map(|stats| stats.construction.as_ref())
            .map(|construction| construction.cost)
            .sum()
    }

    /// The upgrade that would lift a building at `tier` one rung, if
    /// the ladder continues: the next tier's construction row.
    pub fn upgrade_from(self, tier: u8) -> Option<&'static ConstructionStats> {
        self.tiers()
            .get(tier as usize + 1)
            .and_then(|stats| stats.construction.as_ref())
    }

    /// A display name per tier, so upgraded works read as what they are.
    pub const fn tier_name(self, tier: u8) -> &'static str {
        match (self, tier) {
            (BuildingKind::Turret, 1) => "heavy turret",
            (BuildingKind::Turret, 2) => "bulwark",
            (BuildingKind::FlakTurret, 1) => "burst flak",
            (BuildingKind::Reclaimer, 1) => "refinery",
            (BuildingKind::Array, 1) => "deep array",
            (BuildingKind::Extractor, 1) => "deep bore",
            (BuildingKind::Bastion, 1) => "citadel",
            _ => self.name(),
        }
    }

    /// Whether lifting a building of this kind off `tier` also needs a
    /// completed own Foundry within [`EXTRACTOR_SUPPORT_RADIUS`] when the
    /// upgrade starts. Each extra Deep Bore therefore means an expansion.
    pub const fn upgrade_needs_foundry_support(self, tier: u8) -> bool {
        matches!(self, BuildingKind::Extractor) && tier == 0
    }
}

impl BuildingStats {
    /// Whether this kind fires on its own.
    pub const fn can_fight(&self) -> bool {
        !self.weapons.is_empty()
    }
}

/// Parameters of a buildable kind.
#[derive(Debug, Clone, Copy)]
pub struct ConstructionStats {
    /// Scrap price, deducted when the site is placed. Cancelling refunds
    /// `cost x hp / max_hp`, so enemy fire burns the refund.
    pub cost: u32,
    /// Builder-adjacent ticks from site to standing building.
    pub build_ticks: u32,
    /// Building kinds the owner must have completed before placing this
    /// one; identical for humans and bots. Empty means always available.
    pub requires: &'static [BuildingKind],
}

/// Time between an airborne casualty and its ground impact.
pub const AIRCRAFT_CRASH_TICKS: crate::Tick = 13;

/// The share of its last motion a falling airframe keeps over the whole
/// fall.
pub const AIRCRAFT_CRASH_COAST: Fx = Fx::lit("0.8");

/// Ground damage from a large aircraft reaching its crash site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrashProfile {
    /// Damage to each hostile ground body in the blast.
    pub damage: u32,
    /// Blast radius in world tiles, including building footprints.
    pub radius: Fx,
    /// Whether the falling airframe faces the way it was moving. Airframes
    /// that never steer their heading in flight need this to fall in the
    /// direction they drifted.
    pub aligns_to_motion: bool,
}

impl UnitKind {
    /// Hull turn speed in compass steps per tick: the kind's override, or
    /// scaled with ground mobility.
    pub fn ground_turn_rate(self) -> u8 {
        let stats = self.stats();
        if stats.domain != Domain::Ground {
            return 0;
        }
        stats.hull_turn_rate.unwrap_or_else(|| {
            (stats.speed * Fx::from_num(64))
                .ceil()
                .to_num::<u8>()
                .clamp(4, 10)
        })
    }

    /// Ticks a ground chassis loses leaving a stop on the opposite bearing,
    /// against covering the same ground at full speed: the pivot in place
    /// through a half turn, plus the ramps up to speed and back down to rest.
    pub fn ground_reversal_ticks(self) -> u64 {
        let rate = u64::from(self.ground_turn_rate());
        if rate == 0 {
            return 0;
        }
        let pivot = u64::from(128 - GROUND_ALIGNED_STEPS)
            .div_ceil(rate)
            .saturating_sub(1);
        // A linear ramp over `n` ticks covers the ground of `(n + 1) / 2`.
        let ramps = u64::from(GROUND_ACCEL_TICKS + GROUND_BRAKE_TICKS - 2).div_ceil(2);
        pivot + ramps
    }

    /// Ground gun mounts whose bearing is independent of the chassis.
    pub const fn has_ground_turret(self) -> bool {
        let stats = self.stats();
        matches!(stats.domain, Domain::Ground) && stats.turret_turn_rate > 0
    }

    /// Lowercase display name.
    pub const fn name(self) -> &'static str {
        match self {
            UnitKind::Harvester => "harvester",
            UnitKind::Sentinel => "sentinel",
            UnitKind::Scuttler => "scuttler",
            UnitKind::Lancer => "lancer",
            UnitKind::Bombard => "bombard",
            UnitKind::Flakhound => "flakhound",
            UnitKind::Buzzard => "buzzard",
            UnitKind::Talon => "talon",
            UnitKind::Warden => "warden",
            UnitKind::Excavator => "excavator",
            UnitKind::Kestrel => "kestrel",
            UnitKind::Condor => "condor",
            UnitKind::Breaker => "breaker",
            UnitKind::Avalanche => "avalanche",
            UnitKind::Skyhook => "skyhook",
            UnitKind::Sapper => "sapper",
            UnitKind::Mantle => "mantle",
            UnitKind::Piledriver => "piledriver",
            UnitKind::Surveyor => "surveyor",
        }
    }

    /// Role and constraints shared by the codex and training tooltip.
    pub const fn blurb(self) -> &'static str {
        match self {
            UnitKind::Harvester => {
                "Collects scrap and delivers it to a Foundry. Builds structures and repairs ground units."
            }
            UnitKind::Sentinel => "Short-range frontline unit. Can also fire on aircraft.",
            UnitKind::Scuttler => {
                "Fast, lightly armored raider. Best used against workers and exposed artillery."
            }
            UnitKind::Lancer => {
                "Long-range ground artillery. Vulnerable when enemies close the distance."
            }
            UnitKind::Bombard => {
                "Siege artillery with arcing explosive shells. Needs a spotter to use its full range."
            }
            UnitKind::Flakhound => "Armored anti-air platform. Cannot attack ground targets.",
            UnitKind::Buzzard => "Heavy ground-attack aircraft. Cannot attack other aircraft.",
            UnitKind::Talon => {
                "Air-superiority fighter with long sight range. Attacks aircraft only."
            }
            UnitKind::Warden => {
                "Armored frontline unit with a stronger main gun than the Sentinel."
            }
            UnitKind::Excavator => {
                "Heavy harvester with faster mining, more cargo space, and faster construction."
            }
            UnitKind::Kestrel => "Unarmed scout aircraft with long sight range.",
            UnitKind::Condor => {
                "Heavy bomber. Drops one large bomb per attack run and turns for another pass."
            }
            UnitKind::Breaker => "Heavy assault walker. Delivers powerful close-range blasts.",
            UnitKind::Avalanche => {
                "Long-range missile artillery. Cannot fire at enemies inside its minimum range."
            }
            UnitKind::Skyhook => {
                "Unarmed air transport. Carried units cannot fight and are lost if the transport is destroyed."
            }
            UnitKind::Sapper => {
                "Disposable demolition unit. Detonates against its target, dealing heavy damage to structures."
            }
            UnitKind::Mantle => {
                "Unarmed tracked carrier with three point-defense charges. Stops shells, rockets and bomb passes aimed near it; gunfire passes through."
            }
            UnitKind::Piledriver => {
                "Slow, enormous shots that break heavy machines; wasted on small ones. Must brace before firing."
            }
            UnitKind::Surveyor => {
                "Fast, unarmed ground scout with long sight range. Reveals buried Scuttle Charges nearby."
            }
        }
    }
}

/// The most weapons any kind carries; per-weapon cooldown state is sized
/// by this.
pub const MAX_WEAPONS: usize = 2;

const HARVESTER: UnitStats = UnitStats {
    contact_reach: None,
    max_hp: 60,
    speed: Fx::lit("0.125"), // 2.5 tiles/s at 20 tps
    radius: Fx::lit("0.3"),
    cost: 50,
    train_ticks: 100, // 5 s
    domain: Domain::Ground,
    weapons: &[],
    aggro_range: Fx::lit("0"),
    harvest: Some(HarvestStats {
        capacity: 10,
        ticks_per_scrap: 10, // 2 scrap/s while extracting
    }),
    vision: 6,
    requires: &[],
    build_rate: 1,
    demolition: None,
    transport_size: 1,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 0,
    turret_turn_rate: 0,
    hull_turn_rate: None,
    brace: None,
    target_priority: TargetPriority::Nearest,
    crash: None,
    charge_detection: false,
    point_defense: None,
};

const SENTINEL: UnitStats = UnitStats {
    contact_reach: None,
    // A cheap screen and scout rather than an efficient massed army:
    // rails one-shot it, Scuttler swarms out-trade it, and fixed
    // defenses punish unsupported groups.
    max_hp: 60,
    speed: Fx::lit("0.11"), // 2.2 tiles/s; slightly slower than harvesters
    radius: Fx::lit("0.35"),
    cost: 90,
    train_ticks: 150, // 7.5 s
    domain: Domain::Ground,
    weapons: &[
        WeaponStats {
            damage: 10,
            range: Fx::lit("2.5"),
            minimum_range: Fx::ZERO,
            cooldown_ticks: 20, // 1 hit/s
            targets: DomainMask::GROUND,
            splash: None,
            indirect: false,
            projectile: None,
        },
        // A weak anti-air weapon so a pure air army cannot ignore the core
        // army; dedicated anti-air remains the hard counter.
        WeaponStats {
            damage: 4,
            range: Fx::lit("3"),
            minimum_range: Fx::ZERO,
            cooldown_ticks: 30,
            targets: DomainMask::AIR,
            splash: None,
            indirect: false,
            projectile: None,
        },
    ],
    aggro_range: Fx::lit("5"),
    harvest: None,
    vision: 7, // strictly wider than aggro, so acquired targets are seen
    requires: &[],
    build_rate: 1,
    demolition: None,
    transport_size: 1,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 0,
    turret_turn_rate: 8,
    hull_turn_rate: None,
    brace: None,
    target_priority: TargetPriority::Nearest,
    crash: None,
    charge_detection: false,
    point_defense: None,
};

const SCUTTLER: UnitStats = UnitStats {
    contact_reach: Some(Fx::lit("0.16")),
    max_hp: 40,
    speed: Fx::lit("0.16"), // 3.2 tiles/s
    radius: Fx::lit("0.28"),
    cost: 40,
    train_ticks: 80, // 4 s
    domain: Domain::Ground,
    weapons: &[WeaponStats {
        damage: 3,
        range: Fx::lit("0.8"),
        minimum_range: Fx::ZERO,
        cooldown_ticks: 6, // 10 dps
        targets: DomainMask::GROUND,
        splash: None,
        indirect: false,
        projectile: None,
    }],
    aggro_range: Fx::lit("5"),
    harvest: None,
    vision: 6,
    requires: &[],
    build_rate: 1,
    demolition: None,
    transport_size: 1,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 0,
    turret_turn_rate: 0,
    hull_turn_rate: None,
    brace: None,
    target_priority: TargetPriority::Nearest,
    crash: None,
    charge_detection: false,
    point_defense: None,
};

const LANCER: UnitStats = UnitStats {
    contact_reach: None,
    max_hp: 50,
    speed: Fx::lit("0.08"), // 1.6 tiles/s
    radius: Fx::lit("0.35"),
    cost: 110,
    train_ticks: 200, // 10 s
    domain: Domain::Ground,
    weapons: &[WeaponStats {
        // Rewards the first tech rung: it one-shots Sentinels and light
        // machines, while siege and air remain effective counters.
        damage: 60,
        range: Fx::lit("5.5"), // beyond aggro: it only uses this on orders
        minimum_range: Fx::ZERO,
        cooldown_ticks: 60, // one heavy shot per 3 s
        targets: DomainMask::GROUND,
        splash: None,
        indirect: false,
        projectile: None,
    }],
    aggro_range: Fx::lit("5"),
    harvest: None,
    vision: 7,
    requires: &[],
    build_rate: 1,
    demolition: None,
    transport_size: 2,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 0,
    turret_turn_rate: 6,
    hull_turn_rate: None,
    brace: None,
    target_priority: TargetPriority::Nearest,
    crash: None,
    charge_detection: false,
    point_defense: None,
};

const BOMBARD: UnitStats = UnitStats {
    contact_reach: None,
    max_hp: 80,
    speed: Fx::lit("0.06"), // 1.2 tiles/s
    radius: Fx::lit("0.4"),
    cost: 200,
    train_ticks: 300, // 15 s
    domain: Domain::Ground,
    weapons: &[WeaponStats {
        damage: 45,
        range: Fx::lit("9.5"), // beyond its own vision: a spotter weapon
        minimum_range: Fx::ZERO,
        cooldown_ticks: 100, // one shell per 5 s
        targets: DomainMask::GROUND,
        splash: Some(Fx::lit("1.4")),
        indirect: true,
        projectile: Some(ProjectileStats {
            payload: ProjectileKind::Shell,
            speed: Fx::lit("0.30"),
        }),
    }],
    aggro_range: Fx::lit("9.5"), // its whole spotter-enabled firing envelope
    harvest: None,
    vision: 5, // shorter than its range: full reach needs a spotter
    requires: &[],
    build_rate: 1,
    demolition: None,
    transport_size: 3,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 0,
    turret_turn_rate: 0,
    hull_turn_rate: Some(3),
    brace: Some(BraceStats {
        deploy_ticks: 12,
        recoil_ticks: 8,
        retract_per_tick: 3,
    }),
    target_priority: TargetPriority::Nearest,
    crash: None,
    charge_detection: false,
    point_defense: None,
};

const FLAKHOUND: UnitStats = UnitStats {
    contact_reach: None,
    max_hp: 120,
    speed: Fx::lit("0.10"), // 2.0 tiles/s
    radius: Fx::lit("0.38"),
    cost: 90,
    train_ticks: 180, // 9 s
    domain: Domain::Ground,
    weapons: &[WeaponStats {
        damage: 8,
        range: Fx::lit("5"),
        minimum_range: Fx::ZERO,
        cooldown_ticks: 25,
        targets: DomainMask::AIR,
        splash: Some(Fx::lit("1.2")),
        indirect: false,
        projectile: None,
    }],
    aggro_range: Fx::lit("5"),
    harvest: None,
    vision: 7,
    requires: &[],
    build_rate: 1,
    demolition: None,
    transport_size: 2,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 0,
    turret_turn_rate: 0,
    hull_turn_rate: None,
    brace: None,
    target_priority: TargetPriority::Nearest,
    crash: None,
    charge_detection: false,
    point_defense: None,
};

const BUZZARD: UnitStats = UnitStats {
    contact_reach: None,
    max_hp: 110,
    speed: Fx::lit("0.10"), // 2.0 tiles/s
    radius: Fx::lit("0.4"),
    cost: 120,
    train_ticks: 180, // 9 s
    domain: Domain::Air,
    weapons: &[WeaponStats {
        damage: 25,
        range: Fx::lit("3"),
        minimum_range: Fx::ZERO,
        cooldown_ticks: 50,
        targets: DomainMask::GROUND,
        splash: None,
        indirect: false,
        projectile: None,
    }],
    aggro_range: Fx::lit("5"),
    harvest: None,
    vision: 7,
    requires: &[],
    build_rate: 1,
    demolition: None,
    transport_size: 0,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 0,
    turret_turn_rate: 6,
    hull_turn_rate: None,
    brace: None,
    target_priority: TargetPriority::Nearest,
    crash: None,
    charge_detection: false,
    point_defense: None,
};

const TALON: UnitStats = UnitStats {
    contact_reach: None,
    max_hp: 90,
    speed: Fx::lit("0.14"), // 2.8 tiles/s
    radius: Fx::lit("0.35"),
    cost: 110,
    train_ticks: 180, // 9 s
    domain: Domain::Air,
    weapons: &[WeaponStats {
        damage: 14,
        range: Fx::lit("3.5"),
        minimum_range: Fx::ZERO,
        cooldown_ticks: 25,
        targets: DomainMask::AIR,
        splash: None,
        indirect: false,
        projectile: None,
    }],
    aggro_range: Fx::lit("5"),
    harvest: None,
    vision: 8,
    requires: &[],
    build_rate: 1,
    demolition: None,
    transport_size: 0,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 8,
    turret_turn_rate: 0,
    hull_turn_rate: None,
    brace: None,
    target_priority: TargetPriority::Nearest,
    crash: None,
    charge_detection: false,
    point_defense: None,
};

const WARDEN: UnitStats = UnitStats {
    contact_reach: None,
    // The line brawler can trade into massed rails without replacing
    // the Lancer's role as the more efficient dedicated counter.
    max_hp: 340,
    speed: Fx::lit("0.09"),
    radius: Fx::lit("0.45"),
    cost: 280,
    train_ticks: 400,
    domain: Domain::Ground,
    weapons: &[WeaponStats {
        // One shot kills a Scuttler and two a Bombard, which is what lets
        // the middle tank hold against swarms.
        damage: 40,
        range: Fx::lit("3"),
        minimum_range: Fx::ZERO,
        cooldown_ticks: 31,
        targets: DomainMask::GROUND,
        splash: None,
        indirect: false,
        projectile: None,
    }],
    aggro_range: Fx::lit("5"),
    harvest: None,
    vision: 7,
    requires: &[],
    build_rate: 1,
    demolition: None,
    transport_size: 2,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 0,
    turret_turn_rate: 5,
    hull_turn_rate: None,
    brace: None,
    target_priority: TargetPriority::Nearest,
    crash: None,
    charge_detection: false,
    point_defense: None,
};

const EXCAVATOR: UnitStats = UnitStats {
    contact_reach: None,
    max_hp: 160,
    speed: Fx::lit("0.11"),
    radius: Fx::lit("0.42"),
    cost: 200,
    train_ticks: 350,
    domain: Domain::Ground,
    weapons: &[],
    aggro_range: Fx::ZERO,
    harvest: Some(HarvestStats {
        capacity: 30,
        ticks_per_scrap: 5,
    }),
    vision: 6,
    requires: &[BuildingKind::Fabricator],
    build_rate: 2,
    demolition: None,
    transport_size: 2,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 0,
    turret_turn_rate: 0,
    hull_turn_rate: None,
    brace: None,
    target_priority: TargetPriority::Nearest,
    crash: None,
    charge_detection: false,
    point_defense: None,
};

const KESTREL: UnitStats = UnitStats {
    contact_reach: None,
    max_hp: 60,
    speed: Fx::lit("0.2"),
    radius: Fx::lit("0.3"),
    cost: 60,
    train_ticks: 120,
    domain: Domain::Air,
    weapons: &[],
    aggro_range: Fx::ZERO,
    harvest: None,
    vision: 10,
    requires: &[],
    build_rate: 1,
    demolition: None,
    transport_size: 0,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 10,
    turret_turn_rate: 0,
    hull_turn_rate: None,
    brace: None,
    target_priority: TargetPriority::Nearest,
    crash: None,
    charge_detection: true,
    point_defense: None,
};

const CONDOR: UnitStats = UnitStats {
    contact_reach: None,
    max_hp: 260,
    speed: Fx::lit("0.11"),
    radius: Fx::lit("0.45"),
    cost: 700,
    train_ticks: 800,
    domain: Domain::Air,
    weapons: &[WeaponStats {
        damage: 100,
        range: Fx::lit("2.5"), // release point, not a standoff gun
        minimum_range: Fx::ZERO,
        cooldown_ticks: 150, // one bomb per pass
        targets: DomainMask::GROUND,
        splash: Some(Fx::lit("2.2")),
        indirect: true,
        projectile: Some(ProjectileStats {
            payload: ProjectileKind::Bomb,
            speed: Fx::lit("0.30"),
        }),
    }],
    aggro_range: Fx::lit("5"),
    harvest: None,
    vision: 6,
    requires: &[BuildingKind::Crucible],
    build_rate: 1,
    demolition: None,
    transport_size: 0,
    transport_capacity: 0,
    turn_rate: 2, // ~2.2-tile turn radius
    cruise_turn_rate: 0,
    turret_turn_rate: 0,
    hull_turn_rate: None,
    brace: None,
    target_priority: TargetPriority::Nearest,
    crash: Some(CrashProfile {
        damage: 50,
        radius: Fx::lit("2"),
        aligns_to_motion: false,
    }),
    charge_detection: false,
    point_defense: None,
};

const BREAKER: UnitStats = UnitStats {
    contact_reach: None,
    // A costly late-game answer to clustered tier-one armor. One shell
    // destroys a Lancer and punishes the surrounding clump; aircraft,
    // artillery, and economic pressure remain effective counters.
    max_hp: 900,
    speed: Fx::lit("0.055"),
    radius: Fx::lit("0.55"),
    cost: 900,
    train_ticks: 1200,
    domain: Domain::Ground,
    weapons: &[WeaponStats {
        damage: 115,
        range: Fx::lit("4.5"),
        minimum_range: Fx::ZERO,
        cooldown_ticks: 60,
        targets: DomainMask::GROUND,
        splash: Some(Fx::lit("1.5")),
        indirect: false,
        projectile: None,
    }],
    aggro_range: Fx::lit("6"),
    harvest: None,
    vision: 6,
    requires: &[],
    build_rate: 1,
    demolition: None,
    transport_size: 4,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 0,
    turret_turn_rate: 0,
    hull_turn_rate: Some(4),
    brace: None,
    target_priority: TargetPriority::Nearest,
    crash: None,
    charge_detection: false,
    point_defense: None,
};

const PILEDRIVER: UnitStats = UnitStats {
    contact_reach: None,
    // A Breaker takes four slugs; light machines waste most of one, so a
    // swarm outlasts it. Its own hp stays well under two slugs, so it never
    // counts as the heavy machine it hunts.
    max_hp: 240,
    speed: Fx::lit("0.06"), // 1.2 tiles/s
    radius: Fx::lit("0.5"),
    cost: 400,
    train_ticks: 600, // 30 s
    domain: Domain::Ground,
    weapons: &[WeaponStats {
        damage: 280,
        range: Fx::lit("8"), // past the Bulwark, short of the Bastion
        minimum_range: Fx::ZERO,
        cooldown_ticks: 100, // one slug per 5 s
        targets: DomainMask::GROUND,
        splash: None,
        indirect: false,
        projectile: None, // instant, so point defense cannot stop it
    }],
    aggro_range: Fx::lit("8"),
    harvest: None,
    vision: 8,
    requires: &[],
    build_rate: 1,
    demolition: None,
    transport_size: 3,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 0,
    turret_turn_rate: 0,
    hull_turn_rate: Some(3),
    brace: Some(BraceStats {
        deploy_ticks: 12,
        recoil_ticks: 8,
        retract_per_tick: 3,
    }),
    target_priority: TargetPriority::Heaviest,
    crash: None,
    charge_detection: false,
    point_defense: None,
};

const AVALANCHE: UnitStats = UnitStats {
    contact_reach: None,
    max_hp: 300,
    speed: Fx::lit("0.045"),
    radius: Fx::lit("0.5"),
    cost: 700,
    train_ticks: 900,
    domain: Domain::Ground,
    weapons: &[WeaponStats {
        // One shell destroys a Bombard so tier-one artillery does not
        // obsolete its successor. Units inside the blind ring and all
        // aircraft remain lethal counters.
        damage: 110,
        range: Fx::lit("14"),        // beyond its own vision: needs a spotter
        minimum_range: Fx::lit("4"), // blind ring at its feet
        cooldown_ticks: 120,
        targets: DomainMask::GROUND,
        splash: Some(Fx::lit("1.6")),
        indirect: true,
        projectile: Some(ProjectileStats {
            payload: ProjectileKind::Missile,
            speed: Fx::lit("0.30"),
        }),
    }],
    aggro_range: Fx::lit("14"),
    harvest: None,
    vision: 5,
    requires: &[],
    build_rate: 1,
    demolition: None,
    transport_size: 4,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 0,
    turret_turn_rate: 0,
    hull_turn_rate: Some(3),
    brace: None,
    target_priority: TargetPriority::Nearest,
    crash: None,
    charge_detection: false,
    point_defense: None,
};

const SKYHOOK: UnitStats = UnitStats {
    contact_reach: None,
    max_hp: 200,
    speed: Fx::lit("0.13"),
    radius: Fx::lit("0.45"),
    cost: 250,
    train_ticks: 400,
    domain: Domain::Air,
    weapons: &[],
    aggro_range: Fx::ZERO,
    harvest: None,
    vision: 6,
    requires: &[],
    build_rate: 1,
    demolition: None,
    transport_size: 0,
    transport_capacity: 4,
    turn_rate: 0,
    cruise_turn_rate: 0,
    turret_turn_rate: 0,
    hull_turn_rate: None,
    brace: None,
    target_priority: TargetPriority::Nearest,
    crash: Some(CrashProfile {
        damage: 40,
        radius: Fx::lit("2"),
        aligns_to_motion: true,
    }),
    charge_detection: false,
    point_defense: None,
};

const SAPPER: UnitStats = UnitStats {
    contact_reach: None,
    max_hp: 50,
    speed: Fx::lit("0.15"),
    radius: Fx::lit("0.3"),
    cost: 120,
    train_ticks: 180,
    domain: Domain::Ground,
    weapons: &[],
    aggro_range: Fx::ZERO, // never self-acquires a target
    harvest: None,
    vision: 5,
    requires: &[],
    build_rate: 1,
    demolition: Some(DemolitionStats {
        contact_range: Fx::lit("0.9"),
        structure_damage: 250,
        splash_damage: 60,
        blast_radius: Fx::lit("1.5"),
    }),
    transport_size: 1,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 0,
    turret_turn_rate: 0,
    hull_turn_rate: None,
    brace: None,
    target_priority: TargetPriority::Nearest,
    crash: None,
    charge_detection: false,
    point_defense: None,
};

const MANTLE: UnitStats = UnitStats {
    contact_reach: None,
    max_hp: 350,
    speed: Fx::lit("0.07"),
    radius: Fx::lit("0.45"),
    cost: 450,
    train_ticks: 500,
    domain: Domain::Ground,
    weapons: &[],
    aggro_range: Fx::ZERO,
    harvest: None,
    // Sees past its reach, so anything it shoots down is in its own sight.
    vision: 6,
    requires: &[],
    build_rate: 1,
    demolition: None,
    transport_size: 2,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 0,
    turret_turn_rate: 0,
    hull_turn_rate: None,
    brace: None,
    target_priority: TargetPriority::Nearest,
    crash: None,
    charge_detection: false,
    point_defense: Some(PointDefenseStats {
        charges: 3,
        radius: Fx::lit("4.5"),
        recharge_ticks: 60, // one charge back every 3 s
    }),
};

const SURVEYOR: UnitStats = UnitStats {
    contact_reach: None,
    max_hp: 40,
    speed: Fx::lit("0.18"), // 3.6 tiles/s; the fastest ground unit
    radius: Fx::lit("0.25"),
    cost: 30,
    train_ticks: 70, // 3.5 s
    domain: Domain::Ground,
    weapons: &[],
    aggro_range: Fx::ZERO,
    harvest: None,
    vision: 9,
    requires: &[],
    build_rate: 1,
    demolition: None,
    transport_size: 1,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 0,
    turret_turn_rate: 0,
    hull_turn_rate: None,
    brace: None,
    target_priority: TargetPriority::Nearest,
    crash: None,
    charge_detection: true,
    point_defense: None,
};

const FOUNDRY: BuildingStats = BuildingStats {
    // Durable enough that an opening rush creates pressure without
    // routinely ending a match before either side develops.
    max_hp: 1600,
    vision: 8,
    produces: &[
        UnitKind::Harvester,
        UnitKind::Sentinel,
        UnitKind::Scuttler,
        UnitKind::Excavator,
        UnitKind::Surveyor,
    ],
    weapons: &[],
    // The expansion base and comeback path. Its Fabricator prerequisite
    // makes a proxy Foundry a committed tech play, while its price keeps
    // expansion a credible mid-game choice.
    construction: Some(ConstructionStats {
        cost: 300,
        build_ticks: 600,
        requires: &[BuildingKind::Fabricator],
    }),
};

const TURRET: BuildingStats = BuildingStats {
    max_hp: 350,
    vision: 6,
    produces: &[],
    weapons: &[WeaponStats {
        damage: 12,
        range: Fx::lit("5"),
        minimum_range: Fx::ZERO,
        cooldown_ticks: 25,
        targets: DomainMask::GROUND,
        splash: None,
        indirect: false,
        projectile: None,
    }],
    construction: Some(ConstructionStats {
        cost: 100,
        build_ticks: 300, // 15 s of builder attention
        requires: &[],
    }),
};

const FABRICATOR: BuildingStats = BuildingStats {
    max_hp: 500,
    vision: 6,
    // Order groups the roles for the HUD's slot labels.
    produces: &[
        UnitKind::Lancer,
        UnitKind::Bombard,
        UnitKind::Flakhound,
        UnitKind::Warden,
        UnitKind::Sapper,
    ],
    weapons: &[],
    construction: Some(ConstructionStats {
        cost: 120,
        build_ticks: 280, // 14 s — the tech window must fit inside the rush window
        requires: &[],
    }),
};

const FLAK_TURRET: BuildingStats = BuildingStats {
    max_hp: 300,
    vision: 7,
    produces: &[],
    weapons: &[WeaponStats {
        damage: 7,
        range: Fx::lit("5.5"),
        minimum_range: Fx::ZERO,
        cooldown_ticks: 12,
        targets: DomainMask::AIR,
        splash: Some(Fx::lit("1.2")),
        indirect: false,
        projectile: None,
    }],
    construction: Some(ConstructionStats {
        cost: 90,
        build_ticks: 250,
        requires: &[],
    }),
};

const BASTION: BuildingStats = BuildingStats {
    max_hp: 500,
    vision: 6,
    produces: &[],
    weapons: &[WeaponStats {
        damage: 40,
        // A building aims from its center while a gun firing at it measures
        // to its nearest edge, so a Bombard at its full 9.5 outranges this by
        // the footprint's half-diagonal: mobile siege beats static siege until
        // the Citadel. Full reach needs a spotter.
        range: Fx::lit("9.5"),
        minimum_range: Fx::lit("2.5"),
        cooldown_ticks: 90,
        targets: DomainMask::GROUND,
        splash: Some(Fx::lit("1.3")),
        indirect: true,
        projectile: Some(ProjectileStats {
            payload: ProjectileKind::Shell,
            speed: Fx::lit("0.30"),
        }),
    }],
    // Competes with the mobile Bombard by trading mobility for a durable
    // firing position rather than losing the comparison on price alone.
    construction: Some(ConstructionStats {
        cost: 210,
        build_ticks: 500,
        requires: &[BuildingKind::Fabricator],
    }),
};

const ARRAY: BuildingStats = BuildingStats {
    // A permanent early-warning sentry. Unlike a scout aircraft it does
    // not need attention, but it cannot move or identify radar contacts.
    max_hp: 250,
    vision: 9, // the inner ring: true sight
    produces: &[],
    weapons: &[],
    construction: Some(ConstructionStats {
        cost: 90,
        build_ticks: 300,
        requires: &[],
    }),
};

const RECLAIMER: BuildingStats = BuildingStats {
    max_hp: 300,
    vision: 4,
    produces: &[],
    weapons: &[],
    construction: Some(ConstructionStats {
        cost: 150,
        build_ticks: 350,
        requires: &[],
    }),
};

const REPAIR_BAY: BuildingStats = BuildingStats {
    max_hp: 400,
    vision: 5,
    produces: &[],
    weapons: &[],
    construction: Some(ConstructionStats {
        cost: 200,
        build_ticks: 350,
        requires: &[],
    }),
};

const AIRWORKS: BuildingStats = BuildingStats {
    max_hp: 500,
    vision: 6,
    produces: &[
        UnitKind::Buzzard,
        UnitKind::Talon,
        UnitKind::Kestrel,
        UnitKind::Condor,
        UnitKind::Skyhook,
    ],
    weapons: &[],
    construction: Some(ConstructionStats {
        cost: 200,
        build_ticks: 350,
        requires: &[BuildingKind::Fabricator],
    }),
};

const CRUCIBLE: BuildingStats = BuildingStats {
    max_hp: 900,
    vision: 6,
    produces: &[
        UnitKind::Breaker,
        UnitKind::Avalanche,
        UnitKind::Mantle,
        UnitKind::Piledriver,
    ],
    weapons: &[],
    // This gate must be affordable often enough for the late-game roster
    // to appear; stronger units do not matter when their factory is never
    // a credible purchase.
    construction: Some(ConstructionStats {
        cost: 400,
        build_ticks: 550,
        requires: &[BuildingKind::Fabricator],
    }),
};

const BARRICADE: BuildingStats = BuildingStats {
    max_hp: 400,
    vision: 1,
    produces: &[],
    weapons: &[],
    construction: Some(ConstructionStats {
        cost: 40,
        build_ticks: 120,
        requires: &[],
    }),
};

const SCUTTLE_CHARGE: BuildingStats = BuildingStats {
    max_hp: 20,
    vision: 1,
    produces: &[],
    weapons: &[],
    construction: Some(ConstructionStats {
        cost: 30,
        build_ticks: 60,
        requires: &[BuildingKind::Fabricator],
    }),
};

const EXTRACTOR: BuildingStats = BuildingStats {
    max_hp: 600,
    vision: 4,
    produces: &[],
    weapons: &[],
    // Cheap to restore but hard to hold: the price buys durable income on
    // map-authored ground every player can read.
    construction: Some(ConstructionStats {
        cost: 100,
        build_ticks: 300,
        requires: &[],
    }),
};

// ---- Upgrade tiers ----------------------------------------------------
//
// Each upgradeable kind carries an array of tier structs; a building's
// `tier` indexes it. A tier's `construction` row is the price of the
// upgrade that produced it (tier 0 keeps the ordinary build price), so
// repair pricing and refund logic read the tier they are welding.
// `BuildingKind::upgrade_from` reads the next tier's row where one exists.

const HEAVY_TURRET: BuildingStats = BuildingStats {
    max_hp: 500,
    vision: 6,
    produces: &[],
    weapons: &[WeaponStats {
        damage: 20,
        range: Fx::lit("6"),
        minimum_range: Fx::ZERO,
        cooldown_ticks: 25,
        targets: DomainMask::GROUND,
        splash: None,
        indirect: false,
        projectile: None,
    }],
    construction: Some(ConstructionStats {
        cost: 150,
        build_ticks: 300,
        requires: &[BuildingKind::Fabricator],
    }),
};

const BULWARK: BuildingStats = BuildingStats {
    max_hp: 900,
    vision: 7,
    produces: &[],
    weapons: &[WeaponStats {
        damage: 60,
        range: Fx::lit("7.5"),
        minimum_range: Fx::ZERO,
        cooldown_ticks: 50,
        targets: DomainMask::GROUND,
        splash: None,
        indirect: false,
        projectile: None,
    }],
    construction: Some(ConstructionStats {
        cost: 300,
        build_ticks: 500,
        requires: &[BuildingKind::Crucible],
    }),
};

const BURST_FLAK: BuildingStats = BuildingStats {
    max_hp: 400,
    vision: 7,
    produces: &[],
    weapons: &[WeaponStats {
        damage: 12,
        range: Fx::lit("6"),
        minimum_range: Fx::ZERO,
        cooldown_ticks: 10,
        targets: DomainMask::AIR,
        splash: Some(Fx::lit("1.5")),
        indirect: false,
        projectile: None,
    }],
    construction: Some(ConstructionStats {
        cost: 120,
        build_ticks: 250,
        requires: &[BuildingKind::Fabricator],
    }),
};

const REFINERY: BuildingStats = BuildingStats {
    max_hp: 400,
    vision: 4,
    produces: &[],
    weapons: &[],
    construction: Some(ConstructionStats {
        cost: 300,
        build_ticks: 600,
        requires: &[BuildingKind::Fabricator],
    }),
};

const DEEP_BORE: BuildingStats = BuildingStats {
    max_hp: 900,
    vision: 4,
    produces: &[],
    weapons: &[],
    construction: Some(ConstructionStats {
        cost: 350,
        build_ticks: 600,
        requires: &[BuildingKind::Crucible],
    }),
};

const DEEP_ARRAY: BuildingStats = BuildingStats {
    max_hp: 300,
    vision: 11,
    produces: &[],
    weapons: &[],
    construction: Some(ConstructionStats {
        cost: 150,
        build_ticks: 300,
        // The Crucible itself requires the Fabricator.
        requires: &[BuildingKind::Crucible],
    }),
};

const CITADEL: BuildingStats = BuildingStats {
    max_hp: 500,
    vision: 6,
    produces: &[],
    weapons: &[WeaponStats {
        damage: 40,
        // Clears a Bombard's 9.5 plus the footprint's half-diagonal, so it
        // answers one from any side, and stays short of the Avalanche's 14.
        range: Fx::lit("12.5"),
        minimum_range: Fx::lit("2.5"),
        cooldown_ticks: 90,
        targets: DomainMask::GROUND,
        splash: Some(Fx::lit("1.3")),
        indirect: true,
        projectile: Some(ProjectileStats {
            payload: ProjectileKind::Shell,
            speed: Fx::lit("0.30"),
        }),
    }],
    construction: Some(ConstructionStats {
        cost: 250,
        build_ticks: 400,
        requires: &[BuildingKind::Crucible],
    }),
};

impl UnitKind {
    /// Static stats for this kind.
    pub const fn stats(self) -> &'static UnitStats {
        match self {
            UnitKind::Harvester => &HARVESTER,
            UnitKind::Sentinel => &SENTINEL,
            UnitKind::Scuttler => &SCUTTLER,
            UnitKind::Lancer => &LANCER,
            UnitKind::Bombard => &BOMBARD,
            UnitKind::Flakhound => &FLAKHOUND,
            UnitKind::Buzzard => &BUZZARD,
            UnitKind::Talon => &TALON,
            UnitKind::Warden => &WARDEN,
            UnitKind::Excavator => &EXCAVATOR,
            UnitKind::Kestrel => &KESTREL,
            UnitKind::Condor => &CONDOR,
            UnitKind::Breaker => &BREAKER,
            UnitKind::Avalanche => &AVALANCHE,
            UnitKind::Skyhook => &SKYHOOK,
            UnitKind::Sapper => &SAPPER,
            UnitKind::Mantle => &MANTLE,
            UnitKind::Piledriver => &PILEDRIVER,
            UnitKind::Surveyor => &SURVEYOR,
        }
    }

    /// Whether this unit can serve as the ground escort in a stranded
    /// economy's recovery package.
    pub(crate) fn is_recovery_screen(self) -> bool {
        let stats = self.stats();
        stats.domain == Domain::Ground
            && stats
                .weapons
                .iter()
                .any(|weapon| weapon.targets.covers(Domain::Ground) && weapon.projectile.is_none())
    }
}

impl BuildingKind {
    /// Lowercase display name.
    pub const fn name(self) -> &'static str {
        match self {
            BuildingKind::Foundry => "foundry",
            BuildingKind::Turret => "turret",
            BuildingKind::Fabricator => "fabricator",
            BuildingKind::FlakTurret => "flak turret",
            BuildingKind::Bastion => "bastion",
            BuildingKind::Array => "array",
            BuildingKind::Reclaimer => "reclaimer",
            BuildingKind::RepairBay => "repair bay",
            BuildingKind::Extractor => "extractor",
            BuildingKind::Airworks => "airworks",
            BuildingKind::Crucible => "crucible",
            BuildingKind::Barricade => "barricade",
            BuildingKind::ScuttleCharge => "scuttle charge",
        }
    }

    /// Role and constraints shared by the codex and construction tooltip.
    pub const fn blurb(self) -> &'static str {
        match self {
            BuildingKind::Foundry => {
                "Headquarters, basic unit production, and scrap drop-off. Losing every Foundry loses the match."
            }
            BuildingKind::Turret => {
                "Automatic defense against ground units. Requires line of sight."
            }
            BuildingKind::Fabricator => {
                "Produces advanced ground units and unlocks further construction."
            }
            BuildingKind::FlakTurret => "Fixed anti-air defense. Cannot attack ground targets.",
            BuildingKind::Bastion => {
                "Long-range artillery with explosive shells. Needs a spotter beyond its sight range. Upgrade to a Citadel for more range."
            }
            BuildingKind::Array => {
                "Extends vision, detects hidden charges, and tracks distant radar contacts that weapons can fire on."
            }
            BuildingKind::Reclaimer => {
                "Produces scrap continuously without workers. Upgrade to a Refinery for higher output."
            }
            BuildingKind::RepairBay => {
                "Repairs up to four nearby friendly machines at a time, fast once they are out of combat, and completed buildings slowly, using scrap. Condors must land to be repaired. Cannot repair itself."
            }
            BuildingKind::Extractor => {
                "Rebuild on a derelict mining frame to generate scrap. A nearby own Foundry increases output. Upgrade to a Deep Bore for triple output."
            }
            BuildingKind::Airworks => "Produces aircraft.",
            BuildingKind::Crucible => {
                "Produces heavy ground units and unlocks the highest upgrades."
            }
            BuildingKind::Barricade => {
                "Blocks ground movement. Does not block aircraft or gunfire."
            }
            BuildingKind::ScuttleCharge => {
                "Visible while building, concealed when armed. Triggered by hostile ground units or construction. Revealed by scouts or an Array."
            }
        }
    }

    /// Tier-zero stats for this kind. Most callers want a live
    /// building's [`crate::state::Building::stats`], which follows the
    /// upgrade ladder; this base row is for costs, footprints, and
    /// other tier-invariant questions.
    pub const fn base_stats(self) -> &'static BuildingStats {
        match self {
            BuildingKind::Foundry => &FOUNDRY,
            BuildingKind::Turret => &TURRET,
            BuildingKind::Fabricator => &FABRICATOR,
            BuildingKind::FlakTurret => &FLAK_TURRET,
            BuildingKind::Bastion => &BASTION,
            BuildingKind::Array => &ARRAY,
            BuildingKind::Reclaimer => &RECLAIMER,
            BuildingKind::RepairBay => &REPAIR_BAY,
            BuildingKind::Extractor => &EXTRACTOR,
            BuildingKind::Airworks => &AIRWORKS,
            BuildingKind::Crucible => &CRUCIBLE,
            BuildingKind::Barricade => &BARRICADE,
            BuildingKind::ScuttleCharge => &SCUTTLE_CHARGE,
        }
    }

    /// Whether harvesters can deliver their cargo here. The one funnel
    /// every drop-off decision consults — deliveries, withdrawal homes,
    /// and route planning alike.
    pub const fn is_drop_off(self) -> bool {
        matches!(self, BuildingKind::Foundry)
    }

    /// Whether this kind hides from enemies until actively detected
    /// (see `State::building_apparent`). The Scuttle Charge is the
    /// game's only stealth.
    pub const fn is_stealthy(self) -> bool {
        matches!(self, BuildingKind::ScuttleCharge)
    }

    /// Whether every seat knows where a building of this kind stands at
    /// `tier`, regardless of fog: the Deep Bore, from the moment its
    /// upgrade starts.
    pub const fn is_public(self, tier: u8) -> bool {
        matches!(self, BuildingKind::Extractor) && tier >= 1
    }
}

/// Scrap contained in a freshly parsed node tile.
pub const SCRAP_NODE_AMOUNT: u32 = 400;

/// Numerator of the fraction of a destroyed entity's price left on the
/// field as wreck salvage.
pub const WRECK_VALUE_NUM: u32 = 45;
/// Denominator of the wreck-value fraction.
pub const WRECK_VALUE_DEN: u32 = 100;

/// Wreck price basis for a destroyed building whose kind has no
/// construction cost.
pub const FOUNDRY_WRECK_VALUE: u32 = 300;

/// Ticks between global wreck-decay steps (every wreck tile loses one
/// salvage per step). Battlefield scrap outlasts the battle by minutes but
/// is never a permanent bank.
pub const WRECK_DECAY_TICKS: u64 = 300;

/// Outer detection ring of the Array, in tiles: hostile units and buildings
/// inside it but out of true sight appear as blips (a tile, no kind, no
/// owner). Each blip feeds a contact track that attacks may target.
pub const RADAR_DETECT_RADIUS: i32 = 20;

/// Ticks per scrap credited by each built Reclaimer. Slow enough to serve as
/// insurance and a stalemate valve rather than an opening.
pub const RECLAIMER_PERIOD: u64 = 40;

/// Ticks per scrap credited by each completed Foundry: the income floor.
///
/// Exhausted nodes, lost Reclaimers, and camped salvage can slow a seat but
/// never leave it with no income. Credit is per Foundry, but the rate is low
/// enough that this income alone is not a reason to expand.
pub const FOUNDRY_DRIP_PERIOD: u64 = 60;

/// First completed tick eligible for the Foundry income floor. The warm-up
/// keeps it from becoming free opening economy.
pub const FOUNDRY_DRIP_START_TICK: u64 = 2_400;

/// Ticks per emergency scrap credited by a surviving Foundry after its
/// owner's last harvesting machine is gone. Each real deposit arms one finite
/// recovery entitlement; spending or cancelling that package cannot refill
/// it.
pub const FOUNDRY_RECOVERY_PERIOD: u64 = 10;

/// Maximum symmetric emergency entitlement available to a stranded seat:
/// one cheap screen plus its replacement Harvester. A seat with a paid
/// ground screen captures only the Harvester-sized deficit.
pub const FOUNDRY_RECOVERY_RESERVE: u32 = SENTINEL.cost + HARVESTER.cost;

/// Release gate for turn-limited bombers: the target must sit inside
/// the forward cone, `dot(heading, to_target) >= |to_target| * CONE`.
/// 0.92 is a half-angle of about 23 degrees: wide enough that a clean pass
/// releases, narrow enough that a bomber circling its target must
/// straighten out first.
pub const BOMBER_CONE_DOT: Fx = Fx::lit("0.92");

/// The furthest ahead, in ticks, a projectile weapon leads a moving target.
pub const MAX_LEAD_TICKS: u64 = 96;

/// The furthest ring searched for chase stand-ins; it covers the longest
/// anti-air reach (range 5 lands exactly on ring 5's axis tiles).
pub const CHASE_STAND_RADIUS: i32 = 5;

/// Acceptance slack added to a turn-limited flier's computed turn
/// radius: the ring inside which a waypoint or goal counts as reached.
/// An acceptance ring smaller than the turn radius is an orbit trap the
/// aircraft can circle forever.
pub const BOMBER_ACCEPT_SLACK: Fx = Fx::lit("0.4");
/// Ticks an idle turn-limited flier orbits before setting itself down.
pub const AUTO_LAND_IDLE_TICKS: u16 = 60;
/// Ticks between auto-land ground scans after a probe finds nowhere to
/// set down. The scan walks every tile in the radius with full run-in
/// geometry, so a crowded or sealed pocket must not pay it every tick.
/// Chosen to divide `u16::MAX - AUTO_LAND_IDLE_TICKS`: `settled`
/// saturates, and a saturated orbiter falls back to probing every tick
/// rather than never probing again.
pub const AUTO_LAND_RETRY_TICKS: u16 = 15;
/// How close to the tile center a landing pass must come to touch down.
pub const LANDING_TOUCHDOWN: Fx = Fx::lit("0.35");
/// Run-in initial-point distances tried farthest first for attack passes.
pub const RUN_IN_DISTANCES: [i64; 2] = [7, 5];
/// Run-in initial-point distances tried nearest first for landings that
/// cannot be flown straight in; the entry fix twice as far out is what
/// lines the approach up, so the shorter procedure wins when it fits.
pub const LANDING_RUN_IN_DISTANCES: [i64; 2] = [5, 7];
/// Tiles searched around a blocked landing tile for another place to set
/// down.
pub const LANDING_REPLAN_RADIUS: i32 = 3;
/// Tiles searched around an idle flier for somewhere to land on its own.
pub const AUTO_LAND_SCAN_RADIUS: i32 = 4;
/// How close to its destination a turn-limited flier hands a ground-goal
/// order over to a landing on that tile. Matches the longest run-in so the
/// approach is planned with room to line up.
pub const LANDING_HANDOFF_REACH: Fx = Fx::lit("7");

/// How close a boarding machine must stand to its transport before the
/// sling takes it.
pub const LOAD_REACH: Fx = Fx::lit("1.5");

/// Ring-scan radius when a transport sets its cargo down: the farthest
/// tile from the drop point a disgorged machine may appear on.
pub const UNLOAD_SCAN_RADIUS: i32 = 4;

/// A hostile ground machine inside this radius of a buried charge sets
/// it off.
pub const CHARGE_TRIGGER_RADIUS: Fx = Fx::lit("0.8");

/// Damage a detonating charge deals to every hostile ground machine in
/// its blast ring.
pub const CHARGE_DAMAGE: u32 = 60;

/// The charge's blast ring.
pub const CHARGE_BLAST_RADIUS: Fx = Fx::lit("1.5");

/// A machine with [`UnitStats::charge_detection`] within this many tiles
/// reveals buried charges to its team.
pub const CHARGE_SCOUT_DETECT_RADIUS: i32 = 4;

/// A built base-tier Array reveals buried charges inside this closer
/// ring (euclidean, like radar contacts). The Deep Array upgrade buys the
/// wider `CHARGE_ARRAY_DETECT_RADIUS`, and scouts remain the mobile
/// detection channel.
pub const CHARGE_BASE_ARRAY_DETECT_RADIUS: i32 = 12;

/// A built Deep Array (Array tier 1) reveals buried charges inside this
/// ring (euclidean, like radar contacts).
pub const CHARGE_ARRAY_DETECT_RADIUS: i32 = 22;

/// Ticks per scrap credited by a tier-one Reclaimer (the Refinery).
pub const REFINERY_PERIOD: u64 = 10;

/// Maximum footprint-to-footprint tile distance at which a completed own
/// Foundry supports an Extractor.
pub const EXTRACTOR_SUPPORT_RADIUS: i32 = 8;

/// Remote Extractors pay one scrap every half second: 120 a minute.
pub const EXTRACTOR_REMOTE_YIELD: (u32, u64) = (1, 10);

/// Extractors supported by at least one nearby completed own Foundry pay
/// three scrap every second: 180 a minute. Additional Foundries do not stack.
pub const EXTRACTOR_SUPPORTED_YIELD: (u32, u64) = (3, 20);

/// Remote Deep Bores (Extractor tier 1) pay three times a remote Extractor:
/// 360 a minute.
pub const DEEP_BORE_REMOTE_YIELD: (u32, u64) = (3, 10);

/// Supported Deep Bores pay three times a supported Extractor: 540 a minute.
pub const DEEP_BORE_SUPPORTED_YIELD: (u32, u64) = (9, 20);

/// Ticks between one-hp decay steps on an abandoned tier-zero construction
/// site.
pub const SITE_DECAY_PERIOD: u64 = 8;

/// Per-mille of a building's cost billed per hp welded (against `max_hp`).
/// The three economy verbs price strictly build > repair > salvage: welding
/// always costs more than salvage refunds, so repair followed by salvage
/// loses scrap, and repair is cheaper than replacement but never free.
pub const REPAIR_COST_PERMILLE: u64 = 850;

/// Scrap charged for the next unit-repair tick at the given repair progress.
/// Uses cumulative rounding so consecutive ticks agree with authoritative billing.
pub fn unit_repair_debit(kind: UnitKind, progress: u32) -> u32 {
    let stats = kind.stats();
    let owed = |ticks: u32| {
        let welded = u64::from(stats.max_hp) * u64::from(ticks) / u64::from(stats.train_ticks);
        welded * u64::from(stats.cost) * REPAIR_COST_PERMILLE / u64::from(stats.max_hp)
    };
    u32::try_from(owed(progress + 1).div_ceil(1000) - owed(progress).div_ceil(1000))
        .expect("one unit-repair tick debit fits u32")
}

/// Per-mille of a building's cost refunded per hp drained by salvage
/// (against `max_hp`). A full-health salvage banks cost * permille / 1000.
pub const SALVAGE_REFUND_PERMILLE: u64 = 800;

/// Per-mille of a unit's cost a retirement at a Crucible refunds at full
/// health, scaled linearly by remaining hp. Units price like buildings:
/// below [`REPAIR_COST_PERMILLE`] per hp, so welding a machine up and then
/// retiring it always loses scrap.
pub const RETIRE_REFUND_PERMILLE: u64 = 500;

/// Ticks a unit stands at a Crucible being dismantled. Long enough to read,
/// and to leave fire time to win, so a dying machine cannot cash out.
pub const RETIRE_TICKS: u32 = 60;

/// How near a Crucible's footprint a retiring unit stands or hovers to be
/// dismantled, in tiles. A doorstep tile is within it.
pub const RETIRE_REACH: Fx = Fx::lit("1.5");

/// Scrap a retirement refunds for a `kind` machine at `hp`.
pub fn retire_refund(kind: UnitKind, hp: u32) -> u32 {
    let stats = kind.stats();
    let millis = u64::from(stats.cost) * RETIRE_REFUND_PERMILLE * u64::from(hp.min(stats.max_hp))
        / u64::from(stats.max_hp);
    u32::try_from(millis / 1000).expect("a refund never exceeds the unit's cost")
}

/// Reach of the Repair Bay's aura, in tiles from the nearest point of its
/// footprint. Shorter than every siege weapon's reach, so the counter to a
/// healed defense is firing from outside it.
pub const REPAIR_BAY_RADIUS: Fx = Fx::lit("4.0");

/// Ticks between Repair Bay aura pulses.
pub const REPAIR_BAY_PERIOD: u64 = 8;

/// Machines one Repair Bay services at once, most wounded first.
pub const REPAIR_BAY_PATIENTS: usize = 4;

/// Ticks a machine must go undamaged before a Repair Bay heals it fast.
pub const REPAIR_BAY_CALM_TICKS: u64 = 60;

/// Per mille of max hp per second a Repair Bay restores to a machine that
/// has gone [`REPAIR_BAY_CALM_TICKS`] undamaged: half a Breaker back in
/// about 20 s. Never slower than [`REPAIR_BAY_STEP`] per pulse.
pub const REPAIR_BAY_FAST_PERMILLE_PER_SECOND: u64 = 30;

/// Reach of the Crucible's smelter, in tiles from the nearest point of its
/// footprint.
pub const CRUCIBLE_SMELT_RADIUS: Fx = Fx::lit("6.0");

/// Ticks between smelter pulses; each pulse melts one wreck unit into one
/// scrap. The rate stays below a dedicated harvester working the same field.
pub const CRUCIBLE_SMELT_PERIOD: u64 = 40;

/// Hp each aura pulse offers a patient under fire, and each damaged
/// structure in the ring: well below one Turret's damage rate, so an aura
/// never out-heals focused fire.
pub const REPAIR_BAY_STEP: u32 = 1;

/// Welding ramp for Foundry repair, used instead of its build time.
pub const FOUNDRY_REPAIR_TICKS: u32 = 400;

/// Billing basis for Foundry repair, used instead of its construction cost.
/// A full repair is expensive without making the victory structure
/// impossible to restore during a siege.
pub const FOUNDRY_REPAIR_PRICE: u32 = 100;

/// Scrap in a rich node (the `S` map legend).
pub const RICH_SCRAP_NODE_AMOUNT: u32 = 800;

/// Maximum queued units per Foundry.
pub const QUEUE_CAP: usize = 8;

/// Maximum orders (and patrol waypoints) queued per unit. Bounds what a
/// hostile append stream can make a unit remember.
pub const ORDER_QUEUE_CAP: usize = 32;

/// A* expansion budget per query — bounds worst-case pathfinding work.
pub const PATH_EXPANSION_CAP: u32 = 20_000;

/// Chebyshev radius of a Harvester's work zone around the source the
/// player clicked. Seven spans the widest deliberately connected deposit
/// on the shipped map shelf, while a fixed anchor prevents hop-by-hop drift
/// into another patch.
pub const HARVEST_ZONE_RADIUS: i32 = 7;

/// Chassis overhang into the empty margin of a work target's tile footprint.
pub const WORK_FOOTPRINT_OVERHANG: Fx = Fx::lit("0.42");

/// Clearance between a working chassis center and a footprint's outer margin.
pub const WORK_FOOTPRINT_GAP: Fx = Fx::lit("0.03");

/// Clearance beyond the combined hull radii when approaching a field-repair patient.
pub const WORK_APPROACH_GAP: Fx = Fx::lit("0.10");

/// Tool reach beyond the hull, including a small contact tolerance.
pub const WORK_REACH: Fx = Fx::lit("0.15");

/// An uninterrupted half-second release before cargo enters the bank.
pub const UNLOAD_TICKS: u8 = 10;

/// A radar blip only makes salvage unsafe when it is this close to a
/// candidate source. Contacts carry no identity or range, so a distant
/// blip must not withdraw an otherwise healthy work zone.
pub const HARVEST_RADAR_DANGER_RADIUS: i32 = 4;

/// How long a hit from an unseen attacker keeps the allied victim's tile
/// unsafe for autonomous salvage work. The memory contains only that impact
/// tile, never the hidden attacker's identity or position. It outlasts the
/// slowest artillery cooldown (Avalanche, 6 s), so sustained fire from out of
/// sight keeps the memory alive while a stopped barrage releases workers soon
/// after.
pub const HARVEST_INCIDENT_MEMORY_TICKS: crate::Tick = 8 * crate::TICKS_PER_SECOND as crate::Tick;

/// Radius around a recent allied impact or loss that autonomous Harvest
/// treats as unsafe while the incident memory is live.
pub const HARVEST_INCIDENT_DANGER_RADIUS: i32 = 4;

/// Maximum recent allied impact sites retained per team. Incidents at one
/// tile coalesce, and the oldest expiry is evicted first at the ceiling.
pub const HARVEST_INCIDENT_CAP: usize = 64;

/// Mobile ground threats are treated as dangerous this many tiles beyond
/// their current weapon reach. It gives a visible raider's approach time
/// weight while sight is live; incident memory is the separate, deliberately
/// less informative signal that remains after sight is lost.
pub const HARVEST_MOBILE_DANGER_MARGIN: Fx = Fx::lit("3");

/// Remembered hostile emplacements are static, so their conservative
/// danger margin can stay tighter than a mobile threat's.
pub const HARVEST_STATIC_DANGER_MARGIN: Fx = Fx::lit("1");

/// When a ground group's explored tile goal is impassable, the group spreads
/// around the nearest passable tile within this radius. With none, it heads
/// for the tile itself and ends as close as it can get.
pub const GOAL_SNAP_RADIUS: i32 = 3;

/// The wider ring an air goal on a peak snaps within: flyers clear every
/// tile but peaks, so their nearest open sky can sit farther out.
pub const AIR_GOAL_SNAP_RADIUS: i32 = GOAL_SNAP_RADIUS + 3;

/// Same-owner bodies may close to this fraction of their combined radii,
/// so a friendly group packs tighter than it spaces from enemies.
pub const SAME_OWNER_COMPRESSION: Fx = Fx::lit("0.65");

/// How far from its body a neighbor's destination may lie and still count
/// as a claim on that position.
pub const ARRIVAL_WINDOW: Fx = Fx::lit("1.25");

/// Clearance beyond a unit's diameter that a waiting position keeps from
/// the contested one.
pub const WAITING_CLEARANCE: Fx = Fx::lit("0.20");

/// How far the footprint-eviction pre-pass ring-scans for a walkable
/// escape tile. Any real escape starts on an adjacent open tile (A*
/// cannot leave a fully sealed one), so the reach only pads for
/// corner-cut geometry around the footprint.
pub const EVICT_SCAN_RADIUS: i32 = 3;

/// Relaxation passes of collision resolution per tick. More passes settle
/// dense crowds faster; each pass is a full pairwise sweep.
pub const COLLISION_ITERATIONS: u32 = 3;

/// How close to a waypoint counts as "reached" when another waypoint
/// follows (final waypoints are still landed exactly). Prevents the
/// push-off/re-seek oscillation that makes crowds grind.
pub const WAYPOINT_ACCEPT: Fx = Fx::lit("0.35");

/// Heading error, in compass steps, beyond which a rolling ground chassis
/// brakes and pivots in place instead of steering through the bend. 96
/// steps is 135 degrees: right-angle corners are driven as arcs; a reversal
/// stops first.
pub const GROUND_PIVOT_THRESHOLD: u8 = 96;

/// Heading error, in compass steps, inside which a ground chassis is on its
/// bearing: it rolls from rest and tracks its target point directly.
pub const GROUND_ALIGNED_STEPS: u8 = 8;

/// Ticks a ground motor takes from rest to full speed.
pub const GROUND_ACCEL_TICKS: u8 = 6;

/// Ticks a ground motor takes from full speed to rest.
pub const GROUND_BRAKE_TICKS: u8 = 3;

/// Running ticks of contact cancelling most of a ground body's intended
/// progress before it drops its route and its brain plans again from where
/// the body actually is. Long enough that a slide past a neighbor is not a
/// replan; short enough that shoving a parked worker never lasts a second.
pub const STALL_REPLAN_TICKS: u8 = 12;

/// Ticks a Harvester held by danger waits before searching again: one on its
/// ordered route after a failed search for a safe detour keeps walking that
/// route, and one holding cargo with no safe way to a drop-off keeps standing.
/// Either reacts at once when the hold begins; only the repeat waits.
pub const HARVEST_DANGER_RETRY_TICKS: u64 = 16;

/// Existing routes re-check only this near segment each tick. A Harvester
/// needs 64 ticks to traverse eight clear cardinal tiles, so this is
/// ample deterministic warning without turning every worker tick into a
/// full path-length by threat-count scan.
pub const HARVEST_DANGER_LOOKAHEAD: usize = 8;

/// The near slice of the lookahead that always reacts immediately: a threat
/// inside these route tiles replans this tick. Only a flag beyond this zone
/// defers to the staggered replan window, [`HARVEST_REPLAN_PERIOD`].
pub const HARVEST_DANGER_REACT_ZONE: usize = 3;

/// A worker whose retained route fails the danger lookahead only in the far
/// zone does not re-plan every tick while the threat lingers; that would run
/// a full multi-candidate A* per worker per tick and jitter the fleet
/// between near-equal detours. Far-zone replans stagger on this period,
/// keyed by owner-local unit rank so a fleet never re-plans in unison
/// without making cross-seat production ids a tactical input; near-zone
/// threats never wait.
pub const HARVEST_REPLAN_PERIOD: u64 = 4;

/// Ticks between repeated `DangerHold` reports for one waiting worker.
pub const DANGER_HOLD_REPORT_PERIOD: u64 = 100;

/// How many upcoming route waypoints the ground follower may skip per tick
/// toward the furthest one its hull can reach on a straight, clear leg.
/// Bounds the per-unit line checks; a longer clear leg is rediscovered tick
/// by tick as the body advances.
pub const ROUTE_LOOKAHEAD: usize = 6;

/// Within this range of a shared goal, touching an already-arrived
/// neighbor counts as arriving — crowds settle instead of churning on the
/// click point.
pub const ARRIVAL_NEAR: Fx = Fx::lit("1.5");

/// How far from an unreachable goal's endpoint a parked crowd may extend and
/// still end a walk that touches it. Endpoints sit on the edge of reachable
/// ground, where [`ARRIVAL_NEAR`] alone leaves room for only a few bodies.
pub const CROWD_CHAIN_REACH: Fx = Fx::lit("16");

/// Collision share taken by an anchored unit (extracting or firing from a
/// hold); the mover takes the rest. Passers-by flow around workers.
pub const ANCHORED_PUSH_SHARE: Fx = Fx::lit("0.1");

/// Furthest collision resolution may displace one unit in a whole tick,
/// across every relaxation pass. This stays below the visible-jolt limit
/// while leaving enough separation headroom for dense armies to flow.
pub const COLLISION_MAX_STEP: Fx = Fx::lit("0.155");

/// Overlap two bodies may rest at without a correction. A correction lands
/// a pair exactly at its spacing, and fixed-point rounding leaves the next
/// check a hair inside it; without this allowance parked crowds would be
/// pushed apart by nothing on every pass of every tick.
pub const COLLISION_SLOP: Fx = Fx::lit("0.00390625");

/// The slide blend for a moving unit's collision correction. A pure push
/// along the contact normal is exactly undone by a head-on pair's path
/// following, freezing the pair, so a mover's correction is
/// `RADIAL_SHARE * away + LATERAL_SHARE * sideways`, the sideways half
/// picked toward the mover's own travel. Both constants are exactly
/// representable in Q32.32 and their squares sum to 0.98828125 < 1, so the
/// blended direction never exceeds unit length and [`COLLISION_MAX_STEP`]
/// holds. The radial share must stay well below half the closing rate or
/// the freeze returns; the lateral share converts a grind into a pass-by.
pub const SLIDE_RADIAL_SHARE: Fx = Fx::lit("0.5");
/// See [`SLIDE_RADIAL_SHARE`].
pub const SLIDE_LATERAL_SHARE: Fx = Fx::lit("0.859375");

/// How far from its anchor a self-acquired chase may reach before the
/// guard breaks off and walks home, in tiles. Must stay at least the
/// Bombard's weapon range: a shorter tether would let siege pieces shell a
/// guard that turns back before ever answering.
pub const LEASH_RADIUS: Fx = Fx::lit("10");

/// Ticks a self-acquired chase may continue beyond the leash radius after
/// the fight was joined (a shot fired or answered, each refreshing the
/// window): enough to finish a wounded runner, not enough for a cross-map
/// dive. Bait that never came in reach grants none, so a kited guard
/// breaks at the radius line.
pub const LEASH_PATIENCE: u16 = 60;

/// Ticks a returned guard stands at its post before re-acquiring. Without
/// it, an enemy dancing at the aggro edge strips a picket in an endless
/// acquire/return cycle.
pub const LEASH_REACQUIRE_COOLDOWN: u16 = 60;

/// Ticks of standing idle before a machine counts as stationed; only a
/// stationed machine's self-acquired fights tether. A unit cycling
/// through idle mid-battle (its target fell, the next is a tick away)
/// re-acquires unleashed; otherwise unit-id ordering can decide which
/// side's advancing army is tethered first.
pub const LEASH_STATION_TICKS: u16 = 40;

/// How far from the escorted unit an escort looks for a fight, in tiles,
/// capped by its own aggro range. Welders mend escort-party members within
/// the same distance of it.
pub const ESCORT_ENGAGE_RADIUS: Fx = Fx::lit("7");

/// How far from the escorted unit an escort's victim may stray before the
/// escort breaks off, firing included, and returns to its slot. Wider than
/// [`ESCORT_ENGAGE_RADIUS`] so a fight it just picked never ends on the next
/// tick.
pub const ESCORT_BREAK_RADIUS: Fx = Fx::lit("10");
