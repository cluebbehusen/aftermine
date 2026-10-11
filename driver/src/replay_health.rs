//! `replay-health`: late-game health over recorded games. Bot evaluations
//! and human playtest recordings answer the same questions: where income
//! comes from minute by minute, how long matches run, how much wreck lies
//! on the map, how varied late armies are, and what winning a battle turns
//! into.
//!
//! Each recording is re-executed once. The [`ImpactLedger`] supplies each
//! tick's income credits and hit attribution, and `replay-summary`'s
//! clusterer finds the battles. A construction site destroyed before it
//! completed weighs nothing in a battle here, and a seat's home is the
//! ground near its production buildings. Everything is omniscient QA
//! evidence that never reaches a controller.

use crate::ledger::{ImpactLedger, IncomeSource, Place};
use crate::replay_ledger::SeatPlayer;
use crate::replay_summary::{
    BATTLE_QUIET_TICKS, BATTLE_RADIUS_TILES, BATTLE_VALUE_FLOOR, BattleClusterer, SeatLoss, Verdict,
};
use anyhow::{Context, Result, bail};
use chassis::grid::TilePos;
use oxide_kit::GameReplay;
use oxide_sim::{
    BuildingId, BuildingKind, Event, GameResult, SIM_VERSION, State, TICKS_PER_SECOND, Target,
    TickReport,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

/// Ticks per minute.
const MINUTE: u64 = TICKS_PER_SECOND as u64 * 60;

/// Minute from which a match counts as late: army variety is measured from
/// here on.
pub const LATE_MINUTE: u64 = 15;

/// [`LATE_MINUTE`] in ticks.
const LATE_TICK: u64 = LATE_MINUTE * MINUTE;

/// Minutes a match must reach to count as long.
pub const LONG_MINUTES: u64 = 30;

/// Ticks after a battle's last loss in which its winner's follow-up counts.
pub const FOLLOW_UP_TICKS: u64 = MINUTE;

/// Scrap of income a won battle must destroy to count as taking income:
/// the value a cluster of losses needs to count as a battle at all.
pub const INCOME_FLOOR: u64 = BATTLE_VALUE_FLOOR;

/// Minutes at which the pooled income bands begin.
const INCOME_BANDS: [u64; 6] = [0, 5, 10, 15, 20, 30];

/// Minutes between pooled wreck marks.
const WRECK_MARK_MINUTES: u64 = 5;

/// One recorded match's health.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MatchHealth {
    /// Where it was read from.
    pub path: PathBuf,
    /// Scenario display name.
    pub scenario: String,
    /// Tick the recording begins at.
    pub start: u64,
    /// Tick the match decided, or the recording's end when it did not:
    /// the match's measured length.
    pub end: u64,
    /// The result, absent when the recording ends undecided.
    pub result: Option<GameResult>,
    /// Seats on the winning team.
    pub winner_seats: Vec<u8>,
    /// Whether the match reached [`LONG_MINUTES`].
    pub long: bool,
    /// Minute the per-minute series begin at: `start` in whole minutes.
    pub first_minute: u64,
    /// Seats, by seat.
    pub seats: Vec<SeatHealth>,
    /// Wreck on the map at each whole minute.
    pub wrecks: Vec<WreckSample>,
    /// Every battle above `replay-summary`'s value floor, by first loss.
    pub battles: Vec<BattleHealth>,
}

/// One seat's health over a match.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeatHealth {
    /// Player seat.
    pub seat: u8,
    /// Seat name.
    pub name: String,
    /// Team.
    pub team: u8,
    /// Who played the seat.
    pub controller: SeatPlayer,
    /// Tick the seat was eliminated.
    pub eliminated_at: Option<u64>,
    /// Scrap earned by source in each minute from the match's
    /// `first_minute`; the last minute may be partial.
    pub income: BTreeMap<IncomeSource, Vec<u64>>,
    /// Whole minutes from [`LATE_MINUTE`] the seat was sampled in the
    /// match.
    pub late_minutes: u32,
    /// The seat's standing armed units, carried riders included, summed
    /// over those minutes. Absent, like `late_trained`, when the seat was
    /// out of the match by minute [`LATE_MINUTE`].
    pub late_army: Option<Composition>,
    /// The armed units the seat trained from minute [`LATE_MINUTE`].
    pub late_trained: Option<Composition>,
}

/// What a set of armed units was made of, by kind.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Composition {
    /// Scrap value of each kind.
    pub value: BTreeMap<String, u64>,
    /// Shannon entropy of the kinds' value shares, in bits.
    pub entropy_bits: f64,
    /// Two to the entropy: how many equally valued kinds would be as varied.
    /// Zero when there were no units.
    pub effective_kinds: f64,
}

/// Wreck on the map at one whole minute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WreckSample {
    /// The minute.
    pub minute: u64,
    /// Scrap of salvage left on the map.
    pub total: u64,
    /// Of that, scrap lying at each seat's home, by seat: within home reach
    /// of that seat's production buildings, the nearer seat taking a tile
    /// both reach.
    pub home: Vec<u64>,
}

/// One battle and what its winner made of it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BattleHealth {
    /// First loss tick.
    pub from_tick: u64,
    /// Last loss tick.
    pub to_tick: u64,
    /// Loss centroid tile.
    pub at: [i32; 2],
    /// Per participating seat, heaviest first.
    pub losses: Vec<SeatLoss>,
    /// The cluster is a loaded transport shot down, not a fight.
    pub shootdown: bool,
    /// The team that won it: the lightest-losing team when the heaviest
    /// two team losses differ by more than a quarter, or the only team on
    /// the other side of a one-sided battle.
    pub winner: Option<u8>,
    /// What the winner made of it.
    pub follow_up: Option<FollowUp>,
}

