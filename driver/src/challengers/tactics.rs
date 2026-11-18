//! Plays several plans share: guarding home, attacking in waves that fall
//! back to regroup, and raiding where the enemy is weak.

use super::Memory;
use super::turn::{Build, Prey, Turn, fighter, footprint, footprint_distance};
use aftermine_sim::observation::UnitObs;
use aftermine_sim::stats::Domain;
use aftermine_sim::{BuildingKind, UnitId, UnitKind};
use chassis::grid::TilePos;

/// Keeps `count` units of `kind` at home: trained there when short and
/// `train` allows, hunting what comes near the seat's structures and
/// workers, and back at the home post between fights. Returns the guards,
/// which the rest of a plan leaves alone.
pub(super) fn guard_home(
    turn: &mut Turn<'_>,
    memory: &mut Memory,
    kind: UnitKind,
    count: u32,
    train: bool,
) -> Vec<UnitId> {
    let home = turn.home();
    memory
        .guards
        .retain(|id| turn.obs.my_units.iter().any(|u| u.id == *id));
    let wanted = usize::try_from(count).unwrap_or(0);
    memory.guards.truncate(wanted);
    while memory.guards.len() < wanted {
        let recruit = turn
            .units(kind)
            .filter(|u| !memory.guards.contains(&u.id) && !memory.raid.contains(&u.id))
            .filter(|u| u.tile.chebyshev(home) <= 12)
            .min_by_key(|u| (u.tile.chebyshev(home), u.id))
            .map(|u| u.id);
        match recruit {
            Some(id) => memory.guards.push(id),
            None => break,
        }
    }
    if train && memory.guards.len() < wanted {
        turn.train(kind, home, 1);
    }
    if train && enemy_flies(turn) && turn.structures(BuildingKind::FlakTurret).next().is_none() {
        turn.build(BuildingKind::FlakTurret, home, 5, None);
    }
    let guards = memory.guards.clone();
    if !turn.defend(&guards, 9) {
        let post = home_post(turn);
        let away: Vec<UnitId> = turn
            .obs
            .my_units
            .iter()
            .filter(|u| guards.contains(&u.id) && u.tile.chebyshev(post) > 3)
            .map(|u| u.id)
            .collect();
        turn.hunt(&away, post);
    }
    guards
}

/// The tile home's defenders wait on: a few steps out from home toward the
/// enemy.
pub(super) fn home_post(turn: &Turn<'_>) -> TilePos {
    let route = turn.map.route_length();
    turn.map.approach(route - 5, 4, 1)
}

/// Where the main force marches: the known enemy Foundry nearest home,
/// live or remembered; the enemy's starting Foundry until the seat has seen
/// it gone; then the known enemy structure nearest home.
pub(super) fn objective(turn: &Turn<'_>) -> TilePos {
    let home = turn.home();
    let foundry = turn
        .obs
        .enemy_buildings
        .iter()
        .filter(|b| b.kind == BuildingKind::Foundry && b.hp > 0)
        .min_by_key(|b| (footprint_distance(b, home), b.anchor.y, b.anchor.x))
        .map(|b| b.anchor.offset(1, 1));
    if let Some(tile) = foundry {
        return tile;
    }
    let start = turn.map.enemy;
    let seen_gone = footprint(start, BuildingKind::Foundry.size()).any(|t| turn.obs.visible(t));
    if !seen_gone {
        return turn.map.enemy_centre();
    }
    turn.obs
        .enemy_buildings
        .iter()
        .filter(|b| b.hp > 0 && b.kind != BuildingKind::ScuttleCharge)
        .min_by_key(|b| (footprint_distance(b, home), b.anchor.y, b.anchor.x))
        .map_or(turn.map.enemy_centre(), |b| b.anchor)
}

/// The main force's waves. `army` gathers at `post` until it numbers
/// `force`, plus two for each wave that has fallen back, then attacks the
/// [`objective`]; with `regroup`, a wave cut below half that number falls
/// back to gather again. Units far behind the front march to join it.
pub(super) fn campaign(
    turn: &mut Turn<'_>,
    memory: &mut Memory,
    army: &[UnitId],
    post: TilePos,
    force: u32,
    prey: Prey,
    regroup: bool,
) {
    let size = u32::try_from(army.len()).unwrap_or(u32::MAX);
    let need = force + 2 * memory.waves.min(4);
    if !memory.attacking && size >= need {
        memory.attacking = true;
    }
    if memory.attacking && (size == 0 || (regroup && size * 2 < need)) {
        memory.attacking = false;
        memory.waves += 1;
    }
    if !memory.attacking {
        if !turn.defend(army, 9) {
            gather(turn, army, post, prey);
        }
        return;
    }
    let target = objective(turn);
    let lead = turn
        .obs
        .my_units
        .iter()
        .filter(|u| army.contains(&u.id))
        .min_by_key(|u| (u.tile.chebyshev(target), u.id))
        .map(|u| u.tile);
    let Some(lead) = lead else { return };
    let mut front = Vec::new();
    let mut behind = Vec::new();
    for unit in turn.obs.my_units.iter().filter(|u| army.contains(&u.id)) {
        if unit.tile.chebyshev(lead) <= 10 {
            front.push(unit.id);
        } else {
            behind.push(unit.id);
        }
    }
    turn.engage(&front, target, prey);
    if !behind.is_empty() {
        let join = turn.centre(&front).unwrap_or(lead);
        turn.engage(&behind, join, prey);
    }
}

