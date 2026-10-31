//! The New Match flow: the map browser grid, then the match setup
//! screen (seat cards grouped by team beside a large who-is-where
//! preview) for every map size. Enter-Enter from the grid launches the
//! map as authored.
//!
//! Every answer lands in the draft's per-seat vector, and `launch()`
//! reads only that.

use crate::bot_label::{difficulty_name, stance_name};
use crate::game::Game;
use crate::game::SoundKind;
use crate::game::checkpoint::LanSave;
use crate::game::network::NetRole;
use crate::menu::{Label, Menu};
use crate::menu::{Preview, PreviewCache, ScenarioEntry, discover_scenarios};
use crate::nav::{Axis, Nav, step_line};
use crate::netplay::{HostLobby, Link, StartPoll, shown_host_name, start_failed_notice};
use crate::numeric;
use crate::numeric::Fit;
use crate::press::{Fed, Press};
use crate::screens::browser::{Browser, Out as BrowserOut};
use crate::seating::{Occupant, Seating};
use crate::text_field::{Edit, Field, Form, Keyboard};
use aftermine_net::{InstallId, LobbyEvent, MemberId, PlayerName, RoomSeat, SeatHolder};
use aftermine_protocol::{Key, RawEvent};
use aftermine_sim::scenario::{BotDifficulty, BotStance};
use aftermine_sim::{PlayerId, Scenario};
use anyhow::{Context, Result};
use macroquad::prelude::{
    Rect, Vec2, draw_circle, draw_circle_lines, draw_rectangle, draw_rectangle_lines, draw_text,
    measure_text, vec2,
};
use std::path::PathBuf;
use std::time::Duration;

use crate::theme::{
    BORDER_FAINT, BORDER_STRONG, CARD_IDLE, CHIP, MIN_TOUCH_TARGET, SURFACE_MENU, SURFACE_PLATE,
    Stroke, TEXT_ACCENT, TEXT_DANGER, TEXT_PRIMARY, TEXT_SECONDARY, TEXT_TITLE, Type,
};

/// One seat's editable choices in the draft.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SeatPlan {
    /// Player-facing skill rung for this opponent. It remains attached to the
    /// chair if the human moves elsewhere; launch ignores it only for the
    /// chair the human ultimately takes.
    pub difficulty: BotDifficulty,
    /// Player-facing strategic posture for this opponent.
    pub stance: BotStance,
    /// Whether the chair waits for a LAN player rather than playing a bot.
    /// Only a hosting setup screen opens chairs; closing its lobby turns
    /// them back into their bots.
    pub open: bool,
    /// Team chip (feeds [`team_override`]): 0 is FFA — the seat stands
    /// alone — and `k` is Team `k`. [`NewMatchDraft::set_scenario`]
    /// seeds it from the map's authored teams, so the bare default is
    /// only right for maps that author none. Carried on every card,
    /// the human's included.
    pub team_choice: usize,
}

/// The scenario team a chip value writes onto its seat: `None` (FFA)
/// puts the seat on its own team; `Team k` becomes the 0-based id the
/// sim densifies by first appearance at build.
pub fn team_override(choice: usize) -> Option<u8> {
    choice.checked_sub(1).map(|team| team.fit::<u8>())
}

/// How the human's chair will see every seat once the draft launches: the
/// match's own self, ally, and hostile colors, from the chosen teams.
fn draft_styles(scenario: &Scenario, draft: &NewMatchDraft) -> crate::seat_style::SeatStyles {
    seat_styles(scenario.players.len(), draft.seat_choice, |seat| {
        draft
            .seats
            .get(seat)
            .and_then(|plan| team_override(plan.team_choice))
    })
}

/// How seat `mine` will see each of `seats` seats in the match: self, ally,
/// and hostile colors from each seat's team, where `None` stands alone.
pub(crate) fn seat_styles(
    seats: usize,
    mine: usize,
    team: impl Fn(usize) -> Option<u8>,
) -> crate::seat_style::SeatStyles {
    use crate::seat_style::AllegianceCue;
    let my_team = team(mine);
    crate::seat_style::SeatStyles::from_cues(
        (0..seats).map(|seat| {
            if seat == mine {
                AllegianceCue::Mine
            } else if my_team.is_some() && team(seat) == my_team {
                AllegianceCue::Ally
            } else {
                AllegianceCue::Hostile
            }
        }),
        crate::render::colorblind(),
    )
}

/// The team chip's display label, aligned with [`team_override`].
pub fn team_chip_label(choice: usize) -> String {
    match choice {
        0 => "FFA".to_string(),
        k => format!("Team {k}"),
    }
}

/// The team chip a seat opens on: its authored team shown as the
/// dense first-appearance ordinal (`Team 1`, `Team 2`, ...) — the same
/// normalization the sim applies at build — or FFA when the seat
/// authors none.
fn default_team_choice(scenario: &Scenario, seat: usize) -> usize {
    let Some(team) = scenario.players.get(seat).and_then(|p| p.team) else {
        return 0;
    };
    let mut seen: Vec<u8> = Vec::new();
    for player in &scenario.players {
        if let Some(t) = player.team
            && !seen.contains(&t)
        {
            seen.push(t);
        }
    }
    seen.iter().position(|t| *t == team).map_or(0, |i| i + 1)
}

/// Fresh per-seat plans for a map: every choice at its default, the team
/// chips seeded from the authored teams.
fn authored_seat_plans(scenario: &Scenario) -> Vec<SeatPlan> {
    (0..scenario.players.len())
        .map(|seat| {
            let bot = scenario.players[seat].bot_config.unwrap_or_default();
            SeatPlan {
                difficulty: bot.difficulty,
                stance: bot.stance,
                team_choice: default_team_choice(scenario, seat),
                ..SeatPlan::default()
            }
        })
        .collect()
}

/// Whether the draft groups every seat onto one team — the sim's
/// `OneTeam` build refusal (nobody to fight), caught here so Start can
/// say why instead of failing the launch. All-FFA is the opposite
/// extreme and always legal: every seat stands alone.
fn draft_one_team(draft: &NewMatchDraft) -> bool {
    let Some(scenario) = draft.scenario.as_deref() else {
        return false;
    };
    let n = scenario.players.len();
    if n < 2 {
        return false;
    }
    let mut choices = (0..n).map(|i| draft.seats.get(i).map_or(0, |p| p.team_choice));
    let Some(first) = choices.next() else {
        return false;
    };
    first != 0 && choices.all(|c| c == first)
}

/// Everything New Match has chosen so far. The draft outlives every
/// screen transition: backing from any step to the map list and
/// forward again re-offers each earlier answer instead of forgetting
/// it.
#[derive(Default)]
pub struct NewMatchDraft {
    /// The loaded map, once picked.
    pub scenario: Option<Box<Scenario>>,
    /// The picked map's path (`None` = the embedded skirmish), keyed by
    /// path so the browser's section sort can never move the remembered
    /// highlight onto a different map.
    pub scenario_path: Option<PathBuf>,
    /// Which chair the human takes (index into the scenario's players).
    pub seat_choice: usize,
    /// One plan per seat, aligned with the scenario's player list and
    /// re-derived whenever the scenario changes — Back from an 8-seat
    /// map to a 2-seat map must not leave a stale seat 7. The human's
    /// own row is inert at launch.
    pub seats: Vec<SeatPlan>,
    /// The player's saved name, which a hosted lobby shows for the host.
    pub player_name: Option<aftermine_net::PlayerName>,
}

impl NewMatchDraft {
    /// Installs a picked map. Re-entering the same map keeps every
    /// earlier answer, so the draft survives Back; a different map resets
    /// the seats and the chair, so no chair index carries silently onto a
    /// map with different seats.
    pub fn set_scenario(&mut self, scenario: Scenario, path: Option<PathBuf>) {
        let same_map = self.scenario.is_some() && self.scenario_path == path;
        let count = scenario.players.len();
        let defaults = authored_seat_plans(&scenario);
        if same_map {
            if self.seats.len() < count {
                self.seats.extend_from_slice(&defaults[self.seats.len()..]);
            } else {
                self.seats.truncate(count);
            }
            self.seat_choice = self.seat_choice.min(count.saturating_sub(1));
        } else {
            self.seats = defaults;
            self.seat_choice = 0;
        }
        self.scenario = Some(Box::new(scenario));
        self.scenario_path = path;
    }
}

const COMPACT_PAGE_ITEMS: usize = 5;

/// The setup screen's coaching line. The keyboard hint follows the
/// cursor; a pointer moves the cursor anyway, so its hint is one line.
fn setup_hint(
    one_team: bool,
    on_start: bool,
    cell: Cell,
    hands: crate::platform::Hands,
) -> &'static str {
    match (hands.touch(), hands.keys, one_team) {
        (true, false, true) => {
            "every seat is on one team, nobody to fight - tap a TEAM chip to regroup"
        }
        (true, false, false) => "tap a seat to take it - tap a chip to change it",
        (true, true, true) => {
            "every seat is on one team, nobody to fight - tap a TEAM chip to regroup - {back} back"
        }
        (true, true, false) => "tap a seat to take it - tap a chip to change it - {back} back",
        (false, false, true) => {
            "every seat is on one team, nobody to fight - click a TEAM chip to regroup"
        }
        (false, false, false) => "click a seat to take it - click a chip to change it",
        (false, true, true) => {
            "every seat is on one team, nobody to fight - regroup a TEAM chip - {back} back"
        }
        (false, true, false) if on_start => "{confirm} starts the match - {back} back",
        (false, true, false) => match cell {
            Cell::Difficulty => "{confirm} cycles difficulty - {left}/{right} move - {back} back",
            Cell::Stance => "{confirm} cycles stance - {left}/{right} move - {back} back",
            Cell::Team => "{confirm} cycles the chip - {left}/{right} move - {back} back",
            Cell::Seat => {
                "{confirm} takes this seat - {left}/{right} reach difficulty, stance, and team - {back} back"
            }
        },
    }
}

/// The coaching line while an occupant is picked up.
fn moving_hint(hands: crate::platform::Hands) -> &'static str {
    match (hands.touch(), hands.keys) {
        (true, false) => "tap the seat to swap with - tap the same seat to cancel",
        (false, false) => "click the seat to swap with - click the same seat to cancel",
        (_, true) => "{confirm} on a seat swaps them there - {back} cancels",
    }
}

/// Which wizard screen is up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// The map browser grid.
    Map,
    /// Match setup, every map size: seat cards by team, Start, and a
    /// live map.
    Setup,
}

/// What a wizard frame decided.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Out {
    /// Still asking.
    Stay,
    /// Backed all the way out to the front door.
    Home,
    /// Every question answered: the caller launches from the draft.
    Launch,
}

