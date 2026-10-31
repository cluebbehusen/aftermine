//! Strategic warheads: what a strike on known enemy holdings would cost
//! their owners to recover, the Spires and rounds that worth calls for, and
//! the pass that launches loaded Spires. Every estimate uses the simulation's
//! own falloff and resistance; nothing here persists between decisions.

use crate::decision::Ledger;
use crate::frame::{HomeFrame, footprint_centre};
use crate::investments::{Investment, Step};
use crate::map::MapModel;
use crate::profile::PersonalityTraits;
use aftermine_sim::geometry::{footprint_center, foundry_supports};
use aftermine_sim::observation::{BuildingObs, ObservationData, UnitObs};
use aftermine_sim::scenario::BotDifficulty;
use aftermine_sim::stats::{self, WARHEAD};
use aftermine_sim::{BuildingId, BuildingKind, Command, PlayerId, UnitId, UnitKind};
use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::TilePos;
use std::cmp::Reverse;

/// A building a warhead could damage, as the seat knows it.
#[derive(Clone, Copy)]
pub(crate) struct Mark {
    kind: BuildingKind,
    anchor: TilePos,
    owner: PlayerId,
    tier: u8,
    built: bool,
    /// Health left once warheads already flying land.
    hp: u32,
    max_hp: u32,
    /// Scrap paid for it, upgrades included.
    paid: u64,
    /// Scrap a weld bills against.
    basis: u64,
    /// Rounds it could spend now (see [`rounds`]).
    rounds: u64,
    /// Scrap losing it costs its owner beyond the rebuild (see [`recovery`]
    /// and [`stakes`]).
    extra: u64,
    /// Its owner's only known Foundry: losing it ends the seat.
    last: bool,
}

impl Mark {
    pub(crate) fn of(building: &BuildingObs) -> Self {
        Mark {
            kind: building.kind,
            anchor: building.anchor,
            owner: building.player,
            tier: building.tier,
            built: building.built,
            hp: building.hp,
            max_hp: building.kind.tier_stats(building.tier).max_hp,
            paid: u64::from(building.kind.invested_cost(building.tier)),
            basis: u64::from(building.kind.repair_basis(building.tier)),
            rounds: 0,
            extra: 0,
            last: false,
        }
    }