/// What a battle's winner did with it. Its force is every body of the
/// winning team that hit a body lost in the battle within the battle time
/// gate before that loss; destruction counts when the force landed a hit
/// within the same gate before the death.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FollowUp {
    /// Where the battle was, from the winner's view, by the production
    /// buildings standing at its first loss.
    pub place: Place,
    /// The battle was on the winner's own ground by every building it had
    /// standing then, not just production: holding the ground there proves
    /// nothing.
    pub own_ground: bool,
    /// Teams on the losing side.
    pub losers: Vec<u8>,
    /// Bodies in the winning force.
    pub force: u32,
    /// Last tick counted: [`FOLLOW_UP_TICKS`] after the last loss, or the
    /// match's end.
    pub until: u64,
    /// Scrap of the losers' completed producing buildings the force
    /// destroyed, from the first loss through `until`.
    pub production: u64,
    /// Scrap of the losers' workers, Reclaimers, Refineries and Extractors
    /// the force destroyed over the same span.
    pub income: u64,
    /// Scrap of the winning team's armed units within the battle radius of
    /// its centroid at `until`. Units riding a transport count for neither
    /// side, since they cannot fight until set down.
    pub stayed: u64,
    /// Scrap of the losers' armed units and defenses within it then.
    pub opposed: u64,
    /// The winner held the ground: `stayed` is positive and exceeds
    /// `opposed`.
    pub held: bool,
}

impl FollowUp {
    /// The winning force destroyed a producing building.
    pub fn razed_production(&self) -> bool {
        self.production > 0
    }

    /// The winning force destroyed at least [`INCOME_FLOOR`] of income.
    pub fn took_income(&self) -> bool {
        self.income >= INCOME_FLOOR
    }

    /// The winner held ground that is not its own, where its army stands
    /// anyway.
    pub fn held_ground(&self) -> bool {
        self.held && !self.own_ground
    }

    /// The win knocked the losers back: it razed production, took income or
    /// held ground.
    pub fn converted(&self) -> bool {
        self.razed_production() || self.took_income() || self.held_ground()
    }
}

/// What a destroyed body was to its owner's economy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Production,
    Income,
    Other,
}

/// A destroyed body.
#[derive(Debug, Clone)]
struct Death {
    tick: u64,
    team: u8,
    role: Role,
    value: u64,
    /// Hostile bodies, with their teams, that hit it within the battle time
    /// gate before it died.
    hitters: Vec<(Target, u8)>,
}

/// A building as the pass has known it.
#[derive(Debug, Clone, Copy)]
struct Life {
    id: BuildingId,
    team: u8,
    kind: BuildingKind,
    tile: TilePos,
    born: u64,
    completed: bool,
    gone: Option<u64>,
}

impl Life {
    fn standing(&self, tick: u64) -> bool {
        self.born <= tick && self.gone.is_none_or(|gone| gone >= tick)
    }
}

/// A seat's armed units from [`LATE_MINUTE`].
#[derive(Debug, Clone, Default)]
struct Late {
    minutes: u32,
    army: BTreeMap<&'static str, u64>,
    trained: BTreeMap<&'static str, u64>,
}

/// A battle whose follow-up is still being watched.
struct Watch {
    battle: BattleHealth,
    centroid: TilePos,
    force: BTreeSet<Target>,
    due: u64,
}

/// One pass over a recording.
struct Pass {
    first_minute: u64,
    teams: Vec<u8>,
    income: Vec<BTreeMap<IncomeSource, Vec<u64>>>,
    wrecks: Vec<WreckSample>,
    late: Vec<Option<Late>>,
    /// Every building seen, by id.
    buildings: Vec<Life>,
    /// Recent hitters of each living body: hitter, its team, last hit tick.
    hitters: BTreeMap<Target, BTreeMap<Target, (u8, u64)>>,
    deaths: Vec<Death>,
    died: BTreeMap<Target, usize>,
    clusterer: BattleClusterer,
    closed: usize,
    watching: Vec<Watch>,
    battles: Vec<BattleHealth>,
}

impl Pass {
    fn new(state: &State) -> Self {
        let seats = state.players().len();
        let teams: Vec<u8> = state.players().iter().map(|player| player.team).collect();
        let start = state.current_tick();
        let buildings = state
            .buildings()
            .iter()
            .map(|building| Life {
                id: building.id,
                team: teams[usize::from(building.player.0)],
                kind: building.kind,
                tile: TilePos::containing(building.center()),
                born: start,
                completed: building.built(),
                gone: None,
            })
            .collect();
        let mut pass = Self {
            first_minute: start / MINUTE,
            teams,
            income: vec![BTreeMap::new(); seats],
            wrecks: Vec::new(),
            late: vec![None; seats],
            buildings,
            hitters: BTreeMap::new(),
            deaths: Vec::new(),
            died: BTreeMap::new(),
            clusterer: BattleClusterer::default(),
            closed: 0,
            watching: Vec::new(),
            battles: Vec::new(),
        };
        // A full recording samples this minute as the tick before it ends;
        // a restored one starting on it samples it here.
        if start > 0 && start.is_multiple_of(MINUTE) {
            pass.sample(state, start / MINUTE);
        }
        pass
    }

    fn life(&mut self, id: BuildingId) -> Option<&mut Life> {
        let index = self
            .buildings
            .binary_search_by_key(&id, |life| life.id)
            .ok()?;
        Some(&mut self.buildings[index])
    }

