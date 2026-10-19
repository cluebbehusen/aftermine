use super::*;
use crate::numeric::Fit;
use chassis::grid::TilePos;
use chassis::rng::Pcg32;
use macroquad::prelude::vec2;
use oxide_sim::scenario::ScenarioMode;
use oxide_sim::{AttackTarget, BuildingId, Command, UnitId};

fn begin(lesson: Lesson) -> Game {
    Tutorial::begin_at(lesson, vec2(1280.0, 800.0)).expect("embedded lesson scenes build")
}

fn tutorial(game: &Game) -> &Tutorial {
    game.tutorial.as_ref().expect("the game stages a lesson")
}

fn tutorial_mut(game: &mut Game) -> &mut Tutorial {
    game.tutorial.as_mut().expect("the game stages a lesson")
}

/// The human's machines, of `kind` when one is named.
fn own(game: &Game, kind: Option<UnitKind>) -> Vec<UnitId> {
    Seat::of(game)
        .units()
        .filter(|u| kind.is_none_or(|kind| u.kind == kind))
        .map(|u| u.id)
        .collect()
}

/// One tick with `commands` staged, then the lesson's bookkeeping. Returns
/// the lesson this tick finished.
fn step(game: &mut Game, commands: impl IntoIterator<Item = Command>) -> Option<Lesson> {
    for command in commands {
        game.issue(command);
    }
    game.do_tick();
    Tutorial::advance_in(game)
}

#[test]
fn the_course_numbers_and_chains_its_lessons_in_order() {
    for (i, lesson) in Lesson::ALL.into_iter().enumerate() {
        assert_eq!(lesson.number(), i + 1);
        assert_eq!(lesson.next(), Lesson::ALL.get(i + 1).copied());
        assert!(!lesson.steps().is_empty(), "{lesson:?} teaches something");
    }
}

#[test]
fn every_lesson_opens_on_a_sandbox_scene_with_each_step_ahead_and_in_reach() {
    for lesson in Lesson::ALL {
        let mut game = begin(lesson);
        assert_eq!(
            game.scenario.mode,
            ScenarioMode::Sandbox,
            "{lesson:?} can never be won or lost"
        );
        assert!(
            game.scenario.players.iter().all(|player| !player.bot),
            "{lesson:?} runs no opponent"
        );
        assert_eq!(step(&mut game, []), None);
        let t = tutorial(&game);
        assert_eq!(t.step_index(), 0, "{lesson:?} opens on its first step");
        for step in lesson.steps() {
            // Welding and wrecks wait on a raid to leave damage behind, and
            // pass on their own when it leaves none.
            assert!(
                matches!(step.goal, Goal::Weld | Goal::CollectWreck) || !step.goal.met(&game, t),
                "{lesson:?}: {} is done before the player acts",
                step.title
            );
            assert!(
                step.goal.reachable(&game, t),
                "{lesson:?}: {} is out of reach from the start",
                step.title
            );
        }
        assert!(
            !matches!(
                t.coach(&game),
                Some(CoachLine::Stuck | CoachLine::Recovery(_))
            ),
            "{lesson:?} opens funded and in reach"
        );
    }
}

#[test]
fn each_squad_names_enemy_machines_of_its_scene_and_each_step_a_real_squad() {
    for lesson in Lesson::ALL {
        let game = begin(lesson);
        let human = game.presentation.human;
        let mut named = Vec::new();
        for squad in lesson.squads() {
            assert_eq!(
                squad.standing(&game.state).count(),
                squad.units.len(),
                "{lesson:?}: a squad names a machine its scene does not place"
            );
            assert!(
                squad
                    .standing(&game.state)
                    .all(|unit| game.state.hostile(human, unit.player)),
                "{lesson:?}: a squad is the enemy's"
            );
            named.extend_from_slice(squad.units);
            if let Some(sortie) = &squad.sortie {
                assert!(sortie.step < lesson.steps().len());
            }
        }
        let count = named.len();
        named.sort_unstable();
        named.dedup();
        assert_eq!(
            named.len(),
            count,
            "{lesson:?}: no machine is in two squads"
        );
        for step in lesson.steps() {
            if let Goal::Defeat(squad) = step.goal {
                assert!(squad < lesson.squads().len(), "{}", step.title);
            }
        }
    }
}

