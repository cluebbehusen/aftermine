//! Strategic warheads: what a strike on known enemy buildings would take,
//! the Spires and rounds that worth calls for, and the pass that launches
//! loaded Spires. Every estimate uses the simulation's own falloff and
//! resistance; nothing here persists between decisions.

use crate::decision::Ledger;
use crate::frame::{HomeFrame, footprint_centre};
use crate::investments::{Investment, Step};
use crate::map::MapModel;
use crate::profile::PersonalityTraits;
use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::TilePos;
use oxide_sim::observation::{BuildingObs, ObservationData};
use oxide_sim::scenario::BotDifficulty;
use oxide_sim::stats::WARHEAD;
use oxide_sim::{BuildingId, BuildingKind, Command};

/// A building a warhead could damage, as the seat knows it.
#[derive(Clone, Copy)]
pub(crate) struct Mark {
    kind: BuildingKind,
    anchor: TilePos,
    /// Health left once warheads already flying land.
    hp: u32,
    max_hp: u32,
    /// Scrap paid for it, upgrades included.
    paid: u64,
}

impl Mark {
    pub(crate) fn of(building: &BuildingObs) -> Self {
        Mark {
            kind: building.kind,
            anchor: building.anchor,
            hp: building.hp,
            max_hp: building.kind.tier_stats(building.tier).max_hp,
            paid: u64::from(building.kind.invested_cost(building.tier)),
        }
    }

    /// Damage one warhead landing on `impact` deals it, as the simulation
    /// measures: to the footprint's closest point, after resistance.
    fn taken(&self, impact: Vec2Fx) -> u32 {
        let (width, height) = self.kind.size();
        let low = Vec2Fx::new(Fx::from_num(self.anchor.x), Fx::from_num(self.anchor.y));
        let high = low + Vec2Fx::new(Fx::from_num(width), Fx::from_num(height));
        let closest = Vec2Fx::new(impact.x.clamp(low.x, high.x), impact.y.clamp(low.y, high.y));
        // Most buildings lie past the blast; the squared test spares them
        // the square root.
        if closest.dist_sq(impact) >= WARHEAD.outer_radius * WARHEAD.outer_radius {
            return 0;
        }
        self.kind
            .warhead_taken(WARHEAD.damage_at(closest.dist(impact)))
    }

    /// The scrap one landing on `impact` takes from it, and its health after.
    fn struck(&self, impact: Vec2Fx) -> (u64, u32) {
        let dealt = self.taken(impact).min(self.hp);
        let value = self.paid * u64::from(dealt) / u64::from(self.max_hp.max(1));
        (value, self.hp - dealt)
    }
}

/// Tiles from an anchor's column a blast can still reach a footprint across:
/// the outer radius and the widest footprint, rounded out.
const BLAST_COLUMNS: i32 = 6;

/// `marks` ordered by anchor column, so a strike reads only those near it.
pub(crate) fn by_column(mut marks: Vec<Mark>) -> Vec<Mark> {
    marks.sort_unstable_by_key(|mark| (mark.anchor.x, mark.anchor.y, mark.kind));
    marks
}

/// The scrap `hits` warheads landing one after another on `tile` take from
/// `marks`, which [`by_column`] has ordered.
pub(crate) fn volley(marks: &[Mark], tile: TilePos, hits: u64) -> u64 {
    debug_assert!(marks.is_sorted_by_key(|mark| mark.anchor.x));
    let from = marks.partition_point(|mark| mark.anchor.x < tile.x - BLAST_COLUMNS);
    let to = marks.partition_point(|mark| mark.anchor.x <= tile.x + BLAST_COLUMNS);
    let marks = &marks[from..to.max(from)];
    let impact = tile.center();
    if hits == 1 {
        return marks.iter().map(|mark| mark.struck(impact).0).sum();
    }
    let mut hp: Vec<u32> = marks.iter().map(|mark| mark.hp).collect();
    let mut total = 0;
    for _ in 0..hits {
        for (mark, hp) in marks.iter().zip(&mut hp) {
            let (value, left) = Mark { hp: *hp, ..*mark }.struck(impact);
            total += value;
            *hp = left;
        }
    }
    total
}

/// A tile a warhead could strike: the known enemy scrap one landing there
/// takes, and the known hostile interceptors whose rings hold it.
#[derive(Clone, Copy)]
struct Aim {
    tile: TilePos,
    value: u64,
    interceptors: u64,
}

