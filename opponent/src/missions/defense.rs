//! The defend mission: every threatened Foundry recruits free units that can
//! hit its threats and sends them at the threat nearest it once those
//! gathered match the force they would meet there, gives up a place away
//! from home it could never match, then lets them go once the threat has
//! been gone a while. Enemy scouts and builders among the seat's buildings, and enemy
//! buildings raised among them, are threats too. An ally's Foundry under ground
//! attack gets the units the seat's own Foundries leave free. Shells from a
//! known enemy building send ground fighters beside it, shells from a gun out
//! of sight advance them on where it probably stands, and a gun in sight is
//! attacked, once those gathered match any escort beside it. Defenders
//! falling back run without stopping to fight on the way.

use super::attack::{building_value, concentrates, defense, defense_around};
use super::reserve::LOCAL_REACH;
use super::{
    AttackPhase, DefendPhase, MISSION_CAP, Mission, Missions, Task, UNIT_CAP, against_machines,
    fill, hits, hunt, insert, meeting, mine, run, value, walking_gun, worth,
};
use crate::decision::Ledger;
use crate::frame::{HomeFrame, centre_distance, doubled, footprint_centre, gap, ring};
use crate::map::MapModel;
use crate::memory::Memory;
use aftermine_sim::observation::{BuildingObs, ObservationData, UnitObs};
use aftermine_sim::scenario::BotDifficulty;
use aftermine_sim::stats::{Domain, WeaponStats};
use aftermine_sim::{AttackTarget, BuildingKind, Command, PlayerId, UnitId, UnitKind};
use chassis::fx::Fx;
use chassis::grid::TilePos;

/// Empty tiles between an enemy and an own building inside which the enemy
/// threatens it, unless its weapon reaches further.
const THREAT_GAP: i32 = 8;

/// Ticks a defense waits without a threat before letting its units go.
const QUIET_TICKS: u64 = 120;

/// Ticks a defense may stay engaged before evaluation calls it stuck.
const ENGAGE_TICKS: u64 = 3_600;

/// Tiles a threat may move before its defenders are sent after it again.
const RETARGET_TILES: i32 = 3;

/// Ticks a defense's `phase` should end within; a quiet guard has no
/// deadline.
pub(super) fn timeout(phase: DefendPhase) -> Option<u64> {
    match phase {
        DefendPhase::Guard => None,
        DefendPhase::Engage { .. } => Some(ENGAGE_TICKS),
        DefendPhase::Recover => Some(QUIET_TICKS),
    }
}

