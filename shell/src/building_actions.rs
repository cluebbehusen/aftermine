//! Building capabilities and orders shared by single and grouped selections.

use crate::action::{Action, BindingMap};
use crate::game::{Game, Scene};
use crate::numeric::Fit;
use crate::panel::{Card, CardAction, CardIcon, VerbIcon, tick_time_label};
use crate::typography::entity_name;
use aftermine_sim::{Building, BuildingId, BuildingKind, Command};
use chassis::grid::TilePos;
use std::fmt::Write as _;

#[derive(Clone)]
pub(crate) struct SelectedBuildings {
    pub buildings: Vec<Building>,
    /// The viewer's completed buildings, by kind and tier.
    pub tech: Vec<(BuildingKind, u8)>,
    /// Anchors of the viewer's completed, living Foundries, for upgrades
    /// that need one in reach.
    pub foundries: Vec<TilePos>,
    pub scrap: u32,
    pub accepts: bool,
}

/// Why an upgrade that needs Foundry support cannot start.
pub(crate) fn foundry_reach_reason() -> String {
    format!(
        "needs a {} within {} tiles",
        entity_name(BuildingKind::Foundry.name()),
        aftermine_sim::stats::EXTRACTOR_SUPPORT_RADIUS
    )
}

/// What a magazine holds: a Spire's warheads, a Lattice's interceptors.
pub(crate) fn round_noun(kind: BuildingKind) -> &'static str {
    if kind == BuildingKind::Spire {
        "warhead"
    } else {
        "interceptor"
    }
}

/// The rounds a Fill buys: one per free slot of the selected magazines.
/// How the Order card's second act fills every empty slot, and its price.
fn fill_desc(hands: crate::platform::Hands, wanted: usize, noun: &str, cost: u32) -> String {
    let how = match (hands.touch(), hands.keys) {
        (true, _) => "Long-press",
        (false, true) => "Shift-click or right-click",
        (false, false) => "Right-click",
    };
    format!("{how} to fill every empty slot: {wanted} {noun}s, {cost} scrap.")
}

pub(crate) struct RoundBatch {
    /// The magazine each round goes to, in the order they are bought.
    pub recipients: Vec<BuildingId>,
    /// Free slots across the selection, paid for or not.
    pub wanted: usize,
    /// What the bought rounds cost together.
    pub cost: u32,
}

pub(crate) struct UpgradeBatch {
    pub recipients: Vec<BuildingId>,
    pub cost: u32,
    pub reason: Option<String>,
}

impl SelectedBuildings {
    pub fn inspect(game: &Scene<'_>) -> Self {
        Self::inspect_ids(game, &game.presentation.selection.buildings)
    }

    /// Whether the viewer has a completed building that answers `need`.
    pub fn meets(&self, need: aftermine_sim::stats::Prerequisite) -> bool {
        self.tech
            .iter()
            .any(|(kind, tier)| *kind == need.kind && *tier >= need.tier)
    }

    pub fn inspect_ids(game: &Scene<'_>, ids: &[BuildingId]) -> Self {
        let snapshot = |buildings: &[Building], scrap, accepts| Self {
            buildings: buildings
                .iter()
                .filter(|b| b.player == game.presentation.human && ids.contains(&b.id))
                .cloned()
                .collect(),
            tech: buildings
                .iter()
                .filter(|b| b.player == game.presentation.human && b.built())
                .map(|b| (b.kind, b.tier))
                .collect(),
            foundries: buildings
                .iter()
                .filter(|b| {
                    b.player == game.presentation.human
                        && b.kind == BuildingKind::Foundry
                        && b.built()
                        && b.hp > 0
                })
                .map(|b| b.anchor)
                .collect(),
            scrap,
            accepts,
        };
        if game.pending.is_empty() {
            let player = game.state.player(game.presentation.human);
            snapshot(
                game.state.buildings(),
                player.scrap,
                game.state.accepts_commands(game.presentation.human),
            )
        } else {
            game.state.inspect_command_phase(game.pending, |state| {
                snapshot(
                    state.buildings(),
                    state.scrap(game.presentation.human).unwrap_or(0),
                    state.accepts_commands(game.presentation.human),
                )
            })
        }
    }

    pub fn homogeneous(&self) -> bool {
        self.buildings
            .first()
            .is_some_and(|first| self.buildings.iter().all(|b| b.kind == first.kind))
    }

