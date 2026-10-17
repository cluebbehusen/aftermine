use super::*;
use crate::game::Game;

#[test]
fn category_labels_name_keys_only_where_keys_exist() {
    assert_eq!(
        category_label("DEFENSE", false, "3", None, false),
        "DEFENSE [3]"
    );
    assert_eq!(
        category_label("DEFENSE", false, "3", Some("B"), false),
        "DEFENSE [B > 3]"
    );
    for touch_only in [false, true] {
        assert_eq!(
            category_label("DEFENSE", true, "3", Some("B"), touch_only),
            "DEFENSE *"
        );
    }
    for palette_key in [None, Some("B")] {
        let label = category_label("DEFENSE", false, "3", palette_key, true);
        crate::platform::assert_touch_copy(&label);
    }
}

#[test]
fn collective_queue_keeps_every_kind_visible_in_narrow_windows() {
    for scale in [1.0, 1.5, 2.0] {
        for (width, top) in [(640.0, 110.0), (800.0, 190.0), (1280.0, 650.0)] {
            for count in 1..=8 {
                let (dock, slots, shown) =
                    collective_queue_grid(count, top * scale, scale, width * scale, 0.0);
                assert_eq!(shown, count);
                assert!(dock.right() <= width * scale);
                assert!(dock.y >= crate::layout::TOP_BAR_H * scale);
                for (i, slot) in slots.iter().take(shown).enumerate() {
                    assert!(slot.x >= dock.x && slot.right() <= dock.right());
                    assert!(slot.y >= dock.y && slot.bottom() <= top * scale);
                    assert!(slots[..i].iter().all(|other| !slot.overlaps(other)));
                }
            }
        }
    }
}

#[test]
fn production_dock_width_survives_countdowns_and_blocked_completion() {
    let mut game =
        Game::with_viewport(oxide_sim::Scenario::skirmish(), vec2(1280.0, 800.0)).unwrap();
    let foundry = game
        .state
        .buildings()
        .iter()
        .find(|b| b.player == game.presentation.human && b.kind == oxide_sim::BuildingKind::Foundry)
        .unwrap()
        .id;
    game.presentation.selection.buildings = vec![foundry];
    let train = oxide_sim::PlayerCommand {
        player: game.presentation.human,
        command: oxide_sim::Command::Train {
            building: foundry,
            kind: oxide_sim::UnitKind::Harvester,
        },
    };
    game.state.tick(&[train.clone(), train]);
    let measure = |text: &str| {
        text.chars()
            .map(|c| match c {
                '1' => 3.0,
                '8' => 9.0,
                _ => 7.0,
            })
            .sum::<f32>()
    };
    let mut widths = Vec::new();
    let mut labels = Vec::new();
    for _ in 0..oxide_sim::UnitKind::Harvester.stats().train_ticks {
        let mut panel = crate::panel::build_for_palette(
            &game.view(),
            &crate::action::BindingMap::classic(),
            false,
        )
        .unwrap();
        if panel.queue.len() < 2 {
            assert!(queue_label_width(&panel, measure) < widths[0]);
            break;
        }
        let width = queue_label_width(&panel, measure);
        assert!(measure(&panel.queue_label) <= width);
        labels.push(panel.queue_label.clone());
        widths.push(width);
        panel.queue_label = "Ready".into();
        assert_eq!(queue_label_width(&panel, measure), width);
        assert!(measure(&panel.queue_label) <= width);
        game.state.tick(&[]);
    }
    assert!(labels.iter().any(|label| label == "10s"));
    assert!(labels.iter().any(|label| label == "9.9s"));
    assert!(widths.iter().all(|width| *width == widths[0]));
    assert_eq!(game.state.building(foundry).unwrap().queue.len(), 1);
}