/// The wizard: current step, the browser grid, and the setup cursor.
pub struct Wizard {
    /// Which screen is up.
    pub step: Step,
    /// Discovered scenario entries, section-sorted.
    pub entries: Vec<ScenarioEntry>,
    /// The map grid's state.
    pub browser: Browser,
    /// Setup cursor over the DISPLAY order: seats grouped by team,
    /// then the Start button.
    pub setup_sel: usize,
    /// Which cell of the selected seat card the cursor is on: 0 the
    /// seat itself, then its controls left to right: 1 difficulty,
    /// 2 stance, and 3 team. The two bot controls are absent
    /// from the human's row.
    pub setup_cell: Cell,
    /// Setup zone armed by a press; activation on release inside the
    /// same zone.
    setup_press: Press<SetupZone>,
    /// The corner Back button: Home from the grid, the grid from setup.
    back: crate::button::BackButton,
    /// Compact setup page. Full-height layouts always clamp this to zero.
    setup_page: usize,
    /// The LAN lobby while this setup screen hosts one.
    lan: Option<Box<LanHost>>,
    /// The cursor sits on the LOCAL | LAN switch above the cards.
    on_switch: bool,
    /// The seat whose occupant was picked up, waiting for the seat to
    /// swap with.
    moving: Option<usize>,
    /// A name prompt or a confirmation over the setup screen.
    overlay: Option<Overlay>,
    /// Why hosting or a start failed, until the next change.
    notice: Option<String>,
    /// A name the player gave the prompt, for the caller to save.
    saved_name: Option<PlayerName>,
    /// Where a lobby listens.
    pub(crate) lan_bind: String,
    /// The saved LAN match this setup screen resumes, if it resumes one.
    resume: Option<Box<Resume>>,
    /// The host chose to leave a resume for Home.
    leaving: bool,
}

/// A saved LAN match the setup screen resumes. Its map, bots, and teams are
/// fixed; each joined player's seat waits, reserved, for its player.
struct Resume {
    save: LanSave,
    /// The New Match draft this screen stands in for, given back when the
    /// resume ends.
    previous: NewMatchDraft,
}

impl Resume {
    /// Whether `seat` was a joined player's.
    fn reserved(&self, seat: usize) -> bool {
        self.save
            .players()
            .iter()
            .any(|(joined, _)| usize::from(joined.0) == seat)
    }

    /// The name `seat` was played under.
    fn saved_name(&self, seat: usize) -> &str {
        &self.save.scenario().players[seat].name
    }

    /// The reserved seat a joining player takes: the one its installation
    /// played, else the one played under its name, else the first free one
    /// in display order. An installation or a name another joined player
    /// shares identifies nobody. `None` when no reserved seat is free.
    fn seat_for(
        &self,
        draft: &NewMatchDraft,
        members: &[Option<MemberId>],
        lobby: &HostLobby,
        joiner: (&PlayerName, InstallId),
        order: &[usize],
    ) -> Option<usize> {
        let (name, install) = joiner;
        let free =
            |seat: usize| self.reserved(seat) && draft.seats[seat].open && members[seat].is_none();
        let joined = || members.iter().flatten();
        let saved = || {
            self.save
                .players()
                .iter()
                .map(|&(seat, saved)| (usize::from(seat.0), saved))
                .filter(|&(seat, _)| free(seat))
        };
        let by_install = || {
            (!joined().any(|&member| lobby.install(member) == Some(install)))
                .then(|| saved().find(|&(_, saved)| saved == install))
                .flatten()
        };
        let by_name = || {
            (!joined().any(|&member| lobby.name(member) == Some(name)))
                .then(|| saved().find(|&(seat, _)| self.saved_name(seat) == name.as_str()))
                .flatten()
        };
        by_install()
            .or_else(by_name)
            .map(|(seat, _)| seat)
            .or_else(|| order.iter().copied().find(|&seat| free(seat)))
    }

    /// Shows each reserved seat nobody holds as waiting for its player.
    fn reserve(&self, draft: &NewMatchDraft, members: &[Option<MemberId>], room: &mut [RoomSeat]) {
        for &(seat, _) in self.save.players() {
            let seat = usize::from(seat.0);
            if draft.seats[seat].open
                && members.get(seat).copied().flatten().is_none()
                && let Ok(name) = PlayerName::new(self.saved_name(seat))
            {
                room[seat].holder = SeatHolder::Reserved { name };
            }
        }
    }

    /// The reserved seats the host handed to bots, with their bots.
    fn replaced(&self, draft: &NewMatchDraft) -> Vec<(PlayerId, BotDifficulty, BotStance)> {
        self.save
            .players()
            .iter()
            .map(|&(seat, _)| (seat, draft.seats[usize::from(seat.0)]))
            .filter(|(_, plan)| !plan.open)
            .map(|(seat, plan)| (seat, plan.difficulty, plan.stance))
            .collect()
    }
}

/// One collision-free team key per seat: an authored id stays itself;
/// an omitted seat uses its own index lifted above the whole u8 range,
/// so no authored id can alias it and every pass that groups seats
/// derives the identical key.
fn seat_team_keys(scenario: &Scenario) -> Vec<u16> {
    scenario
        .players
        .iter()
        .enumerate()
        .map(|(i, p)| p.team.map_or(256 + i.fit::<u16>(), u16::from))
        .collect()
}

/// Seats in DISPLAY order: grouped by team (first appearance), seat
/// order within — the setup screen's visual order and its cursor's
/// walking order are the same list.
pub fn seat_display_order(scenario: &Scenario) -> Vec<usize> {
    let keys = seat_team_keys(scenario);
    let mut teams: Vec<u16> = Vec::new();
    for &k in &keys {
        if !teams.contains(&k) {
            teams.push(k);
        }
    }
    let mut order: Vec<usize> = Vec::new();
    for team in &teams {
        for (i, &k) in keys.iter().enumerate() {
            if k == *team {
                order.push(i);
            }
        }
    }
    order
}

/// The setup screen's frame geometry, a pure function of the map and
/// window: seat cards on the left grouped under team headings, the
/// Start button beneath them, the preview panel filling the right.
pub struct SetupLayout {
    /// The LOCAL | LAN switch above the cards: its two halves.
    pub switch: [Rect; 2],
    /// Team headings and their text rects.
    pub headings: Vec<(String, Rect)>,
    /// One card per DISPLAY position (see [`seat_display_order`]);
    /// `None` while the card sits on another compact page.
    pub cards: Vec<Option<CardRects>>,
    /// The Start button; `None` while it sits on another compact page.
    pub start: Option<Rect>,
    /// Where the map preview draws.
    pub preview: Rect,
    /// Previous compact page control, when another page precedes this one.
    pub page_prev: Option<Rect>,
    /// Next compact page control, when another page follows this one.
    pub page_next: Option<Rect>,
    /// Current zero-based compact page.
    pub page: usize,
    /// Number of compact pages; one means no pagination chrome is needed.
    pub page_count: usize,
    /// Half-open protocol item range actually drawn on this page.
    pub visible_range: [usize; 2],
}

/// One control on a seat card, left to right as the keyboard walks them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    /// The card itself: Enter takes the seat.
    Seat,
    Difficulty,
    Stance,
    Team,
}

impl Cell {
    /// Every cell, left to right.
    pub const ALL: [Self; 4] = [Self::Seat, Self::Difficulty, Self::Stance, Self::Team];

    fn index(self) -> usize {
        match self {
            Self::Seat => 0,
            Self::Difficulty => 1,
            Self::Stance => 2,
            Self::Team => 3,
        }
    }
}

/// Where one seat card and its controls sit. Your own card has no
/// difficulty or stance chip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardRects {
    /// The whole card.
    pub card: Rect,
    /// The seat zone left of the chips.
    pub seat: Rect,
    pub difficulty: Option<Rect>,
    pub stance: Option<Rect>,
    pub team: Rect,
}

impl CardRects {
    /// Where `cell` sits, if this card has it.
    pub fn cell(&self, cell: Cell) -> Option<Rect> {
        match cell {
            Cell::Seat => Some(self.seat),
            Cell::Difficulty => self.difficulty,
            Cell::Stance => self.stance,
            Cell::Team => Some(self.team),
        }
    }

    /// A fingertip's target for `cell`: the card's full height, reaching
    /// halfway to the neighboring controls.
    fn touch_cell(&self, cell: Cell) -> Option<Rect> {
        let rect = self.cell(cell)?;
        let index = cell.index();
        let left = Cell::ALL[..index]
            .iter()
            .rev()
            .find_map(|candidate| self.cell(*candidate))
            .map_or(self.card.x, |previous| {
                (previous.x + previous.w + rect.x) * 0.5
            });
        let right = Cell::ALL[index + 1..]
            .iter()
            .find_map(|candidate| self.cell(*candidate))
            .map_or(self.card.x + self.card.w, |next| {
                (rect.x + rect.w + next.x) * 0.5
            });
        Some(Rect::new(left, self.card.y, right - left, self.card.h))
    }
}

/// What a press on the setup screen arms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupZone {
    Card {
        row: usize,
        cell: Cell,
    },
    /// A half of the LOCAL | LAN switch; `true` is LAN.
    Switch(bool),
    Start,
    PrevPage,
    NextPage,
}

/// Computes [`SetupLayout`]. `seat_choice` marks the human card; every
/// other seat receives direct difficulty and stance controls.
#[cfg(test)]
pub fn setup_layout(scenario: &Scenario, seat_choice: usize, view: Vec2, ui: f32) -> SetupLayout {
    setup_layout_page(scenario, seat_choice, &[], view, ui, 0)
}

/// The switch's two halves at the top of the seat column, and the top of
/// what follows it.
fn switch_rects(left_x: f32, top: f32, ui: f32) -> ([Rect; 2], f32) {
    let half = 110.0 * ui;
    let height = MIN_TOUCH_TARGET.max(36.0 * ui);
    (
        [
            Rect::new(left_x, top, half, height),
            Rect::new(left_x + half, top, half, height),
        ],
        top + height + 10.0 * ui,
    )
}

