//! The complete game state and its invariants.
//!
//! Everything that affects game outcomes lives in [`State`] and is
//! serializable; [`State::hash`] fingerprints it canonically. Anything not
//! in here (camera, selection, interpolation) is presentation and belongs to
//! the shell.
//!
//! Invariants:
//! - `units` and `buildings` stay sorted by id (ids are assigned
//!   monotonically and entities are only ever appended or `retain`ed).
//! - A dead entity (hp 0) survives at most until the cleanup phase of the
//!   tick that killed it.
//! - `result` is set at most once; once set, ticks are frozen no-ops.

mod goal;
mod order_key;
mod placement;
mod targeting;
pub use goal::{Aim, Goal};
pub use order_key::OrderKey;
pub use targeting::AttackView;

use crate::ids::{BuildingId, PlayerId, Target, UnitId};
use crate::map::{MAX_MAP_EDGE, Map};
use crate::stats::{BuildingKind, ProjectileKind, UnitKind};
use chassis::Tick;
use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::{TilePos, cell_count};
use serde::{Deserialize, Serialize};

/// Whether a seat whose economy is stranded may begin a recovery cycle, or
/// the package the current cycle captured.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "recovery", rename_all = "snake_case", deny_unknown_fields)]
pub enum Recovery {
    /// A new cycle may begin. A real Harvester deposit returns here;
    /// merely training, cancelling, or losing a worker does not.
    #[default]
    Ready,
    /// A cycle is under way.
    Active {
        /// Bank target captured when the cycle began. It is fixed for the
        /// cycle so selling, queueing, or losing a screen cannot expand
        /// the entitlement after the fact.
        target: u16,
        /// Emergency scrap still available. The allowance is finite:
        /// spending the credited package cannot make the Foundry mint it a
        /// second time.
        allowance: u16,
    },
}

/// A participant in the match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Player {
    /// Display name.
    pub name: String,
    /// Team index: seats sharing one share vision, never fight each
    /// other, and stand or fall together.
    pub team: u8,
    /// Scrap in the bank.
    pub scrap: u32,
    /// The seat's stranded-economy recovery cycle.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub recovery: Recovery,
    /// Whether this seat conceded ([`crate::Command::Surrender`]): its
    /// Foundries no longer keep its team in the match and its commands
    /// reject, while its machines play out their brains as remnants.
    pub resigned: bool,
    /// The tick this seat first stopped counting — resigned, or holding
    /// no Foundry at all — recorded once and never cleared. The FFA
    /// scoreboard's placement key: later elimination places higher.
    pub eliminated_at: Option<crate::Tick>,
}

/// How the match ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum GameResult {
    /// One team still holds a Foundry.
    Victory {
        /// The surviving team (see [`crate::State::winners`] for its
        /// seats).
        team: u8,
    },
    /// Every team's last Foundry died on the same tick.
    Draw,
}

/// A unit's current intent. The brain phase turns intent into paths,
/// attacks, and extraction; commands only ever set intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "order", rename_all = "snake_case")]
pub enum Order {
    /// Stand around. Combat units auto-acquire targets in aggro range.
    Idle,
    /// Run to a tile without firing or engaging, then go idle.
    Run {
        /// The clicked tile, and where around it this unit is headed.
        goal: Goal,
    },
    /// Work a bounded salvage zone, hauling to the nearest Foundry until
    /// every safe remembered source near the clicked anchor is exhausted.
    Harvest {
        /// The node or wreck tile currently being worked.
        node: TilePos,
        /// The source the player clicked: the fixed center of the work
        /// zone.
        anchor: TilePos,
        /// The zone was observed exhausted or unsafe. Sticky until the
        /// Harvester deposits its cargo, reaches a built Foundry, and
        /// advances its queued program.
        #[serde(default, skip_serializing_if = "core::ops::Not::not")]
        withdrawing: bool,
    },
    /// Chase and attack one target until it is gone.
    Attack {
        /// The victim.
        target: crate::AttackTarget,
        /// An explicit commitment may pursue an anonymous contact; automatic
        /// engagements may only fire while radar remains in reach.
        #[serde(default, skip_serializing_if = "core::ops::Not::not")]
        pursue: bool,
        /// The program the engagement interrupted, taken back once the
        /// victim is gone. `None` for a plain attack order.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resume: Option<Resume>,
    },
    /// Walk to an unfinished own site and stand it up (harvesters only).
    Build {
        /// The site under construction.
        site: crate::ids::BuildingId,
    },
    /// Walk adjacent to a damaged own built building and weld it back
    /// toward full (welders only; billed per hp welded).
    Repair {
        /// The patient.
        building: crate::ids::BuildingId,
    },
    /// March to a tile, engaging anything encountered on the way — the
    /// stance for actually fighting, as opposed to [`Order::Run`]'s
    /// oblivious walk.
    Hunt {
        /// The clicked tile, and where around it this unit is headed.
        goal: Goal,
    },
    /// Walk adjacent to an own built building and strip it down for a
    /// partial refund (harvesters only; Foundries refuse). Drains
    /// buffer like damage and resolve after it — fire wins ties, and
    /// fire-forfeited hp refunds nothing.
    Salvage {
        /// The building coming down.
        building: crate::ids::BuildingId,
    },
    /// Approach the paid provisional scaffold at this kind and anchor.
    /// Visibility activates its physical footprint and converts every crew
    /// commitment to `Build` without another payment.
    Found {
        /// What to construct on arrival.
        kind: crate::stats::BuildingKind,
        /// Top-left tile of the claimed footprint.
        anchor: TilePos,
    },
    /// Chase a wounded own ground unit and weld it back toward full
    /// (welders only; billed per hp against the patient's cost).
    /// The weld ticks only while welder and patient both stand still
    /// within their combined hull radii plus [`crate::stats::WORK_REACH`].
    RepairUnit {
        /// The patient.
        unit: crate::ids::UnitId,
    },
    /// Move to a tile without chasing or stopping, taking only
    /// primary-weapon shots that are already in range and visible.
    Advance {
        /// The clicked tile, and where around it this unit is headed.
        goal: Goal,
    },
    /// Walk within [`crate::stats::load_reach`] of an own transport and
    /// board it: the machine leaves the world and rides as cargo.
    Board {
        /// The carrier to climb onto.
        transport: crate::ids::UnitId,
    },
    /// Fly to a tile and set every carried machine down on open ground
    /// around it, or around the nearest reachable tile when the drop point
    /// cannot be reached.
    Unload {
        /// The drop point.
        at: Goal,
        /// Whether the drop ring scans half-turned, in the frame of the
        /// transport's approach when the order was issued, so mirrored
        /// drops set riders on mirrored tiles.
        #[serde(default, skip_serializing_if = "core::ops::Not::not")]
        reverse: bool,
    },
    /// Fly a run-in onto a ground tile and set the airframe down on its
    /// center.
    Land {
        /// The tile to park on.
        goal: TilePos,
        /// The clicked tile of the walk this landing took over, when it
        /// took one over. Automatic landings have none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<TilePos>,
    },
    /// Deliver carried scrap to this Foundry, optionally welding it afterward.
    ReturnCargo {
        /// The owned drop-off chosen when the command was accepted.
        foundry: BuildingId,
        /// Repair this Foundry after delivery if it is still damaged.
        repair: bool,
    },
    /// Hold a slot around an own unit at this machine's own top speed,
    /// fighting threats near it and, for welders, mending it and its other
    /// escorts whenever it stops. Ends when the escorted unit leaves play.
    Accompany {
        /// The escorted unit.
        unit: UnitId,
        /// The wounded escort-party member a welder is mending, which owns
        /// the welder's billing meter.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weld: Option<UnitId>,
    },
    /// Go to an own completed Crucible and stand, hover, or, for a
    /// turn-limited airframe, land beside it while it is dismantled for a
    /// refund. A transport first sets its riders down there.
    Retire {
        /// The Crucible chosen when the order was issued, or the nearest
        /// one since that one fell.
        crucible: BuildingId,
        /// The tile a turn-limited airframe lands on, once it has one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pad: Option<TilePos>,
    },
    /// Scout a tile with the owner's knowledge alone: go there while it is
    /// safe, otherwise stop at the safe tile nearest it that still sees it.
    /// [`Unit::progress`] counts the leg's ticks of caution.
    Scout {
        /// The tile to see.
        tile: TilePos,
    },
    /// Walk toward a tile, welding wounded own ground machines and damaged
    /// own buildings met on the way (workers only): a patrolling worker's
    /// leg, or, outside a looping program, an idle worker's trip out from
    /// its post and back. The torch works and bills like
    /// [`Order::RepairUnit`]'s or [`Order::Repair`]'s, but a patient the
    /// welder cannot pay for, reach, or find standing still is let go
    /// without a stall.
    Tend {
        /// Where the walk ends: the patrol waypoint, or the post an idle
        /// worker returns to.
        goal: Goal,
        /// The patient being welded, which owns the welder's billing meter.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weld: Option<Target>,
    },
}

/// Where a producer's finished units report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rally {
    /// A tile: harvesters mine a rallied scrap node, combat units hunt there,
    /// and everyone else walks.
    Tile(TilePos),
    /// An own unit, in the world or riding a transport, that every newborn
    /// escorts. The rally clears when that unit leaves play, so it never
    /// names one that is gone at a tick boundary.
    Unit(UnitId),
}

/// The program an engagement interrupted, taken back once its victim is gone
/// or out of bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resume {
    /// The march toward this goal: a hunt or a patrol leg.
    Hunt(Goal),
    /// Escort duty around this unit.
    Accompany(UnitId),
}

impl Resume {
    /// The march this engagement resumes, if it resumes one.
    pub fn goal(&self) -> Option<Goal> {
        match *self {
            Resume::Hunt(goal) => Some(goal),
            Resume::Accompany(_) => None,
        }
    }

    /// [`Resume::goal`], for resolving a pending slot or endpoint.
    pub(crate) fn goal_mut(&mut self) -> Option<&mut Goal> {
        match self {
            Resume::Hunt(goal) => Some(goal),
            Resume::Accompany(_) => None,
        }
    }

    /// The order that takes over once the engagement ends.
    fn order(self) -> Order {
        match self {
            Resume::Hunt(goal) => Order::Hunt { goal },
            Resume::Accompany(unit) => Order::Accompany { unit, weld: None },
        }
    }
}

impl Order {
    /// The unit this order escorts: escort duty itself, or a fight escort
    /// duty picked.
    pub fn escorted(&self) -> Option<UnitId> {
        match *self {
            Order::Accompany { unit, .. }
            | Order::Attack {
                resume: Some(Resume::Accompany(unit)),
                ..
            } => Some(unit),
            _ => None,
        }
    }

    /// The machine this order field-welds: a commanded weld, or the
    /// patient an escort or tending welder has taken on.
    pub fn torch(&self) -> Option<UnitId> {
        match *self {
            Order::RepairUnit { unit }
            | Order::Accompany {
                weld: Some(unit), ..
            }
            | Order::Tend {
                weld: Some(Target::Unit(unit)),
                ..
            } => Some(unit),
            _ => None,
        }
    }

    /// The goal of a walking order: Run, Hunt, Advance, Unload, or Tend.
    pub(crate) fn walk_goal(&self) -> Option<Goal> {
        match *self {
            Order::Run { goal }
            | Order::Hunt { goal }
            | Order::Advance { goal }
            | Order::Tend { goal, .. } => Some(goal),
            Order::Unload { at, .. } => Some(at),
            _ => None,
        }
    }

    /// [`Order::walk_goal`], for storing a resolved endpoint.
    pub(crate) fn walk_goal_mut(&mut self) -> Option<&mut Goal> {
        match self {
            Order::Run { goal }
            | Order::Hunt { goal }
            | Order::Advance { goal }
            | Order::Tend { goal, .. } => Some(goal),
            Order::Unload { at, .. } => Some(at),
            _ => None,
        }
    }

    /// Whether issuing `other` to a unit already running `self` continues
    /// the same order rather than starting a new one. A walking order
    /// matches on its variant and clicked tile, and [`Order::reissue`] then
    /// takes the new aim. An escort matches on its escorted unit and keeps
    /// its weld. Every other order must match exactly.
    pub(crate) fn reissue_matches(&self, other: &Order) -> bool {
        match (self, other) {
            (Order::Run { goal: a }, Order::Run { goal: b })
            | (Order::Hunt { goal: a }, Order::Hunt { goal: b })
            | (Order::Advance { goal: a }, Order::Advance { goal: b })
            | (Order::Unload { at: a, .. }, Order::Unload { at: b, .. }) => a.tile() == b.tile(),
            (Order::Accompany { unit: a, .. }, Order::Accompany { unit: b, .. }) => a == b,
            _ => self == other,
        }
    }

    /// Continues this order as the matching re-issue `other`: a walk takes
    /// the new aim and keeps its endpoint only while its target is
    /// unchanged, and an unload takes the new drop frame. Callers check
    /// [`Order::reissue_matches`] first.
    pub(crate) fn reissue(&mut self, other: Order) {
        if let (Some(goal), Some(new)) = (self.walk_goal_mut(), other.walk_goal()) {
            goal.adopt(new);
        }
        if let (Order::Unload { reverse, .. }, Order::Unload { reverse: new, .. }) = (self, other) {
            *reverse = new;
        }
    }
}

/// A self-acquired fight's tether: where the machine stood when it
/// picked the fight itself, and how much chase it has left. Only idle
/// auto-acquisition and retaliation ever set one — an explicit player
/// attack is a commitment and carries no leash — and any new command
/// clears it. The tether binds *locomotion*, not the trigger: chasing
/// spends patience and respects the radius; standing in range and
/// firing costs nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Leash {
    /// The station to walk back to when the fight ends or the tether
    /// runs out.
    pub anchor: TilePos,
    /// Chase ticks the guard may spend beyond the radius, granted only by
    /// a joined fight: refreshed to [`crate::stats::LEASH_PATIENCE`] every
    /// time the guard reaches its firing stance or answers a hit, and spent
    /// only while chasing past the radius. Bait that never comes in reach
    /// grants none, so its chaser breaks at the radius line. Inside the
    /// radius the guard fights freely.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub patience: u16,
    /// Ticks left standing at the post before the guard looks for the
    /// next fight; the leash clears when it reaches zero. Nonzero only
    /// while idle — the answer to an enemy dancing at the aggro edge.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub cooldown: u16,
}

/// An in-progress walk along an A* path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathFollow {
    /// Exact ground-work endpoint within a short final approach of the last tile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_point: Option<Vec2Fx>,
    /// Final tile, used to detect stale paths when intent changes.
    pub goal: TilePos,
    /// Remaining waypoints from A* (start tile excluded).
    pub waypoints: Vec<TilePos>,
    /// Index of the waypoint currently steered toward.
    pub next: u32,
    /// A route off occupied ground that friendly footprints do not bound.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub clearing: bool,
}

/// A point defender's ready charges and its progress toward the next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointDefense {
    /// Charges ready to fire.
    pub charges: u8,
    /// Ticks spent restoring the next charge; held at zero while full.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub recharge: u16,
}

impl PointDefense {
    fn full(stats: crate::stats::PointDefenseStats) -> Self {
        Self {
            charges: stats.charges,
            recharge: 0,
        }
    }
}

/// A mounted hull's gunners and helm.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mounts {
    /// Each gun's bearing against the nose, indexed like the kind's mounts
    /// and always inside its arc, so a gun swings with the hull. Slots past
    /// the mounts stay zero.
    pub bearings: [u8; crate::stats::MAX_WEAPONS],
    /// How an idle hull keeps a facing it turned to.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub facing: IdleFacing,
}

impl Mounts {
    /// Every gun dead ahead.
    pub(crate) fn resting(_: crate::stats::MountedGuns) -> Self {
        Self {
            bearings: [0; crate::stats::MAX_WEAPONS],
            facing: IdleFacing::Free,
        }
    }
}

/// A host's drone bay: the drones docked out of reach, those paid for and
/// building, and how long its launched drones stay out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DroneBay {
    /// Drones docked aboard.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub docked: u8,
    /// Drones paid for and not yet built, the first building.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub ordered: u8,
    /// Ticks spent on the drone building.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub progress: u32,
    /// Whether the bay orders a drone on its own while it has room.
    pub autofill: bool,
    /// Ticks the host's drones stay out after the last threat: while above
    /// zero docked drones launch, and at zero flying ones come home.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub alert: u16,
}

impl DroneBay {
    /// An empty bay with the kind's refill setting.
    fn empty(stats: crate::stats::BayStats) -> Self {
        Self {
            autofill: stats.autofill,
            ..Self::default()
        }
    }
}

/// Whether an idle mounted hull may turn to feed a gun that has nothing to
/// shoot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "facing", rename_all = "snake_case", deny_unknown_fields)]
pub enum IdleFacing {
    /// It may start a turn.
    #[default]
    Free,
    /// It turned last tick and may go on turning.
    Turning,
    /// It keeps the facing it turned to for these many more ticks.
    Holding {
        /// Ticks left.
        ticks: core::num::NonZeroU8,
    },
}

/// What a harvesting machine has aboard and how its delivery stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Worker {
    /// Scrap on board.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub carrying: u32,
    /// Cargo release in progress, separate from extraction and welding
    /// meters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unloading: Option<Unloading>,
    /// While held by danger, the tick from which it searches again. See
    /// [`crate::stats::HARVEST_DANGER_RETRY_TICKS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub danger_retry_at: Option<crate::Tick>,
    /// The owner switched idle welding off: an idle worker no longer goes
    /// out on its own to weld wounded machines near its post. A patrol
    /// welds either way.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub idle_weld_off: bool,
}

/// An uninterrupted cargo release at a completed Foundry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unloading {
    /// Destination held for this release.
    pub foundry: BuildingId,
    /// Completed work ticks; cargo remains aboard until the final tick.
    pub elapsed: u8,
}

/// A mobile entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit {
    /// Stable id; `units` is sorted by it.
    pub id: UnitId,
    /// Owner.
    pub player: PlayerId,
    /// What kind of machine this is.
    pub kind: UnitKind,
    /// World position (tile units).
    pub pos: Vec2Fx,
    /// How the body moves, by its kind's movement class.
    pub motor: Motor,
    /// Current hit points.
    pub hp: u32,
    /// The tick it last took damage. A Repair Bay heals a machine fast only
    /// once this is long enough past.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub damaged_at: Option<Tick>,
    /// The harvest gear's load and delivery, for exactly the kinds that
    /// harvest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker: Option<Worker>,
    /// Ticks until each weapon may fire again, indexed like
    /// `kind.stats().weapons` (unused slots stay zero).
    pub cooldowns: [u32; crate::stats::MAX_WEAPONS],
    /// Order-specific counter (extraction progress).
    pub progress: u32,
    /// Current intent.
    pub order: Order,
    /// Orders waiting behind the active one; completing the active order
    /// pops the front. With [`Unit::looping`] set, the finished order
    /// rotates to the back instead — that cycle is a patrol.
    #[serde(default, skip_serializing_if = "std::collections::VecDeque::is_empty")]
    pub queue: std::collections::VecDeque<Order>,
    /// Whether the queue cycles (patrol) instead of draining.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub looping: bool,
    /// Current walk, if any.
    pub path: Option<PathFollow>,
    /// The tether of a self-acquired fight, if one is live.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leash: Option<Leash>,
    /// Ticks spent standing idle with nothing to fight. A unit is a
    /// stationed guard (its acquisitions tether) only past
    /// [`crate::stats::LEASH_STATION_TICKS`]. A unit cycling through idle
    /// mid-battle re-acquires unleashed, which keeps the tether from
    /// deciding army fights.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub settled: u16,
    /// Compass step (of 256, see [`chassis::compass`]) this body faces,
    /// or Buzzard's turret bearing. Ground chassis and turn-limited aircraft
    /// steer by it. Every `u8` is a valid compass heading.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub heading: u8,
    /// Independent ground gun bearing; absent, the gun follows the hull. A gun
    /// with nothing to aim at and every reload done turns back to the nose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turret_heading: Option<u8>,
    /// Machines riding aboard this transport.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cargo: Vec<Rider>,
    /// Ticks a machine whose radar unfolds only at rest has stood still,
    /// up to its kind's deploy time; driving of its own resets it. See
    /// [`Unit::radar`].
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub radar_deploy: u8,
    /// Point-defense charges, for exactly the kinds that carry point
    /// defense.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point_defense: Option<PointDefense>,
    /// Gun bearings and idle facing, for exactly the kinds with mounted
    /// guns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mounts: Option<Mounts>,
    /// The drone bay, for exactly the kinds with one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bay: Option<DroneBay>,
    /// The host a drone flies for, for exactly the drone kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<UnitId>,
}