impl Missions {
    /// Answers every threatened Foundry: recruits free units that can hit its
    /// threats until, in each domain it is attacked from, they outweigh the
    /// attackers by half again, and sends them at the armed grounded threat
    /// nearest the Foundry, else the nearest unarmed one, else at a gun out
    /// of sight. The ground force they would meet is every known enemy near
    /// that threat, not only those near the buildings. Until the defenders
    /// gathered beside the Foundry or already at the fight match it, those
    /// hunting an enemy that walks in wait beside the Foundry instead, and
    /// ones already out fall back there once they have less than two thirds
    /// of it, at a seat of `difficulty` that does not attack piecemeal,
    /// running back without stopping to fight on the way. Artillery is still
    /// reached: at once when nothing else would join the fight at it, else
    /// once those gathered match everything that would. A threat away from
    /// home that everything
    /// the defense could take would not match is given up, and defenders
    /// out at it fall back home. Unarmed enemies and enemy buildings among the seat's
    /// buildings draw defenders worth what they cost, and a shortfall
    /// counts only armed attackers. A shelling building counts
    /// with the known defense around it, and only while the units the
    /// defense holds or could take would beat that defense by half again. A
    /// defense that is only recovering lends its units, as does an attack
    /// regrouping at home, or travelling out of contact at a seat of
    /// `difficulty` that calls such an army back.
    /// Returns each of the seat's own Foundries whose defense falls short of
    /// its attackers, home-nearest first.
    ///
    /// Only threats standing on or beside the Foundry's ground count. One
    /// across water or a chasm is left to production: chasing it would stall
    /// every defender, and a defense short forever would never save for the
    /// tech that answers it. Nor does a defense against shelling, from a
    /// building or a gun out of sight, count as short: static defenses cannot
    /// reach the guns.
    pub(crate) fn defend(
        &mut self,
        observation: &ObservationData,
        map: &MapModel,
        frame: HomeFrame,
        memory: &Memory,
        difficulty: BotDifficulty,
        ledger: &mut Ledger,
    ) -> Vec<Shortfall> {
        let now = observation.tick;
        let lendable = self.available(observation, Some(difficulty));
        let mut groups = threats(observation, map, frame, memory);
        self.weigh((observation, map, memory, ledger), &lendable, &mut groups);
        // Attackers remembered out of sight keep a defense from standing down.
        groups.retain(|(_, siege, _)| {
            !siege.units.is_empty()
                || !siege.strays.is_empty()
                || !siege.batteries.is_empty()
                || siege.unseen.is_some()
                || siege.remembered != [0, 0]
        });
        let returning = self.stand_down(
            (observation, memory, difficulty),
            (map, frame),
            &groups,
            ledger,
        );
        self.list.retain(|mission| {
            !matches!(
                mission.task,
                Task::Defend {
                    phase: DefendPhase::Recover,
                    ..
                }
            ) || now < mission.since + QUIET_TICKS
        });

        let mut free = self.available(observation, Some(difficulty));
        // Units an attack regroups at home go last, so a call for defense
        // does not take apart an army about to set out while other units can
        // answer it.
        let mut regrouping: Vec<UnitId> = self
            .list
            .iter()
            .filter(|mission| {
                matches!(
                    mission.task,
                    Task::Attack {
                        phase: AttackPhase::Gather | AttackPhase::Recover,
                        ..
                    }
                )
            })
            .flat_map(|mission| mission.units.iter().copied())
            .collect();
        regrouping.sort_unstable();
        let mut shortfalls = Vec::new();
        for (foundry, siege, own) in &groups {
            let centre = footprint_centre(foundry.kind, foundry.anchor);
            let component = map.component(foundry.anchor);
            let nearest = |tiles: &mut dyn Iterator<Item = TilePos>| {
                tiles.min_by_key(|tile| frame.rank(centre, doubled(*tile)))
            };
            // With nothing armed among them, defenders go for the intruder
            // that matters most and hold it, rather than hunt whatever
            // stands nearest, such as a scout, away from a site going up.
            let quarry = siege.quarry(|tile| frame.rank(centre, doubled(tile)));
            let grounded = quarry
                .map(|(_, tile)| tile)
                .or_else(|| siege.nearest_grounded(|tile| frame.rank(centre, doubled(tile))));
            let Some(goal) = grounded.or(siege.unseen).or_else(|| {
                let flyer = nearest(&mut siege.sighted())?;
                own.then(|| guard(observation, map, frame, component, flyer))?
            }) else {
                continue;
            };
            // Artillery is reached rather than hunted: a hunt engages whatever
            // comes first, such as a spotter overhead, while the guns shell the
            // defenders from beyond reach. A gun in sight on the Foundry's
            // ground is attacked; one out of sight is advanced on.
            let shelling = siege
                .units
                .iter()
                .filter(|threat| walking_gun(threat) && map.component(threat.tile) == component)
                .min_by_key(|threat| (frame.rank(centre, doubled(threat.tile)), threat.id));
            let pursuit = Pursuit::of(shelling.copied(), quarry, grounded, siege.unseen.is_some());
            let gun = pursuit.held();
            let hunting = matches!(pursuit, Pursuit::Hunt);
            // A gun with others beside it that would join the fight there is
            // met like any ground force at a seat that gathers: defenders
            // going at it one by one die to its escort on the way.
            let escorted = shelling
                .filter(|_| concentrates(difficulty))
                .is_some_and(|threat| meeting::escort(observation, memory, threat.tile) > 0);
            let gathering = hunting || escorted;
            // Defenders hunting a ground threat meet whatever stands near
            // it, not only the enemies that came near the buildings: a line
            // held a few tiles behind them joins in. Going at an escorted
            // gun, they meet what stands near it too.
            let meets = grounded
                .filter(|_| gathering)
                .map_or(0, |goal| meeting::opposed(observation, memory, goal))
                .max(shelling.filter(|_| escorted).map_or(0, |threat| {
                    meeting::opposed(observation, memory, threat.tile)
                }));
            let pressing = siege.pressing_value(meets);
            let besides: Vec<TilePos> = siege.batteries.iter().map(|(_, beside)| *beside).collect();
            // An escort beside a battery is pressing already: it counts once.
            let guns = defense_around(observation, memory, &besides, &siege.pressing())
                + siege.unseen.map_or(0, |_| gun_value());
            let unarmed = siege.unarmed();
            let need = [
                pressing[0] + guns * 3 / 2 + unarmed[0],
                pressing[1] + unarmed[1],
            ];
            let index = self.list.iter().position(
                |mission| matches!(mission.task, Task::Defend { asset, .. } if asset == foundry.id),
            );
            let members = index.map_or(&[][..], |index| &self.list[index].units);
            let mut have = [Domain::Ground, Domain::Air].map(|domain| {
                members
                    .iter()
                    .filter_map(|id| mine(observation, *id))
                    .filter(|unit| hits(unit, domain))
                    .map(value)
                    .sum::<u64>()
            });
            let room = UNIT_CAP - members.len();
            let mut candidates =
                self.candidates((observation, map, ledger), &free, members, need, component);
            candidates.sort_by_key(|unit| {
                (
                    regrouping.binary_search(&unit.id).is_ok(),
                    frame.rank(centre, doubled(unit.tile)),
                    unit.id,
                )
            });
            let mut recruits = fill(candidates, need, &mut have, room);
            let gap = siege.gap(pressing, have);
            if *own && gap != [0, 0] {
                shortfalls.push(Shortfall {
                    ground: component,
                    gap,
                    attackers: siege.attackers(gap, crate::reads_threats(difficulty)),
                    // Builders and producers with nothing armed beside them
                    // are met with the cheapest unit: the first one there
                    // soonest stops the site going up.
                    read: crate::reads_threats(difficulty) && !siege.unarmed_only(),
                });
            }
            // Guns are reached rather than waited for: they shell from
            // where they stand.
            let mission = index.map(|index| &self.list[index]);
            let met = pressing[0] * 2 / 3 + guns;
            let go = sortie(*own, siege, met, difficulty);
            // A defense already out at its fight is not given up short of the
            // margin: turned out under fire, its members only die on the way
            // home, and the next look sends them back. Where the rung falls
            // back to the post, that rule takes it from there; a rung that
            // never falls back still gives up below even odds. One waiting at
            // its post, or still on its way out, is not out yet.
            let out = mission.is_some_and(|mission| {
                matches!(
                    mission.task,
                    Task::Defend {
                        phase: DefendPhase::Engage { .. },
                        ..
                    }
                ) && !ring(foundry.anchor, foundry.kind.size()).any(|tile| tile == mission.goal)
                    && together(observation, mission.units.iter(), [mission.goal, goal]) > 0
            });
            let bar = match (out, concentrates(difficulty)) {
                (false, _) => Some(go),
                (true, true) => None,
                (true, false) => Some(met),
            };
            if *own
                && gathering
                && met > 0
                && bar.is_some_and(|bar| siege.held < bar)
                && !kept(observation, map, foundry, goal)
            {
                self.give_up(index, observation, map, frame, goal, ledger);
                continue;
            }
            let post = grounded.and_then(|threat| waiting_post(map, frame, foundry, threat));
            // Only defenders at the post or already at the fight count
            // toward going out: those still on their way would reach it
            // after it began.
            let fight = shelling.map_or(goal, |threat| threat.tile);
            let front = front(observation, members, post, fight);
            let gathered = post.map_or(have[0], |post| {
                together(observation, members.iter().chain(&recruits), [post, fight])
            });
            let (goal, pursuit, gun) = match grounded
                .filter(|_| gathering && concentrates(difficulty))
                .filter(|_| waits(mission, foundry, [gathered, front.1], [met, go]))
                .map(|threat| waiting_post(map, frame, foundry, threat))
            {
                Some(Some(post)) => (post, Pursuit::Fall, None),
                Some(None) => continue,
                None => (goal, pursuit, gun),
            };

            let Some(index) = index else {
                if recruits.is_empty() || self.list.len() >= MISSION_CAP {
                    continue;
                }
                recruits.sort_unstable();
                let ordered = dispatch(observation, ledger, recruits, goal, pursuit);
                if !ordered.is_empty() {
                    take(&mut free, &ordered);
                    self.release(&ordered);
                    self.list.push(Mission {
                        id: self.next,
                        since: now,
                        units: ordered,
                        goal,
                        task: Task::Defend {
                            asset: foundry.id,
                            phase: DefendPhase::Engage { focus: gun },
                        },
                    });
                    self.next += 1;
                }
                continue;
            };
            let mission = &self.list[index];
            let Task::Defend { phase, .. } = mission.task else {
                unreachable!("a defend mission's task");
            };
            let recovering = matches!(phase, DefendPhase::Recover | DefendPhase::Guard);
            let focus = match phase {
                DefendPhase::Engage { focus } => focus,
                DefendPhase::Recover | DefendPhase::Guard => None,
            };
            let lost = lost(observation, gun, focus);
            // A defense after an intruder sends again when it chooses
            // another, a site after its builder included, and when something
            // armed comes: held on a builder, defenders would stand under an
            // escort's fire.
            let resend = recovering
                || lost
                || ((gun.is_some() || quarry.is_some() || intruder(observation, focus))
                    && focus != gun)
                || moved(map, (mission.goal, goal), post, component);
            // A defense out at its attackers short of their value holds its
            // recruits and whoever is back at the post as a reserve there,
            // and sends them on together once all of it would match the
            // attackers: one at a time, they only feed the fight.
            let reserve = post.filter(|post| {
                concentrates(difficulty)
                    && gathering
                    && goal != *post
                    && gathered < go
                    && !mission.units.is_empty()
            });
            let onward =
                concentrates(difficulty) && gathering && post.is_some_and(|post| goal != post);
            let (mut sent, waiting) = self.reserve(
                index,
                (observation, ledger, &mut free),
                &recruits,
                (reserve, onward, resend),
                &front.0,
            );
            let members = &self.list[index].units;
            let away = recall(
                (observation, memory, difficulty),
                ledger,
                (members, siege, post, pursuit),
            );
            sent.retain(|id| !away.contains(id));
            if sent.is_empty() {
                // Releasing may drop emptied missions, so it comes last.
                self.release_from_others(index, &waiting);
                continue;
            }
            sent.sort_unstable();
            sent.dedup();
            let ordered = dispatch(observation, ledger, sent, goal, pursuit);
            let mut joined = waiting;
            if ordered.is_empty() {
                self.release_from_others(index, &joined);
                continue;
            }
            // Only recruits that were given an order join.
            let recruits: Vec<UnitId> = recruits
                .into_iter()
                .filter(|id| ordered.contains(id))
                .collect();
            take(&mut free, &recruits);
            // Turning from an intruder to something armed drops the intruder
            // as the focus, so it is chosen again once that has gone.
            let engage = recovering || lost || gun.is_some() || intruder(observation, focus);
            self.joined(
                index,
                &recruits,
                (resend.then_some(goal), engage, gun, recovering),
                now,
            );
            joined.extend(recruits);
            self.release_from_others(index, &joined);
        }
        // Defenses that stood down go home last, so their orders never
        // take the allowance from a defense answering an attack now.
        self.send_home(returning, ledger);
        shortfalls
    }

    /// Runs each defense that stood down, in `returning`, back to its post:
    /// nothing in sight can hit them, and a hunt would keep chasing whatever
    /// it last touched, such as a scout, across the map. One whose order
    /// finds no room keeps its last order and stays engaged as it was, to
    /// stand down again at a later look.
    fn send_home(
        &mut self,
        returning: Vec<(u64, TilePos, Option<UnitId>, u64)>,
        ledger: &mut Ledger,
    ) {
        for (id, post, focus, since) in returning {
            let Some(mission) = self.list.iter_mut().find(|mission| {
                mission.id == id
                    && matches!(
                        mission.task,
                        Task::Defend {
                            phase: DefendPhase::Recover,
                            ..
                        }
                    )
            }) else {
                continue;
            };
            if ledger.order(run(mission.units.clone(), post)) {
                mission.goal = post;
            } else if let Task::Defend { phase, .. } = &mut mission.task {
                *phase = DefendPhase::Engage { focus };
                mission.since = since;
            }
        }
    }

