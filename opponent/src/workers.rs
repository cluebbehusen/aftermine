//! Workers (Harvesters and Excavators): how many the seat wants, training
//! them, keeping them out of harm, and sending free ones to build, weld
//! buildings and wounded units, and harvest.

use crate::decision::{Ledger, Producer};
use crate::frame::{HomeFrame, doubled, footprint_centre, gap, ring};
use crate::map::{MapModel, UNREACHABLE};
use crate::missions::{Hazard, Meal, Scratch};
use crate::profile::ResolvedProfile;
use chassis::fx::Fx;
use chassis::grid::{Grid, TilePos};
use oxide_sim::TICKS_PER_SECOND;
use oxide_sim::ids::Target;
use oxide_sim::observation::{BuildingObs, ObservationData, RepairWork, UnitObs};
use oxide_sim::scenario::BotStance;
use oxide_sim::stats::{FOUNDRY_REPAIR_PRICE, HARVEST_INCIDENT_DANGER_RADIUS};
use oxide_sim::{BuildingKind, Command, UnitId, UnitKind};
use std::borrow::Cow;
use std::cmp::Reverse;

/// Per mille of its health a building must miss before workers weld it.
const WELD_DAMAGE: u32 = 100;

/// Scrap the seat keeps spendable before starting a weld, which bills as it
/// goes: a weld started with less soon stalls for want of scrap.
pub(crate) const WELD_FLOOR: u32 = 100;

/// Empty tiles around a building inside which an armed enemy in sight keeps
/// workers from welding it.
const WELD_CLEARANCE: i32 = 12;

/// Empty tiles from an own Foundry inside which a worker counts as home.
const HOME_TILES: i32 = 8;

/// Per mille of its health a unit must miss before workers weld it or it
/// goes to a Repair Bay.
pub(crate) const UNIT_WELD_DAMAGE: u64 = 100;

/// Scrap of missing health among wounded units one welder answers for at
/// middling support: how many welders the seat sends follows from need.
const WOUNDS_PER_WELDER: u64 = 500;

/// Scrap of missing health among the armed ground units of `units`.
pub(crate) fn wounds<'a>(units: impl Iterator<Item = &'a UnitObs>) -> u64 {
    units
        .filter(|unit| armed_ground(unit.kind))
        .map(|unit| {
            let stats = unit.kind.stats();
            let max = u64::from(stats.max_hp.max(1));
            u64::from(stats.cost) * max.saturating_sub(u64::from(unit.hp)) / max
        })
        .sum()
}

/// Missing health one welder answers for: less the more the seat leans on
/// support.
pub(crate) fn per_welder(support: u8) -> u64 {
    WOUNDS_PER_WELDER * 1_000 / crate::composition::weight(support)
}

/// An armed ground machine a weld can mend.
fn armed_ground(kind: UnitKind) -> bool {
    let stats = kind.stats();
    stats.domain == oxide_sim::stats::Domain::Ground
        && !stats.weapons.is_empty()
        && stats.repairable()
}

/// The nodes the seat's Foundries work, each with the Harvesters it wants
/// there, and the worker slots it fills.
pub(crate) struct Staffing {
    worked: Vec<(TilePos, usize)>,
    /// Known nodes with scrap whose route from home avoids known danger, in
    /// `known_scrap` order.
    reachable: Vec<TilePos>,
    /// Known wreck off any node whose route from home avoids known danger,
    /// with its salvage, in `known_wrecks` order.
    wrecks: Vec<(TilePos, u32)>,
    crew: usize,
    /// Each ground with a built Foundry: the Harvester slots its worked
    /// sources want, at least one for a builder, and those its workers,
    /// alive or queued there, fill. Every ground trains its own workers.
    local: Vec<(u32, usize, usize)>,
}

impl Staffing {
    /// The worked nodes with the Harvesters each wants.
    #[cfg(test)]
    pub(crate) fn crews(&self) -> &[(TilePos, usize)] {
        &self.worked
    }

    fn wanted(&self) -> usize {
        self.worked.iter().map(|(_, crew)| crew).sum()
    }

    /// Filled worker slots as a per-mille share of those wanted.
    pub(crate) fn saturation(&self) -> u32 {
        let wanted = self.wanted();
        if wanted == 0 {
            return 1_000;
        }
        u32::try_from(self.crew.min(wanted) * 1_000 / wanted)
            .expect("a per-mille share fits in u32")
    }
}

/// Whether `kind` harvests, builds and welds: a Harvester or an Excavator.
pub(crate) fn worker(kind: UnitKind) -> bool {
    kind.stats().harvest.is_some()
}

/// Harvester slots a worker of `kind` fills: its mining rate over a
/// Harvester's, so an Excavator, twice as fast, fills two.
fn slots(kind: UnitKind) -> usize {
    let per_scrap = |kind: UnitKind| kind.stats().harvest.map(|harvest| harvest.ticks_per_scrap);
    match (per_scrap(UnitKind::Harvester), per_scrap(kind)) {
        (Some(harvester), Some(ticks)) => (harvester / ticks.max(1)).max(1) as usize,
        _ => 0,
    }
}