#[test]
fn a_squad_sets_out_once_when_its_step_opens() {
    let sent = |game: &Game| -> Vec<(Vec<UnitId>, TilePos)> {
        game.pending
            .iter()
            .filter_map(|c| match &c.command {
                Command::Hunt { units, goal, .. } if c.player != game.presentation.human => {
                    Some((units.clone(), *goal))
                }
                _ => None,
            })
            .collect()
    };
    let squad = |index: usize| {
        let squad = &Lesson::Fight.squads()[index];
        let units = squad.units.iter().map(|id| UnitId(*id)).collect::<Vec<_>>();
        (units, squad.sortie.as_ref().expect("it sets out").toward)
    };
    let mut game = begin(Lesson::Fight);
    assert!(
        sent(&game).is_empty(),
        "nothing moves before the lesson runs"
    );
    Tutorial::advance_in(&mut game);
    assert_eq!(
        sent(&game),
        [squad(0)],
        "the raid leaves as the lesson opens"
    );
    Tutorial::advance_in(&mut game);
    assert_eq!(sent(&game), [squad(0)], "and only once");
    game.do_tick();
    assert!(
        Lesson::Fight.squads()[0]
            .standing(&game.state)
            .all(|unit| matches!(unit.order, Order::Hunt { .. })),
        "the enemy seat's hunt is an ordinary command the world accepts"
    );

    let opened = tutorial(&game).clone().at_step(3);
    game.tutorial = Some(opened);
    Tutorial::advance_in(&mut game);
    assert_eq!(
        sent(&game),
        [squad(3)],
        "opening the Warden's step sends it, and the guards stay put"
    );
}

#[test]
fn the_buzzards_run_past_the_scout_then_hunt_toward_the_base() {
    let human = PlayerId(0);
    let sent = |game: &Game| -> Vec<Command> {
        game.pending
            .iter()
            .filter(|c| c.player != human)
            .map(|c| c.command.clone())
            .collect()
    };
    let mut game = begin(Lesson::Sustain);
    Tutorial::advance_in(&mut game);
    assert!(
        sent(&game).is_empty(),
        "the Buzzards wait while the player scouts and builds"
    );
    let opened = tutorial(&game).clone().at_step(3);
    game.tutorial = Some(opened);
    Tutorial::advance_in(&mut game);
    let units: Vec<UnitId> = Lesson::Sustain.squads()[0]
        .units
        .iter()
        .map(|id| UnitId(*id))
        .collect();
    assert_eq!(
        sent(&game),
        [
            Command::Run {
                units: units.clone(),
                goal: TilePos::new(17, 12),
                queue: false,
            },
            Command::Hunt {
                units,
                goal: TilePos::new(9, 12),
                queue: true,
            },
        ]
    );
    game.do_tick();
    assert!(
        Lesson::Sustain.squads()[0]
            .standing(&game.state)
            .all(|unit| matches!(unit.order, Order::Run { .. }) && unit.queue.len() == 1),
        "the enemy seat's run and queued hunt are ordinary commands the world accepts"
    );
}

#[test]
fn only_see_and_sustain_opens_with_its_harvesters_collecting() {
    for lesson in Lesson::ALL {
        let mut game = begin(lesson);
        step(&mut game, []);
        let seat = Seat::of(&game);
        let expected = if lesson == Lesson::Sustain {
            seat.workers().count()
        } else {
            0
        };
        assert_eq!(seat.harvesting(), expected, "{lesson:?}");
    }
}

#[test]
fn the_raid_leaves_damage_to_weld_and_wrecks_to_collect() {
    let mut game = begin(Lesson::Sustain);
    let goal = |game: &Game| tutorial(game).current().map(|step| step.goal);
    let play_until = |game: &mut Game, until: Goal| {
        for _ in 0..SOLVE_TICKS {
            if goal(game) == Some(until) {
                return;
            }
            play(game);
            step(game, []);
        }
        panic!("{until:?} never opened");
    };
    play_until(&mut game, Goal::Weld);
    let seat = Seat::of(&game);
    assert!(seat.damaged(), "the Buzzards hit something");
    assert!(seat.wreck_left(), "and left wrecks");
    for _ in 0..20 {
        step(&mut game, []);
    }
    assert_eq!(
        goal(&game),
        Some(Goal::Weld),
        "the step waits for a weld order"
    );
    weld(&mut game);
    step(&mut game, []);
    assert_eq!(goal(&game), Some(Goal::Build(BuildingKind::RepairBay, 1)));

    play_until(&mut game, Goal::CollectWreck);
    assert!(Seat::of(&game).wreck_left());
    for _ in 0..20 {
        step(&mut game, []);
    }
    assert_eq!(goal(&game), Some(Goal::CollectWreck));
    play(&mut game);
    assert_eq!(step(&mut game, []), Some(Lesson::Sustain));
}

#[test]
fn steps_advance_in_order_and_skip_what_is_already_done() {
    let mut game = begin(Lesson::Command);
    game.presentation.selection.units = own(&game, None);
    tutorial_mut(&mut game).toggle_collapsed();
    assert_eq!(Tutorial::advance_in(&mut game), None);
    assert_eq!(
        tutorial(&game).step_index(),
        0,
        "a selection cannot skip looking around"
    );
    assert!(
        tutorial(&game).collapsed(),
        "an unchanged objective keeps the card folded"
    );

    game.presentation.camera.center += vec2(5.0, 0.0);
    assert_eq!(Tutorial::advance_in(&mut game), None);
    let t = tutorial(&game);
    assert_eq!(
        t.current().map(|step| step.goal),
        Some(Goal::Find),
        "looking, then both selection steps, pass in one call"
    );
    assert!(!t.collapsed(), "a new objective opens the card");
}

