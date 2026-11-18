//! A small economy, then Scuttler groups sent again and again at the
//! enemy's harvest sites to kill its Harvesters, while the economy grows;
//! a main attack once the seat has the army for it.

use super::tactics::{
    add_producer, campaign, gather, grow_economy, guard_home, home_post, raid, train_mix,
};
use super::turn::{Prey, Turn};
use super::{Memory, between, pick};
use aftermine_sim::{BuildingKind, UnitId, UnitKind};
use chassis::rng::Pcg32;

/// What a seed varies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Params {
    /// Harvesters to keep.
    workers: u32,
    /// Extractors to restore.
    extractors: u32,
    /// Scuttlers per raid.
    group: u32,
    /// The tick the first raid may leave.
    first_raid: u64,
    /// Which raid site the first raid picks.
    site: u32,
    /// Sentinels kept home.
    guards: u32,
    /// The main army that attacks.
    force: u32,
    /// The earliest main attack.
    main_at: u64,
    /// Sentinels per four Scuttlers in the main army.
    sentinels: u32,
    /// Whether the main army adds Lancers from a Fabricator.
    lancers: bool,
}

impl Params {
    pub(super) fn draw(rng: &mut Pcg32) -> Self {
        Self {
            workers: between(rng, 6, 8),
            extractors: between(rng, 2, 3),
            group: between(rng, 2, 4),
            first_raid: u64::from(between(rng, 6, 15)) * 100,
            site: between(rng, 0, 3),
            guards: between(rng, 1, 2),
            force: between(rng, 8, 12),
            main_at: u64::from(between(rng, 42, 66)) * 100,
            sentinels: between(rng, 2, 6),
            lancers: pick(rng, &[false, true]),
        }
    }
}

pub(super) fn decide(turn: &mut Turn<'_>, params: &Params, memory: &mut Memory) {
    grow_economy(turn, params.workers, params.extractors, 3600);
    let home = turn.home();
    let guards = guard_home(
        turn,
        memory,
        UnitKind::Sentinel,
        params.guards,
        turn.tick > 600,
    );

    // A new raid forms from the Scuttlers waiting at home.
    if memory.raid.is_empty() && turn.tick >= params.first_raid {
        let waiting: Vec<UnitId> = turn
            .units(UnitKind::Scuttler)
            .filter(|u| !guards.contains(&u.id) && u.tile.chebyshev(home) <= 14)
            .map(|u| u.id)
            .take(usize::try_from(params.group).unwrap_or(2))
            .collect();
        if u32::try_from(waiting.len()).unwrap_or(0) >= params.group {
            memory.raid = waiting;
        }
    }
    raid(turn, memory, params.site, Prey::Workers);

    let army: Vec<UnitId> = turn
        .army()
        .iter()
        .map(|u| u.id)
        .filter(|id| !guards.contains(id) && !memory.raid.contains(id))
        .collect();
    let post = home_post(turn);
    if turn.tick >= params.main_at || memory.attacking {
        campaign(turn, memory, &army, post, params.force, Prey::Workers, true);
    } else if !turn.defend(&army, 9) {
        gather(turn, &army, post, Prey::Workers);
    }

    // Raiders first, then workers, then the main army.
    let scuttlers = turn.count(UnitKind::Scuttler);
    let raiding = u32::try_from(memory.raid.len()).unwrap_or(0);
    if scuttlers < raiding + params.group {
        turn.produce(UnitKind::Scuttler, home, 1);
    }
    add_producer(turn, BuildingKind::Fabricator, 2);
    if params.lancers && turn.tick > 2400 {
        turn.ensure(BuildingKind::Fabricator, post, 6);
    }
    let lancers = if params.lancers { 2 } else { 0 };
    train_mix(
        turn,
        &[
            (UnitKind::Scuttler, 4),
            (UnitKind::Sentinel, params.sentinels),
            (UnitKind::Lancer, lancers),
        ],
        home,
        false,
    );
    let sites = turn.map.enemy_harvest_sites(14);
    turn.scout_sites(&sites, usize::try_from(params.site).unwrap_or(0));
}
