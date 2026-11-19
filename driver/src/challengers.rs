//! Scripted players that each play one known way to beat the bot, so the bot
//! can be measured against those plans cheaply across maps and seeds.
//!
//! A [`Challenger`] plays one seat of one [`Family`]. It decides from the
//! seat's fog-honest observation and the map's public facts only, at most
//! every [`DECISION_TICKS`] ticks and a handful of commands at a time, and
//! keeps only small phase memory, so it can take over a seat from any state.
//! Its seed varies timings, counts, unit mix, routes and targets within the
//! family's plan, through `chassis::rng::Pcg32`; the same scenario, seed and
//! opponent always give the same commands.

#![deny(clippy::float_arithmetic, clippy::disallowed_types)]

mod ball_rush;
mod bombard_siege;
mod buzzard_harass;
mod forward_fabricator;
mod held_army;
mod kestrel_push;
mod lancer_harass;
mod map;
mod novice;
mod scuttler_raid;
mod tactics;
mod turn;

use aftermine_sim::observation::ObservationData;
use aftermine_sim::{PlayerCommand, PlayerId, Scenario, State, UnitId};
use anyhow::{Context, Result, bail};
use chassis::rng::Pcg32;
use map::MapFacts;
use serde::Serialize;
use turn::{Orders, Turn};

/// Ticks between a challenger's decisions: half a second.
pub const DECISION_TICKS: u64 = 10;

/// A plan for beating the bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Family {
    /// A Fabricator built on the approach to the enemy Foundry, Lancers
    /// trained there, and an early attack on what is near.
    ForwardFabricator,
    /// Scuttler groups that hunt the enemy's Harvesters while the economy
    /// grows, then a main attack.
    ScuttlerRaid,
    /// Lancer pairs and threes that raid harvest lines and Extractors, then
    /// a main push.
    LancerHarass,
    /// A strong economy, a mixed army and home defenses, held until a set
    /// time and then sent at the enemy Foundry with reinforcements.
    HeldArmy,
    /// An aircraft opening: Buzzard groups raid harvest lines and
    /// Extractors while ground units only hold home.
    BuzzardHarass,
    /// Bombards with a Kestrel spotter and an escort, shelling defenses and
    /// Foundries from outside their reach.
    BombardSiege,
    /// A Sentinel and Scuttler ball that attacks at about three minutes.
    BallRush,
    /// A Lancer push spotted by a Kestrel at about five minutes.
    KestrelPush,
    /// A new player: a thin, slow economy, a small mixed army and one late
    /// attack. The lower rungs' yardstick, never a tuning target.
    Novice,
}

impl Family {
    /// Every family, tuning families first.
    pub const ALL: [Family; 9] = [
        Family::ForwardFabricator,
        Family::ScuttlerRaid,
        Family::LancerHarass,
        Family::HeldArmy,
        Family::BuzzardHarass,
        Family::BombardSiege,
        Family::BallRush,
        Family::KestrelPush,
        Family::Novice,
    ];

    /// The families the bot may be tuned against.
    pub const TUNING: [Family; 6] = [
        Family::ForwardFabricator,
        Family::ScuttlerRaid,
        Family::LancerHarass,
        Family::HeldArmy,
        Family::BuzzardHarass,
        Family::BombardSiege,
    ];

    /// The families kept out of tuning, to check that a change generalizes.
    pub const HELD_OUT: [Family; 2] = [Family::BallRush, Family::KestrelPush];

    /// Weaker players the lower rungs are measured against, never tuned on.
    pub const YARDSTICK: [Family; 1] = [Family::Novice];

    /// The kebab-case name.
    pub fn name(self) -> &'static str {
        match self {
            Family::ForwardFabricator => "forward-fabricator",
            Family::ScuttlerRaid => "scuttler-raid",
            Family::LancerHarass => "lancer-harass",
            Family::HeldArmy => "held-army",
            Family::BuzzardHarass => "buzzard-harass",
            Family::BombardSiege => "bombard-siege",
            Family::BallRush => "ball-rush",
            Family::KestrelPush => "kestrel-push",
            Family::Novice => "novice",
        }
    }

    /// Whether the bot may be tuned against this family.
    pub fn held_out(self) -> bool {
        Family::HELD_OUT.contains(&self)
    }
}

impl std::fmt::Display for Family {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for Family {
    type Err = anyhow::Error;

    fn from_str(name: &str) -> Result<Self> {
        match Family::ALL.iter().find(|family| family.name() == name) {
            Some(family) => Ok(*family),
            None => bail!(
                "unknown family {name:?}; expected one of {}",
                Family::ALL.map(Family::name).join(", ")
            ),
        }
    }
}

/// A family's seeded parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Plan {
    ForwardFabricator(forward_fabricator::Params),
    ScuttlerRaid(scuttler_raid::Params),
    LancerHarass(lancer_harass::Params),
    HeldArmy(held_army::Params),
    BuzzardHarass(buzzard_harass::Params),
    BombardSiege(bombard_siege::Params),
    BallRush(ball_rush::Params),
    KestrelPush(kestrel_push::Params),
    Novice(novice::Params),
}

