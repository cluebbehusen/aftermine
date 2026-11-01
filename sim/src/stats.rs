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
        /// Experimental breacher: a huge, slow mining machine that cuts
        /// through structures, is weak against machines, and heals only by
        /// feeding wreck to its kiln. Assembled in a cradle, never trained.
        Maw,
        /// Experimental gunship: a slow, heavy air transport whose two guns
        /// pick their own ground targets. Assembled in a cradle, never
        /// trained.
        Roc,
        /// A Roc's small anti-air drone: built in its bay, launched to fight
        /// enemy aircraft and screen fire on a short leash around it, and
        /// docked again once its host is calm. Never trained or ordered.
        Drone,
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
        /// Strategic missile silo: assembles one warhead at a time, turns
        /// its launch deck toward an ordered tile, and fires it anywhere on
        /// the map.
        Spire,
        /// Interceptor battery: two pods that stop hostile warheads their
        /// team has tracked once those enter its ring.
        Lattice,
        /// Assembly cradle where workers build a Maw. Public once work
        /// starts; on completion the cradle is gone and the Maw stands up.
        MawCradle,
        /// Assembly cradle where workers build a Roc, public like the Maw's;
        /// on completion the Roc lifts off from its centre.
        RocCradle,
    }
}

/// A movement medium. Ground units path and collide on the terrain grid;
/// air units fly over everything but peaks and collide only with each
/// other.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
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
    /// Hit points removed per hit on a structure, when that differs from
    /// [`Self::damage`]: a building, or a machine built like one.
    pub building_damage: Option<u32>,
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
    /// Completed own buildings the owner needs before training this unit;
    /// identical for humans and bots. Empty means the producer alone
    /// decides.
    pub requires: &'static [Prerequisite],
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
    /// Takes the Scout order. Only ground kinds scout, since the caution
    /// that order applies reads ground threats.
    pub scouts: bool,
    /// The machine's radar, if it carries one.
    pub radar: Option<RadarStats>,
    /// Charges that shoot down hostile projectiles aimed near the machine.
    pub point_defense: Option<PointDefenseStats>,
    /// How the machine recovers lost hit points.
    pub mend: Mend,
    /// Built like a structure: demolition charges and building-breaking
    /// weapons deal it their structure damage.
    pub structural: bool,
    /// Guns on fixed pivots of the hull, one per weapon slot, each choosing
    /// its own target inside its arc. `None` for every machine whose weapons
    /// fire from its centre.
    pub mounts: Option<MountedGuns>,
    /// The drones the machine builds, docks and launches. `None` for every
    /// machine without a drone bay.
    pub bay: Option<BayStats>,
}

/// A drone bay: drones built one at a time, docked out of reach, and
/// launched to fight around the host while it is under threat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BayStats {
    /// Drones the host holds, docked, flying and ordered together.
    pub capacity: u8,
    /// The kind of drone it builds.
    pub drone: UnitKind,
    /// Scrap one drone costs.
    pub cost: u32,
    /// Ticks to build one drone.
    pub build_ticks: u32,
    /// Whether a new host refills its bay on its own.
    pub autofill: bool,
}

/// A gun on a fixed pivot of its hull that traverses only inside its arc.
/// Offsets and the arc are measured in the hull's frame: forward along the
/// nose, starboard a quarter turn clockwise of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MountStats {
    /// Tiles the pivot sits ahead of the hull's centre.
    pub forward: Fx,
    /// Tiles the pivot sits to starboard of the centreline; negative to port.
    pub starboard: Fx,
    /// The arc's centre against the nose, in compass steps, starboard
    /// positive.
    pub arc_centre: i8,
    /// Compass steps the gun may turn either side of its arc's centre.
    pub arc_half: u8,
}

impl MountStats {
    /// The pivot's world position on a hull at `pos` facing `heading`.
    pub fn pivot(&self, pos: chassis::fx::Vec2Fx, heading: u8) -> chassis::fx::Vec2Fx {
        pos + chassis::compass::dir(heading) * self.forward
            + chassis::compass::dir(heading.wrapping_add(64)) * self.starboard
    }

    /// Steps from the arc's centre to `relative`, a bearing against the
    /// nose, signed toward starboard.
    pub fn arc_offset(&self, relative: u8) -> i16 {
        i16::from(
            relative
                .wrapping_sub(self.arc_centre.cast_unsigned())
                .cast_signed(),
        )
    }

    /// Whether the gun can bear on `relative`, a bearing against the nose.
    pub fn covers(&self, relative: u8) -> bool {
        self.arc_offset(relative).unsigned_abs() <= u16::from(self.arc_half)
    }
}

/// A hull's mounted guns and how its gunners and helm behave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MountedGuns {
    /// One mount per weapon slot, in slot order.
    pub mounts: &'static [MountStats],
    /// Compass steps per tick each gun traverses.
    pub traverse: u8,
    /// Ticks an idle hull keeps a facing it turned to before it turns again,
    /// so a crowd around it cannot keep it spinning.
    pub facing_hold: u8,
    /// No pivot lies farther than this from the hull's centre: the margin a
    /// gun's range reaches beyond the centre's.
    pub reach: Fx,
}

/// How a damaged machine recovers hit points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mend {
    /// Field welds and Repair Bays restore it, billed from the bank.
    Repair,
    /// Neither: its kiln feeds on wreck salvage around its hull instead.
    Kiln(KilnStats),
    /// Neither: it launches whole from its host's bay each time.
    Dock,
}

/// A kiln that turns wreck salvage into hit points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KilnStats {
    /// Hit points one unit of wreck restores; each bite takes one unit, every
    /// [`KILN_BITE_PERIOD`] ticks.
    pub hp_per_wreck: u32,
    /// How far past the tiles under its hull the intake reaches.
    pub reach: i32,
}