/// `open` marks the chairs (by seat) that wait for LAN players, whose
/// cards trade the difficulty and stance chips for one wide chip.
pub(crate) fn setup_layout_page(
    scenario: &Scenario,
    seat_choice: usize,
    open: &[bool],
    view: Vec2,
    ui: f32,
    page: usize,
) -> SetupLayout {
    let order = seat_display_order(scenario);
    let n = order.len();
    let keys = seat_team_keys(scenario);
    let teams: Vec<u16> = {
        let mut seen = Vec::new();
        for &s in &order {
            let t = keys[s];
            if !seen.contains(&t) {
                seen.push(t);
            }
        }
        seen
    };
    let left_x = 56.0 * ui;
    let left_w = (view.x * 0.42).min(520.0 * ui);
    // Margins yield before content: small windows compress the title
    // zone first, then chrome, then the cards. Cards shorter than a touch
    // target switch to the paginated compact layout below.
    let (switch, top) = switch_rects(left_x, (132.0 * ui).min(view.y * 0.22), ui);
    let bottom = view.y - (44.0 * ui).min(view.y * 0.08);
    // Headings take rows only when a team actually groups seats; a duel
    // or an FFA would otherwise show one heading per lone seat.
    let grouped = teams.len() < n;
    let heading_rows = if grouped { teams.len() as f32 } else { 0.0 };
    let mut heading_h = 26.0 * ui;
    let mut start_h = 46.0 * ui;
    let mut gap = 6.0 * ui;
    let mut avail = bottom - top - heading_rows * heading_h - start_h - 24.0 * ui;
    let mut card_h = ((avail / n.max(1) as f32) - gap).clamp(34.0 * ui, 56.0 * ui);
    if n as f32 * (card_h + gap) > avail {
        heading_h *= 0.7;
        start_h *= 0.75;
        gap *= 0.5;
        avail = bottom - top - heading_rows * heading_h - start_h - 24.0 * ui;
        card_h = ((avail / n.max(1) as f32) - gap).max(18.0 * ui);
        // When even the ui-scaled floor overflows, the floor goes
        // physical; such cards fall below a touch target, so the compact
        // layout takes over below.
        if n as f32 * (card_h + gap) > avail {
            card_h = ((avail / n.max(1) as f32) - gap).max(14.0);
        }
    }

    if card_h < MIN_TOUCH_TARGET {
        return compact_setup_layout(scenario, seat_choice, open, view, ui, page);
    }

    let mut headings = Vec::new();
    let mut cards = Vec::with_capacity(n);
    let mut y = top;
    let mut last_team: Option<u16> = None;
    for &seat in &order {
        let team = keys[seat];
        if grouped && last_team != Some(team) {
            let label = format!(
                "TEAM {}",
                teams.iter().position(|t| *t == team).unwrap() + 1
            );
            headings.push((label, Rect::new(left_x, y, left_w, heading_h)));
            last_team = Some(team);
            y += heading_h;
        }
        let card = Rect::new(left_x, y, left_w, card_h);
        let wide = open.get(seat).copied().unwrap_or(false);
        cards.push(Some(setup_card_controls(card, seat, seat_choice, wide, ui)));
        y += card_h + gap;
    }
    let start = Rect::new(left_x, y + 12.0 * ui, 240.0 * ui, start_h);
    let px = left_x + left_w + 28.0 * ui;
    let preview = Rect::new(px, top, view.x - px - 40.0 * ui, bottom - top - 20.0 * ui);
    SetupLayout {
        switch,
        headings,
        cards,
        start: Some(start),
        preview,
        page_prev: None,
        page_next: None,
        page: 0,
        page_count: 1,
        // Every seat, Start, and the switch the protocol lists last.
        visible_range: [0, n + 2],
    }
}

fn setup_card_controls(
    card: Rect,
    seat: usize,
    seat_choice: usize,
    wide: bool,
    ui: f32,
) -> CardRects {
    // Controls occupy equal semantic lanes on the right. Their visible
    // rectangles are inset within those lanes, so even the compact 640px
    // layout keeps every mouse and touch target independent and at least
    // 44 logical pixels wide.
    let inset = (4.0 * ui).clamp(2.0, 6.0);
    let chip_h = (card.h * 0.72).clamp(10.0, 40.0 * ui);
    let chip_y = card.y + (card.h - chip_h) * 0.5;
    if seat == seat_choice {
        let controls_w = (card.w * 0.18)
            .max(MIN_TOUCH_TARGET)
            .min(card.w - MIN_TOUCH_TARGET);
        let lane_w = controls_w;
        let controls_x = card.x + card.w - controls_w;
        let control = |lane: usize| {
            Rect::new(
                controls_x + lane as f32 * lane_w + inset * 0.5,
                chip_y,
                (lane_w - inset).max(1.0),
                chip_h,
            )
        };
        CardRects {
            card,
            seat: Rect::new(card.x, card.y, controls_x - card.x, card.h),
            difficulty: None,
            stance: None,
            team: control(0),
        }
    } else {
        let controls_w = (card.w * 0.54)
            .max(MIN_TOUCH_TARGET * 3.0)
            .min(card.w - MIN_TOUCH_TARGET);
        let lane_w = controls_w / 3.0;
        let controls_x = card.x + card.w - controls_w;
        let control = |lane: usize| {
            Rect::new(
                controls_x + lane as f32 * lane_w + inset * 0.5,
                chip_y,
                (lane_w - inset).max(1.0),
                chip_h,
            )
        };
        // An open chair's one chip spans the difficulty and stance lanes.
        let (difficulty, stance) = if wide {
            let first = control(0);
            let second = control(1);
            (
                Rect::new(first.x, first.y, second.x + second.w - first.x, first.h),
                None,
            )
        } else {
            (control(0), Some(control(1)))
        };
        CardRects {
            card,
            seat: Rect::new(card.x, card.y, controls_x - card.x, card.h),
            difficulty: Some(difficulty),
            stance,
            team: control(2),
        }
    }
}

fn compact_setup_layout(
    scenario: &Scenario,
    seat_choice: usize,
    open: &[bool],
    view: Vec2,
    ui: f32,
    requested_page: usize,
) -> SetupLayout {
    let left_x = 56.0 * ui;
    let left_w = (view.x * 0.42).min(520.0 * ui);
    let top = (132.0 * ui).min(view.y * 0.22);
    let bottom = view.y - (44.0 * ui).min(view.y * 0.08);
    let order = seat_display_order(scenario);
    // Paged cards need every pixel of the column, so the switch tops the
    // preview instead.
    let px = left_x + left_w + 28.0 * ui;
    let (switch, preview_top) = switch_rects(px, top, ui);
    let n = order.len();
    let total_items = n + 1;
    let page_count = total_items.div_ceil(COMPACT_PAGE_ITEMS).max(1);
    let page = requested_page.min(page_count - 1);
    let first = page * COMPACT_PAGE_ITEMS;
    let past = (first + COMPACT_PAGE_ITEMS).min(total_items);
    let pager_h = MIN_TOUCH_TARGET;
    let gap = 3.0;
    let pager_gap = 4.0;
    let pager_y = bottom - pager_h;
    let content_bottom = pager_y - pager_gap;
    let item_h = ((content_bottom - top - gap * (COMPACT_PAGE_ITEMS - 1) as f32)
        / COMPACT_PAGE_ITEMS as f32)
        .max(1.0);

    let mut cards = vec![None; n];
    let mut start = None;
    for item in first..past {
        let slot = item - first;
        let rect = Rect::new(left_x, top + slot as f32 * (item_h + gap), left_w, item_h);
        if item == n {
            start = Some(Rect::new(rect.x, rect.y, (240.0 * ui).min(rect.w), rect.h));
        } else {
            let wide = open.get(order[item]).copied().unwrap_or(false);
            cards[item] = Some(setup_card_controls(
                rect,
                order[item],
                seat_choice,
                wide,
                ui,
            ));
        }
    }

    let split_gap = 6.0 * ui;
    let pager_w = (left_w - split_gap) * 0.5;
    let page_prev = (page > 0).then(|| Rect::new(left_x, pager_y, pager_w, pager_h));
    let page_next = (page + 1 < page_count)
        .then(|| Rect::new(left_x + pager_w + split_gap, pager_y, pager_w, pager_h));
    let preview = Rect::new(
        px,
        preview_top,
        view.x - px - 40.0 * ui,
        bottom - preview_top - 20.0,
    );
    SetupLayout {
        switch,
        headings: Vec::new(),
        cards,
        start,
        preview,
        page_prev,
        page_next,
        page,
        page_count,
        visible_range: [first, past],
    }
}

/// Steps the controller chip forward: every difficulty, then Open while
/// hosting a LAN lobby, then back to the first difficulty. A chair the
/// LAN switch opened returns to the bot it had.
fn cycle_controller(plan: &mut SeatPlan, lan: bool) {
    if plan.open {
        plan.open = false;
    } else if Some(&plan.difficulty) == BotDifficulty::ALL.last() {
        if lan {
            plan.open = true;
            plan.difficulty = BotDifficulty::ALL[0];
        } else {
            plan.difficulty = BotDifficulty::ALL[0];
        }
    } else {
        plan.difficulty = cycle_difficulty(plan.difficulty, 1);
    }
}

/// Swaps who plays seats `a` and `b`: the human's chair, a bot with its
/// difficulty and stance, an open chair, or the LAN member in it. Each
/// seat keeps its team.
fn swap_seats(draft: &mut NewMatchDraft, members: &mut [Option<MemberId>], a: usize, b: usize) {
    let (team_a, team_b) = (draft.seats[a].team_choice, draft.seats[b].team_choice);
    draft.seats.swap(a, b);
    draft.seats[a].team_choice = team_a;
    draft.seats[b].team_choice = team_b;
    if a < members.len() && b < members.len() {
        members.swap(a, b);
    }
    if draft.seat_choice == a {
        draft.seat_choice = b;
    } else if draft.seat_choice == b {
        draft.seat_choice = a;
    }
}

/// The address a hosting lobby shows, without the port every join
/// assumes.
fn shown_address(address: &str) -> String {
    address.replace(&format!(":{}", aftermine_net::DEFAULT_PORT), "")
}

/// A setup screen's LAN lobby: the sockets and who sits in each open
/// chair.
struct LanHost {
    lobby: HostLobby,
    /// Which joined member sits in each seat; only open chairs hold one.
    members: Vec<Option<MemberId>>,
    /// A running start.
    starting: Option<LanStart>,
}

/// A start the setup screen's lobby is running.
struct LanStart {
    /// The host's match at tick zero.
    game: Box<Game>,
    /// The seating the start froze.
    seated: Vec<(MemberId, PlayerId)>,
}

impl LanHost {
    fn seated(&self) -> Vec<(MemberId, PlayerId)> {
        self.members
            .iter()
            .enumerate()
            .filter_map(|(seat, member)| member.map(|member| (member, PlayerId(seat.fit::<u8>()))))
            .collect()
    }

    fn has_members(&self) -> bool {
        self.members.iter().any(Option::is_some)
    }

    fn name_at(&self, seat: usize) -> Option<&PlayerName> {
        self.members
            .get(seat)
            .copied()
            .flatten()
            .and_then(|member| self.lobby.name(member))
    }
}

/// The seating a draft and its lobby describe.
fn draft_seating(draft: &NewMatchDraft, members: &[Option<MemberId>]) -> Seating {
    Seating::from_occupants(
        draft
            .seats
            .iter()
            .enumerate()
            .map(|(seat, plan)| {
                if seat == draft.seat_choice {
                    Occupant::Host
                } else if plan.open {
                    members
                        .get(seat)
                        .copied()
                        .flatten()
                        .map_or(Occupant::Open, Occupant::Member)
                } else {
                    Occupant::Bot {
                        difficulty: plan.difficulty,
                        stance: plan.stance,
                    }
                }
            })
            .collect(),
    )
}

/// Something over the setup screen that takes its input.
enum Overlay {
    /// Asking for the player's name before hosting.
    Naming(Form),
    /// Asking before a step that drops players; Cancel sits preselected.
    Confirm { ask: Confirm, menu: Menu<bool> },
}

/// A step that asks first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Confirm {
    /// Remove the member in a seat.
    Kick { member: MemberId },
    /// Close the lobby, then go local or back to the map grid.
    Close { to_map: bool },
}