    pub fn producers(&self) -> Vec<BuildingId> {
        self.buildings
            .iter()
            .filter(|b| self.accepts && b.built() && !b.stats().produces.is_empty())
            .map(|b| b.id)
            .collect()
    }

    pub fn defenses(&self, domain: Option<aftermine_sim::stats::Domain>) -> Vec<BuildingId> {
        self.buildings
            .iter()
            .filter(|b| {
                self.accepts
                    && b.built()
                    && b.stats().weapons.first().is_some_and(|weapon| {
                        domain.is_none_or(|domain| weapon.targets.covers(domain))
                    })
            })
            .map(|b| b.id)
            .collect()
    }

    /// Selected built magazine buildings, all of one kind, in id order.
    fn magazines(&self) -> Vec<&Building> {
        if !self.accepts || !self.homogeneous() {
            return Vec::new();
        }
        self.buildings
            .iter()
            .filter(|b| b.built() && b.magazine.is_some())
            .collect()
    }

    /// Selected built Spires, loaded or not, in id order.
    pub fn silos(&self) -> Vec<BuildingId> {
        self.magazines()
            .into_iter()
            .filter(|b| b.launcher.is_some())
            .map(|b| b.id)
            .collect()
    }

    /// Selected Spires that can fire now: loaded, with no strike ordered.
    pub fn ready_silos(&self) -> Vec<BuildingId> {
        self.ready().map(|b| b.id).collect()
    }

    fn ready(&self) -> impl Iterator<Item = &Building> {
        self.magazines().into_iter().filter(|b| {
            b.launcher.is_some_and(|launcher| launcher.target.is_none())
                && b.magazine.is_some_and(|m| m.loaded > 0)
        })
    }

    /// The `size` ready Spires a salvo at `target` fires: those whose decks
    /// need the least turning to face it, then the lowest ids, so the
    /// strike lands soon and the same click picks the same Spires.
    pub fn salvo(&self, target: TilePos, size: usize) -> Vec<BuildingId> {
        let mut ready: Vec<_> = self
            .ready()
            .map(|b| {
                let heading = b.launcher.map_or(0, |launcher| launcher.heading);
                let turn = chassis::compass::heading_of(target.center() - b.center())
                    .wrapping_sub(heading)
                    .cast_signed()
                    .unsigned_abs();
                (turn, b.id)
            })
            .collect();
        ready.sort_unstable();
        ready.into_iter().take(size).map(|(_, id)| id).collect()
    }

    /// The selected magazines' rounds as the panel counts them, when the
    /// selection is magazines of one kind.
    pub fn ammunition(&self) -> Option<String> {
        if !self.homogeneous() {
            return None;
        }
        let kind = self.buildings.first()?.kind;
        let capacity = kind.magazine()?.capacity;
        let (mut loaded, mut firing, mut ordered, mut empty) = (0, 0, 0, 0);
        for b in self.buildings.iter().filter(|b| b.built()) {
            let Some(magazine) = b.magazine else {
                continue;
            };
            // A warhead stays in its magazine while the deck turns to fire it,
            // so it counts as firing rather than as one the next pick can use.
            if b.launcher.is_some_and(|launcher| launcher.target.is_some()) {
                firing += usize::from(magazine.loaded);
            } else {
                loaded += usize::from(magazine.loaded);
            }
            ordered += usize::from(magazine.ordered);
            empty += usize::from(capacity - magazine.loaded - magazine.ordered);
        }
        let noun = if kind == BuildingKind::Spire {
            "Warheads"
        } else {
            "Interceptors"
        };
        let mut line = format!("{noun}: {loaded} loaded");
        if firing > 0 {
            let _ = write!(line, " \u{b7} {firing} firing");
        }
        let _ = write!(line, " \u{b7} {ordered} building \u{b7} {empty} empty");
        Some(line)
    }

    /// Selected Spires turning toward an ordered strike, whatever else is
    /// selected with them.
    fn armed_silos(&self) -> Vec<BuildingId> {
        self.buildings
            .iter()
            .filter(|b| {
                self.accepts
                    && b.built()
                    && b.launcher.is_some_and(|launcher| launcher.target.is_some())
            })
            .map(|b| b.id)
            .collect()
    }