    /// Accounts for the tick `report` describes, which left `state`, after
    /// `ledger` observed it. Income lands in the minute of the tick that
    /// earned it; everything else carries the tick of the state it left, as
    /// `replay-summary` dates its moments.
    fn observe(&mut self, state: &State, report: &TickReport, ledger: &ImpactLedger) {
        let now = state.current_tick();
        let minute = usize::try_from(report.tick / MINUTE - self.first_minute)
            .expect("minutes of a recording fit in usize");
        for credit in ledger.credits() {
            let series = self.income[usize::from(credit.seat)]
                .entry(credit.source)
                .or_default();
            if series.len() <= minute {
                series.resize(minute + 1, 0);
            }
            series[minute] += credit.scrap;
        }
        // Ids only grow, and buildings stay sorted by id.
        let known = self.buildings.last().map(|life| life.id);
        let fresh = state
            .buildings()
            .iter()
            .rev()
            .take_while(|building| known.is_none_or(|known| building.id > known))
            .count();
        for building in &state.buildings()[state.buildings().len() - fresh..] {
            self.buildings.push(Life {
                id: building.id,
                team: self.teams[usize::from(building.player.0)],
                kind: building.kind,
                tile: TilePos::containing(building.center()),
                born: now,
                completed: building.built(),
                gone: None,
            });
        }
        for hit in ledger.hits() {
            let hitters = self.hitters.entry(hit.victim).or_default();
            hitters.retain(|_, (_, last)| *last + BATTLE_QUIET_TICKS >= now);
            for &(source, team) in &hit.sources {
                hitters.insert(source, (team, now));
            }
        }
        for event in &report.events {
            match event {
                Event::BuildingCompleted { building, .. } => {
                    if let Some(life) = self.life(*building) {
                        life.completed = true;
                    }
                }
                Event::BuildingSalvaged { building, .. }
                | Event::BuildCancelled { building, .. } => {
                    if let Some(life) = self.life(*building) {
                        life.gone = Some(now);
                    }
                }
                Event::UnitDied { .. } | Event::BuildingDestroyed { .. } => {
                    self.death(event, now);
                }
                Event::UnitTrained { kind, player, .. }
                    if report.tick >= LATE_TICK && kind.stats().can_fight() =>
                {
                    *self.late[usize::from(player.0)]
                        .get_or_insert_with(Late::default)
                        .trained
                        .entry(kind.name())
                        .or_default() += u64::from(kind.stats().cost);
                }
                _ => {}
            }
        }
        self.clusterer.expire(now);
        self.close(now);
        self.judge(state, now, false);
        if now.is_multiple_of(MINUTE) {
            self.sample(state, now / MINUTE);
        }
    }