/// Sends the units of `army` more than a few tiles from `post` back to it,
/// fighting what they meet.
pub(super) fn gather(turn: &mut Turn<'_>, army: &[UnitId], post: TilePos, prey: Prey) {
    let away: Vec<UnitId> = turn
        .obs
        .my_units
        .iter()
        .filter(|u| army.contains(&u.id) && u.tile.chebyshev(post) > 3)
        .map(|u| u.id)
        .collect();
    if !away.is_empty() {
        turn.engage(&away, post, prey);
    }
}

/// What a group is worth in a fight: the scrap price of each fighter,
/// scaled by its health.
pub(super) fn strength<'a>(units: impl Iterator<Item = &'a UnitObs>) -> u32 {
    units
        .filter(|u| fighter(u.kind))
        .map(|u| {
            let stats = u.kind.stats();
            stats.cost * u.hp.min(stats.max_hp) / stats.max_hp.max(1)
        })
        .sum()
}

/// The visible force that can hurt units moving through `domain` near
/// `tile`: armed units within `reach`, and armed structures whose guns
/// cover the tile, each valued like [`strength`].
pub(super) fn danger(turn: &Turn<'_>, tile: TilePos, reach: i32, domain: Domain) -> u32 {
    let near = |u: &&UnitObs| u.tile.chebyshev(tile) <= reach && u.kind.stats().can_target(domain);
    let units = match domain {
        Domain::Ground => strength(turn.obs.enemy_units.iter().filter(near)),
        // Against aircraft, guns built for them count double and the
        // Sentinel's light anti-air gun counts half.
        Domain::Air => {
            let dedicated = |u: &&UnitObs| matches!(u.kind, UnitKind::Flakhound | UnitKind::Talon);
            let light = |u: &&UnitObs| u.kind == UnitKind::Sentinel;
            let enemies = || turn.obs.enemy_units.iter().filter(near);
            strength(enemies().filter(dedicated)) * 2
                + strength(enemies().filter(light)) / 2
                + strength(enemies().filter(|u| !dedicated(u) && !light(u)))
        }
    };
    let structures: u32 = turn
        .obs
        .enemy_buildings
        .iter()
        .filter(|b| b.hp > 0 && b.built && b.kind.fires())
        .filter(|b| {
            let stats = b.kind.tier_stats(b.tier);
            stats.weapons.iter().any(|w| {
                w.targets.covers(domain)
                    && footprint_distance(b, tile) <= w.range.to_num::<i32>() + 2
            })
        })
        .map(|b| b.kind.invested_cost(b.tier) * 3 / 2)
        .sum();
    units + structures
}

/// Where raids go: the enemy's harvest sites as the map shows them, plus
/// every enemy Extractor the seat knows of, nearest the enemy Foundry first.
pub(super) fn raid_sites(turn: &Turn<'_>) -> Vec<TilePos> {
    let mut sites = turn.map.enemy_harvest_sites(16);
    for b in turn.enemy_structures(BuildingKind::Extractor) {
        let tile = b.anchor.offset(1, 1);
        if sites.iter().all(|s| s.chebyshev(tile) > 3) {
            sites.push(tile);
        }
    }
    sites
}

/// Runs one raid with the group in `memory.raid`: the group goes for the
/// raid site the raid count picks, skipping sites it knows are guarded,
/// takes on what it can beat there, preferring `prey`, and runs home from
/// what it cannot. A raid ends when its group is gone or back home.
pub(super) fn raid(turn: &mut Turn<'_>, memory: &mut Memory, offset: u32, prey: Prey) {
    memory
        .raid
        .retain(|id| turn.obs.my_units.iter().any(|u| u.id == *id));
    if memory.raid.is_empty() {
        if memory.fleeing {
            memory.fleeing = false;
        }
        return;
    }
    let group = memory.raid.clone();
    let Some(centre) = turn.centre(&group) else {
        return;
    };
    let domain = turn
        .obs
        .my_units
        .iter()
        .find(|u| group.contains(&u.id))
        .map_or(Domain::Ground, |u| u.kind.stats().domain);
    let ours = strength(turn.obs.my_units.iter().filter(|u| group.contains(&u.id)));
    let post = home_post(turn);
    if memory.fleeing {
        if centre.chebyshev(post) <= 6 {
            memory.fleeing = false;
            memory.raid.clear();
            memory.raids += 1;
        } else {
            turn.run(&group, post);
        }
        return;
    }
    if danger(turn, centre, 7, domain) > ours {
        memory.fleeing = true;
        turn.run(&group, post);
        return;
    }
    let sites = raid_sites(turn);
    if sites.is_empty() {
        return;
    }
    let count = sites.len();
    let start = usize::try_from(offset + memory.raids).unwrap_or(0);
    let site = (0..count)
        .map(|i| sites[(start + i) % count])
        .find(|&site| danger(turn, site, 6, domain) <= ours)
        .unwrap_or(sites[start % count]);
    let (ground, air) = turn.reach_of(&group);
    let threat = turn
        .threats_near(centre, 7)
        .into_iter()
        .find(|u| match u.body_domain() {
            Domain::Ground => ground,
            Domain::Air => air,
        })
        .map(|u| u.tile);
    if let Some(tile) = threat {
        turn.hunt(&group, tile);
        return;
    }
    if let Some(target) = turn.prey_near(centre, 8, prey, true) {
        turn.attack(&group, target);
        return;
    }
    if centre.chebyshev(site) <= 2 {
        // Nothing here: move on to the next site.
        memory.raids += 1;
    }
    turn.hunt(&group, site);
}

