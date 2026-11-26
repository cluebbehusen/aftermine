//! Opt-in decision diagnostics. Traces are output only: never controller
//! memory, simulation state or replay input.

use crate::OwnEvent;
use crate::composition::{Factors, Reach, Role};
use crate::investments::{Investment, Step};
use crate::missions::MissionStatus;
use aftermine_sim::{BuildingId, BuildingKind, PlayerId, UnitKind};
use chassis::grid::TilePos;
use serde::Serialize;

/// What one decision saw and did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Trace {
    /// Simulation tick the decision observed.
    pub tick: u64,
    /// Seat that decided.
    pub player: PlayerId,
    /// Scrap on hand when the decision started.
    pub bank: u32,
    /// The seat's own order failures since its previous decision, oldest first.
    pub events: Vec<OwnEvent>,
    /// Scrap the decision's purchases committed.
    pub spent: u32,
    /// Purchases, in command order.
    pub purchases: Vec<Purchase>,
    /// Orders issued to units, not counting purchases.
    pub unit_orders: u32,
    /// Unit orders the decision could have issued.
    pub allowance: u32,
    /// The investment the seat was saving for.
    pub target: Option<SavingTarget>,
    /// Scrap held back from ordinary spending after the decision.
    pub protected: u32,
    /// Missions after the decision, by id.
    pub missions: Vec<MissionStatus>,
    /// Why the decision trained the army units it did.
    pub army: ArmyReasons,
}

/// What army production weighed in one decision.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ArmyReasons {
    /// What the seat knows of the enemy army.
    pub enemy: EnemyArmy,
    /// The seat's estimated income, which sets each unit's affordability.
    pub income: u32,
    /// Each role's deficit and weight as production for wanted roles began.
    /// Empty when the decision held production for carriers instead.
    pub roles: Vec<RoleNeed>,
    /// Units queued for the army or the saving target, in order. Workers,
    /// scouts, raiders and carriers trained for needs of their own appear
    /// only among the purchases.
    pub picks: Vec<Pick>,
}

/// The enemy army as one seat knows it, in scrap weighted by confidence,
/// plus what known enemy production foretells.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct EnemyArmy {
    /// Aircraft of every kind, armed or not, plus 200 for each known Airworks
    /// and the full value of an aircraft a known cradle will stand up.
    pub air: u64,
    /// The part of `air` that carries a weapon, counting an armed aircraft a
    /// known cradle will stand up but never a known Airworks.
    pub armed_air: u64,
    /// The part of `air` armed only against the ground.
    pub grounders: u64,
    /// Ground units that can fight, plus the full value of a ground machine
    /// a known cradle will stand up.
    pub ground: u64,
    /// The part of `ground` that is heavy machines.
    pub heavy: u64,
    /// Whether the most recently seen enemy units include a clump.
    pub clustered: bool,
    /// The reach most of the enemy army's value fights at.
    pub reach: Option<Reach>,
}

/// One role's deficit and the personality's weight for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RoleNeed {
    /// The role.
    pub role: Role,
    /// Scrap the role lacks; zero or less wants nothing.
    pub need: i64,
    /// Per mille the personality weighs the role by.
    pub weight: u64,
}

/// One army unit queued and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pick {
    /// The unit queued.
    pub unit: UnitKind,
    /// The producer that queued it.
    pub building: BuildingId,
    /// The rule that chose it.
    pub path: PickPath,
    /// The units it was weighed against, best first: for a wanted role, every
    /// unit for the role the seat's producers train; for a fallback, those
    /// its own producer trains. Empty when no comparison chose it.
    pub candidates: Vec<Candidate>,
}

/// The rule that chose an army unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "path", rename_all = "snake_case")]
pub enum PickPath {
    /// The most wanted role's best unit a ready producer could afford, at no
    /// less than `bar`, the score a unit must reach rather than wait for a
    /// better one.
    Wanted {
        /// The role it filled.
        role: Role,
        /// The score it had to reach.
        bar: u64,
    },
    /// The best unit for a role a short defense or guard raised, the role
    /// its best answer belongs to, at no less than `bar`.
    Answer {
        /// The role it filled.
        role: Role,
        /// The score it had to reach.
        bar: u64,
    },
    /// A producer with no wanted role kept training ground units.
    Fallback {
        /// The ground role it trained for.
        role: Role,
    },
    /// A short defense or guard took what best beats its attackers by the
    /// counter chart, or the cheapest unit against builders alone.
    Shortfall,
    /// The unit the seat was saving for.
    Saved,
}

/// One unit a role could take and its score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Candidate {
    /// The unit.
    pub unit: UnitKind,
    /// The product of its factors.
    pub score: u64,
    /// The factors.
    pub factors: Factors,
}

/// One purchase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "purchase", rename_all = "snake_case")]
pub enum Purchase {
    /// A unit queued at a producer.
    Train {
        /// The producing building.
        building: BuildingId,
        /// The unit queued there.
        unit: UnitKind,
    },
    /// A building placed.
    Build {
        /// What was placed.
        building: BuildingKind,
        /// Its footprint anchor.
        anchor: TilePos,
    },
    /// A building upgraded.
    Upgrade {
        /// The upgraded building.
        building: BuildingId,
    },
    /// A round ordered for a magazine building.
    Round {
        /// The Spire or Lattice.
        building: BuildingId,
    },
}

/// A saving target as one decision saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SavingTarget {
    /// What the seat was saving for.
    pub investment: Investment,
    /// The next purchase toward it, unless a prerequisite was still being
    /// built.
    pub next: Option<NextPurchase>,
}

/// The next purchase toward a saving target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct NextPurchase {
    /// The purchase.
    pub step: Step,
    /// Its price.
    pub price: u32,
}