#[test]
fn zooming_counts_as_looking_where_the_scene_cannot_pan() {
    let mut game = begin(Lesson::Command);
    game.presentation.camera.zoom *= 1.1;
    Tutorial::advance_in(&mut game);
    assert_eq!(tutorial(&game).step_index(), 0, "a nudge is not a look");
    game.presentation.camera.zoom *= 1.2;
    Tutorial::advance_in(&mut game);
    assert_eq!(tutorial(&game).step_index(), 1);
}

#[test]
fn evidence_comes_only_from_the_humans_own_events() {
    let mut t = begin(Lesson::Scrap)
        .tutorial
        .expect("begin stages the lesson");
    let human = PlayerId(0);
    let rival = PlayerId(1);
    let trained = |player| Event::UnitTrained {
        building: BuildingId(1),
        unit: UnitId(9),
        kind: UnitKind::Harvester,
        player,
    };
    let deposited = |player| Event::ScrapDeposited {
        unit: UnitId(9),
        foundry: BuildingId(1),
        player,
        amount: 10,
    };
    t.observe(human, &[trained(rival), deposited(rival)]);
    assert!(!t.evidence.deposited);
    assert!(t.evidence.trained.is_empty());
    t.observe(human, &[trained(human), trained(human), deposited(human)]);
    assert!(t.evidence.deposited);
    assert_eq!(t.evidence.trained, [UnitKind::Harvester]);
}

#[test]
fn the_coach_prices_the_step_and_names_the_way_out_when_broke() {
    let mut game = begin(Lesson::Scrap);
    tutorial_mut(&mut game).step = 1;
    assert_eq!(
        tutorial(&game).current().map(|step| step.goal),
        Some(Goal::Train(UnitKind::Harvester))
    );
    let cost = UnitKind::Harvester.stats().cost;
    let bank = Seat::of(&game).bank();
    assert_eq!(
        tutorial(&game).coach(&game),
        Some(CoachLine::Status(format!(
            "A Harvester costs {cost} scrap. You have {bank}, and no Harvesters are collecting."
        )))
    );

    let mut broke = Lesson::Scrap.scenario();
    broke.players[0].scrap = 0;
    let mut game = Game::with_viewport(broke, vec2(1280.0, 800.0)).unwrap();
    game.tutorial = begin(Lesson::Scrap).tutorial;
    tutorial_mut(&mut game).step = 1;
    assert!(
        matches!(
            tutorial(&game).coach(&game),
            Some(CoachLine::Recovery(line)) if line.contains("idle badge")
        ),
        "an empty bank with nobody hauling points at an idle Harvester"
    );
}

#[test]
fn the_coach_stops_pricing_a_machine_once_it_is_paid_for() {
    let mut exact = Lesson::Scrap.scenario();
    let cost = UnitKind::Harvester.stats().cost;
    exact.players[0].scrap = cost;
    let mut game = Game::with_viewport(exact, vec2(1280.0, 800.0)).unwrap();
    game.tutorial = begin(Lesson::Scrap).tutorial;
    tutorial_mut(&mut game).step = 1;
    assert!(matches!(
        tutorial(&game).coach(&game),
        Some(CoachLine::Status(_))
    ));
    let foundry = game.home_foundry().expect("the lesson's Foundry").id;
    step(
        &mut game,
        [Command::Train {
            building: foundry,
            kind: UnitKind::Harvester,
        }],
    );
    assert_eq!(Seat::of(&game).bank(), 0);
    assert_eq!(
        tutorial(&game).current().map(|step| step.goal),
        Some(Goal::Train(UnitKind::Harvester)),
        "the step waits for the Harvester to roll out"
    );
    assert_eq!(
        tutorial(&game).coach(&game),
        None,
        "a paid machine is no reason to say the bank is short"
    );
}