    /// The selected magazine an order fills: the one with the most room,
    /// then the lowest id.
    pub fn round_recipient(&self) -> Option<BuildingId> {
        self.magazines()
            .into_iter()
            .filter_map(|b| {
                let magazine = b.magazine?;
                let room = b.kind.magazine()?.capacity - magazine.loaded - magazine.ordered;
                (room > 0).then_some((std::cmp::Reverse(room), b.id))
            })
            .min()
            .map(|(_, id)| id)
    }

    /// Every free slot of the selected magazines, bought as Order would buy
    /// them one press at a time (the emptiest magazine, then the lowest id)
    /// while scrap lasts.
    pub fn fill(&self) -> Option<RoundBatch> {
        let magazines = self.magazines();
        let stats = magazines.first()?.kind.magazine()?;
        let mut rooms: Vec<(u8, BuildingId)> = magazines
            .iter()
            .filter_map(|b| {
                let magazine = b.magazine?;
                Some((stats.capacity - magazine.loaded - magazine.ordered, b.id))
            })
            .collect();
        let mut batch = RoundBatch {
            recipients: Vec::new(),
            wanted: rooms.iter().map(|&(room, _)| usize::from(room)).sum(),
            cost: 0,
        };
        let mut bank = self.scrap;
        while bank >= stats.round_cost {
            let Some(slot) = rooms
                .iter_mut()
                .filter(|(room, _)| *room > 0)
                .min_by_key(|(room, id)| (std::cmp::Reverse(*room), *id))
            else {
                break;
            };
            slot.0 -= 1;
            bank -= stats.round_cost;
            batch.cost += stats.round_cost;
            batch.recipients.push(slot.1);
        }
        Some(batch)
    }

    /// The selected magazine an order fills, or why the order cannot go.
    pub fn round_order(&self) -> Result<BuildingId, String> {
        let building = self
            .round_recipient()
            .ok_or_else(|| "magazine is full".to_string())?;
        let cost = self
            .magazines()
            .first()
            .and_then(|b| b.kind.magazine())
            .map_or(0, |stats| stats.round_cost);
        if self.scrap < cost {
            return Err(format!("needs {cost} scrap"));
        }
        Ok(building)
    }

    /// The cancel refunding the latest paid round of the selected magazine
    /// with the most on order.
    pub fn round_cancel(&self) -> Option<Command> {
        self.magazines()
            .into_iter()
            .filter_map(|b| Some((b.magazine.filter(|m| m.ordered > 0)?.ordered, b.id)))
            .max_by_key(|&(ordered, id)| (ordered, std::cmp::Reverse(id)))
            .map(|(ordered, building)| Command::CancelRound {
                building,
                index: ordered - 1,
            })
    }

    /// The autofill command the toggle issues: on unless every selected
    /// magazine already has it on.
    pub fn autofill_toggle(&self) -> Option<Command> {
        let magazines = self.magazines();
        if magazines.is_empty() {
            return None;
        }
        let all_on = magazines
            .iter()
            .all(|b| b.magazine.is_some_and(|m| m.autofill));
        Some(Command::SetAutofill {
            buildings: magazines.iter().map(|b| b.id).collect(),
            enabled: !all_on,
        })
    }