/// How a unit picks an enemy when nobody ordered a target. It ranks only
/// enemies it could engage now, so a unit with nothing better in reach still
/// answers what it can, and every kind but a breacher takes a machine before
/// a building.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetPriority {
    /// Machines, and then buildings, that can hit it from where they stand
    /// come first. Within each group a victim in weapon reach ranks by how
    /// fast the unit's fire removes its danger: its damage rate against the
    /// unit, times the unit's damage rate against it, over its health. The
    /// nearest of the rest follow.
    Danger,
    /// The one with the most full health, then the nearest: a gun whose one
    /// slug a light machine would mostly waste.
    Heaviest,
    /// Buildings before machines: a breacher that strikes a machine only
    /// when no structure is in reach.
    Buildings,
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
    /// How far the gun turns off the hull while the spades are out; `None`
    /// keeps it fixed to the hull.
    pub traverse: Option<PlantedTraverse>,
}

/// A planted gun's swing off its hull.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlantedTraverse {
    /// Compass steps the gun may stand off the hull either way; at most 127.
    pub arc: u8,
    /// Compass steps the gun turns per tick.
    pub rate: u8,
}

/// A machine's radar: hostile units and buildings inside its ring but out
/// of true sight appear to its team as contacts (a tile, no kind, no owner).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadarStats {
    /// Ring radius in tiles, measured from the machine's tile.
    pub radius: i32,
    /// Ticks the machine must stand still before the ring comes up; any
    /// driving of its own folds it again. 0 keeps it up at all times.
    pub deploy_ticks: u8,
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

    /// Whether field welds and Repair Bays restore this kind.
    pub const fn repairable(&self) -> bool {
        matches!(self.mend, Mend::Repair)
    }

    /// Whether this kind is a drone, flying for a host's bay.
    pub const fn docks(&self) -> bool {
        matches!(self.mend, Mend::Dock)
    }

    /// Whether this kind carries any weapon at all.
    pub const fn can_fight(&self) -> bool {
        !self.weapons.is_empty() || self.demolition.is_some()
    }

    /// Whether any weapon covers the given domain.
    pub fn can_target(&self, domain: Domain) -> bool {
        self.weapons.iter().any(|w| w.targets.covers(domain))
    }

    /// The longest reach of any weapon covering the given domain, from the
    /// hull's centre.
    pub fn max_range_vs(&self, domain: Domain) -> Option<Fx> {
        self.weapons
            .iter()
            .filter(|w| w.targets.covers(domain))
            .map(|w| self.reach(w))
            .max()
    }

    /// How far from the hull's centre `weapon` reaches: its range, plus how
    /// far a mounted gun's pivot sits off the centre. Exactly the range for
    /// every machine without mounts.
    pub fn reach(&self, weapon: &WeaponStats) -> Fx {
        weapon.range + self.mounts.map_or(Fx::ZERO, |guns| guns.reach)
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
    /// Compass steps per tick the gun turns. Zero exactly when the tier
    /// fires nothing.
    pub traverse: u8,
    /// Price, build time, and prerequisites for placing this kind. Every
    /// current kind has one; for an upgrade tier it prices the upgrade.
    pub construction: Option<ConstructionStats>,
    /// Radar ring radius in tiles, measured from the anchor tile, for a
    /// built mast: hostile units and buildings inside it but out of true
    /// sight appear as contacts (a tile, no kind, no owner).
    pub radar: Option<i32>,
}

impl BuildingKind {
    /// Whether the kind may stand on a ramp: a ramp may be walled or mined,
    /// never built over.
    pub const fn fits_ramp(self) -> bool {
        matches!(self, BuildingKind::Barricade | BuildingKind::ScuttleCharge)
    }

    /// Whether the kind is a wall, which only stands in the way.
    pub const fn is_wall(self) -> bool {
        matches!(self, BuildingKind::Barricade)
    }

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
            | BuildingKind::Extractor
            | BuildingKind::Spire
            | BuildingKind::Lattice => (2, 2),
            BuildingKind::MawCradle | BuildingKind::RocCradle => (3, 3),
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
            BuildingKind::Spire => &[&SPIRE],
            BuildingKind::Lattice => &[&LATTICE],
            BuildingKind::MawCradle => &[&MAW_CRADLE],
            BuildingKind::RocCradle => &[&ROC_CRADLE],
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

    /// Scrap a weld on a building at `tier` bills against: the active
    /// tier's construction price, or [`FOUNDRY_REPAIR_PRICE`] for the
    /// Foundry, whose construction price would inflate the bill for the
    /// victory structure.
    pub fn repair_basis(self, tier: u8) -> u32 {
        if self == BuildingKind::Foundry {
            return FOUNDRY_REPAIR_PRICE;
        }
        self.tier_stats(tier)
            .construction
            .map_or(FOUNDRY_REPAIR_PRICE, |construction| construction.cost)
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

    /// Whether this kind fires on its own, which every rung of its ladder
    /// then does through a turning gun.
    pub const fn fires(self) -> bool {
        self.base_stats().can_fight()
    }

    /// Whether an idle gun of this kind turns back to its rest. A Bastion's
    /// traverse is too slow to give up its last bearing.
    pub const fn gun_rests(self) -> bool {
        !matches!(self, BuildingKind::Bastion)
    }

    /// The experimental a cradle of this kind assembles, if it is one.
    pub const fn assembles(self) -> Option<UnitKind> {
        match self {
            BuildingKind::MawCradle => Some(UnitKind::Maw),
            BuildingKind::RocCradle => Some(UnitKind::Roc),
            _ => None,
        }
    }

    /// The rounds this kind stores and what it does with them, if any.
    pub const fn ordnance(self) -> Option<&'static Ordnance> {
        match self {
            BuildingKind::Spire => Some(&SPIRE_ORDNANCE),
            BuildingKind::Lattice => Some(&LATTICE_ORDNANCE),
            _ => None,
        }
    }

