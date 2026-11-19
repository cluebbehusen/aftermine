//! Artillery: Bombards with a Kestrel spotter and an escort, advancing to
//! shell defenses and Foundries from beyond their reach while the escort
//! holds the ground in front of the guns.

use super::tactics::{
    add_producer, gather, grow_economy, guard_home, home_post, objective, train_mix,
};
use super::turn::{Prey, Turn, building_target, footprint, footprint_distance};
use super::{Memory, between, pick};
use aftermine_sim::{AttackTarget, BuildingKind, UnitId, UnitKind};
use chassis::grid::TilePos;
use chassis::rng::Pcg32;

/// What a seed varies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Params {
    /// Harvesters to keep.
    workers: u32,
    /// Extractors to restore.
    extractors: u32,
    /// Bombards before the siege sets out.
    bombards: u32,
    /// What escorts the guns.
    escort: UnitKind,
    /// Escorts before the siege sets out.
    escorts: u32,
    /// Kestrels to spot.
    kestrels: u32,
    /// Sentinels kept home.
    guards: u32,
    /// What the guns shell first: defenses, or the Foundry.
    prey: Prey,
}

impl Params {
    pub(super) fn draw(rng: &mut Pcg32) -> Self {
        Self {
            workers: between(rng, 7, 9),
            extractors: between(rng, 2, 4),
            bombards: between(rng, 3, 5),
            escort: pick(rng, &[UnitKind::Sentinel, UnitKind::Warden]),
            escorts: between(rng, 3, 5),
            kestrels: between(rng, 1, 2),
            guards: between(rng, 1, 3),
            prey: pick(rng, &[Prey::Defenses, Prey::Foundry]),
        }
    }
}

pub(super) fn decide(turn: &mut Turn<'_>, params: &Params, memory: &mut Memory) {
    let late = turn.tick > 7200;
    grow_economy(turn, params.workers, params.extractors, 3000);
    let home = turn.home();
    let post = home_post(turn);
    if turn.ensure(BuildingKind::Fabricator, post, 6) && turn.has(BuildingKind::Fabricator) {
        turn.ensure(BuildingKind::Airworks, home, 7);
    }
    let guards = guard_home(
        turn,
        memory,
        UnitKind::Sentinel,
        params.guards + u32::try_from(turn.tick / 3000).unwrap_or(0).min(3),
        turn.has(BuildingKind::Fabricator),
    );
    if turn.has(BuildingKind::Fabricator) && turn.structures(BuildingKind::Turret).next().is_none()
    {
        turn.build(BuildingKind::Turret, post, 5, None);
    }

    let guns: Vec<UnitId> = turn.units(UnitKind::Bombard).map(|u| u.id).collect();
    let escorts: Vec<UnitId> = turn
        .army()
        .iter()
        .filter(|u| u.kind != UnitKind::Bombard && !guards.contains(&u.id))
        .map(|u| u.id)
        .collect();
    let spotters: Vec<UnitId> = turn.units(UnitKind::Kestrel).map(|u| u.id).collect();
    let count = |ids: &[UnitId]| u32::try_from(ids.len()).unwrap_or(0);
    let need = params.bombards + memory.waves.min(3);
    if !memory.attacking && count(&guns) >= need && count(&escorts) >= params.escorts {
        memory.attacking = true;
    }
    if memory.attacking && guns.is_empty() {
        memory.attacking = false;
        memory.waves += 1;
    }
    if memory.attacking {
        siege(turn, params, &guns, &escorts, &spotters);
    } else {
        let mut all = guns.clone();
        all.extend(&escorts);
        if !turn.defend(&escorts, 9) {
            gather(turn, &all, post, Prey::Defenses);
        }
        if let Some(&lead) = guns.first() {
            turn.accompany(&spotters, lead);
        } else {
            gather(turn, &spotters, post, Prey::Defenses);
        }
    }

    let escorted = turn.count(params.escort) >= params.escorts + 2 * u32::from(late);
    if !escorted {
        train_mix(
            turn,
            &[(params.escort, 2), (UnitKind::Lancer, 1)],
            home,
            true,
        );
    }
    if let Some(fabricator) = turn.finished(BuildingKind::Fabricator).next() {
        let (id, anchor) = (fabricator.id, fabricator.anchor);
        turn.rally(id, post);
        turn.produce(UnitKind::Bombard, anchor, 1);
    }
    if escorted {
        train_mix(
            turn,
            &[(params.escort, 2), (UnitKind::Lancer, 1)],
            home,
            false,
        );
    }
    if let Some(airworks) = turn.finished(BuildingKind::Airworks).next()
        && turn.count(UnitKind::Kestrel) < params.kestrels
    {
        let anchor = airworks.anchor;
        turn.train(UnitKind::Kestrel, anchor, 1);
    }
    if turn.spendable() > 400 {
        add_producer(turn, BuildingKind::Fabricator, 3);
    }
    let sites = turn.map.enemy_harvest_sites(14);
    turn.scout_sites(&sites, 1);
}

