//! A Harvester walks toward the enemy Foundry and builds a Fabricator on the
//! approach; Lancers train there, a few Scuttlers or Sentinels join from
//! home, and the force attacks what is near once it is big enough.

use super::tactics::{add_producer, campaign, grow_economy, guard_home, home_post};
use super::turn::{Build, Prey, Turn};
use super::{Memory, between, pick};
use aftermine_sim::{BuildingKind, UnitId, UnitKind};
use chassis::rng::Pcg32;

/// What a seed varies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Params {
    /// Ground steps from the enemy Foundry to the Fabricator.
    distance: i32,
    /// Which side of the approach it stands on.
    lane: u8,
    /// The tick the builder leaves home.
    leave_at: u64,
    /// Harvesters to keep.
    workers: u32,
    /// Extractors to restore at home.
    extractors: u32,
    /// Sentinels kept home.
    guards: u32,
    /// What home trains to escort the Lancers.
    escort: UnitKind,
    /// How many escorts it trains.
    escorts: u32,
    /// Turrets the builder puts up beside the Fabricator.
    turrets: u32,
    /// The force, Lancers and escorts, that starts the attack.
    force: u32,
    /// What the force shoots first when nothing shoots back.
    prey: Prey,
}

impl Params {
    pub(super) fn draw(rng: &mut Pcg32) -> Self {
        Self {
            distance: i32::try_from(between(rng, 6, 12)).unwrap_or(9),
            lane: pick(rng, &[0, 1, 2]),
            leave_at: u64::from(between(rng, 0, 30)) * 10,
            workers: between(rng, 5, 7),
            extractors: between(rng, 2, 3),
            guards: between(rng, 1, 2),
            escort: pick(rng, &[UnitKind::Scuttler, UnitKind::Sentinel]),
            escorts: between(rng, 1, 3),
            turrets: between(rng, 0, 1),
            force: between(rng, 3, 5),
            prey: pick(rng, &[Prey::Workers, Prey::Defenses, Prey::Foundry]),
        }
    }
}

pub(super) fn decide(turn: &mut Turn<'_>, params: &Params, memory: &mut Memory) {
    let fabricator = BuildingKind::Fabricator;
    let back = i32::try_from(memory.setbacks.min(3)).unwrap_or(3) * 4;
    let spot = turn.map.approach(params.distance + back, 6, params.lane);
    let forward = turn
        .structures(fabricator)
        .min_by_key(|b| (b.anchor.chebyshev(spot), b.id))
        .map(|b| (b.id, b.built, b.anchor));
    let standing = forward.is_some_and(|(_, built, _)| built);
    let post = match forward {
        Some((_, true, anchor)) => anchor.offset(1, 3),
        _ => home_post(turn),
    };
    if let Some(builder) = memory.builder {
        turn.keep(builder);
    }

    let late = memory.waves > 0 || turn.tick > 4800;
    if late {
        grow_economy(turn, params.workers, params.extractors, 0);
    } else {
        turn.economy(params.extractors, 12);
    }
    match forward {
        None => send_builder(turn, params, memory, spot),
        Some((id, true, anchor)) => {
            turn.rally(id, post);
            turn.produce(UnitKind::Lancer, anchor, 1);
            let turrets = u32::try_from(turn.structures(BuildingKind::Turret).count()).unwrap_or(0);
            if turrets < params.turrets
                && let Some(builder) = memory.builder
                && turn
                    .units(UnitKind::Harvester)
                    .any(|u| u.id == builder && u.idle)
            {
                turn.build(BuildingKind::Turret, anchor.offset(0, 3), 4, Some(builder));
            }
        }
        Some((_, false, anchor)) => {
            if let Some(builder) = memory.builder
                && turn
                    .units(UnitKind::Harvester)
                    .any(|u| u.id == builder && u.idle)
            {
                turn.resume(fabricator, anchor, &[builder]);
            }
        }
    }
    let guards = guard_home(
        turn,
        memory,
        UnitKind::Sentinel,
        params.guards,
        forward.is_some(),
    );
    let army: Vec<UnitId> = turn
        .army()
        .iter()
        .map(|u| u.id)
        .filter(|id| !guards.contains(id))
        .collect();

    campaign(turn, memory, &army, post, params.force, params.prey, true);

    let home = turn.home();
    if forward.is_some()
        && (turn.count(params.escort) < params.escorts || (standing && turn.spendable() >= 300))
    {
        turn.train(params.escort, home, 1);
    }
    if late {
        add_producer(turn, BuildingKind::Fabricator, 2);
    } else {
        turn.train_workers(params.workers);
    }
    let sites = turn.map.enemy_harvest_sites(14);
    turn.scout_sites(&sites, usize::from(params.lane));
}

/// Walks the builder to the forward spot and places the Fabricator once it
/// sees the ground, keeping its price aside on the last stretch.
fn send_builder(
    turn: &mut Turn<'_>,
    params: &Params,
    memory: &mut Memory,
    spot: chassis::grid::TilePos,
) {
    let fabricator = BuildingKind::Fabricator;
    if memory
        .builder
        .is_some_and(|id| turn.units(UnitKind::Harvester).all(|u| u.id != id))
    {
        memory.builder = None;
        memory.setbacks += 1;
    }
    if turn.tick < params.leave_at {
        return;
    }
    let builder = memory.builder.or_else(|| turn.free_worker(spot));
    memory.builder = builder;
    let Some(builder) = builder else { return };
    let Some(at) = turn
        .units(UnitKind::Harvester)
        .find(|u| u.id == builder)
        .map(|u| u.tile)
    else {
        return;
    };
    let cost = fabricator.base_stats().construction.cost;
    if at.chebyshev(spot) <= 4 {
        match turn.build(fabricator, spot, 5, Some(builder)) {
            Build::Placed => {}
            Build::Short(cost) => turn.reserve(cost),
            Build::NoSite => {
                turn.run(&[builder], spot);
            }
        }
    } else {
        turn.run(&[builder], spot);
        if at.chebyshev(spot) <= 16 {
            turn.reserve(cost);
        }
    }
}
