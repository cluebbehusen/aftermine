//! A new player's game: a thin economy that grows slowly, one Fabricator, a
//! small mixed army, and one attack on the enemy Foundry once the army feels
//! big enough, after a long while. It looks at the game only every few
//! seconds and never expands.

use super::tactics::{campaign, gather, grown, home_post, train_mix};
use super::turn::{Prey, Turn};
use super::{DECISION_TICKS, Memory, between};
use aftermine_sim::{BuildingKind, UnitId, UnitKind};
use chassis::rng::Pcg32;

/// What a seed varies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Params {
    /// Harvesters it starts aiming for; one more each minute, up to 12.
    workers: u32,
    /// Extractors it restores at first; one more each two minutes, up to 4.
    extractors: u32,
    /// The tick its Fabricator may go up.
    fabricator_at: u64,
    /// Decisions it lets pass between looks at the game.
    looks: u64,
    /// The army that feels big enough.
    force: u32,
    /// The earliest attack.
    attack_at: u64,
    /// Sentinels per four units.
    sentinels: u32,
    /// Scuttlers per four units.
    scuttlers: u32,
    /// Lancers per four units.
    lancers: u32,
}

impl Params {
    pub(super) fn draw(rng: &mut Pcg32) -> Self {
        Self {
            workers: between(rng, 5, 7),
            extractors: between(rng, 0, 1),
            fabricator_at: u64::from(between(rng, 24, 48)) * 100,
            looks: u64::from(between(rng, 4, 10)),
            force: between(rng, 6, 10),
            attack_at: u64::from(between(rng, 72, 108)) * 100,
            sentinels: between(rng, 1, 3),
            scuttlers: between(rng, 0, 2),
            lancers: between(rng, 0, 1),
        }
    }
}

pub(super) fn decide(turn: &mut Turn<'_>, params: &Params, memory: &mut Memory) {
    if !(turn.tick / DECISION_TICKS).is_multiple_of(params.looks) {
        return;
    }
    turn.economy(grown(params.extractors, turn.tick, 2400, 4), 12);
    turn.train_workers(grown(params.workers, turn.tick, 1200, 12));
    let home = turn.home();
    let post = home_post(turn);
    if turn.tick >= params.fabricator_at {
        turn.ensure(BuildingKind::Fabricator, post, 6);
    }
    if let Some(fabricator) = turn.finished(BuildingKind::Fabricator).next() {
        let id = fabricator.id;
        turn.rally(id, post);
    }
    train_mix(
        turn,
        &[
            (UnitKind::Sentinel, params.sentinels),
            (UnitKind::Scuttler, params.scuttlers),
            (UnitKind::Lancer, params.lancers),
        ],
        home,
        false,
    );
    let army: Vec<UnitId> = turn.army().iter().map(|u| u.id).collect();
    if memory.attacking || turn.tick >= params.attack_at {
        campaign(
            turn,
            memory,
            &army,
            post,
            params.force,
            Prey::Foundry,
            false,
        );
    } else if !turn.defend(&army, 10) {
        gather(turn, &army, post, Prey::Foundry);
    }
}