/// The small phase memory a plan keeps between decisions. Everything else
/// it recomputes from the observation, so a fresh memory on a game in
/// progress is a sound start.
#[derive(Debug, Clone, Default)]
struct Memory {
    /// Whether the main force is out attacking.
    attacking: bool,
    /// The worker sent to build away from home.
    builder: Option<UnitId>,
    /// The units out on the current raid.
    raid: Vec<UnitId>,
    /// Raids sent so far, which picks each raid's target.
    raids: u32,
    /// Forward builders lost on the way, each pulling the next spot back.
    setbacks: u32,
    /// The units kept home to guard it.
    guards: Vec<UnitId>,
    /// Attacks that fell back to regroup, each of which makes the next wait
    /// for a bigger force.
    waves: u32,
    /// Whether the current raid is running home.
    fleeing: bool,
}

/// One scripted seat.
#[derive(Debug, Clone)]
pub struct Challenger {
    family: Family,
    player: PlayerId,
    seed: u64,
    map: MapFacts,
    plan: Plan,
    memory: Memory,
    orders: Orders,
    next_decision: u64,
}

impl Challenger {
    /// A challenger of `family` for `player` on `scenario`, its plan drawn
    /// from `seed`.
    pub fn new(family: Family, player: PlayerId, seed: u64, scenario: &Scenario) -> Result<Self> {
        let map = MapFacts::new(scenario, player)
            .with_context(|| format!("briefing a {family} challenger on {}", scenario.name))?;
        let mut rng = Pcg32::new(seed, u64::from(family as u8) + 1);
        let plan = match family {
            Family::ForwardFabricator => {
                Plan::ForwardFabricator(forward_fabricator::Params::draw(&mut rng))
            }
            Family::ScuttlerRaid => Plan::ScuttlerRaid(scuttler_raid::Params::draw(&mut rng)),
            Family::LancerHarass => Plan::LancerHarass(lancer_harass::Params::draw(&mut rng)),
            Family::HeldArmy => Plan::HeldArmy(held_army::Params::draw(&mut rng)),
            Family::BuzzardHarass => Plan::BuzzardHarass(buzzard_harass::Params::draw(&mut rng)),
            Family::BombardSiege => Plan::BombardSiege(bombard_siege::Params::draw(&mut rng)),
            Family::BallRush => Plan::BallRush(ball_rush::Params::draw(&mut rng)),
            Family::KestrelPush => Plan::KestrelPush(kestrel_push::Params::draw(&mut rng)),
            Family::Novice => Plan::Novice(novice::Params::draw(&mut rng)),
        };
        Ok(Self {
            family,
            player,
            seed,
            map,
            plan,
            memory: Memory::default(),
            orders: Orders::default(),
            next_decision: 0,
        })
    }

    /// The family this challenger plays.
    pub fn family(&self) -> Family {
        self.family
    }

    /// The seat it plays.
    pub fn player(&self) -> PlayerId {
        self.player
    }

    /// The seed its plan was drawn from.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// The seeded parameters, for reports and tests.
    pub fn parameters(&self) -> String {
        match &self.plan {
            Plan::ForwardFabricator(p) => format!("{p:?}"),
            Plan::ScuttlerRaid(p) => format!("{p:?}"),
            Plan::LancerHarass(p) => format!("{p:?}"),
            Plan::HeldArmy(p) => format!("{p:?}"),
            Plan::BuzzardHarass(p) => format!("{p:?}"),
            Plan::BombardSiege(p) => format!("{p:?}"),
            Plan::BallRush(p) => format!("{p:?}"),
            Plan::KestrelPush(p) => format!("{p:?}"),
            Plan::Novice(p) => format!("{p:?}"),
        }
    }

    /// Commands for this tick: a decision when one is due, nothing between.
    pub fn act(&mut self, state: &State) -> Vec<PlayerCommand> {
        let tick = state.current_tick();
        if tick < self.next_decision || !state.accepts_commands(self.player) {
            return Vec::new();
        }
        self.next_decision = tick + DECISION_TICKS;
        let obs = ObservationData::fog_honest(state, self.player);
        let mut turn = Turn::new(state, &obs, &self.map, &mut self.orders);
        let memory = &mut self.memory;
        match &self.plan {
            Plan::ForwardFabricator(p) => forward_fabricator::decide(&mut turn, p, memory),
            Plan::ScuttlerRaid(p) => scuttler_raid::decide(&mut turn, p, memory),
            Plan::LancerHarass(p) => lancer_harass::decide(&mut turn, p, memory),
            Plan::HeldArmy(p) => held_army::decide(&mut turn, p, memory),
            Plan::BuzzardHarass(p) => buzzard_harass::decide(&mut turn, p, memory),
            Plan::BombardSiege(p) => bombard_siege::decide(&mut turn, p, memory),
            Plan::BallRush(p) => ball_rush::decide(&mut turn, p, memory),
            Plan::KestrelPush(p) => kestrel_push::decide(&mut turn, p, memory),
            Plan::Novice(p) => novice::decide(&mut turn, p, memory),
        }
        turn.harvest_idle();
        turn.finish()
            .into_iter()
            .map(|command| PlayerCommand {
                player: self.player,
                command,
            })
            .collect()
    }
}

/// A seeded whole number from `low` to `high` inclusive.
fn between(rng: &mut Pcg32, low: u32, high: u32) -> u32 {
    low + rng.next_below(high - low + 1)
}

/// One of `options`, seeded.
fn pick<T: Copy>(rng: &mut Pcg32, options: &[T]) -> T {
    options[rng.next_index(options.len())]
}

#[cfg(test)]
mod tests;
