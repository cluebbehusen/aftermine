//! Match statistics: sampled series (scrap, wealth, army value, unit counts),
//! totals and the match's turning points, computed by re-executing a replay
//! (`compute`) or incrementally during a live match (`LiveMatchStats`).

use crate::GameReplay;
use aftermine_sim::{BuildingKind, Event, PlayerId, State, UnitKind};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Structures whose first completion marks a seat's tech turning point.
pub const LOUD_BUILDINGS: &[BuildingKind] = &[BuildingKind::Airworks, BuildingKind::Crucible];

/// Units whose first completion marks a seat's tech turning point: tier-3
/// breakthroughs and the transport. See [`LOUD_BUILDINGS`].
pub const LOUD_UNITS: &[UnitKind] = &[
    UnitKind::Breaker,
    UnitKind::Piledriver,
    UnitKind::Avalanche,
    UnitKind::Condor,
    UnitKind::Skyhook,
    UnitKind::Maw,
    UnitKind::Roc,
];

/// The most turning points a match keeps, however long it runs.
pub const MAX_MOMENTS: usize = 64;

/// What a turning point on the match's timeline marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MomentKind {
    /// The first unit or structure anyone lost in the match.
    FirstLoss,
    /// A seat finished a new Foundry.
    Expansion,
    /// A seat's first of a [`LOUD_BUILDINGS`] structure.
    TechStructure(BuildingKind),
    /// A seat's first of a [`LOUD_UNITS`] unit.
    TechUnit(UnitKind),
    /// A seat's Foundry was destroyed.
    FoundryLost,
}

/// A turning point on the match's timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Moment {
    /// The completed tick it happened on.
    pub tick: u64,
    /// The seat it happened to.
    pub seat: u8,
    /// What happened.
    pub kind: MomentKind,
}

/// One player's sampled series and totals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerStats {
    /// Seat index.
    pub seat: u8,
    /// Banked scrap at each sample point.
    pub scrap: Vec<u32>,
    /// Standing army value per sample: the costs of living units that fight.
    /// Workers, unarmed scouts and transports are not an army.
    pub army_value: Vec<u32>,
    /// Everything the seat owned at each sample: the bank, living units,
    /// structures and prepaid training queues, all at cost.
    pub wealth: Vec<u32>,
    /// Total scrap earned by each sample point.
    pub scrap_earned: Vec<u32>,
    /// Total value lost by each sample point.
    pub value_lost: Vec<u32>,
    /// Living units by kind name at each sample point. `BTreeMap` keys keep
    /// the serialization deterministic.
    #[serde(deserialize_with = "deserialize_kinds")]
    pub kinds: Vec<BTreeMap<&'static str, u16>>,
    /// Scrap brought home by Harvesters across the whole match.
    pub scrap_collected: u32,
    /// Scrap earned across the whole match: deliveries plus what standing
    /// structures paid in. Refunds and recovery grants are not income.
    pub scrap_earned_total: u32,
    /// Cost of everything destroyed across the whole match. Salvage and
    /// retirement are deliberate, and are not losses.
    pub value_lost_total: u32,
    /// Units completed across the whole match, trained or assembled.
    pub units_trained: u32,
    /// Units completed across the whole match, by kind.
    pub units_built: BTreeMap<UnitKind, u32>,
    /// Buildings completed across the whole match, upgrades included.
    pub buildings_completed: u32,
    /// New structures completed across the whole match, by kind. Upgrades
    /// are not new structures.
    pub structures_built: BTreeMap<BuildingKind, u32>,
    /// Units lost across the whole match.
    pub units_lost: u32,
    /// Buildings lost across the whole match.
    pub buildings_lost: u32,
    /// Buildings deliberately taken apart by their own crew; never counted
    /// among losses.
    pub buildings_salvaged: u32,
    /// Units dismantled at their owner's Crucible; never counted among
    /// losses.
    pub units_retired: u32,
}

/// The whole report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchStats {
    /// Tick of each sample column.
    pub sample_ticks: Vec<u64>,
    /// Per-seat series, seat order.
    pub players: Vec<PlayerStats>,
    /// Final tick executed.
    pub final_tick: u64,
    /// The match's turning points, oldest first, up to [`MAX_MOMENTS`].
    pub moments: Vec<Moment>,
}

/// Maximum retained graph columns while a live match runs. The closing
/// snapshot can append one exact final column beyond this bound.
const MAX_LIVE_SAMPLES: usize = 49;

/// Incremental statistics for a live match.
///
/// Totals consume the same deterministic tick events as [`compute`]. Graph
/// samples thin themselves by powers of two, keeping memory and end-of-match
/// work bounded no matter how long the session runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveMatchStats {
    stats: MatchStats,
    every: u64,
}