    fn death(&mut self, event: &Event, now: u64) {
        let (role, building) = match event {
            Event::UnitDied { kind, .. } => (
                if kind.stats().harvest.is_some() {
                    Role::Income
                } else {
                    Role::Other
                },
                None,
            ),
            Event::BuildingDestroyed { building, tier, .. } => {
                let completed = self
                    .life(*building)
                    .filter(|life| life.completed)
                    .map(|life| life.kind);
                let role = match completed {
                    Some(kind) if !kind.tier_stats(*tier).produces.is_empty() => Role::Production,
                    Some(BuildingKind::Reclaimer | BuildingKind::Extractor) => Role::Income,
                    _ => Role::Other,
                };
                (role, Some((*building, completed)))
            }
            _ => return,
        };
        let Some(loss) =
            crate::replay_summary::loss(event, |_| building.and_then(|(_, kind)| kind))
        else {
            return;
        };
        if let Some((id, _)) = building
            && let Some(life) = self.life(id)
        {
            life.gone = Some(now);
        }
        let hitters = self
            .hitters
            .remove(&loss.victim)
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, (_, last))| *last + BATTLE_QUIET_TICKS >= now)
            .map(|(source, (team, _))| (source, team))
            .collect();
        self.died.insert(loss.victim, self.deaths.len());
        self.deaths.push(Death {
            tick: now,
            team: self.teams[usize::from(loss.seat)],
            role,
            value: loss.value,
            hitters,
        });
        self.clusterer.note_loss(now, loss);
    }

    /// Starts watching every battle the clusterer closed since the last
    /// call.
    fn close(&mut self, now: u64) {
        while let Some(battle) = self.clusterer.battles().get(self.closed) {
            self.closed += 1;
            let deaths: Vec<&Death> = battle
                .victims
                .iter()
                .filter_map(|victim| self.died.get(victim))
                .map(|&index| &self.deaths[index])
                .collect();
            let winner = match crate::replay_summary::verdict(&battle.losses, &self.teams) {
                Verdict::Even => None,
                Verdict::Favors(team) => Some(team),
                Verdict::OneSided(against) => {
                    let mut hitting: BTreeSet<u8> = deaths
                        .iter()
                        .flat_map(|death| death.hitters.iter().map(|(_, team)| *team))
                        .filter(|team| *team != against)
                        .collect();
                    if hitting.is_empty() {
                        hitting = self
                            .teams
                            .iter()
                            .copied()
                            .filter(|team| *team != against)
                            .collect();
                    }
                    match hitting.len() {
                        1 => hitting.pop_first(),
                        _ => None,
                    }
                }
            };
            let mut health = BattleHealth {
                from_tick: battle.from_tick,
                to_tick: battle.to_tick,
                at: battle.at,
                losses: battle.losses.clone(),
                shootdown: battle.shootdown,
                winner,
                follow_up: None,
            };
            let Some(winner) = winner else {
                self.battles.push(health);
                continue;
            };
            let losers: Vec<u8> = battle
                .losses
                .iter()
                .map(|loss| self.teams[usize::from(loss.seat)])
                .filter(|team| *team != winner)
                .collect::<BTreeSet<u8>>()
                .into_iter()
                .collect();
            let force: BTreeSet<Target> = deaths
                .iter()
                .flat_map(|death| death.hitters.iter())
                .filter(|(_, team)| *team == winner)
                .map(|(source, _)| *source)
                .collect();
            let centroid = TilePos::new(battle.at[0], battle.at[1]);
            let near = |teams: &dyn Fn(u8) -> bool, marks: fn(BuildingKind) -> bool| {
                self.buildings
                    .iter()
                    .filter(|life| {
                        teams(life.team) && marks(life.kind) && life.standing(battle.from_tick)
                    })
                    .map(|life| life.tile.chebyshev(centroid))
                    .min()
                    .unwrap_or(i32::MAX)
            };
            let place_by = |marks: fn(BuildingKind) -> bool| {
                Place::from_distances(
                    near(&|team| team == winner, marks),
                    near(&|team| losers.contains(&team), marks),
                )
            };
            health.follow_up = Some(FollowUp {
                place: place_by(marks_home),
                own_ground: place_by(Place::marks) == Place::Home,
                losers,
                force: u32::try_from(force.len()).expect("a force fits in u32"),
                until: now,
                production: 0,
                income: 0,
                stayed: 0,
                opposed: 0,
                held: false,
            });
            self.watching.push(Watch {
                due: battle.to_tick + FOLLOW_UP_TICKS,
                battle: health,
                centroid,
                force,
            });
        }
    }

    /// Settles every watched battle due by `now`, or all of them at the
    /// match's end, from `state`, the state tick `now` left.
    fn judge(&mut self, state: &State, now: u64, end: bool) {
        let mut index = 0;
        while index < self.watching.len() {
            if !end && self.watching[index].due > now {
                index += 1;
                continue;
            }
            let watch = self.watching.remove(index);
            self.battles.push(self.settle(watch, state, now));
        }
    }

    fn settle(&self, watch: Watch, state: &State, now: u64) -> BattleHealth {
        let Watch {
            mut battle,
            centroid,
            force,
            ..
        } = watch;
        let follow_up = battle
            .follow_up
            .as_mut()
            .expect("watched battles have a winner");
        let winner = battle.winner.expect("watched battles have a winner");
        follow_up.until = now;
        let first = self
            .deaths
            .partition_point(|death| death.tick < battle.from_tick);
        for death in &self.deaths[first..] {
            if death.tick > now {
                break;
            }
            if !follow_up.losers.contains(&death.team)
                || !death
                    .hitters
                    .iter()
                    .any(|(source, _)| force.contains(source))
            {
                continue;
            }
            match death.role {
                Role::Production => follow_up.production += death.value,
                Role::Income => follow_up.income += death.value,
                Role::Other => {}
            }
        }
        let radius = i32::try_from(BATTLE_RADIUS_TILES).expect("the battle radius fits in i32");
        for unit in state.units() {
            let stats = unit.kind.stats();
            if !stats.can_fight() || unit.tile().chebyshev(centroid) > radius {
                continue;
            }
            let team = self.teams[usize::from(unit.player.0)];
            if team == winner {
                follow_up.stayed += u64::from(stats.cost);
            } else if follow_up.losers.contains(&team) {
                follow_up.opposed += u64::from(stats.cost);
            }
        }
        for building in state.buildings() {
            let team = self.teams[usize::from(building.player.0)];
            if building.built()
                && building.kind.tier_stats(building.tier).can_fight()
                && follow_up.losers.contains(&team)
                && TilePos::containing(building.center()).chebyshev(centroid) <= radius
            {
                follow_up.opposed += u64::from(building.kind.invested_cost(building.tier));
            }
        }
        follow_up.held = follow_up.stayed > 0 && follow_up.stayed > follow_up.opposed;
        battle
    }

    /// Wreck on the map and, from [`LATE_MINUTE`], each seat's armed units.
    fn sample(&mut self, state: &State, minute: u64) {
        let seats = self.teams.len();
        let mut marks: Vec<Vec<TilePos>> = vec![Vec::new(); seats];
        for building in state.buildings() {
            if marks_home(building.kind) {
                marks[usize::from(building.player.0)].push(TilePos::containing(building.center()));
            }
        }
        let mut sample = WreckSample {
            minute,
            total: 0,
            home: vec![0; seats],
        };
        for (tile, info) in state.map().iter() {
            if info.wreck == 0 {
                continue;
            }
            let wreck = u64::from(info.wreck);
            sample.total += wreck;
            let distance: Vec<i32> = marks
                .iter()
                .map(|marks| {
                    marks
                        .iter()
                        .map(|mark| mark.chebyshev(tile))
                        .min()
                        .unwrap_or(i32::MAX)
                })
                .collect();
            let home = (0..seats).find(|&seat| {
                let theirs = (0..seats)
                    .filter(|other| *other != seat)
                    .map(|other| distance[other])
                    .min()
                    .unwrap_or(i32::MAX);
                Place::from_distances(distance[seat], theirs) == Place::Home
            });
            if let Some(seat) = home {
                sample.home[seat] += wreck;
            }
        }
        self.wrecks.push(sample);
        if minute < LATE_MINUTE {
            return;
        }
        for (seat, late) in self.late.iter_mut().enumerate() {
            if state.players()[seat].eliminated_at.is_some() {
                continue;
            }
            let late = late.get_or_insert_with(Late::default);
            late.minutes += 1;
            for unit in state.units() {
                if usize::from(unit.player.0) != seat {
                    continue;
                }
                for kind in
                    std::iter::once(unit.kind).chain(unit.cargo.iter().map(|rider| rider.kind))
                {
                    let stats = kind.stats();
                    if stats.can_fight() {
                        *late.army.entry(kind.name()).or_default() += u64::from(stats.cost);
                    }
                }
            }
        }
    }

    fn finish(mut self, state: &State, path: &Path, replay: &GameReplay) -> MatchHealth {
        let end = state.current_tick();
        self.clusterer.close_all();
        self.close(end);
        self.judge(state, end, true);
        self.battles
            .sort_by_key(|battle| (battle.from_tick, battle.at));
        let start = self.first_minute;
        let minutes = usize::try_from(end.saturating_sub(1) / MINUTE + 1 - start)
            .expect("minutes fit in usize");
        let controllers = crate::replay_ledger::seat_players(replay);
        let seats = self
            .income
            .into_iter()
            .zip(self.late)
            .zip(controllers)
            .enumerate()
            .map(|(seat, ((mut income, late), controller))| {
                for source in IncomeSource::ALL {
                    income.entry(source).or_default().resize(minutes, 0);
                }
                SeatHealth {
                    seat: u8::try_from(seat).expect("seat indices fit in u8"),
                    name: replay.setup.players[seat].name.clone(),
                    team: self.teams[seat],
                    controller,
                    eliminated_at: state.players()[seat].eliminated_at,
                    income,
                    late_minutes: late.as_ref().map_or(0, |late| late.minutes),
                    late_army: late.as_ref().map(|late| composition(&late.army)),
                    late_trained: late.as_ref().map(|late| composition(&late.trained)),
                }
            })
            .collect();
        MatchHealth {
            path: path.to_owned(),
            scenario: replay.setup.name.clone(),
            start: replay.start_tick(),
            end,
            result: state.result(),
            winner_seats: state.winners().into_iter().map(|seat| seat.0).collect(),
            long: end >= LONG_MINUTES * MINUTE,
            first_minute: start,
            seats,
            wrecks: self.wrecks,
            battles: self.battles,
        }
    }
}