/// What the seat knows to strike: known hostile buildings, seen, remembered
/// or public, and a hostile start it has never seen as the Foundry there,
/// each with what the team's warheads already flying will take from it.
fn marks(observation: &ObservationData, map: &MapModel) -> Vec<Mark> {
    let mut marks: Vec<Mark> = observation
        .enemy_buildings
        .iter()
        .filter(|building| !building.provisional)
        .map(Mark::of)
        .collect();
    for owner in map.hostiles(observation.me) {
        let Some(start) = map.start(owner) else {
            continue;
        };
        let (width, height) = BuildingKind::Foundry.size();
        let unseen =
            !(0..height).any(|dy| (0..width).any(|dx| observation.explored(start.offset(dx, dy))));
        if unseen {
            let foundry = BuildingKind::Foundry.base_stats();
            marks.push(Mark {
                kind: BuildingKind::Foundry,
                anchor: start,
                hp: foundry.max_hp,
                max_hp: foundry.max_hp,
                paid: u64::from(BuildingKind::Foundry.invested_cost(0)),
            });
        }
    }
    for line in observation
        .warheads
        .iter()
        .filter(|warhead| !warhead.hostile)
        .filter_map(|warhead| warhead.line)
    {
        for mark in &mut marks {
            mark.hp -= mark.taken(line.impact).min(mark.hp);
        }
    }
    marks.retain(|mark| mark.hp > 0);
    by_column(marks)
}

/// Rounds a known hostile battery could spend now: what it shows in sight,
/// a full magazine for a remembered one, none while unbuilt.
fn rounds(observation: &ObservationData, battery: &BuildingObs) -> u64 {
    let Some(stats) = battery.kind.magazine() else {
        return 0;
    };
    if !battery.built {
        return 0;
    }
    if battery.seen {
        return observation
            .magazines
            .iter()
            .find(|magazine| magazine.building == battery.id)
            .map_or(0, |magazine| u64::from(magazine.loaded));
    }
    u64::from(stats.capacity)
}

/// Every footprint tile of a known hostile building, with what a warhead
/// there would take and the interceptors over it, in the seat's frame order
/// from home.
fn aims(observation: &ObservationData, marks: &[Mark], frame: HomeFrame) -> Vec<Aim> {
    let batteries: Vec<(Vec2Fx, Fx, u64)> = observation
        .enemy_buildings
        .iter()
        .filter_map(|building| {
            let ring = building.kind.battery()?.ring;
            let (width, height) = building.kind.size();
            let centre = building.anchor.center()
                + Vec2Fx::new(Fx::from_num(width - 1) / 2, Fx::from_num(height - 1) / 2);
            Some((centre, ring * ring, rounds(observation, building)))
        })
        .collect();
    let mut tiles: Vec<TilePos> = marks
        .iter()
        .flat_map(|mark| {
            let (width, height) = mark.kind.size();
            (0..height).flat_map(move |dy| (0..width).map(move |dx| mark.anchor.offset(dx, dy)))
        })
        .collect();
    tiles.sort_unstable_by_key(|tile| frame.rank(frame.home, crate::frame::doubled(*tile)));
    tiles.dedup();
    tiles
        .into_iter()
        .map(|tile| {
            let impact = tile.center();
            let interceptors = batteries
                .iter()
                .filter(|(centre, ring_sq, _)| centre.dist_sq(impact) <= *ring_sq)
                .map(|(_, _, rounds)| rounds)
                .sum();
            Aim {
                tile,
                value: volley(marks, tile, 1),
                interceptors,
            }
        })
        .collect()
}

/// What `rounds` warheads cost.
fn price(rounds: u64) -> u64 {
    rounds
        * u64::from(
            BuildingKind::Spire
                .magazine()
                .map_or(0, |stats| stats.round_cost),
        )
}

/// The fewest Spires that could land a warhead worth every round spent
/// somewhere the seat knows, past the interceptors over it, with that aim's
/// worth; `None` while no strike is worth its rounds.
fn needed(aims: &[Aim]) -> Option<(u64, u64)> {
    aims.iter()
        .filter(|aim| aim.value >= price(aim.interceptors + 1))
        .map(|aim| (aim.interceptors + 1, aim.value))
        .min_by_key(|(spires, value)| (*spires, std::cmp::Reverse(*value)))
}