fn is_zero_motion(motion: &Vec2Fx) -> bool {
    *motion == Vec2Fx::ZERO
}

/// How a body moves. Ground kinds drive or, for a Bombard, stand braced;
/// aircraft fly, and the turn-limited ones may park.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "motor", rename_all = "snake_case", deny_unknown_fields)]
pub enum Motor {
    /// A ground chassis under its own power.
    Ground {
        /// Motor speed; overlap corrections do not contribute to it.
        #[serde(default, skip_serializing_if = "crate::is_default")]
        speed: Fx,
        /// Running ticks in which contact cancelled most of this body's
        /// intended progress along its route; reaching
        /// [`crate::stats::STALL_REPLAN_TICKS`] drops the route for a fresh
        /// plan.
        #[serde(default, skip_serializing_if = "crate::is_default")]
        stall_ticks: u8,
    },
    /// A braced gun's spades deploying or planted: it stands still until
    /// they stow.
    Braced {
        /// Deployment, up to fully planted.
        ticks: core::num::NonZeroU8,
        /// The gun's bearing off the hull in compass steps, within its kind's
        /// planted traverse. It swings back as the spades retract.
        #[serde(default, skip_serializing_if = "crate::is_default")]
        traverse: i8,
    },
    /// An aircraft in flight.
    Airborne {
        /// Last airborne displacement per tick, retained for crash
        /// momentum.
        #[serde(default, skip_serializing_if = "is_zero_motion")]
        motion: Vec2Fx,
    },
    /// A turn-limited aircraft parked on the ground at its tile center. It
    /// is physically a ground body (see [`Unit::domain`]) until an order
    /// lifts it off again.
    Landed,
}

impl Motor {
    /// A body of `kind` standing still.
    fn at_rest(kind: UnitKind) -> Self {
        match kind.stats().domain {
            crate::stats::Domain::Ground => Self::Ground {
                speed: Fx::ZERO,
                stall_ticks: 0,
            },
            crate::stats::Domain::Air => Self::Airborne {
                motion: Vec2Fx::ZERO,
            },
        }
    }

    /// Whether a body of `kind` may move this way.
    fn fits(self, kind: UnitKind) -> bool {
        let stats = kind.stats();
        match self {
            Self::Ground { .. } => stats.domain == crate::stats::Domain::Ground,
            Self::Braced { .. } => stats.brace.is_some(),
            Self::Airborne { .. } => stats.domain == crate::stats::Domain::Air,
            Self::Landed => stats.can_land(),
        }
    }
}

/// How fast a Repair Bay heals one of its patients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairPace {
    /// Undamaged for [`crate::stats::REPAIR_BAY_CALM_TICKS`]: a share of
    /// max hp per second.
    Fast,
    /// Recently damaged: [`crate::stats::REPAIR_BAY_STEP`] per pulse.
    Slow,
}

/// A machine riding aboard a transport. Cargo lives OUTSIDE the world's
/// unit list: nothing can see, target, collide with, or command a carried
/// machine, and it contributes no vision. It keeps its id (ids are never
/// reused), its owner is its carrier's, and it dies with the carrier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rider {
    /// The id it had walking and will have again.
    pub id: UnitId,
    /// What kind of machine this is.
    pub kind: UnitKind,
    /// Current hit points.
    pub hp: u32,
    /// Scrap it carried aboard.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub carrying: u32,
    /// Weapon cooldowns, frozen while carried.
    pub cooldowns: [u32; crate::stats::MAX_WEAPONS],
    /// The heading it boarded with.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub heading: u8,
    /// Its independent gun bearing, if it had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turret_heading: Option<u8>,
    /// Point-defense charges and recharge, frozen while carried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point_defense: Option<PointDefense>,
    /// The tick it last took damage, kept through the ride.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub damaged_at: Option<Tick>,
    /// A worker's idle welding switch, kept through the ride.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub idle_weld_off: bool,
}

impl Rider {
    /// Takes a walking unit aboard. Everything a carried machine cannot
    /// keep (its orders, route, tether, motor, and work) is left behind.
    pub(crate) fn board(unit: &Unit) -> Self {
        let Unit {
            id,
            kind,
            hp,
            damaged_at,
            cooldowns,
            heading,
            turret_heading,
            point_defense,
            ..
        } = *unit;
        Self {
            id,
            kind,
            hp,
            carrying: unit.carrying(),
            cooldowns,
            heading,
            turret_heading,
            point_defense,
            damaged_at,
            idle_weld_off: unit.worker.is_some_and(|worker| worker.idle_weld_off),
        }
    }

    /// Sets the rider down at `pos` as an idle machine at rest.
    pub(crate) fn disembark(self, player: PlayerId, pos: Vec2Fx) -> Unit {
        let mut unit = Unit {
            hp: self.hp,
            damaged_at: self.damaged_at,
            cooldowns: self.cooldowns,
            turret_heading: self.turret_heading,
            point_defense: self.point_defense,
            ..Unit::at_rest(self.id, player, self.kind, pos, self.heading)
        };
        if let Some(worker) = &mut unit.worker {
            worker.carrying = self.carrying;
            worker.idle_weld_off = self.idle_weld_off;
        }
        unit
    }
}

impl Unit {
    /// Scrap on board; none for a machine that cannot harvest.
    pub fn carrying(&self) -> u32 {
        self.worker.map_or(0, |worker| worker.carrying)
    }

    /// The cargo release under way, if any.
    pub fn unloading(&self) -> Option<Unloading> {
        self.worker.and_then(|worker| worker.unloading)
    }

    /// Whether this worker goes out on its own, while idle, to weld wounded
    /// machines near its post. Never true for a machine without harvest
    /// gear.
    pub fn idle_weld(&self) -> bool {
        self.worker.is_some_and(|worker| !worker.idle_weld_off)
    }

    /// The tick a danger-held harvester searches again, if it is held.
    pub fn danger_retry_at(&self) -> Option<crate::Tick> {
        self.worker.and_then(|worker| worker.danger_retry_at)
    }

    /// The harvest gear of a machine whose work needs it.
    pub(crate) fn worker_mut(&mut self) -> &mut Worker {
        self.worker
            .as_mut()
            .expect("only machines with harvest gear do harvest work")
    }

    /// Abandons any cargo release under way.
    pub(crate) fn end_release(&mut self) {
        if let Some(worker) = &mut self.worker {
            worker.unloading = None;
        }
    }

    /// Lets a danger-held harvester search again at once.
    pub(crate) fn clear_danger_hold(&mut self) {
        if let Some(worker) = &mut self.worker {
            worker.danger_retry_at = None;
        }
    }

    /// A healthy, idle machine standing at `pos` with nothing aboard.
    fn at_rest(id: UnitId, player: PlayerId, kind: UnitKind, pos: Vec2Fx, heading: u8) -> Self {
        Self {
            id,
            player,
            kind,
            pos,
            motor: Motor::at_rest(kind),
            hp: kind.stats().max_hp,
            damaged_at: None,
            worker: kind.stats().harvest.map(|_| Worker::default()),
            cooldowns: [0; crate::stats::MAX_WEAPONS],
            turret_heading: None,
            progress: 0,
            order: Order::Idle,
            queue: std::collections::VecDeque::new(),
            looping: false,
            path: None,
            leash: None,
            settled: 0,
            heading,
            cargo: Vec::new(),
            radar_deploy: 0,
            point_defense: kind.stats().point_defense.map(PointDefense::full),
            mounts: kind.stats().mounts.map(Mounts::resting),
            bay: kind.stats().bay.map(DroneBay::empty),
            host: None,
        }
    }

    /// Compass bearing used to aim this unit's primary weapon.
    pub fn weapon_heading(&self) -> u8 {
        self.turret_heading
            .unwrap_or_else(|| self.heading.wrapping_add_signed(self.gun_traverse()))
    }

    /// The planted gun's bearing off the hull in compass steps: zero unless
    /// the spades are out.
    pub fn gun_traverse(&self) -> i8 {
        match self.motor {
            Motor::Braced { traverse, .. } => traverse,
            Motor::Ground { .. } | Motor::Airborne { .. } | Motor::Landed => 0,
        }
    }

    /// Turns a planted gun to `value` compass steps off the hull.
    pub(crate) fn set_gun_traverse(&mut self, value: i8) {
        debug_assert!(
            matches!(self.motor, Motor::Braced { .. }),
            "only a planted gun swings"
        );
        if let Motor::Braced { traverse, .. } = &mut self.motor {
            *traverse = value;
        }
    }

    /// The world bearing of the mounted gun in weapon slot `slot`, if this
    /// machine has one there.
    pub fn mount_bearing(&self, slot: usize) -> Option<u8> {
        let guns = self.kind.stats().mounts?;
        guns.mounts.get(slot)?;
        let bearing = self.mounts?.bearings[slot];
        Some(self.heading.wrapping_add(bearing))
    }

    pub(crate) fn retract_braces(&mut self) {
        let stats = self.kind.stats();
        if let Some(brace) = stats.brace
            && self.cooldowns[0] <= stats.weapons[0].cooldown_ticks - brace.recoil_ticks
        {
            let (before, after) = (
                self.braces(),
                self.braces().saturating_sub(brace.retract_per_tick),
            );
            // The gun swings back with the spades, so it is square to the
            // hull once they stow.
            if before > 0 {
                let traverse =
                    i16::from(self.gun_traverse()) * i16::from(after) / i16::from(before);
                self.set_gun_traverse(i8::try_from(traverse).expect("shrinks toward the hull"));
            }
            self.set_braces(after);
        }
    }

    /// Ground motor speed; zero for a body not driving.
    pub fn drive_speed(&self) -> Fx {
        match self.motor {
            Motor::Ground { speed, .. } => speed,
            Motor::Braced { .. } | Motor::Airborne { .. } | Motor::Landed => Fx::ZERO,
        }
    }

    /// Sets a driving body's motor speed.
    pub(crate) fn set_drive_speed(&mut self, value: Fx) {
        debug_assert!(matches!(self.motor, Motor::Ground { .. }));
        if let Motor::Ground { speed, .. } = &mut self.motor {
            *speed = value;
        }
    }

    /// The radius of this machine's radar ring while it is up: always for
    /// a kind whose radar needs no deployment, otherwise once it has stood
    /// still for the kind's deploy time.
    pub fn radar(&self) -> Option<i32> {
        let radar = self.kind.stats().radar?;
        (self.radar_deploy == radar.deploy_ticks).then_some(radar.radius)
    }

    /// Spade deployment, from stowed zero to fully planted.
    pub fn braces(&self) -> u8 {
        match self.motor {
            Motor::Braced { ticks, .. } => ticks.get(),
            Motor::Ground { .. } | Motor::Airborne { .. } | Motor::Landed => 0,
        }
    }

    /// Deploys or stows a standing gun's spades, leaving a planted gun's
    /// bearing where it is. Stowing them returns it to its motor at rest;
    /// deploying drops any stall count, which a body standing braced never
    /// keeps.
    pub(crate) fn set_braces(&mut self, ticks: u8) {
        match core::num::NonZeroU8::new(ticks) {
            Some(ticks) => {
                debug_assert_eq!(self.drive_speed(), Fx::ZERO, "only a stopped body braces");
                let traverse = self.gun_traverse();
                self.motor = Motor::Braced { ticks, traverse };
            }
            None if matches!(self.motor, Motor::Braced { .. }) => {
                self.motor = Motor::at_rest(self.kind);
            }
            None => {}
        }
    }

    /// Whether this airframe is parked on the ground.
    pub fn landed(&self) -> bool {
        self.motor == Motor::Landed
    }

    /// Parks a turn-limited airframe where it stands.
    pub(crate) fn touch_down(&mut self) {
        self.motor = Motor::Landed;
    }

    /// Lifts a parked airframe back into the air; any other body is left
    /// as it is.
    pub(crate) fn lift_off(&mut self) {
        if self.landed() {
            self.motor = Motor::Airborne {
                motion: Vec2Fx::ZERO,
            };
        }
    }

    /// The last airborne displacement per tick, kept for crash momentum.
    pub fn air_motion(&self) -> Vec2Fx {
        match self.motor {
            Motor::Airborne { motion } => motion,
            Motor::Ground { .. } | Motor::Braced { .. } | Motor::Landed => Vec2Fx::ZERO,
        }
    }

    /// Running ticks of contact-cancelled progress along the route.
    pub fn stall_ticks(&self) -> u8 {
        match self.motor {
            Motor::Ground { stall_ticks, .. } => stall_ticks,
            Motor::Braced { .. } | Motor::Airborne { .. } | Motor::Landed => 0,
        }
    }

    /// The tile this unit currently occupies.
    pub fn tile(&self) -> TilePos {
        TilePos::containing(self.pos)
    }

    /// The tiles this body claims: its centre tile for a small body, and
    /// every tile its hull reaches for a wider one. See
    /// [`crate::geometry::body_tiles`].
    pub fn body_tiles(&self) -> crate::geometry::BodyTiles {
        crate::geometry::body_tiles(self.pos, self.kind.stats().radius)
    }

    /// Whether this body is within tool reach of a rectangular resource footprint.
    pub fn in_work_reach(&self, anchor: TilePos, size: (i32, i32)) -> bool {
        let reach = crate::geometry::work_approach_distance(self.kind.stats().radius)
            + crate::stats::WORK_REACH
            - crate::stats::WORK_APPROACH_GAP
            + const { Fx::lit("0.04") };
        self.pos
            .dist_sq(crate::geometry::footprint_contact(self.pos, anchor, size))
            <= reach * reach
            && crate::tick::tile_adjacent_to_rect(self.tile(), anchor, size)
    }

    /// Whether this body's tool can reach a patient's hull.
    pub fn in_repair_reach(&self, patient: &Unit) -> bool {
        let reach =
            self.kind.stats().radius + patient.kind.stats().radius + crate::stats::WORK_REACH;
        self.pos.dist_sq(patient.pos) <= reach * reach
    }

    /// Whether ground work may advance without the motor moving this body.
    pub fn work_stopped(&self) -> bool {
        self.path.is_none() && self.drive_speed() == Fx::ZERO
    }

    /// Whether this body holds still for a field weld: stopped, and not on
    /// its way to found a site.
    pub fn still_for_weld(&self) -> bool {
        self.work_stopped() && !matches!(self.order, Order::Found { .. })
    }

    /// The movement layer this body occupies right now: a landed airframe
    /// is a ground body for targeting, collision, charges, and footprints,
    /// whatever its kind flies as.
    pub fn domain(&self) -> crate::stats::Domain {
        if self.landed() {
            crate::stats::Domain::Ground
        } else {
            self.kind.stats().domain
        }
    }

    /// Whether a landed airframe's program leaves it on the ground: idle,
    /// or a landing on the very tile it rests on. Anything else lifts it
    /// off at the next brain tick.
    pub(crate) fn stays_parked(&self) -> bool {
        match self.order {
            Order::Idle => true,
            Order::Land { goal, .. }
            | Order::Retire {
                pad: Some(goal), ..
            } => goal == self.tile(),
            _ => false,
        }
    }

    /// Whether this machine is on its way to, or at, a Crucible to be
    /// dismantled. No repair source mends it: welding it up would only
    /// spend scrap the retirement refunds at a lower rate.
    pub fn retiring(&self) -> bool {
        matches!(self.order, Order::Retire { .. })
    }

    /// Whether this worker is out welding on its own from an idle post: a
    /// [`Order::Tend`] outside a looping program. The trip is busy work,
    /// but no player's order waits behind it: a queued command replaces it
    /// as it would an idle program.
    pub fn on_idle_trip(&self) -> bool {
        matches!(self.order, Order::Tend { .. }) && !self.looping
    }

    /// Ends the active order cleanly: a looping program rotates it to the
    /// back (patrol), a plain queue drains, an empty queue idles. A rotated
    /// leg forgets its endpoint so the next lap resolves it afresh.
    pub(crate) fn advance_queue(&mut self) {
        let mut finished = std::mem::replace(&mut self.order, Order::Idle);
        if self.looping {
            if let Some(goal) = finished.walk_goal_mut() {
                goal.endpoint = None;
            }
            if let Order::Tend { weld, .. } = &mut finished {
                *weld = None;
            }
            self.queue.push_back(finished);
        }
        match self.queue.pop_front() {
            Some(next) => self.order = next,
            None => self.looping = false,
        }
        self.path = None;
        self.progress = 0;
        self.end_release();
    }

    /// Drops the active order without rotating it into a looping program:
    /// the next queued order starts, or the unit idles and stops looping.
    pub(crate) fn drop_active_order(&mut self) {
        self.order = self.queue.pop_front().unwrap_or(Order::Idle);
        if matches!(self.order, Order::Idle) {
            self.looping = false;
        }
        self.path = None;
        self.progress = 0;
        self.end_release();
    }

    /// Completes an engagement without recycling its target into a patrol.
    pub(crate) fn complete_attack(&mut self, resume: Option<Resume>) {
        self.order = if let Some(resume) = resume {
            resume.order()
        } else {
            if self.leash.take().is_some() {
                self.settled = crate::stats::LEASH_STATION_TICKS;
            }
            self.queue.pop_front().unwrap_or_else(|| {
                self.looping = false;
                Order::Idle
            })
        };
        self.path = None;
        self.progress = 0;
        self.end_release();
    }

    /// Abandons the whole program. Overrides use it, and so do the stalls a
    /// queued program must not outlive: a refused chase, an empty bank, or
    /// a full sling.
    pub(crate) fn clear_program(&mut self) {
        self.order = Order::Idle;
        self.queue.clear();
        self.looping = false;
        self.path = None;
        self.progress = 0;
        self.end_release();
    }
}

/// Where a building stands in its life, with the meter that phase runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
pub enum BuildingPhase {
    /// A paid blueprint awaiting full footprint visibility. It has no
    /// physical occupancy and cannot take damage or receive construction
    /// work.
    Provisional,
    /// A verified construction site: it blocks ground and takes damage but
    /// doesn't see, fight, or produce.
    Site {
        /// Ticks of construction work done.
        #[serde(default, skip_serializing_if = "crate::is_default")]
        progress: u32,
    },
    /// Climbing to its `tier`, whose stats already apply, out of service
    /// like a site until the upgrade completes.
    Upgrading {
        /// Ticks of upgrade work done.
        #[serde(default, skip_serializing_if = "crate::is_default")]
        progress: u32,
    },
    /// Complete and in service.
    Built {
        /// Ticks of training on the front of the queue.
        #[serde(default, skip_serializing_if = "crate::is_default")]
        training: u32,
    },
}

/// A static entity occupying a rectangle of tiles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Building {
    /// Stable id; `buildings` is sorted by it.
    pub id: BuildingId,
    /// Owner.
    pub player: PlayerId,
    /// What kind of building.
    pub kind: BuildingKind,
    /// Top-left tile of the footprint.
    pub anchor: TilePos,
    /// Current hit points.
    pub hp: u32,
    /// Units waiting to be produced, front first.
    pub queue: std::collections::VecDeque<UnitKind>,
    /// Construction, upgrade, or service, with that phase's meter.
    pub phase: BuildingPhase,
    /// Where finished units report. `None` means stand at the doorstep.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rally: Option<Rally>,
    /// A player-designated target this defense prefers while it remains a
    /// live, hostile, truly visible target in the weapon's domain. Range and
    /// cover only decide whether the preference can be fired on now; they do
    /// not erase it or suppress ordinary fallback acquisition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<crate::AttackTarget>,
    /// Position on the kind's upgrade ladder (zero = base). An accepted
    /// [`crate::Command::UpgradeBuilding`] advances it immediately and
    /// starts [`BuildingPhase::Upgrading`]; every stats read follows the
    /// committed tier through [`Building::stats`].
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub tier: u8,
    /// Ticks until this building may fire again (turrets).
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub cooldown: u32,
    /// Total hp drained from this building by salvage work: the cumulative
    /// ledger refund crediting reads, so truncation never drifts across
    /// intervals. Omitted from serialization when zero.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub salvage_drained: u32,
    /// Scrap already credited against `salvage_drained`'s target.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub salvage_credited: u32,
    /// Stored and ordered rounds, on exactly the kinds with a magazine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub magazine: Option<Magazine>,
    /// The launch deck, on exactly the kinds that launch warheads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launcher: Option<Launcher>,
    /// The gun's bearing in compass steps, on exactly the kinds that fire.
    /// It turns toward the gun's target at the tier's traverse, and the gun
    /// fires only once it bears. With nothing to aim at and its reload done,
    /// it turns back toward the map centre unless its kind holds its bearing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gun_heading: Option<u8>,
}