    /// Puts every engaged defense whose Foundry no threat presses any more,
    /// and none of whose members an enemy in sight can still hit, into
    /// recovery. Returns each with the post beside its Foundry its members
    /// go back to, once this decision's defenses have been sent, and the
    /// focus and start of its engagement.
    fn stand_down(
        &mut self,
        (observation, memory, difficulty): (&ObservationData, &Memory, BotDifficulty),
        (map, frame): (&MapModel, HomeFrame),
        groups: &[(&BuildingObs, Siege<'_>, bool)],
        ledger: &mut Ledger,
    ) -> Vec<(u64, TilePos, Option<UnitId>, u64)> {
        let now = observation.tick;
        let margin = engage_margin(difficulty);
        let mut returning = Vec::new();
        for mission in &mut self.list {
            let Task::Defend { asset, phase } = &mut mission.task else {
                continue;
            };
            let threatened = groups.iter().any(|(foundry, _, _)| foundry.id == *asset);
            // Under fire it fights on, rather than turn its back, unless
            // what it faces out there outweighs it.
            let held = mission
                .units
                .iter()
                .filter_map(|id| mine(observation, *id))
                .any(|unit| {
                    exposed(observation, unit)
                        && !outweighed((observation, memory), &mission.units, unit, margin)
                });
            if let DefendPhase::Engage { focus } = *phase
                && !threatened
                && !held
            {
                // Its members come back to the post beside their Foundry:
                // left where the fight ended, often by the enemy's line,
                // they are let go there and picked off. Without a post, a
                // focus order is replaced so they stop chasing a retreating
                // enemy out of the base.
                let post = observation
                    .my_buildings
                    .iter()
                    .chain(&observation.ally_buildings)
                    .find(|building| building.id == *asset)
                    .and_then(|foundry| waiting_post(map, frame, foundry, mission.goal));
                if let Some(post) = post {
                    returning.push((mission.id, post, focus, mission.since));
                } else if focus.is_some() {
                    ledger.order(hunt(mission.units.clone(), mission.goal));
                }
                *phase = DefendPhase::Recover;
                mission.since = now;
            }
        }
        returning
    }

    /// Notes on each siege in `groups` what its Foundry's defense holds and
    /// could take from `lendable` against ground, not units out on missions
    /// that will not lend them, and leaves to production the batteries that
    /// force cannot beat.
    fn weigh(
        &self,
        (observation, map, memory, ledger): (&ObservationData, &MapModel, &Memory, &Ledger),
        lendable: &[UnitId],
        groups: &mut [(&BuildingObs, Siege<'_>, bool)],
    ) {
        for (foundry, siege, _) in groups {
            let component = map.component(foundry.anchor);
            let defenders = self
                .list
                .iter()
                .filter(|mission| {
                    matches!(mission.task, Task::Defend { asset, .. } if asset == foundry.id)
                })
                .flat_map(|mission| mission.units.iter().copied());
            // A recovering defense lends its units, so they may be in both.
            let mut pool: Vec<UnitId> = lendable.iter().copied().chain(defenders).collect();
            pool.sort_unstable();
            pool.dedup();
            let force: u64 = pool
                .into_iter()
                .filter(|id| !ledger.stuck(*id))
                .filter_map(|id| mine(observation, id))
                .filter(|unit| hits(unit, Domain::Ground))
                .filter(|unit| {
                    unit.kind.stats().domain == Domain::Air || map.component(unit.tile) == component
                })
                .map(value)
                .sum();
            siege.held = force;
            siege.leave_unbeaten(observation, memory, force);
        }
    }

    /// Gives up the defense or guard at `index`, if any, against a threat at
    /// `goal` the seat cannot match there: its members fall back to home's
    /// waiting post, without stopping to fight on the way, and the mission
    /// ends. Without a post on home's ground they are let go where they
    /// stand. Production still answers the shortfall.
    pub(super) fn give_up(
        &mut self,
        index: Option<usize>,
        observation: &ObservationData,
        map: &MapModel,
        frame: HomeFrame,
        goal: TilePos,
        ledger: &mut Ledger,
    ) {
        let Some(index) = index else {
            return;
        };
        let units = self.list[index].units.clone();
        // Ended only once every member has its order: one left on its last
        // order would fight on with no mission to bring it back.
        let mut members = units.clone();
        members.sort_unstable();
        if home_post(observation, map, frame, goal)
            .is_none_or(|post| fall_back(observation, ledger, units, post) == members)
        {
            self.list.remove(index);
        }
    }

    /// Adds `recruits` to the defense at `index`, moves its goal to `goal`
    /// when given, puts it in Engage on `focus` when it should `engage`, and
    /// restarts its clock at `now` when it was `recovering`.
    fn joined(
        &mut self,
        index: usize,
        recruits: &[UnitId],
        (goal, engage, focus, recovering): (Option<TilePos>, bool, Option<UnitId>, bool),
        now: u64,
    ) {
        let mission = &mut self.list[index];
        for id in recruits {
            insert(&mut mission.units, *id);
        }
        if let Some(goal) = goal {
            mission.goal = goal;
        }
        if engage && let Task::Defend { phase, .. } = &mut mission.task {
            *phase = DefendPhase::Engage { focus };
        }
        if recovering {
            mission.since = now;
        }
    }

    /// The `free` units a defense of `members` on `ground` could recruit
    /// against a `need`: not sitting out orders, not a loaded carrier or one
    /// a lift counts on, able to hit a domain in need, and on its ground
    /// unless they fly.
    fn candidates<'a>(
        &self,
        (observation, map, ledger): (&'a ObservationData, &MapModel, &Ledger),
        free: &[UnitId],
        members: &[UnitId],
        need: [u64; 2],
        ground: Option<u32>,
    ) -> Vec<&'a UnitObs> {
        free.iter()
            .filter_map(|id| mine(observation, *id))
            .filter(|unit| !members.contains(&unit.id) && !ledger.stuck(unit.id))
            // A loaded carrier, or one a lift counts on, stays with it.
            .filter(|unit| {
                !super::lift::carrier(unit.kind) || (unit.cargo == 0 && !self.lifts(unit.id))
            })
            .filter(|unit| {
                [Domain::Ground, Domain::Air]
                    .into_iter()
                    .enumerate()
                    .any(|(index, domain)| need[index] > 0 && hits(unit, domain))
            })
            .filter(|unit| {
                unit.kind.stats().domain == Domain::Air || map.component(unit.tile) == ground
            })
            .collect()
    }

    /// What the defense at `index` sends on to its goal this decision, and
    /// the recruits it holds at `reserve`, the waiting post, instead: with a
    /// reserve, its recruits wait there and only the `front` goes on, when
    /// it `resend`s; going `onward` together, the units waiting at the post
    /// go with the recruits; else as before, the recruits, and on `resend`
    /// every member.
    fn reserve(
        &mut self,
        index: usize,
        (observation, ledger, free): (&ObservationData, &mut Ledger, &mut Vec<UnitId>),
        recruits: &[UnitId],
        (reserve, onward, resend): (Option<TilePos>, bool, bool),
        front: &[UnitId],
    ) -> (Vec<UnitId>, Vec<UnitId>) {
        let mut sent = recruits.to_vec();
        let mut waiting = Vec::new();
        if let Some(post) = reserve {
            // Recruits that hit ground wait at the post outside the mission,
            // so no order to it reaches them, and count again as recruits
            // the next decision; those answering only aircraft go on.
            let going: Vec<UnitId>;
            (waiting, going) = recruits.iter().copied().partition(|id| {
                mine(observation, *id).is_some_and(|unit| hits(unit, Domain::Ground))
            });
            let mut walking: Vec<UnitId> = waiting
                .iter()
                .copied()
                .filter(|id| {
                    mine(observation, *id)
                        .is_some_and(|unit| !unit.idle || unit.tile.chebyshev(post) > RESERVE_TILES)
                })
                .collect();
            walking.sort_unstable();
            if !walking.is_empty() {
                fall_back(observation, ledger, walking, post);
            }
            take(free, &waiting);
            ledger.reserve(&waiting);
            sent = going;
            if resend {
                sent.extend_from_slice(front);
            }
        } else if resend {
            sent.extend_from_slice(&self.list[index].units);
        } else if onward {
            // Going on together: the reserve at the post goes with them.
            sent.extend(
                self.list[index]
                    .units
                    .iter()
                    .copied()
                    .filter(|id| !front.contains(id)),
            );
        }
        (sent, waiting)
    }
}

/// How far one of the seat's Foundries' defense falls short of its attackers.
pub(crate) struct Shortfall {
    /// The Foundry's ground: ground units help only from there.
    pub(crate) ground: Option<u32>,
    /// The shortfall, in scrap, against ground and air attackers.
    pub(crate) gap: [u64; 2],
    /// The attackers in sight the shortfall counts, by kind with their
    /// value, so what is trained can be what beats them; empty when the
    /// shortfall answers no particular force.
    pub(crate) attackers: Vec<(UnitKind, u64)>,
    /// Whether the seat reads what the attackers are. One that only judges
    /// their size answers with what is cheapest, which leaves its emergency
    /// structures the rest of the bank.
    pub(crate) read: bool,
}

/// Removes `taken` from the sorted `free`.
fn take(free: &mut Vec<UnitId>, taken: &[UnitId]) {
    free.retain(|id| !taken.contains(id));
}

/// Whether a defense's `focus` no longer holds while it has no `gun` to go
/// after: its target fell, left sight, or is a gun this Foundry no longer
/// faces. Any other focus is focus fire's, kept while it lasts.
fn lost(observation: &ObservationData, gun: Option<UnitId>, focus: Option<UnitId>) -> bool {
    gun.is_none()
        && focus.is_some_and(|id| {
            observation
                .enemy_units
                .iter()
                .find(|enemy| enemy.id == id)
                .is_none_or(walking_gun)
        })
}

/// Runs the armed members of a defense hunting or holding an intruder that
/// stand past what it defends back to the `post`: hunting, a defender chases
/// whatever it fights until that dies or leaves sight, out across the map.
/// Past is farther from every own building than the `siege`'s threats
/// stand, with the member's own reach on top. Returns those sent running.
fn recall(
    (observation, memory, difficulty): (&ObservationData, &Memory, BotDifficulty),
    ledger: &mut Ledger,
    (members, siege, post, pursuit): (&[UnitId], &Siege<'_>, Option<TilePos>, Pursuit),
) -> Vec<UnitId> {
    let (Some(post), Pursuit::Hunt | Pursuit::Quarry(_)) = (post, pursuit) else {
        return Vec::new();
    };
    let margin = engage_margin(difficulty);
    let area = siege
        .units
        .iter()
        .filter_map(|threat| ground_reach(threat))
        .max()
        .unwrap_or(THREAT_GAP);
    // One under fire out there fights on unless outweighed where it
    // stands: running, it would take the fire without answering it.
    let away: Vec<UnitId> = members
        .iter()
        .filter_map(|id| mine(observation, *id))
        .filter(|unit| !unit.kind.stats().weapons.is_empty())
        .filter(|unit| {
            let own = crate::defenses::reach(unit.kind).ceil().to_num::<i32>();
            !defended(observation, unit.tile, area + own)
        })
        .filter(|unit| {
            !exposed(observation, unit) || outweighed((observation, memory), members, unit, margin)
        })
        .map(|unit| unit.id)
        .collect();
    if away.is_empty() || !ledger.order(run(away.clone(), post)) {
        return Vec::new();
    }
    away
}

/// Whether an enemy in sight, unit or building, reaches `unit` in its own
/// domain: a landed aircraft is hit as a ground body.
fn exposed(observation: &ObservationData, unit: &UnitObs) -> bool {
    let reach = |weapons: &[WeaponStats], domain: Domain| {
        weapons
            .iter()
            .filter(|weapon| weapon.targets.covers(domain))
            .map(|weapon| weapon.range)
            .max()
            .unwrap_or(Fx::ZERO)
    };
    let domain = unit.body_domain();
    let near =
        |gap: i32, reach: Fx| reach > Fx::ZERO && Fx::from_num(gap) <= reach + Fx::from_num(1);
    observation.enemy_units.iter().any(|enemy| {
        let reach = if domain == Domain::Ground {
            crate::defenses::reach(enemy.kind)
        } else {
            reach(enemy.kind.stats().weapons, domain)
        };
        near(unit.tile.chebyshev(enemy.tile), reach)
    }) || observation.enemy_buildings.iter().any(|building| {
        let gap = gap(building.anchor, building.kind.size(), unit.tile, (1, 1));
        let tier = building.kind.tiers()[usize::from(building.tier)];
        near(gap, reach(tier.weapons, domain))
    })
}

/// Whether the known enemies that would join a fight where `unit` stands,
/// and the enemy guns that reach it, outweigh the `members` of its defense
/// beside it that can fight them by the `margin` per mille a fight needs,
/// all judged in the domain `unit` is hit in: an aircraft by what can hit
/// aircraft there.
fn outweighed(
    (observation, memory): (&ObservationData, &Memory),
    members: &[UnitId],
    unit: &UnitObs,
    margin: u64,
) -> bool {
    let domain = unit.body_domain();
    let met = if domain == Domain::Ground {
        // The guns alone: remembered units near are the meeting's already.
        let counted: Vec<UnitId> = memory.units().iter().map(|seen| seen.id).collect();
        meeting::opposed(observation, memory, unit.tile)
            + defense_around(observation, memory, &[unit.tile], &counted)
    } else {
        let near = |reach: Fx, tile: TilePos| {
            reach > Fx::ZERO
                && tile.chebyshev(unit.tile) <= reach.ceil().to_num::<i32>().max(LOCAL_REACH)
        };
        observation
            .enemy_units
            .iter()
            .filter(|enemy| near(air_reach(enemy.kind.stats().weapons), enemy.tile))
            .map(value)
            .sum::<u64>()
            + observation
                .enemy_buildings
                .iter()
                .filter(|building| {
                    let tier = building.kind.tiers()[usize::from(building.tier)];
                    near(air_reach(tier.weapons), building.anchor)
                })
                .map(building_value)
                .sum::<u64>()
    };
    let could: u64 = members
        .iter()
        .filter_map(|id| mine(observation, *id))
        .filter(|other| other.tile.chebyshev(unit.tile) <= GATHER_TILES)
        .filter(|other| !other.kind.stats().weapons.is_empty())
        .filter(|other| domain == Domain::Air || hits(other, Domain::Ground))
        .map(value)
        .sum();
    met > 0 && could * 1_000 < met * margin
}

/// The longest reach of `weapons` against aircraft.
fn air_reach(weapons: &[WeaponStats]) -> Fx {
    weapons
        .iter()
        .filter(|weapon| weapon.targets.covers(Domain::Air))
        .map(|weapon| weapon.range)
        .max()
        .unwrap_or(Fx::ZERO)
}

/// Whether `tile` lies within `reach` of one of the seat's or its allies'
/// buildings, as a threat to them must.
fn defended(observation: &ObservationData, tile: TilePos, reach: i32) -> bool {
    observation
        .my_buildings
        .iter()
        .chain(&observation.ally_buildings)
        .any(|building| gap(building.anchor, building.kind.size(), tile, (1, 1)) < reach)
}

/// Whether `focus` is an unarmed enemy in sight: an intruder a defense
/// chose, such as a builder.
fn intruder(observation: &ObservationData, focus: Option<UnitId>) -> bool {
    focus.is_some_and(|id| {
        observation
            .enemy_units
            .iter()
            .any(|enemy| enemy.id == id && enemy.kind.stats().weapons.is_empty())
    })
}

/// How defenders go after a Foundry's threats.
#[derive(Clone, Copy)]
enum Pursuit {
    /// March on the nearest threat, engaging everything on the way.
    Hunt,
    /// Move on a gun out of sight without stopping for anything on the way.
    Advance,
    /// Attack a gun in sight.
    Gun(UnitId),
    /// Attack the intruder chosen among unarmed ones.
    Quarry(AttackTarget),
    /// Wait beside a building an aircraft raids, moving there without
    /// chasing it: hunting, a defender follows a flyer out of cover.
    Guard,
    /// Fall back to the waiting post without stopping to fight on the way.
    Fall,
}

impl Pursuit {
    /// The unit a defense pursuing so holds as its focus: a gun it goes at,
    /// or the intruder it chose.
    fn held(self) -> Option<UnitId> {
        match self {
            Self::Gun(id) | Self::Quarry(AttackTarget::Unit(id)) => Some(id),
            _ => None,
        }
    }

    /// How a defense goes after a gun in sight it is `shelling` by, else the
    /// `quarry` it chose, else the gun `unseen` when no `grounded` threat
    /// stands, else the flyers it guards against, else the grounded threat.
    fn of(
        shelling: Option<&UnitObs>,
        quarry: Option<(AttackTarget, TilePos)>,
        grounded: Option<TilePos>,
        unseen: bool,
    ) -> Self {
        match (shelling, quarry, grounded) {
            (Some(threat), _, _) => Self::Gun(threat.id),
            (None, Some((target, _)), _) => Self::Quarry(target),
            (None, None, None) if unseen => Self::Advance,
            (None, None, None) => Self::Guard,
            (None, None, Some(_)) => Self::Hunt,
        }
    }
}

/// Sends defenders after a Foundry's threats: those that can hit ground as
/// `pursuit` says and the rest on a hunt to `goal`. Returns the units given an
/// order, none when the first order was refused.
fn dispatch(
    observation: &ObservationData,
    ledger: &mut Ledger,
    units: Vec<UnitId>,
    goal: TilePos,
    pursuit: Pursuit,
) -> Vec<UnitId> {
    let (shooters, rest): (Vec<UnitId>, Vec<UnitId>) = match pursuit {
        Pursuit::Fall => return fall_back(observation, ledger, units, goal),
        Pursuit::Hunt => (Vec::new(), units),
        Pursuit::Guard => {
            let command = Command::Advance {
                units: units.clone(),
                goal,
                queue: false,
            };
            return if ledger.order(command) {
                units
            } else {
                Vec::new()
            };
        }
        Pursuit::Advance | Pursuit::Gun(_) | Pursuit::Quarry(_) => units
            .into_iter()
            .partition(|id| mine(observation, *id).is_some_and(|unit| hits(unit, Domain::Ground))),
    };
    if shooters.is_empty() {
        return if ledger.order(hunt(rest.clone(), goal)) {
            rest
        } else {
            Vec::new()
        };
    }
    let command = match pursuit {
        Pursuit::Gun(gun) => Command::Attack {
            units: shooters.clone(),
            target: AttackTarget::Unit(gun),
            queue: false,
        },
        Pursuit::Quarry(target) => Command::Attack {
            units: shooters.clone(),
            target,
            queue: false,
        },
        Pursuit::Hunt | Pursuit::Advance | Pursuit::Fall | Pursuit::Guard => Command::Advance {
            units: shooters.clone(),
            goal,
            queue: false,
        },
    };
    if !ledger.order(command) {
        return Vec::new();
    }
    let mut ordered = shooters;
    if !rest.is_empty() && ledger.order(hunt(rest.clone(), goal)) {
        ordered.extend(rest);
        ordered.sort_unstable();
    }
    ordered
}

/// Sends `units` back to the waiting `post`: those not already at it run, so
/// they do not stop to fight whatever stands on the way, and those at it
/// hunt there, so a defender already fighting at the post fights on. Returns
/// the units given an order, sorted.
fn fall_back(
    observation: &ObservationData,
    ledger: &mut Ledger,
    units: Vec<UnitId>,
    post: TilePos,
) -> Vec<UnitId> {
    let (far, near): (Vec<UnitId>, Vec<UnitId>) = units.into_iter().partition(|id| {
        mine(observation, *id).is_some_and(|unit| unit.tile.chebyshev(post) > RESERVE_TILES)
    });
    let mut ordered = Vec::new();
    for (units, order) in [(far, run as fn(_, _) -> _), (near, hunt)] {
        if !units.is_empty() && ledger.order(order(units.clone(), post)) {
            ordered.extend(units);
        }
    }
    ordered.sort_unstable();
    ordered
}

/// Whether a defense with its goal at `from` moves enough going to `to` to
/// order its members again: far, on its own ground, or between the waiting
/// `post` and the fight however close the two stand. Chasing a threat off
/// the Foundry's ground, such as a flyer over a chasm, would stall every
/// defender each time it moved.
fn moved(
    map: &MapModel,
    (from, to): (TilePos, TilePos),
    post: Option<TilePos>,
    ground: Option<u32>,
) -> bool {
    (from.chebyshev(to) > RETARGET_TILES && map.component(to) == ground)
        || post.is_some_and(|post| (to == post) != (from == post))
}

/// Whether `mission`'s defenders wait beside `foundry` rather than meet a
/// ground force worth `met`, with `have` all of them and their recruits
/// against ground and `front` those out at it: short of `go`, the force the
/// seat's rung goes out to it with, at the post; once out, when all of them
/// are short of that and those out are short of two thirds of the force, so
/// a front the reserve cannot bring level comes back to it.
fn waits(
    mission: Option<&Mission>,
    foundry: &BuildingObs,
    [have, front]: [u64; 2],
    [met, go]: [u64; 2],
) -> bool {
    let out = mission.is_some_and(|mission| {
        !mission.units.is_empty()
            && !ring(foundry.anchor, foundry.kind.size()).any(|tile| tile == mission.goal)
    });
    if out {
        have < go && front * 3 < met * 2
    } else {
        have < go
    }
}

/// The defenders among `members` out away from the waiting `post` or on
/// their way, with what those near the post, at the `fight` or on the way
/// between are worth against ground; the rest stand idle at the post as its
/// reserve. Without a post all are out. One farther from the fight than the
/// post is, such as a defender off chasing a fleeing enemy, is in neither.
fn front(
    observation: &ObservationData,
    members: &[UnitId],
    post: Option<TilePos>,
    fight: TilePos,
) -> (Vec<UnitId>, u64) {
    let out: Vec<&UnitObs> = members
        .iter()
        .filter_map(|id| mine(observation, *id))
        .filter(|unit| {
            post.is_none_or(|post| !unit.idle || unit.tile.chebyshev(post) > RESERVE_TILES)
        })
        .collect();
    let way = post.map_or(0, |post| post.chebyshev(fight));
    let near = |unit: &UnitObs| unit.tile.chebyshev(fight) <= way + GATHER_TILES;
    let value = out
        .iter()
        .filter(|unit| hits(unit, Domain::Ground) && near(unit))
        .map(|unit| value(unit))
        .sum();
    (out.iter().map(|unit| unit.id).collect(), value)
}

/// Where defenders short of a ground force wait: beside `foundry` on its
/// ground, nearest `threat`. Defenders sent one by one into a force they
/// cannot match only feed it; beside the Foundry, attackers that come on meet
/// them all.
fn waiting_post(
    map: &MapModel,
    frame: HomeFrame,
    foundry: &BuildingObs,
    threat: TilePos,
) -> Option<TilePos> {
    let component = map.component(foundry.anchor)?;
    ring(foundry.anchor, foundry.kind.size())
        .filter(|tile| map.component(*tile) == Some(component))
        .min_by_key(|tile| frame.rank(doubled(threat), doubled(*tile)))
}

/// What a defense against `siege`'s ground force worth `met` must hold to go
/// out at it: the rung's edge at the seat's `own` Foundry when no gun,
/// standing or walking, is among the attackers, even odds otherwise. Guns
/// shell the post rather than come to it, so waiting there for the edge
/// only gets the defenders shelled, and an ally's defense has no shortfall
/// training behind a wait.
fn sortie(own: bool, siege: &Siege<'_>, met: u64, difficulty: BotDifficulty) -> u64 {
    if own && siege.batteries.is_empty() && !siege.units.iter().any(|threat| walking_gun(threat)) {
        met * engage_margin(difficulty) / 1_000
    } else {
        met
    }
}

/// Per mille of the attackers' worth a defense must hold to go out at them:
/// the upper rungs go out only with the edge their attacks keep before
/// withdrawing, the lower at even odds. A difficulty limit on how well a seat
/// defends.
pub(super) fn engage_margin(difficulty: BotDifficulty) -> u64 {
    match difficulty {
        BotDifficulty::Scrapheap | BotDifficulty::Standard => 1_000,
        BotDifficulty::Veteran | BotDifficulty::Prime => 1_250,
    }
}

/// How near the waiting post a defender stands to count as its reserve: a
/// computation bound on "at the post", not a count of units.
const RESERVE_TILES: i32 = 3;

/// How near the waiting post or the fight a defender stands to count as
/// there: a model of "together", since one farther off arrives after the
/// fight began. Not a count of units.
pub(super) const GATHER_TILES: i32 = 6;

/// What `units` that can hit ground are worth where a defense would fight:
/// those within the gather distance of its waiting `post` or of its `goal`.
fn together<'a>(
    observation: &ObservationData,
    units: impl Iterator<Item = &'a UnitId>,
    [post, goal]: [TilePos; 2],
) -> u64 {
    let mut ids: Vec<UnitId> = units.copied().collect();
    ids.sort_unstable();
    ids.dedup();
    ids.into_iter()
        .filter_map(|id| mine(observation, id))
        .filter(|unit| hits(unit, Domain::Ground))
        .filter(|unit| {
            unit.tile.chebyshev(post) <= GATHER_TILES || unit.tile.chebyshev(goal) <= GATHER_TILES
        })
        .map(value)
        .sum()
}

/// Whether a threat at `goal`, grouped to `foundry`, is one the seat never
/// gives up: at home, or at its last Foundry.
fn kept(
    observation: &ObservationData,
    map: &MapModel,
    foundry: &BuildingObs,
    goal: TilePos,
) -> bool {
    at_home(observation, map, goal)
        || (crate::investments::built_foundries(&observation.my_buildings).count() <= 1
            && gap(foundry.anchor, foundry.kind.size(), goal, (1, 1)) <= LOCAL_REACH)
}

/// Whether `tile` lies at home: within local reach of the seat's start.
pub(super) fn at_home(observation: &ObservationData, map: &MapModel, tile: TilePos) -> bool {
    map.start(observation.me)
        .is_some_and(|start| gap(start, BuildingKind::Foundry.size(), tile, (1, 1)) <= LOCAL_REACH)
}

/// Where defenders given up on a threat at `toward` fall back to: the
/// waiting post beside the seat's home-nearest Foundry.
pub(super) fn home_post(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    toward: TilePos,
) -> Option<TilePos> {
    let home = *foundries_from_home(&observation.my_buildings, frame).first()?;
    waiting_post(map, frame, home, toward)
}

/// What threatens a Foundry: enemies in sight and shelling enemy buildings,
/// or, with neither, a gun out of sight.
#[derive(Default)]
struct Siege<'a> {
    /// Armed enemies in sight.
    units: Vec<&'a UnitObs>,
    /// Unarmed enemies on the ground in sight among the seat's buildings:
    /// scouts and builders.
    strays: Vec<&'a UnitObs>,
    /// Known enemy buildings, in sight or remembered, whose shells land near
    /// the base or that stand among the seat's buildings, each with the tile
    /// beside it defenders go to.
    batteries: Vec<(&'a BuildingObs, TilePos)>,
    /// Where a gun shelling the base from out of sight probably stands.
    unseen: Option<TilePos>,
    /// What armed enemies remembered out of sight near the buildings are
    /// worth, against ground and air, by how sure the seat is they are still
    /// there.
    remembered: [u64; 2],
    /// Those remembered enemies, by id.
    recalled: Vec<UnitId>,
    /// What the units the Foundry's defense holds or could take on its
    /// ground are worth against ground: the most it could bring.
    held: u64,
    /// Whether something armed threatens the Foundry that the lists above
    /// leave out: a gun the seat cannot beat yet, left to production, or a
    /// gun out of sight shelling it while threats in sight hold its entry.
    armed: bool,
}

impl Siege<'_> {
    /// Leaves a battery among defenses the seat's `force` cannot beat now to
    /// production: fighters sent beside it would only feed its guns. An armed
    /// one left still marks the siege armed.
    fn leave_unbeaten(&mut self, observation: &ObservationData, memory: &Memory, force: u64) {
        let beaten = |beside: TilePos| defense(observation, memory, beside) * 3 / 2 <= force;
        self.armed |= self
            .batteries
            .iter()
            .any(|(battery, beside)| !beaten(*beside) && armed(battery));
        self.batteries.retain(|(_, beside)| beaten(*beside));
    }

    /// Whether nothing armed threatens: no armed enemy in sight, no gun or
    /// mine among its batteries, none left to production and none out of
    /// sight.
    fn unarmed_only(&self) -> bool {
        self.units.is_empty()
            && self.unseen.is_none()
            && !self.armed
            && !self.batteries.iter().any(|(building, _)| armed(building))
    }

    /// How far `have` falls short of the armed attackers, `pressing`, and
    /// the enemy producers and builders among the buildings: a seat with no
    /// army must train one for them. Scouts don't count, or every look at the
    /// base would cost the seat its economy.
    fn gap(&self, pressing: [u64; 2], have: [u64; 2]) -> [u64; 2] {
        let builders: u64 = self
            .strays
            .iter()
            .filter(|stray| stray.kind.stats().harvest.is_some())
            .map(|stray| worth(stray.kind, stray.hp))
            .sum();
        let producers = self.producers() + builders;
        [0, 1].map(|index| {
            (pressing[index] + if index == 0 { producers } else { 0 }).saturating_sub(have[index])
        })
    }

    /// The whole price of the unarmed enemy producers among its batteries.
    fn producers(&self) -> u64 {
        self.batteries
            .iter()
            .filter(|(building, _)| {
                building.kind.tier_stats(building.tier).weapons.is_empty()
                    && !building.kind.base_stats().produces.is_empty()
            })
            .map(|(building, _)| u64::from(building.kind.invested_cost(building.tier)))
            .sum()
    }

    /// Its armed enemies in sight, by kind, with their value.
    /// The armed attackers in a domain that `gap` leaves short, so an answer
    /// is weighed against the attackers it must still beat, not those
    /// already covered; none for a seat that does not `read` what they are
    /// and judges the threat by its size.
    fn attackers(&self, gap: [u64; 2], read: bool) -> Vec<(UnitKind, u64)> {
        self.units
            .iter()
            .filter(|_| read)
            .filter(|threat| gap[usize::from(threat.body_domain() == Domain::Air)] > 0)
            .map(|threat| (threat.kind, value(threat)))
            .collect()
    }

    /// Where its threats on the ground stand: enemies in sight and the tiles
    /// beside its batteries.
    fn grounded(&self) -> impl Iterator<Item = TilePos> + '_ {
        self.units
            .iter()
            .chain(&self.strays)
            .filter(|threat| threat.body_domain() == Domain::Ground)
            .map(|threat| threat.tile)
            .chain(self.batteries.iter().map(|(_, beside)| *beside))
    }

    /// What its armed attackers are worth against ground and air, by half
    /// again: those in sight and remembered near the buildings, or on the
    /// ground what the defenders would `meet` where they fight, if more.
    fn pressing_value(&self, meet: u64) -> [u64; 2] {
        [Domain::Ground, Domain::Air].map(|domain| {
            let near = self
                .units
                .iter()
                .filter(|threat| threat.body_domain() == domain)
                .map(|threat| value(threat))
                .sum::<u64>()
                + self.remembered[usize::from(domain == Domain::Air)];
            let met = if domain == Domain::Ground { meet } else { 0 };
            near.max(met) * 3 / 2
        })
    }

    /// Its armed enemies in sight and remembered near the buildings, by id.
    fn pressing(&self) -> Vec<UnitId> {
        self.units
            .iter()
            .map(|threat| threat.id)
            .chain(self.recalled.iter().copied())
            .collect()
    }

    /// What defenders go after when nothing armed threatens, nearest by
    /// `rank` within each kind: a builder raising an enemy site among the
    /// seat's buildings, since killing it stops the site soonest; then that
    /// site or another building among them; then any other builder; a scout
    /// last. Each with where it stands, or the tile beside it.
    fn quarry<K: Ord>(&self, rank: impl Fn(TilePos) -> K) -> Option<(AttackTarget, TilePos)> {
        // An escort remembered just out of sight makes it a fight to gather
        // for, not an intruder to pick off.
        if !self.unarmed_only() || self.remembered != [0, 0] {
            return None;
        }
        let builder = |stray: &UnitObs| stray.kind.stats().harvest.is_some();
        let raising = |stray: &UnitObs| {
            builder(stray)
                && self.batteries.iter().any(|(site, _)| {
                    !site.built
                        && site.player == stray.player
                        && gap(site.anchor, site.kind.size(), stray.tile, (1, 1)) <= 1
                })
        };
        let stray = |pick: &dyn Fn(&UnitObs) -> bool| {
            self.strays
                .iter()
                .filter(|stray| pick(stray))
                .min_by_key(|stray| (rank(stray.tile), stray.id))
                .map(|stray| (AttackTarget::Unit(stray.id), stray.tile))
        };
        stray(&raising)
            .or_else(|| {
                self.batteries
                    .iter()
                    .min_by_key(|(building, beside)| (rank(*beside), building.id))
                    .map(|(building, beside)| (super::raid::aim(building), *beside))
            })
            .or_else(|| stray(&builder))
            .or_else(|| stray(&|_| true))
    }

    /// Where its grounded threat nearest by `rank` stands: an armed one
    /// before scouts, builders and unarmed sites, however much nearer those
    /// stand.
    fn nearest_grounded<K: Ord>(&self, rank: impl Fn(TilePos) -> K) -> Option<TilePos> {
        self.armed_grounded()
            .min_by_key(|tile| rank(*tile))
            .or_else(|| self.grounded().min_by_key(|tile| rank(*tile)))
    }

    /// Where its armed threats on the ground stand: armed enemies in sight
    /// and the tiles beside its batteries that can hurt ground units.
    fn armed_grounded(&self) -> impl Iterator<Item = TilePos> + '_ {
        self.units
            .iter()
            .filter(|threat| threat.body_domain() == Domain::Ground)
            .map(|threat| threat.tile)
            .chain(
                self.batteries
                    .iter()
                    .filter(|(building, _)| armed(building))
                    .map(|(_, beside)| *beside),
            )
    }