#[test]
fn a_step_the_world_can_no_longer_complete_raises_the_retry() {
    // Without Harvesters or scrap, the Foundry's emergency package still
    // pays for a new Harvester. Once it is spent, nothing can.
    let mut scene = Lesson::Scrap.scenario();
    scene.players[0].scrap = 0;
    scene.units.retain(|unit| unit.kind != UnitKind::Harvester);
    let mut game = Game::with_viewport(scene, vec2(1280.0, 800.0)).unwrap();
    game.tutorial = begin(Lesson::Scrap).tutorial;
    tutorial_mut(&mut game).step = 1;
    assert_ne!(tutorial(&game).coach(&game), Some(CoachLine::Stuck));
    spend_recovery(&mut game);
    assert_eq!(tutorial(&game).coach(&game), Some(CoachLine::Stuck));

    // A Harvester already paid for will still roll out.
    let mut scene = Lesson::Scrap.scenario();
    scene.players[0].scrap = UnitKind::Harvester.stats().cost;
    scene.units.retain(|unit| unit.kind != UnitKind::Harvester);
    let mut game = Game::with_viewport(scene, vec2(1280.0, 800.0)).unwrap();
    game.tutorial = begin(Lesson::Scrap).tutorial;
    tutorial_mut(&mut game).step = 1;
    let foundry = game.home_foundry().expect("the lesson's Foundry").id;
    step(
        &mut game,
        [Command::Train {
            building: foundry,
            kind: UnitKind::Harvester,
        }],
    );
    assert_eq!(Seat::of(&game).bank(), 0);
    assert_ne!(tutorial(&game).coach(&game), Some(CoachLine::Stuck));

    // A Lancer needs a Fabricator, and nobody is left to raise one.
    let mut scene = Lesson::Build.scenario();
    scene.units.clear();
    let mut game = Game::with_viewport(scene, vec2(1280.0, 800.0)).unwrap();
    game.tutorial = begin(Lesson::Build).tutorial;
    tutorial_mut(&mut game).step = 3;
    assert_eq!(
        tutorial(&game).current().map(|step| step.goal),
        Some(Goal::Train(UnitKind::Lancer))
    );
    assert_eq!(tutorial(&game).coach(&game), Some(CoachLine::Stuck));

    // An army that is gone can be trained again while scrap allows. A
    // scene's ids are its unit list's order, so lost machines change hands
    // rather than leave the list and renumber the squads.
    let mut scene = Lesson::Fight.scenario();
    scene.players[0].scrap = UnitKind::Scuttler.stats().cost;
    hand_over(&mut scene, |kind| kind != UnitKind::Harvester);
    let mut game = Game::with_viewport(scene.clone(), vec2(1280.0, 800.0)).unwrap();
    game.tutorial = begin(Lesson::Fight).tutorial;
    assert_ne!(tutorial(&game).coach(&game), Some(CoachLine::Stuck));
    scene.players[0].scrap = 0;
    hand_over(&mut scene, |_| true);
    let mut game = Game::with_viewport(scene, vec2(1280.0, 800.0)).unwrap();
    game.tutorial = begin(Lesson::Fight).tutorial;
    spend_recovery(&mut game);
    for at in 0..Lesson::Fight.steps().len() {
        tutorial_mut(&mut game).step = at;
        assert_eq!(
            tutorial(&game).coach(&game),
            Some(CoachLine::Stuck),
            "{}",
            Lesson::Fight.steps()[at].title
        );
    }

    // Ground guns cannot reach aircraft, and a Flak Turret can.
    let mut scene = Lesson::Sustain.scenario();
    scene.players[0].scrap = 0;
    hand_over(&mut scene, |_| true);
    scene.units.push(oxide_sim::scenario::UnitSpec {
        player: 0,
        kind: UnitKind::Lancer,
        x: 10,
        y: 12,
    });
    let mut game = Game::with_viewport(scene.clone(), vec2(1280.0, 800.0)).unwrap();
    game.tutorial = begin(Lesson::Sustain).tutorial;
    tutorial_mut(&mut game).step = 3;
    assert_eq!(
        tutorial(&game).current().map(|step| step.goal),
        Some(Goal::Defeat(0))
    );
    assert_ne!(
        tutorial(&game).coach(&game),
        Some(CoachLine::Stuck),
        "the emergency package can pay for a Sentinel"
    );
    let mut game = Game::with_viewport(scene.clone(), vec2(1280.0, 800.0)).unwrap();
    game.tutorial = begin(Lesson::Sustain).tutorial;
    spend_recovery(&mut game);
    tutorial_mut(&mut game).step = 3;
    assert_eq!(tutorial(&game).coach(&game), Some(CoachLine::Stuck));
    scene.buildings.push(oxide_sim::scenario::BuildingSpec {
        player: 0,
        kind: BuildingKind::FlakTurret,
        x: 9,
        y: 12,
    });
    let mut game = Game::with_viewport(scene, vec2(1280.0, 800.0)).unwrap();
    game.tutorial = begin(Lesson::Sustain).tutorial;
    tutorial_mut(&mut game).step = 3;
    assert_ne!(tutorial(&game).coach(&game), Some(CoachLine::Stuck));

    // A squad that has lost its fighters cannot destroy anything.
    let mut scene = Lesson::Command.scenario();
    scene.units.clear();
    let mut game = Game::with_viewport(scene, vec2(1280.0, 800.0)).unwrap();
    game.tutorial = begin(Lesson::Command).tutorial;
    for at in 1..Lesson::Command.steps().len() {
        tutorial_mut(&mut game).step = at;
        assert_eq!(
            tutorial(&game).coach(&game),
            Some(CoachLine::Stuck),
            "{}",
            Lesson::Command.steps()[at].title
        );
    }
    tutorial_mut(&mut game).step = 0;
    assert_ne!(
        tutorial(&game).coach(&game),
        Some(CoachLine::Stuck),
        "the camera always works"
    );
}

#[test]
fn every_lesson_string_is_ascii_and_names_only_what_the_hands_have() {
    // The bundled font cannot be trusted to cover typographic punctuation.
    let ascii = |line: &str| assert!(line.is_ascii(), "non-ASCII lesson text: {line}");
    ascii(CoachLine::Stuck.text());
    for hands in crate::platform::ALL_HANDS {
        let recovery = recovery_line(hands.touch());
        ascii(recovery);
        crate::platform::assert_copy_fits(hands, recovery);
    }
    for lesson in Lesson::ALL {
        for line in [lesson.title(), lesson.summary(), lesson.outro()] {
            ascii(line);
        }
        for step in lesson.steps() {
            ascii(step.title);
            for hands in crate::platform::ALL_HANDS {
                for line in step.body(hands) {
                    ascii(line);
                    crate::platform::assert_copy_fits(hands, line);
                }
            }
        }
    }
}

