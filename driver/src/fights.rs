//! Fights: where armed units died together, and how each seat went into
//! them. It observes each tick's state and events, as the impact ledger
//! does, so it serves live evaluation and replays alike, and it is
//! omniscient QA evidence that never reaches a controller.
//!
//! An armed unit's death joins the fight whose last death came at most
//! `GAP_TICKS` before it and whose centre, the mean of its deaths, lies
//! within `GAP_TILES`; otherwise it starts one. A fight needs two deaths.
//! Every seat with armed units within `NEAR_TILES` of a fight's centre, from
//! `GAP_TICKS` before its first death to its last, or with a loss in it,
//! took part. For each, the fight records how strong its team stood against
//! the others near the centre when they first met there, when each of its
//! units arrived, which of its losses died with no armed ally within
//! `ALONE_TILES`, and the price of the units each side lost.

use aftermine_sim::stats::UnitKind;
use aftermine_sim::{Event, PlayerId, State, TickReport, UnitId};
use chassis::grid::TilePos;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as _;

/// Ticks between position samples: half a second.
const SAMPLE_TICKS: u64 = 10;

/// Ticks a death may follow a fight's last and still join it.
const GAP_TICKS: u64 = 200;

/// Tiles from a fight's centre a death may lie and still join it.
const GAP_TILES: i32 = 15;

/// Tiles from a fight's centre inside which a unit is in the fight.
const NEAR_TILES: i32 = 10;

/// Tiles inside which an armed ally keeps a dying unit from dying alone.
const ALONE_TILES: i32 = 6;

/// Ticks after its side's first arrival at which a unit arrives late: ten
/// seconds.
const LATE_TICKS: u64 = 200;

/// What one seat did over a match's fights. Counts only, so seat-legs add.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatFights {
    /// Fights the seat took part in.
    pub fights: u32,
    /// Of those, fights where its team and another met near the centre at a
    /// sample.
    pub met: u32,
    /// Of those, fights where its team first stood at under half the others'
    /// worth.
    pub met_short: u32,
    /// Armed units it lost in fights.
    pub losses: u32,
    /// Of those, losses with no armed ally within `ALONE_TILES`.
    pub alone: u32,
    /// Its armed units that came into fights.
    pub arrivals: u32,
    /// Of those, units that came `LATE_TICKS` or more after its first in
    /// the same fight.
    pub late: u32,
    /// Price of its armed units lost in fights.
    pub lost: u64,
    /// Price of other teams' armed units lost in its fights.
    pub killed: u64,
}

impl SeatFights {
    fn add(&mut self, other: &SeatFights) {
        self.fights += other.fights;
        self.met += other.met;
        self.met_short += other.met_short;
        self.losses += other.losses;
        self.alone += other.alone;
        self.arrivals += other.arrivals;
        self.late += other.late;
        self.lost += other.lost;
        self.killed += other.killed;
    }
}

/// An armed unit at a sample.
#[derive(Debug, Clone, Copy)]
struct Armed {
    id: UnitId,
    seat: u8,
    team: u8,
    tile: TilePos,
    /// Its price scaled by its health.
    worth: u64,
}

#[derive(Debug)]
struct Sample {
    tick: u64,
    units: Vec<Armed>,
}

#[derive(Debug)]
struct Death {
    seat: u8,
    team: u8,
    tile: TilePos,
    price: u64,
    alone: bool,
}

/// A fight still taking deaths.
#[derive(Debug)]
struct Open {
    end: u64,
    deaths: Vec<Death>,
    /// Sum of the deaths' tiles, for the centre.
    sum: (i64, i64),
    /// Each unit that came near, with its seat and the sample it came at.
    arrivals: BTreeMap<UnitId, (u8, u64)>,
    /// Each seat's team worth against the others' at the first sample both
    /// stood near.
    met: BTreeMap<u8, (u64, u64)>,
}

impl Open {
    fn centre(&self) -> TilePos {
        let count = i64::try_from(self.deaths.len().max(1)).expect("death counts fit in i64");
        TilePos::new(
            i32::try_from(self.sum.0.div_euclid(count)).expect("tiles fit in i32"),
            i32::try_from(self.sum.1.div_euclid(count)).expect("tiles fit in i32"),
        )
    }

    fn take(&mut self, death: Death) {
        self.sum.0 += i64::from(death.tile.x);
        self.sum.1 += i64::from(death.tile.y);
        self.deaths.push(death);
    }

    /// Notes who stood near the centre at `sample`.
    fn sample(&mut self, sample: &Sample) {
        let centre = self.centre();
        let near: Vec<&Armed> = sample
            .units
            .iter()
            .filter(|unit| unit.tile.chebyshev(centre) <= NEAR_TILES)
            .collect();
        for unit in &near {
            self.arrivals
                .entry(unit.id)
                .or_insert((unit.seat, sample.tick));
        }
        for unit in &near {
            if self.met.contains_key(&unit.seat) {
                continue;
            }
            let (own, others) = near.iter().fold((0, 0), |(own, others), other| {
                if other.team == unit.team {
                    (own + other.worth, others)
                } else {
                    (own, others + other.worth)
                }
            });
            if others > 0 {
                self.met.insert(unit.seat, (own, others));
            }
        }
    }
}