#[test]
fn fabricator_corner_reaches_wrapped_actions_and_keeps_queue_above_it() {
    let mut scenario = oxide_sim::Scenario::skirmish();
    scenario.buildings.push(oxide_sim::scenario::BuildingSpec {
        player: 0,
        kind: oxide_sim::BuildingKind::Fabricator,
        x: 9,
        y: 3,
    });
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap();
    game.presentation.selection.buildings = vec![
        game.state
            .buildings()
            .iter()
            .find(|b| {
                b.player == game.presentation.human && b.kind == oxide_sim::BuildingKind::Fabricator
            })
            .unwrap()
            .id,
    ];
    let mut panel =
        crate::panel::build_for_palette(&game.view(), &crate::action::BindingMap::classic(), false)
            .unwrap();
    // More cards than one row of the Fabricator's band holds, so its
    // actions wrap.
    for _ in 0..3 {
        let last = panel.cards.last().unwrap();
        let extra = crate::panel::Card {
            icon: last.icon,
            title: last.title.clone(),
            cost: last.cost,
            hotkey: last.hotkey.clone(),
            action: last.action,
            enabled: last.enabled,
            why: last.why.clone(),
            desc: last.desc.clone(),
            progress: last.progress,
        };
        panel.cards.push(extra);
    }
    for (viewport, scale) in [(vec2(1280.0, 800.0), 1.0), (vec2(1920.0, 1200.0), 1.5)] {
        let minimap = minimap_rect_scaled(40, 24, viewport, scale);
        let packing = panel_packing(
            viewport,
            minimap,
            scale,
            panel.cards.len(),
            rally_card_count(&panel.cards),
        );
        let (left, _, _, _) = card_metrics(viewport, scale);
        let measured = measure_info(&panel, left, scale, false, &|_, text: &str, size: f32| {
            text.len() as f32 * size * 0.5
        });
        let (cards, right) = command_card_geometry(viewport, scale, packing, &panel.cards);
        let actions = action_panel_rect(packing, left, right, !panel.cards.is_empty());
        assert!(cards.iter().any(|card| card.y > cards[0].y));
        assert!(measured.height < actions.h);
        let info = selection_info_rect(viewport, left, measured.height, actions);
        assert_eq!(info.y, actions.y);
        assert_eq!(info.bottom(), viewport.y);
        let (dock, slots, count) = queue_grid(8, info.y, scale, 0.0);
        assert_eq!(count, 8);
        assert_eq!(dock.bottom(), info.y);
        assert!(slots[..count].iter().all(|slot| slot.bottom() <= info.y));
    }
}

