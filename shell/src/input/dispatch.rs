//! The action dispatcher: every semantic [`Action`] a chord resolves to
//! lands here exactly once, whether it came from a key, a panel card,
//! or an injected event.

use super::orders::digit_action;
use super::select::{cycle_idle_worker, idle_harvesters, select_army};
use super::{ClickVerb, InputState, Pointer};
use crate::action::Action;
use crate::game::Game;
use crate::numeric;
use aftermine_sim::Command;
use macroquad::prelude::Vec2;

#[expect(clippy::too_many_lines, reason = "one arm per action")]
pub(super) fn dispatch_action(
    game: &mut Game,
    input: &mut InputState,
    bindings: &crate::action::BindingMap,
    action: Action,
) {
    if input.construction_open()
        && matches!(
            action,
            Action::Run
                | Action::Hunt
                | Action::Scout
                | Action::Salvage
                | Action::RepairUnit
                | Action::Accompany
                | Action::Unload
                | Action::ReturnCargo
                | Action::Retire
        )
    {
        input.close_construction();
    }
    match action {
        // Continuous pans live in update_held; Confirm belongs to menus.
        Action::PanLeft | Action::PanRight | Action::PanUp | Action::PanDown => {}
        Action::Confirm
        | Action::ReplayPause
        | Action::ReplayBack
        | Action::ReplayForward
        | Action::ReplayStart
        | Action::ReplayEnd
        | Action::ReplaySpeed(_)
        | Action::ReplayStats
        | Action::MenuUp
        | Action::MenuDown
        | Action::MenuLeft
        | Action::MenuRight
        | Action::MenuPageUp
        | Action::MenuPageDown
        | Action::MenuHome
        | Action::MenuEnd
        | Action::DeleteSave
        | Action::PrevTab
        | Action::NextTab => {}
        Action::Slot(n) => digit_action(game, input, (n - 1) as usize),
        Action::AssignGroup(n) => {
            // Only own units or buildings enter a group: a recalled
            // selection holding an ally's would refuse every command under
            // own-gating, so foreign picks drop at assign time.
            let slot = (n - 1) as usize;
            if slot < input.groups.len() {
                let own = super::Group::of_selection(&game.view());
                let selection = &game.presentation.selection;
                let picked = selection.units.len().max(selection.buildings.len());
                if own.len() < picked {
                    game.presentation.toast(match own {
                        super::Group::Units(_) => "Only your own units join a control group",
                        super::Group::Buildings(_) => {
                            "Only your own structures join a control group"
                        }
                    });
                }
                if !own.is_empty()
                    && let Some(tutorial) = game.tutorial.as_mut()
                {
                    tutorial.note_group();
                }
                if picked == 0 || !own.is_empty() {
                    input.groups[slot] = own;
                }
            }
        }
        Action::ReturnCargo => {
            if !game.selection_commandable() {
                game.presentation
                    .toast("You can only command your own units");
                return;
            }
            if !game.presentation.selection.units.iter().any(|id| {
                game.state
                    .unit(*id)
                    .is_some_and(|unit| unit.kind.stats().harvest.is_some() && unit.carrying() > 0)
            }) {
                game.presentation.toast("No scrap carried");
                return;
            }
            input.disarm_click_verbs();
            input.patrol_route = None;
            game.issue(Command::ReturnCargo {
                units: game.presentation.selection.units.clone(),
                foundry: None,
                repair: false,
            });
        }
        Action::Retire => {
            if game.presentation.selection.units.is_empty() || !game.selection_commandable() {
                game.presentation
                    .toast("You can only command your own units");
                return;
            }
            input.disarm_click_verbs();
            game.issue(Command::Retire {
                units: game.presentation.selection.units.clone(),
                queue: input.queue_held(),
            });
        }
        Action::StopOrScrap => {
            // Contextual: units selected halt in place; a selected own
            // unfinished site is scrapped for its refund.
            if !game.presentation.selection.units.is_empty() && !game.selection_commandable() {
                game.presentation
                    .toast("You can only command your own units");
                return;
            }
            if game.presentation.selection.units.is_empty() {
                crate::building_actions::stop_or_scrap(game);
            } else {
                let units = game.presentation.selection.units.clone();
                game.issue(Command::Stop { units });
            }
        }
        Action::Launch => {
            let selected = crate::building_actions::SelectedBuildings::inspect(&game.view());
            if selected.ready_silos().is_empty() {
                game.presentation.toast("No warhead loaded");
                return;
            }
            input.disarm_click_verbs();
            input.striking = Some(super::Strike::new(selected.silos()));
            if input.last_pointer == Pointer::Touch {
                super::drop_aim_in_view(&game.view(), input);
            }
        }
        Action::OrderRound => {
            let bays = crate::unit_bays::SelectedBays::inspect(&game.view());
            // Shift, or a pad's LT, fills every empty slot, as the card shows
            // while it is held.
            if bays.is_empty()
                && input.queue_keys_held()
                && crate::building_actions::SelectedBuildings::inspect(&game.view())
                    .fill()
                    .is_some_and(|fill| fill.wanted > 1)
            {
                crate::building_actions::fill_rounds(game);
                return;
            }
            let order = if bays.is_empty() {
                crate::building_actions::SelectedBuildings::inspect(&game.view())
                    .round_order()
                    .map(|building| Command::OrderRound { building })
            } else {
                bays.order().map(|unit| Command::OrderDrone { unit })
            };
            match order {
                Ok(command) => game.issue(command),
                Err(why) => game
                    .presentation
                    .toast(crate::typography::sentence_case(&why)),
            }
        }
        Action::CancelRound => {
            let bays = crate::unit_bays::SelectedBays::inspect(&game.view());
            let cancel = if bays.is_empty() {
                crate::building_actions::SelectedBuildings::inspect(&game.view()).round_cancel()
            } else {
                bays.cancel()
            };
            if let Some(command) = cancel {
                game.issue(command);
            }
        }
        Action::ToggleAutofill => {
            let bays = crate::unit_bays::SelectedBays::inspect(&game.view());
            let toggle = if bays.is_empty() {
                crate::building_actions::SelectedBuildings::inspect(&game.view()).autofill_toggle()
            } else {
                bays.autofill_toggle()
            };
            if let Some(command) = toggle {
                game.issue(command);
            }
        }
        Action::ToggleIdleWeld => {
            if let Some(command) = crate::idle_weld::SelectedWorkers::inspect(&game.view()).toggle()
            {
                game.issue(command);
            }
        }
        Action::TrainSlot(_)
        | Action::Upgrade
        | Action::SetRally
        | Action::ClearRally
        | Action::Unload
        | Action::Build(_) => super::activate_action_card(game, input, bindings, action),
        Action::BuildCategory(category) => {
            input.disarm_click_verbs();
            input.patrol_route = None;
            input.build_menu = true;
            input.build_category = Some(category);
        }
        Action::TogglePause => {
            if game.net_role() == Some(crate::game::network::NetRole::Client) {
                game.presentation.toast("Only the host can pause");
            } else {
                game.clock.paused = !game.clock.paused;
            }
        }
        Action::ToggleBuildPalette => {
            if input.construction_open() {
                if input.build_category.take().is_some() {
                    input.disarm_click_verbs();
                    input.build_menu = true;
                } else {
                    input.close_construction();
                }
                return;
            }
            input.close_construction();
            if game.view().selection_has_worker() {
                input.build_menu = true;
                input.stop_placing();
            } else {
                // No builder in hand — the key still means "I want to
                // build": grab the nearest own harvester (idle ones
                // first), select it, and open the palette. The camera
                // stays put; the machine walks to wherever the player
                // places.
                let idle = idle_harvesters(&game.view());
                let cx = numeric::to_i32(game.presentation.camera.center.x.floor());
                let cy = numeric::to_i32(game.presentation.camera.center.y.floor());
                let pick = game
                    .state
                    .units()
                    .iter()
                    .filter(|u| {
                        u.player == game.presentation.human && u.kind.stats().harvest.is_some()
                    })
                    .filter(|u| idle.is_empty() || idle.contains(&u.id))
                    .min_by_key(|u| {
                        let t = u.tile();
                        let (dx, dy) = (i64::from(t.x - cx), i64::from(t.y - cy));
                        (dx * dx + dy * dy, u.id.0)
                    })
                    .map(|u| u.id);
                if let Some(id) = pick {
                    game.presentation.selection.units = vec![id];
                    game.presentation.selection.buildings.clear();
                    input.build_menu = true;
                    input.stop_placing();
                } else {
                    game.presentation.toast("No worker to build with");
                }
            }
        }
        Action::Patrol => {
            if !game.selection_commandable() {
                game.presentation
                    .toast("You can only command your own units");
                return;
            }
            // First press arms a route; the second sends the circuit. On
            // a selection already patrolling, the card is lit and a press
            // turns the patrol off: they stop where they are.
            if input.patrol_route.is_some() {
                super::start_patrol(game, input);
            } else if crate::panel::selection_patrols(&game.view()) {
                let units = game.presentation.selection.units.clone();
                game.issue(Command::Stop { units });
            } else if !game.presentation.selection.units.is_empty() {
                input.disarm_click_verbs();
                input.patrol_route = Some(Vec::new());
            }
        }
        // The overlay sees through fog, so it is a development tool: a
        // LAN match refuses it, and a packaged copy binds no key to it.
        Action::ToggleOverlay => {
            if game.net_role().is_some() {
                game.presentation
                    .toast("The debug overlay is off in LAN matches");
            } else {
                game.presentation.overlay = !game.presentation.overlay;
            }
        }
        Action::Back => {
            // A planned row goes first, keeping placement armed; then
            // anything armed.
            if input.drop_stroke() {
                return;
            }
            if input.placing.is_some() {
                input.stop_placing();
                input.build_menu = true;
                return;
            }
            if input.build_category.take().is_some() {
                input.build_menu = true;
                return;
            }
            if input.build_menu {
                input.close_construction();
                return;
            }
            if input.click_verb.take().is_some() {
                return;
            }
            if !input.rallying.is_empty() {
                input.rallying.clear();
                return;
            }
            if input.striking.take().is_some() {
                return;
            }
            if input.patrol_route.take().is_some() {
                return;
            }
            game.presentation.selection.units.clear();
            game.presentation.selection.buildings.clear();
            game.presentation.selection.pile = None;
        }
        Action::SetBookmark(slot) => {
            input.bookmarks[slot as usize] = Some(game.presentation.camera.center);
            game.presentation
                .sounds_pending
                .push((crate::game::SoundKind::Click, None));
        }
        Action::RecallBookmark(slot) => {
            if let Some(center) = input.bookmarks[slot as usize] {
                super::reticle::look_at(game, input, center);
            }
        }
        Action::Salvage => toggle_verb(
            game,
            input,
            ClickVerb::Salvage,
            harvests,
            "No worker to salvage with",
        ),
        Action::RepairUnit => toggle_verb(
            game,
            input,
            ClickVerb::Weld,
            harvests,
            "No worker selected to repair",
        ),
        Action::Accompany => toggle_verb(
            game,
            input,
            ClickVerb::Accompany,
            |_| true,
            "You can only command your own units",
        ),
        Action::Run => toggle_verb(
            game,
            input,
            ClickVerb::Run,
            |_| true,
            "No units selected to run",
        ),
        Action::Scout => toggle_verb(
            game,
            input,
            ClickVerb::Scout,
            |unit| unit.kind.stats().scouts,
            "No Surveyor selected",
        ),
        Action::Hunt => toggle_verb(
            game,
            input,
            ClickVerb::Hunt,
            |_| true,
            "No units selected to hunt",
        ),
        Action::CycleIdleWorker => cycle_idle_worker(game, input),
        Action::SelectArmy => select_army(game, input, false),
        Action::SelectArmyInView => select_army(game, input, true),
        Action::SelectAllOfKind => select_all_of_kind(game, input, bindings),
        Action::JumpToLastAlert => {
            if let Some(world) = game.presentation.last_alert {
                super::reticle::look_at(game, input, world);
            } else {
                game.presentation
                    .sounds_pending
                    .push((crate::game::SoundKind::Denied, None));
            }
        }
        Action::HomeCamera => {
            // From one of your Foundries, under the reticle, the camera moves
            // to the next by id, so repeated presses cycle through them all.
            // The reticle rather than the camera's center decides, since the
            // map's edge can hold the camera to one spot for two Foundries.
            let foundries = own_foundries(&game.view());
            let camera = &game.presentation.camera;
            let looking = input.reticle.ground(camera);
            let next = foundries
                .iter()
                .position(|at| at.distance(looking) <= 0.5)
                .map_or(0, |index| (index + 1) % foundries.len());
            if let Some(target) = foundries.get(next) {
                super::reticle::look_at(game, input, *target);
            }
        }
    }
}