    /// Its magazine, if this kind stores rounds.
    pub const fn magazine(self) -> Option<&'static MagazineStats> {
        match self.ordnance() {
            Some(ordnance) => Some(ordnance.magazine()),
            None => None,
        }
    }

    /// The silo stats, if this kind launches warheads.
    pub const fn silo(self) -> Option<&'static SiloStats> {
        match self.ordnance() {
            Some(Ordnance::Silo(silo)) => Some(silo),
            _ => None,
        }
    }

    /// The battery stats, if this kind intercepts warheads.
    pub const fn battery(self) -> Option<&'static BatteryStats> {
        match self.ordnance() {
            Some(Ordnance::Battery(battery)) => Some(battery),
            _ => None,
        }
    }

    /// The share of warhead damage, in permille, this kind shrugs off.
    /// Only the Foundry is hardened, so a strike cannot end a match alone.
    pub const fn warhead_resistance_permille(self) -> u32 {
        match self {
            BuildingKind::Foundry => 500,
            _ => 0,
        }
    }

    /// Warhead damage a building of this kind takes from `damage`.
    pub fn warhead_taken(self, damage: u32) -> u32 {
        let kept = 1000 - self.warhead_resistance_permille();
        // Widened so a large damage value cannot overflow the product.
        u32::try_from(u64::from(damage) * u64::from(kept) / 1000)
            .expect("resistance never raises damage")
    }
}

/// What a magazine building does with its rounds.
#[derive(Debug, Clone, Copy)]
pub enum Ordnance {
    /// Launches warheads at ordered tiles.
    Silo(SiloStats),
    /// Spends interceptors on tracked hostile warheads in its ring.
    Battery(BatteryStats),
}

impl Ordnance {
    /// The magazine either kind keeps.
    pub const fn magazine(&self) -> &MagazineStats {
        match self {
            Ordnance::Silo(silo) => &silo.magazine,
            Ordnance::Battery(battery) => &battery.magazine,
        }
    }
}

/// A building's store of prepaid rounds, assembled one at a time.
#[derive(Debug, Clone, Copy)]
pub struct MagazineStats {
    /// Rounds it holds when full.
    pub capacity: u8,
    /// Scrap paid when a round is ordered; cancelling refunds it whole.
    pub round_cost: u32,
    /// Ticks to assemble one round.
    pub round_ticks: u32,
    /// Whether a new building orders rounds on its own while it can pay.
    pub autofill: bool,
}

/// A missile silo.
#[derive(Debug, Clone, Copy)]
pub struct SiloStats {
    /// Its stored warheads.
    pub magazine: MagazineStats,
    /// Heading steps per tick the launch deck turns.
    pub turn_rate: u8,
    /// Ticks the deck stands aligned before it fires, while its locks
    /// release the round.
    pub ready_ticks: u8,
}

/// An interceptor battery.
#[derive(Debug, Clone, Copy)]
pub struct BatteryStats {
    /// Its stored interceptors.
    pub magazine: MagazineStats,
    /// Radius around the battery's centre it defends: a warhead whose
    /// impact and current position both lie inside may be stopped.
    pub ring: Fx,
}

/// A strategic warhead: unguided from launch, one falloff splash on
/// impact for units and buildings alike.
#[derive(Debug, Clone, Copy)]
pub struct WarheadStats {
    /// Tiles per tick over the ground.
    pub speed: Fx,
    /// Damage at or inside `full_radius`.
    pub peak_damage: u32,
    /// Radius of full damage around the impact point.
    pub full_radius: Fx,
    /// Radius where damage has fallen linearly to nothing.
    pub outer_radius: Fx,
}

impl WarheadStats {
    /// Flight length in ticks (at least one) from `from` to `to`.
    pub fn flight_ticks(&self, from: chassis::fx::Vec2Fx, to: chassis::fx::Vec2Fx) -> u64 {
        (from.dist(to) / self.speed).ceil().to_num::<u64>().max(1)
    }

    /// Damage at `distance` from the impact point, before resistance.
    pub fn damage_at(&self, distance: Fx) -> u32 {
        if distance <= self.full_radius {
            return self.peak_damage;
        }
        if distance >= self.outer_radius {
            return 0;
        }
        let fade = (self.outer_radius - distance) / (self.outer_radius - self.full_radius);
        (Fx::from_num(self.peak_damage) * fade)
            .floor()
            .to_num::<u32>()
    }
}

/// Ticks, not necessarily consecutive, a team must hold a hostile warhead
/// in sight or radar to track it: its line becomes known.
pub const WARHEAD_TRACK_TICKS: u8 = 30;

/// Further ticks in sight or radar, after tracking, before the team's
/// batteries can lock on and stop it. They cover an interceptor's climb,
/// so a warhead tracked late still meets its interceptor in the air.
pub const WARHEAD_LOCK_TICKS: u8 = 30;

impl BuildingStats {
    /// Whether this kind fires on its own.
    pub const fn can_fight(&self) -> bool {
        !self.weapons.is_empty()
    }
}

/// The stages a site or upgrade passes through, one per equal share of its
/// labor. Each stage has its own scaffold art and its own contact outline.
pub const CONSTRUCTION_STAGES: u8 = 3;

/// Parameters of a buildable kind.
#[derive(Debug, Clone, Copy)]
pub struct ConstructionStats {
    /// Scrap price, deducted when the site is placed. Cancelling refunds
    /// `cost x hp / max_hp`, so enemy fire burns the refund.
    pub cost: u32,
    /// Builder-adjacent ticks from site to standing building.
    pub build_ticks: u32,
    /// Completed own buildings the owner needs before placing this one;
    /// identical for humans and bots. Empty means always available.
    pub requires: &'static [Prerequisite],
}

/// A completed own building that training, construction or an upgrade
/// needs: one of `kind` at `tier` or above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prerequisite {
    /// The building kind.
    pub kind: BuildingKind,
    /// The lowest tier that counts.
    pub tier: u8,
}

impl Prerequisite {
    /// Any completed building of `kind`.
    pub const fn built(kind: BuildingKind) -> Self {
        Self { kind, tier: 0 }
    }