/// Whether a building of `kind` marks its owner's home: anything that trains
/// units. Refineries, defenses and Extractors spread across the map, so
/// counting them would leave no neutral ground.
fn marks_home(kind: BuildingKind) -> bool {
    kind.tiers().iter().any(|stats| !stats.produces.is_empty())
}

fn composition(value: &BTreeMap<&'static str, u64>) -> Composition {
    let entropy_bits = entropy(value.values().copied());
    let total: u64 = value.values().sum();
    Composition {
        value: value
            .iter()
            .map(|(kind, value)| ((*kind).to_owned(), *value))
            .collect(),
        entropy_bits,
        effective_kinds: if total == 0 { 0.0 } else { entropy_bits.exp2() },
    }
}

/// Shannon entropy, in bits, of the shares `values` make of their sum.
fn entropy(values: impl Iterator<Item = u64> + Clone) -> f64 {
    let total: u64 = values.clone().sum();
    if total == 0 {
        return 0.0;
    }
    values
        .filter(|value| *value > 0)
        .map(|value| {
            let share = value as f64 / total as f64;
            share * share.recip().log2()
        })
        .sum()
}

/// Re-executes `replay`, read from `path`, to its decision or its end.
pub fn health(path: &Path, replay: &GameReplay) -> Result<MatchHealth> {
    replay.validate(Some(SIM_VERSION))?;
    let total = oxide_kit::bounded_replay_duration(replay)?;
    let mut state = oxide_kit::recording::initial_state(replay)?;
    let mut ledger = ImpactLedger::new(&state);
    let mut pass = Pass::new(&state);
    let mut playback = oxide_kit::ReplayPlayback::new(replay);
    while state.current_tick() < total && state.result().is_none() {
        let report = playback.step(&mut state);
        ledger.observe(&state, &report);
        pass.observe(&state, &report, &ledger);
    }
    Ok(pass.finish(&state, path, replay))
}

/// `paths` with each directory replaced by the `.json` files directly
/// inside it, in name order: a `bot-eval --replay-dir` reads as its
/// replays.
pub fn expand(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for path in paths {
        if !path.is_dir() {
            out.push(path.clone());
            continue;
        }
        let mut found: Vec<PathBuf> = std::fs::read_dir(path)
            .with_context(|| format!("reading {}", path.display()))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<_>>()
            .with_context(|| format!("reading {}", path.display()))?;
        found.retain(|file| file.is_file() && file.extension().is_some_and(|ext| ext == "json"));
        if found.is_empty() {
            bail!("{} holds no .json recordings", path.display());
        }
        found.sort();
        out.extend(found);
    }
    Ok(out)
}

/// Each recording's health, re-executing at most `jobs` at once, in input
/// order.
pub fn measure(paths: &[PathBuf], jobs: NonZeroUsize) -> Result<Vec<MatchHealth>> {
    let paths = expand(paths)?;
    crate::pool::fan_out_bounded(&paths, jobs, |path| {
        let replay = crate::replay_ledger::load(path)?;
        health(path, &replay).with_context(|| format!("measuring {}", path.display()))
    })
}

/// Health pooled over matches.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HealthSummary {
    /// Matches pooled.
    pub matches: u32,
    /// Matches that decided.
    pub decided: u32,
    /// Matches that reached [`LONG_MINUTES`].
    pub long: u32,
    /// Matches whose recording ended undecided before [`LONG_MINUTES`], so
    /// whether they would have run long is unknown.
    pub cut_short: u32,
    /// Match lengths in minutes, shortest first.
    pub minutes: Vec<f64>,
    /// Income per seat-minute in the match, by band.
    pub income: Vec<IncomeBand>,
    /// Each source's share of all income from minute [`LATE_MINUTE`].
    pub late_income: BTreeMap<IncomeSource, f64>,
    /// Wreck on the map every five minutes.
    pub wrecks: Vec<WreckMark>,
    /// Standing armies from minute [`LATE_MINUTE`].
    pub late_army: ArmySummary,
    /// Armed units trained from minute [`LATE_MINUTE`].
    pub late_trained: ArmySummary,
    /// What won battles turned into.
    pub follow_up: Vec<FollowUpRow>,
}

