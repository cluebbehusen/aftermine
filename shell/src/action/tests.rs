use super::*;

impl BindingMap {
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }
}

impl BindingMap {
    pub(crate) fn conflicts(&self) -> Vec<(Binding, Binding)> {
        let rows: Vec<_> = self.rows().copied().collect();
        let mut result = Vec::new();
        for (i, a) in rows.iter().enumerate() {
            for b in rows.iter().skip(i + 1) {
                if a.chord == b.chord && a.action.overlaps(b.action) {
                    result.push((*a, *b));
                }
            }
        }
        result
    }
}

#[test]
fn exact_chords_beat_bare_ones_and_bare_survives_modifiers() {
    let map = BindingMap::classic();
    assert_eq!(
        map.resolve_in(Key::Num1, true, false, Context::Units),
        Some(Action::AssignGroup(1)),
        "ctrl+1 is its own meaning"
    );
    assert_eq!(
        map.resolve_in(Key::Num1, false, false, Context::Units),
        Some(Action::Slot(1))
    );
    assert_eq!(
        map.resolve_in(Key::Q, true, false, Context::Production),
        Some(Action::TrainSlot(0)),
        "a held modifier never mutes an unmodified binding"
    );
}

#[test]
fn rebinding_refuses_collisions_and_moves_the_chord() {
    let mut map = BindingMap::classic();
    assert!(
        !map.rebind(Action::StopOrScrap, Chord::bare(Key::G)),
        "G belongs to Run in the unit context"
    );
    assert!(map.rebind(Action::StopOrScrap, Chord::ctrl(Key::X)));
    assert_eq!(
        map.resolve_in(Key::X, true, false, Context::Units),
        Some(Action::StopOrScrap)
    );
    assert_eq!(
        map.resolve_in(Key::X, false, false, Context::Units),
        None,
        "the old chord is gone, not shadowed"
    );
}

#[test]
fn no_classic_chord_points_past_the_group_count() {
    let map = BindingMap::classic();
    assert!(
        map.bindings().iter().all(|b| match b.action {
            Action::AssignGroup(n) | Action::Slot(n) => (n as usize) <= CONTROL_GROUPS,
            _ => true,
        }),
        "no chord may point at a group dispatch ignores"
    );
}

#[test]
fn shift_never_flips_an_assign_into_a_recall() {
    // Shift lingers after queueing orders; Ctrl+Shift+digit must
    // still mean assign.
    let map = BindingMap::classic();
    assert_eq!(
        map.resolve_in(Key::Num4, true, true, Context::Units),
        Some(Action::AssignGroup(4))
    );
}

#[test]
fn releases_pair_with_what_the_press_meant() {
    let map = BindingMap::classic();
    let mut r = ActionResolver::default();
    assert_eq!(
        r.press(&map, Key::Ctrl, Context::Units),
        None,
        "modifiers are truth, not actions"
    );
    assert_eq!(
        r.press(&map, Key::Num2, Context::Units),
        Some(Action::AssignGroup(2))
    );
    // Ctrl comes up before the digit: the release still closes the
    // assign, not a phantom recall.
    r.release(Key::Ctrl);
    assert!(r.is_held(Action::AssignGroup(2)));
    r.release(Key::Num2);
    assert!(!r.is_held(Action::AssignGroup(2)));
}

#[test]
fn held_pans_read_back_until_released() {
    let map = BindingMap::classic();
    let mut r = ActionResolver::default();
    r.press(&map, Key::Left, Context::Units);
    assert!(r.is_held(Action::PanLeft));
    r.release(Key::Left);
    assert!(!r.is_held(Action::PanLeft));
}