impl LiveMatchStats {
    pub(crate) fn validate_checkpoint(&self, state: &State) -> Result<()> {
        self.stats.validate_checkpoint(state)?;
        anyhow::ensure!(
            self.stats.final_tick == state.current_tick(),
            "statistics tick mismatch"
        );
        anyhow::ensure!(self.every.is_power_of_two(), "invalid statistics stride");
        anyhow::ensure!(
            self.stats.sample_ticks.len() <= MAX_LIVE_SAMPLES,
            "too many live samples"
        );
        anyhow::ensure!(
            self.stats
                .sample_ticks
                .iter()
                .all(|tick| tick.is_multiple_of(self.every)),
            "statistics samples disagree with stride"
        );
        Ok(())
    }
    /// Starts tracking from the current state, including an exact opening
    /// sample.
    pub fn new(state: &State) -> Self {
        let mut stats = MatchStats {
            sample_ticks: Vec::new(),
            players: blank_players(state.players().len()),
            final_tick: state.current_tick(),
            moments: Vec::new(),
        };
        sample(state, &mut stats.players, &mut stats.sample_ticks);
        Self { stats, every: 1 }
    }

    /// Consumes one completed tick's state and events.
    pub fn observe(&mut self, state: &State, events: &[Event]) {
        accumulate_events(
            state,
            &mut self.stats.players,
            &mut self.stats.moments,
            events,
        );
        self.stats.final_tick = state.current_tick();
        if state.current_tick().is_multiple_of(self.every) {
            sample(state, &mut self.stats.players, &mut self.stats.sample_ticks);
        }
        while self.stats.sample_ticks.len() > MAX_LIVE_SAMPLES {
            let next = self.every.saturating_mul(2);
            if next == self.every {
                break;
            }
            self.every = next;
            thin_samples(&mut self.stats, self.every);
        }
    }

    /// Clones the bounded report and appends an exact sample of `state` when
    /// the current thinning stride did not land on it.
    pub fn snapshot(&self, state: &State) -> MatchStats {
        let mut report = self.stats.clone();
        report.final_tick = state.current_tick();
        if report.sample_ticks.last() != Some(&state.current_tick()) {
            sample(state, &mut report.players, &mut report.sample_ticks);
        }
        report
    }
}

impl MatchStats {
    /// Validates a retained report against its session's seats and time boundary.
    pub fn validate_checkpoint(&self, state: &State) -> Result<()> {
        anyhow::ensure!(
            self.final_tick <= state.current_tick(),
            "statistics are from the future"
        );
        anyhow::ensure!(
            !self.sample_ticks.is_empty() && self.sample_ticks.len() <= MAX_LIVE_SAMPLES + 1,
            "invalid statistics sample count"
        );
        anyhow::ensure!(
            self.sample_ticks.windows(2).all(|pair| pair[0] < pair[1])
                && self
                    .sample_ticks
                    .last()
                    .is_some_and(|tick| *tick <= self.final_tick),
            "invalid statistics sample times"
        );
        anyhow::ensure!(
            self.players.len() == state.players().len(),
            "statistics seat count mismatch"
        );
        for (seat, player) in self.players.iter().enumerate() {
            let samples = self.sample_ticks.len();
            anyhow::ensure!(
                usize::from(player.seat) == seat
                    && player.scrap.len() == samples
                    && player.army_value.len() == samples
                    && player.wealth.len() == samples
                    && player.scrap_earned.len() == samples
                    && player.value_lost.len() == samples
                    && player.kinds.len() == samples,
                "statistics columns or seat mismatch"
            );
        }
        anyhow::ensure!(
            self.moments.len() <= MAX_MOMENTS,
            "too many statistics moments"
        );
        anyhow::ensure!(
            self.moments
                .windows(2)
                .all(|pair| pair[0].tick <= pair[1].tick)
                && self.moments.iter().all(|moment| {
                    moment.tick <= self.final_tick && usize::from(moment.seat) < self.players.len()
                }),
            "invalid statistics moments"
        );
        Ok(())
    }
}

fn deserialize_kinds<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<BTreeMap<&'static str, u16>>, D::Error> {
    Vec::<BTreeMap<String, u16>>::deserialize(deserializer)?
        .into_iter()
        .map(|sample| {
            sample
                .into_iter()
                .map(|(name, count)| {
                    aftermine_sim::UnitKind::ALL
                        .into_iter()
                        .find(|kind| kind.name() == name)
                        .map(|kind| (kind.name(), count))
                        .ok_or_else(|| {
                            serde::de::Error::custom(format!("unknown unit kind {name}"))
                        })
                })
                .collect()
        })
        .collect()
}

