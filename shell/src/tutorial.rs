//! Basic Training: short staged lessons, each a small sandbox scene that
//! holds only what it teaches. A lesson's steps advance when the world
//! shows the step done, however the player got there. A step the world
//! can no longer complete raises a retry instead of a dead end, and
//! lessons never save: one is quick to start over.

use crate::game::Game;
use anyhow::Result;
use macroquad::prelude::Vec2;
use oxide_sim::{BuildingKind, Event, Order, PlayerId, Scenario, UnitKind};

mod lessons;

/// One lesson of Basic Training, in teaching order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lesson {
    /// Camera, selection, movement, and attack.
    Command,
    /// Mining, training workers, and Extractors.
    Scrap,
}

impl Lesson {
    /// Every lesson in teaching order.
    pub const ALL: [Self; 2] = [Self::Command, Self::Scrap];

    /// The lesson's place in the course, from one.
    pub fn number(self) -> usize {
        Self::ALL
            .iter()
            .position(|lesson| *lesson == self)
            .expect("every lesson is listed")
            + 1
    }

    /// The lesson's name.
    pub fn title(self) -> &'static str {
        match self {
            Self::Command => "Command",
            Self::Scrap => "Scrap",
        }
    }

    /// One line on what the lesson teaches.
    pub fn summary(self) -> &'static str {
        match self {
            Self::Command => "move the camera, select units, move them, and attack",
            Self::Scrap => "collect scrap, train Harvesters, and restore Extractors",
        }
    }

    /// The lesson after this one.
    pub fn next(self) -> Option<Self> {
        Self::ALL.get(self.number()).copied()
    }

    /// The demonstrations, in order.
    pub fn steps(self) -> &'static [Step] {
        match self {
            Self::Command => &lessons::COMMAND,
            Self::Scrap => &lessons::SCRAP,
        }
    }

    /// What the finished lesson leaves the player with.
    pub fn outro(self) -> &'static str {
        match self {
            Self::Command => lessons::COMMAND_OUTRO,
            Self::Scrap => lessons::SCRAP_OUTRO,
        }
    }

    /// The staged scene.
    pub fn scenario(self) -> Scenario {
        let text = match self {
            Self::Command => include_str!("../lessons/command.json"),
            Self::Scrap => include_str!("../lessons/scrap.json"),
        };
        Scenario::from_json(text).expect("embedded lesson scenes are validated by tests")
    }
}

/// One step of a lesson: what to do, how to do it with the hands in use,
/// and what in the world shows it done.
pub struct Step {
    /// The objective, as the card heads it.
    pub title: &'static str,
    copy: Copy,
    /// What completes the step.
    pub goal: Goal,
}

/// A step's paragraphs for each pairing of pointer and keyboard: copy
/// names keys only once one is in use, and a finger has no right button,
/// wheel, or Shift.
struct Copy {
    mouse_keys: &'static [&'static str],
    mouse: &'static [&'static str],
    touch_keys: &'static [&'static str],
    touch: &'static [&'static str],
}

impl Step {
    /// The paragraphs the player can follow with the hands in use.
    pub fn body(&self, hands: crate::platform::Hands) -> &'static [&'static str] {
        match (hands.touch(), hands.keys) {
            (false, true) => self.copy.mouse_keys,
            (false, false) => self.copy.mouse,
            (true, true) => self.copy.touch_keys,
            (true, false) => self.copy.touch,
        }
    }
}

/// What the world must show for a step to be done.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Goal {
    /// The camera has panned this many tiles from where the lesson opened,
    /// or zoomed noticeably: a window larger than the scene cannot pan.
    Look(f32),
    /// At least this many own machines are selected.
    Select(usize),
    /// An own machine sees a hostile building.
    Find,
    /// No hostile building stands.
    Destroy,
    /// A harvest load has reached the bank.
    Deposit,
    /// A factory has finished a machine of this kind.
    Train(UnitKind),
    /// At least this many own workers are on harvest programs.
    Harvest(usize),
    /// At least this many own buildings of this kind are complete.
    Build(BuildingKind, usize),
}

/// How far a zoom must move from the opening one to count as looking.
const LOOK_ZOOM_RATIO: f32 = 1.25;

