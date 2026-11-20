//! An aircraft opening: a Fabricator and an Airworks, then Buzzards in
//! groups of two to four raiding harvest lines and Extractors away from
//! enemy flak, with ground units only to hold home; once the flock is big
//! enough, it goes for the Foundry.

use super::tactics::{
    add_producer, campaign, enemy_flies, gather, grow_economy, guard_home, home_post, raid,
    train_mix,
};
use super::turn::{Prey, Turn};
use super::{Memory, between, pick};
use aftermine_sim::stats::Domain;
use aftermine_sim::{BuildingKind, UnitId, UnitKind};
use chassis::rng::Pcg32;

/// What a seed varies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Params {
    /// Harvesters to keep.
    workers: u32,
    /// Extractors to restore.
    extractors: u32,
    /// Buzzards per raid.
    group: u32,
    /// Which raid site the first raid picks.
    site: u32,
    /// What raids shoot first.
    prey: Prey,
    /// Sentinels kept home.
    guards: u32,
    /// Turrets at the home post.
    turrets: u32,
    /// The Buzzards that go for the Foundry.
    force: u32,
    /// Talons per three Buzzards once the enemy flies.
    talons: u32,
}

impl Params {
    pub(super) fn draw(rng: &mut Pcg32) -> Self {
        Self {
            workers: between(rng, 6, 8),
            extractors: between(rng, 2, 3),
            group: between(rng, 2, 4),
            site: between(rng, 0, 3),
            prey: pick(rng, &[Prey::Workers, Prey::Extractors]),
            guards: between(rng, 2, 3),
            turrets: between(rng, 0, 1),
            force: between(rng, 6, 9),
            talons: between(rng, 1, 2),
        }
    }
}

pub(super) fn decide(turn: &mut Turn<'_>, params: &Params, memory: &mut Memory) {
    grow_economy(turn, params.workers, params.extractors, 3600);
    let home = turn.home();
    let post = home_post(turn);
    if turn.ensure(BuildingKind::Fabricator, home, 6) {
        turn.ensure(BuildingKind::Airworks, home, 7);
    }
    let guards = guard_home(
        turn,
        memory,
        UnitKind::Sentinel,
        params.guards,
        turn.structures(BuildingKind::Airworks).next().is_some(),
    );
    let turrets = u32::try_from(turn.structures(BuildingKind::Turret).count()).unwrap_or(0);
    if turrets < params.turrets && turn.has(BuildingKind::Airworks) {
        turn.build(BuildingKind::Turret, post, 5, None);
    }

    if memory.raid.is_empty() && !memory.attacking {
        let waiting: Vec<UnitId> = turn
            .units(UnitKind::Buzzard)
            .filter(|u| u.tile.chebyshev(home) <= 14)
            .map(|u| u.id)
            .take(usize::try_from(params.group).unwrap_or(2))
            .collect();
        if u32::try_from(waiting.len()).unwrap_or(0) >= params.group {
            memory.raid = waiting;
        }
    }
    raid(turn, memory, params.site, params.prey);

    let flock: Vec<UnitId> = turn
        .units(UnitKind::Buzzard)
        .chain(turn.units(UnitKind::Talon))
        .map(|u| u.id)
        .filter(|id| !memory.raid.contains(id))
        .collect();
    campaign(
        turn,
        memory,
        &flock,
        post,
        params.force,
        Prey::Foundry,
        true,
    );
    let ground: Vec<UnitId> = turn
        .army()
        .iter()
        .filter(|u| u.kind.stats().domain == Domain::Ground && !guards.contains(&u.id))
        .map(|u| u.id)
        .collect();
    if !turn.defend(&ground, 9) {
        gather(turn, &ground, post, Prey::Workers);
    }

    if let Some(airworks) = turn.finished(BuildingKind::Airworks).next() {
        let id = airworks.id;
        turn.rally(id, post);
        let talons = if enemy_flies(turn) { params.talons } else { 0 };
        train_mix(
            turn,
            &[(UnitKind::Buzzard, 3), (UnitKind::Talon, talons)],
            home,
            true,
        );
    }
    add_producer(turn, BuildingKind::Airworks, 3);
    let sites = turn.map.enemy_harvest_sites(14);
    turn.scout_sites(&sites, usize::try_from(params.site).unwrap_or(0));
}
