//! The rings: a selection's command cards, or the control groups, set
//! around a circle, so a controller picks one by pointing a stick.
//!
//! Each command ring is built from the command panel's own cards, so it
//! never offers what the panel would refuse, and its slices wear their
//! cards' costs, progress and tooltips. A slice may open a ring of its own:
//! a Harvester's Build opens the build categories, a mixed selection's
//! Harvesters or drones open their own cards, and a full ring's More
//! opens what did not fit. The group ring holds one slice per control
//! group, wearing its lead kind and how many it holds. How a ring opens
//! and how a slice is chosen belong to the hands that drive it;
//! [`crate::input`] drives it from a controller.

use crate::action::{Action, BUILD_CATEGORIES, building_category};
use crate::game::Scene;
use crate::input::Group;
use crate::panel::{Card, CardAction, CardIcon, Panel, UpgradeComparison, VerbIcon};
use crate::typography::{entity_name, plural_name};
use aftermine_sim::UnitKind;
use macroquad::prelude::{Color, Vec2, vec2};

/// The most slices a ring holds, so each is a clear push of the stick.
pub(crate) const MAX_SLICES: usize = 8;

/// How far the stick must travel before it points at a slice.
const POINT_TRAVEL: f32 = 0.5;

/// Which first ring a ring stands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RingKind {
    /// The selection's command cards and the rings they open.
    Commands,
    /// The control groups.
    Groups,
}

/// A ring opened from a slice of the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Branch {
    /// The six build categories, through the construction palette.
    Build,
    /// One build category's structures.
    Category(u8),
    /// The Harvesters' own cards, in a selection mixing them with other
    /// units.
    Workers,
    /// A drone host's bay cards, in a selection mixing it with other
    /// units.
    Drones,
    /// What did not fit on a full ring.
    More,
}

/// What picking a slice does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Pick {
    /// Presses its card, as a click on the card would.
    Card(CardAction),
    /// Opens a ring of its own.
    Open(Branch),
    /// Recalls control group `n`, numbered from 1; X saves the selection
    /// to it.
    Group(u8),
}

/// One slice of a ring: the card it shows and what picking it does.
#[derive(Clone)]
pub(crate) struct Slice {
    /// The card as the panel built it: face, cost, reason and tooltip.
    pub(crate) card: Card,
    /// What picking it does.
    pub(crate) pick: Pick,
    /// How many it counts: of what it trains or orders, those waiting in
    /// the queue dock; of a group, its members.
    pub(crate) count: usize,
    /// What X does to it: the dock's own act that cancels one.
    pub(crate) cancel: Option<CardAction>,
    /// The slices of the ring it opens, when it opens one here.
    within: Vec<Slice>,
}

impl Slice {
    /// A panel card as a slice: the Build card opens the build rings, a
    /// unit slot counts what the dock holds of it, and a round or drone
    /// order counts what the selection has on order once staged commands
    /// run, as the panel's own spending does.
    fn of(game: &Scene<'_>, panel: &Panel, card: &Card) -> Self {
        let pick = match card.action {
            CardAction::Dispatch(Action::ToggleBuildPalette) => Pick::Open(Branch::Build),
            action => Pick::Card(action),
        };
        let mut slice = Self {
            card: card.clone(),
            pick,
            count: 0,
            cancel: None,
            within: Vec::new(),
        };
        let queued = match (card.action, card.icon) {
            (CardAction::Dispatch(Action::TrainSlot(_)), CardIcon::Unit(kind)) => {
                let (count, head) = queued_units(panel, kind);
                Some((
                    count,
                    head,
                    (count > 0).then_some(CardAction::CancelProduction(kind)),
                ))
            }
            (CardAction::Dispatch(Action::OrderRound), _) => {
                // X does what the ring's own Cancel card does, whenever it can.
                let cancel = panel
                    .cards
                    .iter()
                    .find(|card| card.action == CardAction::Dispatch(Action::CancelRound))
                    .filter(|card| card.enabled)
                    .map(|card| card.action);
                let bays = crate::unit_bays::SelectedBays::inspect(game);
                let (count, head) = if bays.is_empty() {
                    crate::building_actions::SelectedBuildings::inspect(game).rounds_on_order()
                } else {
                    (bays.on_order(), drone_progress(panel))
                };
                Some((count, head, cancel))
            }
            _ => None,
        };
        if let Some((count, head, cancel)) = queued {
            slice.count = count;
            slice.card.progress = head;
            slice.cancel = cancel;
        }
        slice
    }