    /// [`of`](Self::of), with the rounds the seat knows it holds.
    fn known(observation: &ObservationData, building: &BuildingObs) -> Self {
        Mark {
            rounds: rounds(observation, building),
            ..Mark::of(building)
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

    /// What its owner must find to recover once its health falls to `hp`.
    /// A site's missing health is work not yet done rather than damage
    /// owed, so a site counts only the health a strike takes.
    fn lost(&self, hp: u32) -> u64 {
        let unbuilt = if self.built {
            0
        } else {
            self.max_hp.saturating_sub(self.hp)
        };
        let after = if hp == 0 { 0 } else { hp + unbuilt };
        let bill = |hp| repair(self.basis, self.max_hp, hp);
        loss(self.paid + self.extra, bill, self.hp + unbuilt, after)
    }
}

/// What recovering from health falling from `before` to `after` costs, for
/// something whose fall costs `fallen` and whose repair from a health costs
/// `bill`: the fall less the repair already owed when it falls, the added
/// repair bill when it survives.
fn loss(fallen: u64, bill: impl Fn(u32) -> u64, before: u32, after: u32) -> u64 {
    if after == 0 {
        fallen.saturating_sub(bill(before))
    } else {
        bill(after) - bill(before)
    }
}

/// What welding back up to `max` from `hp` bills, at `basis` for the whole.
fn repair(basis: u64, max: u32, hp: u32) -> u64 {
    basis * u64::from(max.saturating_sub(hp)) * stats::REPAIR_COST_PERMILLE
        / (1000 * u64::from(max.max(1)))
}

/// An enemy unit seen standing still, which a warhead aimed now would find
/// where it stands.
#[derive(Clone, Copy)]
struct Body {
    kind: UnitKind,
    owner: PlayerId,
    tile: TilePos,
    /// Health left once warheads already flying land.
    hp: u32,
}

impl Body {
    /// Damage one warhead landing on `impact` deals it, as the simulation
    /// measures: a wide hull from its edge.
    fn taken(&self, impact: Vec2Fx) -> u32 {
        let edge = self.tile.center().dist(impact) - stats::hull_excess(self.kind.stats().radius);
        WARHEAD.damage_at(edge.max(Fx::ZERO))
    }

    /// What its owner must find to recover once its health falls to `hp`.
    fn lost(&self, hp: u32) -> u64 {
        let stats = self.kind.stats();
        let cost = u64::from(stats.cost);
        loss(cost, |hp| repair(cost, stats.max_hp, hp), self.hp, hp)
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

/// What warheads landing one after another on `tile` cost their owners to
/// recover from, `marks` and `bodies` in [`by_column`] order: entry `k`
/// after `k + 1` of them, for `hits` in all.
fn recovered(marks: &[Mark], bodies: &[Body], tile: TilePos, hits: usize) -> Vec<u64> {
    debug_assert!(marks.is_sorted_by_key(|mark| mark.anchor.x));
    debug_assert!(bodies.is_sorted_by_key(|body| body.tile.x));
    let impact = tile.center();
    let mut totals = vec![0; hits];
    let from = marks.partition_point(|mark| mark.anchor.x < tile.x - BLAST_COLUMNS);
    let to = marks.partition_point(|mark| mark.anchor.x <= tile.x + BLAST_COLUMNS);
    let marks = &marks[from..to.max(from)];
    // A seat's last known Foundry falling costs its owner everything it
    // holds, already in that Foundry's loss, so nothing else of that owner
    // counts from the warhead that fells it on.
    let ended: Vec<(PlayerId, usize)> = marks
        .iter()
        .filter(|mark| mark.last)
        .filter_map(|mark| {
            let taken = mark.taken(impact);
            let felled = usize::try_from(mark.hp.div_ceil(taken.max(1))).ok()?;
            (taken > 0 && felled <= hits).then_some((mark.owner, felled - 1))
        })
        .collect();
    let until = |owner: PlayerId| {
        ended
            .iter()
            .find(|(ender, _)| *ender == owner)
            .map_or(hits, |(_, at)| *at)
    };
    for mark in marks {
        let counted = if mark.last { hits } else { until(mark.owner) };
        let totals = &mut totals[..counted];
        fall(totals, mark.hp, mark.taken(impact), |hp| mark.lost(hp));
    }
    // A hull is struck from its edge, so the blast reaches the widest one
    // that much further.
    let reach = (WARHEAD.outer_radius + stats::MAX_HULL_EXCESS)
        .ceil()
        .to_num::<i32>();
    let from = bodies.partition_point(|body| body.tile.x < tile.x - reach);
    let to = bodies.partition_point(|body| body.tile.x <= tile.x + reach);
    for body in &bodies[from..to.max(from)] {
        let totals = &mut totals[..until(body.owner)];
        fall(totals, body.hp, body.taken(impact), |hp| body.lost(hp));
    }
    totals
}

/// Adds to `totals` what something at `hp` costs to recover from after each
/// of the warheads in turn deals it `taken`.
fn fall(totals: &mut [u64], hp: u32, taken: u32, lost: impl Fn(u32) -> u64) {
    if taken == 0 || hp == 0 {
        return;
    }
    let mut left = hp;
    for total in totals {
        left -= taken.min(left);
        *total += lost(left);
    }
}

/// A tile a warhead could strike: the known hostile interceptors whose rings
/// hold it, and what those interceptors cost their owner to replace once a
/// volley draws them.
#[derive(Clone, Copy)]
struct Aim {
    tile: TilePos,
    interceptors: u64,
    drawn: u64,
}

/// What the seat knows to strike: known hostile buildings, seen, remembered
/// or public, and a hostile start it has never seen as the Foundry there,
/// each with what the team's warheads already flying will take from it and
/// what losing it costs its owner beyond the rebuild.
fn marks(observation: &ObservationData, map: &MapModel) -> Vec<Mark> {
    let mut marks: Vec<Mark> = observation
        .enemy_buildings
        .iter()
        .filter(|building| !building.provisional)
        .map(|building| Mark::known(observation, building))
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
                owner,
                tier: 0,
                built: true,
                hp: foundry.max_hp,
                max_hp: foundry.max_hp,
                paid: u64::from(BuildingKind::Foundry.invested_cost(0)),
                basis: u64::from(BuildingKind::Foundry.repair_basis(0)),
                rounds: 0,
                extra: 0,
                last: false,
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
    recovery(&mut marks, &observation.enemy_units);
    by_column(marks)
}

/// Sets what losing each of `marks` costs its owner beyond the rebuild: the
/// income it stops until it stands again, and the rounds lost with it. An
/// owner's only known Foundry carries instead the rest of what its seat is
/// known to hold, `units` included, since a seat with no Foundry is out.
fn recovery(marks: &mut [Mark], units: &[UnitObs]) {
    // Each owner's known Foundries, and the scrap it is known to hold.
    let mut seats: Vec<(PlayerId, usize, u64)> = Vec::new();
    let held = marks
        .iter()
        .map(|mark| (mark.owner, mark.kind == BuildingKind::Foundry, mark.paid))
        .chain(
            units
                .iter()
                .map(|unit| (unit.player, false, u64::from(unit.kind.stats().cost))),
        );
    for (owner, foundry, worth) in held {
        match seats.iter_mut().find(|seat| seat.0 == owner) {
            Some(seat) => {
                seat.1 += usize::from(foundry);
                seat.2 += worth;
            }
            None => seats.push((owner, usize::from(foundry), worth)),
        }
    }
    let supporters = |owner: PlayerId, extractor: TilePos| {
        marks
            .iter()
            .filter(|mark| {
                mark.kind == BuildingKind::Foundry
                    && mark.built
                    && mark.owner == owner
                    && foundry_supports(mark.anchor, extractor)
            })
            .count()
    };
    let extras: Vec<(u64, bool)> = marks
        .iter()
        .map(|mark| {
            let (_, foundries, worth) = seats
                .iter()
                .find(|seat| seat.0 == mark.owner)
                .copied()
                .expect("every mark's owner is counted");
            if mark.kind == BuildingKind::Foundry && foundries == 1 {
                return (worth - mark.paid, true);
            }
            let down = rebuild_ticks(mark.kind, mark.tier);
            let extra = match mark.kind {
                BuildingKind::Extractor => {
                    yields(mark.tier, supporters(mark.owner, mark.anchor) > 0, down)
                }
                BuildingKind::Reclaimer if mark.tier > 0 => down / stats::REFINERY_PERIOD,
                BuildingKind::Reclaimer => down / stats::RECLAIMER_PERIOD,
                BuildingKind::Foundry => {
                    // Extractors only it supports fall to remote yield until
                    // it stands again.
                    let unsupported: u64 = marks
                        .iter()
                        .filter(|other| {
                            mark.built
                                && other.kind == BuildingKind::Extractor
                                && other.owner == mark.owner
                                && foundry_supports(mark.anchor, other.anchor)
                                && supporters(other.owner, other.anchor) == 1
                        })
                        .map(|other| {
                            yields(other.tier, true, down) - yields(other.tier, false, down)
                        })
                        .sum();
                    unsupported + down / stats::FOUNDRY_DRIP_PERIOD
                }
                BuildingKind::Spire => mark.rounds * price(1),
                _ => 0,
            };
            (extra, false)
        })
        .collect();
    for (mark, (extra, last)) in marks.iter_mut().zip(extras) {
        mark.extra = extra;
        mark.last = last;
    }
}

/// Scrap an Extractor at `tier` earns over `ticks`, `supported` or remote.
fn yields(tier: u8, supported: bool, ticks: u64) -> u64 {
    let (amount, period) = match (tier > 0, supported) {
        (false, false) => stats::EXTRACTOR_REMOTE_YIELD,
        (false, true) => stats::EXTRACTOR_SUPPORTED_YIELD,
        (true, false) => stats::DEEP_BORE_REMOTE_YIELD,
        (true, true) => stats::DEEP_BORE_SUPPORTED_YIELD,
    };
    u64::from(amount) * ticks / period
}

/// Ticks raising `kind` again up to `tier` takes.
fn rebuild_ticks(kind: BuildingKind, tier: u8) -> u64 {
    kind.tiers()
        .iter()
        .take(usize::from(tier) + 1)
        .filter_map(|stats| stats.construction.as_ref())
        .map(|construction| u64::from(construction.build_ticks))
        .sum()
}

/// Rounds a known battery or Spire could spend now: what it shows in sight,
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

/// Every footprint tile of a known hostile building, with the interceptors
/// over it, in the seat's frame order from home. Units standing in the open
/// are no aim of their own: they walk off before a warhead lands.
fn aims(observation: &ObservationData, marks: &[Mark], frame: HomeFrame) -> Vec<Aim> {
    let batteries: Vec<(Vec2Fx, Fx, u64, u64)> = observation
        .enemy_buildings
        .iter()
        .filter_map(|building| {
            let ring = building.kind.battery()?.ring;
            let round_cost = building.kind.magazine()?.round_cost;
            let centre = footprint_center(building.anchor, building.kind.size());
            let rounds = rounds(observation, building);
            Some((centre, ring * ring, rounds, rounds * u64::from(round_cost)))
        })
        .collect();
    let mut tiles: Vec<TilePos> = marks
        .iter()
        .flat_map(|mark| crate::frame::tiles_of(mark.anchor, mark.kind.size()))
        .collect();
    tiles.sort_unstable_by_key(|tile| frame.rank(frame.home, crate::frame::doubled(*tile)));
    tiles.dedup();
    tiles
        .into_iter()
        .map(|tile| {
            let impact = tile.center();
            let (interceptors, drawn) = batteries
                .iter()
                .filter(|(centre, ring_sq, _, _)| centre.dist_sq(impact) <= *ring_sq)
                .fold((0, 0), |(rounds, scrap), (_, _, more, worth)| {
                    (rounds + more, scrap + worth)
                });
            Aim {
                tile,
                interceptors,
                drawn,
            }
        })
        .collect()
}

/// What else rides on known enemy Lattices and Spires. Each interceptor a
/// Lattice holds costs a strike under it another round, so while a warhead
/// in its ring would be worth its round without it, felling it is worth a
/// full magazine of rounds more. A Spire is worth what one of its warheads
/// could cost the seat where the seat lies most exposed.
fn stakes(observation: &ObservationData, marks: &mut [Mark], aims: &[Aim], bodies: &[Body]) {
    let covers: Vec<(usize, u64)> = marks
        .iter()
        .enumerate()
        .filter(|(_, mark)| mark.built)
        .filter_map(|(index, mark)| {
            let battery = mark.kind.battery()?;
            let centre = footprint_center(mark.anchor, mark.kind.size());
            let without: Vec<Mark> = marks
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != index)
                .map(|(_, other)| *other)
                .collect();
            let drawn = mark.rounds * u64::from(battery.magazine.round_cost);
            let guards = aims
                .iter()
                .filter(|aim| centre.dist_sq(aim.tile.center()) <= battery.ring * battery.ring)
                .any(|aim| {
                    let interceptors = aim.interceptors.saturating_sub(mark.rounds);
                    recovered(&without, bodies, aim.tile, 1)[0] + aim.drawn.saturating_sub(drawn)
                        >= price(interceptors + 1)
                });
            guards.then(|| (index, u64::from(battery.magazine.capacity) * price(1)))
        })
        .collect();
    for (index, cover) in covers {
        marks[index].extra += cover;
    }
    let spire = |mark: &Mark| mark.kind == BuildingKind::Spire && mark.built;
    if marks.iter().any(spire) {
        let threat = exposure(observation);
        for mark in marks.iter_mut().filter(|mark| spire(mark)) {
            mark.extra += threat;
        }
    }
}

/// The most one warhead could cost the seat, landing on its own buildings
/// anywhere outside the ring of every own battery holding a round.
fn exposure(observation: &ObservationData) -> u64 {
    let mut own: Vec<Mark> = observation
        .my_buildings
        .iter()
        .filter(|building| !building.provisional)
        .map(|building| Mark::known(observation, building))
        .collect();
    recovery(&mut own, &observation.my_units);
    let own = by_column(own);
    let guards: Vec<(Vec2Fx, Fx)> = own
        .iter()
        .filter(|mark| mark.rounds > 0)
        .filter_map(|mark| {
            let ring = mark.kind.battery()?.ring;
            Some((footprint_center(mark.anchor, mark.kind.size()), ring * ring))
        })
        .collect();
    own.iter()
        .flat_map(|mark| crate::frame::tiles_of(mark.anchor, mark.kind.size()))
        .filter(|tile| {
            !guards
                .iter()
                .any(|(centre, ring_sq)| centre.dist_sq(tile.center()) <= *ring_sq)
        })
        .map(|tile| recovered(&own, &[], tile, 1)[0])
        .max()
        .unwrap_or(0)
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

/// The most warheads any building needs to fall to direct hits, which
/// bounds the volleys a known strike could call for.
fn most_hits() -> usize {
    BuildingKind::ALL
        .iter()
        .flat_map(|kind| {
            let taken = kind.warhead_taken(WARHEAD.peak_damage).max(1);
            kind.tiers()
                .iter()
                .map(move |stats| stats.max_hp.div_ceil(taken))
        })
        .max()
        .map_or(1, |hits| usize::try_from(hits).unwrap_or(usize::MAX))
}

/// The fewest Spires that could land a volley somewhere the seat knows, past
/// the interceptors over it, where what the volley costs its owners to
/// recover and the interceptors it draws pay for every round spent and every
/// Spire the seat would still lack, `have` standing or rising; with what
/// that volley costs them. `None` while no volley is worth it.
fn needed(aims: &[Aim], marks: &[Mark], have: u64) -> Option<(u64, u64)> {
    let spire = u64::from(BuildingKind::Spire.invested_cost(0));
    let most = most_hits();
    aims.iter()
        .flat_map(|aim| {
            recovered(marks, &[], aim.tile, most)
                .into_iter()
                .zip(aim.interceptors + 1..)
                .filter(move |(value, spent)| {
                    value + aim.drawn >= price(*spent) + spire * spent.saturating_sub(have)
                })
                .map(|(value, spent)| (spent, value))
        })
        .min_by_key(|(spires, value)| (*spires, Reverse(*value)))
}

/// Spires, a round for every empty Spire, and another Spire when those the
/// seat has are all stocked or rising and fewer than a known strike needs.
/// Each needs a standing Crucible and a strike worth its rounds and the
/// Spires still lacking. Units move on, so only buildings justify buying
/// toward a strike. Siege weighs them and income scales them, as it does the
/// Crucible.
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
    let spires: Vec<&BuildingObs> = observation
        .my_buildings
        .iter()
        .filter(|building| building.kind == BuildingKind::Spire)
        .collect();
    let mut marks = marks(observation, map);
    let aims = aims(observation, &marks, frame);
    stakes(observation, &mut marks, &aims, &[]);
    let Some((need, worth)) = needed(&aims, &marks, spires.len() as u64) else {
        return Vec::new();
    };
    let base = crate::investments::weighed(traits.siege, income);
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
    // A volley spends its rounds at once, so every empty Spire loads toward
    // it, however many more it still needs.
    list.extend(
        spires
            .iter()
            .filter(|spire| spire.built && !stocked(spire))
            .map(|spire| (Investment::Round(spire.id), scored)),
    );
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

/// The enemy units in sight that `still` names, standing where the seat
/// last saw them, in [`by_column`] order: a warhead aimed now finds only
/// those where they stand.
fn bodies(observation: &ObservationData, still: &[UnitId]) -> Vec<Body> {
    debug_assert!(still.is_sorted());
    let mut bodies: Vec<Body> = observation
        .enemy_units
        .iter()
        .filter(|unit| still.binary_search(&unit.id).is_ok())
        .map(|unit| Body {
            kind: unit.kind,
            owner: unit.player,
            tile: unit.tile,
            hp: unit.hp,
        })
        .collect();
    for line in observation
        .warheads
        .iter()
        .filter(|warhead| !warhead.hostile)
        .filter_map(|warhead| warhead.line)
    {
        for body in &mut bodies {
            body.hp -= body.taken(line.impact).min(body.hp);
        }
    }
    bodies.retain(|body| body.hp > 0);
    bodies.sort_by_key(|body| (body.tile.x, body.tile.y));
    bodies
}

/// Launches loaded Spires with no strike pending at an aim more of them can
/// reach past its known interceptors than those can stop, when what the
/// warheads landing cost their owners to recover, the enemy units `still`
/// names included, and the interceptors drawn take at least the price of
/// every round spent. Upper rungs take the volley that takes the most beyond
/// its price; lower ones the worthy aim nearest home. The Spires nearest the
/// aim fire, as one unit order: choosing an aim is one act of attention,
/// however many Spires fire at it.
pub(crate) fn fire(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    difficulty: BotDifficulty,
    still: &[UnitId],
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
    let mut marks = marks(observation, map);
    let bodies = bodies(observation, still);
    let aims = aims(observation, &marks, frame);
    stakes(observation, &mut marks, &aims, &bodies);
    let ready = loaded.len() as u64;
    let careful = matches!(difficulty, BotDifficulty::Veteran | BotDifficulty::Prime);
    // Each worthy volley as (rounds spent, aim, scrap taken beyond their
    // price), in the aims' order nearest home first. A volley spends the
    // fewest Spires that get its hits past the interceptors, and draws every
    // one of them.
    let mut volleys = aims
        .iter()
        .filter(|aim| aim.interceptors < ready)
        .flat_map(|aim| {
            let hits = usize::try_from(ready - aim.interceptors).unwrap_or(usize::MAX);
            recovered(&marks, &bodies, aim.tile, hits)
                .into_iter()
                .zip(aim.interceptors + 1..)
                .filter_map(|(value, spent)| {
                    let taken = value + aim.drawn;
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
    let aim = crate::frame::doubled(tile);
    loaded.sort_by_key(|spire| frame.rank(aim, footprint_centre(spire.kind, spire.anchor)));
    ledger.salvo(
        loaded
            .into_iter()
            .take(usize::try_from(spent).unwrap_or(usize::MAX))
            .map(|spire| Command::Launch {
                building: spire.id,
                target: tile,
            }),
    );
}

#[cfg(test)]
mod tests;