    /// Where its enemies in sight stand, on the ground or in the air.
    fn sighted(&self) -> impl Iterator<Item = TilePos> + '_ {
        self.units
            .iter()
            .chain(&self.strays)
            .map(|threat| threat.tile)
    }

    /// What the unarmed intruders cost, against ground and air: the strays,
    /// and the buildings among the batteries that carry no gun against
    /// ground, such as a Flak Turret. Defenders worth as much go after them.
    fn unarmed(&self) -> [u64; 2] {
        // An enemy producer going up among the seat's buildings is worth its
        // whole price, whatever its health: units will come out of it.
        let raised: u64 = self
            .batteries
            .iter()
            .filter(|(building, _)| {
                !building
                    .kind
                    .tier_stats(building.tier)
                    .weapons
                    .iter()
                    .any(|weapon| weapon.targets.ground)
            })
            .map(|(building, _)| {
                if building.kind.base_stats().produces.is_empty() {
                    building_value(building)
                } else {
                    u64::from(building.kind.invested_cost(building.tier))
                }
            })
            .sum();
        [Domain::Ground, Domain::Air].map(|domain| {
            let strays: u64 = self
                .strays
                .iter()
                .filter(|stray| stray.body_domain() == domain)
                .map(|stray| worth(stray.kind, stray.hp))
                .sum();
            strays + if domain == Domain::Ground { raised } else { 0 }
        })
    }
}

