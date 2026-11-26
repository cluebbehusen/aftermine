//! One decision: the precedence it follows, its running total and its
//! unit-order allowance.

use crate::composition::Fighter;
use crate::composition::{self, Needs};
use crate::counters::{Piece, weigh};
use crate::defenses;
use crate::events::OwnEvent;
use crate::frame::{HomeFrame, footprint_centre, gap};
use crate::income::Income;
use crate::investments::{self, Candidate, Investment, Situation, Step};
use crate::map::MapModel;
use crate::memory::Memory;
use crate::missions::{Missions, Scratch, Settlement, Shortfall};
use crate::placement;
use crate::profile::ResolvedProfile;
use crate::saving::Saving;
use crate::trace::{ArmyReasons, NextPurchase, Pick, PickPath, Purchase, SavingTarget};
use crate::workers;
use aftermine_sim::observation::{BuildingObs, ObservationData, UnitObs};
use aftermine_sim::scenario::{BotDifficulty, BotStance};
use aftermine_sim::stats::Domain;
use aftermine_sim::{BuildingId, BuildingKind, Command, PlayerCommand, PlayerId, UnitId, UnitKind};
use chassis::grid::TilePos;
use std::cmp::Reverse;

/// What one decision emits.
#[derive(Default)]
pub(crate) struct Decision {
    pub(crate) commands: Vec<PlayerCommand>,
    /// Attacks launched, for hosts; nothing reads them back.
    pub(crate) launches: Vec<crate::missions::Launch>,
    pub(crate) spent: u32,
    pub(crate) purchases: Vec<Purchase>,
    pub(crate) unit_orders: u32,
    pub(crate) allowance: u32,
    pub(crate) target: Option<SavingTarget>,
    pub(crate) protected: u32,
    /// Why army units were chosen, recorded only for a traced decision.
    pub(crate) army: Option<ArmyReasons>,
}

/// What carries from one decision to the next. Checkpoints save exactly this.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Persistent {
    pub(crate) memory: Memory,
    pub(crate) income: Income,
    pub(crate) saving: Saving,
    pub(crate) missions: Missions,
}

/// The decision's running total and unit-order allowance. It lives only while
/// one decision runs.
pub(crate) struct Ledger {
    me: PlayerId,
    bank: u32,
    protected: u32,
    planned: Vec<(BuildingKind, TilePos)>,
    /// Own units sitting out orders after stalling for want of a route or a
    /// firing position, by id.
    stuck: Vec<UnitId>,
    /// Own workers a mission holds, sorted by id.
    held: Vec<UnitId>,
    /// Wounded own units at or sent to a Repair Bay this decision, by id.
    docked: Vec<UnitId>,
    /// Units a defense holds at its waiting post this decision, by id.
    reserved: Vec<UnitId>,
    /// Own units stepping out of a crusher's way this decision, by id.
    aside: Vec<UnitId>,
    decision: Decision,
}

impl Ledger {
    pub(crate) fn new(me: PlayerId, bank: u32, allowance: u32) -> Self {
        Self {
            me,
            bank,
            protected: 0,
            planned: Vec::new(),
            stuck: Vec::new(),
            held: Vec::new(),
            docked: Vec::new(),
            reserved: Vec::new(),
            aside: Vec::new(),
            decision: Decision {
                allowance,
                ..Decision::default()
            },
        }
    }

    /// Scrap this decision has not committed.
    pub(crate) fn available(&self) -> u32 {
        self.bank - self.decision.spent
    }

    /// Records why an army unit was queued. `pick` runs only in a traced
    /// decision, so an untraced one never scores candidates for it.
    fn explain(&mut self, pick: impl FnOnce() -> Pick) {
        if let Some(army) = self.decision.army.as_mut() {
            army.picks.push(pick());
        }
    }

    /// Uncommitted scrap that is not protected for the saving target.
    pub(crate) fn spendable(&self) -> u32 {
        let available = self.available();
        available - self.protected.min(available)
    }

    /// Queues `kind` at `building` from unprotected scrap.
    pub(crate) fn train(&mut self, building: BuildingId, kind: UnitKind) -> bool {
        self.spendable() >= kind.stats().cost && self.train_from_all(building, kind)
    }

    /// Queues `kind` at `building`, spending protected scrap if it must.
    pub(crate) fn train_urgently(&mut self, building: BuildingId, kind: UnitKind) -> bool {
        self.available() >= kind.stats().cost && self.train_from_all(building, kind)
    }

    fn train_from_all(&mut self, building: BuildingId, kind: UnitKind) -> bool {
        self.decision.spent += kind.stats().cost;
        self.decision.purchases.push(Purchase::Train {
            building,
            unit: kind,
        });
        self.push(Command::Train { building, kind });
        true
    }

    pub(crate) fn build(
        &mut self,
        builder: UnitId,
        kind: BuildingKind,
        anchor: TilePos,
        defer: bool,
        price: u32,
    ) {
        self.decision.spent += price;
        self.decision.purchases.push(Purchase::Build {
            building: kind,
            anchor,
        });
        self.planned.push((kind, anchor));
        self.push(Command::Build {
            units: vec![builder],
            kind,
            anchor,
            queue: false,
            defer,
        });
    }

    pub(crate) fn upgrade(&mut self, building: BuildingId, price: u32) {
        self.decision.spent += price;
        self.decision.purchases.push(Purchase::Upgrade { building });
        self.push(Command::UpgradeBuilding { building });
    }

    fn round(&mut self, building: BuildingId, price: u32) {
        self.decision.spent += price;
        self.decision.purchases.push(Purchase::Round { building });
        self.push(Command::OrderRound { building });
    }

    /// Reports an attack this decision launched.
    pub(crate) fn launched(&mut self, launch: crate::missions::Launch) {
        self.decision.launches.push(launch);
    }

    /// Whether the allowance has room for another unit order.
    pub(crate) fn can_order(&self) -> bool {
        self.decision.unit_orders < self.decision.allowance
    }

    /// Issues one unit order while the allowance lasts, leaving out units
    /// sitting out orders. An order left with no units issues nothing and
    /// counts as given. Purchases never count against the allowance.
    pub(crate) fn order(&mut self, mut command: Command) -> bool {
        if self.decision.unit_orders == self.decision.allowance {
            return false;
        }
        if let Some(units) = units_of(&mut command) {
            units.retain(|unit| !self.stuck(*unit) && !self.aside(*unit));
            if units.is_empty() {
                return true;
            }
        }
        self.decision.unit_orders += 1;
        self.push(command);
        true
    }

    /// Scraps an own unfinished `building`. It moves no unit, so like a
    /// purchase it never counts against the allowance.
    pub(crate) fn cancel(&mut self, building: BuildingId) {
        self.push(Command::Cancel { building });
    }

    /// Issues every launch of one volley as a single unit order while the
    /// allowance lasts.
    pub(crate) fn salvo(&mut self, launches: impl IntoIterator<Item = Command>) {
        if self.decision.unit_orders == self.decision.allowance {
            return;
        }
        self.decision.unit_orders += 1;
        for launch in launches {
            self.push(launch);
        }
    }

    /// Whether `unit` sits out orders after stalling for want of a route or a
    /// firing position.
    pub(crate) fn stuck(&self, unit: UnitId) -> bool {
        self.stuck.binary_search(&unit).is_ok()
    }

    /// Leaves `unit`, sent out of a crusher's way, out of every later order
    /// this decision.
    pub(crate) fn step_aside(&mut self, unit: UnitId) {
        if let Err(index) = self.aside.binary_search(&unit) {
            self.aside.insert(index, unit);
        }
    }

    /// Whether `unit` steps out of a crusher's way this decision.
    pub(crate) fn aside(&self, unit: UnitId) -> bool {
        self.aside.binary_search(&unit).is_ok()
    }

    /// Whether `unit` was sent running this decision.
    pub(crate) fn running(&self, unit: UnitId) -> bool {
        self.decision.commands.iter().any(|command| {
            matches!(&command.command, Command::Run { units, queue: false, .. } if units.contains(&unit))
        })
    }