fn blank_players(seats: usize) -> Vec<PlayerStats> {
    (0..seats)
        .map(|seat| PlayerStats {
            seat: u8::try_from(seat).expect("seat indices fit in u8"),
            scrap: Vec::new(),
            army_value: Vec::new(),
            wealth: Vec::new(),
            scrap_earned: Vec::new(),
            value_lost: Vec::new(),
            kinds: Vec::new(),
            scrap_collected: 0,
            scrap_earned_total: 0,
            value_lost_total: 0,
            units_trained: 0,
            units_built: BTreeMap::new(),
            buildings_completed: 0,
            structures_built: BTreeMap::new(),
            units_lost: 0,
            buildings_lost: 0,
            buildings_salvaged: 0,
            units_retired: 0,
        })
        .collect()
}

fn sample(state: &State, stats: &mut [PlayerStats], ticks: &mut Vec<u64>) {
    ticks.push(state.current_tick());
    let mut army = vec![0u32; stats.len()];
    let mut wealth: Vec<u32> = state.players().iter().map(|player| player.scrap).collect();
    let mut counts: Vec<BTreeMap<&'static str, u16>> = vec![BTreeMap::new(); stats.len()];
    for unit in state.units() {
        let kind = unit.kind.stats();
        // A drone is its host's equipment, counted neither in an army nor
        // among its losses, launched or docked.
        if kind.docks() {
            continue;
        }
        let seat = usize::from(unit.player.0);
        wealth[seat] = wealth[seat].saturating_add(kind.cost);
        if kind.can_fight() || kind.point_defense.is_some() {
            army[seat] = army[seat].saturating_add(kind.cost);
        }
        *counts[seat].entry(unit.kind.name()).or_default() += 1;
    }
    for building in state.buildings() {
        let seat = usize::from(building.player.0);
        let prepaid = building
            .queue
            .iter()
            .map(|kind| kind.stats().cost)
            .fold(0, u32::saturating_add);
        wealth[seat] = wealth[seat]
            .saturating_add(building.kind.invested_cost(building.tier))
            .saturating_add(prepaid);
    }
    for (seat, entry) in stats.iter_mut().enumerate() {
        entry.scrap.push(state.players()[seat].scrap);
        entry.army_value.push(army[seat]);
        entry.wealth.push(wealth[seat]);
        entry.scrap_earned.push(entry.scrap_earned_total);
        entry.value_lost.push(entry.value_lost_total);
        entry.kinds.push(std::mem::take(&mut counts[seat]));
    }
}

fn note_moment(moments: &mut Vec<Moment>, tick: u64, player: PlayerId, kind: MomentKind) {
    if moments.len() < MAX_MOMENTS {
        moments.push(Moment {
            tick,
            seat: player.0,
            kind,
        });
    }
}

fn note_first_loss(moments: &mut Vec<Moment>, tick: u64, player: PlayerId) {
    if !moments
        .iter()
        .any(|moment| moment.kind == MomentKind::FirstLoss)
    {
        note_moment(moments, tick, player, MomentKind::FirstLoss);
    }
}

fn note_unit_built(
    seat: &mut PlayerStats,
    moments: &mut Vec<Moment>,
    tick: u64,
    player: PlayerId,
    kind: UnitKind,
) {
    seat.units_trained = seat.units_trained.saturating_add(1);
    let built = seat.units_built.entry(kind).or_default();
    if *built == 0 && LOUD_UNITS.contains(&kind) {
        note_moment(moments, tick, player, MomentKind::TechUnit(kind));
    }
    *built = built.saturating_add(1);
}