/// Follows a match's fights tick by tick.
#[derive(Debug)]
pub struct FightTracker {
    teams: Vec<u8>,
    history: VecDeque<Sample>,
    open: Vec<Open>,
    seats: Vec<SeatFights>,
}

/// Whether `kind` counts in fights: armed, and not a drone that comes and
/// goes from its host's bay.
fn armed(kind: UnitKind) -> bool {
    let stats = kind.stats();
    stats.can_fight() && !stats.docks()
}

impl FightTracker {
    /// A tracker for the match `state` begins.
    #[must_use]
    pub fn new(state: &State) -> Self {
        Self {
            teams: state.players().iter().map(|player| player.team).collect(),
            history: VecDeque::new(),
            open: Vec::new(),
            seats: vec![SeatFights::default(); state.players().len()],
        }
    }

    fn team(&self, player: PlayerId) -> u8 {
        self.teams[usize::from(player.0)]
    }

    /// Takes in the tick that produced `state` and `report`.
    pub fn observe(&mut self, state: &State, report: &TickReport) {
        let tick = state.current_tick();
        let mut deaths = Vec::new();
        for event in &report.events {
            let Event::UnitDied {
                unit,
                kind,
                player,
                pos,
                ..
            } = event
            else {
                continue;
            };
            if !armed(*kind) {
                continue;
            }
            let tile = TilePos::containing(*pos);
            let team = self.team(*player);
            let alone = !state.units().iter().any(|other| {
                other.id != *unit
                    && armed(other.kind)
                    && self.team(other.player) == team
                    && other.tile().chebyshev(tile) <= ALONE_TILES
            });
            deaths.push(Death {
                seat: player.0,
                team,
                tile,
                price: u64::from(kind.stats().cost),
                alone,
            });
        }
        let units = tick.is_multiple_of(SAMPLE_TICKS).then(|| {
            state
                .units()
                .iter()
                .filter(|unit| armed(unit.kind))
                .map(|unit| {
                    let stats = unit.kind.stats();
                    Armed {
                        id: unit.id,
                        seat: unit.player.0,
                        team: self.team(unit.player),
                        tile: unit.tile(),
                        worth: u64::from(stats.cost) * u64::from(unit.hp)
                            / u64::from(stats.max_hp.max(1)),
                    }
                })
                .collect()
        });
        self.step(tick, deaths, units);
    }

    /// Takes in the deaths of tick `tick` and, on a sample tick, the armed
    /// units standing after it.
    fn step(&mut self, tick: u64, deaths: Vec<Death>, units: Option<Vec<Armed>>) {
        for death in deaths {
            self.die(tick, death);
        }
        let (closed, open): (Vec<Open>, Vec<Open>) = std::mem::take(&mut self.open)
            .into_iter()
            .partition(|fight| tick > fight.end + GAP_TICKS);
        self.open = open;
        for fight in &closed {
            self.close(fight);
        }
        if let Some(units) = units {
            let sample = Sample { tick, units };
            for fight in &mut self.open {
                fight.sample(&sample);
            }
            self.history.push_back(sample);
            while self
                .history
                .front()
                .is_some_and(|oldest| oldest.tick + GAP_TICKS < tick)
            {
                self.history.pop_front();
            }
        }
    }

    /// Adds `death` at `tick` to the fight it joins, or starts one, which
    /// looks back over the samples kept to see who was already near.
    fn die(&mut self, tick: u64, death: Death) {
        if let Some(fight) = self.open.iter_mut().find(|fight| {
            tick <= fight.end + GAP_TICKS && fight.centre().chebyshev(death.tile) <= GAP_TILES
        }) {
            fight.end = tick;
            fight.take(death);
            return;
        }
        let mut fight = Open {
            end: tick,
            deaths: Vec::new(),
            sum: (0, 0),
            arrivals: BTreeMap::new(),
            met: BTreeMap::new(),
        };
        fight.take(death);
        for sample in &self.history {
            fight.sample(sample);
        }
        self.open.push(fight);
    }

    fn close(&mut self, fight: &Open) {
        if fight.deaths.len() < 2 {
            return;
        }
        let mut seats: Vec<u8> = fight
            .arrivals
            .values()
            .map(|(seat, _)| *seat)
            .chain(fight.deaths.iter().map(|death| death.seat))
            .collect();
        seats.sort_unstable();
        seats.dedup();
        for seat in seats {
            let team = self.teams[usize::from(seat)];
            let record = &mut self.seats[usize::from(seat)];
            record.fights += 1;
            if let Some((own, others)) = fight.met.get(&seat) {
                record.met += 1;
                if own * 2 < *others {
                    record.met_short += 1;
                }
            }
            for death in &fight.deaths {
                if death.seat == seat {
                    record.losses += 1;
                    record.alone += u32::from(death.alone);
                    record.lost += death.price;
                } else if death.team != team {
                    record.killed += death.price;
                }
            }
            let came: Vec<u64> = fight
                .arrivals
                .values()
                .filter(|(at, _)| *at == seat)
                .map(|(_, tick)| *tick)
                .collect();
            if let Some(first) = came.iter().min() {
                record.arrivals += u32::try_from(came.len()).expect("unit counts fit in u32");
                record.late += u32::try_from(
                    came.iter()
                        .filter(|tick| **tick >= first + LATE_TICKS)
                        .count(),
                )
                .expect("unit counts fit in u32");
            }
        }
    }