#[test]
fn the_reference_play_finishes_each_lesson_once() {
    for lesson in Lesson::ALL {
        let mut game = begin(lesson);
        let mut finishes = Vec::new();
        for _ in 0..SOLVE_TICKS {
            if tutorial(&game).finished() {
                break;
            }
            play(&mut game);
            finishes.extend(step(&mut game, []));
        }
        assert!(
            tutorial(&game).finished(),
            "{lesson:?} stalled on {:?}",
            tutorial(&game).current().map(|step| step.title)
        );
        for _ in 0..20 {
            finishes.extend(step(&mut game, []));
        }
        assert_eq!(finishes, [lesson], "the finish is reported exactly once");
    }
}

#[test]
fn whatever_the_player_does_a_lesson_finishes_or_offers_a_retry() {
    // Stray orders first, then the reference play: a lesson the coach
    // still calls finishable must finish, so no meddling can leave the
    // player on a silent dead end.
    for lesson in Lesson::ALL {
        for seed in 0..16 {
            let mut game = begin(lesson);
            let mut rng = Pcg32::new(seed, u64::from(lesson.number().fit::<u32>()));
            for tick in 0..MEDDLE_TICKS {
                let commands = (tick % 8 == 0).then(|| meddle(&mut game, &mut rng));
                step(&mut game, commands.into_iter().flatten());
            }
            for _ in 0..SOLVE_TICKS {
                if tutorial(&game).finished()
                    || tutorial(&game).coach(&game) == Some(CoachLine::Stuck)
                {
                    break;
                }
                play(&mut game);
                step(&mut game, []);
            }
            let t = tutorial(&game);
            assert!(
                t.finished() || t.coach(&game) == Some(CoachLine::Stuck),
                "{lesson:?} seed {seed}: stalled on {:?} without offering a retry",
                t.current().map(|step| step.title)
            );
        }
    }
}

/// Gives the human's machines of the kinds `lost` picks to the enemy seat,
/// which keeps every unit's place in the scene.
fn hand_over(scene: &mut Scenario, lost: impl Fn(UnitKind) -> bool) {
    for unit in scene
        .units
        .iter_mut()
        .filter(|u| u.player == 0 && lost(u.kind))
    {
        unit.player = 1;
    }
}

/// Lets a stranded seat's Foundry pay out its whole emergency package,
/// spending it on Surveyors as it arrives. They neither fight nor collect,
/// so only the slow income floor is left.
fn spend_recovery(game: &mut Game) {
    let foundry = game.home_foundry().expect("the lesson's Foundry").id;
    let surveyor = UnitKind::Surveyor.stats().cost;
    for _ in 0..SOLVE_TICKS {
        if matches!(
            game.state.player(PlayerId(0)).recovery,
            Recovery::Active { allowance: 0, .. }
        ) {
            break;
        }
        let order = (Seat::of(game).bank() >= surveyor).then_some(Command::Train {
            building: foundry,
            kind: UnitKind::Surveyor,
        });
        step(game, order);
    }
    assert!(!Seat::of(game).recovering(), "the package is spent");
    assert!(Seat::of(game).bank() < surveyor);
}

/// Ticks the reference play may take to finish what is left of a lesson.
const SOLVE_TICKS: u32 = 20 * 60 * 8;

/// Ticks of stray orders before the reference play takes over.
const MEDDLE_TICKS: u32 = 20 * 60;

/// Whether a worker is doing none of the economy's work, so the plain
/// play may give it some.
fn spare(worker: &oxide_sim::Unit) -> bool {
    !matches!(
        worker.order,
        Order::Harvest { .. }
            | Order::Build { .. }
            | Order::Found { .. }
            | Order::ReturnCargo { .. }
    )
}

/// Whether every tile of `building` is in the human's sight.
fn seen(game: &Game, building: &oxide_sim::Building) -> bool {
    building
        .tiles()
        .all(|tile| game.state.can_see(game.presentation.human, tile))
}

/// The explored salvage tile nearest `from`.
fn nearest_scrap(game: &Game, from: TilePos) -> Option<TilePos> {
    let map = game.state.map();
    let vision = game.state.vision(game.presentation.human);
    (0..map.height())
        .flat_map(|y| (0..map.width()).map(move |x| TilePos::new(x, y)))
        .filter(|tile| {
            (map.scrap_at(*tile) > 0 || map.wreck_at(*tile) > 0) && vision.explored(*tile)
        })
        .min_by_key(|tile| (tile.x - from.x).pow(2) + (tile.y - from.y).pow(2))
}