    fn magazine_cards(&self, bindings: &BindingMap, cards: &mut Vec<Card>) {
        let magazines = self.magazines();
        let Some(first) = magazines.first() else {
            return;
        };
        let Some(stats) = first.kind.magazine() else {
            return;
        };
        let silo = first.kind == BuildingKind::Spire;
        let noun = round_noun(first.kind);
        if silo {
            let ready = self.ready_silos().len();
            cards.push(Card {
                icon: CardIcon::Verb(VerbIcon::Attack),
                title: if ready > 0 {
                    format!("Launch ({ready} loaded)")
                } else {
                    "Launch".into()
                },
                cost: None,
                hotkey: bindings.labels(Action::Launch),
                action: CardAction::Dispatch(Action::Launch),
                enabled: ready > 0,
                why: (ready == 0).then(|| "no warhead loaded".into()),
                desc: vec![
                    "Pick any tile, seen or not. Each pick fires the salvo size the banner shows; the decks turn to face it, then fire.".into(),
                    "Stop calls off a strike that has not launched.".into(),
                ],
                progress: None,
                autocast: None,
                hold: None,
            });
        }
        let order = self.round_order();
        let mut desc = vec![format!(
            "One {noun}, paid now; built one at a time in {}.",
            tick_time_label(stats.round_ticks)
        )];
        let fill = self.fill().filter(|fill| fill.wanted > 1);
        if let Some(fill) = &fill {
            desc.push(fill_desc(
                crate::platform::hands(),
                fill.wanted,
                noun,
                fill.wanted.fit::<u32>() * stats.round_cost,
            ));
        }
        cards.push(Card {
            icon: CardIcon::Round(first.kind),
            title: "Order".into(),
            cost: Some(stats.round_cost),
            hotkey: bindings.labels(Action::OrderRound),
            action: CardAction::Dispatch(Action::OrderRound),
            enabled: order.is_ok(),
            why: order.err(),
            desc,
            progress: None,
            autocast: None,
            hold: fill.is_some().then_some(CardAction::FillRounds),
        });
        let cancel = self.round_cancel();
        cards.push(Card {
            icon: CardIcon::Verb(VerbIcon::Cancel),
            title: "Cancel".into(),
            cost: None,
            hotkey: bindings.labels(Action::CancelRound),
            action: CardAction::Dispatch(Action::CancelRound),
            enabled: cancel.is_some(),
            why: cancel.is_none().then(|| format!("no {noun} on order")),
            desc: vec![
                format!("Refunds the latest paid {noun} in full."),
                "Also turns autofill off, so it is not bought again.".into(),
            ],
            progress: None,
            autocast: None,
            hold: None,
        });
        let on = magazines
            .iter()
            .all(|b| b.magazine.is_some_and(|m| m.autofill));
        cards.push(Card {
            icon: CardIcon::Verb(VerbIcon::Patrol),
            title: format!("Autofill: {}", if on { "on" } else { "off" }),
            cost: None,
            hotkey: bindings.labels(Action::ToggleAutofill),
            action: CardAction::Dispatch(Action::ToggleAutofill),
            enabled: true,
            why: None,
            desc: vec![format!(
                "While on, orders the next {noun} whenever there is room and scrap."
            )],
            progress: None,
            autocast: None,
            hold: None,
        });
    }

    /// The Order card as Shift shows it: the Fill a Shift-click buys, while
    /// more than one slot is empty.
    pub fn fill_card(&self) -> Option<Card> {
        let fill = self.fill().filter(|fill| fill.wanted > 1)?;
        let first = self.magazines().into_iter().next()?;
        let stats = first.kind.magazine()?;
        let bought = fill.recipients.len();
        let noun = round_noun(first.kind);
        Some(Card {
            icon: CardIcon::Round(first.kind),
            title: if bought == fill.wanted {
                format!("Fill {bought}")
            } else {
                format!("Fill {bought}/{}", fill.wanted)
            },
            cost: Some(if bought > 0 {
                fill.cost
            } else {
                fill.wanted.fit::<u32>() * stats.round_cost
            }),
            // Shift+O is a different binding, so no key fills.
            hotkey: String::new(),
            action: CardAction::Dispatch(Action::OrderRound),
            enabled: bought > 0,
            why: (bought == 0).then(|| format!("needs {} scrap", stats.round_cost)),
            desc: vec![format!(
                "One {noun} for every empty slot, paid now; {bought} of {} affordable.",
                fill.wanted
            )],
            progress: None,
            autocast: None,
            hold: None,
        })
    }

    pub fn sites(&self) -> Vec<BuildingId> {
        if !self.accepts || !self.homogeneous() {
            return Vec::new();
        }
        self.buildings
            .iter()
            .filter(|b| !b.built() && b.tier == 0)
            .map(|b| b.id)
            .collect()
    }