    /// Each seat's fights, closing those still open.
    #[must_use]
    pub fn finish(mut self) -> Vec<SeatFights> {
        for fight in &std::mem::take(&mut self.open) {
            self.close(fight);
        }
        self.seats
    }
}

/// Seat fights pooled over seat-legs, keeping each so intervals can treat a
/// seat-leg, not a fight, as one sample: the fights of one game are not
/// independent.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct FightPool {
    /// Each seat-leg's fights.
    pub legs: Vec<SeatFights>,
}

impl FightPool {
    /// Adds one seat-leg.
    pub fn add(&mut self, fights: &SeatFights) {
        self.legs.push(*fights);
    }

    /// Every seat-leg summed.
    #[must_use]
    pub fn total(&self) -> SeatFights {
        self.legs
            .iter()
            .fold(SeatFights::default(), |mut sum, leg| {
                sum.add(leg);
                sum
            })
    }

    /// The pooled ratio of `part` over `whole` across seat-legs, with a 95%
    /// interval from the spread between seat-legs; `None` without a whole.
    #[must_use]
    pub fn ratio(
        &self,
        part: impl Fn(&SeatFights) -> u64,
        whole: impl Fn(&SeatFights) -> u64,
    ) -> Option<(f64, [f64; 2])> {
        pooled_ratio(self.legs.iter().map(|leg| (part(leg), whole(leg))))
    }

    /// One line of the pool's fight measures.
    #[must_use]
    pub fn line(&self) -> String {
        let total = self.total();
        let share = |ratio: Option<(f64, [f64; 2])>| {
            ratio.map_or_else(
                || "-".to_string(),
                |(value, [low, high])| {
                    format!(
                        "{:.0}% [{:.0}-{:.0}%]",
                        value * 100.0,
                        low.max(0.0) * 100.0,
                        high.min(1.0) * 100.0
                    )
                },
            )
        };
        let traded = self.ratio(|leg| leg.killed, |leg| leg.lost).map_or_else(
            || "-".to_string(),
            |(value, [low, high])| format!("{value:.2} [{:.2}-{high:.2}]", low.max(0.0)),
        );
        format!(
            "{} fights; lost alone {} of {}; met short {} of {}; late {} of {}; killed per lost {traded}",
            total.fights,
            share(self.ratio(|leg| u64::from(leg.alone), |leg| u64::from(leg.losses))),
            total.losses,
            share(self.ratio(|leg| u64::from(leg.met_short), |leg| u64::from(leg.met))),
            total.met,
            share(self.ratio(|leg| u64::from(leg.late), |leg| u64::from(leg.arrivals))),
            total.arrivals,
        )
    }
}

/// The ratio of the parts' sum to the wholes' sum over `pairs`, one pair a
/// sample, with a 95% interval from the spread of each sample around that
/// ratio; `None` when the wholes sum to zero. One sample gives no interval
/// width.
#[must_use]
pub fn pooled_ratio(pairs: impl Iterator<Item = (u64, u64)>) -> Option<(f64, [f64; 2])> {
    let pairs: Vec<(f64, f64)> = pairs
        .map(|(part, whole)| (part as f64, whole as f64))
        .collect();
    let whole: f64 = pairs.iter().map(|(_, whole)| whole).sum();
    if whole <= 0.0 {
        return None;
    }
    let ratio = pairs.iter().map(|(part, _)| part).sum::<f64>() / whole;
    let samples = pairs.len() as f64;
    if pairs.len() < 2 {
        return Some((ratio, [ratio, ratio]));
    }
    let spread: f64 = pairs
        .iter()
        .map(|(part, whole)| (part - ratio * whole).powi(2))
        .sum();
    let error = (spread * samples / (samples - 1.0)).sqrt() / whole;
    Some((ratio, [ratio - 1.96 * error, ratio + 1.96 * error]))
}

/// Renders each labelled pool's fight measures, one line a group.
pub fn render(out: &mut String, groups: &[(String, &FightPool)]) {
    let _ = writeln!(
        out,
        "\nfights: losses that died with no armed ally within {ALONE_TILES} tiles, fights met at under half the others' worth, units that came {} s or more after their side's first, price of units killed per price lost (95% intervals over seat-legs)",
        LATE_TICKS / u64::from(aftermine_sim::TICKS_PER_SECOND)
    );
    for (label, pool) in groups {
        let _ = writeln!(out, "  {label:<24} {}", pool.line());
    }
}

#[cfg(test)]
mod tests;