    /// A slice that opens `branch`, holding `within`.
    fn opening(branch: Branch, card: Card, within: Vec<Slice>) -> Self {
        Self {
            card,
            pick: Pick::Open(branch),
            count: 0,
            cancel: None,
            within,
        }
    }

    /// Control group `number`'s slice: its lead kind and members while it
    /// holds any, else an empty slot that only X can fill.
    fn group(game: &Scene<'_>, number: u8, group: &Group) -> Self {
        let live = group.live(game);
        let card = Card {
            // An empty slot draws no face, only its number.
            icon: live.lead(game).unwrap_or(CardIcon::Verb(VerbIcon::Idle)),
            title: format!("Group {number}"),
            cost: None,
            hotkey: String::new(),
            action: CardAction::None,
            enabled: !live.is_empty(),
            why: None,
            desc: vec![members(game, &live)],
            progress: None,
            autocast: None,
            hold: None,
        };
        Self {
            card,
            pick: Pick::Group(number),
            count: live.len(),
            cancel: None,
            within: Vec::new(),
        }
    }

    /// Whether X does anything to it: cancels one of what it queues, or
    /// saves the selection to its group.
    pub(crate) fn takes_x(&self) -> bool {
        self.cancel.is_some() || matches!(self.pick, Pick::Group(_))
    }
}

/// A ring as it stands this frame.
pub(crate) struct Ring {
    /// Which first ring it stands in.
    pub(crate) kind: RingKind,
    /// The small label the ring wears over its hub.
    pub(crate) heading: String,
    /// The branches it stands under, from the first ring.
    pub(crate) path: Vec<Branch>,
    /// Its slices clockwise from the top.
    pub(crate) slices: Vec<Slice>,
    /// The color the selection's owner wears on card art.
    pub(crate) owner: Color,
    /// An upgrade's before and after, for the Upgrade card's tooltip.
    pub(crate) upgrade: Option<UpgradeComparison>,
}

impl Ring {
    /// The slice `pick` names, if the ring still holds it.
    pub(crate) fn position(&self, pick: Pick) -> Option<usize> {
        self.slices.iter().position(|slice| slice.pick == pick)
    }
}

/// The ring the panel offers at `path`, or at the deepest part of it that
/// still stands: a branch whose slice has gone ends the walk there. None
/// when the panel offers nothing.
pub(crate) fn ring(game: &Scene<'_>, panel: &Panel, path: &[Branch]) -> Option<Ring> {
    let (heading, shown, slices) = if palette(panel) {
        // The construction palette is up: only the build rings show, and
        // whatever led to Build stays for B to return to.
        let at = path.iter().position(|branch| *branch == Branch::Build);
        let mut shown = at.map_or_else(|| vec![Branch::Build], |at| path[..=at].to_vec());
        let category = at.and_then(|at| match path.get(at + 1) {
            Some(Branch::Category(category)) => Some(*category),
            _ => None,
        });
        match category {
            Some(category) => {
                shown.push(Branch::Category(category));
                (
                    BUILD_CATEGORIES[usize::from(category)].0.to_uppercase(),
                    shown,
                    structures(game, panel, category),
                )
            }
            None => ("BUILD".to_string(), shown, categories(game, panel)),
        }
    } else {
        let mut slices = first_ring(game, panel);
        let mut heading = "COMMANDS".to_string();
        let mut shown = Vec::new();
        for &branch in path {
            // Build opens through the palette, which has closed.
            if branch == Branch::Build {
                break;
            }
            let Some(at) = slices.iter().position(|s| s.pick == Pick::Open(branch)) else {
                break;
            };
            let slice = slices.swap_remove(at);
            heading = slice.card.title.to_uppercase();
            slices = slice.within;
            shown.push(branch);
        }
        (heading, shown, slices)
    };
    (!slices.is_empty()).then(|| Ring {
        kind: RingKind::Commands,
        heading,
        path: shown,
        slices,
        owner: panel.owner,
        upgrade: panel.info.upgrade.clone(),
    })
}