#[test]
fn clear_forgets_holds_and_modifiers() {
    let map = BindingMap::classic();
    let mut r = ActionResolver::default();
    r.press(&map, Key::Ctrl, Context::Units);
    r.press(&map, Key::Up, Context::Units);
    r.clear();
    assert!(!r.is_held(Action::PanUp));
    assert_eq!(
        r.press(&map, Key::Num3, Context::Units),
        Some(Action::Slot(3)),
        "ctrl must not survive a clear"
    );
}
#[test]
fn contexts_allow_panel_reuse_but_reserve_camera_and_group_keys() {
    let mut map = BindingMap::classic();
    assert!(map.valid());
    assert!(BindingMap::left_handed().valid());
    assert_eq!(
        map.resolve_in(Key::R, false, false, Context::Units),
        Some(Action::Patrol)
    );
    assert_eq!(
        map.resolve_in(Key::R, false, false, Context::Production),
        Some(Action::TrainSlot(2))
    );
    assert_eq!(
        map.resolve_in(Key::R, false, false, Context::Construction),
        Some(Action::BuildCategory(2))
    );
    assert!(!map.rebind(Action::Build(BuildingKind::Turret), Chord::bare(Key::W)));
    assert!(!map.rebind(Action::TrainSlot(0), Chord::bare(Key::Num1)));
    assert!(map.rebind(Action::Build(BuildingKind::Turret), Chord::ctrl(Key::G)));
    assert_eq!(
        map.resolve_in(Key::G, true, true, Context::BuildCategory(2)),
        Some(Action::Build(BuildingKind::Turret))
    );
    assert_eq!(
        map.resolve_in(Key::G, true, true, Context::BuildCategory(0)),
        Some(Action::Run)
    );
}

#[test]
fn releasing_one_pan_alias_or_changing_context_does_not_release_the_other() {
    let map = BindingMap::classic();
    let mut resolver = ActionResolver::default();
    resolver.press(&map, Key::W, Context::Units);
    resolver.press(&map, Key::Up, Context::Units);
    resolver.press(&map, Key::Shift, Context::Construction);
    resolver.release(Key::W);
    assert!(resolver.is_held(Action::PanUp));
    resolver.release(Key::Up);
    assert!(!resolver.is_held(Action::PanUp));
    resolver.press(&map, Key::W, Context::Playback);
    resolver.clear();
    assert!(!resolver.is_held(Action::PanUp));
}

#[test]
fn malformed_secondary_rows_and_unknown_slots_are_rejected() {
    let mut map = BindingMap::classic();
    map.secondary.push(Binding {
        action: Action::Patrol,
        chord: Chord::bare(Key::W),
    });
    assert!(!map.valid());
    let mut map = BindingMap::classic();
    map.bindings.push(Binding {
        action: Action::BuildCategory(9),
        chord: Chord::bare(Key::G),
    });
    assert!(!map.valid());
}

#[test]
fn rebound_menu_keys_emit_only_their_contextual_navigation() {
    use aftermine_protocol::RawEvent;
    let mut map = BindingMap::classic();
    assert!(map.rebind(Action::Confirm, Chord::bare(Key::Z)));
    let events = map.menu_events(
        &[
            RawEvent::KeyDown { key: Key::Z },
            RawEvent::KeyDown { key: Key::W },
            RawEvent::Text { ch: 'z' },
        ],
        false,
        false,
    );
    assert_eq!(
        events,
        vec![
            RawEvent::KeyDown { key: Key::Enter },
            RawEvent::Text { ch: 'z' }
        ]
    );
}

#[test]
fn tab_keys_reach_menus_and_stay_left_then_right_on_either_hand() {
    use aftermine_protocol::RawEvent;
    let tabs = |map: &BindingMap, keys: [Key; 2]| {
        map.menu_events(&keys.map(|key| RawEvent::KeyDown { key }), false, false)
    };
    let canonical = vec![
        RawEvent::KeyDown { key: Key::Q },
        RawEvent::KeyDown { key: Key::E },
    ];
    assert_eq!(tabs(&BindingMap::classic(), [Key::Q, Key::E]), canonical);
    assert_eq!(
        tabs(&BindingMap::left_handed(), [Key::U, Key::O]),
        canonical,
        "on the right hand U, the left key, is still Previous"
    );
}

