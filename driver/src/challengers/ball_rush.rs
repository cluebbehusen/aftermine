//! A ball of Sentinels and Scuttlers gathered at home and sent at the
//! enemy Foundry at about three minutes, with reinforcements following.

use super::tactics::{campaign, gather, grow_economy, home_post, train_mix};
use super::turn::{Prey, Turn};
use super::{Memory, between, pick};
use aftermine_sim::{UnitId, UnitKind};
use chassis::rng::Pcg32;

/// What a seed varies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Params {
    /// Harvesters to keep.
    workers: u32,
    /// Extractors to restore.
    extractors: u32,
    /// The tick the ball leaves, from 2:30 to 3:30.
    attack_at: u64,
    /// Sentinels per three units.
    sentinels: u32,
    /// What the ball shoots first when nothing shoots back.
    prey: Prey,
}

impl Params {
    pub(super) fn draw(rng: &mut Pcg32) -> Self {
        Self {
            workers: between(rng, 5, 7),
            extractors: between(rng, 1, 2),
            attack_at: u64::from(between(rng, 30, 42)) * 100,
            sentinels: between(rng, 1, 2),
            prey: pick(rng, &[Prey::Workers, Prey::Foundry]),
        }
    }
}

pub(super) fn decide(turn: &mut Turn<'_>, params: &Params, memory: &mut Memory) {
    grow_economy(
        turn,
        params.workers,
        params.extractors,
        params.attack_at + 2400,
    );
    let home = turn.home();
    let post = home_post(turn);
    let army: Vec<UnitId> = turn.army().iter().map(|u| u.id).collect();
    if turn.tick >= params.attack_at {
        campaign(turn, memory, &army, post, 1, params.prey, false);
    } else if !turn.defend(&army, 9) {
        gather(turn, &army, post, params.prey);
    }
    if turn.count(UnitKind::Harvester) >= 4 {
        train_mix(
            turn,
            &[
                (UnitKind::Sentinel, params.sentinels),
                (UnitKind::Scuttler, 3 - params.sentinels),
            ],
            home,
            false,
        );
    }
    let sites = turn.map.enemy_harvest_sites(14);
    turn.scout_sites(&sites, 0);
}