    /// A completed building of `kind` upgraded to `tier` or above.
    pub const fn upgraded(kind: BuildingKind, tier: u8) -> Self {
        Self { kind, tier }
    }

    /// What players call it: the tier's own name for an upgrade.
    pub const fn name(self) -> &'static str {
        self.kind.tier_name(self.tier)
    }
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
    /// Whether a cradle assembles this kind: an experimental, of which a
    /// player fields one at a time and which no Crucible dismantles.
    pub const fn assembled(self) -> bool {
        let mut i = 0;
        while i < BuildingKind::ALL.len() {
            if let Some(kind) = BuildingKind::ALL[i].assembles()
                && kind as u8 == self as u8
            {
                return true;
            }
            i += 1;
        }
        false
    }

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
            UnitKind::Maw => "maw",
            UnitKind::Roc => "roc",
            UnitKind::Drone => "drone",
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
    target_priority: TargetPriority::Danger,
    crash: None,
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: None,
    mend: Mend::Repair,
    structural: false,
    bay: None,
    mounts: None,
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
            building_damage: None,
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
            building_damage: None,
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
    target_priority: TargetPriority::Danger,
    crash: None,
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: None,
    mend: Mend::Repair,
    structural: false,
    bay: None,
    mounts: None,
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
        building_damage: None,
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
    target_priority: TargetPriority::Danger,
    crash: None,
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: None,
    mend: Mend::Repair,
    structural: false,
    bay: None,
    mounts: None,
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
        building_damage: None,
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
    target_priority: TargetPriority::Danger,
    crash: None,
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: None,
    mend: Mend::Repair,
    structural: false,
    bay: None,
    mounts: None,
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
        building_damage: None,
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
        traverse: None,
    }),
    target_priority: TargetPriority::Danger,
    crash: None,
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: None,
    mend: Mend::Repair,
    structural: false,
    bay: None,
    mounts: None,
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
        building_damage: None,
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
    target_priority: TargetPriority::Danger,
    crash: None,
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: None,
    mend: Mend::Repair,
    structural: false,
    bay: None,
    mounts: None,
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
        building_damage: None,
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
    target_priority: TargetPriority::Danger,
    crash: None,
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: None,
    mend: Mend::Repair,
    structural: false,
    bay: None,
    mounts: None,
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
        building_damage: None,
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
    target_priority: TargetPriority::Danger,
    crash: None,
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: None,
    mend: Mend::Repair,
    structural: false,
    bay: None,
    mounts: None,
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
        building_damage: None,
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
    target_priority: TargetPriority::Danger,
    crash: None,
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: None,
    mend: Mend::Repair,
    structural: false,
    bay: None,
    mounts: None,
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
    requires: &[Prerequisite::built(BuildingKind::Fabricator)],
    build_rate: 2,
    demolition: None,
    transport_size: 2,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 0,
    turret_turn_rate: 0,
    hull_turn_rate: None,
    brace: None,
    target_priority: TargetPriority::Danger,
    crash: None,
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: None,
    mend: Mend::Repair,
    structural: false,
    bay: None,
    mounts: None,
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
    target_priority: TargetPriority::Danger,
    crash: None,
    charge_detection: true,
    scouts: false,
    radar: Some(RadarStats {
        radius: 14,
        deploy_ticks: 0,
    }),
    point_defense: None,
    mend: Mend::Repair,
    structural: false,
    bay: None,
    mounts: None,
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
        building_damage: None,
        projectile: Some(ProjectileStats {
            payload: ProjectileKind::Bomb,
            speed: Fx::lit("0.30"),
        }),
    }],
    aggro_range: Fx::lit("5"),
    harvest: None,
    vision: 6,
    requires: &[Prerequisite::built(BuildingKind::Crucible)],
    build_rate: 1,
    demolition: None,
    transport_size: 0,
    transport_capacity: 0,
    turn_rate: 2, // ~2.2-tile turn radius
    cruise_turn_rate: 0,
    turret_turn_rate: 0,
    hull_turn_rate: None,
    brace: None,
    target_priority: TargetPriority::Danger,
    crash: Some(CrashProfile {
        damage: 50,
        radius: Fx::lit("2"),
        aligns_to_motion: false,
    }),
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: None,
    mend: Mend::Repair,
    structural: false,
    bay: None,
    mounts: None,
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
        building_damage: None,
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
    target_priority: TargetPriority::Danger,
    crash: None,
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: None,
    mend: Mend::Repair,
    structural: false,
    bay: None,
    mounts: None,
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
        building_damage: None,
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
        deploy_ticks: 20,
        recoil_ticks: 8,
        retract_per_tick: 2,
        // Planted, the gun follows a target drifting up to 22.5 degrees
        // either way at 28 degrees a second before the spades must come up.
        traverse: Some(PlantedTraverse { arc: 16, rate: 1 }),
    }),
    target_priority: TargetPriority::Heaviest,
    crash: None,
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: None,
    mend: Mend::Repair,
    structural: false,
    bay: None,
    mounts: None,
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
        building_damage: None,
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
    target_priority: TargetPriority::Danger,
    crash: None,
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: None,
    mend: Mend::Repair,
    structural: false,
    bay: None,
    mounts: None,
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
    target_priority: TargetPriority::Danger,
    crash: Some(CrashProfile {
        damage: 40,
        radius: Fx::lit("2"),
        aligns_to_motion: true,
    }),
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: None,
    mend: Mend::Repair,
    structural: false,
    bay: None,
    mounts: None,
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
    target_priority: TargetPriority::Danger,
    crash: None,
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: None,
    mend: Mend::Repair,
    structural: false,
    bay: None,
    mounts: None,
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
    target_priority: TargetPriority::Danger,
    crash: None,
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: Some(PointDefenseStats {
        charges: 3,
        radius: Fx::lit("4.5"),
        recharge_ticks: 60, // one charge back every 3 s
    }),
    mend: Mend::Repair,
    structural: false,
    bay: None,
    mounts: None,
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
    target_priority: TargetPriority::Danger,
    crash: None,
    charge_detection: true,
    scouts: true,
    radar: Some(RadarStats {
        radius: 12,
        deploy_ticks: 40, // 2 s
    }),
    point_defense: None,
    mend: Mend::Repair,
    structural: false,
    bay: None,
    mounts: None,
};