#[test]
fn unbound_tab_actions_keep_their_classic_keys_while_menus_leave_them_free() {
    use aftermine_protocol::RawEvent;
    let mut map = BindingMap::classic();
    map.unbind_slot(Action::PrevTab, 0);
    map.unbind_slot(Action::NextTab, 0);
    let menu = |map: &BindingMap| {
        map.menu_events(
            &[
                RawEvent::KeyDown { key: Key::Q },
                RawEvent::KeyDown { key: Key::E },
            ],
            false,
            false,
        )
    };
    assert_eq!(
        menu(&map),
        vec![
            RawEvent::KeyDown { key: Key::Q },
            RawEvent::KeyDown { key: Key::E },
        ],
        "the keyboard can still reach the Controls tab to bind them again"
    );
    assert_eq!(map.tab_chord(Action::PrevTab), Some(Chord::bare(Key::Q)));
    assert!(map.rebind(Action::MenuUp, Chord::bare(Key::Q)));
    assert_eq!(
        menu(&map),
        vec![
            RawEvent::KeyDown { key: Key::Up },
            RawEvent::KeyDown { key: Key::E },
        ],
        "a key another menu action claims means that action"
    );
    assert_eq!(map.tab_chord(Action::PrevTab), None);
}

/// A packaged copy hides the debug overlay from Controls, so its key must
/// not stay reserved by a row the player can neither see nor clear.
#[test]
fn a_packaged_copy_leaves_the_overlay_key_free() {
    let f1 = Chord::bare(Key::F1);
    let mut workspace = BindingMap::classic();
    workspace.keep_offered_in(false);
    assert_eq!(workspace, BindingMap::classic());
    assert_eq!(
        workspace.conflict(Action::HomeCamera, 0, f1),
        Some(Action::ToggleOverlay)
    );

    let mut packaged = BindingMap::classic();
    packaged.keep_offered_in(true);
    assert_eq!(packaged.chord_for(Action::ToggleOverlay), None);
    assert_eq!(
        packaged.resolve_in(Key::F1, false, false, Context::Empty),
        None
    );
    assert!(packaged.rebind(Action::HomeCamera, f1));
    assert_eq!(
        packaged.resolve_in(Key::F1, false, false, Context::Empty),
        Some(Action::HomeCamera)
    );
    assert!(packaged.valid());
    assert!(!Action::ToggleOverlay.offered_in(true));
    assert!(Action::ToggleOverlay.offered_in(false));
}

#[test]
fn ctrl_a_and_ctrl_s_select_the_army_while_bare_a_and_s_still_pan() {
    let map = BindingMap::classic();
    for context in [
        Context::Empty,
        Context::Units,
        Context::Production,
        Context::Construction,
    ] {
        assert_eq!(
            map.resolve_in(Key::A, true, false, context),
            Some(Action::SelectArmy)
        );
        assert_eq!(
            map.resolve_in(Key::S, true, false, context),
            Some(Action::SelectArmyInView)
        );
        assert_eq!(
            map.resolve_in(Key::A, false, false, context),
            Some(Action::PanLeft)
        );
    }
    let left = BindingMap::left_handed();
    assert!(left.valid());
    assert_eq!(
        left.chord_for(Action::SelectArmy),
        Some(Chord::ctrl(Key::J))
    );
    assert_eq!(
        left.chord_for(Action::SelectArmyInView),
        Some(Chord::ctrl(Key::K))
    );
}

#[test]
fn controller_buttons_reach_menus_as_the_canonical_keys_whatever_the_bindings() {
    use aftermine_protocol::{PadButton, RawEvent};
    // The frame runs key bindings first, then the pads' fixed layout, so
    // a remapped key never moves a button and a button's key is never
    // bound again.
    let menu = |map: &BindingMap, events: &[RawEvent]| {
        crate::pad::menu_keys(&map.menu_events(events, false, false), true)
    };
    let events = [
        RawEvent::PadDown {
            pad: 0,
            button: PadButton::South,
        },
        RawEvent::PadDown {
            pad: 0,
            button: PadButton::DpadDown,
        },
    ];
    let canonical = vec![
        RawEvent::KeyDown { key: Key::Enter },
        RawEvent::KeyDown { key: Key::Down },
    ];
    assert_eq!(menu(&BindingMap::classic(), &events), canonical);
    let mut map = BindingMap::classic();
    assert!(map.rebind(Action::Confirm, Chord::bare(Key::Z)));
    assert!(map.rebind(Action::MenuDown, Chord::bare(Key::S)));
    assert_eq!(menu(&map, &events), canonical);
    assert_eq!(
        map.menu_events(&events, false, false),
        events,
        "key bindings pass controller buttons through untouched"
    );
}