impl Goal {
    fn met(self, game: &Game, tutorial: &Tutorial) -> bool {
        let seat = Seat::of(game);
        match self {
            Goal::Look(tiles) => {
                let camera = &game.presentation.camera;
                let ratio = camera.zoom / tutorial.origin.zoom;
                camera.center.distance(tutorial.origin.center) >= tiles
                    || !(1.0 / LOOK_ZOOM_RATIO..=LOOK_ZOOM_RATIO).contains(&ratio)
            }
            Goal::Select(count) => {
                game.presentation
                    .selection
                    .units
                    .iter()
                    .filter(|id| {
                        seat.state
                            .unit(**id)
                            .is_some_and(|u| u.player == seat.human)
                    })
                    .count()
                    >= count
            }
            Goal::Find => {
                let mut hostile = seat.hostile_buildings().peekable();
                hostile.peek().is_none()
                    || hostile.any(|b| b.tiles().any(|tile| seat.state.can_see(seat.human, tile)))
            }
            Goal::Destroy => seat.hostile_buildings().next().is_none(),
            Goal::Deposit => tutorial.evidence.deposited,
            Goal::Train(kind) => tutorial.evidence.trained.contains(&kind),
            Goal::Harvest(count) => seat.harvesting() >= count,
            Goal::Build(kind, count) => {
                seat.own_buildings(kind).filter(|b| b.built()).count() >= count
            }
        }
    }

    /// What the step's literal instruction still has to buy, by its
    /// lowercase name, and its price in scrap.
    fn spend(self, game: &Game) -> Option<(&'static str, u32)> {
        let seat = Seat::of(game);
        let harvester = UnitKind::Harvester;
        match self {
            Goal::Train(kind) => Some((kind.name(), kind.stats().cost)),
            Goal::Harvest(count) => {
                (seat.workers().count() < count).then(|| (harvester.name(), harvester.stats().cost))
            }
            Goal::Build(kind, count) => (seat.own_buildings(kind).count() < count).then(|| {
                let cost = kind.base_stats().construction.map_or(0, |c| c.cost);
                (kind.name(), cost)
            }),
            Goal::Look(_) | Goal::Select(_) | Goal::Find | Goal::Destroy | Goal::Deposit => None,
        }
    }

    /// Whether the world still holds what the step needs: machines to
    /// order, a factory and the scrap or income to pay for what it asks,
    /// scrap to mine, and ground to build on.
    fn reachable(self, game: &Game) -> bool {
        let seat = Seat::of(game);
        let affords = |cost: u32| seat.bank() >= cost || seat.can_earn();
        match self {
            Goal::Look(_) => true,
            Goal::Select(count) => seat.units().count() >= count,
            Goal::Find => seat.units().next().is_some(),
            Goal::Destroy => seat.units().any(|u| u.kind.stats().can_fight()),
            Goal::Deposit => seat.can_mine(),
            Goal::Train(kind) => {
                seat.produces(kind) && (seat.queued(kind) || affords(kind.stats().cost))
            }
            Goal::Harvest(count) => {
                let harvester = UnitKind::Harvester;
                seat.scrap_left()
                    && (seat.workers().count() >= count
                        || (seat.produces(harvester)
                            && (seat.queued(harvester) || affords(harvester.stats().cost))))
            }
            Goal::Build(kind, count) => {
                let placed = seat.own_buildings(kind).count();
                let cost = kind.base_stats().construction.map_or(0, |c| c.cost);
                seat.workers().next().is_some()
                    && (placed >= count || (affords(cost) && seat.room_for(kind) >= count - placed))
            }
        }
    }
}

/// What the player has shown so far that the world does not keep.
#[derive(Debug, Default, Clone)]
struct Evidence {
    /// A harvest load has reached the bank.
    deposited: bool,
    /// The kinds own factories have finished.
    trained: Vec<UnitKind>,
}

/// Where the camera stood when the lesson opened.
#[derive(Debug, Clone, Copy)]
struct Origin {
    center: Vec2,
    zoom: f32,
}

/// One line of live coaching under a step.
#[derive(Debug, Clone, PartialEq)]
pub enum CoachLine {
    /// What the step costs against the live bank and Harvesters mining.
    Status(String),
    /// The step is unaffordable and nothing is earning, so the coach
    /// points at an idle Harvester.
    Recovery(&'static str),
    /// The world can no longer complete the step; the card offers a retry.
    Stuck,
}

impl CoachLine {
    /// The line as drawn.
    pub fn text(&self) -> &str {
        match self {
            CoachLine::Status(text) => text,
            CoachLine::Recovery(text) => text,
            CoachLine::Stuck => {
                "This lesson can no longer be finished. Use RETRY to start it over."
            }
        }
    }
}

/// A running lesson.
#[derive(Debug, Clone)]
pub struct Tutorial {
    lesson: Lesson,
    /// Index into the lesson's steps; their count once it is finished.
    step: usize,
    origin: Origin,
    evidence: Evidence,
    /// Whether the card shows only its heading and objective.
    collapsed: bool,
}

impl Tutorial {
    /// Starts `lesson` in a fresh session.
    pub fn begin(lesson: Lesson) -> Result<Game> {
        Self::begin_at(lesson, crate::render::viewport())
    }