/// Counts the worked nodes and wrecks with their crews and the worker slots
/// filled by workers on each base's ground or queued there. Wreck a wounded kiln machine
/// feeds on where it stands, or is bound for in `meals`, is left to it.
pub(crate) fn staffing(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    profile: &ResolvedProfile,
    (foundries, meals): (&[Producer<'_>], &[Meal]),
    scratch: &Scratch,
) -> Staffing {
    let homes: Vec<&BuildingObs> = foundries.iter().map(|foundry| foundry.building).collect();
    let mut routes = Routes::new(map, frame, scratch, &homes);
    let reachable: Vec<TilePos> = observation
        .known_scrap
        .iter()
        .filter(|(_, amount)| *amount > 0)
        .map(|(node, _)| *node)
        .filter(|node| routes.clear(*node))
        .collect();
    // Wreck lies where something died: until the warning of an allied loss
    // expires, salvage keeps clear of its ground, even at home, as the
    // simulation's own salvage does.
    let warned = |tile: TilePos| {
        observation
            .salvage_incidents
            .iter()
            .any(|incident| incident.chebyshev(tile) <= HARVEST_INCIDENT_DANGER_RADIUS)
    };
    let mut salvage_routes = routes.around_incidents(&observation.salvage_incidents);
    let wrecks: Vec<(TilePos, u32)> = observation
        .known_wrecks
        .iter()
        .filter(|(tile, amount)| {
            *amount > 0 && !observation.known_scrap_at(*tile) && !warned(*tile)
        })
        // A wounded kiln machine has the wreck it feeds on, and the wreck it
        // drives to: workers would strip it before the slow machine arrives.
        .filter(|(tile, _)| {
            !observation
                .my_units
                .iter()
                .any(|unit| crate::missions::feeds(unit, *tile))
                && !meals.iter().any(|meal| meal.reserves(*tile))
        })
        .filter(|(tile, _)| salvage_routes.clear(*tile))
        .copied()
        .collect();
    let worked = worked_nodes(
        observation,
        map,
        frame,
        profile,
        foundries,
        &reachable,
        &wrecks,
    );
    let mut components: Vec<_> = foundries
        .iter()
        .filter_map(|foundry| map.component(foundry.building.anchor))
        .collect();
    components.sort_unstable();
    components.dedup();
    let local: Vec<_> = components
        .into_iter()
        .map(|component| {
            let on_ground = |tile| map.component(tile) == Some(component);
            // A base keeps at least a builder for its construction and welds,
            // whatever is left to mine.
            let wanted = worked
                .iter()
                .filter(|(tile, _)| on_ground(*tile))
                .map(|(_, wanted)| wanted)
                .sum::<usize>()
                .max(1);
            let crew = observation
                .my_units
                .iter()
                .filter(|u| on_ground(u.tile))
                .map(|u| slots(u.kind))
                .sum::<usize>()
                + observation
                    .my_buildings
                    .iter()
                    .zip(&observation.my_queues)
                    .filter(|(b, _)| on_ground(b.anchor))
                    .flat_map(|(_, q)| q)
                    .map(|kind| slots(*kind))
                    .sum::<usize>();
            (component, wanted, crew)
        })
        .collect();
    Staffing {
        worked,
        reachable,
        wrecks,
        crew: local.iter().map(|(_, wanted, crew)| crew.min(wanted)).sum(),
        local,
    }
}

/// With no worker alive or queued, the nearest Foundry queues a Harvester
/// even behind other work, from protected scrap if it must.
pub(crate) fn recover(
    observation: &ObservationData,
    foundries: &[Producer<'_>],
    ledger: &mut Ledger,
) {
    if crew(observation) > 0 {
        return;
    }
    if let Some(foundry) = foundries.first() {
        ledger.train_urgently(foundry.building.id, UnitKind::Harvester);
    }
}

/// Trains workers at ready Foundries until the worked nodes' crews are full,
/// counting those alive and queued, spending at most `budget` on them.
pub(crate) fn train(
    observation: &ObservationData,
    map: &MapModel,
    foundries: &[Producer<'_>],
    staffing: &Staffing,
    greed: u8,
    mut budget: u64,
    ledger: &mut Ledger,
) {
    let mut local = staffing.local.clone();
    for (component, _, count) in &mut local {
        *count += foundries
            .iter()
            .filter(|foundry| map.component(foundry.building.anchor) == Some(*component))
            .flat_map(|foundry| ledger.trained_at(foundry.building.id))
            .map(slots)
            .sum::<usize>();
    }
    for foundry in foundries {
        let Some((_, wanted, count)) = local
            .iter_mut()
            .find(|(component, _, _)| map.component(foundry.building.anchor) == Some(*component))
        else {
            continue;
        };
        let open = wanted.saturating_sub(*count);
        if open == 0 {
            continue;
        }
        if !foundry.ready || ledger.queued_at(foundry.building.id) {
            continue;
        }
        let kind = if excavate(observation, open, greed, ledger) {
            UnitKind::Excavator
        } else {
            UnitKind::Harvester
        };
        let cost = u64::from(kind.stats().cost);
        if cost > budget {
            break;
        }
        if ledger.train(foundry.building.id, kind) {
            *count += slots(kind);
            budget -= cost;
        }
    }
}

/// Brings workers away from home back out of known enemy fire, sends a
/// worker to each unattended construction site, welds damaged buildings and
/// then the wounded among `patients`, then sends idle workers to the
/// least-worked node whose route and site are clear of known danger.
#[expect(
    clippy::too_many_arguments,
    reason = "each pass reads a different slice of the decision"
)]
pub(crate) fn run(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    hazards: &[Hazard],
    staffing: &Staffing,
    patients: &[UnitId],
    support: u8,
    ledger: &mut Ledger,
) {
    flee(observation, map, frame, hazards, ledger);
    resume_sites(observation, map, frame, ledger);
    crew_cradles(observation, map, frame, ledger);
    weld(observation, map, frame, hazards, ledger);
    weld_units(
        observation,
        map,
        frame,
        hazards,
        patients,
        per_welder(support),
        ledger,
    );
    assign_idle(observation, map, frame, staffing, hazards, ledger);
}

/// Whether the next worker is an Excavator: once a Fabricator stands, when
/// two Harvesters' slots are open and the seat can pay with scrap to spare,
/// less to spare the greedier it is.
fn excavate(observation: &ObservationData, open: usize, greed: u8, ledger: &Ledger) -> bool {
    let fabricator = observation
        .my_buildings
        .iter()
        .any(|building| building.kind == BuildingKind::Fabricator && building.built);
    let spare = 2 * (100 - u32::from(greed.min(100)));
    fabricator
        && open >= slots(UnitKind::Excavator)
        && ledger.spendable() >= UnitKind::Excavator.stats().cost + spare
}

/// Sends each harvesting, idle or tending worker away from home that known
/// enemy fire reaches back beside the nearest own Foundry on its ground. An
/// explicit harvest order keeps a worker at its node under fire until it is
/// told otherwise.
fn flee(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    hazards: &[Hazard],
    ledger: &mut Ledger,
) {
    let foundries = built_foundries(observation);
    if hazards.is_empty() {
        return;
    }
    for unit in &observation.my_units {
        let working =
            unit.idle || unit.repair == Some(RepairWork::Tending) || unit.harvesting.is_some();
        if !worker(unit.kind) || !working || ledger.employs(unit.id) {
            continue;
        }
        let threatened = hazards
            .iter()
            .any(|hazard| hazard.covers(doubled(unit.tile)));
        if !threatened || at_home(&foundries, unit.tile) {
            continue;
        }
        let Some(refuge) = refuge(map, frame, &foundries, unit.tile) else {
            continue;
        };
        if !ledger.order(Command::Run {
            units: vec![unit.id],
            goal: refuge,
            queue: false,
        }) {
            return;
        }
    }
}

/// The seat's built Foundries.
/// Whether `tile` lies within [`HOME_TILES`] of one of `foundries`.
fn at_home(foundries: &[&BuildingObs], tile: TilePos) -> bool {
    let size = BuildingKind::Foundry.size();
    foundries
        .iter()
        .any(|foundry| gap(foundry.anchor, size, tile, (1, 1)) <= HOME_TILES)
}

/// Whether `unit` is a worker on `ground` free for new work: not building,
/// founding or welding, given no work by this decision and not sitting out
/// orders.
fn free_on(unit: &UnitObs, map: &MapModel, ground: u32, ledger: &Ledger) -> bool {
    worker(unit.kind)
        && unit.site.is_none()
        && unit.founding.is_none()
        && unit.repair != Some(RepairWork::Ordered)
        && map.component(unit.tile) == Some(ground)
        && !ledger.employs(unit.id)
        && !ledger.stuck(unit.id)
}

fn built_foundries(observation: &ObservationData) -> Vec<&BuildingObs> {
    observation
        .my_buildings
        .iter()
        .filter(|building| building.kind == BuildingKind::Foundry && building.built)
        .collect()
}

/// The tile beside one of `foundries` on the ground of `tile` nearest it.
fn refuge(
    map: &MapModel,
    frame: HomeFrame,
    foundries: &[&BuildingObs],
    tile: TilePos,
) -> Option<TilePos> {
    let size = BuildingKind::Foundry.size();
    let ground = map.component(tile)?;
    let from = doubled(tile);
    foundries
        .iter()
        .flat_map(|foundry| ring(foundry.anchor, size))
        .filter(|beside| map.component(*beside) == Some(ground))
        .min_by_key(|beside| (frame.rank(from, doubled(*beside)), *beside))
}

/// Sends the nearest free worker to weld each damaged own building nobody
/// welds yet, most missing value first, while no armed enemy in sight stands
/// near it, no known enemy weapon reaches it, and the seat has scrap to pay.
fn weld(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    hazards: &[Hazard],
    ledger: &mut Ledger,
) {
    // Buildings a worker already welds, on order or on its own.
    let tended: Vec<oxide_sim::BuildingId> = observation
        .my_units
        .iter()
        .filter(|unit| worker(unit.kind) && unit.repair.is_some())
        .filter_map(|unit| match observation.repair_target(unit.id)? {
            Target::Building(building) => Some(building),
            Target::Unit(_) => None,
        })
        .collect();
    let mut patients: Vec<(u64, (i64, i64), &BuildingObs)> = observation
        .my_buildings
        .iter()
        .filter(|building| building.built && !building.provisional)
        .filter(|building| !tended.contains(&building.id))
        .filter_map(|building| {
            let stats = building.kind.tier_stats(building.tier);
            let (hp, max) = (u64::from(building.hp), u64::from(stats.max_hp.max(1)));
            if hp * 1_000 > max * u64::from(1_000 - WELD_DAMAGE) {
                return None;
            }
            let size = building.kind.size();
            let centre = footprint_centre(building.kind, building.anchor);
            if !calm(observation, hazards, building.anchor, size, centre) {
                return None;
            }
            // A building other buildings and rock wall in on every side
            // leaves a welder nowhere to stand.
            let ground = map.component(building.anchor)?;
            if room(observation, map, building.anchor, size, ground) == 0 {
                return None;
            }
            let price = building
                .kind
                .base_stats()
                .construction
                .as_ref()
                .map_or(FOUNDRY_REPAIR_PRICE, |construction| construction.cost);
            let missing = u64::from(price) * (max - hp) / max;
            Some((missing, centre, building))
        })
        .collect();
    patients.sort_by_key(|(missing, centre, building)| {
        (
            Reverse(*missing),
            frame.rank(frame.home, *centre),
            building.id,
        )
    });
    for (_, centre, building) in patients {
        if ledger.spendable() < WELD_FLOOR {
            return;
        }
        let Some(welder) = builder(observation, map, frame, building.anchor, centre, ledger) else {
            continue;
        };
        if !ledger.order(Command::Repair {
            units: vec![welder],
            building: building.id,
            queue: false,
        }) {
            return;
        }
    }
}

/// Whether no armed enemy in sight stands within the weld clearance of the
/// footprint at `anchor` and no known enemy weapon reaches `centre`.
pub(crate) fn calm(
    observation: &ObservationData,
    hazards: &[Hazard],
    anchor: TilePos,
    size: (i32, i32),
    centre: (i64, i64),
) -> bool {
    let threatened = observation.enemy_units.iter().any(|enemy| {
        enemy.kind.stats().can_fight() && gap(anchor, size, enemy.tile, (1, 1)) <= WELD_CLEARANCE
    });
    !threatened && !hazards.iter().any(|hazard| hazard.covers(centre))
}

/// Sends workers at home to weld the wounded armed ground units of `patients`
/// at home that no worker welds yet, most missing value first, while they are
/// calm and the seat has scrap to pay. They send as many welders as the
/// wounds at home call for, counting those already welding, one to a
/// patient: the sturdiest workers first, so Excavators before Harvesters,
/// then the idle before the harvesting, then the nearest. A worker welding a
/// unit that has left home stops rather than follow it out; one tending on
/// its own counts toward its patient at home and is otherwise left to the
/// simulation, which ends its trip within reach of its post.
fn weld_units(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    hazards: &[Hazard],
    patients: &[UnitId],
    per_welder: u64,
    ledger: &mut Ledger,
) {
    let foundries = built_foundries(observation);
    let home = |tile: TilePos| at_home(&foundries, tile);
    let mine = |id: &UnitId| {
        let index = observation
            .my_units
            .binary_search_by_key(id, |unit| unit.id)
            .ok()?;
        Some(&observation.my_units[index])
    };
    let mut welded: Vec<UnitId> = Vec::new();
    for unit in observation
        .my_units
        .iter()
        .filter(|unit| worker(unit.kind) && unit.repair.is_some())
    {
        let Some(Target::Unit(patient)) = observation.repair_target(unit.id) else {
            continue;
        };
        if mine(&patient).is_some_and(|patient| home(patient.tile)) {
            welded.push(patient);
        } else if unit.repair == Some(RepairWork::Ordered)
            && !ledger.order(Command::Stop {
                units: vec![unit.id],
            })
        {
            return;
        }
    }
    let wounded: Vec<(u64, &UnitObs)> = patients
        .iter()
        .filter_map(mine)
        .filter(|unit| armed_ground(unit.kind) && home(unit.tile))
        .filter(|unit| calm(observation, hazards, unit.tile, (1, 1), doubled(unit.tile)))
        .filter_map(|unit| {
            let stats = unit.kind.stats();
            let (hp, max) = (u64::from(unit.hp), u64::from(stats.max_hp.max(1)));
            (hp * 1_000 <= max * (1_000 - UNIT_WELD_DAMAGE))
                .then(|| (u64::from(stats.cost) * (max - hp) / max, unit))
        })
        .collect();
    let wanted = wounds(wounded.iter().map(|(_, unit)| *unit)) / per_welder.max(1);
    let (attended, mut unattended): (Vec<_>, Vec<_>) = wounded
        .into_iter()
        .partition(|(_, unit)| welded.contains(&unit.id));
    let mut room = usize::try_from(wanted.max(1))
        .unwrap_or(usize::MAX)
        .saturating_sub(attended.len())
        .min(unattended.len());
    unattended.sort_by_key(|(missing, unit)| {
        (
            Reverse(*missing),
            frame.rank(frame.home, doubled(unit.tile)),
            unit.id,
        )
    });
    for (_, patient) in unattended {
        if room == 0 || ledger.spendable() < WELD_FLOOR {
            return;
        }
        let Some(ground) = map.component(patient.tile) else {
            continue;
        };
        let at = doubled(patient.tile);
        let welder = observation
            .my_units
            .iter()
            .filter(|unit| free_on(unit, map, ground, ledger) && home(unit.tile))
            .min_by_key(|unit| {
                (
                    Reverse(unit.kind.stats().max_hp),
                    !unit.idle,
                    frame.rank(at, doubled(unit.tile)),
                    unit.id,
                )
            });
        let Some(welder) = welder else {
            continue;
        };
        if !ledger.order(Command::RepairUnit {
            units: vec![welder.id],
            target: patient.id,
            queue: false,
        }) {
            return;
        }
        room -= 1;
    }
}

/// The nearest worker to `centre` that stands on the same ground as `site`,
/// is not constructing or welding, and has no work from this decision yet.
pub(crate) fn builder(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    site: TilePos,
    centre: (i64, i64),
    ledger: &Ledger,
) -> Option<UnitId> {
    let ground = map.component(site)?;
    observation
        .my_units
        .iter()
        .filter(|unit| free_on(unit, map, ground, ledger))
        .min_by_key(|unit| (frame.rank(centre, doubled(unit.tile)), unit.id))
        .map(|unit| unit.id)
}

/// Sends the nearest free worker to every paid base-tier site nobody is
/// building. Upgrades rebuild themselves and provisional scaffolds already
/// have their founder.
fn resume_sites(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    ledger: &mut Ledger,
) {
    for site in &observation.my_buildings {
        let attended = observation
            .my_units
            .iter()
            .any(|unit| unit.site == Some(site.id));
        if site.built || site.provisional || site.tier > 0 || attended {
            continue;
        }
        let centre = footprint_centre(site.kind, site.anchor);
        let Some(builder) = builder(observation, map, frame, site.anchor, centre, ledger) else {
            continue;
        };
        let command = Command::Build {
            units: vec![builder],
            kind: site.kind,
            anchor: site.anchor,
            queue: false,
            defer: false,
        };
        if !ledger.order(command) {
            return;
        }
    }
}

/// Tops each own cradle's crew up to what it admits with one order: free
/// workers on its ground, the fastest builders first and then the nearest,
/// never a node's last Harvester. Admission takes a faster worker in place
/// of a slower holder, so the crew only gets quicker.
fn crew_cradles(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    ledger: &mut Ledger,
) {
    for site in observation
        .my_buildings
        .iter()
        .filter(|building| building.kind.assembles().is_some() && !building.built)
    {
        let crew = observation
            .my_units
            .iter()
            .filter(|unit| {
                unit.site == Some(site.id) || unit.founding == Some((site.kind, site.anchor))
            })
            .count();
        let room = oxide_sim::stats::ASSEMBLY_CREW_CAP.saturating_sub(crew);
        let Some(ground) = map.component(site.anchor) else {
            continue;
        };
        if room == 0 {
            continue;
        }
        let last = |unit: &UnitObs| {
            unit.harvesting.is_some_and(|node| {
                observation
                    .my_units
                    .iter()
                    .filter(|other| other.harvesting == Some(node))
                    .count()
                    == 1
            })
        };
        let centre = footprint_centre(site.kind, site.anchor);
        let mut free: Vec<&UnitObs> = observation
            .my_units
            .iter()
            .filter(|unit| free_on(unit, map, ground, ledger) && !last(unit))
            .collect();
        free.sort_by_key(|unit| {
            (
                std::cmp::Reverse(unit.kind.stats().build_rate),
                frame.rank(centre, doubled(unit.tile)),
                unit.id,
            )
        });
        let mut units: Vec<UnitId> = free.iter().take(room).map(|unit| unit.id).collect();
        if units.is_empty() {
            continue;
        }
        units.sort_unstable();
        let command = Command::Build {
            units,
            kind: site.kind,
            anchor: site.anchor,
            queue: false,
            defer: false,
        };
        if !ledger.order(command) {
            return;
        }
    }
}

/// Worker slots filled by workers alive, carried or queued.
fn crew(observation: &ObservationData) -> usize {
    observation
        .my_units
        .iter()
        .map(|unit| unit.kind)
        .chain(observation.my_carried_units.iter().map(|unit| unit.kind))
        .chain(observation.my_queues.iter().flatten().copied())
        .map(slots)
        .sum()
}

/// The live known nodes and wrecks the seat works, each with its crew. A
/// source belongs to the nearest built Foundry whose ground reaches it, is
/// worked when its route from home avoids known danger and a Harvester
/// hauling from there repays its price within the seat's horizon. Its crew
/// fills the free tiles around it that its remaining scrap or salvage
/// repays, as full as stance and greed make it.
fn worked_nodes(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    profile: &ResolvedProfile,
    foundries: &[Producer<'_>],
    reachable: &[TilePos],
    wrecks: &[(TilePos, u32)],
) -> Vec<(TilePos, usize)> {
    let harvester = UnitKind::Harvester.stats();
    let Some(harvest) = harvester.harvest else {
        return Vec::new();
    };
    let horizon = horizon(profile);
    let fill = fill(profile);
    let price = u64::from(harvester.cost.max(1));
    let payback = price.div_ceil(u64::from(harvest.capacity.max(1)));
    let homes: Vec<(u32, (i64, i64), &BuildingObs)> = foundries
        .iter()
        .filter_map(|foundry| {
            let component = map.component(foundry.building.anchor)?;
            let centre = footprint_centre(BuildingKind::Foundry, foundry.building.anchor);
            Some((component, centre, foundry.building))
        })
        .collect();
    observation
        .known_scrap
        .iter()
        .filter(|(node, _)| {
            reachable
                .binary_search_by_key(&(node.y, node.x), |tile| (tile.y, tile.x))
                .is_ok()
        })
        .chain(wrecks)
        .filter_map(|(node, amount)| {
            let (component, reach) = homes
                .iter()
                .filter(|(component, _, _)| map.touches(*node, *component))
                .map(|(component, centre, building)| {
                    let reach = octile_tenths(*centre, doubled(*node));
                    (
                        (reach, frame.rank(doubled(*node), *centre), building.id),
                        *component,
                    )
                })
                .min()
                .map(|((reach, _, _), component)| (component, reach))?;
            // Ticks for one load: standing at the node, then there and back.
            let trip = Fx::from_num(2 * reach) / (Fx::from_num(10) * harvester.speed);
            let cycle =
                u64::from(harvest.capacity * harvest.ticks_per_scrap) + trip.ceil().to_num::<u64>();
            if payback * cycle > horizon {
                return None;
            }
            let room = room(observation, map, *node, (1, 1), component) as u64;
            let repaid = u64::from(*amount) / price;
            let places = room.min(repaid);
            (places > 0).then(|| {
                let crew = (places * fill).div_ceil(1_000).max(1);
                (*node, usize::try_from(crew).expect("crews fit in usize"))
            })
        })
        .collect()
}

/// Tiles known enemy ground fire covers or that lie around an enemy Foundry.
fn danger(map: &MapModel, scratch: &Scratch) -> Grid<bool> {
    let (width, height) = map.size();
    let mut danger = Grid::new(width, height, false);
    let mut mark = |tile: TilePos| {
        if let Some(cell) = danger.get_mut(tile) {
            *cell = true;
        }
    };
    for hazard in &scratch.ground {
        hazard.tiles().for_each(&mut mark);
    }
    let (foundry_width, foundry_height) = BuildingKind::Foundry.size();
    for anchor in scratch.enemy_foundries() {
        for dy in -(HOME_TILES + 1)..=foundry_height + HOME_TILES {
            for dx in -(HOME_TILES + 1)..=foundry_width + HOME_TILES {
                mark(anchor.offset(dx, dy));
            }
        }
    }
    danger
}

/// Whether the shortest ground routes from the seat's start to known
/// sources, which workers roughly follow, stay out of known danger until they
/// come home to one of the seat's Foundries: the reach of known enemy ground
/// fire, and the surroundings of enemy Foundries, known or presumed, where
/// their owners' defenders gather. A route is traced back from its source down
/// the start's distance field; a source that field does not reach is judged
/// by itself. Every route through a tile ends as the first one traced through
/// it did, so a decision traces each tile once however many sources share it.
pub(crate) struct Routes<'a> {
    map: &'a MapModel,
    frame: HomeFrame,
    homes: &'a [&'a BuildingObs],
    danger: Grid<bool>,
    /// Whether the route on from each tile already traced is clear.
    traced: Grid<Option<bool>>,
    distances: Cow<'a, Grid<u16>>,
}

impl<'a> Routes<'a> {
    pub(crate) fn new(
        map: &'a MapModel,
        frame: HomeFrame,
        scratch: &Scratch,
        homes: &'a [&'a BuildingObs],
    ) -> Self {
        let (width, height) = map.size();
        let home = homes
            .iter()
            .find(|base| footprint_centre(base.kind, base.anchor) == frame.home);
        let origins = homes
            .iter()
            .filter(|base| {
                home.is_none_or(|home| {
                    base.id == home.id || map.component(base.anchor) != map.component(home.anchor)
                })
            })
            .map(|base| base.anchor);
        Self {
            map,
            frame,
            homes,
            danger: danger(map, scratch),
            traced: Grid::new(width, height, None),
            distances: map.distances_from(origins),
        }
    }

    /// These routes with the ground around each recent allied loss in
    /// `incidents` counted as danger too.
    pub(crate) fn around_incidents(&self, incidents: &[TilePos]) -> Routes<'_> {
        let mut danger = self.danger.clone();
        let radius = HARVEST_INCIDENT_DANGER_RADIUS;
        for incident in incidents {
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    if let Some(cell) = danger.get_mut(incident.offset(dx, dy)) {
                        *cell = true;
                    }
                }
            }
        }
        let (width, height) = self.map.size();
        Routes {
            map: self.map,
            frame: self.frame,
            homes: self.homes,
            danger,
            traced: Grid::new(width, height, None),
            distances: Cow::Borrowed(self.distances.as_ref()),
        }
    }

    /// Whether the route from home to `source` is clear.
    pub(crate) fn clear(&mut self, source: TilePos) -> bool {
        let map = self.map;
        let mut tile = source;
        let mut distance = self.distances.get(tile).copied().unwrap_or(UNREACHABLE);
        let mut walked = Vec::new();
        let clear = loop {
            // The source's own first step may cut a corner, so only the
            // tiles past it share their outcome.
            if tile != source {
                if let Some(clear) = self.traced.get(tile).copied().flatten() {
                    break clear;
                }
                walked.push(tile);
            }
            if at_home(self.homes, tile) {
                break true;
            }
            if self.danger.get(tile) == Some(&true) {
                break false;
            }
            let from = doubled(tile);
            let open = |tile: TilePos| map.component(tile).is_some();
            // Diagonal steps never cut a corner, as in the distance field. A
            // worker stands on any tile beside the source, so the first step
            // may.
            let legal = |next: &TilePos| {
                let (dx, dy) = (next.x - tile.x, next.y - tile.y);
                tile == source
                    || dx == 0
                    || dy == 0
                    || (open(tile.offset(dx, 0)) && open(tile.offset(0, dy)))
            };
            let next = ring(tile, (1, 1))
                .filter(legal)
                .map(|next| {
                    (
                        self.distances.get(next).copied().unwrap_or(UNREACHABLE),
                        next,
                    )
                })
                .filter(|(closer, _)| *closer < distance)
                .min_by_key(|(closer, next)| (*closer, self.frame.rank(from, doubled(*next))));
            let Some((closer, next)) = next else {
                break true;
            };
            tile = next;
            distance = closer;
        };
        for tile in walked {
            if let Some(cell) = self.traced.get_mut(tile) {
                *cell = Some(clear);
            }
        }
        clear
    }
}

