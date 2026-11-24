//! Staged pressure scenarios. Each stages one situation a credible opponent
//! must answer: a scripted attacker seat issues tick-stamped commands for its
//! preset units while the defender seat's bot plays, and a check reads the
//! authoritative state to decide whether the bot answered in time.
//!
//! Each scenario runs once per variant: the defender's bot at a difficulty
//! and personality seed, on the staging as authored or reflected left to
//! right, so an answer that holds for one rung, one personality or one side
//! of the map alone shows.
//!
//! The checks read omniscient QA state and never reach a controller.

use aftermine_kit::GameReplay;
use aftermine_kit::controller::{record_events, seat_controllers};
use anyhow::{Context, Result, bail, ensure};

use aftermine_sim::scenario::BotDifficulty;
use aftermine_sim::stats::Domain;
use aftermine_sim::{
    AttackTarget, BuildingId, BuildingKind, Command, PlayerCommand, PlayerId, Rally, Scenario,
    State, UnitId, UnitKind,
};
use chassis::grid::TilePos;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::num::NonZeroUsize;
use std::ops::Range;
use std::path::Path;

/// One staged situation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PressureScenario {
    /// Short name used in reports.
    pub name: String,
    /// The staged match. The defender seat must be a configured bot; the
    /// attacker seat is driven only by `script`.
    pub scenario: Scenario,
    /// The bot seat under test.
    pub defender: u8,
    /// The scripted seat.
    pub attacker: u8,
    /// Ticks the check waits for.
    pub deadline: u64,
    /// Commands for the attacker's preset units. Unit ids follow scenario
    /// order.
    pub script: Vec<Scripted>,
    /// What counts as an answer.
    pub check: Check,
}

/// One attacker command, issued before the given tick executes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scripted {
    /// Tick the command is stamped with.
    pub tick: u64,
    /// The command.
    pub command: Command,
}

/// What the defender must achieve by the deadline. Every check but
/// [`Check::AnswersAir`] also needs a starting Foundry to stand: one the
/// defender held built at tick zero. A Foundry placed later, finished or not,
/// does not count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Check {
    /// A starting Foundry stands.
    KeepsFoundry,
    /// Within `within` ticks of first seeing an attacker aircraft, the
    /// defender owns anti-air: a unit built for it or a built Flak Turret.
    AnswersAir {
        /// Ticks allowed after the first sighting.
        within: u64,
    },
    /// Every attacker indirect-fire unit is destroyed or out of its range of
    /// the defender's buildings. Killing only the spotter does not pass:
    /// artillery left in range fires again once anything spots for it.
    SilencesArtillery,
    /// Every attacker unit that was carried and set down is destroyed, or the
    /// carrier fell before setting anyone down.
    ClearsLanding,
    /// Every attacker Maw is destroyed.
    StopsBreach,
    /// Every attacker unit is destroyed.
    DestroysAttackers,
}

impl Check {
    /// The check's name in scenario files.
    pub fn name(self) -> &'static str {
        match self {
            Check::KeepsFoundry => "keeps_foundry",
            Check::AnswersAir { .. } => "answers_air",
            Check::SilencesArtillery => "silences_artillery",
            Check::ClearsLanding => "clears_landing",
            Check::StopsBreach => "stops_breach",
            Check::DestroysAttackers => "destroys_attackers",
        }
    }
}

/// The defender's bot and the orientation of one run of a scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Variant {
    /// The defender's rung.
    pub difficulty: BotDifficulty,
    /// The defender's personality seed.
    pub seed: u64,
    /// Whether the staging is reflected left to right.
    pub mirrored: bool,
}

impl Variant {
    /// The variant in words.
    pub fn label(self) -> String {
        format!(
            "{} seed {}{}",
            self.difficulty,
            self.seed,
            if self.mirrored { " mirrored" } else { "" }
        )
    }

    /// The file name of a replay of `scenario` under this variant.
    pub fn replay_name(self, scenario: &str) -> String {
        format!(
            "{}-{}-seed{}{}.json",
            scenario.replace(' ', "-"),
            self.difficulty,
            self.seed,
            if self.mirrored { "-mirrored" } else { "" }
        )
    }
}

