//! Lancers and a few Sentinels gathered at home, pushed at the enemy
//! Foundry at about five minutes with a Kestrel flying over the lead Lancer
//! to spot for them.

use super::tactics::{
    add_producer, campaign, gather, grow_economy, guard_home, home_post, train_mix,
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
    /// The tick the push leaves, from 4:30 to 5:30.
    attack_at: u64,
    /// Sentinels per four units.
    sentinels: u32,
    /// Kestrels to spot.
    kestrels: u32,
    /// What the push shoots first when nothing shoots back.
    prey: Prey,
}

impl Params {
    pub(super) fn draw(rng: &mut Pcg32) -> Self {
        Self {
            workers: between(rng, 7, 9),
            extractors: between(rng, 2, 3),
            attack_at: u64::from(between(rng, 54, 66)) * 100,
            sentinels: between(rng, 0, 2),
            kestrels: between(rng, 1, 2),
            prey: pick(rng, &[Prey::Defenses, Prey::Foundry]),
        }
    }
}

pub(super) fn decide(turn: &mut Turn<'_>, params: &Params, memory: &mut Memory) {
    grow_economy(turn, params.workers, params.extractors, 3600);
    let home = turn.home();
    let post = home_post(turn);
    if turn.ensure(BuildingKind::Fabricator, post, 6) && turn.has(BuildingKind::Fabricator) {
        turn.ensure(BuildingKind::Airworks, home, 7);
    }
    let guards = guard_home(turn, memory, UnitKind::Sentinel, 1, true);
    let army: Vec<UnitId> = turn
        .army()
        .iter()
        .map(|u| u.id)
        .filter(|id| !guards.contains(id))
        .collect();
    if turn.tick >= params.attack_at {
        campaign(turn, memory, &army, post, 1, params.prey, false);
    } else if !turn.defend(&army, 9) {
        gather(turn, &army, post, params.prey);
    }
    let target = super::tactics::objective(turn);
    let lead = turn
        .obs
        .my_units
        .iter()
        .filter(|u| u.kind == UnitKind::Lancer)
        .min_by_key(|u| (u.tile.chebyshev(target), u.id))
        .map(|u| u.id);
    let spotters: Vec<UnitId> = turn.units(UnitKind::Kestrel).map(|u| u.id).collect();
    match lead {
        Some(lead) => {
            turn.accompany(&spotters, lead);
        }
        None => gather(turn, &spotters, post, params.prey),
    }

    if let Some(fabricator) = turn.finished(BuildingKind::Fabricator).next() {
        let id = fabricator.id;
        turn.rally(id, post);
    }
    if let Some(airworks) = turn.finished(BuildingKind::Airworks).next()
        && turn.count(UnitKind::Kestrel) < params.kestrels
    {
        let anchor = airworks.anchor;
        turn.train(UnitKind::Kestrel, anchor, 1);
    }
    add_producer(turn, BuildingKind::Fabricator, 2);
    train_mix(
        turn,
        &[
            (UnitKind::Lancer, 4 - params.sentinels),
            (UnitKind::Sentinel, params.sentinels),
        ],
        home,
        false,
    );
    let sites = turn.map.enemy_harvest_sites(14);
    turn.scout_sites(&sites, 0);
}