/// The siege creeps forward: the escort holds the ground a few tiles
/// ahead of the guns, the guns turn on anything that comes at them and
/// otherwise shell the best known structure in range or close the distance
/// behind the escort, and the spotters ride over the lead gun.
fn siege(
    turn: &mut Turn<'_>,
    params: &Params,
    guns: &[UnitId],
    escorts: &[UnitId],
    spotters: &[UnitId],
) {
    let Some(centre) = turn.centre(guns) else {
        return;
    };
    let start_seen_gone = footprint(turn.map.enemy, BuildingKind::Foundry.size())
        .any(|t| turn.obs.visible(t))
        && turn
            .enemy_structures(BuildingKind::Foundry)
            .all(|b| b.anchor != turn.map.enemy);
    let goal = if start_seen_gone {
        objective(turn)
    } else {
        turn.map.enemy_centre()
    };
    let target = siege_target(turn, centre, params.prey);
    let aim = target.map_or(goal, |(tile, _)| tile);
    let ahead = turn.map.walk_toward(centre, aim, 4);
    let threat = turn.threats_near(centre, 8).first().map(|u| u.tile);
    if !escorts.is_empty() {
        turn.hunt(escorts, threat.unwrap_or(ahead));
    }
    let pressed = !turn.threats_near(centre, 3).is_empty();
    match target {
        _ if pressed => {
            let back = turn.map.walk_toward(centre, turn.home(), 4);
            turn.run(guns, back);
        }
        Some((tile, target)) if tile.chebyshev(centre) <= 12 => {
            turn.attack(guns, target);
        }
        _ => {
            turn.hunt(guns, turn.map.walk_toward(centre, aim, 3));
        }
    }
    let lead = turn
        .obs
        .my_units
        .iter()
        .filter(|u| guns.contains(&u.id))
        .min_by_key(|u| (u.tile.chebyshev(aim), u.id))
        .map(|u| u.id);
    if let Some(lead) = lead {
        turn.accompany(spotters, lead);
    }
}

/// The known enemy structure the guns should shell from near `centre`:
/// armed ones within reach of the march first when `prey` says defenses,
/// otherwise the nearest Foundry, then anything near.
fn siege_target(turn: &Turn<'_>, centre: TilePos, prey: Prey) -> Option<(TilePos, AttackTarget)> {
    let known = |want: &dyn Fn(BuildingKind) -> bool, reach: i32| {
        turn.obs
            .enemy_buildings
            .iter()
            .filter(|b| b.hp > 0 && !b.provisional && want(b.kind))
            .filter(|b| footprint_distance(b, centre) <= reach)
            .min_by_key(|b| (footprint_distance(b, centre), b.anchor.y, b.anchor.x))
            .map(|b| (b.anchor, building_target(b)))
    };
    let armed = || known(&|kind| kind.fires(), 11);
    let foundry = || known(&|kind| kind == BuildingKind::Foundry, 14);
    let any = || known(&|kind| kind != BuildingKind::ScuttleCharge, 10);
    match prey {
        Prey::Foundry => foundry().or_else(armed).or_else(any),
        _ => armed().or_else(foundry).or_else(any),
    }
}