/// Every difficulty and seed, each as authored and then, with `mirror`,
/// reflected: difficulty first, then seed, then orientation.
pub fn variants(difficulties: &[BotDifficulty], seeds: Range<u64>, mirror: bool) -> Vec<Variant> {
    let orientations: &[bool] = if mirror { &[false, true] } else { &[false] };
    difficulties
        .iter()
        .flat_map(|&difficulty| {
            seeds.clone().flat_map(move |seed| {
                orientations.iter().map(move |&mirrored| Variant {
                    difficulty,
                    seed,
                    mirrored,
                })
            })
        })
        .collect()
}

/// How one run went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PressureOutcome {
    /// Scenario name.
    pub name: String,
    /// The check applied.
    pub check: Check,
    /// The defender's bot and the orientation.
    pub variant: Variant,
    /// Whether the defender answered.
    pub passed: bool,
    /// The deciding fact, in words.
    pub detail: String,
    /// Tick the run stopped at.
    pub ticks: u64,
}

/// Loads every scenario in `dir`, in file-name order.
pub fn load_all(dir: &Path) -> Result<Vec<PressureScenario>> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<_>>()?;
    paths.retain(|path| {
        path.extension()
            .is_some_and(|extension| extension == "json")
    });
    paths.sort();
    paths
        .iter()
        .map(|path| {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
        })
        .collect()
}

impl PressureScenario {
    /// The scenario as `variant` plays it: the defender's bot at its rung and
    /// personality seed with the authored stance, reflected when mirrored.
    pub fn staged(&self, variant: Variant) -> Result<Self> {
        let mut staged = if variant.mirrored {
            self.mirrored()?
        } else {
            self.clone()
        };
        let config = staged
            .scenario
            .players
            .get_mut(usize::from(self.defender))
            .and_then(|seat| seat.bot_config.as_mut())
            .context("defender seat is not a configured bot")?;
        config.difficulty = variant.difficulty;
        config.personality_seed = variant.seed;
        Ok(staged)
    }

    /// The scenario reflected left to right: terrain, units, structures and
    /// every tile the script names. Seats, units and structures keep their
    /// order, so every id the script names still means the same thing.
    pub fn mirrored(&self) -> Result<Self> {
        let scenario = &self.scenario;
        let columns = scenario.map.first().map_or(0, |row| row.chars().count());
        ensure!(
            scenario
                .map
                .iter()
                .chain(&scenario.levels)
                .all(|row| row.chars().count() == columns),
            "{}: map rows differ in width",
            self.name
        );
        let width = i32::try_from(columns).context("map width fits in i32")?;
        let mut mirrored = self.clone();
        mirrored.scenario.map = scenario
            .map
            .iter()
            .map(|row| mirror_row(row))
            .collect::<Result<_>>()
            .with_context(|| format!("mirroring {}", self.name))?;
        mirrored.scenario.levels = scenario
            .levels
            .iter()
            .map(|row| row.chars().rev().collect())
            .collect();
        for unit in &mut mirrored.scenario.units {
            unit.x = width - 1 - unit.x;
        }
        for building in &mut mirrored.scenario.buildings {
            building.x = width - building.kind.size().0 - building.x;
        }
        for scripted in &mut mirrored.script {
            mirror_command(&mut scripted.command, width).with_context(|| {
                format!(
                    "mirroring {}'s command at tick {}",
                    self.name, scripted.tick
                )
            })?;
        }
        Ok(mirrored)
    }
}

/// One map row reflected. A glyph that marks the top-left tile of a
/// footprint, a Foundry anchor or an Extractor frame, moves to the reflected
/// footprint's top-left tile.
fn mirror_row(row: &str) -> Result<String> {
    let glyphs: Vec<char> = row.chars().collect();
    let mut mirrored: Vec<char> = glyphs.iter().rev().copied().collect();
    for (x, glyph) in glyphs.iter().enumerate() {
        let footprint = match glyph {
            '1'..='8' | 'a'..='h' => BuildingKind::Foundry,
            'E' => BuildingKind::Extractor,
            _ => continue,
        };
        let width = usize::try_from(footprint.size().0).expect("footprints are positive");
        ensure!(
            x + width <= glyphs.len(),
            "the footprint at column {x} runs off the map"
        );
        let left = glyphs.len() - width - x;
        mirrored[left..left + width].rotate_right(1);
    }
    Ok(mirrored.into_iter().collect())
}