const MAW: UnitStats = UnitStats {
    // The drum cuts with the hull against its target: its lowered edge is
    // the hull's collision edge.
    contact_reach: Some(Fx::lit("0.05")),
    // Four Breakers' price in one hull: it outlasts a fortified position it
    // reaches, and loses an open fight to the same scrap of machines.
    max_hp: 6000,
    speed: Fx::lit("0.045"), // 0.9 tiles/s
    radius: Fx::lit("1.4"),
    cost: 3600,
    // Never trained: workers assemble it in its cradle. This is the full
    // crew's assembly time.
    train_ticks: 2400,
    domain: Domain::Ground,
    weapons: &[WeaponStats {
        // 30 per second against machines, 300 against structures.
        damage: 6,
        range: Fx::lit("3.4"), // the hulls plus contact reach decide
        minimum_range: Fx::ZERO,
        cooldown_ticks: 4,
        targets: DomainMask::GROUND,
        splash: None,
        indirect: false,
        building_damage: Some(60),
        projectile: None,
    }],
    aggro_range: Fx::lit("3"),
    harvest: None,
    vision: 6,
    requires: &[],
    build_rate: 1,
    demolition: None,
    transport_size: 0,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 0,
    turret_turn_rate: 0,
    hull_turn_rate: Some(3),
    brace: None,
    target_priority: TargetPriority::Buildings,
    crash: None,
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: None,
    mend: Mend::Kiln(KilnStats {
        hp_per_wreck: 2,
        reach: 1,
    }),
    structural: true,
    bay: None,
    mounts: None,
};

/// One of the Roc's two guns: 30 damage a second against ground, measured
/// from its own pivot, short of a Flak Turret's reach.
const ROC_GUN: WeaponStats = WeaponStats {
    damage: 15,
    range: Fx::lit("5"),
    minimum_range: Fx::ZERO,
    cooldown_ticks: 10,
    targets: DomainMask::GROUND,
    splash: None,
    indirect: false,
    building_damage: None,
    projectile: None,
};

const ROC: UnitStats = UnitStats {
    contact_reach: None,
    max_hp: 4500,
    speed: Fx::lit("0.07"), // 1.4 tiles/s, half a Skyhook
    radius: Fx::lit("1.3"),
    cost: 3200,
    // Never trained: workers assemble it in its cradle. This is the full
    // crew's assembly time.
    train_ticks: 2125,
    domain: Domain::Air,
    weapons: &[ROC_GUN, ROC_GUN],
    // The guns choose their own targets; nothing acquires through this.
    aggro_range: Fx::lit("5"),
    harvest: None,
    vision: 7,
    requires: &[],
    build_rate: 1,
    demolition: None,
    transport_size: 0,
    transport_capacity: 12,
    turn_rate: 0,
    // 180 degrees in 3.2 s.
    cruise_turn_rate: 2,
    turret_turn_rate: 0,
    hull_turn_rate: None,
    brace: None,
    target_priority: TargetPriority::Danger,
    // A pushed hull keeps its heading, so its guns' arcs never jump.
    crash: Some(CrashProfile {
        damage: 120,
        radius: Fx::lit("3"),
        aligns_to_motion: false,
    }),
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: None,
    mend: Mend::Repair,
    structural: false,
    bay: Some(ROC_BAY),
    mounts: Some(MountedGuns {
        // Port then starboard, where the art's gun pivots sit. Each arc of
        // 270 degrees is centred 45 degrees outboard of the nose, so its
        // blind quarter lies behind the far wing: both guns bear ahead of
        // the beam, one on each rear quarter.
        mounts: &[
            MountStats {
                forward: Fx::lit("0.671875"),
                starboard: Fx::lit("-0.53125"),
                arc_centre: -32,
                arc_half: 96,
            },
            MountStats {
                forward: Fx::lit("0.671875"),
                starboard: Fx::lit("0.53125"),
                arc_centre: 32,
                arc_half: 96,
            },
        ],
        // 180 degrees in about one second.
        traverse: 6,
        facing_hold: 60,
        // Covers sqrt(0.65^2 + 0.55^2), about 0.851.
        reach: Fx::lit("0.86"),
    }),
};

/// The Roc's drone bay: six drones, built one at a time.
const ROC_BAY: BayStats = BayStats {
    capacity: 6,
    drone: UnitKind::Drone,
    cost: DRONE.cost,
    build_ticks: DRONE.train_ticks,
    autofill: true,
};