/// A building's prepaid rounds. Ordered rounds assemble one at a time,
/// front first; `loaded + ordered` never exceeds the kind's capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Magazine {
    /// Rounds ready to spend.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub loaded: u8,
    /// Paid rounds not yet assembled.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub ordered: u8,
    /// Ticks of work on the round being assembled.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub progress: u32,
    /// Whether the magazine orders a round on its own whenever it has room,
    /// nothing ordered, and a bank that can pay.
    pub autofill: bool,
}

/// A silo's turning launch deck and the tile it was ordered to strike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launcher {
    /// The deck's facing, in compass steps; it fires once aligned.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub heading: u8,
    /// The ordered strike, held until the deck comes about and fires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<TilePos>,
    /// Ticks the deck has stood aligned on its strike; it fires on the
    /// silo's [`ready_ticks`](crate::stats::SiloStats::ready_ticks)th.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub ready: u8,
}

/// Whether `magazine` is what a `kind` building can hold: present exactly
/// on magazine kinds, within capacity, its meter only on an ordered round,
/// and empty until the building stands.
fn magazine_fits(kind: BuildingKind, built: bool, magazine: Option<Magazine>) -> bool {
    match (kind.magazine(), magazine) {
        (None, None) => true,
        (Some(stats), Some(magazine)) => {
            u16::from(magazine.loaded) + u16::from(magazine.ordered) <= u16::from(stats.capacity)
                && magazine.progress < stats.round_ticks
                && (magazine.ordered > 0 || magazine.progress == 0)
                && (built || (magazine.loaded == 0 && magazine.ordered == 0))
        }
        _ => false,
    }
}

/// Whether `launcher` is what a `kind` building can hold: present exactly
/// on silos, with a strike only on the map, once built, and while loaded.
fn launcher_fits(
    state: &State,
    kind: BuildingKind,
    built: bool,
    magazine: Option<Magazine>,
    launcher: Option<Launcher>,
) -> bool {
    match (kind.silo(), launcher) {
        (None, None) => true,
        (Some(silo), Some(launcher)) => {
            launcher.ready < silo.ready_ticks
                && (launcher.ready == 0 || launcher.target.is_some())
                && launcher.target.is_none_or(|target| {
                    state.map.tile(target).is_some()
                        && built
                        && magazine.is_some_and(|magazine| magazine.loaded > 0)
                })
        }
        _ => false,
    }
}

/// Whether a completed own Foundry lies inside an Extractor's support radius.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractorSupport {
    /// No completed own Foundry lies inside the support radius.
    Remote,
    /// At least one completed own Foundry lies inside the support radius.
    Supported,
}

/// The recurring-income state of a completed, living Extractor. Its tier
/// and its Foundry support set the yield independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractorIncome {
    tier: u8,
    support: ExtractorSupport,
}

impl ExtractorIncome {
    /// The income of an Extractor at `tier` with `support`.
    pub const fn new(tier: u8, support: ExtractorSupport) -> Self {
        Self { tier, support }
    }

    /// The Extractor's tier: 1 is the Deep Bore.
    pub const fn tier(self) -> u8 {
        self.tier
    }

    /// Whether a completed own Foundry currently supports the Extractor.
    pub const fn support(self) -> ExtractorSupport {
        self.support
    }

    /// Whether nearby Foundry support is currently active.
    pub const fn is_supported(self) -> bool {
        matches!(self.support, ExtractorSupport::Supported)
    }

    /// Scrap paid per payout and the ticks between payouts.
    pub const fn yield_cadence(self) -> (u32, u64) {
        use crate::stats::{
            DEEP_BORE_REMOTE_YIELD, DEEP_BORE_SUPPORTED_YIELD, EXTRACTOR_REMOTE_YIELD,
            EXTRACTOR_SUPPORTED_YIELD,
        };
        match (self.tier, self.support) {
            (0, ExtractorSupport::Remote) => EXTRACTOR_REMOTE_YIELD,
            (0, ExtractorSupport::Supported) => EXTRACTOR_SUPPORTED_YIELD,
            (_, ExtractorSupport::Remote) => DEEP_BORE_REMOTE_YIELD,
            (_, ExtractorSupport::Supported) => DEEP_BORE_SUPPORTED_YIELD,
        }
    }

    /// Scrap generated per minute at the fixed simulation rate.
    pub fn scrap_per_minute(self) -> u32 {
        let (amount, period) = self.yield_cadence();
        let per_minute = u64::from(amount) * 60 * u64::from(crate::TICKS_PER_SECOND) / period;
        u32::try_from(per_minute).expect("Extractor yields fit in u32")
    }
}

impl Building {
    /// This building's stats at its current tier — the accessor every
    /// live read goes through; [`crate::stats::BuildingKind::base_stats`]
    /// answers only tier-invariant questions.
    pub fn stats(&self) -> &'static crate::stats::BuildingStats {
        self.kind.tier_stats(self.tier)
    }

    /// Whether the building is complete and in service: only a built
    /// building sees, fights, trains, or works.
    pub fn built(&self) -> bool {
        matches!(self.phase, BuildingPhase::Built { .. })
    }

    /// Whether this is a paid blueprint still awaiting its ground.
    pub fn provisional(&self) -> bool {
        self.phase == BuildingPhase::Provisional
    }

    /// Whether an upgrade is under way.
    pub fn upgrading(&self) -> bool {
        matches!(self.phase, BuildingPhase::Upgrading { .. })
    }

    /// Whether the building was paid for but never finished: a provisional
    /// blueprint or a site. An upgrading building is not: it already stood
    /// complete at its previous rung.
    pub fn under_construction(&self) -> bool {
        matches!(
            self.phase,
            BuildingPhase::Provisional | BuildingPhase::Site { .. }
        )
    }

    /// Whether every seat knows of this building regardless of fog: a kind
    /// public at its tier, or an assembly cradle once its first work lands.
    pub fn is_public(&self) -> bool {
        self.kind.is_public(self.tier) || (self.kind.public_once_started() && !self.unstarted())
    }

    /// Whether construction has not begun, so cancelling refunds in full.
    pub fn unstarted(&self) -> bool {
        matches!(
            self.phase,
            BuildingPhase::Provisional | BuildingPhase::Site { progress: 0 }
        )
    }

    /// Ticks of construction or upgrade work done; none once built.
    pub fn construction_progress(&self) -> Option<u32> {
        match self.phase {
            BuildingPhase::Site { progress } | BuildingPhase::Upgrading { progress } => {
                Some(progress)
            }
            BuildingPhase::Provisional => Some(0),
            BuildingPhase::Built { .. } => None,
        }
    }

    /// The construction stage a site or upgrade has reached: the share of
    /// its rung's `build_ticks` its progress has passed, the last stage
    /// holding until it stands; none once built. A rung without a labor
    /// budget counts as one tick long, and a zero-tick one stays at the
    /// first stage.
    pub fn construction_stage(&self) -> Option<u8> {
        let progress = u64::from(self.construction_progress()?);
        let total = u64::from(
            self.stats()
                .construction
                .map_or(1, |stats| stats.build_ticks),
        );
        let stages = u64::from(crate::stats::CONSTRUCTION_STAGES);
        let stage = (progress * stages)
            .checked_div(total)
            .map_or(0, |stage| stage.min(stages - 1));
        Some(u8::try_from(stage).expect("stages fit in u8"))
    }

    /// Ticks of training on the front of the queue; zero unless built.
    pub fn training_progress(&self) -> u32 {
        match self.phase {
            BuildingPhase::Built { training } => training,
            BuildingPhase::Provisional
            | BuildingPhase::Site { .. }
            | BuildingPhase::Upgrading { .. } => 0,
        }
    }

    /// Adds `work` ticks to a site's or an upgrade's meter, capped at the
    /// rung's `build_ticks`, and returns the meter before and after; none
    /// for a building that runs no such meter.
    pub(crate) fn add_construction_work(
        &mut self,
        work: u32,
        build_ticks: u32,
    ) -> Option<(u32, u32)> {
        match &mut self.phase {
            BuildingPhase::Site { progress } | BuildingPhase::Upgrading { progress } => {
                let before = *progress;
                *progress = before.saturating_add(work).min(build_ticks);
                Some((before, *progress))
            }
            BuildingPhase::Provisional | BuildingPhase::Built { .. } => None,
        }
    }

    /// Turns a provisional blueprint whose ground checked out into a site.
    pub(crate) fn activate(&mut self) {
        debug_assert!(self.provisional());
        self.phase = BuildingPhase::Site { progress: 0 };
    }

    /// Finishes construction or an upgrade.
    pub(crate) fn complete(&mut self) {
        debug_assert!(self.under_construction() || self.upgrading());
        self.phase = BuildingPhase::Built { training: 0 };
    }

    /// Commits the next rung, whose stats apply at once, and starts the
    /// upgrade clock.
    pub(crate) fn begin_upgrade(&mut self) {
        debug_assert!(self.built());
        self.tier += 1;
        self.phase = BuildingPhase::Upgrading { progress: 0 };
    }

    /// Iterates the footprint tiles row-major.
    pub fn tiles(&self) -> impl Iterator<Item = TilePos> + use<> {
        let (w, h) = self.kind.size();
        let anchor = self.anchor;
        (0..h).flat_map(move |dy| (0..w).map(move |dx| anchor.offset(dx, dy)))
    }

    /// Whether `pos` lies inside the footprint.
    pub fn contains(&self, pos: TilePos) -> bool {
        let (w, h) = self.kind.size();
        pos.x >= self.anchor.x
            && pos.y >= self.anchor.y
            && pos.x < self.anchor.x + w
            && pos.y < self.anchor.y + h
    }

    /// Center of the footprint in world coordinates.
    pub fn center(&self) -> Vec2Fx {
        crate::geometry::footprint_center(self.anchor, self.kind.size())
    }

    /// The point of the footprint rectangle closest to `from` — what range
    /// checks measure against, so big buildings don't get phantom reach.
    pub fn closest_point_to(&self, from: Vec2Fx) -> Vec2Fx {
        let (w, h) = self.kind.size();
        let min = self.anchor.center() - Vec2Fx::new(chassis::fx::HALF, chassis::fx::HALF);
        let max = min + Vec2Fx::new(chassis::fx::Fx::from_num(w), chassis::fx::Fx::from_num(h));
        Vec2Fx::new(from.x.clamp(min.x, max.x), from.y.clamp(min.y, max.y))
    }
}

/// The whole world. See module docs for invariants.
///
/// Every field is crate-private: the only way anything outside the sim can
/// change a `State` is [`State::tick`] with tick-stamped commands. Read
/// access goes through the accessor methods below.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct State {
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub(crate) mode: crate::scenario::ScenarioMode,
    pub(crate) tick: Tick,
    pub(crate) map: Map,
    pub(crate) players: Vec<Player>,
    pub(crate) vision: Vec<crate::vision::Vision>,
    pub(crate) units: Vec<Unit>,
    pub(crate) buildings: Vec<Building>,
    pub(crate) shells: Vec<Shell>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) aircraft_crashes: Vec<AircraftCrash>,
    /// Warheads in flight, sorted by id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) warheads: Vec<Warhead>,
    pub(crate) result: Option<GameResult>,
    next_unit_id: u32,
    next_building_id: u32,
    #[serde(default, skip_serializing_if = "crate::is_default")]
    next_warhead_id: u32,
    /// Derived: one flag per tile, set while a non-stealthy building
    /// covers it. Never serialized or hashed; rebuilt on assembly and load,
    /// and maintained at the placement and removal funnels. Route searches
    /// ask whether a tile is blocked thousands of times per tick.
    #[serde(skip)]
    pub(crate) building_occupancy: Vec<u8>,
}

/// Map tiles under friendly ground bodies standing still, one bit per tile
/// in row-major order. Route lookahead never cuts through them; routes stay
/// body-blind and the collision resolver separates whatever bodies meet.
/// Ground off the map is closed anyway, so a body resting there marks
/// nothing.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ParkedBodies {
    width: i32,
    height: i32,
    bits: Vec<u64>,
}

impl ParkedBodies {
    fn index(&self, tile: TilePos) -> Option<usize> {
        ((0..self.width).contains(&tile.x) && (0..self.height).contains(&tile.y))
            .then(|| tile.row_major(self.width))
    }

    /// Whether a friendly body stands still on map tile `tile`.
    pub(crate) fn blocks(&self, tile: TilePos) -> bool {
        self.index(tile)
            .is_some_and(|i| self.bits[i / 64] >> (i % 64) & 1 == 1)
    }
}

/// How far beyond a body's radius a contact surface governs its motion.
const CONTACT_BAND: Fx = Fx::lit("0.25");

/// Ground passability for one moment: terrain plus the derived building
/// occupancy grid. Equivalent to [`State::passable`] for every tile.
#[derive(Clone, Copy)]
pub(crate) struct GroundTerrain<'a> {
    map: &'a Map,
    occupancy: &'a [u8],
    contact: Option<crate::building_contact::Surface>,
    /// Outlines of other buildings that bound a body pressing on the
    /// contact surface by their drawn shape rather than their whole tiles.
    outlines: &'a [crate::building_contact::Surface],
}

impl<'a> GroundTerrain<'a> {
    pub(crate) fn new(map: &'a Map, occupancy: &'a [u8]) -> Self {
        Self {
            map,
            occupancy,
            contact: None,
            outlines: &[],
        }
    }

    pub(crate) fn with_contact(
        mut self,
        contact: Option<crate::building_contact::Surface>,
    ) -> Self {
        self.contact = contact;
        self
    }

    pub(crate) fn with_outlines(
        mut self,
        outlines: &'a [crate::building_contact::Surface],
    ) -> Self {
        self.outlines = outlines;
        self
    }

    pub(crate) fn at_contact(&self, pos: Vec2Fx, radius: Fx) -> bool {
        self.contact.is_some_and(|surface| {
            let reach = radius + CONTACT_BAND;
            pos.dist_sq(surface.closest(pos)) <= reach * reach
                && self.contact_clear(pos, pos, radius)
        })
    }

    /// The point straight out from the contact surface just past the band
    /// where it governs motion, when the body rests in that band, clear of
    /// the surface, and can back out to it.
    pub(crate) fn contact_exit(&self, pos: Vec2Fx, radius: Fx) -> Option<Vec2Fx> {
        let surface = self.contact?;
        // In the band, the body is at least its radius off the surface, so
        // the outward projection is well defined.
        if !self.at_contact(pos, radius) {
            return None;
        }
        // Margin for the rounding in the projected point.
        let exit = surface.stance(pos, radius + CONTACT_BAND + const { Fx::lit("0.015625") });
        self.contact_clear(pos, exit, radius).then_some(exit)
    }

    pub(crate) fn contact_clear(&self, from: Vec2Fx, to: Vec2Fx, radius: Fx) -> bool {
        self.contact.is_some_and(|surface| {
            let open = |tile| {
                self.open(tile)
                    || ((surface.covers(tile) || self.outlines.iter().any(|s| s.covers(tile)))
                        && self.map.terrain_passable(tile))
            };
            crate::geometry::circle_clear(to, radius, open)
                && surface.clear(from, to, radius)
                && self.outlines.iter().all(|s| s.clear(from, to, radius))
                && !chassis::path::swept_line_blocked(from, to, radius, open)
        })
    }

    /// Whether a ground body may occupy `tile`.
    pub(crate) fn open(&self, tile: TilePos) -> bool {
        self.map.terrain_passable(tile) && !self.building_blocks(tile)
    }

    /// Whether a non-stealthy, non-provisional building covers `tile`.
    pub(crate) fn building_blocks(&self, tile: TilePos) -> bool {
        let width = self.map.width();
        if tile.x < 0 || tile.y < 0 || tile.x >= width || tile.y >= self.map.height() {
            return false;
        }
        let idx = tile.row_major(width);
        self.occupancy.get(idx).copied().unwrap_or(0) != 0
    }
}

/// One allied source of Scuttle Charge detection; the rule is
/// [`State::building_apparent`]'s.
#[derive(Debug, Clone, Copy)]
enum ChargeDetector {
    /// A live machine with charge detection: a Chebyshev square.
    Scout(TilePos),
    /// A built Array: a euclidean disc of its tier's radius.
    Mast { anchor: TilePos, radius: i32 },
}

impl ChargeDetector {
    fn covers(self, anchor: TilePos) -> bool {
        match self {
            Self::Scout(tile) => tile.chebyshev(anchor) <= crate::stats::CHARGE_SCOUT_DETECT_RADIUS,
            Self::Mast {
                anchor: mast,
                radius,
            } => {
                let (dx, dy) = (anchor.x - mast.x, anchor.y - mast.y);
                dx * dx + dy * dy <= radius * radius
            }
        }
    }
}

/// [`State::building_apparent`] for one viewer over many buildings. The
/// viewer's allied detectors are gathered at the first stealth test, so a
/// pass over many charges reads every unit and building once rather than
/// once per charge. The shared borrow keeps the state, and so the gathered
/// list, unchanged for the lens's lifetime.
pub(crate) struct ChargeLens<'a> {
    state: &'a State,
    viewer: PlayerId,
    detectors: std::cell::OnceCell<Vec<ChargeDetector>>,
}

impl<'a> ChargeLens<'a> {
    pub(crate) fn new(state: &'a State, viewer: PlayerId) -> Self {
        Self {
            state,
            viewer,
            detectors: std::cell::OnceCell::new(),
        }
    }

    /// [`State::charge_detected_at`] for this lens's viewer.
    pub(crate) fn detected_at(&self, anchor: TilePos) -> bool {
        self.detectors
            .get_or_init(|| self.state.charge_detectors(self.viewer).collect())
            .iter()
            .any(|d| d.covers(anchor))
    }

    /// [`State::building_apparent`] for this lens's viewer.
    pub(crate) fn building_apparent(&self, building: &Building) -> bool {
        self.state
            .apparent_unless_undetected(self.viewer, building, |anchor| self.detected_at(anchor))
    }
}

impl State {
    /// Assembles a state from parts; [`crate::Scenario::build`] is the public
    /// entry point.
    pub(crate) fn assemble(map: Map, players: Vec<Player>) -> Self {
        let (map_width, map_height) = (map.width().max(0), map.height().max(0));
        let vision = players
            .iter()
            .map(|_| crate::vision::Vision::new(map.width(), map.height()))
            .collect();
        Self {
            mode: crate::scenario::ScenarioMode::Match,
            tick: 0,
            map,
            players,
            vision,
            units: Vec::new(),
            buildings: Vec::new(),
            shells: Vec::new(),
            aircraft_crashes: Vec::new(),
            warheads: Vec::new(),
            result: None,
            next_unit_id: 0,
            next_building_id: 0,
            next_warhead_id: 0,
            building_occupancy: vec![0; cell_count(map_width, map_height)],
        }
    }

    /// Canonical fingerprint of the entire state. Two states with equal
    /// hashes evolved from the same inputs are the same state.
    pub fn hash(&self) -> u64 {
        chassis::hash::state_hash(self)
    }

    /// Ticks elapsed since scenario start. (The mutating step is
    /// [`State::tick`]; this is the counter it advances.)
    pub fn current_tick(&self) -> Tick {
        self.tick
    }

    /// Setup and completion rules carried by this world.
    pub fn mode(&self) -> crate::scenario::ScenarioMode {
        self.mode
    }

    /// Whether a seat can issue commands. Each command still validates its own
    /// ownership, resources and other prerequisites.
    pub fn accepts_commands(&self, player: PlayerId) -> bool {
        self.result.is_none()
            && self.try_player(player).is_some_and(|seat| !seat.resigned)
            && (self.mode == crate::scenario::ScenarioMode::Sandbox
                || self.buildings.iter().any(|building| {
                    building.player == player
                        && !building.provisional()
                        && building.kind == crate::stats::BuildingKind::Foundry
                }))
    }