fn accumulate_events(
    state: &State,
    stats: &mut [PlayerStats],
    moments: &mut Vec<Moment>,
    events: &[Event],
) {
    let tick = state.current_tick();
    for event in events {
        match event {
            Event::ScrapDeposited { player, amount, .. } => {
                let seat = &mut stats[player.0 as usize];
                seat.scrap_collected = seat.scrap_collected.saturating_add(*amount);
                seat.scrap_earned_total = seat.scrap_earned_total.saturating_add(*amount);
            }
            Event::ScrapCredited { player, amount } => {
                let seat = &mut stats[player.0 as usize];
                seat.scrap_earned_total = seat.scrap_earned_total.saturating_add(*amount);
            }
            // Counters saturate: a checkpoint or snapshot from elsewhere may
            // start them anywhere.
            Event::UnitTrained { player, kind, .. } | Event::UnitAssembled { player, kind, .. } => {
                note_unit_built(&mut stats[player.0 as usize], moments, tick, *player, *kind);
            }
            Event::BuildingCompleted {
                building,
                player,
                kind,
            } => {
                let seat = &mut stats[player.0 as usize];
                seat.buildings_completed = seat.buildings_completed.saturating_add(1);
                // A completion at a nonzero tier is an upgrade finishing.
                if state.building(*building).is_some_and(|b| b.tier > 0) {
                    continue;
                }
                let built = seat.structures_built.entry(*kind).or_default();
                let first = *built == 0;
                *built = built.saturating_add(1);
                if *kind == BuildingKind::Foundry {
                    note_moment(moments, tick, *player, MomentKind::Expansion);
                } else if first && LOUD_BUILDINGS.contains(kind) {
                    note_moment(moments, tick, *player, MomentKind::TechStructure(*kind));
                }
            }
            Event::UnitDied { player, kind, .. } if !kind.stats().docks() => {
                let seat = &mut stats[player.0 as usize];
                seat.units_lost = seat.units_lost.saturating_add(1);
                seat.value_lost_total = seat.value_lost_total.saturating_add(kind.stats().cost);
                note_first_loss(moments, tick, *player);
            }
            Event::BuildingDestroyed {
                player, kind, tier, ..
            } => {
                let seat = &mut stats[player.0 as usize];
                seat.buildings_lost = seat.buildings_lost.saturating_add(1);
                seat.value_lost_total = seat
                    .value_lost_total
                    .saturating_add(kind.invested_cost(*tier));
                note_first_loss(moments, tick, *player);
                if *kind == BuildingKind::Foundry {
                    note_moment(moments, tick, *player, MomentKind::FoundryLost);
                }
            }
            Event::BuildingSalvaged { player, .. } => {
                let counter = &mut stats[player.0 as usize].buildings_salvaged;
                *counter = counter.saturating_add(1);
            }
            // A retired Harvester's load reached the bank like a delivery.
            Event::UnitRetired { player, cargo, .. } => {
                let seat = &mut stats[player.0 as usize];
                seat.units_retired = seat.units_retired.saturating_add(1);
                seat.scrap_collected = seat.scrap_collected.saturating_add(*cargo);
                seat.scrap_earned_total = seat.scrap_earned_total.saturating_add(*cargo);
            }
            _ => {}
        }
    }
}

fn thin_samples(stats: &mut MatchStats, every: u64) {
    let keep: Vec<usize> = stats
        .sample_ticks
        .iter()
        .enumerate()
        .filter_map(|(index, tick)| tick.is_multiple_of(every).then_some(index))
        .collect();
    stats.sample_ticks = keep
        .iter()
        .map(|index| stats.sample_ticks[*index])
        .collect();
    for player in &mut stats.players {
        player.scrap = keep.iter().map(|index| player.scrap[*index]).collect();
        player.army_value = keep.iter().map(|index| player.army_value[*index]).collect();
        player.wealth = keep.iter().map(|index| player.wealth[*index]).collect();
        player.scrap_earned = keep
            .iter()
            .map(|index| player.scrap_earned[*index])
            .collect();
        player.value_lost = keep.iter().map(|index| player.value_lost[*index]).collect();
        player.kinds = keep
            .iter()
            .map(|index| player.kinds[*index].clone())
            .collect();
    }
}

/// Re-executes a replay, sampling every `every` ticks. The same replay
/// yields the same report.
pub fn compute(replay: &GameReplay, every: u64) -> Result<MatchStats> {
    // Untrusted input: an out-of-order or cross-version record would
    // otherwise produce a plausible, wrong report instead of an error.
    replay
        .validate(Some(aftermine_sim::SIM_VERSION))
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    let every = every.max(1);
    let total = crate::bounded_replay_duration(replay)?;
    let mut state = crate::recording::initial_state(replay)?;
    let mut playback = crate::ReplayPlayback::new(replay);

    let mut stats = blank_players(state.players().len());
    let mut sample_ticks = Vec::new();
    let mut moments = Vec::new();

    let start = state.current_tick();
    sample(&state, &mut stats, &mut sample_ticks);
    for _ in start..total {
        let report = playback.step(&mut state);
        accumulate_events(&state, &mut stats, &mut moments, &report.events);
        if (state.current_tick() - start).is_multiple_of(every) {
            sample(&state, &mut stats, &mut sample_ticks);
        }
    }
    // The outcome always makes the record: without this, any length not
    // divisible by the stride reports stale closing numbers.
    if sample_ticks.last() != Some(&state.current_tick()) {
        sample(&state, &mut stats, &mut sample_ticks);
    }
    Ok(MatchStats {
        sample_ticks,
        players: stats,
        final_tick: state.current_tick(),
        moments,
    })
}

#[cfg(test)]
mod tests;