/// A Roc's drone. Six trade about evenly with three Talons.
const DRONE: UnitStats = UnitStats {
    contact_reach: None,
    max_hp: 40,
    speed: Fx::lit("0.12"), // 2.4 tiles/s
    radius: Fx::lit("0.25"),
    cost: 40,
    // Never trained: its host's bay builds it in this long.
    train_ticks: 160, // 8 s
    domain: Domain::Air,
    // 5 damage a second against aircraft.
    weapons: &[WeaponStats {
        damage: 5,
        range: Fx::lit("3"),
        minimum_range: Fx::ZERO,
        cooldown_ticks: 20,
        targets: DomainMask::AIR,
        splash: None,
        indirect: false,
        building_damage: None,
        projectile: None,
    }],
    aggro_range: DRONE_LEASH,
    harvest: None,
    vision: 5,
    requires: &[],
    build_rate: 1,
    demolition: None,
    transport_size: 0,
    transport_capacity: 0,
    turn_rate: 0,
    cruise_turn_rate: 0,
    turret_turn_rate: 8,
    hull_turn_rate: None,
    brace: None,
    target_priority: TargetPriority::Danger,
    crash: None,
    charge_detection: false,
    scouts: false,
    radar: None,
    point_defense: None,
    mend: Mend::Dock,
    structural: false,
    bay: None,
    mounts: None,
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
    traverse: 0,
    construction: Some(ConstructionStats {
        cost: 300,
        build_ticks: 600,
        requires: &[Prerequisite::built(BuildingKind::Fabricator)],
    }),
    radar: None,
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
        building_damage: None,
        projectile: None,
    }],
    traverse: 8,
    construction: Some(ConstructionStats {
        cost: 100,
        build_ticks: 300, // 15 s of builder attention
        requires: &[],
    }),
    radar: None,
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
    traverse: 0,
    construction: Some(ConstructionStats {
        cost: 120,
        build_ticks: 280, // 14 s — the tech window must fit inside the rush window
        requires: &[],
    }),
    radar: None,
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
        building_damage: None,
        projectile: None,
    }],
    traverse: 12,
    construction: Some(ConstructionStats {
        cost: 90,
        build_ticks: 250,
        requires: &[],
    }),
    radar: None,
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
        building_damage: None,
        projectile: Some(ProjectileStats {
            payload: ProjectileKind::Shell,
            speed: Fx::lit("0.30"),
        }),
    }],
    // Competes with the mobile Bombard by trading mobility for a durable
    // firing position rather than losing the comparison on price alone.
    traverse: 3,
    construction: Some(ConstructionStats {
        cost: 210,
        build_ticks: 500,
        requires: &[Prerequisite::built(BuildingKind::Fabricator)],
    }),
    radar: None,
};

const ARRAY: BuildingStats = BuildingStats {
    // A permanent early-warning sentry with the widest radar ring. Unlike
    // a scout it needs no attention, but it cannot move.
    max_hp: 250,
    vision: 9, // the inner ring: true sight
    produces: &[],
    weapons: &[],
    traverse: 0,
    construction: Some(ConstructionStats {
        cost: 90,
        build_ticks: 300,
        requires: &[],
    }),
    radar: Some(20),
};

const RECLAIMER: BuildingStats = BuildingStats {
    max_hp: 300,
    vision: 4,
    produces: &[],
    weapons: &[],
    traverse: 0,
    construction: Some(ConstructionStats {
        cost: 150,
        build_ticks: 350,
        requires: &[],
    }),
    radar: None,
};

const REPAIR_BAY: BuildingStats = BuildingStats {
    max_hp: 400,
    vision: 5,
    produces: &[],
    weapons: &[],
    traverse: 0,
    construction: Some(ConstructionStats {
        cost: 200,
        build_ticks: 350,
        requires: &[],
    }),
    radar: None,
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
    traverse: 0,
    construction: Some(ConstructionStats {
        cost: 200,
        build_ticks: 350,
        requires: &[Prerequisite::built(BuildingKind::Fabricator)],
    }),
    radar: None,
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
    traverse: 0,
    construction: Some(ConstructionStats {
        cost: 400,
        build_ticks: 550,
        requires: &[Prerequisite::built(BuildingKind::Fabricator)],
    }),
    radar: None,
};

const BARRICADE: BuildingStats = BuildingStats {
    max_hp: 400,
    vision: 1,
    produces: &[],
    weapons: &[],
    traverse: 0,
    construction: Some(ConstructionStats {
        cost: 40,
        build_ticks: 120,
        requires: &[],
    }),
    radar: None,
};

const SCUTTLE_CHARGE: BuildingStats = BuildingStats {
    max_hp: 20,
    vision: 1,
    produces: &[],
    weapons: &[],
    traverse: 0,
    construction: Some(ConstructionStats {
        cost: 30,
        build_ticks: 60,
        requires: &[Prerequisite::built(BuildingKind::Fabricator)],
    }),
    radar: None,
};

const SPIRE: BuildingStats = BuildingStats {
    max_hp: 800,
    vision: 5,
    produces: &[],
    weapons: &[],
    traverse: 0,
    construction: Some(ConstructionStats {
        cost: 500,
        build_ticks: 900,
        requires: &[
            Prerequisite::built(BuildingKind::Crucible),
            Prerequisite::upgraded(BuildingKind::Extractor, 1),
        ],
    }),
    radar: None,
};

const MAW_CRADLE: BuildingStats = BuildingStats {
    max_hp: 2000,
    vision: 3,
    produces: &[],
    weapons: &[],
    traverse: 0,
    construction: Some(ConstructionStats {
        cost: MAW.cost,
        // Four Excavators at two work a tick finish in about two minutes.
        build_ticks: 19_200,
        requires: &[
            Prerequisite::built(BuildingKind::Crucible),
            Prerequisite::upgraded(BuildingKind::Extractor, 1),
        ],
    }),
    radar: None,
};

const ROC_CRADLE: BuildingStats = BuildingStats {
    max_hp: 2000,
    vision: 3,
    produces: &[],
    weapons: &[],
    traverse: 0,
    construction: Some(ConstructionStats {
        cost: ROC.cost,
        // Four Excavators at two work a tick finish in under two minutes.
        build_ticks: 17_000,
        requires: &[
            Prerequisite::built(BuildingKind::Crucible),
            Prerequisite::upgraded(BuildingKind::Extractor, 1),
            Prerequisite::built(BuildingKind::Airworks),
        ],
    }),
    radar: None,
};

const SPIRE_ORDNANCE: Ordnance = Ordnance::Silo(SiloStats {
    magazine: MagazineStats {
        capacity: 1,
        round_cost: 600,
        round_ticks: 1200,
        autofill: false,
    },
    turn_rate: 2,
    ready_ticks: 12,
});

/// The one warhead every silo fires.
pub const WARHEAD: WarheadStats = WarheadStats {
    speed: Fx::lit("0.2"),
    peak_damage: 800,
    full_radius: Fx::lit("1.5"),
    outer_radius: Fx::lit("4.0"),
};