    /// Whether a living Foundry owns no machine that can harvest: none in the
    /// world, none riding a transport, and none prepaid in a live production
    /// queue.
    pub(crate) fn harvester_recovery_needed(&self, player: PlayerId) -> bool {
        let harvests = |kind: UnitKind| kind.stats().harvest.is_some();
        !self.player(player).resigned
            && self.buildings.iter().any(|building| {
                building.player == player
                    && building.hp > 0
                    && building.built()
                    && building.kind == BuildingKind::Foundry
            })
            && !self.units.iter().any(|unit| {
                unit.player == player
                    && ((unit.hp > 0 && harvests(unit.kind))
                        || unit
                            .cargo
                            .iter()
                            .any(|rider| rider.hp > 0 && harvests(rider.kind)))
            })
            && !self.buildings.iter().any(|building| {
                building.player == player
                    && building.hp > 0
                    && building.built()
                    && building.queue.iter().any(|kind| harvests(*kind))
            })
    }

    /// Scrap an automatic Repair Bay leaves in a stranded seat's bank: the
    /// package its current recovery cycle captured, or the one a new cycle
    /// would capture. Like a voluntary purchase, the aura must not spend
    /// the package, but reserving the universal maximum instead would
    /// strand the army the bay exists to sustain.
    pub(crate) fn recovery_reserve(&self, player: PlayerId) -> u32 {
        if !self.harvester_recovery_needed(player) {
            return 0;
        }
        match self.player(player).recovery {
            Recovery::Ready => self.recovery_package_target(player),
            Recovery::Active { target, .. } => u32::from(target),
        }
    }

    /// Bank target for a newly stranded economy. A surviving paid ground
    /// screen means the seat needs only a replacement worker; otherwise
    /// the public package includes one cheapest dependable guard.
    pub(crate) fn recovery_package_target(&self, player: PlayerId) -> u32 {
        let screen_value: u32 = self
            .units
            .iter()
            .filter(|unit| unit.player == player && unit.hp > 0 && unit.kind.is_recovery_screen())
            .map(|unit| unit.kind.stats().cost)
            .chain(
                self.buildings
                    .iter()
                    .filter(|building| building.player == player && building.hp > 0)
                    .flat_map(|building| building.queue.iter())
                    .filter(|kind| kind.is_recovery_screen())
                    .map(|kind| kind.stats().cost),
            )
            .fold(0, u32::saturating_add);
        UnitKind::Harvester.stats().cost
            + if screen_value >= UnitKind::Sentinel.stats().cost {
                0
            } else {
                UnitKind::Sentinel.stats().cost
            }
    }

    /// Terrain and scrap.
    pub fn map(&self) -> &Map {
        &self.map
    }

    /// All players, indexed by [`PlayerId`].
    pub fn players(&self) -> &[Player] {
        &self.players
    }

    /// All living units, sorted by id.
    pub fn units(&self) -> &[Unit] {
        &self.units
    }

    /// All standing buildings, sorted by id.
    pub fn buildings(&self) -> &[Building] {
        &self.buildings
    }