    /// [`Self::begin`] at an explicit viewport.
    pub fn begin_at(lesson: Lesson, viewport: Vec2) -> Result<Game> {
        let mut game = Game::with_viewport(lesson.scenario(), viewport)?;
        let human = game.presentation.human;
        // A scene without an own building opens on the player's machines.
        if !game.state.buildings().iter().any(|b| b.player == human) {
            let own: Vec<Vec2> = game
                .state
                .units()
                .iter()
                .filter(|u| u.player == human)
                .map(|u| crate::game::world_vec(u.pos))
                .collect();
            if !own.is_empty() {
                let camera = &mut game.presentation.camera;
                camera.center = own.iter().copied().sum::<Vec2>() / own.len() as f32;
                camera.pan(Vec2::ZERO);
            }
        }
        let camera = &game.presentation.camera;
        game.tutorial = Some(Self {
            lesson,
            step: 0,
            origin: Origin {
                center: camera.center,
                zoom: camera.zoom,
            },
            evidence: Evidence::default(),
            collapsed: false,
        });
        Ok(game)
    }

    /// The lesson running.
    pub fn lesson(&self) -> Lesson {
        self.lesson
    }

    /// The current step's index.
    pub fn step_index(&self) -> usize {
        self.step
    }

    /// The current step, until the lesson is finished.
    pub fn current(&self) -> Option<&'static Step> {
        self.lesson.steps().get(self.step)
    }

    /// Whether every step is done.
    pub fn finished(&self) -> bool {
        self.current().is_none()
    }

    /// Whether the card shows only its heading and objective.
    pub fn collapsed(&self) -> bool {
        self.collapsed
    }

    /// Folds the card to its heading, or opens it again.
    pub fn toggle_collapsed(&mut self) {
        self.collapsed = !self.collapsed;
    }

    /// Notes what a tick showed the human doing.
    pub(crate) fn observe(&mut self, human: PlayerId, events: &[Event]) {
        for event in events {
            match event {
                Event::ScrapDeposited { player, .. } if *player == human => {
                    self.evidence.deposited = true;
                }
                Event::UnitTrained { player, kind, .. }
                    if *player == human && !self.evidence.trained.contains(kind) =>
                {
                    self.evidence.trained.push(*kind);
                }
                _ => {}
            }
        }
    }

    /// Moves past every step the world already shows done. Returns true
    /// when this call finished the lesson.
    pub fn advance(&mut self, game: &Game) -> bool {
        let before = self.step;
        while let Some(step) = self.current() {
            if !step.goal.met(game, self) {
                break;
            }
            self.step += 1;
        }
        if self.step != before {
            // A new objective opens the card so it is read.
            self.collapsed = false;
        }
        self.step != before && self.finished()
    }

    /// Advances the lesson `game` stages, if any. Returns the lesson when
    /// this call finished it.
    pub fn advance_in(game: &mut Game) -> Option<Lesson> {
        let mut tutorial = game.tutorial.take()?;
        let finished = tutorial.advance(game);
        let lesson = tutorial.lesson;
        game.tutorial = Some(tutorial);
        finished.then_some(lesson)
    }

    /// The card's live line: the retry when stuck, otherwise what the step
    /// costs against the bank and who is mining, or the recovery nudge
    /// when it is unaffordable and nothing earns.
    pub fn coach(&self, game: &Game) -> Option<CoachLine> {
        let step = self.current()?;
        if !step.goal.reachable(game) {
            return Some(CoachLine::Stuck);
        }
        let (item, cost) = step.goal.spend(game)?;
        let seat = Seat::of(game);
        let bank = seat.bank();
        let mining = seat.harvesting();
        if bank < cost && mining == 0 && !seat.extracting() {
            return Some(CoachLine::Recovery(recovery_line(
                crate::platform::hands().touch(),
            )));
        }
        Some(CoachLine::Status(status_line(item, cost, bank, mining)))
    }
}

#[cfg(test)]
impl Tutorial {
    /// The same lesson standing at step `at`; the step count is finished.
    pub(crate) fn at_step(mut self, at: usize) -> Self {
        self.step = at;
        self
    }
}

/// The coach's price line: what the step buys, its cost, the bank, and
/// how many Harvesters are mining.
fn status_line(item: &str, cost: u32, bank: u32, mining: usize) -> String {
    let item = crate::typography::entity_name(item);
    let article = if item.starts_with(['A', 'E', 'I', 'O', 'U']) {
        "An"
    } else {
        "A"
    };
    let mining = match mining {
        0 => "no Harvesters are collecting".to_string(),
        1 => "1 Harvester is collecting".to_string(),
        n => format!("{n} Harvesters are collecting"),
    };
    format!("{article} {item} costs {cost} scrap. You have {bank}, and {mining}.")
}