/// Reflects every tile `command` names on a map `width` tiles wide, refusing
/// a command whose tiles it cannot reflect.
fn mirror_command(command: &mut Command, width: i32) -> Result<()> {
    let tile = |tile: &mut TilePos| tile.x = width - 1 - tile.x;
    let anchor = |kind: BuildingKind, anchor: &mut TilePos| {
        anchor.x = width - kind.size().0 - anchor.x;
    };
    match command {
        Command::Run { goal, .. }
        | Command::Hunt { goal, .. }
        | Command::Advance { goal, .. }
        | Command::Scout { goal, .. } => tile(goal),
        Command::Harvest { node, .. } => tile(node),
        Command::Patrol { waypoints, .. } => waypoints.iter_mut().for_each(tile),
        Command::Unload { at, .. } => tile(at),
        Command::Launch { target, .. } => tile(target),
        Command::SetRally {
            rally: Some(Rally::Tile(at)),
            ..
        } => tile(at),
        Command::Build {
            kind, anchor: at, ..
        }
        | Command::CancelFound { kind, anchor: at } => anchor(*kind, at),
        Command::Attack {
            target: AttackTarget::RememberedBuilding(building),
            ..
        }
        | Command::FocusFire {
            target: AttackTarget::RememberedBuilding(building),
            ..
        } => anchor(building.building_kind, &mut building.anchor),
        Command::CancelOrder { .. } => {
            bail!("cancel_order is not mirrored: it names an order by its tiles")
        }
        Command::Attack { .. }
        | Command::FocusFire { .. }
        | Command::SetRally { .. }
        | Command::Stop { .. }
        | Command::Train { .. }
        | Command::Cancel { .. }
        | Command::Repair { .. }
        | Command::Salvage { .. }
        | Command::CancelTrain { .. }
        | Command::Surrender
        | Command::RepairUnit { .. }
        | Command::ClearFocus { .. }
        | Command::UpgradeBuilding { .. }
        | Command::Load { .. }
        | Command::ReturnCargo { .. }
        | Command::Accompany { .. }
        | Command::Retire { .. }
        | Command::OrderRound { .. }
        | Command::CancelRound { .. }
        | Command::SetAutofill { .. }
        | Command::OrderDrone { .. }
        | Command::CancelDrone { .. }
        | Command::SetDroneAutofill { .. }
        | Command::SetIdleWeld { .. } => {}
    }
    Ok(())
}

/// Runs one variant of a scenario, recording a replay when asked.
pub fn run(
    pressure: &PressureScenario,
    variant: Variant,
    record: bool,
) -> Result<(PressureOutcome, Option<GameReplay>)> {
    let pressure = pressure.staged(variant)?;
    let scenario = pressure.scenario.clone();
    let defender = PlayerId(pressure.defender);
    let attacker = PlayerId(pressure.attacker);
    let seat = scenario
        .players
        .get(usize::from(pressure.defender))
        .context("defender seat is not in the scenario")?;
    ensure!(
        seat.bot && seat.bot_config.is_some(),
        "defender seat is not a configured bot"
    );
    ensure!(
        scenario
            .players
            .get(usize::from(pressure.attacker))
            .is_some_and(|seat| !seat.bot),
        "attacker seat must be scripted, not a bot"
    );
    let mut state = scenario.build().context("building pressure scenario")?;
    let mut bots = seat_controllers(&scenario).context("building pressure controllers")?;
    let mut replay = record.then(|| {
        GameReplay::new(
            aftermine_sim::SIM_VERSION,
            crate::build_identity().label(),
            scenario.clone(),
        )
    });
    let mut script = pressure.script.clone();
    script.sort_by_key(|scripted| scripted.tick);
    let mut script = script.into_iter().peekable();
    let mut watch = Watch::new(pressure.check, &state, defender);
    while state.current_tick() < pressure.deadline && state.result().is_none() {
        let tick = state.current_tick();
        let mut commands = Vec::new();
        while let Some(scripted) = script.next_if(|scripted| scripted.tick <= tick) {
            commands.push(PlayerCommand {
                player: attacker,
                command: scripted.command,
            });
        }
        commands.extend(aftermine_kit::bot_execution::commands(&state, &mut bots));
        let report = aftermine_kit::runner::record_and_tick(&mut state, &commands, replay.as_mut());
        record_events(&mut bots, &report);
        watch.observe(&state, defender, attacker);
    }
    let (passed, detail) = watch.verdict(&state, defender, attacker)?;
    if let Some(replay) = replay.as_mut() {
        replay.meta.ticks = Some(state.current_tick());
    }
    let outcome = PressureOutcome {
        name: pressure.name.clone(),
        check: pressure.check,
        variant,
        passed,
        detail,
        ticks: state.current_tick(),
    };
    Ok((outcome, replay))
}