/// Sends every idle worker to its nearest known scrap.
fn put_idle_workers_to_work(game: &mut Game) {
    let orders: Vec<Command> = Seat::of(game)
        .workers()
        .filter(|u| spare(u))
        .filter_map(|u| {
            nearest_scrap(game, TilePos::containing(u.pos)).map(|node| Command::Harvest {
                units: vec![u.id],
                node,
                queue: false,
            })
        })
        .collect();
    for order in orders {
        game.issue(order);
    }
}

/// Queues `kind` at an idle own factory once the bank covers it.
fn train(game: &mut Game, kind: UnitKind) {
    let seat = Seat::of(game);
    let factory = seat
        .state
        .buildings()
        .iter()
        .find(|b| {
            b.player == seat.human
                && b.built()
                && b.queue.is_empty()
                && b.stats().produces.contains(&kind)
        })
        .map(|b| b.id);
    if let Some(building) = factory
        && seat.bank() >= kind.stats().cost
    {
        game.issue(Command::Train { building, kind });
    }
}

/// Raises `kind` until `count` stand: crews unfinished sites, and claims
/// free ground (a free frame, for an Extractor) once the bank covers it.
fn build(game: &mut Game, kind: BuildingKind, count: usize) {
    let seat = Seat::of(game);
    let cost = kind.base_stats().construction.map_or(0, |c| c.cost);
    let placed: Vec<&oxide_sim::Building> = seat.own_buildings(kind).collect();
    let idle_worker = seat.workers().find(|u| spare(u)).map(|u| u.id);
    let site = placed.iter().find(|b| !b.built()).map(|b| b.anchor);
    let order = if let Some(anchor) = site {
        idle_worker.map(|worker| (worker, anchor))
    } else if placed.len() < count && seat.bank() >= cost && kind != BuildingKind::Extractor {
        let worker = idle_worker.or_else(|| seat.workers().next().map(|u| u.id));
        worker.zip(open_ground(game, kind))
    } else if placed.len() < count && seat.bank() >= cost && kind == BuildingKind::Extractor {
        let frame = seat
            .state
            .map()
            .extractor_frames()
            .iter()
            .copied()
            .find(|frame| {
                !seat
                    .state
                    .buildings()
                    .iter()
                    .any(|b| b.kind == BuildingKind::Extractor && b.anchor == *frame)
            });
        let worker = idle_worker.or_else(|| seat.workers().next().map(|u| u.id));
        worker.zip(frame)
    } else {
        None
    };
    let Some((worker, anchor)) = order else {
        return;
    };
    let (w, h) = kind.size();
    let defer = !(0..h).all(|dy| {
        (0..w).all(|dx| {
            game.state
                .can_see(game.presentation.human, anchor.offset(dx, dy))
        })
    });
    game.issue(Command::Build {
        units: vec![worker],
        kind,
        anchor,
        queue: false,
        defer,
    });
}

/// The spot nearest the home Foundry where `kind` can go up now, or for
/// a defense, nearest where the lesson's raid is headed.
fn open_ground(game: &Game, kind: BuildingKind) -> Option<TilePos> {
    let raid = tutorial(game)
        .lesson
        .squads()
        .iter()
        .find_map(|squad| squad.sortie.as_ref().map(|sortie| sortie.toward))
        .filter(|_| !kind.base_stats().weapons.is_empty());
    let home = match raid {
        Some(tile) => tile,
        None => TilePos::containing(game.home_foundry()?.center()),
    };
    let map = game.state.map();
    (0..map.height())
        .flat_map(|y| (0..map.width()).map(move |x| TilePos::new(x, y)))
        .filter(|tile| game.state.can_place(game.presentation.human, kind, *tile))
        .min_by_key(|tile| {
            (
                (tile.x - home.x).pow(2) + (tile.y - home.y).pow(2),
                tile.y,
                tile.x,
            )
        })
}