fn cycle_difficulty(current: BotDifficulty, direction: i8) -> BotDifficulty {
    let index = BotDifficulty::ALL
        .iter()
        .position(|candidate| *candidate == current)
        .expect("every difficulty is in ALL");
    let len = BotDifficulty::ALL.len();
    let next = if direction < 0 {
        index.checked_sub(1).unwrap_or(len - 1)
    } else {
        (index + 1) % len
    };
    BotDifficulty::ALL[next]
}

fn cycle_stance(current: BotStance, direction: i8) -> BotStance {
    let index = BotStance::ALL
        .iter()
        .position(|candidate| *candidate == current)
        .expect("every stance is in ALL");
    let len = BotStance::ALL.len();
    let next = if direction < 0 {
        index.checked_sub(1).unwrap_or(len - 1)
    } else {
        (index + 1) % len
    };
    BotStance::ALL[next]
}

/// Every Foundry anchor authored on an ASCII map: `(seat, (x, y))` for
/// each digit `1`..=`8` and letter `a`..=`h` (seats 9-16), in
/// row-major order.
pub fn seat_anchors(map: &[String]) -> Vec<(usize, (i32, i32))> {
    let mut anchors = Vec::new();
    for (y, row) in map.iter().enumerate() {
        for (x, ch) in row.chars().enumerate() {
            let seat = match ch {
                '1'..='8' => Some(ch as usize - '1' as usize),
                'a'..='h' => Some(8 + ch as usize - 'a' as usize),
                _ => None,
            };
            if let Some(seat) = seat {
                anchors.push((seat, (x.fit::<i32>(), y.fit::<i32>())));
            }
        }
    }
    anchors
}

/// Marks every seat's foundry on a drawn preview rect: numbered discs
/// in the seat's ownership color as `seat_choice` will see it, a white
/// ring for that chair, an accent ring for the focused seat.
pub fn draw_seat_markers(
    scenario: &Scenario,
    styles: &crate::seat_style::SeatStyles,
    rect: Rect,
    seat_choice: usize,
    focus_seat: Option<usize>,
    ui: f32,
) {
    let map_w = scenario.map.first().map_or(1, |r| r.chars().count()) as f32;
    let map_h = scenario.map.len() as f32;
    for (seat, (ax, ay)) in seat_anchors(&scenario.map) {
        if scenario.players.get(seat).is_none() {
            continue;
        }
        // Foundry anchors are the 2x2's top-left; mark its center.
        let px = rect.x + (ax as f32 + 1.0) / map_w * rect.w;
        let py = rect.y + (ay as f32 + 1.0) / map_h * rect.h;
        let accent = styles.get(aftermine_sim::PlayerId::from_index(seat));
        if seat == seat_choice {
            draw_circle_lines(px, py, 10.0 * ui, 2.5, macroquad::prelude::WHITE);
        } else if focus_seat == Some(seat) {
            draw_circle_lines(px, py, 10.0 * ui, 2.0, accent);
        }
        draw_circle(px, py, 7.5 * ui, accent);
        let label = format!("{}", seat + 1);
        let tw = measure_text(&label, None, numeric::font_size(Type::Small.at(ui)), 1.0).width;
        draw_text(
            &label,
            px - tw * 0.5,
            py + 4.5 * ui,
            Type::Small.at(ui),
            SURFACE_PLATE,
        );
    }
}

/// Draws the setup layout's team headings, each with a rule to its right.
pub(crate) fn draw_team_headings(layout: &SetupLayout, ui: f32) {
    for (label, rect) in &layout.headings {
        draw_text(
            label,
            rect.x,
            rect.y + rect.h * 0.7,
            Type::Body.at(ui),
            TEXT_TITLE,
        );
        let dims = measure_text(label, None, numeric::font_size(Type::Body.at(ui)), 1.0);
        draw_rectangle(
            rect.x + dims.width + 12.0 * ui,
            rect.y + rect.h * 0.55,
            rect.w - dims.width - 12.0 * ui,
            Stroke::Hairline.at(ui),
            BORDER_FAINT,
        );
    }
}

/// Draws a map preview centered in `area`, graded with the map's theme,
/// and returns where the map itself landed.
pub(crate) fn draw_preview(preview: &Preview<'_>, theme: &str, area: Rect, ui: f32) -> Rect {
    let rect = preview.rect(area);
    draw_rectangle(
        rect.x - 8.0 * ui,
        rect.y - 8.0 * ui,
        rect.w + 16.0 * ui,
        rect.h + 16.0 * ui,
        SURFACE_MENU,
    );
    preview.draw(area, crate::render::theme_tint(theme))
}

/// Who a seat card's controller chip names.
pub(crate) enum CardController<'a> {
    /// The viewer's own card, which has no controller chip.
    Own,
    /// A bot.
    Bot {
        difficulty: BotDifficulty,
        stance: BotStance,
    },
    /// A chair waiting for a LAN player.
    Open,
    /// A chair waiting for the player who held it in a saved match.
    Reserved { name: &'a str },
    /// A LAN player, and whether they host.
    Person { name: &'a str, host: bool },
}

/// What one seat card shows.
pub(crate) struct CardView<'a> {
    pub seat: usize,
    /// The seat's ownership color as the viewer will see it.
    pub accent: macroquad::prelude::Color,
    /// The seat's name.
    pub name: &'a str,
    pub controller: CardController<'a>,
    pub team: String,
    /// The cursor's cell, while the cursor is on this card.
    pub focus: Option<Cell>,
    /// The card's occupant is picked up to move.
    pub moving: bool,
}

/// Draws one seat card: its numbered disc, its name, its controller chip
/// (one wide chip for a person or an open chair), and its team chip.
pub(crate) fn draw_seat_card(card: &CardRects, view: &CardView, ui: f32) {
    let rect = &card.card;
    let seat = view.seat;
    let display = view.name;
    let selected = view.focus.is_some();
    let is_you = matches!(view.controller, CardController::Own);
    let moving = view.moving;
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, SURFACE_MENU);
    draw_rectangle_lines(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        if moving {
            Stroke::Heavy
        } else if selected {
            Stroke::Focus
        } else {
            Stroke::Hairline
        }
        .at(ui),
        if moving {
            TEXT_ACCENT
        } else if selected {
            TEXT_TITLE
        } else {
            BORDER_FAINT
        },
    );
    let accent = view.accent;
    let cy = rect.y + rect.h * 0.5;
    let chip_x = rect.x + 22.0 * ui;
    // Everything on a card scales to the card, so a compressed
    // roster never draws across its neighbors and chips.
    let disc = (10.0 * ui).min(rect.h * 0.38);
    if is_you {
        draw_circle_lines(chip_x, cy, disc * 1.3, 2.0, macroquad::prelude::WHITE);
    }
    draw_circle(chip_x, cy, disc, accent);
    let num = format!("{}", seat + 1);
    let num_font = Type::Small.at(ui).min(rect.h * 0.55);
    let ndims = measure_text(&num, None, numeric::font_size(num_font), 1.0);
    draw_text(
        &num,
        chip_x - ndims.width * 0.5,
        cy + num_font * 0.35,
        num_font,
        SURFACE_PLATE,
    );
    let text_right = card
        .difficulty
        .map_or(card.seat.x + card.seat.w, |difficulty| difficulty.x);
    let name_room = (text_right - rect.x - 48.0 * ui).max(20.0);
    let name_font = crate::typography::fit(
        display,
        Type::Body.at(ui).min(rect.h * 0.62),
        name_room,
        8.0,
    );
    draw_text(
        display,
        rect.x + 44.0 * ui,
        cy + name_font * 0.35,
        name_font,
        TEXT_PRIMARY,
    );
    if is_you || moving {
        let tag = if moving { "moving" } else { "your seat" };
        let tag_font = Type::Small.at(ui).min(rect.h * 0.55);
        let tdims = measure_text(tag, None, numeric::font_size(tag_font), 1.0);
        draw_text(
            tag,
            card.team.x - tdims.width - 14.0 * ui,
            cy + tag_font * 0.35,
            tag_font,
            TEXT_SECONDARY,
        );
    }
    let person_label;
    let (bot_labels, waiting) = match view.controller {
        CardController::Own => (["", ""], false),
        CardController::Bot { difficulty, stance } => {
            ([difficulty_name(difficulty), stance_name(stance)], false)
        }
        CardController::Open => (["Open", ""], true),
        CardController::Reserved { name } => {
            person_label = format!("Was {name}");
            ([person_label.as_str(), ""], true)
        }
        CardController::Person { name, host } => {
            person_label = if host {
                format!("{name} (host)")
            } else {
                name.to_owned()
            };
            ([person_label.as_str(), ""], false)
        }
    };
    for (cell, control, label) in [
        (Cell::Difficulty, card.difficulty, bot_labels[0]),
        (Cell::Stance, card.stance, bot_labels[1]),
    ] {
        let Some(control) = control.filter(|_| !label.is_empty()) else {
            continue;
        };
        let on_cell = view.focus == Some(cell);
        draw_rectangle(control.x, control.y, control.w, control.h, CARD_IDLE);
        draw_rectangle_lines(
            control.x,
            control.y,
            control.w,
            control.h,
            if on_cell {
                Stroke::Focus
            } else {
                Stroke::Hairline
            }
            .at(ui),
            if on_cell { TEXT_TITLE } else { accent },
        );
        let font = crate::typography::fit(label, Type::Small.at(ui), control.w - 6.0, 8.0);
        let dims = measure_text(label, None, numeric::font_size(font), 1.0);
        // An empty open chair reads quieter than a bot or a player.
        draw_text(
            label,
            control.x + (control.w - dims.width) * 0.5,
            control.y + control.h * 0.5 + font * 0.35,
            font,
            if waiting { TEXT_SECONDARY } else { accent },
        );
    }
    // The boxed team chip; the cursor's cell wears the accent.
    let label = view.team.as_str();
    let chip = card.team;
    let on_cell = view.focus == Some(Cell::Team);
    draw_rectangle(chip.x, chip.y, chip.w, chip.h, CHIP);
    draw_rectangle_lines(
        chip.x,
        chip.y,
        chip.w,
        chip.h,
        if on_cell {
            Stroke::Focus
        } else {
            Stroke::Hairline
        }
        .at(ui),
        if on_cell { TEXT_TITLE } else { BORDER_STRONG },
    );
    // The label fits its chip: squeezed cards shrink the type
    // instead of spilling text across neighbors.
    let font = crate::typography::fit(label, Type::Small.at(ui), chip.w - 6.0, 8.0);
    let ldims = measure_text(label, None, numeric::font_size(font), 1.0);
    draw_text(
        label,
        chip.x + (chip.w - ldims.width) * 0.5,
        chip.y + chip.h * 0.5 + font * 0.35,
        font,
        if on_cell {
            TEXT_PRIMARY
        } else {
            TEXT_SECONDARY
        },
    );
    // The seat-zone cell cursor: a soft inner line under
    // the name, so "Enter takes this chair" reads.
    if view.focus == Some(Cell::Seat) && !is_you {
        let zone = card.seat;
        draw_rectangle(
            zone.x + 44.0 * ui,
            cy + name_font * 0.55,
            measure_text(display, None, numeric::font_size(name_font), 1.0).width,
            1.5,
            TEXT_TITLE,
        );
    }
}