/// Income in one band of minutes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IncomeBand {
    /// First minute.
    pub from: u64,
    /// Minute after the last, absent for the open last band.
    pub to: Option<u64>,
    /// Seat-minutes in the match within the band.
    pub seat_minutes: f64,
    /// Scrap per seat-minute by source.
    pub per_minute: BTreeMap<IncomeSource, f64>,
}

/// Wreck at one minute, over the matches still running then.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WreckMark {
    /// The minute.
    pub minute: u64,
    /// Matches sampled.
    pub matches: u32,
    /// Mean scrap of wreck on the map.
    pub mean: f64,
    /// Share of it lying at a seat's home.
    pub home: f64,
}

/// Late compositions pooled over seat-games, each weighted equally so one
/// long match cannot outweigh the rest.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ArmySummary {
    /// Seat-games with any units.
    pub seats: u32,
    /// Mean entropy over those seat-games, in bits.
    pub entropy_bits: f64,
    /// Mean effective kinds over them.
    pub effective_kinds: f64,
    /// Each kind's mean share of a seat-game's value, most first.
    pub shares: Vec<(String, f64)>,
}

/// Won battles of one kind and what they turned into.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FollowUpRow {
    /// Which battles: pushes, by phase, by place, or all.
    pub label: String,
    /// Battles won, shootdowns excluded.
    pub won: u32,
    /// Of those, the wins that [`FollowUp::converted`].
    pub converted: u32,
    /// The wins that [`FollowUp::razed_production`].
    pub production: u32,
    /// The wins that [`FollowUp::took_income`].
    pub income: u32,
    /// The wins that [`FollowUp::held_ground`].
    pub held: u32,
    /// Mean production scrap destroyed per won battle.
    pub production_scrap: f64,
    /// Mean income scrap destroyed per won battle.
    pub income_scrap: f64,
}

/// Pools `matches`.
pub fn summarize(matches: &[MatchHealth]) -> HealthSummary {
    let mut minutes: Vec<f64> = matches
        .iter()
        .map(|game| game.end as f64 / MINUTE as f64)
        .collect();
    minutes.sort_by(f64::total_cmp);
    HealthSummary {
        matches: count(matches.iter()),
        decided: count(matches.iter().filter(|game| game.result.is_some())),
        long: count(matches.iter().filter(|game| game.long)),
        cut_short: count(
            matches
                .iter()
                .filter(|game| game.result.is_none() && !game.long),
        ),
        minutes,
        income: income_bands(matches),
        late_income: late_income(matches),
        wrecks: wreck_marks(matches),
        late_army: pool(matches, |seat| seat.late_army.as_ref()),
        late_trained: pool(matches, |seat| seat.late_trained.as_ref()),
        follow_up: follow_up(matches),
    }
}

fn count<T>(items: impl Iterator<Item = T>) -> u32 {
    u32::try_from(items.count()).expect("counts fit in u32")
}

/// Ticks `seat` was in `game` within minute `minute`.
fn ticks_in(game: &MatchHealth, seat: &SeatHealth, minute: u64) -> u64 {
    let out = seat
        .eliminated_at
        .map_or(game.end, |tick| tick.min(game.end));
    let from = (minute * MINUTE).max(game.start);
    let to = ((minute + 1) * MINUTE).min(out);
    to.saturating_sub(from)
}

fn income_bands(matches: &[MatchHealth]) -> Vec<IncomeBand> {
    let band = |minute: u64| {
        INCOME_BANDS
            .iter()
            .filter(|start| minute >= **start)
            .count()
            - 1
    };
    let mut scrap = vec![BTreeMap::<IncomeSource, u64>::new(); INCOME_BANDS.len()];
    let mut ticks = vec![0_u64; INCOME_BANDS.len()];
    for game in matches {
        let span = game.end.saturating_sub(1) / MINUTE + 1 - game.first_minute;
        for seat in &game.seats {
            for index in 0..usize::try_from(span).expect("minutes fit in usize") {
                let minute = game.first_minute + index as u64;
                ticks[band(minute)] += ticks_in(game, seat, minute);
                for (source, series) in &seat.income {
                    *scrap[band(minute)].entry(*source).or_default() +=
                        series.get(index).copied().unwrap_or(0);
                }
            }
        }
    }
    INCOME_BANDS
        .iter()
        .enumerate()
        .filter(|(index, _)| ticks[*index] > 0)
        .map(|(index, from)| {
            let seat_minutes = ticks[index] as f64 / MINUTE as f64;
            IncomeBand {
                from: *from,
                to: INCOME_BANDS.get(index + 1).copied(),
                seat_minutes,
                per_minute: IncomeSource::ALL
                    .iter()
                    .map(|source| {
                        let earned = scrap[index].get(source).copied().unwrap_or(0);
                        (*source, earned as f64 / seat_minutes)
                    })
                    .collect(),
            }
        })
        .collect()
}