    /// Keeps `unit` at a Repair Bay for the rest of this decision: no
    /// mission or welder takes it.
    pub(crate) fn dock(&mut self, unit: UnitId) {
        if let Err(index) = self.docked.binary_search(&unit) {
            self.docked.insert(index, unit);
        }
    }

    /// Whether `unit` is at or bound for a Repair Bay this decision.
    pub(crate) fn docked(&self, unit: UnitId) -> bool {
        self.docked.binary_search(&unit).is_ok()
    }

    /// Keeps `units` waiting at a defense's post for the rest of this
    /// decision: they join no mission, so no later mission takes them.
    pub(crate) fn reserve(&mut self, units: &[UnitId]) {
        for unit in units {
            if let Err(index) = self.reserved.binary_search(unit) {
                self.reserved.insert(index, *unit);
            }
        }
    }

    /// Whether a defense holds `unit` at its waiting post this decision.
    pub(crate) fn reserved(&self, unit: UnitId) -> bool {
        self.reserved.binary_search(&unit).is_ok()
    }

    /// Unit orders this decision may still issue.
    pub(crate) fn room(&self) -> u32 {
        self.decision.allowance - self.decision.unit_orders
    }

    /// Footprints this decision already committed to.
    pub(crate) fn planned(&self) -> &[(BuildingKind, TilePos)] {
        &self.planned
    }

    /// Whether this decision already queued something at `building`.
    pub(crate) fn queued_at(&self, building: BuildingId) -> bool {
        self.decision.purchases.iter().any(
            |purchase| matches!(purchase, Purchase::Train { building: at, .. } if *at == building),
        )
    }

    /// Units this decision queued at `building`.
    pub(crate) fn trained_at(&self, building: BuildingId) -> impl Iterator<Item = UnitKind> + '_ {
        self.decision
            .purchases
            .iter()
            .filter_map(move |purchase| match purchase {
                Purchase::Train { building: at, unit } if *at == building => Some(*unit),
                _ => None,
            })
    }

    /// Units of `kind` this decision queued.
    pub(crate) fn queued(&self, kind: UnitKind) -> usize {
        self.decision
            .purchases
            .iter()
            .filter(|purchase| matches!(purchase, Purchase::Train { unit, .. } if *unit == kind))
            .count()
    }

    /// Marks `workers` as held by missions, replacing those marked before:
    /// ordinary worker orders leave them be.
    pub(crate) fn hold(&mut self, mut workers: Vec<UnitId>) {
        workers.sort_unstable();
        self.held = workers;
    }

    /// Whether this decision already gave `unit` work, or a mission holds it.
    pub(crate) fn employs(&self, unit: UnitId) -> bool {
        self.held.binary_search(&unit).is_ok()
            || self
                .decision
                .commands
                .iter()
                .any(|command| match &command.command {
                    Command::Harvest { units, .. }
                    | Command::Build { units, .. }
                    | Command::Run { units, .. }
                    | Command::Repair { units, .. }
                    | Command::RepairUnit { units, .. }
                    | Command::Salvage { units, .. }
                    | Command::ReturnCargo { units, .. }
                    | Command::Scout { units, .. } => units.contains(&unit),
                    _ => false,
                })
    }

    fn push(&mut self, command: Command) {
        self.decision.commands.push(PlayerCommand {
            player: self.me,
            command,
        });
    }
}

/// The units `command` orders, when it orders units.
fn units_of(command: &mut Command) -> Option<&mut Vec<UnitId>> {
    match command {
        Command::Run { units, .. }
        | Command::Attack { units, .. }
        | Command::Hunt { units, .. }
        | Command::Harvest { units, .. }
        | Command::Patrol { units, .. }
        | Command::Stop { units, .. }
        | Command::Build { units, .. }
        | Command::Repair { units, .. }
        | Command::Salvage { units, .. }
        | Command::RepairUnit { units, .. }
        | Command::Advance { units, .. }
        | Command::Load { units, .. }
        | Command::ReturnCargo { units, .. }
        | Command::Scout { units, .. } => Some(units),
        _ => None,
    }
}

/// A built own producer, and whether its queue runs out before the next
/// decision, so a unit queued now keeps it working. Every unit trains for
/// longer than a decision interval, so one unit a decision keeps a ready
/// producer busy.
#[derive(Clone, Copy)]
pub(crate) struct Producer<'a> {
    pub(crate) building: &'a BuildingObs,
    pub(crate) ready: bool,
}