const LATTICE: BuildingStats = BuildingStats {
    max_hp: 500,
    vision: 4,
    produces: &[],
    weapons: &[],
    traverse: 0,
    construction: Some(ConstructionStats {
        cost: 400,
        build_ticks: 750,
        requires: &[Prerequisite::built(BuildingKind::Fabricator)],
    }),
    radar: None,
};

const LATTICE_ORDNANCE: Ordnance = Ordnance::Battery(BatteryStats {
    magazine: MagazineStats {
        capacity: 2,
        round_cost: 300,
        round_ticks: 900,
        autofill: true,
    },
    ring: Fx::lit("15"),
});

const EXTRACTOR: BuildingStats = BuildingStats {
    max_hp: 600,
    vision: 4,
    produces: &[],
    weapons: &[],
    // Cheap to restore but hard to hold: the price buys durable income on
    // map-authored ground every player can read.
    traverse: 0,
    construction: Some(ConstructionStats {
        cost: 100,
        build_ticks: 300,
        requires: &[],
    }),
    radar: None,
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
        building_damage: None,
        projectile: None,
    }],
    traverse: 7,
    construction: Some(ConstructionStats {
        cost: 150,
        build_ticks: 300,
        requires: &[Prerequisite::built(BuildingKind::Fabricator)],
    }),
    radar: None,
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
        building_damage: None,
        projectile: None,
    }],
    traverse: 5,
    construction: Some(ConstructionStats {
        cost: 300,
        build_ticks: 500,
        requires: &[Prerequisite::built(BuildingKind::Crucible)],
    }),
    radar: None,
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
        building_damage: None,
        projectile: None,
    }],
    traverse: 14,
    construction: Some(ConstructionStats {
        cost: 120,
        build_ticks: 250,
        requires: &[Prerequisite::built(BuildingKind::Fabricator)],
    }),
    radar: None,
};

const REFINERY: BuildingStats = BuildingStats {
    max_hp: 400,
    vision: 4,
    produces: &[],
    weapons: &[],
    traverse: 0,
    construction: Some(ConstructionStats {
        cost: 300,
        build_ticks: 600,
        requires: &[Prerequisite::built(BuildingKind::Fabricator)],
    }),
    radar: None,
};

const DEEP_BORE: BuildingStats = BuildingStats {
    max_hp: 900,
    vision: 4,
    produces: &[],
    weapons: &[],
    traverse: 0,
    construction: Some(ConstructionStats {
        cost: 350,
        build_ticks: 600,
        requires: &[Prerequisite::built(BuildingKind::Crucible)],
    }),
    radar: None,
};