    pub fn upgrade_batch(&self) -> Option<UpgradeBatch> {
        if !self.homogeneous()
            || !self
                .buildings
                .iter()
                .any(|b| b.kind.upgrade_from(b.tier).is_some())
        {
            return None;
        }
        let mut batch = UpgradeBatch {
            recipients: Vec::new(),
            cost: 0,
            reason: None,
        };
        if !self.accepts {
            batch.reason = Some("building orders unavailable".into());
            return Some(batch);
        }
        let mut bank = self.scrap;
        let mut reasons = Vec::new();
        for b in &self.buildings {
            let reason = if !b.built() {
                Some("offline".into())
            } else if let Some(upgrade) = b.kind.upgrade_from(b.tier) {
                if let Some(req) = upgrade.requires.iter().find(|req| !self.meets(**req)) {
                    Some(format!("needs a standing {}", entity_name(req.name())))
                } else if b.kind.upgrade_needs_foundry_support(b.tier)
                    && !self.foundries.iter().any(|&foundry| {
                        aftermine_sim::geometry::foundry_supports(foundry, b.anchor)
                    })
                {
                    Some(foundry_reach_reason())
                } else if bank < upgrade.cost {
                    Some(format!("needs {} scrap", upgrade.cost))
                } else {
                    bank -= upgrade.cost;
                    batch.cost += upgrade.cost;
                    batch.recipients.push(b.id);
                    None
                }
            } else {
                Some("max tier".into())
            };
            if let Some(reason) = reason {
                reasons.push(reason);
            }
        }
        reasons.sort();
        reasons.dedup();
        if !reasons.is_empty() {
            batch.reason = Some(reasons.join(", "));
        }
        Some(batch)
    }

    pub fn cards(&self, bindings: &BindingMap) -> Vec<Card> {
        let mut cards = Vec::new();
        let producers = self.producers();
        let plural = self.buildings.len() > 1;
        if !producers.is_empty() {
            let any_rally = self
                .buildings
                .iter()
                .any(|b| producers.contains(&b.id) && b.rally.is_some());
            let noun = if plural { "rallies" } else { "rally" };
            cards.push(Card {
                icon: CardIcon::Verb(VerbIcon::Rally),
                title: format!("{} {noun}", if any_rally { "Reset" } else { "Set" }),
                cost: None,
                hotkey: bindings.labels(Action::SetRally),
                action: CardAction::ArmRally,
                enabled: true,
                why: None,
                desc: vec![
                    format!("Set one destination for {} producers.", producers.len()),
                    "A scrap rally sends new Harvesters straight to work;".into(),
                    "a friendly unit has every new unit escort it.".into(),
                ],
                progress: None,
                autocast: None,
                hold: None,
            });
            cards.push(Card {
                icon: CardIcon::Verb(VerbIcon::Rally),
                title: format!("Clear {noun}"),
                cost: None,
                hotkey: bindings.labels(Action::ClearRally),
                action: CardAction::ClearRally,
                enabled: any_rally,
                why: (!any_rally).then(|| "no rally points set".into()),
                desc: vec!["Return new units to their producer doors.".into()],
                progress: None,
                autocast: None,
                hold: None,
            });
        }
        self.magazine_cards(bindings, &mut cards);
        let defenses = self.defenses(None);
        let sites = self.sites();
        if !sites.is_empty() {
            cards.push(Card {
                icon: CardIcon::Verb(VerbIcon::Cancel),
                title: if plural { format!("Scrap {} sites", sites.len()) } else { "Scrap site".into() },
                cost: None,
                hotkey: if defenses.is_empty() { bindings.labels(Action::StopOrScrap) } else { String::new() },
                action: CardAction::ScrapSites,
                enabled: true,
                why: None,
                desc: vec!["Abandon fresh construction sites; full refund before work starts, partial refund afterward.".into(), "Committed upgrades cannot be cancelled.".into()],
                progress: None,
                autocast: None,
                hold: None,
            });
        }
        // A single upgrading works retains its progress card. Group actions
        // remain available for other members that are still in service.
        if let [b] = self.buildings.as_slice()
            && !b.built()
            && b.tier > 0
        {
            cards.push(Card {
                icon: CardIcon::Verb(VerbIcon::Cancel),
                title: "Upgrading".into(),
                cost: None,
                hotkey: String::new(),
                action: CardAction::None,
                enabled: false,
                why: Some("upgrades cannot be cancelled".into()),
                desc: vec!["The works returns to service when the upgrade finishes.".into()],
                progress: b.stats().construction.map(|c| {
                    (b.construction_progress().unwrap_or(0) as f32 / c.build_ticks.max(1) as f32)
                        .clamp(0.0, 1.0)
                }),
                autocast: None,
                hold: None,
            });
        } else if let Some(batch) = self.upgrade_batch() {
            let first = self
                .buildings
                .iter()
                .find(|b| b.kind.upgrade_from(b.tier).is_some())
                .unwrap();
            let mut desc = Vec::new();
            let mut tiers: Vec<_> = self.buildings.iter().map(|b| b.tier).collect();
            tiers.sort_unstable();
            tiers.dedup();
            for tier in tiers {
                if let Some(upgrade) = first.kind.upgrade_from(tier) {
                    desc.push(format!(
                        "{} to {}: {} scrap each. Offline for {} while upgrading.",
                        entity_name(first.kind.tier_name(tier)),
                        entity_name(first.kind.tier_name(tier + 1)),
                        upgrade.cost,
                        tick_time_label(upgrade.build_ticks)
                    ));
                }
                if first.kind.is_public(tier + 1) {
                    desc.push(format!(
                        "Every player will see where your {} stands.",
                        entity_name(first.kind.tier_name(tier + 1))
                    ));
                }
            }
            if plural {
                desc.push(format!(
                    "Upgrade {} of {} buildings; {} scrap total.",
                    batch.recipients.len(),
                    self.buildings.len(),
                    batch.cost
                ));
                if let Some(reason) = &batch.reason {
                    desc.push(format!("Skipped: {reason}."));
                }
            }
            cards.push(Card {
                icon: CardIcon::Building(first.kind, first.tier),
                title: if plural {
                    format!(
                        "Upgrade {}/{}",
                        batch.recipients.len(),
                        self.buildings.len()
                    )
                } else {
                    format!(
                        "Upgrade: {}",
                        entity_name(first.kind.tier_name(first.tier + 1))
                    )
                },
                cost: Some(if plural {
                    batch.cost
                } else {
                    first.kind.upgrade_from(first.tier).unwrap().cost
                }),
                hotkey: bindings.labels(Action::Upgrade),
                action: CardAction::Upgrade,
                enabled: !batch.recipients.is_empty(),
                why: batch
                    .recipients
                    .is_empty()
                    .then_some(batch.reason)
                    .flatten(),
                desc,
                progress: None,
                autocast: None,
                hold: None,
            });
        }
        cards
    }
}