/// Defense first, then worker recovery, then an affordable saving target
/// unless a defense is short, then the wounded to Repair Bays and kilns, then
/// workers, who also weld the wounded, then island settlements, local guards,
/// lifts, attacks, anti-air clearing, strikes, raids, Spire launches, focus
/// fire and scouting, then production: carriers for a severed seat and the
/// Sappers attacks want first, then the army's roles, then scouts queued
/// behind whatever their producer has and Scuttlers on the producers the army
/// leaves, then ground units on any still idle. A short defense instead keeps
/// the price of its best answer from an emergency static defense, raises that
/// answer's role ahead of the army's others, or trains the answer itself when
/// no role holds it, and trains no Harvester beyond worker recovery or scouts.
/// A short local guard raises or trains its answer the same way.
#[expect(
    clippy::too_many_lines,
    reason = "one decision in its documented priority order"
)]
pub(crate) fn decide(
    observation: &ObservationData,
    events: &[OwnEvent],
    map: &MapModel,
    frame: HomeFrame,
    profile: &ResolvedProfile,
    persistent: &mut Persistent,
    traced: bool,
) -> Decision {
    let rejected = events
        .iter()
        .any(|event| matches!(event, OwnEvent::CommandRejected { .. }));
    persistent.memory.stalls(observation, events);
    workers::unreached(observation, events, &mut persistent.memory);
    let mut ledger = Ledger::new(
        observation.me,
        observation.scrap,
        allowance(profile.difficulty),
    );
    ledger.decision.army = traced.then(ArmyReasons::default);
    ledger.stuck = persistent.memory.stuck();
    let tick = observation.tick;
    let producers = producers(
        observation,
        frame,
        crate::decision_interval(profile.difficulty),
    );
    let foundries: Vec<Producer<'_>> = producers
        .iter()
        .copied()
        .filter(|producer| producer.building.kind == BuildingKind::Foundry)
        .collect();
    let earned = persistent.income.observe(tick, observation.scrap, rejected);
    persistent.memory.forget(tick);
    persistent.memory.lost_extractors(observation);
    let home = map
        .start(observation.me)
        .and_then(|start| map.component(start));
    let still = persistent.memory.standing(observation);
    persistent.memory.assembled(observation, events, |owner| {
        map.hostile(observation.me, owner)
    });
    persistent.memory.observe_stranded(observation, |unit| {
        crate::missions::stranded(unit, map, home)
    });
    crate::missions::look(observation, map, &mut persistent.memory);
    // A setting on own buildings spends nothing and is no unit order.
    for command in crate::defenses::autofill(observation) {
        ledger.push(command);
    }
    crate::crush::step_aside(
        observation,
        map,
        frame,
        crate::decision_interval(profile.difficulty),
        &mut ledger,
    );
    let mut scratch = Scratch::new(
        observation,
        map,
        frame,
        &persistent.memory,
        profile.stance,
        profile.traits,
        &persistent.missions,
    );
    scratch.gunships_clear =
        persistent
            .missions
            .gunships_clear(observation, map, profile, &persistent.memory, &scratch);
    let scratch = scratch;
    let meals = persistent
        .missions
        .meals(observation, map, &scratch, &ledger);
    let staffing = workers::staffing(
        observation,
        map,
        frame,
        profile,
        (&foundries, &meals),
        &scratch,
    );
    let airworks = crate::investments::built(observation, BuildingKind::Airworks);
    let income = persistent.income.per_minute();
    // A seat whose ground reaches no enemy delivers ground units only by
    // lift, so until an Airworks stands its army is aircraft, and line units
    // only against invaders already on its ground.
    let outlet = composition::Outlet {
        ground: !scratch.severed || airworks,
        lifted: scratch.severed,
        invaders: scratch.invaders,
        air_strikes: airworks || scratch.severed,
        strike: if scratch.severed {
            crate::missions::strike_need(observation, &persistent.memory, profile, &scratch).max(
                persistent.missions.clear_need(
                    observation,
                    map,
                    profile,
                    &persistent.memory,
                    &scratch,
                ),
            )
        } else {
            0
        },
    };
    let mut needs = composition::needs(
        observation,
        &persistent.memory,
        profile.traits,
        profile.stance,
        income,
        outlet,
    );
    if let Some(army) = ledger.decision.army.as_mut() {
        needs.describe_enemy(army);
    }
    // A lift carries at least the stance's minimum army, so until the seat has
    // that much a lift could take it neither pulls toward an Airworks nor holds
    // production for carriers: an army and home defense come first.
    let minimum = crate::missions::minimum(profile.stance);
    let exposed = army(observation) < minimum;
    let carryable = scratch.payload >= minimum;
    let lift = carryable && scratch.severed;
    let mut pull = needs.pull(observation);
    if lift {
        pull.push((BuildingKind::Airworks, LIFT_PULL));
    }
    let units = needs.premium(
        observation,
        observation
            .scrap
            .saturating_sub(persistent.saving.protected()),
    );
    // A saved unit whose role no longer lacks it is no reason for another
    // producer.
    let wanted = needs.wanted();
    let waiting = match persistent.saving.investment() {
        Some(Investment::Unit(kind))
            if observation.scrap >= kind.stats().cost
                && composition::role(kind).is_some_and(|role| wanted.contains(&role)) =>
        {
            let ready = producers.iter().any(|producer| {
                producer.ready
                    && investments::producers_of(observation, kind)
                        .any(|building| building.id == producer.building.id)
            });
            investments::producers_of(observation, kind)
                .next()
                .filter(|_| !ready)
                .map(|building| building.kind)
        }
        _ => None,
    };
    let situation = Situation {
        wanted,
        observation,
        map,
        frame,
        memory: &persistent.memory,
        traits: profile.traits,
        saturation: staffing.saturation(),
        income,
        depletion: depletion(observation, map),
        pull,
        exposed,
        stakes: defenses::Stakes::of(profile),
        severed: scratch.severed,
        units,
        waiting,
        margin: crate::missions::margin(profile.difficulty),
        economy: crate::economy(profile.difficulty),
        plans_bores: crate::plans_deep_bores(profile.difficulty),
        lift_flak: persistent.missions.lift_flak(
            observation,
            map,
            profile,
            &persistent.memory,
            &scratch,
        ),
        air_share: needs.air_share(),
    };
    let mut candidates = investments::candidates(&situation);
    let danger = placement::Danger::of(observation, map, &persistent.memory);
    // Units healing at a calm Repair Bay sit out every mission, so an island
    // settlement's next purchase counts its escort, and the carriers that
    // escort needs, without them, as the settlement will recruit it.
    for unit in persistent.missions.healing(observation, &scratch, &ledger) {
        ledger.dock(unit);
    }
    sited(
        &Settlement::new(
            observation,
            map,
            profile,
            &persistent.memory,
            &danger,
            &scratch,
        ),
        &persistent.missions,
        &mut candidates,
        persistent.saving.investment(),
        &ledger,
    );
    let share = share(observation, profile);
    persistent.saving.settle(
        observation,
        &candidates,
        share,
        earned,
        &mut persistent.memory,
    );
    ledger.protected = persistent.saving.protected();
    let target = persistent
        .saving
        .investment()
        .map(|investment| SavingTarget {
            investment,
            next: candidates
                .iter()
                .find(|candidate| candidate.investment == investment)
                .and_then(|candidate| candidate.next)
                .map(|(step, price)| NextPurchase { step, price }),
        });
    ledger.decision.target = target;
    persistent
        .missions
        .prune(observation, &mut persistent.memory);
    let unseen = placement::unexplained(observation, map);
    let shortfalls = persistent.missions.defend(
        observation,
        map,
        frame,
        (&persistent.memory, &unseen),
        profile.difficulty,
        &mut ledger,
    );
    let short = !shortfalls.is_empty();
    ledger.hold(persistent.missions.held_workers(observation));
    workers::recover(observation, &foundries, &mut ledger);
    if short {
        // The best answer to a shortfall, when it is a unit, is kept from
        // emergency structures so production can train it. With no answer
        // that pays back, the scrap kept for the saving target stays kept:
        // spent on what the attackers outfight, it only feeds them.
        if let Some(kept) = awaited(
            observation,
            map,
            &producers,
            &shortfalls,
            income,
            ledger.available(),
        ) {
            ledger.protected = kept;
        }
        defenses::emergency(
            observation,
            map,
            frame,
            &persistent.memory,
            defenses::Stakes::of(profile),
            &mut ledger,
        );
    } else {
        buy(
            observation,
            map,
            frame,
            &producers,
            persistent,
            &mut needs,
            &mut ledger,
        );
    }
    // A short defense leaves scrap to the army: only the recovery Harvester
    // above is trained while it lasts. Until the army reaches the stance
    // minimum, workers that would cost more than it take only what the army
    // leaves, after production.
    let pace = if exposed {
        army(observation).saturating_sub(workforce(observation))
    } else {
        u64::MAX
    };
    if !short {
        workers::train(
            observation,
            map,
            &foundries,
            &staffing,
            profile.traits.greed,
            pace,
            &mut ledger,
        );
    }
    persistent
        .missions
        .repair(observation, map, &scratch, &mut ledger);
    Missions::feed(observation, &scratch, &meals, &mut ledger);
    let patients = persistent.missions.patients(observation, &ledger);
    let unseen: Vec<BuildingId> = unseen.iter().map(|building| building.id).collect();
    workers::run(
        observation,
        map,
        frame,
        &persistent.memory,
        (&scratch.ground, &unseen),
        &staffing,
        &patients,
        profile.traits.support,
        &mut ledger,
    );
    // Settlements move before guards so an escort let go at its site joins
    // the site's guard in the same decision; a new settlement recruits only
    // after the guards have.
    let settlement = Settlement::new(
        observation,
        map,
        profile,
        &persistent.memory,
        &danger,
        &scratch,
    );
    let failures = persistent
        .missions
        .advance_settlements(&settlement, &mut ledger);
    let guard_gaps = persistent.missions.guard(
        observation,
        map,
        &scratch,
        &persistent.memory,
        profile.stance,
        &mut ledger,
    );
    if !short {
        persistent
            .missions
            .settle(&settlement, persistent.saving.investment(), &mut ledger);
    }
    ledger.hold(persistent.missions.held_workers(observation));
    for (kind, anchor) in failures {
        persistent.memory.fail(kind, anchor, tick);
    }
    if let Some((kind, anchor)) = persistent.missions.lift(
        observation,
        map,
        profile,
        &persistent.memory,
        &scratch,
        &mut ledger,
    ) {
        persistent.memory.abandon(kind, anchor, tick);
    }
    for (kind, anchor) in persistent.missions.attack(
        observation,
        map,
        profile,
        &persistent.memory,
        &scratch,
        &mut ledger,
    ) {
        persistent.memory.abandon(kind, anchor, tick);
    }
    persistent.missions.clear_anti_air(
        observation,
        map,
        profile,
        &persistent.memory,
        &scratch,
        &mut ledger,
    );
    for (kind, anchor) in persistent.missions.strike(
        observation,
        map,
        profile,
        &persistent.memory,
        &scratch,
        &mut ledger,
    ) {
        persistent.memory.abandon(kind, anchor, tick);
    }
    for (kind, anchor) in persistent.missions.raid(
        observation,
        map,
        profile,
        &persistent.memory,
        &scratch,
        &mut ledger,
    ) {
        persistent.memory.raid(kind, anchor, tick);
    }
    crate::warheads::fire(
        observation,
        map,
        frame,
        profile.difficulty,
        &still,
        &mut ledger,
    );
    persistent
        .missions
        .focus(observation, map, frame, profile.difficulty, &mut ledger);
    persistent
        .missions
        .scouts_stopped(events, &mut persistent.memory, tick);
    // An army big enough to attack that knows no target has lost track of
    // the enemy, and the scouts search for it.
    let lost = army(observation) >= crate::missions::minimum(profile.stance);
    let lacking = persistent.missions.scout(
        observation,
        (map, frame),
        &mut persistent.memory,
        &scratch,
        lost,
        &mut ledger,
    );
    let saving = match persistent.saving.investment() {
        Some(Investment::Unit(kind)) => Some(kind),
        _ => None,
    };
    // Producers that train the unit the seat saves for wait for it, so no
    // army unit queues there first. Carriers and scouts, small stocks, still
    // draw on them: a seat saving for a Condor would otherwise lift nothing
    // and look at nothing until it bought one.
    let free: Vec<Producer<'_>> = producers
        .iter()
        .copied()
        .filter(|producer| !saved_for(observation, producer, saving))
        .collect();
    // Local guards rank ahead of lifts, so their gaps train before a lift
    // holds production for carriers.
    let reach = u64::from(ledger.available()) + soon(income);
    let guard_gaps = by_role(
        (observation, map, &producers),
        guard_gaps,
        reach,
        &emergency_raised(&ledger),
        &mut needs,
    );
    arm(
        observation,
        map,
        &producers,
        guard_gaps,
        income,
        &mut needs,
        &mut ledger,
    );
    // A short defense trains next, so its answer gets the producer before
    // general production queues something else there. What it spends of the
    // scrap kept for its answer is kept no longer.
    if short {
        let before = ledger.available();
        let reach = u64::from(before) + soon(income);
        let shortfalls = by_role(
            (observation, map, &producers),
            shortfalls,
            reach,
            &emergency_raised(&ledger),
            &mut needs,
        );
        arm(
            observation,
            map,
            &producers,
            shortfalls,
            income,
            &mut needs,
            &mut ledger,
        );
        ledger.protected = ledger.protected.saturating_sub(before - ledger.available());
    }
    // A role a local guard raised trains ahead of carriers, as its answer
    // ranks ahead of the lift.
    let carrying = lift
        && !short
        && !needs.urgent()
        && train_carriers(
            observation,
            map,
            profile,
            persistent,
            &scratch,
            &producers,
            &mut ledger,
        );
    if !carrying {
        // Sappers cost more than any army unit their producer trains, so the
        // ones attacks want against known defenses train ahead of the army.
        if !short {
            let sappers = persistent.missions.sappers_wanted(
                observation,
                map,
                profile,
                &persistent.memory,
                &scratch,
            );
            train_raiders(
                observation,
                profile,
                (0, sappers),
                &persistent.missions.held_outside_raids(),
                &free,
                &mut ledger,
            );
        }
        let settlement = Settlement::new(
            observation,
            map,
            profile,
            &persistent.memory,
            &danger,
            &scratch,
        );
        if !short
            && let Some((gap, by_air)) = persistent.saving.investment().and_then(|investment| {
                settlement.capabilities(&persistent.missions, investment, &ledger)
            })
        {
            needs.coverage(observation, gap, by_air);
        }
        if let Some(army) = ledger.decision.army.as_mut() {
            needs.describe_roles(army);
        }
        produce(observation, &producers, &mut needs, &mut ledger, saving);
        if !short {
            // A scout joins its producer's queue behind whatever is there;
            // Scuttlers draw on the producers the army's roles leave.
            if lacking > 0 {
                train_scout(&producers, &mut ledger);
            }
            let raiding = income.saturating_add(4 * u32::from(profile.traits.guile)) >= RAID_INCOME;
            let scuttlers = if raiding {
                Missions::raid_squad(observation, map, profile, &persistent.memory, &scratch)
            } else {
                0
            };
            train_raiders(
                observation,
                profile,
                (scuttlers, 0),
                &persistent.missions.held_outside_raids(),
                &free,
                &mut ledger,
            );
        }
        fill(observation, &producers, &mut needs, &mut ledger, saving);
    }
    if !short && exposed {
        workers::train(
            observation,
            map,
            &foundries,
            &staffing,
            profile.traits.greed,
            u64::MAX,
            &mut ledger,
        );
    }

    workers::seek(observation, map, frame, &scratch.ground, &mut ledger);
    persistent.saving.keep_at_most(ledger.available());
    persistent
        .income
        .record(tick, observation.scrap, ledger.decision.spent);
    Decision {
        target,
        protected: persistent.saving.protected(),
        ..ledger.decision
    }
}