/// The group ring: one slice per control group, the first at the top. A
/// spectator keeps no groups, so is offered none.
pub(crate) fn groups(game: &Scene<'_>, groups: &[Group]) -> Option<Ring> {
    if game.presentation.spectating() {
        return None;
    }
    let human = game.presentation.human;
    Some(Ring {
        kind: RingKind::Groups,
        heading: "GROUPS".to_string(),
        path: Vec::new(),
        slices: groups
            .iter()
            .zip(1..)
            .map(|(group, number)| Slice::group(game, number, group))
            .collect(),
        owner: crate::render::seat_identity_color(game, human),
        upgrade: None,
    })
}

/// A group's members by kind, the most numerous first and then by name,
/// as one line: "3 Sentinels, 1 Scuttler".
fn members(game: &Scene<'_>, group: &Group) -> String {
    let names: Vec<&str> = match group {
        Group::Units(ids) => ids
            .iter()
            .filter_map(|id| game.state.unit(*id))
            .map(|unit| unit.kind.name())
            .collect(),
        Group::Buildings(ids) => ids
            .iter()
            .filter_map(|id| game.state.building(*id))
            .map(|building| building.kind.name())
            .collect(),
    };
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for name in names {
        match counts.iter_mut().find(|(seen, _)| *seen == name) {
            Some((_, count)) => *count += 1,
            None => counts.push((name, 1)),
        }
    }
    if counts.is_empty() {
        return "Empty.".to_string();
    }
    counts.sort_by(|(a, m), (b, n)| n.cmp(m).then(a.cmp(b)));
    let parts: Vec<String> = counts
        .iter()
        .map(|&(name, count)| {
            let name = entity_name(name);
            if count == 1 {
                format!("1 {name}")
            } else {
                format!("{count} {}", plural_name(&name))
            }
        })
        .collect();
    format!("{}.", parts.join(", "))
}

/// Whether the panel shows the construction palette.
fn palette(panel: &Panel) -> bool {
    panel
        .cards
        .iter()
        .any(|card| matches!(card.action, CardAction::ArmBuild(_)))
}

/// Whether a panel card belongs on a ring. Display chips do nothing, the
/// palette's Back card is what B already does, and the Stop that empties
/// every production queue would undo a whole queue on one stray push: X
/// cancels one at a time instead. A disabled card stays to explain itself.
fn offered(card: &Card) -> bool {
    match card.action {
        CardAction::None => !card.enabled,
        CardAction::Refused | CardAction::ClosePalette | CardAction::ClearQueues => false,
        _ => true,
    }
}

/// The selection's first ring: its command cards in panel order, then
/// Stop. Return cargo and Stop keep their places while they do not apply,
/// so nothing moves as they come and go. A mixed selection's Harvesters
/// and drone hosts keep their own cards behind one slice each, and a ring
/// that would still hold too many gives the rest to More.
fn first_ring(game: &Scene<'_>, panel: &Panel) -> Vec<Slice> {
    let stop = CardAction::Dispatch(Action::StopOrScrap);
    let mut slices: Vec<Slice> = panel
        .cards
        .iter()
        .filter(|card| offered(card))
        .chain(panel.absent.iter().filter(|card| card.action != stop))
        .chain(
            panel
                .stop
                .iter()
                .filter(|card| offered(card))
                .chain(panel.absent.iter().filter(|card| card.action == stop))
                .take(1),
        )
        .map(|card| Slice::of(game, panel, card))
        .collect();
    let kinds: Vec<UnitKind> = game
        .presentation
        .selection
        .units
        .iter()
        .filter_map(|id| game.state.unit(*id))
        .map(|unit| unit.kind)
        .collect();
    let harvests = |kind: &UnitKind| kind.stats().harvest.is_some();
    if let Some(worker) = kinds.iter().copied().find(harvests)
        && !kinds.iter().all(harvests)
    {
        fold(
            &mut slices,
            Branch::Workers,
            CardIcon::Unit(worker),
            &plural_name(&entity_name(worker.name())),
            |slice| {
                matches!(
                    slice.pick,
                    Pick::Open(Branch::Build)
                        | Pick::Card(CardAction::Dispatch(
                            Action::Salvage | Action::RepairUnit | Action::ReturnCargo
                        ))
                )
            },
        );
    }
    let hosts = |kind: &UnitKind| kind.stats().bay.is_some();
    if let Some(host) = kinds.iter().copied().find(hosts)
        && !kinds.iter().all(hosts)
        && let Some(drone) = host.stats().bay.map(|bay| bay.drone)
    {
        fold(
            &mut slices,
            Branch::Drones,
            CardIcon::Unit(drone),
            &plural_name(&entity_name(drone.name())),
            |slice| {
                matches!(
                    slice.pick,
                    Pick::Card(CardAction::Dispatch(
                        Action::OrderRound | Action::CancelRound | Action::ToggleAutofill
                    ))
                )
            },
        );
    }
    overflow(slices)
}