/// Visible enemies and shelling enemy buildings that threaten the seat's
/// buildings, grouped by the built Foundry each is nearest, home-nearest
/// Foundry first, then the seat's other Foundries shelled by a gun out of
/// sight, then enemies that could hit an ally's buildings, grouped by the
/// ally's Foundry, each marked whether the Foundry is the seat's own. An
/// enemy joins only if it stands on or beside that Foundry's ground.
fn threats<'a>(
    observation: &'a ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    memory: &Memory,
) -> Vec<(&'a BuildingObs, Siege<'a>, bool)> {
    let mut groups: Vec<(&BuildingObs, Siege, bool)> = Vec::new();
    let mut claimed: Vec<UnitId> = Vec::new();
    // Remembered buildings share one id, so batteries are told apart by anchor.
    let mut claimed_batteries: Vec<TilePos> = Vec::new();
    for (buildings, own) in [
        (&observation.my_buildings, true),
        (&observation.ally_buildings, false),
    ] {
        let side = own.then_some(observation.me);
        for (foundry, siege) in besieged(
            observation,
            map,
            frame,
            (buildings, side),
            (&claimed, &claimed_batteries),
            own.then_some(memory),
        ) {
            claimed.extend(
                siege
                    .units
                    .iter()
                    .chain(&siege.strays)
                    .map(|threat| threat.id),
            );
            claimed_batteries.extend(siege.batteries.iter().map(|(battery, _)| battery.anchor));
            groups.push((foundry, siege, own));
        }
        if own {
            // A gun out of sight waits until the threats in sight are gone:
            // answered together, shelling that never stops keeps the defense
            // recruiting ground units and the army never leaves home.
            for (foundry, gun) in unseen(observation, map, frame) {
                if let Some((_, siege, _)) = groups
                    .iter_mut()
                    .find(|(other, _, _)| other.id == foundry.id)
                {
                    siege.armed = true;
                } else {
                    let siege = Siege {
                        unseen: Some(gun),
                        ..Siege::default()
                    };
                    groups.push((foundry, siege, true));
                }
            }
        }
    }
    groups
}

