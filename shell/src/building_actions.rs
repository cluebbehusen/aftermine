//! Building capabilities and orders shared by single and grouped selections.

use crate::action::{Action, BindingMap};
use crate::game::{Game, Scene};
use crate::panel::{Card, CardAction, CardIcon, VerbIcon, tick_time_label};
use crate::typography::entity_name;
use chassis::grid::TilePos;
use oxide_sim::{Building, BuildingId, BuildingKind, Command};

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
        oxide_sim::stats::EXTRACTOR_SUPPORT_RADIUS
    )
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
    pub fn meets(&self, need: oxide_sim::stats::Prerequisite) -> bool {
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

    pub fn defenses(&self, domain: Option<oxide_sim::stats::Domain>) -> Vec<BuildingId> {
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

    /// Selected Spires holding a warhead.
    pub fn loaded_silos(&self) -> Vec<BuildingId> {
        self.magazines()
            .into_iter()
            .filter(|b| b.launcher.is_some() && b.magazine.is_some_and(|m| m.loaded > 0))
            .map(|b| b.id)
            .collect()
    }

    /// Selected Spires turning toward an ordered strike.
    fn armed_silos(&self) -> Vec<BuildingId> {
        self.magazines()
            .into_iter()
            .filter(|b| b.launcher.is_some_and(|launcher| launcher.target.is_some()))
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

    /// The selected magazine whose latest paid round a cancel refunds.
    pub fn round_to_cancel(&self) -> Option<BuildingId> {
        self.magazines()
            .into_iter()
            .filter(|b| b.magazine.is_some_and(|m| m.ordered > 0))
            .max_by_key(|b| (b.magazine.map_or(0, |m| m.ordered), std::cmp::Reverse(b.id)))
            .map(|b| b.id)
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
        let noun = if silo { "warhead" } else { "interceptor" };
        if silo {
            let loaded = self.loaded_silos();
            cards.push(Card {
                icon: CardIcon::Verb(VerbIcon::Attack),
                title: "Launch".into(),
                cost: None,
                hotkey: bindings.labels(Action::Launch),
                action: CardAction::Dispatch(Action::Launch),
                enabled: !loaded.is_empty(),
                why: loaded.is_empty().then(|| "no warhead loaded".into()),
                desc: vec![
                    "Pick any tile, seen or not. The deck turns to face it, then fires.".into(),
                    "Stop calls off a strike that has not launched.".into(),
                ],
                progress: None,
            });
        }
        let recipient = self.round_recipient();
        let ordering = magazines
            .iter()
            .filter_map(|b| b.magazine)
            .find(|m| m.ordered > 0);
        cards.push(Card {
            icon: CardIcon::Building(first.kind, 0),
            title: format!("Order {noun}"),
            cost: Some(stats.round_cost),
            hotkey: bindings.labels(Action::OrderRound),
            action: CardAction::Dispatch(Action::OrderRound),
            enabled: recipient.is_some(),
            why: recipient.is_none().then(|| "magazine is full".into()),
            desc: vec![format!(
                "Paid now; built one at a time in {}.",
                tick_time_label(stats.round_ticks)
            )],
            progress: ordering
                .map(|m| (m.progress as f32 / stats.round_ticks.max(1) as f32).clamp(0.0, 1.0)),
        });
        let cancel = self.round_to_cancel();
        cards.push(Card {
            icon: CardIcon::Verb(VerbIcon::Cancel),
            title: format!("Cancel {noun}"),
            cost: None,
            hotkey: bindings.labels(Action::CancelRound),
            action: CardAction::Dispatch(Action::CancelRound),
            enabled: cancel.is_some(),
            why: cancel.is_none().then(|| format!("no {noun} on order")),
            desc: vec![
                "Refunds the latest paid round in full.".into(),
                "Also turns autofill off, so it is not bought again.".into(),
            ],
            progress: None,
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
        });
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
                    && !self
                        .foundries
                        .iter()
                        .any(|&foundry| oxide_sim::geometry::foundry_supports(foundry, b.anchor))
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