/// Ticks within which a Harvester must repay its price for its node to be
/// worked: longer the more a stance invests at home, stretched by greed.
fn horizon(profile: &ResolvedProfile) -> u64 {
    let minutes_tenths: u64 = match profile.stance {
        BotStance::Turtle => 30,
        BotStance::Balanced => 20,
        BotStance::Aggressive => 15,
    };
    let per_minute = u64::from(TICKS_PER_SECOND) * 60;
    minutes_tenths * per_minute * (200 + u64::from(profile.traits.greed)) / 2_000
}

/// Per mille of a node's places the seat fills: all of them for an economic
/// stance, about half for an aggressive one, more the greedier it is, and as
/// much of that as the seat's difficulty builds of its economy.
pub(crate) fn fill(profile: &ResolvedProfile) -> u64 {
    let base: i64 = match profile.stance {
        BotStance::Turtle => 1_000,
        BotStance::Balanced => 750,
        BotStance::Aggressive => 500,
    };
    (base + (i64::from(profile.traits.greed) - 50) * 5)
        .clamp(250, 1_000)
        .cast_unsigned()
        * crate::economy(profile.difficulty)
        / 1_000
}

/// Free tiles a worker could stand on to work the footprint of `size` at
/// `anchor`: its neighbours on `component` that no known building, the
/// seat's, an ally's or an enemy's, or scrap node covers.
fn room(
    observation: &ObservationData,
    map: &MapModel,
    anchor: TilePos,
    size: (i32, i32),
    component: u32,
) -> usize {
    let covered = |tile: TilePos| {
        observation
            .my_buildings
            .iter()
            .chain(&observation.ally_buildings)
            .chain(&observation.enemy_buildings)
            .any(|building| {
                let (width, height) = building.kind.size();
                (building.anchor.x..building.anchor.x + width).contains(&tile.x)
                    && (building.anchor.y..building.anchor.y + height).contains(&tile.y)
            })
            || observation
                .known_scrap
                .binary_search_by_key(&(tile.y, tile.x), |(other, _)| (other.y, other.x))
                .is_ok()
    };
    ring(anchor, size)
        .filter(|tile| map.component(*tile) == Some(component) && !covered(*tile))
        .count()
}