/// Visible enemies not `claimed` that could hit one of `buildings`, and known
/// enemy buildings whose anchors are not in `claimed_batteries` shelling near
/// them, grouped by the built Foundry among them each is nearest,
/// home-nearest first. For buildings of `side`, the seat's own, unarmed
/// enemies and enemy buildings within the threat gap of one count too, when
/// they stand on its side of the map: nearer its start by ground than any
/// hostile start. With the seat's `memory`, armed enemies remembered out of
/// sight where they would join a group count toward its force.
fn besieged<'a>(
    observation: &'a ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    (buildings, side): (&'a [BuildingObs], Option<PlayerId>),
    (claimed, claimed_batteries): (&[UnitId], &[TilePos]),
    memory: Option<&Memory>,
) -> Vec<(&'a BuildingObs, Siege<'a>)> {
    let ours = |tile: TilePos| {
        side.is_some_and(|me| map.distance(me, tile) < map.hostile_distance(me, tile))
    };
    let foundries = foundries_from_home(buildings, frame);
    let mut groups: Vec<(&BuildingObs, Siege)> = foundries
        .iter()
        .map(|foundry| (*foundry, Siege::default()))
        .collect();
    let shelled = shelled(observation, buildings);
    let nearest = |anchor: TilePos, size: (i32, i32), centre: (i64, i64)| {
        (0..foundries.len()).min_by_key(|index| {
            let foundry = foundries[*index];
            (
                gap(foundry.anchor, foundry.kind.size(), anchor, size),
                frame.rank(centre, footprint_centre(foundry.kind, foundry.anchor)),
            )
        })
    };
    for enemy in &observation.enemy_units {
        if claimed.contains(&enemy.id) {
            continue;
        }
        let armed = ground_reach(enemy);
        // An unarmed flyer is a spotter or a carrier, which the guns it
        // spots for and the landing it carries answer; only scouts and
        // builders on the ground are strays.
        if armed.is_none() && (enemy.body_domain() != Domain::Ground || !ours(enemy.tile)) {
            continue;
        }
        let reach = armed.unwrap_or(THREAT_GAP);
        // A gun shelling the base from beyond the threat gap still counts.
        let near = buildings
            .iter()
            .any(|building| gap(building.anchor, building.kind.size(), enemy.tile, (1, 1)) < reach)
            || (armed.is_some() && shelled.iter().any(|impact| lobs(enemy, *impact)));
        if !near {
            continue;
        }
        let Some(index) = nearest(enemy.tile, (1, 1), doubled(enemy.tile)) else {
            continue;
        };
        let reachable = map
            .component(foundries[index].anchor)
            .is_some_and(|component| {
                map.component(enemy.tile) == Some(component) || map.touches(enemy.tile, component)
            });
        if reachable && armed.is_some() {
            groups[index].1.units.push(enemy);
        } else if reachable {
            groups[index].1.strays.push(enemy);
        }
    }
    // An attacker that steps out of sight is still there: counting only those
    // in sight, a defense facing one that blinks at the edge of its sight
    // marches out and back with every blink.
    for seen in memory.map_or(&[][..], Memory::units) {
        if seen.seen >= observation.tick {
            continue;
        }
        let reach = crate::defenses::reach(seen.kind);
        if reach <= Fx::ZERO {
            continue;
        }
        let reach = reach.ceil().to_num::<i32>().max(THREAT_GAP);
        if !buildings
            .iter()
            .any(|building| gap(building.anchor, building.kind.size(), seen.tile, (1, 1)) < reach)
        {
            continue;
        }
        let Some(index) = nearest(seen.tile, (1, 1), doubled(seen.tile)) else {
            continue;
        };
        let reachable = map
            .component(foundries[index].anchor)
            .is_some_and(|component| {
                map.component(seen.tile) == Some(component) || map.touches(seen.tile, component)
            });
        if reachable {
            let domain = usize::from(seen.kind.stats().domain == Domain::Air);
            groups[index].1.remembered[domain] +=
                seen.value(observation.tick) * against_machines(seen.kind) / 1_000;
            groups[index].1.recalled.push(seen.id);
        }
    }
    for building in &observation.enemy_buildings {
        let size = building.kind.size();
        let among = !building.provisional
            && ours(building.anchor)
            && buildings
                .iter()
                .any(|own| gap(own.anchor, own.kind.size(), building.anchor, size) < THREAT_GAP);
        if claimed_batteries.contains(&building.anchor)
            || (!among && !shelled.iter().any(|impact| battery(building, *impact)))
        {
            continue;
        }
        let centre = footprint_centre(building.kind, building.anchor);
        let Some(index) = nearest(building.anchor, size, centre) else {
            continue;
        };
        let foundry = foundries[index];
        let home = footprint_centre(foundry.kind, foundry.anchor);
        let component = map.component(foundry.anchor);
        // A battery off the Foundry's ground is left to production, as a
        // unit across a chasm is.
        let beside = ring(building.anchor, size)
            .filter(|tile| component.is_some() && map.component(*tile) == component)
            .min_by_key(|tile| frame.rank(home, doubled(*tile)));
        if let Some(beside) = beside {
            groups[index].1.batteries.push((building, beside));
        }
    }
    groups.retain(|(_, siege)| {
        !siege.units.is_empty()
            || !siege.strays.is_empty()
            || !siege.batteries.is_empty()
            || siege.remembered != [0, 0]
    });
    groups
}