    /// Every living building every seat knows of regardless of fog, in id
    /// order. Decisions must rank these by their own geometry, never take
    /// the first.
    pub fn public_buildings(&self) -> impl Iterator<Item = crate::vision::PublicBuilding> + '_ {
        self.buildings
            .iter()
            .filter(|b| b.hp > 0 && b.is_public())
            .map(|b| crate::vision::PublicBuilding {
                owner: b.player,
                kind: b.kind,
                tier: b.tier,
                anchor: b.anchor,
                progress: b
                    .kind
                    .public_once_started()
                    .then(|| b.construction_progress().unwrap_or(0)),
            })
    }

    /// Returns the current income state of a completed, living Extractor.
    ///
    /// Foundry support is binary and owner-specific. It uses the shortest
    /// Chebyshev distance between the two building footprints, so multiple
    /// supporting Foundries never stack and unfinished sites confer nothing.
    pub fn extractor_income(&self, id: BuildingId) -> Option<ExtractorIncome> {
        let extractor = self.building(id)?;
        if extractor.kind != BuildingKind::Extractor || !extractor.built() || extractor.hp == 0 {
            return None;
        }

        Some(ExtractorIncome::new(
            extractor.tier,
            self.foundry_support(extractor),
        ))
    }

    /// Whether a completed own Foundry supports a completed, living
    /// Extractor. An unfinished or fallen Extractor is remote.
    pub(crate) fn foundry_support(&self, extractor: &Building) -> ExtractorSupport {
        if self
            .buildings
            .iter()
            .any(|building| self.extractor_supported_by(extractor.id, building.id))
        {
            ExtractorSupport::Supported
        } else {
            ExtractorSupport::Remote
        }
    }

    /// The machines each built, living Repair Bay services this pulse and how
    /// fast it heals each, Bays in building-id order and patients most
    /// wounded first: own, wounded and in reach, not retiring, landed or
    /// unable to land, not taken by an earlier Bay, at most
    /// [`crate::stats::REPAIR_BAY_PATIENTS`] per Bay. The aura and
    /// presentation read the same choice; the aura also slows a patient the
    /// tick's own volley hits, before that hit stamps it.
    pub fn repair_bay_patients(&self) -> Vec<(BuildingId, Vec<(UnitId, RepairPace)>)> {
        use crate::stats::{REPAIR_BAY_CALM_TICKS, REPAIR_BAY_PATIENTS, REPAIR_BAY_RADIUS};
        let mut taken: Vec<UnitId> = Vec::new();
        let mut bays = Vec::new();
        for b in self
            .buildings
            .iter()
            .filter(|b| b.kind == BuildingKind::RepairBay && b.built() && b.hp > 0)
        {
            let mut patients: Vec<&Unit> = self
                .units
                .iter()
                .filter(|u| {
                    let max = u.kind.stats().max_hp;
                    u.player == b.player
                        && u.hp > 0
                        && u.hp < max
                        && u.kind.stats().repairable()
                        && !u.retiring()
                        && (u.landed() || !u.kind.stats().can_land())
                        && b.closest_point_to(u.pos).dist_sq(u.pos)
                            <= REPAIR_BAY_RADIUS * REPAIR_BAY_RADIUS
                        && !taken.contains(&u.id)
                })
                .collect();
            // Every patient and every Bay that could take it is the owner's,
            // so id order is the owner-local rank and mirrored seats choose
            // alike.
            let missing = |u: &Unit| {
                let max = u64::from(u.kind.stats().max_hp);
                (max - u64::from(u.hp), max)
            };
            patients.sort_by(|a, b| {
                let ((a_missing, a_max), (b_missing, b_max)) = (missing(a), missing(b));
                (b_missing * a_max)
                    .cmp(&(a_missing * b_max))
                    .then(a.id.cmp(&b.id))
            });
            patients.truncate(REPAIR_BAY_PATIENTS);
            taken.extend(patients.iter().map(|u| u.id));
            let chosen = patients
                .into_iter()
                .map(|u| {
                    let calm = u
                        .damaged_at
                        .is_none_or(|at| self.tick.saturating_sub(at) >= REPAIR_BAY_CALM_TICKS);
                    let pace = if calm {
                        RepairPace::Fast
                    } else {
                        RepairPace::Slow
                    };
                    (u.id, pace)
                })
                .collect();
            bays.push((b.id, chosen));
        }
        bays
    }

    /// Whether one completed own Foundry currently supports an Extractor.
    ///
    /// This endpoint query lets presentation and diagnostics show the same
    /// connection that recurring income uses without reimplementing its
    /// footprint geometry.
    pub fn extractor_supported_by(&self, extractor: BuildingId, foundry: BuildingId) -> bool {
        let (Some(extractor), Some(foundry)) = (self.building(extractor), self.building(foundry))
        else {
            return false;
        };
        extractor.kind == BuildingKind::Extractor
            && extractor.built()
            && extractor.hp > 0
            && foundry.kind == BuildingKind::Foundry
            && foundry.built()
            && foundry.hp > 0
            && foundry.player == extractor.player
            && crate::geometry::foundry_supports(foundry.anchor, extractor.anchor)
    }

    /// The match outcome, once decided.
    pub fn result(&self) -> Option<GameResult> {
        self.result
    }

    /// The player behind `id`. Panics on a foreign id — player ids come from
    /// scenario setup and never dangle.
    pub fn player(&self, id: PlayerId) -> &Player {
        &self.players[id.0 as usize]
    }

    /// Fallible sibling of [`State::player`], for callers holding ids from
    /// outside the sim (protocol traffic, tooling).
    pub fn try_player(&self, id: PlayerId) -> Option<&Player> {
        self.players.get(id.0 as usize)
    }

    /// A player's fog-of-war view. Panics on a foreign id, like
    /// [`State::player`].
    pub fn vision(&self, id: PlayerId) -> &crate::vision::Vision {
        &self.vision[id.0 as usize]
    }

    /// Checks every structural invariant field-level deserialization alone
    /// cannot. This is the sim's trust boundary: [`State`]'s `Deserialize`
    /// impl calls it, there is no unvalidated constructor, and everything
    /// downstream — the tick pipeline, renderers, and observation
    /// builders — is entitled to assume the whole checklist below. In
    /// particular the coordinate envelope is what *licenses* the sim's
    /// unchecked tile arithmetic ([`TilePos::offset`],
    /// [`Building::contains`], the neighborhood scans): a coordinate that
    /// got through here cannot overflow them.
    ///
    /// The checklist, in the order it runs:
    /// - Players and result: a non-empty table, addressable by
    ///   [`PlayerId`], with team indices inside it and any victory naming
    ///   a team a player actually carries.
    /// - Map: consistent grid dimensions, within [`MAX_MAP_EDGE`].
    /// - Per-player tables: one vision per seat, its grids sized to the
    ///   map.
    /// - Entity lists: strictly sorted by id, both id counters ahead of
    ///   every live id.
    /// - Units: owner in the table, a motor, harvest gear, turret bearing
    ///   and cargo hold only where the kind has them, a planted gun's
    ///   bearing within its traverse, hp inside
    ///   `(0, max_hp]`, meters and per-weapon cooldowns bounded, queue within
    ///   [`crate::stats::ORDER_QUEUE_CAP`], every coordinate inside the
    ///   envelope, every anchored Harvest source inside its work zone,
    ///   every entity named by an order actually minted.
    /// - Buildings: the same, plus a phase that fits the tier, a queue
    ///   only once built and only of what this kind produces, and a
    ///   coherent salvage ledger.
    /// - Shells: coordinates inside the envelope, shooter minted.
    /// - Vision: ghost owners in the table and hostile to the viewer,
    ///   ghost tiers on the kind's ladder and hp inside `(0, max_hp]` for
    ///   that tier; ghosts, contacts, and recent allied impact sites inside
    ///   their bounds and in canonical order.
    ///
    /// Two rules are deliberately *permissive*, because tighter ones would
    /// refuse legitimately reachable states. References are checked
    /// against the id counters, never against the live tables: an order
    /// or a shell outliving its subject by a tick is ordinary (brains
    /// re-validate, and a shell's shooter may be dead by impact), while an
    /// id the run never minted is forgery. And the envelopes are
    /// generous sanity boxes rather than map-relative bounds: the bug
    /// being killed is the overflow class, not nonsense geometry, and a
    /// body the separation phase shoved a fraction of a tile past the
    /// border is a state the sim really does produce.
    ///
    /// Every struct is destructured field by field, so a new field does not
    /// compile until its checks are decided; give each new check a fixture
    /// in `sim/tests/integration/state_integrity.rs`.
    ///
    /// Public for tooling that wants to re-check a state it mutated by
    /// hand; the sim itself never calls it inside [`State::tick`].
    pub fn validate_invariants(&self) -> Result<(), StateIntegrityError> {
        use StateIntegrityError as E;
        // Destructured so a new field cannot compile until its checks are
        // decided here or in the per-entity validators below.
        let Self {
            mode: _,
            tick,
            map,
            players,
            vision,
            units,
            buildings,
            shells,
            aircraft_crashes,
            warheads,
            result,
            next_unit_id,
            next_building_id,
            next_warhead_id,
            building_occupancy: _,
        } = self;

        self.validate_players()?;

        // Nested grids: derived Deserialize accepts any cell count, and a
        // short one panics deep inside vision refresh instead of here.
        if !map.is_consistent() {
            return Err(E::MalformedMapGrid);
        }
        let (w, h) = (map.width(), map.height());
        // The parse-time bound, re-applied: the neighborhood scans add
        // unchecked radii to the map dimensions.
        if w > i32::from(MAX_MAP_EDGE) || h > i32::from(MAX_MAP_EDGE) {
            return Err(E::MapTooLarge {
                width: w,
                height: h,
            });
        }
        if vision.len() != players.len() {
            return Err(E::VisionTableMismatch);
        }
        if vision.iter().any(|v| !v.is_consistent(w, h)) {
            return Err(E::MalformedVisionGrid);
        }

        if !units.windows(2).all(|a| a[0].id < a[1].id) {
            return Err(E::UnsortedUnits);
        }
        if !buildings.windows(2).all(|a| a[0].id < a[1].id) {
            return Err(E::UnsortedBuildings);
        }
        if let Some(u) = units.last()
            && u.id.0 >= *next_unit_id
        {
            return Err(E::StaleUnitCounter);
        }
        if let Some(b) = buildings.last()
            && b.id.0 >= *next_building_id
        {
            return Err(E::StaleBuildingCounter);
        }
        if !warheads.windows(2).all(|a| a[0].id < a[1].id) {
            return Err(E::UnsortedWarheads);
        }
        if let Some(w) = warheads.last()
            && w.id.0 >= *next_warhead_id
        {
            return Err(E::StaleWarheadCounter);
        }
        // The counters and the clock increment unchecked in the tick
        // pipeline; a forged extreme is a next-step panic (debug) or a
        // wrap that aliases live ids (release).
        if *tick > TICK_ENVELOPE {
            return Err(E::TickBeyondEnvelope);
        }
        if *next_unit_id > ID_COUNTER_ENVELOPE
            || *next_building_id > ID_COUNTER_ENVELOPE
            || *next_warhead_id > ID_COUNTER_ENVELOPE
        {
            return Err(E::IdCounterBeyondEnvelope);
        }

        for u in units {
            self.validate_unit(u)?;
            for rider in &u.cargo {
                self.validate_rider(u, rider)?;
            }
        }
        if let Some(id) = self.escort_loop() {
            return Err(E::InvalidEscort(id));
        }
        self.validate_drones()?;
        // Two parked bodies inside their combined radius could never have
        // met: a touchdown needs that clearance and nothing moves a parked
        // body afterwards.
        for (i, a) in units.iter().enumerate() {
            if !a.landed() {
                continue;
            }
            for b in units[i + 1..].iter().filter(|b| b.landed()) {
                let clearance = a.kind.stats().radius + b.kind.stats().radius;
                if a.pos.dist_sq(b.pos) < clearance * clearance {
                    return Err(E::LandedOverlap(a.id, b.id));
                }
            }
        }
        // Every id in the world — walking or riding — is minted once.
        let mut ids: Vec<u32> = units
            .iter()
            .flat_map(|u| std::iter::once(u.id.0).chain(u.cargo.iter().map(|r| r.id.0)))
            .collect();
        ids.sort_unstable();
        if ids.windows(2).any(|w| w[0] == w[1]) {
            return Err(E::AliasedCargoId);
        }

        for b in buildings {
            self.validate_building(b)?;
        }
        self.validate_assembly()?;
        self.validate_footprints()?;
        for (i, crash) in aircraft_crashes.iter().enumerate() {
            self.validate_crash(i, crash)?;
        }
        for (i, shell) in shells.iter().enumerate() {
            self.validate_shell(i, shell)?;
        }
        for (i, warhead) in warheads.iter().enumerate() {
            self.validate_warhead(i, warhead, result.is_some())?;
        }
        for (i, view) in vision.iter().enumerate() {
            self.validate_vision(PlayerId::from_index(i), view)?;
        }
        Ok(())
    }

    fn validate_players(&self) -> Result<(), StateIntegrityError> {
        use StateIntegrityError as E;
        if self.players.is_empty() {
            return Err(E::NoPlayers);
        }
        // PlayerId is a u8: past 256 seats the ids alias and winners()
        // would name the wrong ones.
        if self.players.len() > usize::from(u8::MAX) + 1 {
            return Err(E::TooManyPlayers);
        }
        for (index, player) in self.players.iter().enumerate() {
            let Player {
                name: _,
                team,
                scrap: _,
                recovery,
                resigned: _,
                eliminated_at,
            } = player;
            let seat = PlayerId::from_index(index);
            // Teams normalize to dense ids at scenario build, so a team
            // index is always a seat index too.
            if usize::from(*team) >= self.players.len() {
                return Err(E::ForeignTeam(seat));
            }
            if let Recovery::Active { target, allowance } = recovery
                && (u32::from(*target) > crate::stats::FOUNDRY_RECOVERY_RESERVE
                    || allowance > target)
            {
                return Err(E::InvalidRecoveryLedger(seat));
            }
            if eliminated_at.is_some_and(|at| at > TICK_ENVELOPE) {
                return Err(E::EliminationBeyondEnvelope(seat));
            }
            // Victory treats the stamp as immutable history, so a stamp
            // later than the present would flow to placement and views
            // as an elimination that never happened.
            if eliminated_at.is_some_and(|at| at > self.tick) {
                return Err(E::EliminationInTheFuture(seat));
            }
        }
        if self.mode == crate::scenario::ScenarioMode::Sandbox
            && (self.result.is_some()
                || self
                    .players
                    .iter()
                    .any(|player| player.eliminated_at.is_some()))
        {
            return Err(E::SandboxElimination);
        }
        if let Some(GameResult::Victory { team }) = self.result
            && !self.players.iter().any(|player| player.team == team)
        {
            return Err(E::UnknownVictoryTeam(team));
        }
        Ok(())
    }

    /// What admission guarantees: no cradle holds more crew than it admits.
    fn validate_assembly(&self) -> Result<(), StateIntegrityError> {
        use StateIntegrityError as E;
        for cradle in self
            .buildings
            .iter()
            .filter(|b| b.kind.assembles().is_some())
        {
            let crew = self
                .units
                .iter()
                .filter(|u| crate::tick::assembly_holds(u, cradle))
                .count();
            if crew > crate::stats::ASSEMBLY_CREW_CAP {
                return Err(E::CrewBeyondCap(cradle.id));
            }
        }
        Ok(())
    }

    /// What a unit's route must hold: a work endpoint within reach of its goal,
    /// and a clearing route only on a ground chassis.
    fn validate_route(&self, u: &Unit, path: &PathFollow) -> Result<(), StateIntegrityError> {
        use StateIntegrityError as E;
        let stats = u.kind.stats();
        let id = u.id;
        if let Some(point) = path.final_point
            && (!point_inside_envelope(point)
                || (point.x - path.goal.center().x).abs() > const { Fx::lit("1.5") }
                || (point.y - path.goal.center().y).abs() > const { Fx::lit("1.5") }
                || self.map.tile(path.goal).is_none()
                || path.waypoints.last() != Some(&path.goal)
                || path.next as usize >= path.waypoints.len()
                || (u.domain() != crate::stats::Domain::Ground && stats.turn_rate > 0))
        {
            return Err(E::InvalidWorkEndpoint(id));
        }
        if path.clearing && stats.domain != crate::stats::Domain::Ground {
            return Err(E::InvalidClearingRoute(id));
        }
        Ok(())
    }

    fn validate_unit(&self, u: &Unit) -> Result<(), StateIntegrityError> {
        use StateIntegrityError as E;
        let Unit {
            id,
            player,
            kind,
            // `pos`, airborne motion, and the envelope of every order,
            // leash, and path are checked by the whole-unit helpers below.
            pos,
            motor,
            hp,
            damaged_at,
            worker,
            cooldowns,
            progress,
            order,
            queue,
            looping,
            path,
            leash,
            settled: _,
            heading,
            turret_heading,
            cargo,
            radar_deploy,
            point_defense,
            mounts,
            bay,
            host,
        } = u;
        let id = *id;
        if usize::from(player.0) >= self.players.len() {
            return Err(E::ForeignUnitOwner(id));
        }
        let stats = kind.stats();
        if !valid_air_motion(u) {
            return Err(E::InvalidAirMotion(id));
        }
        if *hp == 0 || *hp > stats.max_hp {
            return Err(E::UnitHpOutOfRange(id));
        }
        // Damage lands while a tick runs and the clock then advances, so a
        // stamp is always behind the clock.
        if damaged_at.is_some_and(|at| at >= self.tick) {
            return Err(E::DamageStampInTheFuture(id));
        }
        if worker.is_some() != stats.harvest.is_some()
            || point_defense.is_some() != stats.point_defense.is_some()
            || mounts.is_some() != stats.mounts.is_some()
            || bay.is_some() != stats.bay.is_some()
            || host.is_some() != stats.docks()
            || !motor.fits(*kind)
            || (turret_heading.is_some() && !kind.has_ground_turret())
            || (!cargo.is_empty() && stats.transport_capacity == 0)
        {
            return Err(E::UnitPartMismatch(id));
        }
        if !point_defense_in_range(*point_defense, stats) {
            return Err(E::PointDefenseOutOfRange(id));
        }
        if !mounts_in_range(*mounts, stats) {
            return Err(E::MountsOutOfRange(id));
        }
        // The idle welding switch is an owner's preference with no
        // invariant of its own; `Worker` exists only on harvest kinds.
        let Worker {
            carrying,
            unloading,
            danger_retry_at,
            idle_weld_off: _,
        } = worker.unwrap_or_default();
        if carrying > stats.harvest.map_or(0, |harvest| harvest.capacity) {
            return Err(E::ScrapBeyondCapacity(id));
        }
        if let Some(release) = unloading
            && (carrying == 0
                || release.elapsed == 0
                || release.elapsed >= crate::stats::UNLOAD_TICKS
                || !matches!(order, Order::Harvest { .. } | Order::ReturnCargo { .. })
                || matches!(order, Order::ReturnCargo { foundry, .. } if *foundry != release.foundry)
                || !self.minted(Target::Building(release.foundry))
                || self
                    .building(release.foundry)
                    .is_some_and(|b| b.player != *player || !b.kind.is_drop_off() || !b.built()))
        {
            return Err(E::InvalidUnloading(id));
        }
        if let Some(path) = path {
            self.validate_route(u, path)?;
        }
        if *progress > PROGRESS_ENVELOPE {
            return Err(E::UnitProgressOutOfRange(id));
        }
        // Slot i belongs to weapon i; slots past the roster stay zero for
        // the machine's whole life.
        if cooldowns_out_of_range(*cooldowns, stats) {
            return Err(E::UnitCooldownOutOfRange(id));
        }
        if queue.len() > crate::stats::ORDER_QUEUE_CAP {
            return Err(E::OverlongUnitQueue(id));
        }
        match *motor {
            Motor::Ground { speed, stall_ticks } => {
                if speed < Fx::ZERO || speed > stats.speed {
                    return Err(E::InvalidGroundSpeed(id));
                }
                if stall_ticks >= crate::stats::STALL_REPLAN_TICKS
                    || (stall_ticks != 0 && path.is_none())
                {
                    return Err(E::InvalidStallTicks(id));
                }
            }
            Motor::Braced { ticks, traverse } => {
                let arc = stats
                    .brace
                    .and_then(|brace| brace.traverse)
                    .map_or(0, |traverse| traverse.arc);
                if ticks.get() > stats.brace.map_or(0, |brace| brace.deploy_ticks)
                    || traverse.unsigned_abs() > arc
                {
                    return Err(E::InvalidUnitBraces(id));
                }
            }
            Motor::Airborne { .. } | Motor::Landed => {}
        }
        if danger_retry_at.is_some_and(|tick| {
            tick > self
                .tick
                .saturating_add(crate::stats::HARVEST_DANGER_RETRY_TICKS)
        }) {
            return Err(E::InvalidDangerRetry(id));
        }
        if leash.is_some_and(|leash| {
            leash.patience > crate::stats::LEASH_PATIENCE
                || leash.cooldown > crate::stats::LEASH_REACQUIRE_COOLDOWN
        }) {
            return Err(E::InvalidLeashClock(id));
        }
        if *radar_deploy > stats.radar.map_or(0, |radar| radar.deploy_ticks)
            || (*radar_deploy > 0 && (path.is_some() || u.drive_speed() != Fx::ZERO))
        {
            return Err(E::InvalidRadarDeploy(id));
        }
        if !unit_inside_envelope(u) {
            return Err(E::UnitOutsideEnvelope(id));
        }
        let orders = || std::iter::once(order).chain(queue);
        if !orders().all(order_goals_canonical) {
            return Err(E::NonCanonicalGoal(id));
        }
        if *motor == Motor::Landed {
            let touchdown = crate::stats::LANDING_TOUCHDOWN;
            if pos.dist_sq(u.tile().center()) > touchdown * touchdown {
                return Err(E::LandedOffCenter(id));
            }
            if path.is_some() {
                return Err(E::LandedWithPath(id));
            }
            if !crate::tick::flight::escapable(&self.map, *pos, *heading, stats.turn_radius()) {
                return Err(E::LandedUnescapable(id));
            }
            // Terrain only: a friendly site may claim the tile under a
            // parked airframe between ticks, and eviction resolves it on
            // the next.
            if self
                .map
                .tile(u.tile())
                .is_none_or(|t| t.terrain.blocks_ground())
            {
                return Err(E::LandedOnUnstandableGround(id));
            }
        }
        if orders().any(|order| !harvest_order_inside_zone(order)) {
            return Err(E::HarvestSourceOutsideZone(id));
        }
        if orders().any(|order| {
            matches!(order, Order::ReturnCargo { foundry, .. } if
                stats.harvest.is_none()
                || self.building(*foundry).is_some_and(|building|
                    building.player != *player || !building.kind.is_drop_off()))
        }) {
            return Err(E::InvalidReturnCargo(id));
        }
        // An escort follows another own unit, and only harvest gear holds a
        // torch. A foreign escort would track an enemy body through fog.
        let own_other = |unit: UnitId| {
            unit != id
                && self
                    .escorted(unit)
                    .is_none_or(|body| body.player == *player)
        };
        if orders().any(|order| match *order {
            Order::Accompany { unit, weld } => {
                !own_other(unit)
                    || weld.is_some_and(|patient| stats.harvest.is_none() || !own_other(patient))
            }
            Order::Attack {
                resume: Some(Resume::Accompany(unit)),
                ..
            } => !own_other(unit),
            _ => false,
        }) {
            return Err(E::InvalidEscort(id));
        }
        // A tending walk is a worker's, either a leg of a patrol or a trip
        // out from an idle post with nothing queued behind it. Only the
        // active walk holds a patient: another own unit, as an escort's
        // does, or an own building.
        let tends = |order: &Order| matches!(order, Order::Tend { .. });
        let foreign = |patient: Target| match patient {
            Target::Unit(unit) => !own_other(unit),
            Target::Building(building) => self
                .building(building)
                .is_some_and(|building| building.player != *player),
        };
        if orders().any(|order| {
            matches!(*order, Order::Tend { weld, .. } if stats.harvest.is_none()
                || weld.is_some_and(foreign))
        }) || queue
            .iter()
            .any(|order| matches!(order, Order::Tend { weld: Some(_), .. }))
            || (!*looping && (tends(order) && !queue.is_empty() || queue.iter().any(tends)))
        {
            return Err(E::InvalidTend(id));
        }
        // A retirement names an own Crucible while it stands, lands only a
        // turn-limited airframe, and never rests on a full meter: cleanup
        // takes a unit apart the tick its meter fills.
        if orders().any(|order| {
            matches!(order, Order::Retire { crucible, pad } if
                (pad.is_some() && stats.turn_rate == 0)
                || self.building(*crucible).is_some_and(|building|
                    building.player != *player || building.kind != BuildingKind::Crucible))
        }) || (u.retiring() && *progress >= crate::stats::RETIRE_TICKS)
        {
            return Err(E::InvalidRetirement(id));
        }
        if !stats.scouts && orders().any(|order| matches!(order, Order::Scout { .. })) {
            return Err(E::InvalidScoutOrder(id));
        }
        if orders()
            .flat_map(order_references)
            .any(|target| !self.minted(target))
        {
            return Err(E::UnmintedOrderTarget(id));
        }
        if orders().any(|order| {
            matches!(order, Order::Attack { target, .. } if !self.valid_attack_reference(*player, *target))
        }) {
            return Err(E::UnmintedOrderTarget(id));
        }
        // Cargo is a trusted enclave: nothing in the tick pipeline
        // re-examines a rider until it is set down, so a forged save must
        // not smuggle in anything the sling could never have taken — the
        // wrong carrier, the wrong rider kind, an overfull hold, or an
        // aliased id.
        let hold: u32 = cargo
            .iter()
            .map(|r| u32::from(r.kind.stats().transport_size))
            .sum();
        if hold > u32::from(stats.transport_capacity) {
            return Err(E::CargoBeyondCapacity(id));
        }
        Ok(())
    }

    fn validate_rider(&self, carrier: &Unit, rider: &Rider) -> Result<(), StateIntegrityError> {
        use StateIntegrityError as E;
        let Rider {
            id,
            kind,
            hp,
            carrying,
            cooldowns,
            heading: _,
            turret_heading,
            point_defense,
            damaged_at,
            idle_weld_off,
        } = rider;
        let rstats = kind.stats();
        if damaged_at.is_some_and(|at| at >= self.tick) {
            return Err(E::DamageStampInTheFuture(*id));
        }
        if rstats.transport_size == 0 {
            return Err(E::UncarriableCargo(carrier.id));
        }
        if *hp == 0 || *hp > rstats.max_hp {
            return Err(E::CargoHpOutOfRange(carrier.id));
        }
        if *carrying > rstats.harvest.map_or(0, |harvest| harvest.capacity) {
            return Err(E::ScrapBeyondCapacity(*id));
        }
        if (turret_heading.is_some() && !kind.has_ground_turret())
            || point_defense.is_some() != rstats.point_defense.is_some()
            || (*idle_weld_off && rstats.harvest.is_none())
        {
            return Err(E::UnitPartMismatch(*id));
        }
        if !point_defense_in_range(*point_defense, rstats) {
            return Err(E::PointDefenseOutOfRange(*id));
        }
        // Cooldowns are the one scalar boarding does NOT reset — a machine
        // slung mid-cooldown keeps it frozen — so the bound is the walking
        // unit's weapon table, and a smuggled oversize would silence a
        // weapon for its life.
        if cooldowns_out_of_range(*cooldowns, rstats) {
            return Err(E::CargoCooldownOutOfRange(carrier.id));
        }
        if id.0 >= self.next_unit_id {
            return Err(E::StaleUnitCounter);
        }
        Ok(())
    }

    fn validate_building(&self, b: &Building) -> Result<(), StateIntegrityError> {
        use StateIntegrityError as E;
        let Building {
            id,
            player,
            kind,
            anchor,
            hp,
            queue,
            phase,
            rally,
            focus,
            tier,
            cooldown,
            salvage_drained,
            salvage_credited,
            magazine,
            launcher,
            gun_heading,
        } = b;
        let id = *id;
        if usize::from(player.0) >= self.players.len() {
            return Err(E::ForeignBuildingOwner(id));
        }
        if usize::from(*tier) >= kind.tiers().len() {
            return Err(E::TierBeyondLadder(id));
        }
        // Construction starts at the base rung; an upgrade climbs above it.
        // A cradle never stands finished: its machine replaces it.
        let phase_fits_tier = match phase {
            BuildingPhase::Provisional | BuildingPhase::Site { .. } => *tier == 0,
            BuildingPhase::Upgrading { .. } => *tier > 0,
            BuildingPhase::Built { .. } => kind.assembles().is_none(),
        };
        if !phase_fits_tier {
            return Err(E::InvalidBuildingPhase(id));
        }
        let stats = b.stats();
        if *phase == BuildingPhase::Provisional
            && (*hp != stats.max_hp / 5
                || stats.construction.is_none()
                || rally.is_some()
                || focus.is_some()
                || *cooldown != 0
                || *salvage_drained != 0
                || *salvage_credited != 0
                || !self
                    .units
                    .iter()
                    .any(|unit| crate::tick::construction::committed(unit, b)))
        {
            return Err(E::InvalidProvisionalSite(id));
        }
        if *hp == 0 || *hp > stats.max_hp {
            return Err(E::BuildingHpOutOfRange(id));
        }
        if !magazine_fits(*kind, b.built(), *magazine) {
            return Err(E::InvalidMagazine(id));
        }
        if !launcher_fits(self, *kind, b.built(), *magazine, *launcher) {
            return Err(E::InvalidLauncher(id));
        }
        if gun_heading.is_some() != kind.fires() {
            return Err(E::GunMismatch(id));
        }
        let meter_overrun = match *phase {
            BuildingPhase::Provisional => false,
            BuildingPhase::Site { progress } | BuildingPhase::Upgrading { progress } => stats
                .construction
                .is_none_or(|construction| progress > construction.build_ticks),
            BuildingPhase::Built { training } => training > PROGRESS_ENVELOPE,
        };
        if meter_overrun {
            return Err(E::BuildingProgressOutOfRange(id));
        }
        // A building fires its first weapon and nothing else.
        if *cooldown
            > stats
                .weapons
                .first()
                .map_or(0, |weapon| weapon.cooldown_ticks)
        {
            return Err(E::BuildingCooldownOutOfRange(id));
        }
        if let Some(target) = *focus {
            if !self.valid_attack_reference(*player, target) {
                return Err(E::UnmintedBuildingFocus(id));
            }
            let current_domain = match target.entity() {
                Some(entity) => self.visible_hostile_target_domain(*player, entity),
                None => self.attack_view(*player, target).map(|view| view.domain),
            };
            let live_entity = target.entity().is_some_and(|entity| match entity {
                Target::Unit(id) => self.unit(id).is_some(),
                Target::Building(id) => self.building(id).is_some(),
            });
            if !b.built()
                || stats.weapons.is_empty()
                || ((live_entity || target.entity().is_none()) && current_domain.is_none())
                || current_domain.is_some_and(|domain| !stats.weapons[0].targets.covers(domain))
            {
                return Err(E::InvalidBuildingFocus(id));
            }
        }
        if queue.len() > crate::stats::QUEUE_CAP {
            return Err(E::OverlongBuildingQueue(id));
        }
        // Training needs a complete building, and an upgrade keeps the queue
        // only of a kind that trains nothing.
        if (!queue.is_empty() && !b.built())
            || queue.iter().any(|kind| !stats.produces.contains(kind))
        {
            return Err(E::UnproducibleQueueEntry(id));
        }
        // The footprint needs no separate check: sizes are single digits,
        // so an anchor inside the envelope keeps `anchor + size` inside it
        // too.
        if !tile_inside_envelope(*anchor)
            || matches!(rally, Some(Rally::Tile(tile)) if !tile_inside_envelope(*tile))
        {
            return Err(E::BuildingOutsideEnvelope(id));
        }
        // Unlike an order's reference, a unit rally may not dangle: cleanup
        // clears it in the tick its unit leaves play.
        if let Some(Rally::Unit(unit)) = rally
            && self
                .escorted(*unit)
                .is_none_or(|body| body.player != *player)
        {
            return Err(E::InvalidRally(id));
        }
        if !salvage_ledger_coherent(b) {
            return Err(E::IncoherentSalvageLedger(id));
        }
        Ok(())
    }

    /// The occupancy grid marks each cell present or absent, so two
    /// footprints that both mark it would leave a hole when either one is
    /// cleared. Buried charges and provisional sites never mark, and may
    /// legitimately sit over a hidden building.
    fn validate_footprints(&self) -> Result<(), StateIntegrityError> {
        let width = self.map.width();
        let mut owner: Vec<Option<BuildingId>> = vec![None; cell_count(width, self.map.height())];
        for b in self
            .buildings
            .iter()
            .filter(|b| !b.kind.is_stealthy() && !b.provisional())
        {
            let (w, h) = b.kind.size();
            for tile in (0..h).flat_map(|dy| (0..w).map(move |dx| b.anchor.offset(dx, dy))) {
                if self.map.tile(tile).is_none() {
                    continue;
                }
                let cell = &mut owner[tile.row_major(width)];
                if let Some(other) = *cell {
                    return Err(StateIntegrityError::OverlappingBuildings(other, b.id));
                }
                *cell = Some(b.id);
            }
        }
        Ok(())
    }

    fn validate_crash(&self, i: usize, crash: &AircraftCrash) -> Result<(), StateIntegrityError> {
        let AircraftCrash {
            unit,
            player,
            kind,
            heading: _,
            launch,
            impact,
            started,
            arrival,
        } = crash;
        let live = self
            .units
            .iter()
            .any(|u| u.id == *unit || u.cargo.iter().any(|rider| rider.id == *unit));
        let earlier = &self.aircraft_crashes[..i];
        if usize::from(player.0) >= self.players.len()
            || unit.0 >= self.next_unit_id
            || live
            || kind.stats().crash.is_none()
            || !point_inside_envelope(*launch)
            || !point_inside_envelope(*impact)
            || *started >= self.tick
            || *arrival < self.tick
            || arrival.checked_sub(*started) != Some(crate::stats::AIRCRAFT_CRASH_TICKS)
            || self.result.is_some()
            || earlier
                .last()
                .is_some_and(|prev| (prev.started, prev.unit) >= (*started, *unit))
            || earlier.iter().any(|other| other.unit == *unit)
        {
            return Err(StateIntegrityError::InvalidAircraftCrash(i));
        }
        let reach = kind.stats().speed * Fx::from_num(crate::stats::AIRCRAFT_CRASH_TICKS);
        if launch.dist_sq(*impact) > reach * reach {
            return Err(StateIntegrityError::InvalidAircraftCrash(i));
        }
        Ok(())
    }

    fn validate_shell(&self, i: usize, s: &Shell) -> Result<(), StateIntegrityError> {
        use StateIntegrityError as E;
        // Arrival and damage only change play: landing compares ticks and
        // damage saturates, so neither can overflow. The payload kind only
        // drives presentation.
        let Shell {
            kind: _,
            shooter,
            player,
            launch,
            impact,
            launched_at,
            arrival,
            damage: _,
            targets: _,
            splash,
        } = s;
        // Shells carry a seat too: hostile() indexes the player table on
        // impact, so a foreign owner would panic ticks after acceptance.
        if usize::from(player.0) >= self.players.len() {
            return Err(E::ForeignShellOwner(i));
        }
        if !point_inside_envelope(*launch)
            || !point_inside_envelope(*impact)
            || splash.is_some_and(|r| r < Fx::ZERO || r > Fx::from_num(COORD_ENVELOPE))
        {
            return Err(E::ShellOutsideEnvelope(i));
        }
        if !self.minted(*shooter) {
            return Err(E::UnmintedShellShooter(i));
        }
        if launched_at > arrival {
            return Err(E::ShellLaunchedAfterArrival(i));
        }
        Ok(())
    }

    fn validate_warhead(
        &self,
        i: usize,
        w: &Warhead,
        decided: bool,
    ) -> Result<(), StateIntegrityError> {
        use StateIntegrityError as E;
        let Warhead {
            id: _,
            player,
            silo,
            launch,
            impact,
            launched_at,
            arrival,
        } = w;
        if usize::from(player.0) >= self.players.len() {
            return Err(E::ForeignWarheadOwner(i));
        }
        // Landing compares positions against footprints and bodies, and
        // the flight is always the silo-to-impact line at warhead speed;
        // a decided match carries no warheads.
        let on_map = |p: Vec2Fx| {
            p.x >= Fx::ZERO
                && p.y >= Fx::ZERO
                && p.x <= Fx::from_num(self.map.width())
                && p.y <= Fx::from_num(self.map.height())
        };
        if decided
            || !on_map(*launch)
            || !on_map(*impact)
            || *launched_at >= self.tick
            || *arrival < self.tick
            || arrival - launched_at != crate::stats::WARHEAD.flight_ticks(*launch, *impact)
            || silo.0 >= self.next_building_id
        {
            return Err(E::InvalidWarheadFlight(i));
        }
        Ok(())
    }

    fn validate_vision(
        &self,
        seat: PlayerId,
        v: &crate::vision::Vision,
    ) -> Result<(), StateIntegrityError> {
        use StateIntegrityError as E;
        let i = usize::from(seat.0);
        for ghost in v.ghosts() {
            // Renderers index the player table with this to pick a tint; an
            // owner outside it is a panic, not a wrong color.
            if usize::from(ghost.owner.0) >= self.players.len() {
                return Err(E::ForeignGhostOwner(seat));
            }
            if self.players[usize::from(ghost.owner.0)].team == self.players[i].team {
                return Err(E::FriendlyGhost(seat));
            }
            if !tile_inside_envelope(ghost.anchor) {
                return Err(E::GhostOutsideEnvelope(seat));
            }
            if usize::from(ghost.tier) >= ghost.kind.tiers().len() {
                return Err(E::GhostTierBeyondLadder(seat));
            }
            if ghost.hp == 0 || ghost.hp > ghost.kind.tier_stats(ghost.tier).max_hp {
                return Err(E::GhostHpOutOfRange(seat));
            }
        }
        // The real sort key carries the owner; two seats can hold footprints
        // a memory records under the same corner. Equal keys are reachable:
        // a buried charge's memory can outlive its unseen anchor while its
        // owner builds over the spot.
        let ghost_key = |g: &crate::vision::GhostBuilding| (g.anchor.y, g.anchor.x, g.owner);
        if !v
            .ghosts()
            .windows(2)
            .all(|a| ghost_key(&a[0]) <= ghost_key(&a[1]))
        {
            return Err(E::UnsortedGhosts(seat));
        }
        if v.contacts().iter().any(|c| !tile_inside_envelope(c.tile)) {
            return Err(E::ContactOutsideEnvelope(seat));
        }
        // Blips are sorted and deduplicated every refresh.
        if !v
            .contacts()
            .windows(2)
            .all(|a| a[0].sort_key() < a[1].sort_key())
        {
            return Err(E::UnsortedContacts(seat));
        }
        let incidents = v.salvage_incidents();
        if incidents.len() > crate::stats::HARVEST_INCIDENT_CAP {
            return Err(E::OverlongSalvageIncidentMemory(seat));
        }
        if incidents
            .iter()
            .any(|incident| !tile_inside_envelope(incident.tile))
        {
            return Err(E::SalvageIncidentOutsideEnvelope(seat));
        }
        if self.result.is_none()
            && incidents
                .iter()
                .any(|incident| incident.expires_at < self.tick)
        {
            return Err(E::ExpiredSalvageIncident(seat));
        }
        let expiry_horizon = self
            .tick
            .saturating_add(crate::stats::HARVEST_INCIDENT_MEMORY_TICKS);
        if incidents
            .iter()
            .any(|incident| incident.expires_at > expiry_horizon)
        {
            return Err(E::SalvageIncidentExpiryBeyondHorizon(seat));
        }
        if !incidents
            .windows(2)
            .all(|a| (a[0].tile.y, a[0].tile.x) < (a[1].tile.y, a[1].tile.x))
        {
            return Err(E::UnsortedSalvageIncidents(seat));
        }
        if !v.tracking_valid(self, seat) {
            return Err(E::InvalidContactTracking(seat));
        }
        if let Some(other) = (0..i).find(|&j| self.players[j].team == self.players[i].team)
            && !v.shares_tracking(&self.vision[other])
        {
            return Err(E::InvalidContactTracking(seat));
        }
        if !v.warheads().windows(2).all(|a| a[0].id < a[1].id)
            || v.warheads().iter().any(|track| {
                track.exposure == 0
                    || track.exposure
                        > crate::stats::WARHEAD_TRACK_TICKS + crate::stats::WARHEAD_LOCK_TICKS
                    || self
                        .warheads
                        .binary_search_by_key(&track.id, |w| w.id)
                        .map_or(true, |at| !self.hostile(seat, self.warheads[at].player))
            })
            || (0..i)
                .find(|&j| self.players[j].team == self.players[i].team)
                .is_some_and(|other| self.vision[other].warheads() != v.warheads())
        {
            return Err(E::InvalidWarheadTracking(seat));
        }
        Ok(())
    }

    /// Whether this run ever handed out `target`'s id — the permissive
    /// reference rule. Dangling is fine; unminted is forgery.
    fn minted(&self, target: Target) -> bool {
        match target {
            Target::Unit(id) => id.0 < self.next_unit_id,
            Target::Building(id) => id.0 < self.next_building_id,
        }
    }

    /// Whether `player` currently sees `pos`.
    pub fn can_see(&self, player: PlayerId, pos: TilePos) -> bool {
        self.vision(player).visible(pos)
    }

    /// Whether `player` currently sees any tile `unit` claims.
    pub fn sees_unit(&self, player: PlayerId, unit: &Unit) -> bool {
        self.vision(player).sees_unit(unit)
    }

    /// A test of which tiles `player`'s Harvesters would judge too
    /// dangerous to collect from on their own. It reads one snapshot of
    /// what that player knows now: armed machines in sight, remembered
    /// armed buildings, radar blips, and recent hits from unseen
    /// attackers.
    pub fn salvage_danger(&self, player: PlayerId) -> impl Fn(TilePos) -> bool + use<> {
        let danger = crate::vision::GroundSalvageDanger::capture(self, player);
        move |tile| danger.contains(tile)
    }

    /// The movement domain of a live hostile target under the viewer's
    /// current true sight. Radar contacts and remembered buildings do not
    /// identify a target and therefore never satisfy this query.
    pub(crate) fn visible_hostile_target_domain(
        &self,
        viewer: PlayerId,
        target: Target,
    ) -> Option<crate::stats::Domain> {
        match target {
            Target::Unit(id) => self.unit(id).and_then(|unit| {
                (unit.hp > 0 && self.hostile(viewer, unit.player) && self.sees_unit(viewer, unit))
                    .then_some(unit.domain())
            }),
            Target::Building(id) => self
                .building(id)
                .filter(|building| self.sees_hostile_building(viewer, building))
                .map(|_| crate::stats::Domain::Ground),
        }
    }

    /// Whether a building already in hand is a live hostile target under the
    /// viewer's current true sight.
    pub(crate) fn sees_hostile_building(&self, viewer: PlayerId, building: &Building) -> bool {
        building.hp > 0
            && self.hostile(viewer, building.player)
            && building.tiles().any(|tile| self.can_see(viewer, tile))
            && self.building_apparent(viewer, building)
    }

    /// Whether two seats are enemies. Every combat, targeting, and
    /// detection decision routes through this — teammates are never
    /// valid victims, and a seat is never hostile to itself.
    pub fn hostile(&self, a: PlayerId, b: PlayerId) -> bool {
        self.players[a.0 as usize].team != self.players[b.0 as usize].team
    }

    /// Shells currently in flight, in launch order.
    pub fn shells(&self) -> &[Shell] {
        &self.shells
    }

    /// Falling aircraft awaiting their authoritative ground impact.
    pub fn aircraft_crashes(&self) -> &[AircraftCrash] {
        &self.aircraft_crashes
    }

    /// Every warhead in flight, omnisciently. Fog-honest readers use
    /// [`State::known_warheads`].
    pub fn warheads(&self) -> &[Warhead] {
        &self.warheads
    }

    /// The next warhead id, never reused.
    pub(crate) fn mint_warhead_id(&mut self) -> crate::ids::WarheadId {
        let id = crate::ids::WarheadId(self.next_warhead_id);
        self.next_warhead_id += 1;
        id
    }

    /// The seats on the winning team, in id order — empty until a
    /// victory is declared.
    pub fn winners(&self) -> Vec<PlayerId> {
        match self.result {
            Some(GameResult::Victory { team }) => (0..self.players.len())
                .filter(|&i| self.players[i].team == team)
                .map(PlayerId::from_index)
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Rebuilds every player's visible set; runs each tick and once at
    /// scenario build so tick 0 already has sight.
    pub(crate) fn refresh_vision(&mut self) {
        crate::vision::refresh(self);
        self.reconcile_attack_knowledge();
    }

    pub(crate) fn reconcile_attack_knowledge(&mut self) {
        let keep: Vec<bool> = self
            .buildings
            .iter()
            .map(|building| {
                building.focus.is_none_or(|target| {
                    building.built()
                        && self
                            .attack_view(building.player, target)
                            .is_some_and(|view| {
                                building
                                    .stats()
                                    .weapons
                                    .first()
                                    .is_some_and(|weapon| weapon.targets.covers(view.domain))
                            })
                })
            })
            .collect();
        for (building, keep) in self.buildings.iter_mut().zip(keep) {
            if !keep {
                building.focus = None;
            }
        }
        for index in 0..self.units.len() {
            while let Order::Attack { target, resume, .. } = self.units[index].order {
                let unit = &self.units[index];
                let stats = unit.kind.stats();
                if self.attack_view(unit.player, target).is_some_and(|view| {
                    stats.can_target(view.domain)
                        || (stats.demolition.is_some()
                            && view.domain == crate::stats::Domain::Ground)
                }) {
                    break;
                }
                self.units[index].complete_attack(resume);
            }
        }
    }

    /// Remembers only where this team suffered combat damage, never where
    /// the attacker stood. Every teammate receives the same deterministic
    /// record so team-shared vision cannot depend on seat iteration order.
    pub(crate) fn record_salvage_incident(&mut self, victim: PlayerId, tile: TilePos) {
        let team = self.player(victim).team;
        let expires_at = self
            .tick
            .saturating_add(crate::stats::HARVEST_INCIDENT_MEMORY_TICKS)
            .saturating_add(1);
        for (player, vision) in self.players.iter().zip(&mut self.vision) {
            if player.team == team {
                vision.remember_salvage_incident(tile, expires_at);
            }
        }
    }

    /// Mutable access to a player.
    pub(crate) fn player_mut(&mut self, id: PlayerId) -> &mut Player {
        &mut self.players[id.0 as usize]
    }

    /// Looks up a living unit.
    pub fn unit(&self, id: UnitId) -> Option<&Unit> {
        self.units
            .binary_search_by_key(&id, |u| u.id)
            .ok()
            .map(|i| &self.units[i])
    }

    /// The body escorts of unit `id` follow: the unit itself while it is in
    /// the world, or the transport carrying it. `None` once it has left play.
    pub fn escorted(&self, id: UnitId) -> Option<&Unit> {
        self.unit(id).or_else(|| {
            self.units
                .iter()
                .find(|carrier| carrier.cargo.iter().any(|rider| rider.id == id))
        })
    }

    /// Every drone flies for its owner's live machine with a bay, and every
    /// bay, with its flying drones, stays within what it admits.
    fn validate_drones(&self) -> Result<(), StateIntegrityError> {
        use StateIntegrityError as E;
        for drone in &self.units {
            let Some(host) = drone.host else {
                continue;
            };
            let bound = self
                .unit(host)
                .is_some_and(|u| u.player == drone.player && u.bay.is_some());
            if !bound {
                return Err(E::StrayDrone(drone.id));
            }
        }
        for u in &self.units {
            let (Some(bay), Some(stats)) = (u.bay, u.kind.stats().bay) else {
                continue;
            };
            let flying = self.units.iter().filter(|d| d.host == Some(u.id)).count();
            let held = usize::from(bay.docked) + usize::from(bay.ordered) + flying;
            if held > usize::from(stats.capacity)
                || bay.progress >= stats.build_ticks
                || (bay.ordered == 0 && bay.progress > 0)
                || bay.alert > crate::stats::RECALL_TICKS
            {
                return Err(E::InvalidDroneBay(u.id));
            }
        }
        Ok(())
    }

    /// A unit that escorts itself through a chain of escorts, counting every
    /// escort duty in every program, active and queued. Dispatch never
    /// closes such a loop, so only a forged state holds one.
    fn escort_loop(&self) -> Option<UnitId> {
        let index = |id: UnitId| self.units.binary_search_by_key(&id, |u| u.id).ok();
        let escorted = |slot: usize| -> Vec<usize> {
            let unit = &self.units[slot];
            std::iter::once(&unit.order)
                .chain(&unit.queue)
                .filter_map(Order::escorted)
                .filter_map(index)
                .collect()
        };
        // Depth-first over the escort graph: 1 marks a unit on the current
        // chain, 2 one whose chains are all known to end.
        let mut mark = vec![0u8; self.units.len()];
        for start in 0..self.units.len() {
            if mark[start] != 0 {
                continue;
            }
            mark[start] = 1;
            let mut chain = vec![(start, escorted(start), 0usize)];
            while let Some(top) = chain.len().checked_sub(1) {
                let (slot, next) = (chain[top].0, chain[top].2);
                let Some(&target) = chain[top].1.get(next) else {
                    mark[slot] = 2;
                    chain.pop();
                    continue;
                };
                chain[top].2 += 1;
                match mark[target] {
                    0 => {
                        mark[target] = 1;
                        chain.push((target, escorted(target), 0));
                    }
                    1 => return Some(self.units[target].id),
                    _ => {}
                }
            }
        }
        None
    }

    /// Mutable lookup of a living unit.
    pub(crate) fn unit_mut(&mut self, id: UnitId) -> Option<&mut Unit> {
        self.units
            .binary_search_by_key(&id, |u| u.id)
            .ok()
            .map(|i| &mut self.units[i])
    }

    /// Looks up a standing building.
    pub fn building(&self, id: BuildingId) -> Option<&Building> {
        self.buildings
            .binary_search_by_key(&id, |b| b.id)
            .ok()
            .map(|i| &self.buildings[i])
    }

    /// Mutable lookup of a standing building.
    pub(crate) fn building_mut(&mut self, id: BuildingId) -> Option<&mut Building> {
        self.buildings
            .binary_search_by_key(&id, |b| b.id)
            .ok()
            .map(|i| &mut self.buildings[i])
    }

    /// Every building covering `pos`, in id order. A buried charge can share
    /// ground with an unstarted site; filter by visibility, ownership, or
    /// movement rules before choosing an occupant.
    pub fn buildings_at(&self, pos: TilePos) -> impl Iterator<Item = &Building> {
        self.buildings.iter().filter(move |b| b.contains(pos))
    }

    /// Whether a unit may stand on `pos`: ground terrain, no live scrap, no
    /// building. Units never block tiles; overlap is resolved by the
    /// separation phase instead. A buried charge blocks nothing: a mine
    /// that closed its tile could never be stepped on, and enemy
    /// pathfinding routing around it would leak its position.
    pub fn passable(&self, pos: TilePos) -> bool {
        self.map.terrain_passable(pos) && !self.building_blocks(pos)
    }

    /// Whether a non-stealthy building covers `pos`, by the derived
    /// occupancy grid. Equivalent to scanning every building's
    /// footprint; the grid is maintained at the single placement
    /// funnel and every removal site.
    fn building_blocks(&self, pos: TilePos) -> bool {
        self.ground_terrain().building_blocks(pos)
    }

    /// Whether a footprint hostile to `player` closes `pos`. A body driving
    /// off occupied ground may cross any other.
    pub(crate) fn hostile_footprint_at(&self, player: PlayerId, pos: TilePos) -> bool {
        self.buildings_at(pos).any(|building| {
            !building.kind.is_stealthy()
                && !building.provisional()
                && self.hostile(player, building.player)
        })
    }

    /// Ground passability borrowed apart from the unit table, so a movement
    /// pass can move bodies while consulting the same terrain and building
    /// occupancy [`State::passable`] reads.
    pub(crate) fn ground_terrain(&self) -> GroundTerrain<'_> {
        GroundTerrain::new(&self.map, &self.building_occupancy)
    }

    /// Friendly ground bodies of `player`'s side that stand still right
    /// now. Only friendly bodies count: steering around an unseen enemy
    /// before contact would leak its position.
    pub(crate) fn parked_bodies(&self, player: PlayerId) -> ParkedBodies {
        let (width, height) = (self.map.width(), self.map.height());
        let mut parked = ParkedBodies {
            width,
            height,
            bits: vec![0; cell_count(width, height).div_ceil(64)],
        };
        for unit in self.units.iter().filter(|u| {
            u.hp > 0
                && u.domain() == crate::stats::Domain::Ground
                && u.drive_speed() == Fx::ZERO
                && u.path.is_none()
                && !self.hostile(player, u.player)
        }) {
            for tile in unit.body_tiles() {
                if let Some(i) = parked.index(tile) {
                    parked.bits[i / 64] |= 1 << (i % 64);
                }
            }
        }
        parked
    }

    /// Marks or clears one building's footprint in the occupancy grid.
    /// Stealthy kinds never mark: a buried charge blocks nothing.
    pub(crate) fn stamp_building_occupancy(&mut self, building_index: usize, present: bool) {
        let b = &self.buildings[building_index];
        if b.kind.is_stealthy() || b.provisional() {
            return;
        }
        let (anchor, kind) = (b.anchor, b.kind);
        let width = self.map.width();
        let height = self.map.height();
        let (w, h) = kind.size();
        for dy in 0..h {
            for dx in 0..w {
                // Checked: this runs on deserialized bytes BEFORE the
                // trust boundary rejects them, so a forged anchor near
                // i32::MAX must fall out of bounds, not overflow.
                let (Some(x), Some(y)) = (anchor.x.checked_add(dx), anchor.y.checked_add(dy))
                else {
                    continue;
                };
                if x < 0 || y < 0 || x >= width || y >= height {
                    continue;
                }
                let idx = TilePos::new(x, y).row_major(width);
                if let Some(cell) = self.building_occupancy.get_mut(idx) {
                    *cell = u8::from(present);
                }
            }
        }
    }

    /// Rebuilds the whole occupancy grid from the building list — the
    /// deserialization path's one-shot recovery of derived state.
    pub(crate) fn rebuild_building_occupancy(&mut self) {
        let cells = cell_count(self.map.width(), self.map.height());
        self.building_occupancy.clear();
        self.building_occupancy.resize(cells, 0);
        for index in 0..self.buildings.len() {
            self.stamp_building_occupancy(index, true);
        }
    }

    /// Whether a unit of the given movement domain may stand on `pos`.
    /// Ground units need open terrain and no building; air units need any
    /// map tile except a peak.
    pub fn passable_for(&self, domain: crate::stats::Domain, pos: TilePos) -> bool {
        match domain {
            crate::stats::Domain::Ground => self.passable(pos),
            crate::stats::Domain::Air => {
                self.map.tile(pos).is_some_and(|t| !t.terrain.blocks_air())
            }
        }
    }

    /// Whether `viewer` may observe this building's current condition, over
    /// and above ordinary tile sight. Retained memory is a separate surface.
    /// True for everything except a completed enemy
    /// [`BuildingKind::is_stealthy`] charge, which must be
    /// actively detected: an allied machine with
    /// [`crate::stats::UnitStats::charge_detection`] within
    /// [`crate::stats::CHARGE_SCOUT_DETECT_RADIUS`] tiles, or an allied
    /// built Array whose detection ring covers it —
    /// [`crate::stats::CHARGE_BASE_ARRAY_DETECT_RADIUS`] at base tier,
    /// widening to [`crate::stats::CHARGE_ARRAY_DETECT_RADIUS`] once the
    /// mast is upgraded to a Deep Array (tier 1+). A mast still under
    /// construction sees nothing.
    /// Every fog-honest surface — ghosts, targeting, views, rendering —
    /// must consult this before showing a hostile building.
    pub fn building_apparent(&self, viewer: PlayerId, building: &Building) -> bool {
        self.apparent_unless_undetected(viewer, building, |anchor| {
            self.charge_detected_at(viewer, anchor)
        })
    }

    /// [`Self::building_apparent`] with the detection test supplied, so a
    /// [`ChargeLens`] can answer it from its gathered detectors.
    fn apparent_unless_undetected(
        &self,
        viewer: PlayerId,
        building: &Building,
        detected: impl FnOnce(TilePos) -> bool,
    ) -> bool {
        if building.provisional() {
            return !self.hostile(viewer, building.player);
        }
        if !building.kind.is_stealthy()
            || !building.built()
            || !self.hostile(viewer, building.player)
        {
            return true;
        }
        detected(building.anchor)
    }

    /// Detector coverage is independent of whether a charge still exists.
    pub(crate) fn charge_detected_at(&self, viewer: PlayerId, anchor: TilePos) -> bool {
        self.charge_detectors(viewer).any(|d| d.covers(anchor))
    }

    /// Every detector allied to `viewer`, scouts before masts.
    fn charge_detectors(&self, viewer: PlayerId) -> impl Iterator<Item = ChargeDetector> + '_ {
        let scouts = self
            .units
            .iter()
            .filter(move |u| {
                u.hp > 0 && !self.hostile(viewer, u.player) && u.kind.stats().charge_detection
            })
            .map(|u| ChargeDetector::Scout(u.tile()));
        let masts = self
            .buildings
            .iter()
            .filter(move |b| {
                b.hp > 0
                    && b.built()
                    && b.kind == BuildingKind::Array
                    && !self.hostile(viewer, b.player)
            })
            .map(|b| ChargeDetector::Mast {
                anchor: b.anchor,
                radius: if b.tier >= 1 {
                    crate::stats::CHARGE_ARRAY_DETECT_RADIUS
                } else {
                    crate::stats::CHARGE_BASE_ARRAY_DETECT_RADIUS
                },
            });
        scouts.chain(masts)
    }

    /// Spawns a unit at full health. Position is the caller's problem to
    /// validate.
    pub(crate) fn spawn_unit(&mut self, player: PlayerId, kind: UnitKind, pos: Vec2Fx) -> UnitId {
        let id = UnitId(self.next_unit_id);
        self.next_unit_id += 1;
        // Every unit starts facing the map centre, so mirrored seats'
        // units start mirrored, aircraft included: a turning flyer's first
        // heading decides how long it takes to come about.
        let heading = chassis::compass::heading_of(
            Vec2Fx::new(
                Fx::from_num(self.map.width()) / 2,
                Fx::from_num(self.map.height()) / 2,
            ) - pos,
        );
        self.units
            .push(Unit::at_rest(id, player, kind, pos, heading));
        id
    }

    /// Places a building at full health. Footprint validity is the caller's
    /// problem.
    pub(crate) fn place_building(
        &mut self,
        player: PlayerId,
        kind: BuildingKind,
        anchor: TilePos,
    ) -> BuildingId {
        let building = self.mint_building(player, kind, anchor);
        self.insert_building(building)
    }

    /// The bearing from `point` to the map's centre.
    pub(crate) fn centre_bearing(&self, point: Vec2Fx) -> u8 {
        chassis::compass::heading_of(
            Vec2Fx::new(
                Fx::from_num(self.map.width()) / 2,
                Fx::from_num(self.map.height()) / 2,
            ) - point,
        )
    }

    /// A complete, full-health building with the next id, not yet placed.
    fn mint_building(&mut self, player: PlayerId, kind: BuildingKind, anchor: TilePos) -> Building {
        let id = BuildingId(self.next_building_id);
        self.next_building_id += 1;
        // Decks and guns start toward the map centre, as units face on
        // spawn, so mirrored works come about in mirrored time.
        let facing_centre =
            self.centre_bearing(crate::geometry::footprint_center(anchor, kind.size()));
        Building {
            id,
            player,
            kind,
            anchor,
            hp: kind.base_stats().max_hp,
            queue: std::collections::VecDeque::new(),
            phase: BuildingPhase::Built { training: 0 },
            rally: None,
            focus: None,
            tier: 0,
            cooldown: 0,
            salvage_drained: 0,
            salvage_credited: 0,
            magazine: kind.magazine().map(|stats| Magazine {
                loaded: 0,
                ordered: 0,
                progress: 0,
                autofill: stats.autofill,
            }),
            launcher: kind.silo().map(|_| Launcher {
                heading: facing_centre,
                target: None,
                ready: 0,
            }),
            gun_heading: kind.fires().then_some(facing_centre),
        }
    }

    /// Appends a minted building and marks its occupancy.
    fn insert_building(&mut self, building: Building) -> BuildingId {
        let id = building.id;
        self.buildings.push(building);
        self.stamp_building_occupancy(self.buildings.len() - 1, true);
        id
    }

    /// Claims ground for a construction site: blocks the footprint at once
    /// but starts at a fifth of its hit points, unfinished. Site validity
    /// is checked by [`State::can_place`] at the command layer.
    pub(crate) fn place_site(
        &mut self,
        player: PlayerId,
        kind: BuildingKind,
        anchor: TilePos,
    ) -> BuildingId {
        let mut site = self.mint_building(player, kind, anchor);
        site.phase = BuildingPhase::Site { progress: 0 };
        site.hp = kind.base_stats().max_hp / 5;
        self.insert_building(site)
    }

    /// Records a paid blueprint whose ground is not yet verified. It claims
    /// no occupancy, so it may sit over a building its owner cannot see.
    pub(crate) fn place_provisional_site(
        &mut self,
        player: PlayerId,
        kind: BuildingKind,
        anchor: TilePos,
    ) -> BuildingId {
        let mut site = self.mint_building(player, kind, anchor);
        site.phase = BuildingPhase::Provisional;
        site.hp = kind.base_stats().max_hp / 5;
        self.insert_building(site)
    }

    /// Undoes a just-placed site completely, id counter included — for
    /// validation paths that must leave no trace on rejection (a rejected
    /// command must not move the state hash).
    pub(crate) fn retract_site(&mut self, id: BuildingId) {
        debug_assert_eq!(
            id.0 + 1,
            self.next_building_id,
            "only the newest site retracts"
        );
        if let Some(index) = self.buildings.iter().position(|b| b.id == id) {
            self.stamp_building_occupancy(index, false);
        }
        self.buildings.retain(|b| b.id != id);
        self.next_building_id = id.0;
    }
}