impl Wizard {
    /// Opens at the map grid, the remembered map re-highlighted.
    pub fn open(draft: &NewMatchDraft) -> Self {
        let entries = discover_scenarios();
        let mut browser = Browser::new();
        if draft.scenario.is_some() {
            browser.select_path(&entries, draft.scenario_path.as_deref());
        }
        Self {
            step: Step::Map,
            entries,
            browser,
            setup_sel: 0,
            setup_cell: Cell::Seat,
            setup_press: Press::default(),
            back: crate::button::BackButton::default(),
            setup_page: 0,
            lan: None,
            on_switch: false,
            moving: None,
            overlay: None,
            notice: None,
            saved_name: None,
            lan_bind: format!("0.0.0.0:{}", aftermine_net::DEFAULT_PORT),
            resume: None,
            leaving: false,
        }
    }

    /// Opens setup on a saved LAN match and hosts its lobby, each joined
    /// player's seat reserved. The draft stands in for New Match's until
    /// the resume ends.
    pub(crate) fn resume(save: LanSave, draft: &mut NewMatchDraft) -> Self {
        let bind = format!("0.0.0.0:{}", aftermine_net::DEFAULT_PORT);
        Self::resume_on(save, draft, &bind)
    }

    fn resume_on(save: LanSave, draft: &mut NewMatchDraft, bind: &str) -> Self {
        let mut wizard = Self::open(draft);
        bind.clone_into(&mut wizard.lan_bind);
        let previous = std::mem::take(draft);
        let scenario = save.scenario().clone();
        draft.player_name.clone_from(&previous.player_name);
        draft.seats = authored_seat_plans(&scenario);
        draft.seat_choice = usize::from(save.host().0);
        draft.scenario = Some(Box::new(scenario));
        let resume = Resume { save, previous };
        // The saved players' seats wait for them even while the lobby is
        // not open.
        for (seat, plan) in draft.seats.iter_mut().enumerate() {
            plan.open = resume.reserved(seat);
        }
        wizard.resume = Some(Box::new(resume));
        wizard.goto(Step::Setup, draft);
        wizard.open_lan(draft);
        wizard
    }

    /// Whether this setup screen resumes a saved LAN match.
    pub(crate) fn resuming(&self) -> bool {
        self.resume.is_some()
    }

    /// Ends a resume, giving back New Match's draft.
    pub(crate) fn finish_resume(&mut self, draft: &mut NewMatchDraft) {
        if let Some(resume) = self.resume.take() {
            *draft = resume.previous;
        }
    }

    /// The saved match as the host resumes it alone, every reserved seat
    /// handed to its bot.
    pub(crate) fn resume_alone(&self, draft: &NewMatchDraft) -> Result<Game> {
        let resume = self.resume.as_deref().context("not resuming a match")?;
        resume.save.local_game(&resume.replaced(draft))
    }

    /// The entry for the draft's picked map, with its stable index —
    /// what the setup screen keys its preview by.
    pub fn picked_entry(&self, draft: &NewMatchDraft) -> Option<(usize, &ScenarioEntry)> {
        self.entries
            .iter()
            .position(|e| e.path == draft.scenario_path)
            .and_then(|i| self.entries.get(i).map(|e| (i, e)))
    }

    fn goto(&mut self, step: Step, draft: &mut NewMatchDraft) {
        self.step = step;
        self.back.cancel();
        self.moving = None;
        self.on_switch = false;
        self.notice = None;
        match step {
            Step::Map => {
                // The map stays fixed while a lobby is open, so leaving
                // setup closes it.
                self.close_lan(draft);
                self.entries = discover_scenarios();
                self.browser
                    .select_path(&self.entries, draft.scenario_path.as_deref());
            }
            Step::Setup => {
                // Start preselected: Enter-Enter from the grid plays
                // the map as authored.
                self.setup_sel = draft.seats.len();
                self.setup_page = self.setup_sel / COMPACT_PAGE_ITEMS;
                self.setup_cell = Cell::Seat;
                self.setup_press.cancel();
            }
        }
    }

    /// Applies a frame's events. Windowless: the screens navigate, the
    /// draft records answers, and the return says whether the caller
    /// should stay, go Home, or launch the match.
    pub fn update(
        &mut self,
        events: &[RawEvent],
        mouse: &mut Vec2,
        draft: &mut NewMatchDraft,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
    ) -> Result<Out> {
        // A running start launches the setup it froze, so nothing may
        // change the setup until the start succeeds or fails; Back and
        // Escape call it off.
        if self.lan.as_ref().is_some_and(|lan| lan.starting.is_some()) {
            let (back, events) = self.back.route(events);
            let escaped = events
                .iter()
                .any(|event| matches!(event, RawEvent::KeyDown { key: Key::Escape }));
            if (back || escaped)
                && let Some(lan) = self.lan.as_mut()
            {
                sounds.push((SoundKind::Click, None));
                lan.lobby.cancel_start();
                lan.starting = None;
            }
            return Ok(Out::Stay);
        }
        if self.overlay.is_some() {
            self.update_overlay(events, mouse, draft, sounds);
            return Ok(if std::mem::take(&mut self.leaving) {
                Out::Home
            } else {
                Out::Stay
            });
        }
        // The corner Back button sees the pointer first; the step only
        // gets the events it leaves alone.
        let (back, events) = self.back.route(events);
        if back {
            sounds.push((SoundKind::Click, None));
            match self.step {
                Step::Map => return Ok(Out::Home),
                Step::Setup => {
                    if self.back_from_setup(draft) {
                        return Ok(Out::Home);
                    }
                    return Ok(Out::Stay);
                }
            }
        }
        match self.step {
            Step::Map => match self.browser.handle(&self.entries, &events, mouse) {
                BrowserOut::Back => return Ok(Out::Home),
                BrowserOut::Pick(entry) => {
                    sounds.push((SoundKind::Click, None));
                    let scenario = match &self.entries[entry].path {
                        Some(path) => Scenario::load(path)
                            .with_context(|| format!("loading {}", path.display()))?,
                        None => Scenario::skirmish(),
                    };
                    draft.set_scenario(scenario, self.entries[entry].path.clone());
                    self.goto(Step::Setup, draft);
                }
                BrowserOut::Stay => {}
            },
            Step::Setup => {
                if let Some(out) = self.update_setup(&events, mouse, draft, sounds) {
                    return Ok(out);
                }
            }
        }
        Ok(Out::Stay)
    }

    /// The setup screen's input: Up/Down walk the seat cards and the
    /// Start button; Left/Right walk the seat, difficulty, stance, and
    /// team cells; Enter takes the seat or cycles the
    /// control under the cursor; clicks hit each zone directly.
    fn update_setup(
        &mut self,
        events: &[RawEvent],
        mouse: &mut Vec2,
        draft: &mut NewMatchDraft,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
    ) -> Option<Out> {
        let Some(scenario) = draft.scenario.as_deref() else {
            self.goto(Step::Map, draft);
            return None;
        };
        let order = seat_display_order(scenario);
        let start_index = order.len();
        let view = crate::render::viewport();
        let ui = crate::render::ui_scale();
        self.setup_page = self.setup_sel.min(start_index) / COMPACT_PAGE_ITEMS;
        let open = self.open_flags(draft);
        let lan_on = self.lan.is_some();
        let layout = setup_layout_page(
            scenario,
            draft.seat_choice,
            &open,
            view,
            ui,
            self.setup_page,
        );
        self.setup_page = layout.page;
        // A resume fixes the map's bots and teams and the host's seat: only
        // the reserved seats change hands.
        let resuming = self.resume.is_some();
        let movable: Vec<bool> = (0..draft.seats.len())
            .map(|seat| {
                self.resume
                    .as_deref()
                    .is_none_or(|resume| resume.reserved(seat))
            })
            .collect();
        let cell_live = |row: usize, cell: Cell| -> bool {
            row < start_index
                && match cell {
                    Cell::Seat => movable[order[row]],
                    Cell::Team => !resuming,
                    Cell::Difficulty => order[row] != draft.seat_choice && movable[order[row]],
                    Cell::Stance => {
                        order[row] != draft.seat_choice && !open[order[row]] && movable[order[row]]
                    }
                }
        };
        let zone_at = |p: Vec2, touch: bool| -> Option<SetupZone> {
            for (index, half) in layout.switch.iter().enumerate() {
                if half.contains(p) {
                    return Some(SetupZone::Switch(index == 1));
                }
            }
            for (row, card) in layout.cards.iter().enumerate() {
                let Some(card) = card else {
                    continue;
                };
                for cell in Cell::ALL.into_iter().filter(|cell| cell_live(row, *cell)) {
                    let rect = if touch {
                        card.touch_cell(cell)
                    } else {
                        card.cell(cell)
                    };
                    if rect.is_some_and(|rect| rect.contains(p)) {
                        return Some(SetupZone::Card { row, cell });
                    }
                }
            }
            if layout.start.is_some_and(|rect| rect.contains(p)) {
                return Some(SetupZone::Start);
            }
            if layout.page_prev.is_some_and(|rect| rect.contains(p)) {
                return Some(SetupZone::PrevPage);
            }
            layout
                .page_next
                .is_some_and(|rect| rect.contains(p))
                .then_some(SetupZone::NextPage)
        };
        let mut activate: Option<SetupZone> = None;
        for event in events {
            if let Fed::Activated(zone) = self.setup_press.feed(event, zone_at) {
                if let Some(p) = crate::press::position(event) {
                    *mouse = p;
                }
                self.on_switch = matches!(zone, SetupZone::Switch(_));
                match zone {
                    SetupZone::Card { row, cell } => {
                        self.setup_sel = row;
                        if cell_live(row, cell) {
                            self.setup_cell = cell;
                        }
                    }
                    SetupZone::Start => self.setup_sel = start_index,
                    SetupZone::Switch(_) | SetupZone::PrevPage | SetupZone::NextPage => {}
                }
                activate = Some(zone);
                break;
            }
            match *event {
                RawEvent::KeyDown { key: Key::Escape } => {
                    return self.back_from_setup(draft).then_some(Out::Home);
                }
                RawEvent::KeyDown { key: Key::Enter } => {
                    activate = Some(if self.on_switch {
                        SetupZone::Switch(!lan_on)
                    } else if self.setup_sel >= start_index {
                        SetupZone::Start
                    } else {
                        // The sticky column falls back to the seat zone
                        // on rows where its cell is dead.
                        let cell = if cell_live(self.setup_sel, self.setup_cell) {
                            self.setup_cell
                        } else {
                            Cell::Seat
                        };
                        SetupZone::Card {
                            row: self.setup_sel,
                            cell,
                        }
                    });
                    break;
                }
                RawEvent::KeyDown { .. } => {
                    let Some(nav) = Nav::decode(event) else {
                        continue;
                    };
                    // The switch is the line above the first card; Home,
                    // End, and paging stay on the cards and Start.
                    let target = if matches!(nav, Nav::Up | Nav::Down) {
                        let line = if self.on_switch {
                            0
                        } else {
                            self.setup_sel + 1
                        };
                        step_line(start_index + 2, line, nav, Axis::Vertical, 1, |_| true)
                    } else {
                        step_line(
                            start_index + 1,
                            self.setup_sel,
                            nav,
                            Axis::Vertical,
                            COMPACT_PAGE_ITEMS,
                            |_| true,
                        )
                        .map(|row| row + 1)
                    };
                    if let Some(line) = target {
                        self.on_switch = line == 0;
                        if let Some(row) = line.checked_sub(1) {
                            self.setup_sel = row;
                            self.setup_page = row / COMPACT_PAGE_ITEMS;
                        }
                    } else if !self.on_switch
                        && let Some(cell) = step_line(
                            Cell::ALL.len(),
                            self.setup_cell.index(),
                            nav,
                            Axis::Horizontal,
                            1,
                            |cell| cell_live(self.setup_sel, Cell::ALL[cell]),
                        )
                    {
                        self.setup_cell = Cell::ALL[cell];
                    }
                }
                RawEvent::MouseMove { x, y } => *mouse = vec2(x, y),
                RawEvent::TouchDown { id, x, y } | RawEvent::TouchMove { id, x, y }
                    if self.setup_press.owns(id) =>
                {
                    *mouse = vec2(x, y);
                }
                _ => {}
            }
        }
        let zone = activate?;
        self.setup_press.cancel();
        match zone {
            SetupZone::PrevPage => {
                self.setup_page = self.setup_page.saturating_sub(1);
                self.setup_sel = self.setup_page * COMPACT_PAGE_ITEMS;
                self.setup_cell = Cell::Seat;
                sounds.push((SoundKind::Click, None));
            }
            SetupZone::NextPage => {
                self.setup_page = (self.setup_page + 1).min(layout.page_count - 1);
                self.setup_sel = (self.setup_page * COMPACT_PAGE_ITEMS).min(start_index);
                self.setup_cell = Cell::Seat;
                sounds.push((SoundKind::Click, None));
            }
            SetupZone::Switch(_) if resuming => {
                // A saved LAN match resumes only with its players; the
                // switch can only retry a lobby that failed to open.
                if lan_on {
                    sounds.push((SoundKind::Denied, None));
                } else {
                    sounds.push((SoundKind::Click, None));
                    self.open_lan(draft);
                }
            }
            SetupZone::Switch(lan) => {
                sounds.push((SoundKind::Click, None));
                if lan && !lan_on {
                    self.open_lan(draft);
                } else if !lan && lan_on {
                    self.leave_lan(draft, false);
                }
            }
            SetupZone::Start => {
                // Refuse an all-one-team draft here so the reason shows
                // inline instead of a failed-launch notice.
                if draft_one_team(draft) {
                    sounds.push((SoundKind::Denied, None));
                    return None;
                }
                if resuming && self.lan.is_none() {
                    sounds.push((SoundKind::Denied, None));
                    self.notice = Some("Players rejoin through the LAN lobby".to_owned());
                    return None;
                }
                if let Some(lan) = &self.lan {
                    let waiting = order
                        .iter()
                        .copied()
                        .find(|&seat| open[seat] && lan.members[seat].is_none());
                    if let Some(seat) = waiting {
                        sounds.push((SoundKind::Denied, None));
                        self.notice = Some(format!("Seat {} is still open", seat + 1));
                        return None;
                    }
                    if !lan.has_members() {
                        // Nobody joined: a plain local match.
                        self.close_lan(draft);
                    }
                }
                sounds.push((SoundKind::Click, None));
                return Some(Out::Launch);
            }
            SetupZone::Card { row, cell } => {
                sounds.push((SoundKind::Click, None));
                self.notice = None;
                let seat = order[row];
                let member = self.members().get(seat).copied().flatten();
                let plan = &mut draft.seats[seat];
                match cell {
                    Cell::Seat => self.seat_pressed(draft, seat),
                    Cell::Difficulty => match member {
                        Some(member) => {
                            let name = self.lan.as_ref().and_then(|lan| lan.lobby.name(member));
                            self.overlay = Some(confirm_overlay(Confirm::Kick { member }, name));
                        }
                        None => cycle_controller(plan, lan_on || resuming),
                    },
                    Cell::Stance => plan.stance = cycle_stance(plan.stance, 1),
                    // FFA, then every team up to the seat count
                    // (start_index is the full roster's length),
                    // wrapping back to FFA.
                    Cell::Team => plan.team_choice = (plan.team_choice + 1) % (start_index + 1),
                }
            }
        }
        None
    }