const DEEP_ARRAY: BuildingStats = BuildingStats {
    max_hp: 300,
    vision: 11,
    produces: &[],
    weapons: &[],
    traverse: 0,
    construction: Some(ConstructionStats {
        cost: 150,
        build_ticks: 300,
        // The Crucible itself requires the Fabricator.
        requires: &[Prerequisite::built(BuildingKind::Crucible)],
    }),
    radar: Some(20),
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
        building_damage: None,
        projectile: Some(ProjectileStats {
            payload: ProjectileKind::Shell,
            speed: Fx::lit("0.30"),
        }),
    }],
    traverse: 3,
    construction: Some(ConstructionStats {
        cost: 250,
        build_ticks: 400,
        requires: &[Prerequisite::built(BuildingKind::Crucible)],
    }),
    radar: None,
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
            UnitKind::Maw => &MAW,
            UnitKind::Roc => &ROC,
            UnitKind::Drone => &DRONE,
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
            BuildingKind::Spire => "spire",
            BuildingKind::Lattice => "lattice",
            BuildingKind::MawCradle => "maw cradle",
            BuildingKind::RocCradle => "roc cradle",
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
            BuildingKind::Spire => &SPIRE,
            BuildingKind::Lattice => &LATTICE,
            BuildingKind::MawCradle => &MAW_CRADLE,
            BuildingKind::RocCradle => &ROC_CRADLE,
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

    /// Whether every seat knows where a building of this kind stands, and
    /// how far along it is, from the first work on its site: a cradle.
    pub const fn public_once_started(self) -> bool {
        self.assembles().is_some()
    }

    /// The tiles an assembly crew works from, as offsets from the anchor:
    /// two off the north face and two off the south, at its corners, one
    /// per crew place. `None` for a kind without a crew.
    pub const fn crew_pads(self) -> Option<[(i32, i32); ASSEMBLY_CREW_CAP]> {
        if self.assembles().is_none() {
            return None;
        }
        let (w, h) = self.size();
        Some([(0, -1), (w - 1, -1), (0, h), (w - 1, h)])
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

/// How close a boarding machine must stand to a small transport's centre
/// before the sling takes it; a wider hull reaches by its
/// [`hull_excess`] more (see [`load_reach`]).
pub const LOAD_REACH: Fx = Fx::lit("1.5");

/// How close a boarding machine must stand to `carrier`'s centre: a riders'
/// hatch reaches as far past a wide hull as a small one's past its own.
pub fn load_reach(carrier: &UnitStats) -> Fx {
    LOAD_REACH + hull_excess(carrier.radius)
}

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

/// Scrap charged at the first unit-repair tick from `progress` on that bills
/// anything: what a bank must hold for a weld to make headway. A weld's
/// opening ticks can bill nothing, so checking the next tick alone would let
/// an empty bank light the torch for free and stall a moment later. Zero only
/// for a kind whose repair never bills.
pub fn next_paid_unit_repair_debit(kind: UnitKind, progress: u32) -> u32 {
    let span = kind.stats().train_ticks.max(1);
    (progress..progress.saturating_add(span))
        .map(|p| unit_repair_debit(kind, p))
        .find(|&due| due > 0)
        .unwrap_or(0)
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

/// Ticks between kiln bites. Every kiln bites on the same pulse, so their
/// order can alternate pulse by pulse.
pub const KILN_BITE_PERIOD: u64 = 4;

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

/// Ticks of caution one Scout leg may spend holding, retreating or detouring
/// around a danger flag before the scout gives that leg up.
pub const SCOUT_HOLD_TICKS: u32 = 160;

/// Chebyshev tiles around a known hostile charge a Scout route keeps clear
/// of. One ring is not enough: the ground follower cuts corners within a
/// tile of the planned path, closer than [`CHARGE_TRIGGER_RADIUS`].
pub const SCOUT_CHARGE_AVOID_RADIUS: i32 = 2;

/// Chebyshev reach of the search for safe ground when a scout finds itself
/// in danger. It covers the widest known envelope: an Avalanche's reach plus
/// [`HARVEST_MOBILE_DANGER_MARGIN`].
pub const SCOUT_RETREAT_RADIUS: i32 = 18;

/// Searches one Scout plan may let hit the expansion cap before it settles
/// for holding.
pub const SCOUT_ROUTE_ATTEMPTS: usize = 4;

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

/// Workers whose labor counts on one assembly cradle at once. Without a
/// cap a mass of Excavators would finish an experimental before anyone
/// could answer it.
pub const ASSEMBLY_CREW_CAP: usize = 4;

/// The widest body whose centre tile stands for all of it. A body this size
/// may overhang the ground around that tile, as every machine up to the
/// Breaker does; only a wider body's hull claims tiles beyond it.
pub const SMALL_BODY_RADIUS: Fx = Fx::lit("0.55");

/// How far a body of `radius` reaches past [`SMALL_BODY_RADIUS`]: exactly
/// zero for every small body, so rules written in terms of it leave small
/// bodies exactly as they were.
pub fn hull_excess(radius: Fx) -> Fx {
    (radius - SMALL_BODY_RADIUS).max(Fx::ZERO)
}

/// The widest collision radius any unit kind has.
pub const MAX_UNIT_RADIUS: Fx = {
    let mut widest = 0;
    let mut i = 0;
    while i < UnitKind::ALL.len() {
        let bits = UnitKind::ALL[i].stats().radius.to_bits();
        if bits > widest {
            widest = bits;
        }
        i += 1;
    }
    Fx::from_bits(widest)
};

// Production spawns a new unit on a tile beside its producer, which a hull
// wider than a Breaker would partly overlap. No producer may train one until
// spawning makes room for the whole hull.
const _: () = {
    let mut b = 0;
    while b < BuildingKind::ALL.len() {
        let tiers = BuildingKind::ALL[b].tiers();
        let mut t = 0;
        while t < tiers.len() {
            let produces = tiers[t].produces;
            let mut u = 0;
            while u < produces.len() {
                assert!(
                    produces[u].stats().radius.to_bits() <= SMALL_BODY_RADIUS.to_bits(),
                    "a producer trains a hull wider than spawning makes room for"
                );
                u += 1;
            }
            t += 1;
        }
        b += 1;
    }
};

// A mounted kind names one mount per weapon, turns its hull as a cruise
// flier does, and never lands, so its hull's heading is the frame its guns'
// arcs are measured in.
const _: () = {
    let mut u = 0;
    while u < UnitKind::ALL.len() {
        let stats = UnitKind::ALL[u].stats();
        if let Some(guns) = stats.mounts {
            assert!(
                guns.mounts.len() == stats.weapons.len() && guns.mounts.len() <= MAX_WEAPONS,
                "a mounted kind names one mount per weapon"
            );
            assert!(
                stats.cruise_turn_rate > 0 && stats.turn_rate == 0,
                "a mounted hull turns as a cruise flier"
            );
            let mut m = 0;
            while m < guns.mounts.len() {
                assert!(
                    guns.mounts[m].arc_half < 128,
                    "an arc leaves a blind quarter"
                );
                m += 1;
            }
        }
        u += 1;
    }
};

// A planted gun's bearing off its hull is a signed compass step, and a gun
// that cannot turn would hold its spades down for aims it never meets.
const _: () = {
    let mut u = 0;
    while u < UnitKind::ALL.len() {
        if let Some(BraceStats {
            traverse: Some(traverse),
            ..
        }) = UnitKind::ALL[u].stats().brace
        {
            assert!(traverse.arc < 128, "a planted arc fits a signed bearing");
            assert!(traverse.rate > 0, "a planted gun turns");
        }
        u += 1;
    }
};

/// The widest [`hull_excess`] any unit kind has.
pub const MAX_HULL_EXCESS: Fx = {
    let excess = MAX_UNIT_RADIUS.to_bits() - SMALL_BODY_RADIUS.to_bits();
    Fx::from_bits(if excess > 0 { excess } else { 0 })
};

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

/// Ticks between the looks a unit fighting a victim it chose itself takes
/// for a better one. Every unit looks on the same ticks, so mirrored armies
/// look alike.
pub const TARGET_RECHECK_TICKS: crate::Tick = 10;

/// How far from the escorted unit an escort looks for a fight, in tiles,
/// capped by its own aggro range. Welders mend escort-party members within
/// the same distance of it.
pub const ESCORT_ENGAGE_RADIUS: Fx = Fx::lit("7");

/// How near a seen enemy aircraft comes to a host before its drones launch,
/// in tiles.
pub const LAUNCH_RADIUS: Fx = Fx::lit("6");

/// How far beyond its host's edge a drone takes a fight, in tiles.
pub const DRONE_LEASH: Fx = Fx::lit("4");

/// The radius of the ring drones hold around their host with nothing to
/// fight, in tiles.
pub const DRONE_ORBIT: Fx = Fx::lit("1.7");

/// Ticks a host's drones stay out after the last threat, about 3 s.
pub const RECALL_TICKS: u16 = 60;

/// How far from the escorted unit an escort's victim may stray before the
/// escort breaks off, firing included, and returns to its slot. Wider than
/// [`ESCORT_ENGAGE_RADIUS`] so a fight it just picked never ends on the next
/// tick.
pub const ESCORT_BREAK_RADIUS: Fx = Fx::lit("10");