fn late_income(matches: &[MatchHealth]) -> BTreeMap<IncomeSource, f64> {
    let mut earned: BTreeMap<IncomeSource, u64> = BTreeMap::new();
    for game in matches {
        let skip = usize::try_from(LATE_MINUTE.saturating_sub(game.first_minute))
            .expect("minutes fit in usize");
        for seat in &game.seats {
            for (source, series) in &seat.income {
                *earned.entry(*source).or_default() += series.iter().skip(skip).sum::<u64>();
            }
        }
    }
    let total: u64 = earned.values().sum();
    earned
        .into_iter()
        .map(|(source, scrap)| (source, ratio(scrap, total)))
        .collect()
}

fn wreck_marks(matches: &[MatchHealth]) -> Vec<WreckMark> {
    let last = matches
        .iter()
        .filter_map(|game| game.wrecks.last())
        .map(|sample| sample.minute)
        .max()
        .unwrap_or(0);
    (1..=last / WRECK_MARK_MINUTES)
        .map(|mark| mark * WRECK_MARK_MINUTES)
        .filter_map(|minute| {
            let samples: Vec<&WreckSample> = matches
                .iter()
                .filter_map(|game| game.wrecks.iter().find(|sample| sample.minute == minute))
                .collect();
            if samples.is_empty() {
                return None;
            }
            let total: u64 = samples.iter().map(|sample| sample.total).sum();
            let home: u64 = samples
                .iter()
                .map(|sample| sample.home.iter().sum::<u64>())
                .sum();
            Some(WreckMark {
                minute,
                matches: count(samples.iter()),
                mean: total as f64 / samples.len() as f64,
                home: ratio(home, total),
            })
        })
        .collect()
}

fn pool(matches: &[MatchHealth], pick: fn(&SeatHealth) -> Option<&Composition>) -> ArmySummary {
    let armies: Vec<&Composition> = matches
        .iter()
        .flat_map(|game| &game.seats)
        .filter_map(pick)
        .filter(|army| army.value.values().any(|value| *value > 0))
        .collect();
    let mut pooled: BTreeMap<&str, f64> = BTreeMap::new();
    for army in &armies {
        let total: u64 = army.value.values().sum();
        for (kind, value) in &army.value {
            *pooled.entry(kind).or_default() += ratio(*value, total) / armies.len() as f64;
        }
    }
    let mut shares: Vec<(String, f64)> = pooled
        .into_iter()
        .map(|(kind, share)| (kind.to_owned(), share))
        .collect();
    shares.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mean = |metric: fn(&Composition) -> f64| {
        if armies.is_empty() {
            0.0
        } else {
            armies.iter().map(|army| metric(army)).sum::<f64>() / armies.len() as f64
        }
    };
    ArmySummary {
        seats: count(armies.iter()),
        entropy_bits: mean(|army| army.entropy_bits),
        effective_kinds: mean(|army| army.effective_kinds),
        shares,
    }
}

/// Which won battles a follow-up row pools.
type Keep = fn(&BattleHealth, &FollowUp) -> bool;

fn follow_up(matches: &[MatchHealth]) -> Vec<FollowUpRow> {
    let won: Vec<(&BattleHealth, &FollowUp)> = matches
        .iter()
        .flat_map(|game| &game.battles)
        .filter(|battle| !battle.shootdown)
        .filter_map(|battle| Some((battle, battle.follow_up.as_ref()?)))
        .collect();
    let groups: [(&str, Keep); 7] = [
        ("pushes", |_, follow| follow.place != Place::Home),
        ("pushes before minute 15", |battle, follow| {
            follow.place != Place::Home && battle.from_tick < LATE_TICK
        }),
        ("pushes from minute 15", |battle, follow| {
            follow.place != Place::Home && battle.from_tick >= LATE_TICK
        }),
        ("at the winner's home", |_, follow| {
            follow.place == Place::Home
        }),
        ("in the field", |_, follow| follow.place == Place::Field),
        ("at the loser's home", |_, follow| {
            follow.place == Place::Away
        }),
        ("all", |_, _| true),
    ];
    groups
        .iter()
        .map(|(label, keep)| {
            let rows: Vec<&FollowUp> = won
                .iter()
                .filter(|(battle, follow)| keep(battle, follow))
                .map(|(_, follow)| *follow)
                .collect();
            let n = rows.len().max(1) as f64;
            FollowUpRow {
                label: (*label).to_owned(),
                won: count(rows.iter()),
                converted: count(rows.iter().filter(|follow| follow.converted())),
                production: count(rows.iter().filter(|follow| follow.razed_production())),
                income: count(rows.iter().filter(|follow| follow.took_income())),
                held: count(rows.iter().filter(|follow| follow.held_ground())),
                production_scrap: rows.iter().map(|follow| follow.production).sum::<u64>() as f64
                    / n,
                income_scrap: rows.iter().map(|follow| follow.income).sum::<u64>() as f64 / n,
            }
        })
        .collect()
}