/// Orders the current step the plain way, with commands any player
/// could give: the reference that a step the coach calls finishable can
/// be finished.
fn play(game: &mut Game) {
    let Some(goal) = tutorial(game).current().map(|step| step.goal) else {
        return;
    };
    match goal {
        Goal::Look(_) => game.presentation.camera.zoom *= 2.0,
        Goal::Select(_) => game.presentation.selection.units = own(game, None),
        Goal::Find if Seat::of(game).units().any(|u| u.kind.stats().scouts) => {
            let seat = Seat::of(game);
            let goal = seat.hostile_buildings().next().map(|b| b.anchor);
            let scouts: Vec<UnitId> = seat
                .units()
                .filter(|u| {
                    u.kind.stats().scouts
                        && !matches!(u.order, Order::Scout { tile } if Some(tile) == goal)
                })
                .map(|u| u.id)
                .collect();
            if let Some(goal) = goal.filter(|_| !scouts.is_empty()) {
                game.issue(Command::Scout {
                    units: scouts,
                    goal,
                    queue: false,
                });
            }
        }
        Goal::Find | Goal::Destroy => {
            // The squad attacks a building in sight and walks toward one
            // otherwise; any fighter doing something else is ordered again.
            // A squad that can still grow gathers first.
            reinforce(game);
            let seat = Seat::of(game);
            let fighters: Vec<&oxide_sim::Unit> = seat
                .units()
                .filter(|u| u.kind.stats().can_fight())
                .collect();
            if fighters.len() < 4 && growing(&seat) {
                return;
            }
            let units: Vec<UnitId> = fighters.iter().map(|u| u.id).collect();
            // Armed buildings first, since they shoot back.
            let target = seat
                .hostile_buildings()
                .filter(|b| seen(game, b))
                .min_by_key(|b| b.stats().weapons.is_empty())
                .map(|b| b.id);
            let goal = seat.hostile_buildings().next().map(|b| b.anchor);
            let (order, on_task): (Command, fn(&Order) -> bool) = match (target, goal) {
                (Some(building), _) => (
                    Command::Attack {
                        units,
                        target: AttackTarget::Building(building),
                        queue: false,
                    },
                    |order| matches!(order, Order::Attack { .. }),
                ),
                (None, Some(goal)) => (
                    Command::Run {
                        units,
                        goal,
                        queue: false,
                    },
                    |order| matches!(order, Order::Run { .. }),
                ),
                (None, None) => return,
            };
            if !fighters.iter().all(|u| on_task(&u.order)) {
                game.issue(order);
            }
        }
        Goal::Defeat(squad) => {
            defeat(game, squad);
            reinforce(game);
        }
        Goal::Deposit => put_idle_workers_to_work(game),
        Goal::Train(kind) => {
            let seat = Seat::of(game);
            match seat.factory_for(kind).filter(|_| !seat.produces(kind)) {
                Some(factory) => build(game, factory, 1),
                None => train(game, kind),
            }
            put_idle_workers_to_work(game);
        }
        Goal::Harvest(count) => {
            if Seat::of(game).workers().count() < count {
                train(game, UnitKind::Harvester);
            }
            put_idle_workers_to_work(game);
        }
        Goal::Build(kind, count) => {
            build(game, kind, count);
            put_idle_workers_to_work(game);
        }
        Goal::Rally => {
            let factory = Seat::of(game)
                .factories()
                .next()
                .map(|b| (b.id, TilePos::containing(b.center()).offset(0, 4)));
            if let Some((building, tile)) = factory {
                game.issue(Command::SetRally {
                    building,
                    rally: Some(oxide_sim::Rally::Tile(tile)),
                });
            }
        }
        // Saving a group is input's work, which reports it as this does.
        Goal::Group => tutorial_mut(game).note_group(),
        // The weld or wreck order comes last, so it is the one that stands.
        Goal::Weld => {
            reinforce(game);
            weld(game);
        }
        Goal::CollectWreck => {
            reinforce(game);
            let seat = Seat::of(game);
            let worker = seat.workers().find(|u| spare(u));
            let order = worker.or_else(|| seat.workers().next()).and_then(|u| {
                nearest_wreck(game, TilePos::containing(u.pos)).map(|node| Command::Harvest {
                    units: vec![u.id],
                    node,
                    queue: false,
                })
            });
            if let Some(order) = order {
                game.issue(order);
            }
        }
    }
}

/// Orders a Harvester onto the first damaged own building, or else the
/// first wounded own ground machine.
fn weld(game: &mut Game) {
    let seat = Seat::of(game);
    let Some(worker) = seat.workers().next() else {
        return;
    };
    let building = seat
        .state
        .buildings()
        .iter()
        .find(|b| b.player == seat.human && b.built() && b.hp < b.stats().max_hp)
        .map(|b| b.id);
    let machine = seat
        .units()
        .find(|u| {
            u.id != worker.id
                && u.hp < u.kind.stats().max_hp
                && u.domain() == oxide_sim::stats::Domain::Ground
        })
        .map(|u| u.id);
    let units = vec![worker.id];
    let order = match (building, machine) {
        (Some(building), _) => Some(Command::Repair {
            units,
            building,
            queue: false,
        }),
        (None, Some(target)) => Some(Command::RepairUnit {
            units,
            target,
            queue: false,
        }),
        (None, None) => None,
    };
    if let Some(order) = order {
        game.issue(order);
    }
}

/// The explored wreck nearest `from`.
fn nearest_wreck(game: &Game, from: TilePos) -> Option<TilePos> {
    let map = game.state.map();
    let vision = game.state.vision(game.presentation.human);
    tiles(map)
        .filter(|tile| map.wreck_at(*tile) > 0 && vision.explored(*tile))
        .min_by_key(|tile| (tile.x - from.x).pow(2) + (tile.y - from.y).pow(2))
}

/// Rebuilds a crew of three Harvesters, then trains Sentinels.
fn reinforce(game: &mut Game) {
    if Seat::of(game).workers().count() < 3 {
        train(game, UnitKind::Harvester);
    } else {
        train(game, UnitKind::Sentinel);
    }
    put_idle_workers_to_work(game);
}

/// Whether the player can still add a Sentinel to the army.
fn growing(seat: &Seat) -> bool {
    seat.produces(UnitKind::Sentinel)
        && (seat.bank() >= UnitKind::Sentinel.stats().cost || seat.can_earn())
}