    /// Draws the setup screen: team-grouped seat cards, the Start
    /// button, and the live map with every chair marked.
    pub fn draw_setup(&self, draft: &NewMatchDraft, previews: &mut PreviewCache, mouse: Vec2) {
        let Some(scenario) = draft.scenario.as_deref() else {
            return;
        };
        let view = crate::render::viewport();
        let ui = crate::render::ui_scale();
        let open = self.open_flags(draft);
        let layout = setup_layout_page(
            scenario,
            draft.seat_choice,
            &open,
            view,
            ui,
            self.setup_page,
        );
        let order = seat_display_order(scenario);
        let hosting = self.hosting_line(draft);

        let title = "MATCH SETUP";
        let compact = layout.page_count > 1;
        let tsize = if compact {
            Type::Title.at(ui).min(42.0)
        } else {
            Type::Title.at(ui)
        };
        let tdims = measure_text(title, None, numeric::font_size(tsize), 1.0);
        draw_text(
            title,
            (view.x - tdims.width) * 0.5,
            if compact { 58.0 } else { 64.0 * ui },
            tsize,
            TEXT_TITLE,
        );
        if !compact {
            let sub = if let Some(hosting) = &hosting {
                format!("{} - {hosting}", scenario.name)
            } else if crate::hints::showing() {
                format!("{} - pick your seat, opponents, and teams", scenario.name)
            } else {
                scenario.name.clone()
            };
            let sdims = measure_text(&sub, None, numeric::font_size(Type::Label.at(ui)), 1.0);
            draw_text(
                &sub,
                (view.x - sdims.width) * 0.5,
                92.0 * ui,
                Type::Label.at(ui),
                TEXT_SECONDARY,
            );
        }

        self.draw_switch(&layout, ui);
        if compact && let Some(hosting) = &hosting {
            let switch = layout.switch[1];
            let x = switch.x + switch.w + 14.0 * ui;
            let room = layout.preview.x - x - 14.0 * ui;
            let size = crate::typography::fit(hosting, Type::Small.at(ui), room, 8.0);
            draw_text(
                hosting,
                x,
                switch.y + switch.h * 0.5 + size * 0.35,
                size,
                TEXT_SECONDARY,
            );
        }
        draw_team_headings(&layout, ui);
        let styles = draft_styles(scenario, draft);
        for (pos, card) in layout.cards.iter().enumerate() {
            let Some(card) = card else {
                continue;
            };
            let seat = order[pos];
            let plan = draft.seats[seat];
            let controller = if seat == draft.seat_choice {
                CardController::Own
            } else if open[seat] {
                match self.lan.as_ref().and_then(|lan| lan.name_at(seat)) {
                    Some(name) => CardController::Person {
                        name: name.as_str(),
                        host: false,
                    },
                    None => match &self.resume {
                        Some(resume) => CardController::Reserved {
                            name: resume.saved_name(seat),
                        },
                        None => CardController::Open,
                    },
                }
            } else {
                CardController::Bot {
                    difficulty: plan.difficulty,
                    stance: plan.stance,
                }
            };
            let view = CardView {
                seat,
                accent: styles.get(aftermine_sim::PlayerId::from_index(seat)),
                name: scenario.players[seat].name.as_str(),
                controller,
                team: team_chip_label(plan.team_choice),
                focus: (self.setup_sel == pos && !self.on_switch).then_some(self.setup_cell),
                moving: self.moving == Some(seat),
            };
            draw_seat_card(card, &view, ui);
        }
        // Start button, disabled for an all-one-team draft.
        let one_team = draft_one_team(draft);
        let start_selected = self.setup_sel == layout.cards.len();
        if let Some(start) = layout.start {
            draw_rectangle(start.x, start.y, start.w, start.h, SURFACE_MENU);
            draw_rectangle_lines(
                start.x,
                start.y,
                start.w,
                start.h,
                if start_selected {
                    Stroke::Heavy
                } else {
                    Stroke::Edge
                }
                .at(ui),
                if one_team {
                    BORDER_STRONG
                } else if start_selected {
                    TEXT_TITLE
                } else {
                    TEXT_SECONDARY
                },
            );
            let label = "Start match";
            let ldims = measure_text(label, None, numeric::font_size(Type::Heading.at(ui)), 1.0);
            draw_text(
                label,
                start.x + (start.w - ldims.width) * 0.5,
                start.y + start.h * 0.66,
                Type::Heading.at(ui),
                if !one_team && start_selected {
                    TEXT_PRIMARY
                } else {
                    TEXT_SECONDARY
                },
            );
        }

        for (rect, label) in [
            (
                layout.page_prev,
                format!("PREV  {}/{}", layout.page + 1, layout.page_count),
            ),
            (
                layout.page_next,
                format!("{}/{}  NEXT", layout.page + 1, layout.page_count),
            ),
        ] {
            let Some(rect) = rect else {
                continue;
            };
            draw_rectangle(rect.x, rect.y, rect.w, rect.h, SURFACE_MENU);
            draw_rectangle_lines(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                Stroke::Edge.at(ui),
                TEXT_SECONDARY,
            );
            let size = crate::typography::fit(&label, Type::Body.at(ui), rect.w - 8.0, 8.0);
            let dims = measure_text(&label, None, numeric::font_size(size), 1.0);
            draw_text(
                &label,
                rect.x + (rect.w - dims.width) * 0.5,
                rect.y + rect.h * 0.5 + size * 0.35,
                size,
                TEXT_SECONDARY,
            );
        }

        // A resumed match's map comes from its save, not from a file.
        let preview = if self.resume.is_some() {
            previews.get_map(scenario, layout.preview).map(|preview| {
                let theme = scenario
                    .meta
                    .as_ref()
                    .map_or("", |meta| meta.theme.as_str());
                (preview, theme)
            })
        } else {
            self.picked_entry(draft).and_then(|(_, entry)| {
                previews
                    .get(entry, layout.preview)
                    .map(|preview| (preview, entry.theme.as_str()))
            })
        };
        if let Some((preview, theme)) = preview {
            let drawn = draw_preview(&preview, theme, layout.preview, ui);
            let focus = (self.setup_sel < order.len()).then(|| order[self.setup_sel]);
            draw_seat_markers(scenario, &styles, drawn, draft.seat_choice, focus, ui);
        }

        if !compact || self.notice.is_some() {
            let hands = crate::platform::hands();
            let (hint, warning) = if let Some(notice) = &self.notice {
                (notice.clone(), true)
            } else if self.moving.is_some() {
                (crate::menu::binding_hint(moving_hint(hands)), false)
            } else {
                let hint = setup_hint(
                    one_team,
                    self.setup_sel == order.len(),
                    self.setup_cell,
                    hands,
                );
                (crate::menu::binding_hint(hint), one_team)
            };
            let size = crate::typography::fit(&hint, Type::Body.at(ui), view.x - 32.0 * ui, 10.0);
            let hdims = measure_text(&hint, None, numeric::font_size(size), 1.0);
            draw_text(
                &hint,
                (view.x - hdims.width) * 0.5,
                view.y - 20.0 * ui,
                size,
                // Warnings are information, not coaching.
                if warning {
                    TEXT_DANGER
                } else {
                    crate::hints::fade(TEXT_SECONDARY)
                },
            );
        }
        self.draw_overlay(mouse);
    }

