//! A ranged army, Lancers behind Wardens, held on a line across the middle
//! of the route: it fights what comes within reach of the line and chases no
//! further, so an enemy that sends its units a few at a time meets the whole
//! line each time. After a set time between 7:30 and 11:00 it marches on the
//! enemy Foundry once it is large enough, and a wave cut down falls back to
//! the line.

use super::tactics::{add_producer, campaign, gather, grow_economy, guard_home, train_mix};
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
    /// Where the line stands: route steps from the enemy Foundry, in percent
    /// of the route.
    line: i32,
    /// Which lane across the route the line takes.
    lane: u8,
    /// Tiles from the line inside which the army fights.
    reach: i32,
    /// Lancers per mix.
    lancers: u32,
    /// Wardens per mix.
    wardens: u32,
    /// Sentinels per mix.
    sentinels: u32,
    /// The army that marches.
    force: u32,
    /// The earliest march.
    march_at: u64,
}

impl Params {
    pub(super) fn draw(rng: &mut Pcg32) -> Self {
        Self {
            workers: between(rng, 8, 11),
            extractors: between(rng, 2, 4),
            line: i32::try_from(between(rng, 45, 60)).unwrap_or(50),
            lane: pick(rng, &[0, 1, 2]),
            reach: i32::try_from(between(rng, 6, 8)).unwrap_or(7),
            lancers: between(rng, 2, 3),
            wardens: between(rng, 1, 2),
            sentinels: between(rng, 0, 1),
            force: between(rng, 14, 20),
            march_at: u64::from(between(rng, 90, 132)) * 100,
        }
    }
}

pub(super) fn decide(turn: &mut Turn<'_>, params: &Params, memory: &mut Memory) {
    grow_economy(turn, params.workers, params.extractors, 3000);
    let home = turn.home();
    let route = turn.map.route_length();
    let line = turn.map.approach(route * params.line / 100, 4, params.lane);
    let guards = guard_home(
        turn,
        memory,
        UnitKind::Sentinel,
        1,
        turn.has(BuildingKind::Fabricator),
    );
    let army: Vec<UnitId> = turn
        .army()
        .iter()
        .map(|u| u.id)
        .filter(|id| !guards.contains(id))
        .collect();
    if turn.tick >= params.march_at || memory.attacking {
        campaign(
            turn,
            memory,
            &army,
            line,
            params.force,
            Prey::Defenses,
            true,
        );
    } else {
        let (ground, air) = turn.reach_of(&army);
        let threat = turn
            .threats_near(line, params.reach)
            .into_iter()
            .find(|u| match u.body_domain() {
                Domain::Ground => ground,
                Domain::Air => air,
            })
            .map(|u| u.tile);
        match threat {
            Some(tile) => {
                turn.hunt(&army, tile);
            }
            None => gather(turn, &army, line, Prey::Defenses),
        }
    }

    if turn.count(UnitKind::Harvester) >= 6 {
        turn.ensure(BuildingKind::Fabricator, home, 6);
    }
    add_producer(turn, BuildingKind::Fabricator, 2);
    let fabricators: Vec<_> = turn
        .finished(BuildingKind::Fabricator)
        .map(|b| b.id)
        .collect();
    for id in fabricators {
        turn.rally(id, line);
    }
    train_mix(
        turn,
        &[
            (UnitKind::Lancer, params.lancers),
            (UnitKind::Warden, params.wardens),
            (UnitKind::Sentinel, params.sentinels),
        ],
        home,
        false,
    );
    let sites = turn.map.enemy_harvest_sites(14);
    turn.scout_sites(&sites, 0);
}