/// Arms the click `verb` for a selection holding an own unit `qualifies`
/// accepts, else toasts `refusal`. Pressing an armed verb again stands it
/// down.
fn toggle_verb(
    game: &mut Game,
    input: &mut InputState,
    verb: ClickVerb,
    qualifies: impl Fn(&aftermine_sim::Unit) -> bool,
    refusal: &'static str,
) {
    if input.armed(verb) {
        input.click_verb = None;
    } else if game.view().selection_has_own(qualifies) {
        input.disarm_click_verbs();
        input.click_verb = Some(verb);
    } else {
        game.presentation.toast(refusal);
    }
}

fn harvests(unit: &aftermine_sim::Unit) -> bool {
    unit.kind.stats().harvest.is_some()
}

/// Presses the panel's All button for a selection of one kind; with no
/// such offer, as when every one is already selected, the press is refused.
/// The panel is built afresh rather than read from the published one: a key
/// can follow a click that changed the selection in the same batch, before
/// any relayout.
fn select_all_of_kind(
    game: &mut Game,
    input: &mut InputState,
    bindings: &crate::action::BindingMap,
) {
    let offer = crate::panel::build_for_input(&game.view(), bindings, input)
        .and_then(|panel| panel.select_all)
        .map(|all| all.card.action);
    match offer {
        Some(action) => super::activate_card(game, input, bindings, action),
        None => super::deny(game),
    }
}

/// The centers of your built Foundries, in id order.
pub(super) fn own_foundries(game: &crate::game::Scene<'_>) -> Vec<Vec2> {
    game.own_foundries()
        .map(|b| {
            let center = b.center();
            crate::game::world_vec(center)
        })
        .collect()
}