#[test]
fn production_cards_stay_inside_the_band_across_layouts_and_actions() {
    use crate::action::BindingMap;
    use oxide_sim::{BuildingKind, Command, PlayerCommand, Scenario};

    for kind in BuildingKind::ALL
        .into_iter()
        .filter(|kind| !kind.base_stats().produces.is_empty())
    {
        for full_queue in [false, true] {
            let mut scenario = Scenario::skirmish();
            scenario.players[0].scrap = if full_queue { 10_000 } else { 0 };
            if kind != BuildingKind::Foundry {
                scenario.buildings.push(oxide_sim::scenario::BuildingSpec {
                    player: 0,
                    kind,
                    x: 9,
                    y: 3,
                });
            }
            let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap();
            let building = game
                .state
                .buildings()
                .iter()
                .find(|building| {
                    building.player == game.presentation.human && building.kind == kind
                })
                .unwrap()
                .id;
            game.presentation.selection.buildings = vec![building];
            if full_queue {
                let unit = *kind.base_stats().produces.first().unwrap();
                for _ in 0..oxide_sim::stats::QUEUE_CAP {
                    game.state.tick(&[PlayerCommand {
                        player: game.presentation.human,
                        command: Command::Train {
                            building,
                            kind: unit,
                        },
                    }]);
                }
                assert_eq!(
                    game.state.building(building).unwrap().queue.len(),
                    oxide_sim::stats::QUEUE_CAP
                );
            }
            for rally in [false, true] {
                if rally {
                    game.state.tick(&[PlayerCommand {
                        player: game.presentation.human,
                        command: Command::SetRally {
                            building,
                            rally: Some(oxide_sim::Rally::Tile(chassis::grid::TilePos::new(12, 8))),
                        },
                    }]);
                }
                let panel =
                    crate::panel::build_for_palette(&game.view(), &BindingMap::classic(), false)
                        .unwrap();
                for width in (640..=1920).step_by(17).chain([799, 800, 1280, 1440, 1920]) {
                    for height in [400, 499, 500, 600, 800, 1080] {
                        let viewport = vec2(width as f32, height as f32);
                        for preference in [0.75, 1.0, 1.25, 1.5] {
                            let scale = effective_ui_scale(preference, viewport);
                            let minimap = minimap_rect_scaled(40, 24, viewport, scale);
                            let packing = panel_packing(
                                viewport,
                                minimap,
                                scale,
                                panel.cards.len(),
                                rally_card_count(&panel.cards),
                            );
                            let (slots, right) =
                                command_card_geometry(viewport, scale, packing, &panel.cards);
                            assert_eq!(slots.len(), panel.cards.len());
                            let rally_count = rally_card_count(&panel.cards);
                            if rally_count > 0 {
                                let context = rally_context_rect(slots[0], scale);
                                assert!(context.x >= card_metrics(viewport, scale).0);
                                assert!(context.y >= packing.top && context.bottom() <= viewport.y);
                                assert!(slots.iter().all(|slot| !context.overlaps(slot)));
                                assert!(slots[..rally_count].iter().all(|r| r.y == slots[0].y));
                                let production_x = slots[rally_count].x;
                                assert!(slots[rally_count..].iter().all(|r| r.x >= production_x));
                            }
                            assert!(packing.top >= crate::layout::TOP_BAR_H * scale);
                            for (index, rect) in slots.iter().enumerate() {
                                assert!(
                                    rect.x + rect.w + CARD_RIGHT_INSET * scale <= right + 0.001,
                                    "{kind:?} rally={rally} queue={full_queue} viewport={viewport:?} scale={scale}: card {index} ends at {}, band ends at {right}",
                                    rect.x + rect.w
                                );
                                assert!(
                                    rect.y >= packing.top && rect.y + rect.h <= viewport.y + 0.001
                                );
                                assert!(
                                    rect.w >= crate::theme::MIN_TOUCH_TARGET * scale
                                        && rect.h >= crate::theme::MIN_TOUCH_TARGET * scale
                                );
                                assert!(slots[..index].iter().all(|other| !rect.overlaps(other)));
                                assert!(packing.hides_minimap || !rect.overlaps(&minimap));
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn narrow_cards_keep_short_orders_on_one_line_and_wrap_long_names() {
    let width = |text: &str| text.len() as f32;
    assert_eq!(card_title_lines("Hunt", width, 7.0), ["Hunt"]);
    assert_eq!(card_title_lines("Hunt", width, 11.0), ["Hunt"]);
    assert_eq!(
        card_title_lines("Scuttle Charge", width, 8.0),
        ["Scuttle", "Charge"]
    );
    assert_eq!(
        card_title_lines("Auto-weld: on", width, 10.0),
        ["Auto-weld:", "on"],
        "halves of a hyphenated word that share a line keep no space"
    );
    assert_eq!(
        card_title_lines("Auto-weld: off", width, 5.0),
        ["Auto-", "weld:", "off"]
    );
    assert_eq!(
        card_title_lines("Weld - Sentinel", width, 8.0),
        ["Weld -", "Sentinel"]
    );
}

/// The supported windows from the iPad mini floor up: iPad mini, iPad and
/// iPad Air 11", iPad Pro 11", Steam Deck, iPad Pro 13", and desktop.
const SUPPORTED: [(f32, f32); 6] = [
    (1133.0, 744.0),
    (1180.0, 820.0),
    (1210.0, 834.0),
    (1280.0, 800.0),
    (1376.0, 1032.0),
    (1920.0, 1080.0),
];

fn command(action: crate::panel::CardAction) -> crate::panel::Card {
    crate::panel::Card {
        icon: crate::panel::CardIcon::Verb(crate::panel::VerbIcon::Move),
        title: String::new(),
        cost: None,
        hotkey: String::new(),
        action,
        enabled: true,
        why: None,
        desc: Vec::new(),
        progress: None,
    }
}

#[test]
fn a_carrying_harvester_and_a_full_factory_fill_one_row_at_supported_sizes() {
    use crate::action::Action;
    use crate::panel::CardAction;

    let verbs: Vec<_> = (0..8)
        .map(|_| command(CardAction::Dispatch(Action::Patrol)))
        .collect();
    let mut factory = vec![
        command(CardAction::ArmRally),
        command(CardAction::ClearRally),
    ];
    factory.extend((0..5).map(|slot| command(CardAction::Dispatch(Action::TrainSlot(slot)))));
    for (w, h) in SUPPORTED {
        let viewport = vec2(w, h);
        let minimap = minimap_rect_scaled(40, 24, viewport, 1.0);
        for cards in [&verbs, &factory] {
            let packing =
                panel_packing(viewport, minimap, 1.0, cards.len(), rally_card_count(cards));
            let (slots, right) = command_card_geometry(viewport, 1.0, packing, cards);
            assert!(!packing.hides_minimap, "{w}x{h}");
            assert!(right <= minimap.x, "{w}x{h}: band ends at {right}");
            assert!(
                slots.iter().all(|slot| slot.y == slots[0].y),
                "{} cards wrap at {w}x{h}",
                cards.len()
            );
        }
        // Return cargo arrives last and moves no card already shown.
        let packing = panel_packing(viewport, minimap, 1.0, 7, 0);
        let (seven, _) = command_card_geometry(viewport, 1.0, packing, &verbs[..7]);
        let packing = panel_packing(viewport, minimap, 1.0, 8, 0);
        let (eight, _) = command_card_geometry(viewport, 1.0, packing, &verbs);
        assert_eq!(seven[..], eight[..7], "{w}x{h}");
    }
}

#[test]
fn the_construction_palette_groups_categories_beside_back_at_supported_sizes() {
    let categories = &crate::action::BUILD_CATEGORIES;
    let count = categories
        .iter()
        .map(|(_, kinds)| kinds.len())
        .sum::<usize>()
        + 1;
    for ((w, h), touch_only) in SUPPORTED
        .into_iter()
        .flat_map(|size| [(size, false), (size, true)])
    {
        let viewport = vec2(w, h);
        let minimap = minimap_rect_scaled(36, 24, viewport, 1.0);
        let (band, slots, grouped) = catalog_geometry(viewport, 1.0, minimap, count, touch_only);
        assert!(grouped, "{w}x{h}");
        assert_eq!(slots.len(), count);
        assert!(band.y >= crate::layout::TOP_BAR_H);
        assert!(band.right() <= minimap.x, "{w}x{h}");
        for (i, rect) in slots.iter().enumerate() {
            let min_w = if i + 1 == count {
                crate::theme::MIN_TOUCH_TARGET
            } else {
                86.0
            };
            assert!(rect.w >= min_w && rect.h >= crate::theme::MIN_TOUCH_TARGET);
            assert!(rect.x >= band.x && rect.right() <= band.right());
            assert!(rect.y >= band.y && rect.bottom() <= viewport.y);
            assert!(slots[..i].iter().all(|other| !rect.overlaps(other)));
        }
        let back = slots[count - 1];
        assert!(slots.iter().all(|slot| slot.x >= back.x), "Back leads");
        if touch_only {
            assert!(
                back.x >= crate::layout::TOUCH_CORNER_INSET,
                "Back clears a rounded screen corner at {w}x{h}"
            );
            let (_, desktop, _) = catalog_geometry(viewport, 1.0, minimap, count, false);
            assert_eq!(
                slots[..count - 1],
                desktop[..count - 1],
                "only Back moves at {w}x{h}"
            );
            assert_eq!(back.right(), desktop[count - 1].right());
        }
        assert_eq!(back.y, slots[0].y, "Back starts on the first row");
        assert_eq!(back.bottom(), slots[1].bottom(), "and ends on the second");
        let mut first = 0;
        let mut previous_right = back.right();
        for (name, kinds) in categories {
            let cells = &slots[first..first + kinds.len()];
            first += kinds.len();
            let left = cells
                .iter()
                .map(|cell| cell.x)
                .fold(f32::INFINITY, f32::min);
            assert!(
                left > previous_right,
                "{name} sits right of what precedes it"
            );
            previous_right = cells.iter().map(Rect::right).fold(0.0, f32::max);
            assert!(
                cells
                    .iter()
                    .all(|cell| cell.y == slots[0].y || cell.y == slots[1].y),
                "{name} fills the palette's two rows"
            );
        }
    }
}

#[test]
fn a_window_below_the_floor_keeps_every_palette_choice_in_a_flat_grid() {
    let viewport = vec2(640.0, 400.0);
    let minimap = minimap_rect_scaled(36, 24, viewport, 1.0);
    let (band, slots, grouped) = catalog_geometry(viewport, 1.0, minimap, 16, false);
    assert!(!grouped);
    assert_eq!(slots.len(), 16);
    for (i, rect) in slots.iter().enumerate() {
        assert!(band.contains(vec2(rect.x, rect.y)));
        assert!(slots[..i].iter().all(|other| !rect.overlaps(other)));
    }
}

#[test]
fn an_info_column_a_sliver_taller_than_the_band_shares_its_top() {
    let viewport = vec2(1133.0, 744.0);
    let minimap = minimap_rect_scaled(40, 24, viewport, 1.0);
    let packing = panel_packing(viewport, minimap, 1.0, 7, 2);
    let flush = flush_band(packing, packing.band_h + 2.0, viewport.y, 1.0);
    assert_eq!(flush.band_h, packing.band_h + 2.0);
    assert_eq!(flush.top, viewport.y - flush.band_h);
    let (left, ..) = card_metrics(viewport, 1.0);
    let (_, right) = command_card_geometry(viewport, 1.0, flush, &[]);
    let actions = action_panel_rect(flush, left, right, true);
    let info = selection_info_rect(viewport, left, packing.band_h + 2.0, actions);
    assert_eq!(info.y, actions.y);
    for info_height in [packing.band_h - 2.0, packing.band_h + 40.0] {
        assert_eq!(flush_band(packing, info_height, viewport.y, 1.0), packing);
    }
}

#[test]
fn rally_changes_preserve_single_and_grouped_production_geometry() {
    use crate::action::BindingMap;
    use oxide_sim::{BuildingKind, Command, PlayerCommand, Scenario};

    for kind in BuildingKind::ALL
        .into_iter()
        .filter(|kind| !kind.base_stats().produces.is_empty())
    {
        let mut scenario = Scenario::skirmish();
        for x in [9, 14] {
            scenario.buildings.push(oxide_sim::scenario::BuildingSpec {
                player: 0,
                kind,
                x,
                y: 3,
            });
        }
        let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap();
        let producers: Vec<_> = game
            .state
            .buildings()
            .iter()
            .filter(|b| b.player == game.presentation.human && b.kind == kind)
            .map(|b| b.id)
            .collect();
        for selection in [
            vec![producers[0]],
            producers.clone(),
            game.state
                .buildings()
                .iter()
                .filter(|b| b.player == game.presentation.human)
                .map(|b| b.id)
                .collect(),
        ] {
            game.presentation.selection.buildings = selection.clone();
            let geometry = |game: &Game| {
                let panel =
                    crate::panel::build_for_palette(&game.view(), &BindingMap::classic(), false)
                        .unwrap();
                assert_eq!(rally_card_count(&panel.cards), 2);
                let mut result = Vec::new();
                for viewport in [
                    vec2(640.0, 400.0),
                    vec2(800.0, 600.0),
                    vec2(1280.0, 800.0),
                    vec2(1920.0, 1080.0),
                ] {
                    for preference in [0.75, 1.0, 1.25, 1.5] {
                        let scale = effective_ui_scale(preference, viewport);
                        let minimap = minimap_rect_scaled(40, 24, viewport, scale);
                        let packing = panel_packing(viewport, minimap, scale, panel.cards.len(), 2);
                        let (slots, right) =
                            command_card_geometry(viewport, scale, packing, &panel.cards);
                        assert_eq!(slots[0].y, slots[1].y);
                        assert!(slots[1].x >= slots[0].right());
                        if viewport == vec2(1920.0, 1080.0) && preference == 1.0 {
                            assert_eq!(packing.band_h, 92.0);
                        }
                        result.push((packing, slots, right));
                    }
                }
                result
            };
            let baseline = geometry(&game);
            // Partial and shared rallies exercise both grouped panel states.
            for ids in [vec![selection[0]], selection.clone()] {
                for rally in [
                    Some(oxide_sim::Rally::Tile(chassis::grid::TilePos::new(20, 10))),
                    None,
                ] {
                    let commands: Vec<_> = ids
                        .iter()
                        .map(|id| PlayerCommand {
                            player: game.presentation.human,
                            command: Command::SetRally {
                                building: *id,
                                rally,
                            },
                        })
                        .collect();
                    game.state.tick(&commands);
                    assert_eq!(geometry(&game), baseline);
                    let panel = crate::panel::build_for_palette(
                        &game.view(),
                        &BindingMap::classic(),
                        false,
                    )
                    .unwrap();
                    assert_eq!(panel.cards[1].enabled, rally.is_some());
                }
            }
        }
    }
}

#[test]
fn production_wraps_in_its_own_columns_beside_rally_controls() {
    assert_eq!(grouped_card_rows(7, 1, 6), 2);
    assert_eq!(grouped_card_slot(1, 1, 6), (0, 1, true));
    assert_eq!(grouped_card_slot(6, 1, 6), (1, 1, true));
    assert_eq!(grouped_card_rows(8, 2, 6), 2);
    assert_eq!(grouped_card_slot(0, 2, 6), (0, 0, false));
    assert_eq!(grouped_card_slot(1, 2, 6), (0, 0, false));
    assert_eq!(grouped_card_slot(2, 2, 6), (0, 1, true));
    assert_eq!(grouped_card_slot(7, 2, 6), (1, 1, true));
    assert_eq!(grouped_card_gap(7, 2, 5, 400.0, 66.0, 6.0), 10.0);
    assert_eq!(grouped_card_gap(7, 2, 5, 354.0, 66.0, 6.0), 0.0);

    assert_eq!(grouped_card_rows(7, 0, 5), 2);
    assert_eq!(grouped_card_slot(5, 0, 5), (1, 0, false));
    assert_eq!(grouped_card_rows(7, 2, 1), 6);
    assert_eq!(grouped_card_slot(2, 2, 1), (1, 0, false));
}

#[test]
fn eight_paid_slots_form_a_fully_clickable_small_window_dock() {
    // A 120px command band leaves panel_top=280 in the
    // 640×400 stress case. Every slot must remain present, above the band,
    // and below the top bar rather than folding into "+N".
    let (dock, slots, count) = queue_grid(8, 280.0, 1.0, 0.0);
    assert_eq!(count, 8);
    assert!(dock.y >= crate::layout::TOP_BAR_H);
    assert_eq!(
        slots[..count]
            .iter()
            .map(|slot| slot.x.to_bits())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        2,
        "the full queue uses two columns"
    );
    assert_eq!(
        slots[..count]
            .iter()
            .map(|slot| slot.y.to_bits())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        4,
        "the full queue uses four rows"
    );
    for slot in &slots[..count] {
        assert!(slot.x >= dock.x && slot.x + slot.w <= dock.x + dock.w);
        assert!(slot.y >= dock.y && slot.y + slot.h <= dock.y + dock.h);
        assert!(slot.y + slot.h <= 280.0);
    }
}

#[test]
fn the_stop_plate_sits_above_the_label_and_every_slot() {
    let header = STOP_PLATE + STOP_GAP;
    for (len, top) in [(3, 680.0), (8, 280.0)] {
        let (plain, _, _) = queue_grid(len, top, 1.0, 0.0);
        let (dock, slots, count) = queue_grid(len, top, 1.0, header);
        assert_eq!(count, len, "the square never folds a slot away");
        assert!(dock.y >= crate::layout::TOP_BAR_H);
        assert_eq!(dock.bottom(), top, "the dock still rests on the band");
        assert!(dock.h >= plain.h, "the square adds a row, not an overlap");
        for slot in &slots[..count] {
            assert!(
                slot.y >= dock.y + header + 24.0,
                "below the square and label"
            );
            assert!(slot.bottom() <= top);
        }
    }
}

#[test]
fn a_queue_that_fits_stays_in_one_quiet_column() {
    let (_, slots, count) = queue_grid(5, 680.0, 1.0, 0.0);
    assert_eq!(count, 5);
    assert!(slots[..count].iter().all(|slot| slot.x == slots[0].x));
}

#[test]
fn a_dense_small_window_panel_yields_the_minimap_before_overflowing() {
    let viewport = vec2(640.0, 400.0);
    let minimap = minimap_rect_scaled(40, 24, viewport, 1.0);
    let packing = panel_packing(viewport, minimap, 1.0, 16, 0);

    assert!(packing.hides_minimap);
    assert_eq!(packing.right, viewport.x);
    assert!(packing.top >= crate::layout::TOP_BAR_H);
    assert!(packing.top + packing.band_h <= viewport.y);
}

#[test]
fn a_mixed_small_window_selection_keeps_roster_and_queue_in_the_corner() {
    let mut scenario = oxide_sim::Scenario::skirmish();
    scenario.units.clear();
    for (index, kind) in [
        oxide_sim::UnitKind::Sentinel,
        oxide_sim::UnitKind::Harvester,
        oxide_sim::UnitKind::Excavator,
        oxide_sim::UnitKind::Skyhook,
        oxide_sim::UnitKind::Scuttler,
        oxide_sim::UnitKind::Lancer,
        oxide_sim::UnitKind::Bombard,
        oxide_sim::UnitKind::Kestrel,
    ]
    .into_iter()
    .enumerate()
    {
        scenario.units.push(oxide_sim::scenario::UnitSpec {
            player: 0,
            kind,
            x: 6 + index.fit::<i32>(),
            y: 5,
        });
    }
    let mut game = Game::with_viewport(scenario, vec2(640.0, 400.0)).unwrap();
    game.presentation.selection.units = game.state.units().iter().map(|u| u.id).collect();
    let panel =
        crate::panel::build_for_palette(&game.view(), &crate::action::BindingMap::classic(), false)
            .unwrap();
    let info = measure_info(&panel, 204.0, 1.0, true, &|_, text: &str, size: f32| {
        text.len() as f32 * size * 0.5
    });
    assert_eq!(panel.roster.len(), 8);
    assert_eq!(info.roster_columns, 4);
    let (dock, slots, count) = queue_grid(8, 400.0 - info.height, 1.0, 0.0);
    assert_eq!(count, 8);
    assert!(dock.y >= crate::layout::TOP_BAR_H);
    assert!(dock.w <= 204.0);
    assert!(
        slots[..count]
            .iter()
            .all(|slot| slot.y >= crate::layout::TOP_BAR_H)
    );
}

#[test]
fn a_simple_panel_keeps_the_minimap() {
    let viewport = vec2(640.0, 400.0);
    let minimap = minimap_rect_scaled(40, 24, viewport, 1.0);
    let packing = panel_packing(viewport, minimap, 1.0, 2, 0);

    assert!(!packing.hides_minimap);
    assert!(packing.right < viewport.x);
}

#[test]
fn a_commandless_building_does_not_reserve_an_empty_card_row() {
    let viewport = vec2(1280.0, 800.0);
    let minimap = minimap_rect_scaled(40, 24, viewport, 1.0);
    let packing = panel_packing(viewport, minimap, 1.0, 0, 0);
    let actions = action_panel_rect(packing, 228.0, 228.0, false);
    assert_eq!(actions.w, 0.0);
    assert_eq!(actions.h, 0.0);
    let info = selection_info_rect(viewport, 228.0, 218.0, actions);
    assert_eq!(info.y, 582.0);
}

#[test]
fn cards_light_under_the_mouse_only_while_it_is_the_pointer_in_use() {
    let mut input = InputState::new();
    let rect = Rect::new(0.0, 0.0, 50.0, 50.0);
    input.mouse = vec2(10.0, 10.0);
    input.last_pointer = crate::input::Pointer::Mouse;
    assert!(hovers(&input, rect));
    input.last_pointer = crate::input::Pointer::Touch;
    assert!(
        !hovers(&input, rect),
        "a finger leaves the mouse point stale"
    );
}

#[test]
fn a_tile_icon_drops_below_a_hotkey_that_reaches_its_column() {
    let tile = Rect::new(100.0, 600.0, 84.0, 72.0);
    let letter = Rect::new(104.0, 603.0, 9.0, 10.0);
    assert_eq!(tile_icon_floor(tile, 22.0, None, 1.0), 604.0);
    assert_eq!(tile_icon_floor(tile, 22.0, Some(letter), 1.0), 604.0);
    let chord = Rect::new(104.0, 603.0, 45.0, 10.0);
    let floor = tile_icon_floor(tile, 22.0, Some(chord), 1.0);
    assert!(floor > chord.bottom(), "the icon clears Shift+O");
}