/// The out-of-scrap nudge, in the order gesture the pointer in use has.
fn recovery_line(touch: bool) -> &'static str {
    if touch {
        "You can't afford this yet, and no Harvester is collecting. Tap the idle badge to select a \
         Harvester, then long-press a scrap pile."
    } else {
        "You can't afford this yet, and no Harvester is collecting. Click the idle badge to select a \
         Harvester, then right-click a scrap pile."
    }
}

/// The human seat's view of the authoritative world. A lesson has no
/// opponent to hide anything from, so its coach reads the world as it is.
struct Seat<'a> {
    state: &'a oxide_sim::State,
    human: PlayerId,
}

impl<'a> Seat<'a> {
    fn of(game: &'a Game) -> Self {
        Self {
            state: &game.state,
            human: game.presentation.human,
        }
    }

    fn bank(&self) -> u32 {
        self.state.player(self.human).scrap
    }

    fn units(&self) -> impl Iterator<Item = &'a oxide_sim::Unit> + use<'a> {
        let human = self.human;
        self.state.units().iter().filter(move |u| u.player == human)
    }

    fn workers(&self) -> impl Iterator<Item = &'a oxide_sim::Unit> + use<'a> {
        self.units().filter(|u| u.kind.stats().harvest.is_some())
    }

    fn harvesting(&self) -> usize {
        self.workers()
            .filter(|u| matches!(u.order, Order::Harvest { .. }))
            .count()
    }

    fn own_buildings(
        &self,
        kind: BuildingKind,
    ) -> impl Iterator<Item = &'a oxide_sim::Building> + use<'a> {
        let human = self.human;
        self.state
            .buildings()
            .iter()
            .filter(move |b| b.player == human && b.kind == kind)
    }

    fn hostile_buildings(&self) -> impl Iterator<Item = &'a oxide_sim::Building> + use<'a> {
        let (state, human) = (self.state, self.human);
        state
            .buildings()
            .iter()
            .filter(move |b| state.hostile(human, b.player))
    }

    /// Whether a built own factory trains `kind`.
    fn produces(&self, kind: UnitKind) -> bool {
        let human = self.human;
        self.state
            .buildings()
            .iter()
            .any(|b| b.player == human && b.built() && b.stats().produces.contains(&kind))
    }

    /// Whether an own factory has `kind` paid for and in its queue.
    fn queued(&self, kind: UnitKind) -> bool {
        let human = self.human;
        self.state
            .buildings()
            .iter()
            .any(|b| b.player == human && b.queue.contains(&kind))
    }

    /// Whether any scrap or wreck is left to mine.
    fn scrap_left(&self) -> bool {
        let map = self.state.map();
        (0..map.height()).any(|y| {
            (0..map.width()).any(|x| {
                let tile = chassis::grid::TilePos::new(x, y);
                map.scrap_at(tile) > 0 || map.wreck_at(tile) > 0
            })
        })
    }

    /// Whether a load can still reach the bank.
    fn can_mine(&self) -> bool {
        self.has_foundry()
            && self.workers().next().is_some()
            && (self.scrap_left()
                || self
                    .workers()
                    .any(|u| u.worker.is_some_and(|w| w.carrying > 0)))
    }

    /// Whether a built own Foundry is there to receive loads.
    fn has_foundry(&self) -> bool {
        self.own_buildings(BuildingKind::Foundry)
            .any(oxide_sim::Building::built)
    }

    /// Whether a built own Extractor pays.
    fn extracting(&self) -> bool {
        self.own_buildings(BuildingKind::Extractor)
            .any(oxide_sim::Building::built)
    }

    /// Whether the bank can still grow.
    fn can_earn(&self) -> bool {
        self.extracting() || self.can_mine()
    }

    /// How many more buildings of `kind` the map leaves room for. Only an
    /// Extractor needs authored ground: a frame nobody has claimed.
    fn room_for(&self, kind: BuildingKind) -> usize {
        if kind != BuildingKind::Extractor {
            return usize::MAX;
        }
        self.state
            .map()
            .extractor_frames()
            .iter()
            .filter(|frame| {
                !self
                    .state
                    .buildings()
                    .iter()
                    .any(|b| b.kind == BuildingKind::Extractor && b.anchor == **frame)
            })
            .count()
    }
}

#[cfg(test)]
mod tests;