fn ratio(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

fn percent(part: u32, whole: u32) -> String {
    if whole == 0 {
        "-".to_owned()
    } else {
        format!("{:.0}%", f64::from(part) * 100.0 / f64::from(whole))
    }
}

/// The report over `matches`: the pooled summary, then a line per match.
pub fn render(matches: &[MatchHealth], summary: &HealthSummary) -> String {
    let mut out = String::new();
    let median = summary
        .minutes
        .get(summary.minutes.len() / 2)
        .copied()
        .unwrap_or(0.0);
    let _ = writeln!(
        out,
        "{} match{}, {} decided; {} reached {LONG_MINUTES} minutes ({}){}; length {:.1}-{:.1} min, median {median:.1}",
        summary.matches,
        if summary.matches == 1 { "" } else { "es" },
        summary.decided,
        summary.long,
        percent(summary.long, summary.matches),
        if summary.cut_short > 0 {
            format!(", {} cut short undecided", summary.cut_short)
        } else {
            String::new()
        },
        summary.minutes.first().copied().unwrap_or(0.0),
        summary.minutes.last().copied().unwrap_or(0.0),
    );
    let _ = writeln!(out, "\n== income per seat-minute in the match");
    let _ = write!(out, "  {:<8} {:>8} {:>7}", "minutes", "seat-min", "total");
    for source in IncomeSource::ALL {
        let _ = write!(out, " {:>13}", source.name());
    }
    let _ = writeln!(out);
    for band in &summary.income {
        let range = band.to.map_or_else(
            || format!("{}+", band.from),
            |to| format!("{}-{to}", band.from),
        );
        let total: f64 = band.per_minute.values().sum();
        let _ = write!(out, "  {range:<8} {:>8.0} {total:>7.0}", band.seat_minutes);
        for source in IncomeSource::ALL {
            let rate = band.per_minute.get(&source).copied().unwrap_or(0.0);
            let _ = write!(out, " {rate:>8.0} {:>4}", share(rate, total));
        }
        let _ = writeln!(out);
    }
    let _ = writeln!(
        out,
        "  share from minute {LATE_MINUTE}: {}",
        IncomeSource::ALL
            .iter()
            .filter_map(|source| {
                let share = summary.late_income.get(source).copied().unwrap_or(0.0);
                (share > 0.0).then(|| format!("{} {:.0}%", source.name(), share * 100.0))
            })
            .collect::<Vec<_>>()
            .join(", ")
    );
    let _ = writeln!(
        out,
        "\n== wreck on the map (mean scrap over matches still running; share at a seat's home)"
    );
    let _ = writeln!(
        out,
        "  {}",
        summary
            .wrecks
            .iter()
            .map(|mark| format!(
                "{}m {:.0} ({:.0}%, n={})",
                mark.minute,
                mark.mean,
                mark.home * 100.0,
                mark.matches
            ))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let _ = writeln!(
        out,
        "\n== armed units from minute {LATE_MINUTE} by value, each seat-game weighted equally"
    );
    for (label, army) in [
        ("standing", &summary.late_army),
        ("trained", &summary.late_trained),
    ] {
        let _ = writeln!(
            out,
            "  {label:<8} {} seat-games, entropy {:.2} bits ({:.1} effective kinds): {}",
            army.seats,
            army.entropy_bits,
            army.effective_kinds,
            army.shares
                .iter()
                .take(8)
                .map(|(kind, share)| format!("{kind} {:.0}%", share * 100.0))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let _ = writeln!(
        out,
        "\n== won battles and the minute after; pushes are wins away from the winner's home"
    );
    let _ = writeln!(
        out,
        "  converted: the winning force razed production, took at least {INCOME_FLOOR} scrap of income, or held ground nearer the losers' buildings than its own"
    );
    let _ = writeln!(
        out,
        "  {:<24} {:>4} {:>9} {:>10} {:>7} {:>5} | {:>9} {:>7}",
        "battles", "won", "converted", "production", "income", "held", "prod/won", "inc/won"
    );
    for row in &summary.follow_up {
        let _ = writeln!(
            out,
            "  {:<24} {:>4} {:>9} {:>10} {:>7} {:>5} | {:>9.0} {:>7.0}",
            row.label,
            row.won,
            percent(row.converted, row.won),
            percent(row.production, row.won),
            percent(row.income, row.won),
            percent(row.held, row.won),
            row.production_scrap,
            row.income_scrap,
        );
    }
    let _ = writeln!(out, "\n== matches");
    for game in matches {
        let won: Vec<&FollowUp> = game
            .battles
            .iter()
            .filter(|battle| !battle.shootdown)
            .filter_map(|battle| battle.follow_up.as_ref())
            .collect();
        let result = match game.result {
            Some(GameResult::Victory { team }) => format!("team {team} won"),
            Some(GameResult::Draw) => "draw".to_owned(),
            None => "undecided".to_owned(),
        };
        let kinds = |pick: fn(&SeatHealth) -> Option<&Composition>| {
            game.seats
                .iter()
                .map(|seat| {
                    pick(seat).map_or_else(
                        || "-".to_owned(),
                        |army| format!("{:.1}", army.effective_kinds),
                    )
                })
                .collect::<Vec<_>>()
                .join("/")
        };
        let armies = format!(
            "standing {}, trained {}",
            kinds(|seat| seat.late_army.as_ref()),
            kinds(|seat| seat.late_trained.as_ref())
        );
        let _ = writeln!(
            out,
            "  {}: {}, {:.1} min, {result}; {} battles, {} won, {} converted; late kinds {armies}; peak wreck {}",
            game.path.file_name().map_or_else(
                || game.path.display().to_string(),
                |name| name.to_string_lossy().into_owned()
            ),
            game.scenario,
            game.end as f64 / MINUTE as f64,
            game.battles.len(),
            won.len(),
            won.iter().filter(|follow| follow.converted()).count(),
            game.wrecks
                .iter()
                .map(|sample| sample.total)
                .max()
                .unwrap_or(0),
        );
    }
    out
}

fn share(part: f64, whole: f64) -> String {
    if whole <= 0.0 {
        "-".to_owned()
    } else {
        format!("{:.0}%", part * 100.0 / whole)
    }
}

/// The report as JSON: each match's row, then the pooled summary.
#[derive(Debug, Serialize)]
pub struct HealthReport<'a> {
    /// One row per match, in input order.
    pub matches: &'a [MatchHealth],
    /// The pooled summary.
    pub summary: &'a HealthSummary,
}

#[cfg(test)]
mod tests;