/// Buys the saving target's next step once the whole uncommitted bank covers
/// it: an upgrade, a unit at the nearest ready producer that trains it, or a
/// building on the first spot the seat's knowledge allows, raised by the
/// nearest free Harvester.
fn buy(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    producers: &[Producer<'_>],
    persistent: &mut Persistent,
    needs: &mut Needs,
    ledger: &mut Ledger,
) {
    let Some(investment) = persistent.saving.investment() else {
        return;
    };
    if persistent.saving.pending() {
        return;
    }
    let Some(NextPurchase { step, price }) = ledger.decision.target.and_then(|target| target.next)
    else {
        return;
    };
    let affordable = ledger.available() >= price;
    let tick = observation.tick;
    match step {
        Step::Upgrade(id) => {
            if !affordable {
                return;
            }
            let Some(building) = observation
                .my_buildings
                .iter()
                .find(|building| building.id == id)
            else {
                return;
            };
            ledger.upgrade(id, price);
            ledger.protected = 0;
            persistent.saving.attempted(step, building.anchor);
        }
        Step::Round(id) => {
            if !affordable {
                return;
            }
            let Some(building) = observation
                .my_buildings
                .iter()
                .find(|building| building.id == id)
            else {
                return;
            };
            ledger.round(id, price);
            ledger.protected = 0;
            persistent.saving.attempted(step, building.anchor);
        }
        Step::Train(kind) => {
            if !affordable {
                return;
            }
            let producer = producers.iter().find(|producer| {
                producer.ready
                    && !ledger.queued_at(producer.building.id)
                    && investments::producers_of(observation, kind)
                        .any(|building| building.id == producer.building.id)
            });
            let Some(producer) = producer else {
                return;
            };
            if ledger.train_urgently(producer.building.id, kind) {
                ledger.explain(|| Pick {
                    unit: kind,
                    building: producer.building.id,
                    path: PickPath::Saved,
                    candidates: Vec::new(),
                });
                needs.queued(kind);
                ledger.protected = 0;
                persistent.saving.attempted(step, producer.building.anchor);
            }
        }
        Step::Build(kind) => {
            let packed = investments::layout(investment, kind) == placement::Layout::Packed;
            let danger = placement::Danger::of(observation, map, &persistent.memory);
            let mut tried = 0;
            let (anchor, allowed) = loop {
                let (anchor, allowed) = match site(
                    observation,
                    map,
                    &persistent.memory,
                    &danger,
                    investment,
                    kind,
                    Some((frame, ledger)),
                ) {
                    Site::At(anchor, allowed) => (anchor, allowed),
                    Site::Unseen(anchor) => {
                        if affordable {
                            explore(observation, map, frame, anchor, kind, ledger);
                        }
                        return;
                    }
                    // With nowhere left to look, protecting scrap for a
                    // building that cannot be placed would starve
                    // production. This is checked before the bank covers the
                    // price, or the protection would keep building up toward
                    // it.
                    Site::Nowhere => {
                        ledger.protected = 0;
                        persistent.saving.keep_at_most(0);
                        return;
                    }
                };
                // A footprint that would close a way out is refused for a
                // while, before scrap is held for it: a packed building tries
                // the next spot, a few a decision, and a Barricade, whose
                // spot is its own, gives the scrap back.
                if (packed || kind == BuildingKind::Barricade)
                    && !placement::keeps_paths(observation, map, kind, anchor, &ledger.planned)
                {
                    persistent.memory.fail(kind, anchor, tick);
                    tried += 1;
                    if packed && tried < PATH_TRIES {
                        continue;
                    }
                    if !packed {
                        ledger.protected = 0;
                        persistent.saving.keep_at_most(0);
                    }
                    return;
                }
                if !affordable {
                    return;
                }
                break (anchor, allowed);
            };
            let centre = footprint_centre(kind, anchor);
            let Some(builder) = workers::builder(observation, map, frame, anchor, centre, ledger)
            else {
                return;
            };
            ledger.build(builder.id, kind, anchor, allowed.defer, price);
            ledger.protected = 0;
            persistent.saving.attempted(step, anchor);
        }
    }
}

/// Spots one decision tries for a packed building that would close a way
/// out before leaving the rest to the next decision, which skips the refused
/// ones. A computation bound: few spots close a way out.
const PATH_TRIES: u32 = 4;

/// Where a building of `kind` toward `investment` would stand, as far as the
/// seat knows.
pub(crate) enum Site {
    /// The best anchor the seat's knowledge allows.
    At(TilePos, placement::Allowed),
    /// A better anchor the seat has not fully seen, which may turn out
    /// placeable once explored.
    Unseen(TilePos),
    /// Nowhere the seat knows of or could look.
    Nowhere,
}

/// The first of the anchors toward `investment` that the seat's knowledge
/// allows or has not fully seen, skipping those refused lately, those where
/// the site would not stand and, but for a defense, those something the seat
/// knows of could hit. A site bought now beside this decision's `ledger`
/// keeps clear of the footprints it planned and needs a builder that pays
/// its way.
pub(crate) fn site(
    observation: &ObservationData,
    map: &MapModel,
    memory: &Memory,
    danger: &placement::Danger,
    investment: Investment,
    kind: BuildingKind,
    ledger: Option<(HomeFrame, &Ledger)>,
) -> Site {
    let tick = observation.tick;
    let planned = ledger.map_or(&[][..], |(_, ledger)| ledger.planned());
    let defense =
        matches!(investment, Investment::Defense { kind: defense, .. } if defense == kind);
    investments::anchors(map, observation, investment, kind)
        .filter(|anchor| !memory.failed(kind, *anchor, tick))
        .filter(|anchor| defense || !danger.hits(kind, *anchor))
        .filter(|anchor| {
            !defense || !crate::missions::meeting::overruns(observation, memory, map, kind, *anchor)
        })
        .filter(|anchor| {
            let builder = ledger.and_then(|(frame, ledger)| {
                let centre = footprint_centre(kind, *anchor);
                workers::builder(observation, map, frame, *anchor, centre, ledger)
            });
            placement::stands(
                observation,
                map,
                memory,
                placement::Raising::fresh(kind, *anchor),
                &[],
                builder.as_slice(),
            )
        })
        .find_map(|anchor| {
            match placement::check(
                map,
                observation,
                kind,
                anchor,
                planned,
                investments::layout(investment, kind),
            ) {
                Ok(allowed) => Some(Site::At(anchor, allowed)),
                Err(placement::Refusal::Unexplored) => Some(Site::Unseen(anchor)),
                Err(_) => None,
            }
        })
        .unwrap_or(Site::Nowhere)
}

/// Drops the investments the seat could not place anywhere, from the best
/// down until one it could, along with `current` if it cannot: a target with
/// nowhere to stand would hold the saving no other purchase could use. Each
/// one checked gets its next purchase.
fn sited(
    settlement: &Settlement<'_>,
    missions: &Missions,
    candidates: &mut Vec<Candidate>,
    current: Option<Investment>,
    ledger: &Ledger,
) {
    let mut found = false;
    candidates.retain_mut(|candidate| {
        if found && Some(candidate.investment) != current {
            return true;
        }
        candidate.next = settlement.next(missions, candidate.investment, ledger);
        let placeable = match candidate.next {
            Some((Step::Build(kind), _)) => {
                !matches!(settlement.site(candidate.investment, kind), Site::Nowhere)
            }
            _ => true,
        };
        found |= placeable;
        placeable
    });
}

/// Sends the nearest free Harvester to look at `anchor`.
fn explore(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    anchor: TilePos,
    kind: BuildingKind,
    ledger: &mut Ledger,
) {
    let centre = footprint_centre(kind, anchor);
    if let Some(builder) = workers::builder(observation, map, frame, anchor, centre, ledger) {
        ledger.order(Command::Run {
            units: vec![builder.id],
            goal: anchor,
            queue: false,
        });
    }
}

/// Keeps enough carriers, alive, queued or being assembled, to lift every
/// free rider at home at once, training a Skyhook at a ready Airworks when
/// short. It is a stock, like
/// the Harvesters: no mission is promised the carriers it buys. Returns
/// whether a ready Airworks waits for the scrap to train one while riders at
/// home already fill every carrier, so that cheaper units do not spend it
/// first.
fn train_carriers(
    observation: &ObservationData,
    map: &MapModel,
    profile: &ResolvedProfile,
    persistent: &Persistent,
    scratch: &Scratch,
    producers: &[Producer<'_>],
    ledger: &mut Ledger,
) -> bool {
    // Every carrier's hold: alive and not away clearing, queued, bought this
    // decision, or being assembled in a cradle.
    let off_lifts = persistent.missions.gunships_off_lifts(observation, scratch);
    let holds: Vec<u8> = observation
        .my_units
        .iter()
        .filter(|unit| !off_lifts.contains(&unit.id))
        .map(|unit| unit.kind)
        .chain(observation.my_queues.iter().flatten().copied())
        .chain(std::iter::repeat_n(
            UnitKind::Skyhook,
            ledger.queued(UnitKind::Skyhook),
        ))
        .chain(
            observation
                .my_buildings
                .iter()
                .filter_map(|building| building.kind.assembles()),
        )
        .map(|kind| kind.stats().transport_capacity)
        .filter(|room| *room > 0)
        .collect();
    let Some(waiting) = persistent.missions.carriers_short(
        observation,
        map,
        profile,
        &persistent.memory,
        scratch,
        &holds,
    ) else {
        return false;
    };
    producers
        .iter()
        .find(|producer| {
            producer.building.kind == BuildingKind::Airworks
                && producer.ready
                && !ledger.queued_at(producer.building.id)
        })
        .is_some_and(|airworks| !ledger.train(airworks.building.id, UnitKind::Skyhook) && waiting)
}

/// Trains a scout for stale places no scout out or queued would see in time:
/// a Kestrel at a built Airworks, else a Surveyor at a Foundry, queued behind
/// whatever that producer already has: waiting for an idle one leaves stale
/// points unseen while workers and the army keep every producer busy.
fn train_scout(producers: &[Producer<'_>], ledger: &mut Ledger) {
    let producer = |kind: BuildingKind| {
        producers
            .iter()
            .find(|producer| producer.building.kind == kind)
    };
    let (producer, kind) = if let Some(airworks) = producer(BuildingKind::Airworks) {
        (airworks, UnitKind::Kestrel)
    } else if let Some(foundry) = producer(BuildingKind::Foundry) {
        (foundry, UnitKind::Surveyor)
    } else {
        return;
    };
    ledger.train(producer.building.id, kind);
}

/// Keeps `scuttlers`, alive or queued and held by no mission but a raid, for
/// raiding, and the `sappers` the seat's attacks want once it has scrap to
/// spare, less the more it leans on siege. Each trains at a ready producer
/// that can. `elsewhere` lists the units missions other than raids hold.
fn train_raiders(
    observation: &ObservationData,
    profile: &ResolvedProfile,
    (scuttlers, sappers): (usize, usize),
    elsewhere: &[UnitId],
    producers: &[Producer<'_>],
    ledger: &mut Ledger,
) {
    let spare = 2 * (100 - u32::from(profile.traits.siege.min(100)));
    for (kind, wanted, spare) in [
        (UnitKind::Scuttler, scuttlers, 0),
        (UnitKind::Sapper, sappers, spare),
    ] {
        // A Scuttler another mission holds cannot raid; a Sapper in an attack
        // is what the attack wanted it for.
        let free = |unit: &&UnitObs| {
            kind != UnitKind::Scuttler || elsewhere.binary_search(&unit.id).is_err()
        };
        let have = observation
            .my_units
            .iter()
            .filter(free)
            .map(|unit| unit.kind)
            .chain(observation.my_queues.iter().flatten().copied())
            .filter(|owned| *owned == kind)
            .count()
            + ledger.queued(kind);
        if have >= wanted || ledger.spendable() < kind.stats().cost + spare {
            continue;
        }
        let producer = producers.iter().find(|producer| {
            producer.ready
                && !ledger.queued_at(producer.building.id)
                && producer.building.kind.base_stats().produces.contains(&kind)
        });
        if let Some(producer) = producer {
            ledger.train(producer.building.id, kind);
        }
    }
}

/// Built producers, nearest home first, each ready when the work left in its
/// queue ends within `interval` ticks.
fn producers(observation: &ObservationData, frame: HomeFrame, interval: u64) -> Vec<Producer<'_>> {
    let mut producers: Vec<Producer<'_>> = observation
        .my_buildings
        .iter()
        .zip(&observation.my_queues)
        .enumerate()
        .filter(|(_, (building, _))| crate::investments::built_producer(building))
        .map(|(index, (building, queue))| {
            let queued: u64 = queue
                .iter()
                .map(|kind| u64::from(kind.stats().train_ticks))
                .sum();
            let done = u64::from(observation.own_queue_progress(index).unwrap_or(0));
            Producer {
                building,
                ready: queued.saturating_sub(done) < interval,
            }
        })
        .collect();
    producers.sort_by_key(|producer| {
        let centre = footprint_centre(producer.building.kind, producer.building.anchor);
        (frame.rank(frame.home, centre), producer.building.id)
    });
    producers
}

/// Per mille of how well the best unit for a role fills it, whatever its
/// producer is doing and whatever it costs, that a ready producer's unit must
/// reach for the role to take it rather than wait: a scoring shape under
/// which a unit that fills the role a third better is worth waiting for and a
/// slightly better one is not.
const WAIT_FOR_BETTER: u64 = 750;

/// Gives the most wanted role first claim on ready producers, from
/// unprotected scrap: in turn, of the ready producers that can afford a unit
/// for it, the one whose best unit fills the role best queues it, the nearest
/// on a tie. A role gives way to the next when no ready producer can afford a
/// unit for it, or when the units they can afford fall short of
/// [`WAIT_FOR_BETTER`] of the best one any producer trains: the role waits
/// for that producer and the scrap, and the income it leaves unspent is what
/// adds another producer. Otherwise every expansion Foundry fills the line
/// with Sentinels while the Fabricators are busy, and that spending hides
/// that the seat lacks a Fabricator. While the seat saves for `saving`, a
/// unit, the producers that train it wait for it and no other producer trains
/// a cheaper unit of its role.
fn produce(
    observation: &ObservationData,
    producers: &[Producer<'_>],
    needs: &mut Needs,
    ledger: &mut Ledger,
    saving: Option<UnitKind>,
) {
    // A role a short defense or guard raised answers it: it may take a
    // producer held for the saved unit, the saved unit's role and the scrap
    // kept for it, as the defense's own answer always could. Other roles
    // keep to those holds.
    let mut idle = idle(observation, producers, ledger, None);
    while let Some((index, kind, role, bar)) = needs.wanted().into_iter().find_map(|role| {
        let raised = needs.raised(role);
        let budget = if raised {
            ledger.available()
        } else {
            ledger.spendable()
        };
        let bar = producers
            .iter()
            .filter_map(|producer| needs.unit(observation, producer.building.kind, role, u32::MAX))
            .map(|kind| needs.suitability(kind, role))
            .max()
            .map_or(0, |best| best * WAIT_FOR_BETTER / 1_000);
        idle.iter()
            .enumerate()
            .filter(|(_, producer)| raised || !saved_for(observation, producer, saving))
            .filter_map(|(index, producer)| {
                needs
                    .unit(observation, producer.building.kind, role, budget)
                    .filter(|kind| {
                        (raised || !instead(saving, *kind)) && needs.suitability(*kind, role) >= bar
                    })
                    .map(|kind| (index, kind))
            })
            .max_by_key(|(index, kind)| (needs.rank(*kind, role), Reverse(*index)))
            .map(|(index, kind)| (index, kind, role, bar))
    }) {
        let producer = idle.remove(index);
        let trained = if needs.raised(role) {
            ledger.train_urgently(producer.building.id, kind)
        } else {
            ledger.train(producer.building.id, kind)
        };
        if trained {
            ledger.explain(|| Pick {
                unit: kind,
                building: producer.building.id,
                path: if needs.raised(role) {
                    PickPath::Answer { role, bar }
                } else {
                    PickPath::Wanted { role, bar }
                },
                candidates: needs.candidates(
                    observation,
                    producers.iter().map(|other| other.building.kind),
                    role,
                ),
            });
            needs.queued(kind);
        }
    }
}

/// Ready producers left with no wanted role and no small stock train ground
/// units while ground units can reach an enemy, unless another producer
/// serves a wanted role: the scrap waits for it. A producer trains only a
/// unit within [`WAIT_FOR_BETTER`] of the best any of the seat's producers
/// trains for the role: a Foundry's Sentinels do not pad a line a
/// Fabricator's Wardens fill far better.
fn fill(
    observation: &ObservationData,
    producers: &[Producer<'_>],
    needs: &mut Needs,
    ledger: &mut Ledger,
    saving: Option<UnitKind>,
) {
    if !needs.fallback() {
        return;
    }
    for producer in idle(observation, producers, ledger, saving) {
        let kind = producer.building.kind;
        let wanted = needs.wanted();
        if producers.iter().any(|other| {
            wanted
                .iter()
                .any(|role| composition::serves(observation, other.building.kind, *role))
        }) {
            continue;
        }
        if let Some(role) = needs.fallback_role(observation, kind)
            && let Some(unit) = needs.unit(observation, kind, role, ledger.spendable())
            && needs.suitability(unit, role) * 1_000
                >= best_for(observation, producers, needs, role) * WAIT_FOR_BETTER
            && !instead(saving, unit)
            && ledger.train(producer.building.id, unit)
        {
            ledger.explain(|| Pick {
                unit,
                building: producer.building.id,
                path: PickPath::Fallback { role },
                candidates: needs.candidates(observation, [kind], role),
            });
            needs.queued(unit);
        }
    }
}

/// How well the best unit any of `producers` trains suits `role`.
fn best_for(
    observation: &ObservationData,
    producers: &[Producer<'_>],
    needs: &Needs,
    role: composition::Role,
) -> u64 {
    producers
        .iter()
        .filter_map(|producer| needs.unit(observation, producer.building.kind, role, u32::MAX))
        .map(|unit| needs.suitability(unit, role))
        .max()
        .unwrap_or(0)
}

/// The ready producers nothing queued at this decision, less those that wait
/// for `saving`.
fn idle<'a, 'b>(
    observation: &ObservationData,
    producers: &'b [Producer<'a>],
    ledger: &Ledger,
    saving: Option<UnitKind>,
) -> Vec<&'b Producer<'a>> {
    producers
        .iter()
        .filter(|producer| producer.ready && !ledger.queued_at(producer.building.id))
        .filter(|producer| !saved_for(observation, producer, saving))
        .collect()
}

/// Whether `kind` would take the place of `saving` in its role.
fn instead(saving: Option<UnitKind>, kind: UnitKind) -> bool {
    saving.is_some_and(|unit| {
        kind != unit
            && composition::role(kind).is_some()
            && composition::role(kind) == composition::role(unit)
    })
}

/// Whether `producer` trains `saving`, the unit the seat saves for.
fn saved_for(
    observation: &ObservationData,
    producer: &Producer<'_>,
    saving: Option<UnitKind>,
) -> bool {
    saving.is_some_and(|unit| {
        composition::producible(observation, producer.building.kind).any(|kind| kind == unit)
    })
}

/// Whether `kind`, trained on ground `at`, can help `short`: it can reach
/// the Foundry and hit a domain the shortfall is short in.
fn helps(kind: UnitKind, at: Option<u32>, short: &Shortfall) -> bool {
    (kind.stats().domain == Domain::Air || (at.is_some() && at == short.ground))
        && [Domain::Ground, Domain::Air]
            .into_iter()
            .enumerate()
            .any(|(index, domain)| short.gap[index] > 0 && hits(kind, domain))
}

fn hits(kind: UnitKind, domain: Domain) -> bool {
    kind.stats()
        .weapons
        .iter()
        .any(|weapon| weapon.targets.covers(domain))
}

/// Per mille of what the counter chart measures a structure destroys that it
/// keeps as an answer: the chart measures it standing, but it fights only
/// once a builder raises it, under the very fire it is to answer. Not how
/// strong it is but how soon it is there.
const STANDING: u64 = 600;

/// How well `fighter` answers `short`: attacker value it destroys per scrap
/// against the attackers the shortfall counts in a domain still short; when
/// it counts none there, or the seat does not read the attackers, how cheap
/// it is.
pub(crate) fn answer(fighter: Fighter, short: &Shortfall) -> u64 {
    let short_in = |attacker: UnitKind| match attacker.stats().domain {
        Domain::Ground => short.gap[0] > 0,
        Domain::Air => short.gap[1] > 0,
    };
    let pressing: Vec<&(UnitKind, u64)> = short
        .attackers
        .iter()
        .filter(|(attacker, _)| short_in(*attacker))
        .collect();
    // A seat that judges the threat by its size alone answers with what is
    // cheapest to throw at it, as does one short only of builders and
    // producers: the first unit there soonest stops what they raise.
    let total: u64 = pressing.iter().map(|(_, value)| *value).sum();
    if !short.read || total == 0 {
        return 1_000_000 / fighter.cost().max(1);
    }
    pressing
        .iter()
        .map(|(attacker, value)| {
            // Units and structures alike trade as the counter chart
            // measured them.
            let trades = match fighter {
                Fighter::Unit(kind) => crate::counters::pair(kind, *attacker),
                Fighter::Structure(kind) => {
                    crate::counters::pair(crate::counters::Piece::Gun(kind, 0), *attacker)
                        * STANDING
                        / 1_000
                }
            };
            trades * value
        })
        .sum::<u64>()
        / total
}

/// Per mille of its price in attacker value a unit must destroy for a short
/// defense to buy it: one that dies having paid back less than half its price
/// only feeds the attackers.
const PAYS_BACK: u64 = 500;

/// Ticks of income a short defense waits for a better answer than its bank
/// buys now.
const SAVE_TICKS: u64 = 300;

/// The structures an emergency puts up against a short defense's attackers.
const EMERGENCY_KINDS: [BuildingKind; 3] = [
    BuildingKind::Turret,
    BuildingKind::Bastion,
    BuildingKind::FlakTurret,
];

/// Those of `shortfalls` the army's roles do not answer. Each one on the
/// start's ground whose best answer within `reach`, the emergency
/// `structures` raised this decision among them, is a unit with a role
/// raises that role's need by its gap in the domains the unit hits, so the
/// role's units are picked as any of its units are, by what they trade
/// against the enemy, the army's variety and the seat's personality, and not
/// only the best unit the bank buys this moment. One answered best by a unit
/// with no role, such as a Scuttler while the seat trains nothing better, or
/// by a structure, is left to [`arm`], and so is one on other ground, where
/// only a producer standing there helps.
fn by_role(
    (observation, map, producers): (&ObservationData, &MapModel, &[Producer<'_>]),
    shortfalls: Vec<Shortfall>,
    reach: u64,
    structures: &[BuildingKind],
    needs: &mut Needs,
) -> Vec<Shortfall> {
    let home = map
        .start(observation.me)
        .and_then(|start| map.component(start));
    shortfalls
        .into_iter()
        .filter(|short| {
            if short.ground != home {
                return true;
            }
            let Some((Fighter::Unit(kind), _)) =
                best_answer(observation, map, producers, short, reach, structures)
            else {
                return true;
            };
            let Some(role) = composition::role(kind) else {
                return true;
            };
            let by: u64 = [Domain::Ground, Domain::Air]
                .into_iter()
                .enumerate()
                .filter(|(_, domain)| hits(kind, *domain))
                .map(|(index, _)| short.gap[index])
                .sum();
            needs.raise(role, by);
            false
        })
        .collect()
}

/// The emergency structures this decision put up.
fn emergency_raised(ledger: &Ledger) -> Vec<BuildingKind> {
    EMERGENCY_KINDS
        .into_iter()
        .filter(|kind| {
            ledger.decision.purchases.iter().any(
                |purchase| matches!(purchase, Purchase::Build { building, .. } if building == kind),
            )
        })
        .collect()
}

/// Scrap the bank gains in [`SAVE_TICKS`] at `income` a minute.
fn soon(income: u32) -> u64 {
    u64::from(income) * SAVE_TICKS / (60 * u64::from(aftermine_sim::TICKS_PER_SECOND))
}

/// The best answer to `short` whose price is within `reach`: a unit a
/// finished producer trains, or one of the emergency `structures`, with its
/// score.
fn best_answer(
    observation: &ObservationData,
    map: &MapModel,
    producers: &[Producer<'_>],
    short: &Shortfall,
    reach: u64,
    structures: &[BuildingKind],
) -> Option<(Fighter, u64)> {
    // Only a producer free to train now answers: a unit queued behind
    // others comes too late to keep scrap from the emergency for it.
    let units = producers
        .iter()
        .filter(|producer| producer.ready)
        .flat_map(|producer| {
            let at = map.component(producer.building.anchor);
            composition::producible(observation, producer.building.kind)
                .filter(move |kind| helps(*kind, at, short))
                .map(Fighter::Unit)
        });
    // A structure answers only where a worker could put it up, once what it
    // needs stands, and when it fires back at the attackers.
    let builder = observation.my_units.iter().any(|unit| {
        unit.kind.stats().harvest.is_some() && map.component(unit.tile) == short.ground
    });
    let structures = structures
        .iter()
        .filter(|kind| {
            builder
                && crate::investments::buildable(observation, **kind)
                && defenses::returns_fire(**kind, &short.attackers)
        })
        .map(|kind| Fighter::Structure(*kind));
    units
        .chain(structures)
        .filter(|fighter| fighter.cost() <= reach)
        .map(|fighter| (fighter, answer(fighter, short)))
        .filter(|(_, score)| short.attackers.is_empty() || *score >= PAYS_BACK)
        .max_by_key(|(fighter, score)| (*score, fighter.cost()))
}

/// What a short defense keeps from its emergency structures this decision:
/// the price of the dearest best answer to its shortfalls that is a unit the
/// bank reaches soon, so production can train it. A structure that answers
/// best leaves the bank to the emergency. None when any shortfall has no
/// answer the bank reaches soon, so the saving target keeps its scrap.
fn awaited(
    observation: &ObservationData,
    map: &MapModel,
    producers: &[Producer<'_>],
    shortfalls: &[Shortfall],
    income: u32,
    bank: u32,
) -> Option<u32> {
    let reach = u64::from(bank) + soon(income);
    let answers: Vec<Fighter> = shortfalls
        .iter()
        .filter_map(|short| {
            best_answer(observation, map, producers, short, reach, &EMERGENCY_KINDS)
        })
        .map(|(fighter, _)| fighter)
        .collect();
    // One Foundry's answer must not free the savings for another's
    // emergency that pays back nothing: protection is one total.
    (answers.len() == shortfalls.len()).then(|| {
        answers
            .into_iter()
            .filter_map(|fighter| match fighter {
                Fighter::Unit(kind) => Some(kind.stats().cost),
                Fighter::Structure(_) => None,
            })
            .max()
            .unwrap_or(0)
            .min(bank)
    })
}

/// At each producer free to train while one of `shortfalls` is still short,
/// train the unit that best answers it among those the bank buys, until what
/// this decision queued makes up each shortfall. Only the shortfalls no role
/// answers come here: [`by_role`] hands the rest to the army's roles. A unit
/// that would not pay back half its price against the attackers is never
/// bought, and one worth less than half the best answer the bank reaches
/// soon, a unit or an emergency structure this decision put up, waits for
/// it: units that die before they pay only feed the attackers. A unit
/// queued makes up a shortfall at what it trades against the attackers, as
/// defenses and guards count their own units.
fn arm(
    observation: &ObservationData,
    map: &MapModel,
    producers: &[Producer<'_>],
    mut shortfalls: Vec<Shortfall>,
    income: u32,
    needs: &mut Needs,
    ledger: &mut Ledger,
) {
    if shortfalls.is_empty() {
        return;
    }
    let cover = |shortfalls: &mut [Shortfall], kind: UnitKind, at: Option<u32>| {
        if let Some(short) = shortfalls.iter_mut().find(|short| helps(kind, at, short)) {
            let cost = u64::from(kind.stats().cost);
            let made = [Domain::Ground, Domain::Air].map(|domain| {
                let there: Vec<(UnitKind, u64)> = short
                    .attackers
                    .iter()
                    .copied()
                    .filter(|(attacker, _)| attacker.stats().domain == domain)
                    .collect();
                cost * crate::counters::weight(kind, &there) / 1_000
            });
            for (index, domain) in [Domain::Ground, Domain::Air].into_iter().enumerate() {
                if hits(kind, domain) {
                    short.gap[index] = short.gap[index].saturating_sub(made[index]);
                }
            }
        }
    };
    let ground = |building: BuildingId| {
        observation
            .my_buildings
            .iter()
            .find(|own| own.id == building)
            .and_then(|own| map.component(own.anchor))
    };
    let queued: Vec<(BuildingId, UnitKind)> = ledger
        .decision
        .purchases
        .iter()
        .filter_map(|purchase| match purchase {
            Purchase::Train { building, unit } => Some((*building, *unit)),
            _ => None,
        })
        .collect();
    for (building, kind) in queued {
        cover(&mut shortfalls, kind, ground(building));
    }
    // A structure answers only once the emergency has put one up: one it
    // passed over, its approach already covered or no site found, is no
    // reason to hold back a unit.
    let raised = emergency_raised(ledger);
    let idle: Vec<&Producer<'_>> = producers
        .iter()
        .filter(|producer| producer.ready && !ledger.queued_at(producer.building.id))
        .collect();
    for producer in idle {
        // A defense spends what it kept from emergency structures, and so do
        // local guards: a guard at a raided expansion that waited for free
        // scrap lost more games to harass and raids (280-game gauntlets at
        // Prime) than the scrap it took from a defense elsewhere cost.
        let budget = ledger.available();
        let at = map.component(producer.building.anchor);
        let kind = shortfalls.iter().find_map(|short| {
            let best = best_answer(
                observation,
                map,
                producers,
                short,
                u64::from(budget) + soon(income),
                &raised,
            )
            .map_or(0, |(_, score)| score);
            let worth = |kind: UnitKind| answer(Fighter::Unit(kind), short);
            composition::producible(observation, producer.building.kind)
                .filter(|kind| {
                    kind.stats().cost <= budget
                        && helps(*kind, at, short)
                        && (short.attackers.is_empty() || worth(*kind) >= PAYS_BACK)
                        && 2 * worth(*kind) >= best
                })
                .max_by_key(|kind| (worth(*kind), kind.stats().cost))
        });
        let trained = kind.is_some_and(|kind| ledger.train_urgently(producer.building.id, kind));
        if let Some(kind) = kind.filter(|_| trained) {
            ledger.explain(|| Pick {
                unit: kind,
                building: producer.building.id,
                path: PickPath::Shortfall,
                candidates: Vec::new(),
            });
            needs.queued(kind);
            cover(&mut shortfalls, kind, at);
        }
    }
}

/// Per mille of income protected for the saving target. Stance and greed set
/// it; armed enemy units in sight near the seat's buildings lower it when
/// they outweigh half the seat's army by the counter chart, and more when
/// they outweigh all of it, so the seat spends on defense when it is
/// threatened. Drones count on neither side: they come and go from their
/// host's bay.
pub(crate) fn share(observation: &ObservationData, profile: &ResolvedProfile) -> u32 {
    let base = match profile.stance {
        BotStance::Turtle => 550,
        BotStance::Balanced => 500,
        BotStance::Aggressive => 400,
    };
    let greed = 3 * (i32::from(profile.traits.greed) - 50);
    let threat: Vec<(Piece, u64)> = observation
        .enemy_units
        .iter()
        .filter(|unit| unit.kind.stats().can_fight() && !unit.kind.stats().docks())
        .filter(|unit| {
            observation.my_buildings.iter().any(|building| {
                let size = building.kind.size();
                gap(building.anchor, size, unit.tile, (1, 1)) < 12
            })
        })
        .map(|unit| (Piece::Unit(unit.kind), u64::from(unit.kind.stats().cost)))
        .collect();
    let own = army_force(observation);
    let (threat, army) = (weigh(&threat, &own), weigh(&own, &threat));
    let cut = if threat == 0 {
        0
    } else if threat >= army {
        300
    } else if 2 * threat >= army {
        150
    } else {
        0
    };
    (base + greed - cut).clamp(200, 800).cast_unsigned()
}

/// Price of the seat's workers, alive or queued.
fn workforce(observation: &ObservationData) -> u64 {
    observation
        .my_units
        .iter()
        .map(|unit| unit.kind)
        .chain(observation.my_queues.iter().flatten().copied())
        .filter(|kind| workers::worker(*kind))
        .map(|kind| u64::from(kind.stats().cost))
        .sum()
}

/// What the seat's armed units cost.
pub(crate) fn army(observation: &ObservationData) -> u64 {
    armed(observation)
        .map(|unit| u64::from(unit.kind.stats().cost))
        .sum()
}

/// The seat's armed machines. A drone comes and goes from its host's bay, so
/// only machines count.
fn armed(observation: &ObservationData) -> impl Iterator<Item = &UnitObs> {
    observation
        .my_units
        .iter()
        .filter(|unit| !unit.kind.stats().weapons.is_empty() && !unit.kind.stats().docks())
}

/// The seat's armed machines, each at its price: the army [`army`] prices,
/// as the counter chart weighs it.
pub(crate) fn army_force(observation: &ObservationData) -> Vec<(Piece, u64)> {
    armed(observation)
        .map(|unit| (Piece::Unit(unit.kind), u64::from(unit.kind.stats().cost)))
        .collect()
}

/// Per mille of the scrap that started around the seat's home already gone.
/// Unexplored nodes count as untouched.
fn depletion(observation: &ObservationData, map: &MapModel) -> u32 {
    let nodes = map.home_nodes(observation.me);
    let initial: u64 = nodes.iter().map(|(_, amount)| u64::from(*amount)).sum();
    if initial == 0 {
        return 0;
    }
    let left: u64 = nodes
        .iter()
        .map(|(node, amount)| {
            if !observation.explored(*node) {
                return u64::from(*amount);
            }
            observation
                .known_scrap
                .binary_search_by_key(&(node.y, node.x), |(tile, _)| (tile.y, tile.x))
                .map_or(0, |index| u64::from(observation.known_scrap[index].1))
        })
        .sum();
    u32::try_from(1_000 - left.min(initial) * 1_000 / initial)
        .expect("a per-mille share fits in u32")
}

/// Income per minute, less four for each point of guile, at which the seat
/// starts keeping Scuttlers for raiding: a personality limit, since raiding
/// waits for an economy that can spare it.
const RAID_INCOME: u32 = 900;

/// What a needed lift adds to the Airworks' investment score.
const LIFT_PULL: u32 = 600;

/// Unit orders one decision may issue: a difficulty limit on attention. With
/// each rung's decision interval this is about 100 orders a minute for
/// Scrapheap, 200 for Standard, 800 for Veteran and 2,000 for Prime.
pub(crate) fn allowance(difficulty: BotDifficulty) -> u32 {
    match difficulty {
        BotDifficulty::Scrapheap => 8,
        BotDifficulty::Standard => 6,
        BotDifficulty::Veteran => 8,
        BotDifficulty::Prime => 10,
    }
}

#[cfg(test)]
mod tests;