/// How far outside the map a coordinate may sit before a snapshot is
/// refused, in tiles: eight times the largest legal map edge. Deliberately
/// a generous sanity box rather than a map-relative bound — see
/// [`State::validate_invariants`] for why. Every offset the sim adds to a
/// coordinate (footprint sizes, ring scans, vision spans) is smaller than
/// one map edge, so nothing inside this box can overflow the unchecked
/// arithmetic downstream, and the squared distances it feeds stay far
/// inside [`Fx`]'s integer range.
const COORD_ENVELOPE: i32 = 8 * MAX_MAP_EDGE as i32;

/// Ceiling on the tick meters a snapshot may carry ([`Unit::progress`] and
/// a built building's training meter), far above any meter a match of
/// playable length reaches. Construction and upgrade meters are bounded
/// tighter, by their rung's build time. The live weld/salvage meters saturate just short of here (the
/// economy brain's `metered` read), so a torch held on one job for millions
/// of ticks keeps billing at its marginal rate and its meter never wraps.
pub(crate) const PROGRESS_ENVELOPE: u32 = 1 << 21;

/// Ceiling on a building's cumulative salvage ledger. Repairing a
/// half-stripped building and stripping it again legitimately drains more
/// hp than it ever had, so the ledger has no semantic bound — only this
/// one, which keeps the running total clear of a `u32` wrap.
const SALVAGE_LEDGER_CEILING: u32 = u32::MAX / 2;