/// Gathers the slices `belongs` picks out into one slice that opens them,
/// standing where the first of them stood.
fn fold(
    slices: &mut Vec<Slice>,
    branch: Branch,
    icon: CardIcon,
    title: &str,
    belongs: impl Fn(&Slice) -> bool,
) {
    let Some(at) = slices.iter().position(&belongs) else {
        return;
    };
    let (within, rest): (Vec<Slice>, Vec<Slice>) =
        std::mem::take(slices).into_iter().partition(&belongs);
    *slices = rest;
    slices.insert(
        at,
        Slice::opening(branch, gathering(icon, title, &within), within),
    );
}

/// The card a gathering slice wears: what it holds, by name.
fn gathering(icon: CardIcon, title: &str, within: &[Slice]) -> Card {
    let names: Vec<&str> = within
        .iter()
        .map(|slice| slice.card.title.as_str())
        .collect();
    Card {
        icon,
        title: title.to_string(),
        cost: None,
        hotkey: String::new(),
        action: CardAction::None,
        enabled: true,
        why: None,
        desc: vec![format!("{}.", names.join(", "))],
        progress: None,
        autocast: None,
        hold: None,
    }
}

/// Keeps a ring within [`MAX_SLICES`]: from the seventh slice on, all but
/// Stop move into More, which keeps Stop last.
fn overflow(mut slices: Vec<Slice>) -> Vec<Slice> {
    if slices.len() <= MAX_SLICES {
        return slices;
    }
    let stop = slices
        .last()
        .is_some_and(|slice| slice.pick == Pick::Card(CardAction::Dispatch(Action::StopOrScrap)))
        .then(|| slices.pop())
        .flatten();
    let rest = slices.split_off(MAX_SLICES - 1 - usize::from(stop.is_some()));
    let card = gathering(CardIcon::Verb(VerbIcon::Idle), "More", &rest);
    slices.push(Slice::opening(Branch::More, card, overflow(rest)));
    slices.extend(stop);
    slices
}

/// The build categories, each opening its structures.
fn categories(game: &Scene<'_>, panel: &Panel) -> Vec<Slice> {
    (0..BUILD_CATEGORIES.len())
        .filter_map(|index| {
            let category = u8::try_from(index).ok()?;
            let within = structures(game, panel, category);
            let first = within.first()?;
            let card = gathering(first.card.icon, BUILD_CATEGORIES[index].0, &within);
            Some(Slice::opening(Branch::Category(category), card, Vec::new()))
        })
        .collect()
}

/// A category's structures, as the palette offers them.
fn structures(game: &Scene<'_>, panel: &Panel, category: u8) -> Vec<Slice> {
    panel
        .cards
        .iter()
        .filter(|card| {
            matches!(card.action, CardAction::ArmBuild(kind) if building_category(kind) == category)
        })
        .map(|card| Slice::of(game, panel, card))
        .collect()
}

/// How many units of `kind` the queue dock holds, and the head's progress
/// when one is building in a lone producer.
fn queued_units(panel: &Panel, kind: UnitKind) -> (usize, Option<f32>) {
    let mut count = 0;
    let mut head = None;
    for (index, card) in panel.queue.iter().enumerate() {
        match card.action {
            CardAction::CancelQueue(..) if card.icon == CardIcon::Unit(kind) => {
                count += 1;
                head = head.or(card.progress);
            }
            CardAction::CancelProduction(queued) if queued == kind => {
                count += panel.queue_groups.get(index).map_or(0, |group| group.count);
            }
            _ => {}
        }
    }
    (count, head)
}