    /// The LOCAL | LAN switch: the active half filled, the cursor's outline
    /// when it sits on the switch.
    fn draw_switch(&self, layout: &SetupLayout, ui: f32) {
        let lan = self.lan.is_some();
        for (index, (half, label)) in layout.switch.iter().zip(["LOCAL", "LAN"]).enumerate() {
            let active = (index == 1) == lan;
            draw_rectangle(
                half.x,
                half.y,
                half.w,
                half.h,
                if active { CARD_IDLE } else { SURFACE_MENU },
            );
            draw_rectangle_lines(
                half.x,
                half.y,
                half.w,
                half.h,
                Stroke::Hairline.at(ui),
                if active { TEXT_ACCENT } else { BORDER_STRONG },
            );
            let size = Type::Small.at(ui);
            let dims = measure_text(label, None, numeric::font_size(size), 1.0);
            draw_text(
                label,
                half.x + (half.w - dims.width) * 0.5,
                half.y + half.h * 0.5 + size * 0.35,
                size,
                if active { TEXT_PRIMARY } else { TEXT_SECONDARY },
            );
        }
        if self.on_switch {
            let [left, right] = layout.switch;
            draw_rectangle_lines(
                left.x,
                left.y,
                right.x + right.w - left.x,
                left.h,
                Stroke::Focus.at(ui),
                TEXT_TITLE,
            );
        }
    }

    /// Where the lobby stands, for the line under the title: its address
    /// and how many players are here; `None` while local.
    fn hosting_line(&self, draft: &NewMatchDraft) -> Option<String> {
        let lan = self.lan.as_ref()?;
        if lan.starting.is_some() {
            return Some("starting the match...".to_owned());
        }
        let open = self.open_flags(draft);
        let here = 1 + lan.members.iter().flatten().count();
        let waiting = (0..open.len())
            .filter(|&seat| open[seat] && lan.members[seat].is_none())
            .count();
        Some(format!(
            "hosting on {} - {here} of {} players here",
            shown_address(lan.lobby.address()),
            here + waiting
        ))
    }

    /// Draws the name prompt or a confirmation over a veil.
    fn draw_overlay(&self, mouse: Vec2) {
        let Some(overlay) = &self.overlay else {
            return;
        };
        let view = crate::render::viewport();
        draw_rectangle(0.0, 0.0, view.x, view.y, crate::theme::VEIL);
        match overlay {
            Overlay::Naming(form) => form.draw("", TEXT_SECONDARY, mouse),
            Overlay::Confirm { ask, menu } => {
                let line = match ask {
                    Confirm::Kick { .. } => String::new(),
                    Confirm::Close { .. } => {
                        let players = self
                            .lan
                            .as_ref()
                            .map_or(0, |lan| lan.members.iter().flatten().count());
                        match players {
                            1 => "1 player will be disconnected".to_owned(),
                            n => format!("{n} players will be disconnected"),
                        }
                    }
                };
                menu.draw(&line);
            }
        }
    }

    /// The debug protocol's stable mode name for the current step, which
    /// automation scripts depend on.
    pub fn mode_name(&self) -> &'static str {
        match (&self.overlay, self.step) {
            (Some(Overlay::Naming(_)), _) => "player_name",
            (Some(Overlay::Confirm { .. }), _) => "confirm_lobby",
            (None, Step::Map) => "main_menu",
            (None, Step::Setup) => "match_setup",
        }
    }

    /// The (title, items, selected) surface `QueryUi` reports — the
    /// custom screens speak the same protocol the row menus do. Setup
    /// lists the seats, Start, and last the LOCAL | LAN switch.
    pub fn ui_surface(&self, draft: &NewMatchDraft) -> (String, Vec<String>, usize) {
        match (&self.overlay, self.step) {
            (Some(Overlay::Naming(form)), _) => {
                let view = form.menu(|_| ()).view();
                (view.title, view.items, view.selected)
            }
            (Some(Overlay::Confirm { menu, .. }), _) => {
                let view = menu.view();
                (view.title, view.items, view.selected)
            }
            (None, Step::Map) => (
                "OXIDE".to_string(),
                self.entries.iter().map(|e| e.label.clone()).collect(),
                self.browser.selected,
            ),
            (None, Step::Setup) => {
                let open = self.open_flags(draft);
                let mut items: Vec<String> = draft
                    .scenario
                    .as_deref()
                    .map(|sc| {
                        seat_display_order(sc)
                            .into_iter()
                            .map(|seat| self.seat_row(sc, draft, &open, seat))
                            .collect()
                    })
                    .unwrap_or_default();
                items.push("Start match".to_string());
                items.push(format!(
                    "Mode: {}",
                    if self.lan.is_some() { "LAN" } else { "Local" }
                ));
                let selected = if self.on_switch {
                    items.len() - 1
                } else {
                    self.setup_sel
                };
                ("MATCH SETUP".to_string(), items, selected)
            }
        }
    }

    /// One seat's protocol row: who plays it and its team.
    fn seat_row(
        &self,
        scenario: &Scenario,
        draft: &NewMatchDraft,
        open: &[bool],
        seat: usize,
    ) -> String {
        let name = &scenario.players[seat].name;
        let plan = draft.seats[seat];
        let team = team_chip_label(plan.team_choice);
        let who = if seat == draft.seat_choice {
            "you".to_owned()
        } else if open[seat] {
            match (
                self.lan.as_ref().and_then(|lan| lan.name_at(seat)),
                &self.resume,
            ) {
                (Some(player), _) => format!("player {player}"),
                (None, Some(resume)) => format!("was {}", resume.saved_name(seat)),
                (None, None) => "open".to_owned(),
            }
        } else {
            return format!(
                "{}. {} | Difficulty {} | Stance {} | {}",
                seat + 1,
                name,
                difficulty_name(plan.difficulty),
                stance_name(plan.stance),
                team
            );
        };
        let moving = if self.moving == Some(seat) {
            " moving"
        } else {
            ""
        };
        format!("{}. {} ({who}{moving}) | {team}", seat + 1, name)
    }

    /// The half-open item-index range actually on screen — `QueryUi`'s
    /// `visible_range`, computed from the same injected viewport the
    /// frame drew with. The map grid reads the browser's real layout
    /// (visible cards are a contiguous run of entry indices; a window
    /// showing none reports `[0, 0]`). Compact setup reports only the
    /// current nonoverlapping page; full-height setup reports every row.
    pub fn ui_visible_range(&self, draft: &NewMatchDraft, view: Vec2, ui: f32) -> [usize; 2] {
        if self.overlay.is_some() {
            return [0, self.ui_surface(draft).1.len()];
        }
        match self.step {
            Step::Map => {
                let layout = self.browser.layout(&self.entries, view, ui);
                match (layout.cards.first(), layout.cards.last()) {
                    (Some(&(first, _)), Some(&(last, _))) => [first, last + 1],
                    _ => [0, 0],
                }
            }
            Step::Setup => draft.scenario.as_deref().map_or([0, 0], |scenario| {
                let open = self.open_flags(draft);
                setup_layout_page(
                    scenario,
                    draft.seat_choice,
                    &open,
                    view,
                    ui,
                    self.setup_page,
                )
                .visible_range
            }),
        }
    }

    /// The pointer's current highlight for the protocol surface: the
    /// grid's hovered card on the map step (the UX battery's row
    /// discovery sweeps this), nothing on setup.
    pub fn ui_hover(&self) -> Option<usize> {
        match self.step {
            Step::Map => self.browser.hover,
            Step::Setup => None,
        }
    }
}

fn name_check(name: &str) -> Result<(), &'static str> {
    PlayerName::new(name)
        .map(drop)
        .map_err(aftermine_net::NameError::reason)
}

/// The first chair, in display order, a joining member takes: an open one
/// nobody holds, or else a bot's; `None` when every chair holds a person.
fn seat_for_joiner(
    draft: &NewMatchDraft,
    members: &[Option<MemberId>],
    order: &[usize],
) -> Option<usize> {
    let free = |seat: usize| seat != draft.seat_choice && members[seat].is_none();
    order
        .iter()
        .copied()
        .find(|&seat| free(seat) && draft.seats[seat].open)
        .or_else(|| {
            order
                .iter()
                .copied()
                .find(|&seat| free(seat) && !draft.seats[seat].open)
        })
}

/// A confirmation for `ask`; Cancel sits preselected.
fn confirm_overlay(ask: Confirm, name: Option<&PlayerName>) -> Overlay {
    let (title, yes) = match ask {
        Confirm::Kick { .. } => (
            format!(
                "REMOVE {}?",
                name.map_or("THIS PLAYER".to_owned(), |name| name
                    .as_str()
                    .to_uppercase())
            ),
            "Remove",
        ),
        Confirm::Close { .. } => ("CLOSE THE LOBBY?".to_owned(), "Close the lobby"),
    };
    let mut menu = Menu::rows(
        title,
        [(Label::from("Cancel"), false), (Label::from(yes), true)],
    );
    menu.select(0);
    Overlay::Confirm { ask, menu }
}

impl Wizard {
    /// Whether the chairs (by seat) wait for LAN players.
    fn open_flags(&self, draft: &NewMatchDraft) -> Vec<bool> {
        draft
            .seats
            .iter()
            .enumerate()
            .map(|(seat, plan)| {
                (self.lan.is_some() || self.resume.is_some())
                    && plan.open
                    && seat != draft.seat_choice
            })
            .collect()
    }

    /// The lobby's members, one slot per seat; empty while local.
    fn members(&self) -> &[Option<MemberId>] {
        self.lan.as_ref().map_or(&[], |lan| lan.members.as_slice())
    }

    /// Opens the LAN lobby, asking for the player's name first if none is
    /// saved. Every chair but the player's opens for joiners.
    fn open_lan(&mut self, draft: &mut NewMatchDraft) {
        let Some(scenario) = draft.scenario.as_deref() else {
            return;
        };
        if scenario.players.is_empty() {
            self.notice = Some("This map has no seats".to_owned());
            return;
        }
        if draft.player_name.is_none() {
            let field =
                Field::new("", PlayerName::MAX_CHARS, PlayerName::accepts).validated(name_check);
            self.overlay = Some(Overlay::Naming(Form::new(
                "YOUR NAME",
                "HOST",
                vec![field],
                0,
            )));
            return;
        }
        match HostLobby::open(&self.lan_bind, &crate::build_identity().revision, scenario) {
            Ok(lobby) => {
                self.lan = Some(Box::new(LanHost {
                    lobby,
                    members: vec![None; scenario.players.len()],
                    starting: None,
                }));
                let host = draft.seat_choice;
                let resume = self.resume.as_deref();
                for (seat, plan) in draft.seats.iter_mut().enumerate() {
                    plan.open = resume.map_or(seat != host, |resume| resume.reserved(seat));
                }
                self.notice = None;
            }
            Err(error) => self.notice = Some(format!("Cannot host: {error:#}")),
        }
    }