/// Ceiling on a snapshot's tick. `State::tick` increments unchecked; a
/// forged `u64::MAX` panics on the very next step in a debug build and
/// wraps in release. Half the type is billions of years at 20 ticks/s.
const TICK_ENVELOPE: u64 = u64::MAX / 2;

/// Ceiling on the id counters. Spawning increments unchecked, and a
/// wrapped counter would alias live ids and break the sorted-id
/// invariant this same validator enforces. Half the type leaves two
/// billion spawns of headroom.
const ID_COUNTER_ENVELOPE: u32 = u32::MAX / 2;

/// Whether a tile coordinate sits inside [`COORD_ENVELOPE`].
fn tile_inside_envelope(t: TilePos) -> bool {
    t.x >= -COORD_ENVELOPE
        && t.x <= COORD_ENVELOPE
        && t.y >= -COORD_ENVELOPE
        && t.y <= COORD_ENVELOPE
}

/// Whether a world position sits inside [`COORD_ENVELOPE`].
fn point_inside_envelope(p: Vec2Fx) -> bool {
    let (lo, hi) = (Fx::from_num(-COORD_ENVELOPE), Fx::from_num(COORD_ENVELOPE));
    p.x >= lo && p.x <= hi && p.y >= lo && p.y <= hi
}

/// Whether a goal's clicked tile, slot, and endpoint sit inside the
/// envelope. None needs to be on the map: endpoint scans clamp onto it. A
/// pending slot's rank and frame need no bound, since any rank names a slot.
fn goal_inside_envelope(goal: &Goal) -> bool {
    tile_inside_envelope(goal.tile())
        && tile_inside_envelope(goal.target())
        && goal.endpoint.is_none_or(tile_inside_envelope)
}

/// Whether every goal an order carries keeps its slot distinct from its
/// clicked tile and its endpoint distinct from its target. Those shapes are
/// never stored, so each order has one serialized form.
fn order_goals_canonical(order: &Order) -> bool {
    match order {
        Order::Run { goal }
        | Order::Hunt { goal }
        | Order::Advance { goal }
        | Order::Tend { goal, .. } => goal.canonical(),
        Order::Unload { at, .. } => at.canonical(),
        Order::Attack { resume, .. } => resume
            .and_then(|resume| resume.goal())
            .is_none_or(|goal| goal.canonical()),
        _ => true,
    }
}

/// Whether every coordinate an order names sits inside the envelope. The
/// match is exhaustive on purpose: a new [`Order`] variant carrying a tile
/// must decide its row here rather than slip through a catch-all.
fn order_inside_envelope(order: &Order) -> bool {
    match order {
        Order::Idle
        | Order::ReturnCargo { .. }
        | Order::Build { .. }
        | Order::Repair { .. }
        | Order::Salvage { .. }
        | Order::RepairUnit { .. }
        | Order::Accompany { .. } => true,
        Order::Run { goal }
        | Order::Hunt { goal }
        | Order::Advance { goal }
        | Order::Tend { goal, .. } => goal_inside_envelope(goal),
        Order::Harvest { node, anchor, .. } => {
            tile_inside_envelope(*node) && tile_inside_envelope(*anchor)
        }
        Order::Attack { resume, .. } => resume
            .and_then(|resume| resume.goal())
            .is_none_or(|goal| goal_inside_envelope(&goal)),
        Order::Found { anchor, .. } => tile_inside_envelope(*anchor),
        Order::Board { .. } => true,
        Order::Unload { at, .. } => goal_inside_envelope(at),
        Order::Land { goal, from } => {
            tile_inside_envelope(*goal) && from.is_none_or(tile_inside_envelope)
        }
        Order::Scout { tile } => tile_inside_envelope(*tile),
        Order::Retire { pad, .. } => pad.is_none_or(tile_inside_envelope),
    }
}

/// A Harvest order may change its active source, but only within the fixed
/// local work zone.
fn harvest_order_inside_zone(order: &Order) -> bool {
    match order {
        Order::Harvest { node, anchor, .. } => {
            node.chebyshev(*anchor) <= crate::stats::HARVEST_ZONE_RADIUS
        }
        _ => true,
    }
}

/// The entities an order names — exhaustive for the same reason as
/// [`order_inside_envelope`].
fn order_references(order: &Order) -> impl Iterator<Item = Target> {
    let unit = |id: UnitId| Some(Target::Unit(id));
    let names: [Option<Target>; 2] = match *order {
        Order::Idle
        | Order::Run { .. }
        | Order::Harvest { .. }
        | Order::Hunt { .. }
        | Order::Advance { .. }
        | Order::Found { .. }
        | Order::Land { .. }
        | Order::Unload { .. }
        | Order::Scout { .. } => [None, None],
        Order::Attack { target, resume, .. } => [
            target.entity(),
            match resume {
                Some(Resume::Accompany(escorted)) => unit(escorted),
                Some(Resume::Hunt(_)) | None => None,
            },
        ],
        Order::Build { site } => [Some(Target::Building(site)), None],
        Order::ReturnCargo { foundry, .. } => [Some(Target::Building(foundry)), None],
        Order::Repair { building } | Order::Salvage { building } => {
            [Some(Target::Building(building)), None]
        }
        Order::RepairUnit { unit: patient } => [unit(patient), None],
        Order::Board { transport } => [unit(transport), None],
        Order::Accompany {
            unit: escorted,
            weld,
        } => [unit(escorted), weld.and_then(unit)],
        Order::Retire { crucible, .. } => [Some(Target::Building(crucible)), None],
        Order::Tend { weld, .. } => [weld, None],
    };
    names.into_iter().flatten()
}

/// Whether every coordinate a unit carries — body, orders, walk, tether —
/// sits inside the envelope.
fn unit_inside_envelope(u: &Unit) -> bool {
    point_inside_envelope(u.pos)
        && order_inside_envelope(&u.order)
        && u.queue.iter().all(order_inside_envelope)
        && u.leash.is_none_or(|l| tile_inside_envelope(l.anchor))
        && u.path.as_ref().is_none_or(|p| {
            tile_inside_envelope(p.goal) && p.waypoints.iter().copied().all(tile_inside_envelope)
        })
}

/// Whether a building's salvage ledger is a coherent record of the hp
/// stripped from it: crediting reads the cumulative drain through one
/// exact formula, so any other pairing is a forgery — and one that would
/// underflow the `u32` subtraction the next drain performs.
fn salvage_ledger_coherent(b: &Building) -> bool {
    if b.salvage_drained > SALVAGE_LEDGER_CEILING {
        return false;
    }
    let stats = b.stats();
    let basis = stats.construction.map_or(0, |c| c.cost);
    let target =
        u64::from(b.salvage_drained) * u64::from(basis) * crate::stats::SALVAGE_REFUND_PERMILLE
            / (1000 * u64::from(stats.max_hp));
    u64::from(b.salvage_credited) == target
}

/// Why [`State::place_refusal`] said no. Own-state facts only — no
/// variant may ever derive from what fog hides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceRefusal {
    /// The kind is scenario-authored, never player-buildable.
    NotConstructible,
    /// A footprint tile is not currently visible.
    Fog,
    /// A footprint tile is impassable ground.
    Terrain,
    /// A building already holds a footprint tile.
    Building,
    /// A hostile machine holds a footprint tile (friendly machines
    /// make way instead of blocking).
    Unit,
    /// The owner has not completed the kind's required tech buildings.
    Prerequisite,
    /// An Extractor rebuilds only on a map-authored derelict frame.
    FrameRequired,
    /// The footprint overlaps a derelict frame, which no other kind may
    /// pave over.
    FrameBlocked,
}

#[cfg(test)]
mod tests;

