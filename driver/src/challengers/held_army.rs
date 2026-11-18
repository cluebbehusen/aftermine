//! A strong economy, a mixed army and armed defenses at home, nothing sent
//! out before a set time between 4:00 and 6:00; then everything goes at the
//! enemy Foundry and reinforcements keep following.

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
    /// Turrets at the home post.
    turrets: u32,
    /// The tick the army leaves.
    attack_at: u64,
    /// Lancers per four units.
    lancers: u32,
    /// Wardens per four units.
    wardens: u32,
    /// Sentinels per four units.
    sentinels: u32,
    /// What the army shoots first when nothing shoots back.
    prey: Prey,
}

impl Params {
    pub(super) fn draw(rng: &mut Pcg32) -> Self {
        Self {
            workers: between(rng, 9, 12),
            extractors: between(rng, 3, 5),
            turrets: between(rng, 1, 3),
            attack_at: u64::from(between(rng, 48, 72)) * 100,
            lancers: between(rng, 1, 3),
            wardens: between(rng, 0, 2),
            sentinels: between(rng, 1, 3),
            prey: pick(rng, &[Prey::Defenses, Prey::Foundry]),
        }
    }
}

pub(super) fn decide(turn: &mut Turn<'_>, params: &Params, memory: &mut Memory) {
    grow_economy(turn, params.workers, params.extractors, 2400);
    let home = turn.home();
    let post = home_post(turn);
    let guards = guard_home(turn, memory, UnitKind::Sentinel, 1, true);
    let army: Vec<UnitId> = turn
        .army()
        .iter()
        .map(|u| u.id)
        .filter(|id| !guards.contains(id))
        .collect();
    if turn.tick >= params.attack_at {
        campaign(turn, memory, &army, post, 1, params.prey, false);
    } else if !turn.defend(&army, 10) {
        gather(turn, &army, post, params.prey);
    }

    if turn.count(UnitKind::Harvester) >= 6 {
        turn.ensure(BuildingKind::Fabricator, post, 6);
    }
    let turrets = u32::try_from(turn.structures(BuildingKind::Turret).count()).unwrap_or(0);
    if turrets < params.turrets && turn.has(BuildingKind::Fabricator) {
        turn.build(BuildingKind::Turret, post, 5, None);
    }
    add_producer(turn, BuildingKind::Fabricator, 2);
    if let Some(fabricator) = turn.finished(BuildingKind::Fabricator).next() {
        let id = fabricator.id;
        turn.rally(id, post);
    }
    train_mix(
        turn,
        &[
            (UnitKind::Sentinel, params.sentinels),
            (UnitKind::Lancer, params.lancers),
            (UnitKind::Warden, params.wardens),
        ],
        home,
        false,
    );
    let sites = turn.map.enemy_harvest_sites(14);
    turn.scout_sites(&sites, 0);
}