    /// Closes the LAN lobby, dropping its players; open chairs go back to
    /// their bots.
    fn close_lan(&mut self, draft: &mut NewMatchDraft) {
        self.lan = None;
        self.moving = None;
        for plan in &mut draft.seats {
            plan.open = false;
        }
    }

    /// Asks before closing a lobby with players in it, and closes an empty
    /// one at once; returns whether it closed.
    fn leave_lan(&mut self, draft: &mut NewMatchDraft, to_map: bool) -> bool {
        if self.lan.as_ref().is_some_and(|lan| lan.has_members()) {
            self.overlay = Some(confirm_overlay(Confirm::Close { to_map }, None));
            return false;
        }
        self.close_lan(draft);
        true
    }

    /// Runs the LAN lobby for a frame: seats joiners, frees leavers' seats,
    /// shows everyone the room, and advances a running start. Returns the
    /// match once every machine is at tick zero.
    pub fn poll_lan(&mut self, draft: &mut NewMatchDraft, now: Duration) -> Option<(Game, Link)> {
        let lan = self.lan.as_mut()?;
        let scenario = draft.scenario.as_deref()?;
        let order = seat_display_order(scenario);
        loop {
            for event in lan.lobby.poll(now, lan.starting.is_some()) {
                match event {
                    LobbyEvent::Joined {
                        member,
                        name,
                        install,
                    } => {
                        let seat = match self.resume.as_deref() {
                            Some(resume) => resume.seat_for(
                                draft,
                                &lan.members,
                                &lan.lobby,
                                (&name, install),
                                &order,
                            ),
                            None => seat_for_joiner(draft, &lan.members, &order),
                        };
                        match seat {
                            Some(seat) => {
                                draft.seats[seat].open = true;
                                lan.members[seat] = Some(member);
                            }
                            None => lan.lobby.refuse(member),
                        }
                    }
                    LobbyEvent::Left { member, .. } => {
                        for slot in &mut lan.members {
                            if *slot == Some(member) {
                                *slot = None;
                            }
                        }
                    }
                    // The lobby closes these itself.
                    LobbyEvent::Turned { .. } => {}
                }
            }
            let Some(start) = &lan.starting else {
                break;
            };
            match lan.lobby.poll_start(now, &start.seated) {
                StartPoll::Waiting => return None,
                StartPoll::Started(started) => {
                    let game = lan.starting.take()?.game;
                    self.lan = None;
                    for plan in &mut draft.seats {
                        plan.open = false;
                    }
                    let mut link = Link::host(*started, &game, now);
                    if let Some(resume) = &self.resume {
                        link.remember_installs(resume.save.joined());
                    }
                    self.finish_resume(draft);
                    return Some((*game, link));
                }
                // The failed start dropped its culprits: free their chairs
                // now, before anything can start again this frame.
                StartPoll::Failed(failed) => {
                    lan.starting = None;
                    self.notice = Some(start_failed_notice(failed).to_owned());
                }
            }
        }
        let host = shown_host_name(
            draft.player_name.as_ref(),
            scenario,
            PlayerId(draft.seat_choice.fit::<u8>()),
        );
        let seating = draft_seating(draft, &lan.members);
        let lobby = &lan.lobby;
        let mut room = seating.room(
            &host,
            |member| lobby.name(member).cloned(),
            |seat| team_override(draft.seats[seat].team_choice),
        );
        if let Some(resume) = &self.resume {
            resume.reserve(draft, &lan.members, &mut room);
        }
        lan.lobby
            .publish(&room, |member| seating.seat_of(member), now);
        // Joiners can take any chair but the host's, so the map's seats are
        // the lobby's room.
        let here = 1 + lan.members.iter().flatten().count();
        lan.lobby
            .advertise(&host, &scenario.name, here, draft.seats.len(), now);
        None
    }

    /// Whether Start hosts a LAN match: the lobby holds players.
    pub fn hosting_players(&self) -> bool {
        self.lan.as_ref().is_some_and(|lan| lan.has_members())
    }

    /// The name of the LAN player in each seat, for naming their seats.
    pub fn member_names(&self) -> Vec<Option<PlayerName>> {
        self.lan.as_ref().map_or_else(Vec::new, |lan| {
            (0..lan.members.len())
                .map(|seat| lan.name_at(seat).cloned())
                .collect()
        })
    }

    /// Starts the lobby's match on `scenario`, built from the draft; the
    /// start then runs through [`Wizard::poll_lan`].
    pub fn start_lan(
        &mut self,
        draft: &NewMatchDraft,
        scenario: &Scenario,
        now: Duration,
    ) -> Result<()> {
        let lan = self.lan.as_mut().context("no lobby is open")?;
        let host = PlayerId(draft.seat_choice.fit::<u8>());
        let game = Game::networked(
            scenario.clone(),
            host,
            NetRole::Host,
            crate::render::viewport(),
        )?;
        let seated = lan.seated();
        lan.lobby.start(
            aftermine_net::StartContent::Fresh(scenario),
            host,
            &seated,
            game.state.hash(),
            now,
        );
        lan.starting = Some(LanStart {
            game: Box::new(game),
            seated,
        });
        self.notice = None;
        Ok(())
    }

    /// Starts the lobby's players on the saved match, the reserved seats the
    /// host handed to bots taken over; the start then runs through
    /// [`Wizard::poll_lan`].
    pub fn start_resume(&mut self, draft: &NewMatchDraft, now: Duration) -> Result<()> {
        let resume = self.resume.as_deref().context("not resuming a match")?;
        let lan = self.lan.as_mut().context("no lobby is open")?;
        let (game, pending) = resume.save.host_game(&resume.replaced(draft))?;
        let snapshot = crate::netplay::snapshot::encode(&game.world_snapshot()?)?;
        let seated = lan.seated();
        let seats: Vec<PlayerId> = resume
            .save
            .players()
            .iter()
            .map(|&(seat, _)| seat)
            .filter(|&seat| draft.seats[usize::from(seat.0)].open)
            .chain([game.presentation.human])
            .collect();
        lan.lobby.start(
            aftermine_net::StartContent::Resume {
                seats: &seats,
                tick: game.state.current_tick(),
                snapshot: &snapshot,
                pending: &pending,
            },
            game.presentation.human,
            &seated,
            game.state.hash(),
            now,
        );
        lan.starting = Some(LanStart {
            game: Box::new(game),
            seated,
        });
        self.notice = None;
        Ok(())
    }

    /// A name the player gave the prompt since the last call, to save.
    pub fn take_saved_name(&mut self) -> Option<PlayerName> {
        self.saved_name.take()
    }

    /// The keyboard the name prompt asks for while it is up.
    pub fn keyboard(&self) -> Option<Keyboard> {
        match &self.overlay {
            Some(Overlay::Naming(form)) => Some(form.keyboard()),
            _ => None,
        }
    }

    /// Consumes the name prompt's request to raise the on-screen keyboard.
    pub fn take_keyboard_request(&mut self) -> bool {
        match &mut self.overlay {
            Some(Overlay::Naming(form)) => form.take_keyboard_request(),
            _ => false,
        }
    }

    /// Runs the overlay for a frame.
    fn update_overlay(
        &mut self,
        events: &[RawEvent],
        mouse: &mut Vec2,
        draft: &mut NewMatchDraft,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
    ) {
        match self.overlay.as_mut() {
            Some(Overlay::Naming(form)) => match form.update(events, sounds) {
                Edit::Commit => {
                    let name = PlayerName::new(form.value(0));
                    self.overlay = None;
                    if let Ok(name) = name {
                        draft.player_name = Some(name.clone());
                        self.saved_name = Some(name);
                        self.open_lan(draft);
                    }
                }
                Edit::Cancel => self.overlay = None,
                Edit::Stay => {}
            },
            Some(Overlay::Confirm { ask, menu }) => {
                let ask = *ask;
                let escaped = events
                    .iter()
                    .any(|event| matches!(event, RawEvent::KeyDown { key: Key::Escape }));
                if let Some(picked) = menu.handle(events, mouse) {
                    sounds.push((SoundKind::Click, None));
                    self.overlay = None;
                    if picked.value {
                        self.confirmed(ask, draft);
                    }
                } else if escaped {
                    self.overlay = None;
                }
            }
            None => {}
        }
    }

    fn confirmed(&mut self, ask: Confirm, draft: &mut NewMatchDraft) {
        match ask {
            Confirm::Kick { member } => {
                if let Some(lan) = self.lan.as_mut() {
                    lan.lobby.remove(member);
                    for slot in &mut lan.members {
                        if *slot == Some(member) {
                            *slot = None;
                        }
                    }
                }
            }
            Confirm::Close { .. } if self.resume.is_some() => self.leaving = true,
            Confirm::Close { to_map } => {
                self.close_lan(draft);
                if to_map {
                    self.goto(Step::Map, draft);
                }
            }
        }
    }

    /// Back or Escape on setup: drops a pick first, then leaves for the map
    /// grid, asking first when that would drop players. A resume has no
    /// map grid to go back to: returns whether to leave for Home instead.
    fn back_from_setup(&mut self, draft: &mut NewMatchDraft) -> bool {
        if self.moving.take().is_some() {
            return false;
        }
        if self.resume.is_some() {
            // Leaving drops the players who came back; ask first.
            if self.lan.as_ref().is_some_and(|lan| lan.has_members()) {
                self.overlay = Some(confirm_overlay(Confirm::Close { to_map: true }, None));
                return false;
            }
            return true;
        }
        if self.leave_lan(draft, true) {
            self.goto(Step::Map, draft);
        }
        false
    }

    /// A press on a seat card's name: puts a picked-up occupant here, picks
    /// up the human or a LAN player sitting here, or sits the human here.
    fn seat_pressed(&mut self, draft: &mut NewMatchDraft, seat: usize) {
        let mut none: [Option<MemberId>; 0] = [];
        let members = match self.lan.as_mut() {
            Some(lan) => lan.members.as_mut_slice(),
            None => &mut none,
        };
        match self.moving.take() {
            Some(from) if from == seat => {}
            Some(from) => swap_seats(draft, members, from, seat),
            None => {
                let held = members.get(seat).copied().flatten().is_some();
                if self.resume.is_some() {
                    // The host keeps its seat; players move among the
                    // reserved ones.
                    if held {
                        self.moving = Some(seat);
                    }
                } else if seat == draft.seat_choice || held {
                    self.moving = Some(seat);
                } else {
                    let you = draft.seat_choice;
                    swap_seats(draft, members, you, seat);
                }
            }
        }
    }
}

pub(crate) mod launch;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod lan_tests;

#[cfg(test)]
mod resume_tests;