/// Whether `building` can hurt ground units: a gun, or a mine.
fn armed(building: &BuildingObs) -> bool {
    building
        .kind
        .tier_stats(building.tier)
        .weapons
        .iter()
        .any(|weapon| weapon.targets.ground)
        || building.kind == BuildingKind::ScuttleCharge
}

/// The built Foundries among `buildings`, nearest home first.
fn foundries_from_home(buildings: &[BuildingObs], frame: HomeFrame) -> Vec<&BuildingObs> {
    let mut foundries: Vec<&BuildingObs> = crate::investments::built_foundries(buildings).collect();
    foundries.sort_by_key(|foundry| {
        (
            frame.rank(frame.home, footprint_centre(foundry.kind, foundry.anchor)),
            foundry.id,
        )
    });
    foundries
}

/// Whether `enemy` sends shells that could land on `impact`.
pub(super) fn lobs(enemy: &UnitObs, impact: TilePos) -> bool {
    enemy
        .kind
        .stats()
        .weapons
        .iter()
        .any(|weapon| shells(weapon, doubled(enemy.tile), impact))
}

/// Impacts of hostile shells near any of `buildings`.
fn shelled(observation: &ObservationData, buildings: &[BuildingObs]) -> Vec<TilePos> {
    observation
        .incoming_shells
        .iter()
        .copied()
        .filter(|impact| {
            buildings.iter().any(|building| {
                gap(building.anchor, building.kind.size(), *impact, (1, 1)) < THREAT_GAP
            })
        })
        .collect()
}

