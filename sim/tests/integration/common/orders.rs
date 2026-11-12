//! Command constructors, so a suite states the order it gives. Each takes
//! the variant's fields in declaration order and leaves `queue` and `defer`
//! off; turn them on with [`CommandExt`].

use aftermine_sim::{AttackTarget, BuildingId, BuildingKind, Command, Rally, UnitId, UnitKind};
use chassis::grid::TilePos;

pub fn run(units: impl Into<Vec<UnitId>>, goal: TilePos) -> Command {
    Command::Run {
        units: units.into(),
        goal,
        queue: false,
    }
}

pub fn attack(units: impl Into<Vec<UnitId>>, target: impl Into<AttackTarget>) -> Command {
    Command::Attack {
        units: units.into(),
        target: target.into(),
        queue: false,
    }
}

pub fn hunt(units: impl Into<Vec<UnitId>>, goal: TilePos) -> Command {
    Command::Hunt {
        units: units.into(),
        goal,
        queue: false,
    }
}

pub fn advance(units: impl Into<Vec<UnitId>>, goal: TilePos) -> Command {
    Command::Advance {
        units: units.into(),
        goal,
        queue: false,
    }
}

pub fn scout(units: impl Into<Vec<UnitId>>, goal: TilePos) -> Command {
    Command::Scout {
        units: units.into(),
        goal,
        queue: false,
    }
}

pub fn harvest(units: impl Into<Vec<UnitId>>, node: TilePos) -> Command {
    Command::Harvest {
        units: units.into(),
        node,
        queue: false,
    }
}

pub fn patrol(units: impl Into<Vec<UnitId>>, waypoints: impl Into<Vec<TilePos>>) -> Command {
    Command::Patrol {
        units: units.into(),
        waypoints: waypoints.into(),
    }
}

pub fn stop(units: impl Into<Vec<UnitId>>) -> Command {
    Command::Stop {
        units: units.into(),
    }
}

pub fn train(building: BuildingId, kind: UnitKind) -> Command {
    Command::Train { building, kind }
}

pub fn build(units: impl Into<Vec<UnitId>>, kind: BuildingKind, anchor: TilePos) -> Command {
    Command::Build {
        units: units.into(),
        kind,
        anchor,
        queue: false,
        defer: false,
    }
}

pub fn repair(units: impl Into<Vec<UnitId>>, building: BuildingId) -> Command {
    Command::Repair {
        units: units.into(),
        building,
        queue: false,
    }
}

pub fn salvage(units: impl Into<Vec<UnitId>>, building: BuildingId) -> Command {
    Command::Salvage {
        units: units.into(),
        building,
        queue: false,
    }
}

pub fn set_rally(building: BuildingId, rally: Option<Rally>) -> Command {
    Command::SetRally { building, rally }
}

pub fn repair_unit(units: impl Into<Vec<UnitId>>, target: UnitId) -> Command {
    Command::RepairUnit {
        units: units.into(),
        target,
        queue: false,
    }
}

pub fn focus_fire(
    buildings: impl Into<Vec<BuildingId>>,
    target: impl Into<AttackTarget>,
) -> Command {
    Command::FocusFire {
        buildings: buildings.into(),
        target: target.into(),
    }
}

pub fn load(units: impl Into<Vec<UnitId>>, transport: UnitId) -> Command {
    Command::Load {
        units: units.into(),
        transport,
        queue: false,
    }
}

pub fn unload(transport: UnitId, at: TilePos) -> Command {
    Command::Unload {
        transport,
        at,
        queue: false,
    }
}

pub fn accompany(units: impl Into<Vec<UnitId>>, target: UnitId) -> Command {
    Command::Accompany {
        units: units.into(),
        target,
        queue: false,
    }
}

/// The flags a constructor leaves off.
pub trait CommandExt {
    /// Appends behind the units' current orders instead of replacing them.
    fn queued(self) -> Command;
    /// Pays for a provisional site on the issuer's knowledge alone.
    fn deferred(self) -> Command;
}

impl CommandExt for Command {
    #[track_caller]
    fn queued(mut self) -> Command {
        match &mut self {
            Command::Run { queue, .. }
            | Command::Attack { queue, .. }
            | Command::Hunt { queue, .. }
            | Command::Harvest { queue, .. }
            | Command::Build { queue, .. }
            | Command::Repair { queue, .. }
            | Command::Salvage { queue, .. }
            | Command::RepairUnit { queue, .. }
            | Command::Advance { queue, .. }
            | Command::Load { queue, .. }
            | Command::Unload { queue, .. }
            | Command::Accompany { queue, .. }
            | Command::Retire { queue, .. }
            | Command::Scout { queue, .. } => *queue = true,
            other => panic!("{other:?} has no queue"),
        }
        self
    }

    #[track_caller]
    fn deferred(mut self) -> Command {
        match &mut self {
            Command::Build { defer, .. } => *defer = true,
            other => panic!("{other:?} cannot defer"),
        }
        self
    }
}