pub(crate) fn upgrade(game: &mut Game) {
    let selected = SelectedBuildings::inspect(&game.view());
    let Some(batch) = selected.upgrade_batch() else {
        return;
    };
    let count = batch.recipients.len();
    for building in batch.recipients {
        game.issue(Command::UpgradeBuilding { building });
    }
    if let Some(reason) = batch.reason {
        game.presentation.toast(if selected.buildings.len() > 1 {
            format!(
                "Upgrading {count} of {}: {reason}",
                selected.buildings.len()
            )
        } else {
            reason
        });
    }
}

/// Orders a round into every empty slot of the selected magazines, as far
/// as scrap goes.
pub(crate) fn fill_rounds(game: &mut Game) {
    let selected = SelectedBuildings::inspect(&game.view());
    let Some(batch) = selected.fill() else {
        return;
    };
    let Some(first) = selected.magazines().into_iter().next() else {
        return;
    };
    let (kind, cost) = (
        first.kind,
        first.kind.magazine().map_or(0, |m| m.round_cost),
    );
    let noun = round_noun(kind);
    let bought = batch.recipients.len();
    for &building in &batch.recipients {
        game.issue(Command::OrderRound { building });
    }
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    game.presentation.toast(if batch.wanted == 0 {
        "Magazines are full".to_string()
    } else if bought == 0 {
        format!("Needs {cost} scrap")
    } else if bought < batch.wanted {
        format!("Ordered {bought} of {} {noun}s: out of scrap", batch.wanted)
    } else {
        format!("Ordered {bought} {noun}{}", plural(bought))
    });
}

pub(crate) fn scrap_sites(game: &mut Game) {
    let sites = SelectedBuildings::inspect(&game.view()).sites();
    for &building in &sites {
        game.issue(Command::Cancel { building });
    }
    game.presentation
        .selection
        .buildings
        .retain(|id| !sites.contains(id));
}

pub(crate) fn stop_or_scrap(game: &mut Game) {
    let selected = SelectedBuildings::inspect(&game.view());
    let mut buildings = selected.defenses(None);
    buildings.extend(selected.armed_silos());
    if !buildings.is_empty() {
        game.issue(Command::ClearFocus { buildings });
    } else if !selected.sites().is_empty() {
        scrap_sites(game);
    } else if selected.buildings.iter().any(|b| !b.built() && b.tier > 0) {
        game.presentation.toast("Upgrades cannot be cancelled");
    }
}

#[cfg(test)]
pub(crate) mod tests;