/// The progress of the drone a lone host's bay is building.
fn drone_progress(panel: &Panel) -> Option<f32> {
    panel
        .drones
        .iter()
        .flat_map(|column| column.chips.iter())
        .find_map(|chip| chip.progress)
}

/// A slice's direction, in radians clockwise from straight up: a ring's
/// slices spread evenly from the top.
pub(crate) fn slice_angle(index: usize, count: usize) -> f32 {
    index as f32 * std::f32::consts::TAU / count.max(1) as f32
}

/// The unit vector from a ring's hub toward slice `index` of `count`,
/// y down.
pub(crate) fn slice_direction(index: usize, count: usize) -> Vec2 {
    let angle = slice_angle(index, count);
    vec2(angle.sin(), -angle.cos())
}

/// The slice of `count` a stick pushed past half travel points at: the
/// one whose share of the circle holds its direction.
pub(crate) fn slice_toward(stick: Vec2, count: usize) -> Option<usize> {
    if stick.length() < POINT_TRAVEL || count == 0 {
        return None;
    }
    let tau = std::f32::consts::TAU;
    let angle = stick.x.atan2(-stick.y).rem_euclid(tau);
    Some(crate::numeric::to_usize((angle / (tau / count as f32)).round()) % count)
}

/// Logical sizes at UI scale 1, before a short window shrinks them.
mod size {
    /// From the hub's center to each slice's center.
    pub(super) const RADIUS: f32 = 108.0;
    /// A slice's radius.
    pub(super) const SLICE: f32 = 32.0;
    /// The plate behind the ring reaches this far past a slice's center.
    pub(super) const PLATE: f32 = 48.0;
    /// From the plate's edge to the legend's baseline.
    pub(super) const LEGEND: f32 = 34.0;
    /// Kept clear of the window's edges and the HUD's chrome.
    pub(super) const MARGIN: f32 = 12.0;
    /// From the center to the plate's edge.
    pub(super) const REACH: f32 = RADIUS + PLATE;
    /// From the center to below the legend.
    pub(super) const BELOW: f32 = REACH + LEGEND;
}

/// Where a ring sits and how large it draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Geometry {
    /// The hub's center, in window pixels.
    pub(crate) center: Vec2,
    /// The scale its sizes are drawn at: the UI scale, shrunk where the
    /// window is too small.
    pub(crate) scale: f32,
}

impl Geometry {
    /// Places a ring on `anchor` as far as it fits between `top` and
    /// `bottom` inside the window, shrinking it where the space is smaller
    /// than the ring.
    pub(crate) fn fit(anchor: Vec2, viewport: Vec2, ui: f32, top: f32, bottom: f32) -> Self {
        let margin = size::MARGIN * ui;
        let room_y = (bottom - top - 2.0 * margin).max(0.0);
        let room_x = (viewport.x - 2.0 * margin).max(0.0);
        let scale = ui
            .min(room_y / (size::REACH + size::BELOW))
            .min(room_x / (2.0 * size::REACH))
            .max(0.0);
        let clamp = |value: f32, low: f32, high: f32| {
            if low <= high {
                value.clamp(low, high)
            } else {
                f32::midpoint(low, high)
            }
        };
        let center = vec2(
            clamp(
                anchor.x,
                margin + size::REACH * scale,
                viewport.x - margin - size::REACH * scale,
            ),
            clamp(
                anchor.y,
                top + margin + size::REACH * scale,
                bottom - margin - size::BELOW * scale,
            ),
        );
        Self { center, scale }
    }

    /// From the hub to each slice's center.
    pub(crate) fn radius(&self) -> f32 {
        size::RADIUS * self.scale
    }

    /// A slice's radius.
    pub(crate) fn slice(&self) -> f32 {
        size::SLICE * self.scale
    }

    /// The plate's radius.
    pub(crate) fn reach(&self) -> f32 {
        size::REACH * self.scale
    }

    /// Slice `index` of `count`'s center.
    pub(crate) fn slice_center(&self, index: usize, count: usize) -> Vec2 {
        self.center + slice_direction(index, count) * self.radius()
    }

    /// The legend's baseline under the ring.
    pub(crate) fn legend_baseline(&self) -> f32 {
        self.center.y + (size::BELOW - size::MARGIN) * self.scale
    }
}

#[cfg(test)]
mod tests;