/// Why a deserialized snapshot was refused. Every variant is a structural
/// contradiction the tick pipeline is entitled to assume away, and every
/// one names the entity that broke it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StateIntegrityError {
    /// Open-ended sandboxes cannot carry match elimination or victory state.
    #[error("sandbox contains an elimination stamp or a game result")]
    SandboxElimination,
    /// A walking or transported unit holds more scrap than its harvest gear permits.
    #[error("unit {0} carries scrap beyond its harvest capacity")]
    ScrapBeyondCapacity(UnitId),
    /// An incomplete cargo release has invalid timing, cargo, or destination.
    #[error("unit {0} carries invalid unloading state")]
    InvalidUnloading(UnitId),
    /// A precise ground-work endpoint does not belong to its final waypoint.
    #[error("unit {0} carries an invalid work endpoint")]
    InvalidWorkEndpoint(UnitId),
    /// Only a ground chassis drives a route across footprints.
    #[error("unit {0} follows a clearing route without a ground chassis")]
    InvalidClearingRoute(UnitId),
    /// A stored aircraft displacement is not physically bounded.
    #[error("unit {0} carries invalid airborne motion")]
    InvalidAirMotion(UnitId),
    /// A pending crash has invalid identity, geometry, timing, or ordering.
    #[error("invalid pending aircraft crash {0}")]
    InvalidAircraftCrash(usize),
    /// The player table is empty.
    #[error("no players")]
    NoPlayers,
    /// The player table is longer than [`PlayerId`] can address.
    #[error("more players than a player id can address")]
    TooManyPlayers,
    /// A player sits on a team index no seat carries.
    #[error("player {0} sits on a team outside the table")]
    ForeignTeam(PlayerId),
    /// A player's finite emergency-income ledger exceeds its public cap
    /// or claims both an armed and active cycle.
    #[error("player {0} carries an invalid recovery ledger")]
    InvalidRecoveryLedger(PlayerId),
    /// A victory names a team no player carries.
    #[error("victory names team {0}, which no player carries")]
    UnknownVictoryTeam(u8),
    /// The map grid's dimensions disagree with its cells.
    #[error("map grid dimensions disagree with its cells")]
    MalformedMapGrid,
    /// The map is larger than the supported maximum per side.
    #[error("map is {width}x{height}; the supported maximum is {MAX_MAP_EDGE} per side")]
    MapTooLarge {
        /// Columns claimed by the grid.
        width: i32,
        /// Rows claimed by the grid.
        height: i32,
    },
    /// The vision table does not match the player list.
    #[error("vision table does not match the player list")]
    VisionTableMismatch,
    /// A contact history or its current observation is inconsistent.
    #[error("player {0} has invalid contact tracking")]
    InvalidContactTracking(PlayerId),
    /// A vision grid disagrees with the map dimensions.
    #[error("a vision table disagrees with the map dimensions")]
    MalformedVisionGrid,
    /// Units are not strictly sorted by id.
    #[error("units not strictly sorted by id")]
    UnsortedUnits,
    /// Buildings are not strictly sorted by id.
    #[error("buildings not strictly sorted by id")]
    UnsortedBuildings,
    /// A provisional scaffold carries physical building state.
    #[error("building {0} has invalid provisional state")]
    InvalidProvisionalSite(BuildingId),
    /// The unit id counter sits behind a live unit.
    #[error("unit id counter behind a live unit")]
    StaleUnitCounter,
    /// The building id counter sits behind a live building.
    #[error("building id counter behind a live building")]
    StaleBuildingCounter,
    /// The tick is past the envelope the pipeline's unchecked
    /// increment tolerates.
    #[error("tick beyond the sanity envelope")]
    TickBeyondEnvelope,
    /// A recorded elimination stamp past the same sanity envelope.
    #[error("player {0}'s elimination stamp lies beyond the sanity envelope")]
    EliminationBeyondEnvelope(PlayerId),
    /// A recorded elimination stamp later than the state's own tick.
    #[error("player {0}'s elimination stamp lies in the future")]
    EliminationInTheFuture(PlayerId),
    /// An id counter is past the envelope spawning tolerates.
    #[error("an id counter is beyond the sanity envelope")]
    IdCounterBeyondEnvelope,
    /// A unit is owned by a player outside the table.
    #[error("unit {0} is owned by a player outside the table")]
    ForeignUnitOwner(UnitId),
    /// A unit's hit points sit outside `(0, max_hp]`.
    #[error("unit {0} carries hit points its kind cannot hold")]
    UnitHpOutOfRange(UnitId),
    /// A unit's work meter is past the ceiling.
    #[error("unit {0} carries a work meter past the ceiling")]
    UnitProgressOutOfRange(UnitId),
    /// A unit's weapon cooldown is longer than that weapon's period, or a
    /// slot past its roster is armed.
    #[error("unit {0} carries a cooldown no weapon of its kind sets")]
    UnitCooldownOutOfRange(UnitId),
    /// Spade deployment exceeds its range or belongs to a non-siege unit, or
    /// a planted gun stands off its hull beyond its kind's traverse.
    #[error("unit {0} carries an invalid braced stance")]
    InvalidUnitBraces(UnitId),
    /// A stall counter at or past its replan bound, or on a body that is not
    /// walking a ground route.
    #[error("unit {0} carries an invalid stall counter")]
    InvalidStallTicks(UnitId),
    /// A detour retry scheduled further ahead than a failed search sets, or
    /// held by a unit that never harvests.
    #[error("unit {0} carries an invalid danger retry")]
    InvalidDangerRetry(UnitId),
    /// A machine stamped as damaged at or after the current tick.
    #[error("unit {0} was damaged at a tick that has not happened")]
    DamageStampInTheFuture(UnitId),
    /// A chase allowance or reacquisition cooldown exceeds its legal bound.
    #[error("unit {0} carries an invalid leash clock")]
    InvalidLeashClock(UnitId),
    /// Motor speed exceeds the chassis limit or belongs to a stationary/air body.
    #[error("unit {0} carries invalid ground motor speed")]
    InvalidGroundSpeed(UnitId),
    /// A unit's order queue is longer than [`crate::stats::ORDER_QUEUE_CAP`].
    #[error("unit {0} queues more orders than the cap allows")]
    OverlongUnitQueue(UnitId),
    /// A unit's body, order, walk, or tether names a coordinate outside
    /// the sanity envelope.
    #[error("unit {0} names a coordinate outside the envelope")]
    UnitOutsideEnvelope(UnitId),
    /// A walking order stores a slot on its own clicked tile, or an
    /// endpoint equal to its own target; neither is ever stored.
    #[error("unit {0} stores a slot or endpoint equal to its own goal")]
    NonCanonicalGoal(UnitId),
    /// An anchored Harvest order names a source outside its bounded work zone.
    #[error("unit {0} names a harvest source outside its work zone")]
    HarvestSourceOutsideZone(UnitId),
    /// Cargo delivery requires a worker and an own drop-off target.
    #[error("unit {0} has an invalid cargo delivery")]
    InvalidReturnCargo(UnitId),
    /// A unit rally names a unit that is out of play or another seat's.
    #[error("building {0} rallies to a unit that is gone or not its owner's")]
    InvalidRally(BuildingId),
    /// Escort duty names the escort itself or another seat's unit, loops
    /// back through other escorts, or puts a torch in a machine without
    /// harvest gear.
    #[error("unit {0} has an invalid escort duty")]
    InvalidEscort(UnitId),
    /// A tending walk is given to a machine without harvest gear, welds the
    /// welder itself or another seat's unit, holds a patient while queued,
    /// or sits outside a patrol beside other queued orders.
    #[error("unit {0} has an invalid tending walk")]
    InvalidTend(UnitId),
    /// A retirement names a foreign building or one that is no Crucible,
    /// gives a pad to an airframe that cannot land, or holds a full meter.
    #[error("unit {0} has an invalid retirement")]
    InvalidRetirement(UnitId),
    /// Only scouts take the Scout order.
    #[error("unit {0} scouts without being a scout")]
    InvalidScoutOrder(UnitId),
    /// A radar unfolds only on a kind that deploys one, up to its deploy
    /// time, and only while the machine stands still.
    #[error("unit {0} has an impossible radar deployment")]
    InvalidRadarDeploy(UnitId),
    /// A unit's order names an entity id this run never handed out.
    #[error("unit {0} is ordered against an id the run never minted")]
    UnmintedOrderTarget(UnitId),
    /// A building is owned by a player outside the table.
    #[error("building {0} is owned by a player outside the table")]
    ForeignBuildingOwner(BuildingId),
    /// A building's hit points sit outside `(0, max_hp]`.
    #[error("building {0} carries hit points its kind cannot hold")]
    BuildingHpOutOfRange(BuildingId),
    /// A construction or upgrade meter is past its rung's build time, or
    /// a training meter is past the ceiling.
    #[error("building {0} carries a progress meter past its ceiling")]
    BuildingProgressOutOfRange(BuildingId),
    /// A site sits above the base rung, or an upgrade climbs to it.
    #[error("building {0} is in a phase its tier cannot hold")]
    InvalidBuildingPhase(BuildingId),
    /// A building's cooldown is longer than its weapon's period.
    #[error("building {0} carries a cooldown its weapon never sets")]
    BuildingCooldownOutOfRange(BuildingId),
    /// A building focus names an id this run never minted.
    #[error("building {0} focuses an id the run never minted")]
    UnmintedBuildingFocus(BuildingId),
    /// A live building focus could not have passed the command gate.
    #[error("building {0} carries an invalid defense focus")]
    InvalidBuildingFocus(BuildingId),
    /// A building's production queue is longer than
    /// [`crate::stats::QUEUE_CAP`].
    #[error("building {0} queues more units than the cap allows")]
    OverlongBuildingQueue(BuildingId),
    /// A building queues a unit its kind cannot train, or anything at all
    /// before it is built.
    #[error("building {0} queues a unit it could never train")]
    UnproducibleQueueEntry(BuildingId),
    /// A building's anchor or rally point sits outside the sanity
    /// envelope.
    #[error("building {0} names a coordinate outside the envelope")]
    BuildingOutsideEnvelope(BuildingId),
    /// A building's salvage ledger is not a coherent record of the hp
    /// stripped from it.
    #[error("building {0} carries an incoherent salvage ledger")]
    IncoherentSalvageLedger(BuildingId),
    /// A tier index past the kind's upgrade ladder.
    #[error("building {0} claims a tier its kind's ladder does not reach")]
    TierBeyondLadder(BuildingId),
    /// Two buildings that mark the occupancy grid cover the same tile.
    #[error("buildings {0} and {1} overlap")]
    OverlappingBuildings(BuildingId, BuildingId),
    /// A transport's riders total more room than its sling offers.
    #[error("unit {0} carries more cargo than its sling holds")]
    CargoBeyondCapacity(UnitId),
    /// A rider of a kind no sling can take (a flyer, or a transport).
    #[error("unit {0} carries a rider that can never be carried")]
    UncarriableCargo(UnitId),
    /// A unit carries a part its kind does not have, or lacks one it does:
    /// harvest gear, a motor of the wrong movement class, a turret bearing
    /// without a ground turret, or cargo without a sling.
    #[error("unit {0} carries parts that do not match its kind")]
    UnitPartMismatch(UnitId),
    /// A rider outside the living hp range.
    #[error("unit {0} carries a rider with impossible hp")]
    CargoHpOutOfRange(UnitId),
    /// A rider whose weapon cooldowns exceed its own weapon table.
    #[error("unit {0} carries a rider with impossible weapon cooldowns")]
    CargoCooldownOutOfRange(UnitId),
    /// A point defender, walking or carried, holding more charges than its
    /// kind, a recharge counter at or past a full recharge, or a running
    /// counter while full.
    #[error("unit {0} holds impossible point-defense charges")]
    PointDefenseOutOfRange(UnitId),
    /// A landed airframe resting farther from its tile center than a
    /// touchdown allows.
    #[error("unit {0} is landed off its tile center")]
    LandedOffCenter(UnitId),
    /// A landed airframe carrying a path; takeoff clears the flag before
    /// any route is written.
    #[error("unit {0} is landed but holds a path")]
    LandedWithPath(UnitId),
    /// A landed airframe parked on a heading no turn can fly out of
    /// inside the world.
    #[error("unit {0} is landed on a heading it cannot fly out of")]
    LandedUnescapable(UnitId),
    /// A landed airframe resting on terrain no ground body can stand on.
    #[error("unit {0} is landed on ground it cannot stand on")]
    LandedOnUnstandableGround(UnitId),
    /// Two landed airframes resting inside their combined radius.
    #[error("units {0} and {1} are parked inside each other")]
    LandedOverlap(UnitId, UnitId),
    /// The same unit id appears twice across the world and every hold.
    #[error("a unit id is aliased between the world and a cargo hold")]
    AliasedCargoId,
    /// A shell in flight is owned by a player outside the table.
    #[error("shell {0} is owned by a player outside the table")]
    ForeignShellOwner(usize),
    /// A shell's launch, impact, or splash radius is outside the sanity
    /// envelope.
    #[error("shell {0} names a coordinate outside the envelope")]
    ShellOutsideEnvelope(usize),
    /// A shell was fired by an entity id this run never handed out.
    #[error("shell {0} was fired by an id the run never minted")]
    UnmintedShellShooter(usize),
    /// A shell lands before it launched, so its flight has no length.
    #[error("shell {0} lands before it launched")]
    ShellLaunchedAfterArrival(usize),
    /// A remembered building is owned by a player outside the table.
    #[error("player {0} remembers a building owned outside the table")]
    ForeignGhostOwner(PlayerId),
    /// A player remembers a building belonging to their own team; ghosts
    /// are memories of the enemy.
    #[error("player {0} remembers a building of their own team")]
    FriendlyGhost(PlayerId),
    /// A remembered building's anchor sits outside the sanity envelope.
    #[error("player {0} remembers a building outside the envelope")]
    GhostOutsideEnvelope(PlayerId),
    /// A remembered building claims a tier its kind's ladder does not reach.
    #[error("player {0} remembers a building at a tier its ladder does not reach")]
    GhostTierBeyondLadder(PlayerId),
    /// A remembered building's hit points fall outside `(0, max_hp]` for
    /// its remembered tier.
    #[error("player {0} remembers a building with impossible hit points")]
    GhostHpOutOfRange(PlayerId),
    /// A player's remembered buildings are not in canonical order.
    #[error("player {0} remembers buildings out of canonical order")]
    UnsortedGhosts(PlayerId),
    /// A radar contact sits outside the sanity envelope.
    #[error("player {0} holds a radar contact outside the envelope")]
    ContactOutsideEnvelope(PlayerId),
    /// A player's radar contacts are not sorted and deduplicated.
    #[error("player {0} holds radar contacts out of canonical order")]
    UnsortedContacts(PlayerId),
    /// A team carries more recent allied impact sites than the bounded
    /// memory permits.
    #[error("player {0} holds more salvage incidents than the cap allows")]
    OverlongSalvageIncidentMemory(PlayerId),
    /// A recent allied impact site sits outside the sanity envelope.
    #[error("player {0} remembers a salvage incident outside the envelope")]
    SalvageIncidentOutsideEnvelope(PlayerId),
    /// An active match retained an allied impact past its expiry.
    #[error("player {0} carries an expired salvage incident in an active match")]
    ExpiredSalvageIncident(PlayerId),
    /// A recent allied impact expiry sits beyond one legal cooldown from
    /// the state's current tick.
    #[error("player {0} carries a salvage incident expiry beyond its memory horizon")]
    SalvageIncidentExpiryBeyondHorizon(PlayerId),
    /// Recent allied impact sites are not sorted and deduplicated.
    #[error("player {0} holds salvage incidents out of canonical order")]
    UnsortedSalvageIncidents(PlayerId),
    /// A magazine on a kind without one, missing from a kind with one,
    /// past its capacity, with a meter out of range, or stocked before the
    /// building stands.
    #[error("building {0} carries an invalid magazine")]
    InvalidMagazine(BuildingId),
    /// A launch deck on a kind that launches nothing, missing from a silo,
    /// holding a strike off the map, before the silo stands, or with
    /// nothing loaded, or counting ready ticks with no strike or past the
    /// tick it fires on.
    #[error("building {0} carries an invalid launch deck")]
    InvalidLauncher(BuildingId),
    /// A gun bearing on a kind that fires nothing, or missing from one that
    /// fires.
    #[error("building {0} has a gun bearing that does not match its kind")]
    GunMismatch(BuildingId),
    /// Warheads are not strictly ordered by id.
    #[error("warheads are out of id order")]
    UnsortedWarheads,
    /// A warhead id at or past the next id the run would hand out.
    #[error("a warhead id is at or past the warhead counter")]
    StaleWarheadCounter,
    /// A warhead in flight is owned by a player outside the table.
    #[error("warhead {0} is owned by a player outside the table")]
    ForeignWarheadOwner(usize),
    /// A warhead launches or lands off the map, flies for a time its
    /// line does not take, names a silo id never minted, or outlives the
    /// match.
    #[error("warhead {0} has an impossible flight")]
    InvalidWarheadFlight(usize),
    /// A team's warhead holds are out of order, name a warhead not in
    /// flight or not hostile, carry an impossible exposure, or differ
    /// between teammates.
    #[error("player {0} holds impossible warhead tracks")]
    InvalidWarheadTracking(PlayerId),
    /// More workers hold a cradle than its crew admits.
    #[error("cradle {0} holds more crew than it admits")]
    CrewBeyondCap(BuildingId),
    /// A mounted gun aimed outside its arc, a bearing on a slot without a
    /// mount, or an idle facing held longer than the kind ever holds one.
    #[error("unit {0} holds impossible gun bearings")]
    MountsOutOfRange(UnitId),
    /// A drone bay holding more drones, docked, flying and ordered, than it
    /// admits, building with nothing ordered or past its build time, or on
    /// alert longer than any threat keeps it.
    #[error("unit {0} holds an impossible drone bay")]
    InvalidDroneBay(UnitId),
    /// A drone whose host is not its owner's live machine with a bay: cleanup
    /// removes a drone the tick its host leaves play.
    #[error("drone {0} has no host to fly for")]
    StrayDrone(UnitId),
}

/// Slot i belongs to weapon i; slots past the roster stay zero for the
/// machine's whole life.
fn cooldowns_out_of_range(
    cooldowns: [u32; crate::stats::MAX_WEAPONS],
    stats: &crate::stats::UnitStats,
) -> bool {
    cooldowns.iter().enumerate().any(|(i, cd)| {
        *cd > stats
            .weapons
            .get(i)
            .map_or(0, |weapon| weapon.cooldown_ticks)
    })
}

/// Whether held charges and the recharge counter are reachable for a kind
/// with point defense: recharge counts only below full and resets on each
/// restored charge. Presence against the kind is checked separately.
fn point_defense_in_range(
    point_defense: Option<PointDefense>,
    stats: &crate::stats::UnitStats,
) -> bool {
    let (Some(held), Some(stats)) = (point_defense, stats.point_defense) else {
        return true;
    };
    held.charges <= stats.charges
        && held.recharge < stats.recharge_ticks
        && (held.charges < stats.charges || held.recharge == 0)
}

/// Whether every gun bears inside its arc, slots without a mount stay at
/// zero, and an idle hold is no longer than the kind's. Presence against the
/// kind is checked separately.
fn mounts_in_range(mounts: Option<Mounts>, stats: &crate::stats::UnitStats) -> bool {
    let (Some(held), Some(guns)) = (mounts, stats.mounts) else {
        return true;
    };
    let bearings =
        held.bearings
            .iter()
            .enumerate()
            .all(|(slot, bearing)| match guns.mounts.get(slot) {
                Some(mount) => mount.covers(*bearing),
                None => *bearing == 0,
            });
    let hold = match held.facing {
        IdleFacing::Holding { ticks } => ticks.get() <= guns.facing_hold,
        IdleFacing::Free | IdleFacing::Turning => true,
    };
    bearings && hold
}

fn valid_air_motion(unit: &Unit) -> bool {
    let motion = unit.air_motion();
    let speed = unit.kind.stats().speed;
    if motion == Vec2Fx::ZERO {
        return true;
    }
    unit.kind.stats().crash.is_some()
        && unit.domain() == crate::stats::Domain::Air
        && motion.x >= -speed
        && motion.x <= speed
        && motion.y >= -speed
        && motion.y <= speed
        && motion.length_sq() <= speed * speed
}

/// A destroyed airframe coasting toward a fixed impact point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AircraftCrash {
    /// The removed aircraft's stable identity.
    pub unit: UnitId,
    /// The removed aircraft's owner, retained for damage attribution.
    pub player: PlayerId,
    /// Airframe and crash damage profile.
    pub kind: UnitKind,
    /// Last hull bearing; the airframe stays level during its fall.
    pub heading: u8,
    /// Ground position when the aircraft died.
    pub launch: Vec2Fx,
    /// Fixed ground contact point.
    pub impact: Vec2Fx,
    /// Tick on which the aircraft died.
    pub started: Tick,
    /// Tick on which impact damage resolves.
    pub arrival: Tick,
}

/// A shell in flight: launched toward a fixed fire-time aim point, unguided
/// from that instant, and resolved on its arrival tick against whatever
/// stands there. It may outlive its shooter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shell {
    /// Retained visual identity, including after the shooter dies.
    pub kind: ProjectileKind,
    /// Who fired it (may be dead by impact; retaliation copes).
    pub shooter: crate::ids::Target,
    /// The firing seat.
    pub player: crate::ids::PlayerId,
    /// Where it launched, for presentation.
    pub launch: Vec2Fx,
    /// Where it will land — fixed at fire time.
    pub impact: Vec2Fx,
    /// The tick it launched on.
    pub launched_at: Tick,
    /// The tick it resolves on.
    pub arrival: Tick,
    /// Damage on the direct hit.
    pub damage: u32,
    /// Which movement domains the splash covers.
    pub targets: crate::stats::DomainMask,
    /// Splash radius, if the weapon splashes.
    pub splash: Option<chassis::fx::Fx>,
}

impl Shell {
    /// Where the shell is over the ground on tick `now`: the straight track
    /// from launch to impact, standing at impact from its arrival tick on.
    /// Deserialized shells may carry any ticks with `launched_at <=
    /// arrival`, so the progress is computed in wide integers instead of
    /// trusting the span to fit a fixed-point value.
    pub fn flight_position(&self, now: Tick) -> Vec2Fx {
        flight_point(
            self.launch,
            self.impact,
            self.launched_at,
            self.arrival,
            now,
        )
    }
}

/// Where a straight flight from `launch` (at `launched_at`) to `impact`
/// (at `arrival`) stands on tick `now`, standing at impact from its arrival
/// on. Deserialized flights may carry any ticks with `launched_at <=
/// arrival`, so the progress is computed in wide integers instead of
/// trusting the span to fit a fixed-point value.
fn flight_point(
    launch: Vec2Fx,
    impact: Vec2Fx,
    launched_at: Tick,
    arrival: Tick,
    now: Tick,
) -> Vec2Fx {
    let span = arrival.saturating_sub(launched_at);
    let elapsed = now.saturating_sub(launched_at);
    if elapsed >= span {
        return impact;
    }
    let progress = (u128::from(elapsed) << 32) / u128::from(span);
    let progress = Fx::from_bits(i64::try_from(progress).expect("progress below one fits"));
    launch + (impact - launch) * progress
}

/// A strategic warhead flying from a silo to a fixed impact point,
/// unguided from launch. Point defense never sees it; only a battery
/// whose team has tracked it can stop it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Warhead {
    /// Stable id; `warheads` is sorted by it.
    pub id: crate::ids::WarheadId,
    /// The launching seat.
    pub player: PlayerId,
    /// The silo that fired it, which may be gone by now.
    pub silo: BuildingId,
    /// The silo's centre at launch.
    pub launch: Vec2Fx,
    /// Where it lands.
    pub impact: Vec2Fx,
    /// The tick it launched on.
    pub launched_at: Tick,
    /// The tick it lands on.
    pub arrival: Tick,
}

impl Warhead {
    /// Where the warhead is over the ground on tick `now`.
    pub fn position(&self, now: Tick) -> Vec2Fx {
        flight_point(
            self.launch,
            self.impact,
            self.launched_at,
            self.arrival,
            now,
        )
    }
}

/// The wire shape of [`State`]: a private mirror that derives the actual
/// field-level `Deserialize`, so the only path from bytes to a `State`
/// runs through `State::validate_invariants` — there is no public
/// unvalidated constructor to call by accident. The exhaustive `From`
/// below keeps the mirror honest: if `State` grows or loses a field, this
/// module stops compiling instead of silently desyncing.
#[derive(Deserialize)]
#[serde(rename = "State")]
struct StateWire {
    #[serde(default)]
    mode: crate::scenario::ScenarioMode,
    tick: Tick,
    map: Map,
    players: Vec<Player>,
    vision: Vec<crate::vision::Vision>,
    units: Vec<Unit>,
    buildings: Vec<Building>,
    shells: Vec<Shell>,
    #[serde(default)]
    aircraft_crashes: Vec<AircraftCrash>,
    #[serde(default)]
    warheads: Vec<Warhead>,
    result: Option<GameResult>,
    next_unit_id: u32,
    next_building_id: u32,
    #[serde(default)]
    next_warhead_id: u32,
}

impl From<StateWire> for State {
    fn from(w: StateWire) -> Self {
        // Every field named on both sides: drift breaks the build.
        let StateWire {
            mode,
            tick,
            map,
            players,
            vision,
            units,
            buildings,
            shells,
            aircraft_crashes,
            warheads,
            result,
            next_unit_id,
            next_building_id,
            next_warhead_id,
        } = w;
        State {
            mode,
            building_occupancy: Vec::new(),
            tick,
            map,
            players,
            vision,
            units,
            buildings,
            shells,
            aircraft_crashes,
            warheads,
            result,
            next_unit_id,
            next_building_id,
            next_warhead_id,
        }
    }
}

impl<'de> Deserialize<'de> for State {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut state: State = StateWire::deserialize(deserializer)?.into();
        state.rebuild_building_occupancy();
        state
            .validate_invariants()
            .map_err(serde::de::Error::custom)?;
        Ok(state)
    }
}