/// Runs every scenario under every variant, scenario by scenario, at most
/// `jobs` at once, and saves each run's replay in `replay_dir` when given.
/// Outcomes come back in that order whatever the job count.
pub fn run_all(
    scenarios: &[PressureScenario],
    variants: &[Variant],
    jobs: NonZeroUsize,
    replay_dir: Option<&Path>,
) -> Result<Vec<PressureOutcome>> {
    if let Some(dir) = replay_dir {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let runs: Vec<(&PressureScenario, Variant)> = scenarios
        .iter()
        .flat_map(|pressure| variants.iter().map(move |&variant| (pressure, variant)))
        .collect();
    crate::pool::fan_out_bounded(&runs, jobs, |&(pressure, variant)| {
        let (outcome, replay) = run(pressure, variant, replay_dir.is_some())
            .with_context(|| format!("running {} at {}", pressure.name, variant.label()))?;
        if let (Some(dir), Some(replay)) = (replay_dir, replay) {
            let path = dir.join(variant.replay_name(&pressure.name));
            replay
                .save(&path)
                .with_context(|| format!("saving {}", path.display()))?;
        }
        Ok(outcome)
    })
}

/// Facts a check gathers while the scenario runs.
struct Watch {
    check: Check,
    /// The defender's Foundries that stood built at tick zero.
    foundries: Vec<BuildingId>,
    first_air_sighting: Option<u64>,
    answered_air: Option<u64>,
    carriers: Vec<UnitId>,
    carried: Vec<UnitId>,
    landed: Vec<UnitId>,
}

impl Watch {
    fn new(check: Check, start: &State, defender: PlayerId) -> Self {
        Self {
            check,
            foundries: start
                .buildings()
                .iter()
                .filter(|building| {
                    building.player == defender
                        && building.kind == BuildingKind::Foundry
                        && building.built()
                        && building.hp > 0
                })
                .map(|building| building.id)
                .collect(),
            first_air_sighting: None,
            answered_air: None,
            carriers: Vec::new(),
            carried: Vec::new(),
            landed: Vec::new(),
        }
    }

    fn observe(&mut self, state: &State, defender: PlayerId, attacker: PlayerId) {
        let now = state.current_tick();
        match self.check {
            Check::KeepsFoundry
            | Check::SilencesArtillery
            | Check::StopsBreach
            | Check::DestroysAttackers => {}
            Check::AnswersAir { .. } => {
                if self.first_air_sighting.is_none() {
                    let seen = state.units().iter().any(|unit| {
                        unit.player == attacker
                            && unit.hp > 0
                            && unit.kind.stats().domain == Domain::Air
                            && state.sees_unit(defender, unit)
                    });
                    if seen {
                        self.first_air_sighting = Some(now);
                    }
                }
                if self.answered_air.is_none() && owns_anti_air(state, defender) {
                    self.answered_air = Some(now);
                }
            }
            Check::ClearsLanding => {
                for carrier in state.units().iter().filter(|unit| unit.player == attacker) {
                    if !carrier.cargo.is_empty() && !self.carriers.contains(&carrier.id) {
                        self.carriers.push(carrier.id);
                    }
                    for passenger in &carrier.cargo {
                        if !self.carried.contains(&passenger.id) {
                            self.carried.push(passenger.id);
                        }
                    }
                }
                for id in &self.carried {
                    if !self.landed.contains(id) && state.unit(*id).is_some_and(|unit| unit.hp > 0)
                    {
                        self.landed.push(*id);
                    }
                }
            }
        }
    }

    fn verdict(
        &self,
        state: &State,
        defender: PlayerId,
        attacker: PlayerId,
    ) -> Result<(bool, String)> {
        let has_foundry = self
            .foundries
            .iter()
            .any(|id| state.building(*id).is_some_and(|building| building.hp > 0));
        Ok(match self.check {
            Check::KeepsFoundry => (
                has_foundry,
                if has_foundry {
                    "the starting Foundry stands".into()
                } else {
                    "the starting Foundry fell".into()
                },
            ),
            Check::AnswersAir { within } => {
                let Some(seen) = self.first_air_sighting else {
                    bail!("the attacker's aircraft were never seen; the scenario is broken");
                };
                match self.answered_air {
                    Some(answered) if answered <= seen + within => (
                        true,
                        format!(
                            "anti-air by tick {answered}, {} after first sighting",
                            answered.saturating_sub(seen)
                        ),
                    ),
                    Some(answered) => (
                        false,
                        format!(
                            "anti-air only at tick {answered}, {} after first sighting",
                            answered - seen
                        ),
                    ),
                    None => (
                        false,
                        format!("no anti-air since first sighting at tick {seen}"),
                    ),
                }
            }
            Check::SilencesArtillery => {
                let shelling = state
                    .units()
                    .iter()
                    .filter(|unit| unit.player == attacker && unit.hp > 0)
                    .filter(|unit| {
                        unit.kind.stats().weapons.iter().any(|weapon| {
                            weapon.indirect
                                && state.buildings().iter().any(|building| {
                                    building.player == defender
                                        && building.hp > 0
                                        && unit.pos.dist(building.closest_point_to(unit.pos))
                                            <= weapon.range
                                })
                        })
                    })
                    .count();
                (
                    has_foundry && shelling == 0,
                    if !has_foundry {
                        "the starting Foundry fell".into()
                    } else if shelling == 0 {
                        "no artillery in range of the base".into()
                    } else {
                        format!("{shelling} artillery still in range of the base")
                    },
                )
            }
            Check::StopsBreach => {
                let breachers = state
                    .units()
                    .iter()
                    .filter(|unit| {
                        unit.player == attacker && unit.hp > 0 && unit.kind == UnitKind::Maw
                    })
                    .count();
                (
                    has_foundry && breachers == 0,
                    if !has_foundry {
                        "the starting Foundry fell".into()
                    } else if breachers == 0 {
                        "every Maw destroyed".into()
                    } else {
                        format!("{breachers} Maw still standing")
                    },
                )
            }
            Check::DestroysAttackers => {
                let standing = state
                    .units()
                    .iter()
                    .filter(|unit| unit.player == attacker && unit.hp > 0)
                    .count();
                (
                    has_foundry && standing == 0,
                    if !has_foundry {
                        "the starting Foundry fell".into()
                    } else if standing == 0 {
                        "every attacker destroyed".into()
                    } else {
                        format!("attackers still standing: {standing}")
                    },
                )
            }
            Check::ClearsLanding if self.landed.is_empty() => {
                let shot_down = !self.carriers.is_empty()
                    && self
                        .carriers
                        .iter()
                        .all(|id| state.unit(*id).is_none_or(|unit| unit.hp == 0));
                ensure!(
                    shot_down,
                    "no carried unit was ever set down; the scenario is broken"
                );
                (
                    has_foundry,
                    if has_foundry {
                        "the carrier fell before setting anyone down".into()
                    } else {
                        "the starting Foundry fell".into()
                    },
                )
            }
            Check::ClearsLanding => {
                let alive = self
                    .landed
                    .iter()
                    .filter(|id| state.unit(**id).is_some_and(|unit| unit.hp > 0))
                    .count();
                (
                    has_foundry && alive == 0,
                    if !has_foundry {
                        "the starting Foundry fell".into()
                    } else if alive == 0 {
                        format!("all {} landed units destroyed", self.landed.len())
                    } else {
                        format!("{alive} of {} landed units alive", self.landed.len())
                    },
                )
            }
        })
    }
}

/// Whether `player` owns anti-air: a unit whose job is to shoot aircraft,
/// or a completed Flak Turret.
pub(crate) fn owns_anti_air(state: &State, player: PlayerId) -> bool {
    let unit = state.units().iter().any(|unit| {
        unit.player == player
            && unit.hp > 0
            && matches!(unit.kind, UnitKind::Flakhound | UnitKind::Talon)
    });
    unit || state.buildings().iter().any(|building| {
        building.player == player && building.kind == BuildingKind::FlakTurret && building.built()
    })
}

/// A plain-text summary: each scenario's passes over its runs with a 95%
/// Wilson interval, by difficulty and by orientation, then every failing run.
pub fn report(outcomes: &[PressureOutcome]) -> String {
    let difficulties: Vec<BotDifficulty> = BotDifficulty::ALL
        .into_iter()
        .filter(|difficulty| {
            outcomes
                .iter()
                .any(|outcome| outcome.variant.difficulty == *difficulty)
        })
        .collect();
    let orientations: &[(bool, &str)] = if outcomes.iter().any(|outcome| outcome.variant.mirrored) {
        &[(false, "authored"), (true, "mirrored")]
    } else {
        &[]
    };
    let mut text = format!(
        "{:<18} {:<18} {:<7} {:<12}",
        "scenario", "check", "passed", "95% interval"
    );
    for difficulty in &difficulties {
        let _ = write!(text, " {:<9}", difficulty.as_str());
    }
    for (_, name) in orientations {
        let _ = write!(text, " {name:<9}");
    }
    text.push('\n');
    let mut names: Vec<&str> = Vec::new();
    for outcome in outcomes {
        if !names.contains(&outcome.name.as_str()) {
            names.push(&outcome.name);
        }
    }
    let count = |runs: &[&PressureOutcome]| {
        let passed = runs.iter().filter(|outcome| outcome.passed).count();
        format!("{passed}/{}", runs.len())
    };
    for name in &names {
        let runs: Vec<&PressureOutcome> = outcomes
            .iter()
            .filter(|outcome| outcome.name == *name)
            .collect();
        let passed = u32::try_from(runs.iter().filter(|outcome| outcome.passed).count())
            .expect("run counts fit in u32");
        let total = u32::try_from(runs.len()).expect("run counts fit in u32");
        let [low, high] = crate::bot_ladder::wilson(passed, total);
        let _ = write!(
            text,
            "{:<18} {:<18} {:<7} {:<12}",
            name,
            runs[0].check.name(),
            count(&runs),
            format!("[{:.0}-{:.0}%]", low * 100.0, high * 100.0)
        );
        for difficulty in &difficulties {
            let slice: Vec<&PressureOutcome> = runs
                .iter()
                .copied()
                .filter(|outcome| outcome.variant.difficulty == *difficulty)
                .collect();
            let _ = write!(text, " {:<9}", count(&slice));
        }
        for (mirrored, _) in orientations {
            let slice: Vec<&PressureOutcome> = runs
                .iter()
                .copied()
                .filter(|outcome| outcome.variant.mirrored == *mirrored)
                .collect();
            let _ = write!(text, " {:<9}", count(&slice));
        }
        text.push('\n');
    }
    let failing: Vec<&PressureOutcome> =
        outcomes.iter().filter(|outcome| !outcome.passed).collect();
    if !failing.is_empty() {
        text.push_str("\nfailing runs\n");
        for outcome in &failing {
            let _ = writeln!(
                text,
                "{:<18} {:<28} {:>5}  {}",
                outcome.name,
                outcome.variant.label(),
                outcome.ticks,
                outcome.detail
            );
        }
    }
    let passed = outcomes.iter().filter(|outcome| outcome.passed).count();
    let _ = writeln!(text, "\n{passed}/{} runs passed", outcomes.len());
    text
}

#[cfg(test)]
mod tests;