/// Orders the army against the lesson's squad at `index`. A squad that
/// marches on the base is met at home, beside the defenses, as the lesson
/// says. One that stands guard is hunted once the army has gathered,
/// since reinforcements sent one at a time only feed it.
fn defeat(game: &mut Game, index: usize) {
    let squad = &tutorial(game).lesson.squads()[index];
    let seat = Seat::of(game);
    let Some(target) = squad
        .standing(seat.state)
        .next()
        .map(|unit| TilePos::containing(unit.pos))
    else {
        return;
    };
    let near = |a: TilePos, x: i32, y: i32| (x - a.x).pow(2) + (y - a.y).pow(2) <= 36;
    let fighters: Vec<&oxide_sim::Unit> = seat
        .units()
        .filter(|u| u.kind.stats().can_fight())
        .collect();
    let home = squad.sortie.as_ref().map(|sortie| sortie.toward);
    let command = match home {
        Some(home) if !near(home, target.x, target.y) => {
            let units: Vec<UnitId> = fighters
                .iter()
                .filter(|u| {
                    let at = TilePos::containing(u.pos);
                    !near(home, at.x, at.y) && !matches!(u.order, Order::Run { .. })
                })
                .map(|u| u.id)
                .collect();
            Command::Run {
                units,
                goal: home,
                queue: false,
            }
        }
        _ => {
            let gathered = fighters.len() >= squad.standing(seat.state).count() + 3;
            if home.is_none() && !gathered && growing(&seat) {
                return;
            }
            let units: Vec<UnitId> = fighters
                .iter()
                .filter(|u| match u.order {
                    Order::Attack { .. } => false,
                    Order::Hunt { goal } => !near(target, goal.x, goal.y),
                    _ => true,
                })
                .map(|u| u.id)
                .collect();
            Command::Hunt {
                units,
                goal: target,
                queue: false,
            }
        }
    };
    let (Command::Run { units, .. } | Command::Hunt { units, .. }) = &command else {
        unreachable!("only moves are built above");
    };
    if !units.is_empty() {
        game.issue(command);
    }
}

/// A stray order or two of the kinds a confused player gives: moves,
/// halts, hunts, harvests, purchases, placements, salvage, and
/// cancellations at random, plus a shuffled selection and camera.
fn meddle(game: &mut Game, rng: &mut Pcg32) -> Vec<Command> {
    let seat = Seat::of(game);
    let units: Vec<UnitId> = seat.units().map(|u| u.id).collect();
    let buildings: Vec<BuildingId> = seat
        .state
        .buildings()
        .iter()
        .filter(|b| b.player == seat.human)
        .map(|b| b.id)
        .collect();
    let map = seat.state.map();
    let tile = |rng: &mut Pcg32| {
        let below = |rng: &mut Pcg32, bound: i32| {
            i32::try_from(rng.next_below(u32::try_from(bound).unwrap())).unwrap()
        };
        TilePos::new(below(rng, map.width()), below(rng, map.height()))
    };
    let some_unit = |rng: &mut Pcg32| vec![units[rng.next_index(units.len())]];
    let some_building = |rng: &mut Pcg32| buildings[rng.next_index(buildings.len())];
    let mut commands = Vec::new();
    if !units.is_empty() {
        let goal = tile(rng);
        commands.push(match rng.next_below(6) {
            0 => Command::Run {
                units: some_unit(rng),
                goal,
                queue: false,
            },
            1 => Command::Stop {
                units: units.clone(),
            },
            2 => Command::Hunt {
                units: some_unit(rng),
                goal,
                queue: false,
            },
            3 => Command::Harvest {
                units: some_unit(rng),
                node: nearest_scrap(game, goal).unwrap_or(goal),
                queue: false,
            },
            4 => {
                let kinds: Vec<BuildingKind> = BuildingKind::ALL
                    .into_iter()
                    .filter(|kind| kind.base_stats().construction.is_some())
                    .collect();
                Command::Build {
                    units: some_unit(rng),
                    kind: kinds[rng.next_index(kinds.len())],
                    anchor: goal,
                    queue: false,
                    defer: false,
                }
            }
            _ => Command::Patrol {
                units: some_unit(rng),
                waypoints: vec![goal, tile(rng)],
            },
        });
    }
    if !buildings.is_empty() {
        let building = some_building(rng);
        match rng.next_below(4) {
            0 => commands.push(Command::Train {
                building,
                kind: UnitKind::ALL[rng.next_index(UnitKind::ALL.len())],
            }),
            1 if !units.is_empty() => commands.push(Command::Salvage {
                units: some_unit(rng),
                building,
                queue: false,
            }),
            2 => commands.push(Command::Cancel { building }),
            _ => commands.push(Command::CancelTrain { building, index: 0 }),
        }
    }
    let selection: Vec<UnitId> = units
        .iter()
        .copied()
        .filter(|_| rng.next_below(2) == 0)
        .collect();
    let pan = vec2(
        rng.next_below(9) as f32 - 4.0,
        rng.next_below(9) as f32 - 4.0,
    );
    game.presentation.selection.units = selection;
    game.presentation.camera.pan(pan);
    commands
}