/// Trains the kind of `mix` furthest under its share, each weighted by its
/// count, at the producer nearest `near`; with `save`, sets its price aside
/// when the bank is short. Once the enemy flies, a ground mix with nothing
/// that shoots aircraft well adds Flakhounds, one for every three units.
pub(super) fn train_mix(
    turn: &mut Turn<'_>,
    mix: &[(UnitKind, u32)],
    near: TilePos,
    save: bool,
) -> bool {
    let mut mix = mix.to_vec();
    let answers_air = mix
        .iter()
        .any(|(kind, weight)| *weight > 0 && matches!(kind, UnitKind::Flakhound | UnitKind::Talon));
    if !answers_air && enemy_flies(turn) {
        let total: u32 = mix.iter().map(|(_, weight)| weight).sum();
        mix.push((UnitKind::Flakhound, (total / 3).max(1)));
    }
    let next = mix
        .iter()
        .filter(|(kind, weight)| *weight > 0 && turn.can_train(*kind))
        .min_by_key(|(kind, weight)| (turn.count(*kind) * 64 / weight, kind.stats().cost))
        .map(|(kind, _)| *kind);
    match next {
        Some(kind) if save => turn.produce(kind, near, 1),
        Some(kind) => turn.train(kind, near, 1),
        None => false,
    }
}

/// Whether the seat has seen enemy aircraft or an Airworks.
pub(super) fn enemy_flies(turn: &Turn<'_>) -> bool {
    turn.obs
        .enemy_units
        .iter()
        .any(|u| u.kind.stats().domain == Domain::Air && u.kind != UnitKind::Drone)
        || turn
            .enemy_structures(BuildingKind::Airworks)
            .next()
            .is_some()
}

/// A count that starts at `base` and grows by one every `every` ticks up
/// to `cap`: how many workers or Extractors a growing economy wants.
pub(super) fn grown(base: u32, tick: u64, every: u64, cap: u32) -> u32 {
    let growth = u32::try_from(tick / every.max(1)).unwrap_or(u32::MAX);
    base.saturating_add(growth).min(cap.max(base))
}

/// The economy a plan grows past its opening, ahead of what the plan buys:
/// Extractors from `extractors`, one more each 45 seconds, on frames ever
/// farther from its Foundries; workers from `workers`, one more each 20
/// seconds; from `expand_at`, a Fabricator if it lacks one and a second
/// Foundry, then another every two and a half minutes up to four.
pub(super) fn grow_economy(turn: &mut Turn<'_>, workers: u32, extractors: u32, expand_at: u64) {
    let tick = turn.tick;
    let reach = 12 + i32::try_from((tick / 1800).min(8)).unwrap_or(8);
    turn.economy(grown(extractors, tick, 900, 16), reach);
    grow_workers(turn, workers);
    if tick >= expand_at {
        let post = home_post(turn);
        if turn.ensure(BuildingKind::Fabricator, post, 6) {
            let more = u32::try_from((tick - expand_at) / 3000).unwrap_or(2).min(2);
            turn.expand(2 + more);
        }
    }
}

/// Trains workers toward `workers`, one more each 20 seconds, up to 32.
pub(super) fn grow_workers(turn: &mut Turn<'_>, workers: u32) {
    let target = grown(workers, turn.tick, 400, 32);
    turn.train_workers(target);
}

/// Puts up another `kind` near home, up to `max`, when every finished one
/// is busy and the bank holds its price with room to spare.
pub(super) fn add_producer(turn: &mut Turn<'_>, kind: BuildingKind, max: u32) -> bool {
    let cost = kind.base_stats().construction.cost;
    let all = u32::try_from(turn.structures(kind).count()).unwrap_or(u32::MAX);
    if all == 0
        || all >= max
        || turn.structures(kind).any(|b| !b.built)
        || turn.spendable() < cost + 250
    {
        return false;
    }
    let busy = turn
        .obs
        .my_buildings
        .iter()
        .zip(&turn.obs.my_queues)
        .filter(|(b, _)| b.kind == kind && b.built)
        .all(|(_, queue)| !queue.is_empty());
    let home = turn.home();
    busy && turn.build(kind, home, 9, None) == Build::Placed
}