/// Sends each idle worker, nearest home first, to the reachable worked node
/// or wreck with the most places open; with none open, to the nearest known
/// wreck holding more salvage than the workers sent to it carry away in a
/// load each, then to the most open worked source with something left, or to
/// its nearest reachable known node when none is worked. It never sends one to a source inside
/// known danger or whose route crosses it. A worker harvesting a source whose
/// route has turned dangerous is sent elsewhere the same way, or home when
/// nowhere is left. A worker with nowhere to go that still carries scrap
/// delivers it instead. One order per source.
fn assign_idle(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    staffing: &Staffing,
    hazards: &[Hazard],
    ledger: &mut Ledger,
) {
    let safe = |node: &TilePos| {
        let point = doubled(*node);
        !hazards.iter().any(|hazard| hazard.covers(point))
    };
    let mut miners: Vec<(TilePos, i64)> = staffing
        .worked
        .iter()
        .filter(|(node, _)| safe(node))
        .map(|(node, crew)| {
            let working = observation
                .my_units
                .iter()
                .filter(|unit| unit.harvesting == Some(*node))
                .count();
            (
                *node,
                i64::try_from(*crew).expect("crews fit in i64")
                    - i64::try_from(working).expect("unit counts fit in i64"),
            )
        })
        .collect();
    // The simulation's harvest loop moves a worker on to nearby wreck when
    // its node runs dry, so a reachable wreck is as good as a node.
    let stale = |unit: &UnitObs| {
        unit.harvesting.is_some_and(|source| {
            let key = (source.y, source.x);
            staffing
                .reachable
                .binary_search_by_key(&key, |tile| (tile.y, tile.x))
                .is_err()
                && staffing
                    .wrecks
                    .binary_search_by_key(&key, |(tile, _)| (tile.y, tile.x))
                    .is_err()
        })
    };
    let mut salvage: Vec<(TilePos, i64)> = staffing
        .wrecks
        .iter()
        .filter(|(tile, _)| safe(tile))
        .map(|(tile, amount)| {
            let loads: u32 = observation
                .my_units
                .iter()
                .filter(|unit| unit.harvesting == Some(*tile))
                .map(|unit| capacity(unit.kind))
                .sum();
            (*tile, i64::from(*amount) - i64::from(loads))
        })
        .collect();
    let mut idle: Vec<_> = observation
        .my_units
        .iter()
        .filter(|unit| {
            (unit.idle || stale(unit))
                && worker(unit.kind)
                && !ledger.employs(unit.id)
                && !ledger.stuck(unit.id)
        })
        .collect();
    idle.sort_by_key(|unit| (frame.rank(frame.home, doubled(unit.tile)), unit.id));
    let mut assignments: Vec<(TilePos, UnitId)> = Vec::new();
    let mut loaded: Vec<&UnitObs> = Vec::new();
    let mut stranded: Vec<&UnitObs> = Vec::new();
    for unit in idle {
        let Some(component) = map.component(unit.tile) else {
            continue;
        };
        let from = doubled(unit.tile);
        // A worked wreck whose salvage the workers already sent carry away
        // has nothing left for another.
        let spoken_for = |source: TilePos| {
            salvage
                .binary_search_by_key(&(source.y, source.x), |(tile, _)| (tile.y, tile.x))
                .is_ok_and(|index| salvage[index].1 <= 0)
        };
        let most_open = miners
            .iter_mut()
            .filter(|(node, _)| map.touches(*node, component) && !spoken_for(*node))
            .min_by_key(|(node, open)| (Reverse(*open), frame.rank(from, doubled(*node))));
        let leftover = salvage
            .iter_mut()
            .filter(|(tile, left)| *left > 0 && map.touches(*tile, component))
            .min_by_key(|(tile, _)| frame.rank(from, doubled(*tile)));
        let node = match (most_open, leftover) {
            (Some((node, open)), _) if *open > 0 => {
                *open -= 1;
                *node
            }
            (_, Some((tile, _))) => *tile,
            (Some((node, open)), None) => {
                *open -= 1;
                *node
            }
            (None, None) => {
                let nearest = staffing
                    .reachable
                    .iter()
                    .copied()
                    .filter(|node| map.touches(*node, component) && safe(node))
                    .min_by_key(|node| frame.rank(from, doubled(*node)));
                let Some(node) = nearest else {
                    if unit.carrying > 0 {
                        loaded.push(unit);
                    } else if stale(unit) {
                        stranded.push(unit);
                    }
                    continue;
                };
                node
            }
        };
        // A worker filling an open place on a worked wreck carries its load
        // away too, so spare workers count it.
        if let Ok(index) =
            salvage.binary_search_by_key(&(node.y, node.x), |(tile, _)| (tile.y, tile.x))
        {
            salvage[index].1 -= i64::from(capacity(unit.kind));
        }
        assignments.push((node, unit.id));
    }
    assignments.sort_by_key(|(node, unit)| (frame.rank(frame.home, doubled(*node)), *unit));
    for group in assignments.chunk_by(|a, b| a.0 == b.0) {
        let command = Command::Harvest {
            units: group.iter().map(|(_, unit)| *unit).collect(),
            node: group[0].0,
            queue: false,
        };
        if !ledger.order(command) {
            return;
        }
    }
    let foundries = built_foundries(observation);
    // Only a worker with a Foundry on its ground can deliver, and an order
    // in which none can is refused.
    let returning: Vec<UnitId> = loaded
        .iter()
        .filter(|unit| refuge(map, frame, &foundries, unit.tile).is_some())
        .map(|unit| unit.id)
        .collect();
    if !returning.is_empty()
        && !ledger.order(Command::ReturnCargo {
            units: returning,
            foundry: None,
            repair: false,
        })
    {
        return;
    }
    for unit in stranded {
        let Some(refuge) = refuge(map, frame, &foundries, unit.tile) else {
            continue;
        };
        if !ledger.order(Command::Run {
            units: vec![unit.id],
            goal: refuge,
            queue: false,
        }) {
            return;
        }
    }
}

/// Scrap a worker of `kind` carries in one load.
fn capacity(kind: UnitKind) -> u32 {
    kind.stats().harvest.map_or(0, |harvest| harvest.capacity)
}

/// Octile distance between two doubled-coordinate points, in tenths of a tile.
fn octile_tenths(a: (i64, i64), b: (i64, i64)) -> i64 {
    let (dx, dy) = ((a.0 - b.0).abs(), (a.1 - b.1).abs());
    let doubled = 10 * dx.max(dy) + 4 * dx.min(dy);
    doubled / 2
}
