//! A Fabricator at home, Lancers in pairs or threes raiding the enemy's
//! harvest lines and Extractors from beyond its turrets' reach, then a main
//! push.

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
    /// The tick the Fabricator may go up.
    fabricator_at: u64,
    /// Lancers per raid.
    group: u32,
    /// Which raid site the first raid picks.
    site: u32,
    /// What raids shoot first.
    prey: Prey,
    /// Sentinels kept home.
    guards: u32,
    /// The army that pushes.
    force: u32,
    /// The earliest push.
    main_at: u64,
    /// Sentinels per two Lancers in the main army.
    sentinels: u32,
}

impl Params {
    pub(super) fn draw(rng: &mut Pcg32) -> Self {
        Self {
            workers: between(rng, 6, 8),
            extractors: between(rng, 2, 3),
            fabricator_at: u64::from(between(rng, 0, 8)) * 100,
            group: between(rng, 2, 3),
            site: between(rng, 0, 3),
            prey: pick(rng, &[Prey::Workers, Prey::Extractors]),
            guards: between(rng, 1, 2),
            force: between(rng, 6, 9),
            main_at: u64::from(between(rng, 48, 72)) * 100,
            sentinels: between(rng, 0, 2),
        }
    }
}

pub(super) fn decide(turn: &mut Turn<'_>, params: &Params, memory: &mut Memory) {
    grow_economy(turn, params.workers, params.extractors, 3600);
    let home = turn.home();
    let post = home_post(turn);
    if turn.tick >= params.fabricator_at {
        turn.ensure(BuildingKind::Fabricator, post, 6);
    }
    let guards = guard_home(
        turn,
        memory,
        UnitKind::Sentinel,
        params.guards,
        turn.has(BuildingKind::Fabricator),
    );

    if memory.raid.is_empty() && !memory.attacking {
        let waiting: Vec<UnitId> = turn
            .units(UnitKind::Lancer)
            .filter(|u| u.tile.chebyshev(home) <= 14)
            .map(|u| u.id)
            .take(usize::try_from(params.group).unwrap_or(2))
            .collect();
        if u32::try_from(waiting.len()).unwrap_or(0) >= params.group {
            memory.raid = waiting;
        }
    }
    raid(turn, memory, params.site, params.prey);

    let army: Vec<UnitId> = turn
        .army()
        .iter()
        .map(|u| u.id)
        .filter(|id| !guards.contains(id) && !memory.raid.contains(id))
        .collect();
    if turn.tick >= params.main_at || memory.attacking {
        campaign(
            turn,
            memory,
            &army,
            post,
            params.force,
            Prey::Defenses,
            true,
        );
    } else if !turn.defend(&army, 9) {
        gather(turn, &army, post, Prey::Defenses);
    }

    if let Some(fabricator) = turn.finished(BuildingKind::Fabricator).next() {
        let (id, anchor) = (fabricator.id, fabricator.anchor);
        turn.rally(id, post);
        turn.produce(UnitKind::Lancer, anchor, 1);
    }
    add_producer(turn, BuildingKind::Fabricator, 3);
    train_mix(
        turn,
        &[
            (UnitKind::Lancer, 2),
            (UnitKind::Sentinel, params.sentinels),
        ],
        home,
        false,
    );
    let sites = turn.map.enemy_harvest_sites(14);
    turn.scout_sites(&sites, usize::try_from(params.site).unwrap_or(0));
}