/// Spires, rounds for empty Spires, and another Spire when known
/// interceptors over every worthy aim match the Spires the seat has. Each
/// needs a standing Crucible and a strike worth its round. Siege weighs them
/// and income scales them, as it does the Crucible.
pub(crate) fn investments(
    observation: &ObservationData,
    map: &MapModel,
    traits: PersonalityTraits,
    income: u32,
) -> Vec<(Investment, u32)> {
    let crucible = observation
        .my_buildings
        .iter()
        .any(|building| building.kind == BuildingKind::Crucible && building.built);
    let Some(frame) = HomeFrame::of(observation, map) else {
        return Vec::new();
    };
    if !crucible {
        return Vec::new();
    }
    let aims = aims(observation, &marks(observation, map), frame);
    let Some((need, worth)) = needed(&aims) else {
        return Vec::new();
    };
    let base = crate::investments::weighed(traits.siege, income);
    let spires: Vec<&BuildingObs> = observation
        .my_buildings
        .iter()
        .filter(|building| building.kind == BuildingKind::Spire)
        .collect();
    let stocked = |spire: &BuildingObs| {
        observation
            .magazines
            .iter()
            .find(|magazine| magazine.building == spire.id)
            .is_some_and(|magazine| {
                magazine.loaded > 0 || magazine.detail.is_some_and(|detail| detail.ordered > 0)
            })
    };
    // A richer known strike raises every step toward it, the Spire itself
    // included, as it raises a round, by the round's price.
    let bar = price(1).max(1);
    let score = crate::investments::capped(u64::from(base), worth, bar);
    let scored = u32::try_from(score).unwrap_or(u32::MAX);
    let mut list = Vec::new();
    if spires.is_empty() {
        list.push((Investment::Tech(BuildingKind::Spire), scored));
        return list;
    }
    let busy = spires.iter().all(|spire| !spire.built || stocked(spire));
    if busy && (spires.len() as u64) < need {
        list.push((Investment::Capacity(BuildingKind::Spire), scored));
    }
    let stocked_count = spires.iter().filter(|spire| stocked(spire)).count() as u64;
    // An empty Spire is worth a round once, with the others stocked, the
    // seat could land one where it is worth it.
    if stocked_count + 1 >= need {
        list.extend(
            spires
                .iter()
                .filter(|spire| spire.built && !stocked(spire))
                .map(|spire| (Investment::Round(spire.id), scored)),
        );
    }
    list
}

/// The next purchase toward a round for `spire`: ordering it, while the
/// Spire stands empty.
pub(crate) fn round_step(observation: &ObservationData, spire: BuildingId) -> Option<(Step, u32)> {
    let building = observation
        .my_buildings
        .iter()
        .find(|building| building.id == spire && building.built)?;
    let stats = building.kind.magazine()?;
    let magazine = observation
        .magazines
        .iter()
        .find(|magazine| magazine.building == spire)?;
    let detail = magazine.detail?;
    (u16::from(magazine.loaded) + u16::from(detail.ordered) < u16::from(stats.capacity))
        .then_some((Step::Round(spire), stats.round_cost))
}

/// Launches loaded Spires with no strike pending at an aim more of them can
/// reach past its known interceptors than those can stop, when the warheads
/// landing take at least the price of every round spent. Upper rungs take
/// the volley that takes the most beyond its price; lower ones the worthy
/// aim nearest home. The Spires nearest the aim fire.
pub(crate) fn fire(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    difficulty: BotDifficulty,
    ledger: &mut Ledger,
) {
    let mut loaded: Vec<&BuildingObs> = observation
        .my_buildings
        .iter()
        .filter(|building| {
            building.kind == BuildingKind::Spire
                && building.built
                && observation.magazines.iter().any(|magazine| {
                    magazine.building == building.id
                        && magazine.loaded > 0
                        && magazine
                            .detail
                            .is_some_and(|detail| detail.target.is_none())
                })
        })
        .collect();
    if loaded.is_empty() {
        return;
    }
    let marks = marks(observation, map);
    let ready = loaded.len() as u64;
    let careful = matches!(difficulty, BotDifficulty::Veteran | BotDifficulty::Prime);
    // Each worthy volley as (rounds spent, aim, scrap taken beyond their
    // price), in the aims' order nearest home first. A volley spends the
    // fewest Spires that get its hits past the interceptors.
    let mut volleys = aims(observation, &marks, frame)
        .into_iter()
        .flat_map(|aim| {
            let marks = &marks;
            (aim.interceptors + 1..=ready).filter_map(move |spent| {
                let taken = volley(marks, aim.tile, spent - aim.interceptors);
                (taken >= price(spent)).then(|| (spent, aim.tile, taken - price(spent)))
            })
        });
    let best = if careful {
        volleys.fold(None, |best: Option<(u64, TilePos, u64)>, volley| {
            if best.is_none_or(|(_, _, net)| volley.2 > net) {
                Some(volley)
            } else {
                best
            }
        })
    } else {
        volleys.next()
    };
    let Some((spent, tile, _)) = best else {
        return;
    };
    if u64::from(ledger.room()) < spent {
        return;
    }
    let aim = crate::frame::doubled(tile);
    loaded.sort_by_key(|spire| frame.rank(aim, footprint_centre(spire.kind, spire.anchor)));
    for spire in loaded
        .into_iter()
        .take(usize::try_from(spent).unwrap_or(usize::MAX))
    {
        ledger.order(Command::Launch {
            building: spire.id,
            target: tile,
        });
    }
}

#[cfg(test)]
mod tests;