/// Whether the known enemy `building` could have sent a shell landing on
/// `impact`. A site in sight cannot fire; a remembered one may have been
/// finished since it was seen.
pub(super) fn battery(building: &BuildingObs, impact: TilePos) -> bool {
    let centre = footprint_centre(building.kind, building.anchor);
    (building.built || !building.seen)
        && building
            .kind
            .tier_stats(building.tier)
            .weapons
            .iter()
            .any(|weapon| shells(weapon, centre, impact))
}

/// Whether `weapon`, fired from the doubled position `from`, sends shells
/// that could land on `impact`.
fn shells(weapon: &WeaponStats, from: (i64, i64), impact: TilePos) -> bool {
    let reach = 2 * i64::from(weapon.range.ceil().to_num::<i32>());
    let to = doubled(impact);
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    weapon.targets.ground
        && (weapon.indirect || weapon.projectile.is_some())
        && dx * dx + dy * dy <= reach * reach
}

/// The seat's built Foundries shelled by a gun out of sight, home-nearest
/// first, each with the tile the gun probably stands at: a hostile shell
/// lands near the seat's buildings, no visible enemy or known enemy building
/// could have fired it, and the built Foundry nearest the impact answers for
/// it. Of several such impacts, the one nearest that Foundry counts.
fn unseen<'a>(
    observation: &'a ObservationData,
    map: &MapModel,
    frame: HomeFrame,
) -> Vec<(&'a BuildingObs, TilePos)> {
    let fired = |impact: TilePos| {
        let units = observation
            .enemy_units
            .iter()
            .any(|enemy| lobs(enemy, impact));
        units
            || observation
                .enemy_buildings
                .iter()
                .any(|building| battery(building, impact))
    };
    let impacts: Vec<TilePos> = shelled(observation, &observation.my_buildings)
        .into_iter()
        .filter(|impact| !fired(*impact))
        .collect();
    if impacts.is_empty() {
        return Vec::new();
    }
    let foundries = foundries_from_home(&observation.my_buildings, frame);
    let nearest = |impact: TilePos| {
        foundries.iter().copied().min_by_key(|foundry| {
            (
                gap(foundry.anchor, foundry.kind.size(), impact, (1, 1)),
                frame.rank(
                    doubled(impact),
                    footprint_centre(foundry.kind, foundry.anchor),
                ),
            )
        })
    };
    foundries
        .iter()
        .filter_map(|foundry| {
            let centre = footprint_centre(foundry.kind, foundry.anchor);
            let impact = impacts
                .iter()
                .copied()
                .filter(|impact| nearest(*impact).is_some_and(|other| other.id == foundry.id))
                .min_by_key(|impact| frame.rank(centre, doubled(*impact)))?;
            let gun = gun(
                observation,
                map,
                frame,
                impact,
                map.component(foundry.anchor),
            )?;
            Some((*foundry, gun))
        })
        .collect()
}

/// Where a gun that shelled `impact` from out of sight probably stands: the
/// impact moved toward the nearest hostile start by the longest artillery
/// reach, or less where that leaves `component`.
fn gun(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    impact: TilePos,
    component: Option<u32>,
) -> Option<TilePos> {
    component?;
    let from = doubled(impact);
    let to = map
        .hostiles(observation.me)
        .filter_map(|owner| map.start(owner))
        .map(|start| footprint_centre(BuildingKind::Foundry, start))
        .min_by_key(|centre| (centre_distance(from, *centre), frame.rank(from, *centre)))?;
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = (dx * dx + dy * dy).isqrt();
    let steps = (2 * artillery().map_or(0, |(reach, _)| reach)).min(length);
    // A doubled coordinate on a tile edge rounds toward the impact, so
    // mirrored impacts give mirrored tiles.
    let tile = |at: i64, toward: i64| -> i32 {
        let whole = if at % 2 != 0 {
            (at - 1) / 2
        } else if toward > 0 {
            at / 2 - 1
        } else {
            at / 2
        };
        i32::try_from(whole).unwrap_or(0)
    };
    (0..=steps).rev().step_by(2).find_map(|step| {
        let (x, y) = if length == 0 {
            from
        } else {
            (from.0 + dx * step / length, from.1 + dy * step / length)
        };
        let goal = TilePos::new(tile(x, dx), tile(y, dy));
        (map.component(goal) == component).then_some(goal)
    })
}

/// The longest reach in tiles among ground units that shell ground from out
/// of sight, and the lowest price among them.
pub(super) fn artillery() -> Option<(i64, u64)> {
    let guns = UnitKind::ALL.iter().filter(|kind| {
        kind.stats().domain == Domain::Ground
            && kind
                .stats()
                .weapons
                .iter()
                .any(|weapon| weapon.indirect && weapon.targets.ground)
    });
    let reach = guns
        .clone()
        .flat_map(|kind| kind.stats().weapons.iter())
        .filter(|weapon| weapon.indirect && weapon.targets.ground)
        .map(|weapon| i64::from(weapon.range.ceil().to_num::<i32>()))
        .max()?;
    let price = guns.map(|kind| u64::from(kind.stats().cost)).min()?;
    Some((reach, price))
}

/// The value a defense against a gun out of sight wants to outweigh: one of
/// the cheapest guns.
fn gun_value() -> u64 {
    artillery().map_or(0, |(_, price)| price)
}

/// Where ground defenders wait out an air raid: beside the seat's building
/// on the Foundry's ground nearest `flyer`, on the tile nearest it. Chasing a
/// flyer's shadow only sends them to tiles they cannot stand on.
fn guard(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    component: Option<u32>,
    flyer: TilePos,
) -> Option<TilePos> {
    component?;
    let building = observation
        .my_buildings
        .iter()
        .filter(|building| map.component(building.anchor) == component)
        .min_by_key(|building| {
            (
                gap(building.anchor, building.kind.size(), flyer, (1, 1)),
                frame.rank(
                    doubled(flyer),
                    footprint_centre(building.kind, building.anchor),
                ),
            )
        })?;
    ring(building.anchor, building.kind.size())
        .filter(|tile| map.component(*tile) == component)
        .min_by_key(|tile| frame.rank(doubled(flyer), doubled(*tile)))
}

/// Empty tiles inside which `enemy` threatens a building: the threat gap, or
/// its longest reach against ground if longer. `None` when it cannot hit
/// ground at all.
fn ground_reach(enemy: &UnitObs) -> Option<i32> {
    let reach = crate::defenses::reach(enemy.kind);
    (reach > Fx::ZERO).then(|| reach.ceil().to_num::<i32>().max(THREAT_GAP))
}

#[cfg(test)]
mod tests;
