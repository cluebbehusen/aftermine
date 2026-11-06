use super::*;
use crate::numeric;
use crate::numeric::Fit;
use aftermine_sim::Scenario;
use aftermine_sim::scenario::PlayerSpec;
use aftermine_sim::scenario::ScenarioMode;

fn scenario(count: usize, team: impl Fn(usize) -> Option<u8>) -> Scenario {
    let mut map = vec![vec!['.'; 70]; 70];
    for (seat, anchor) in "12345678abcdefgh".chars().take(count).enumerate() {
        map[3 + (seat / 4) * 16][3 + (seat % 4) * 16] = anchor;
    }
    Scenario {
        levels: Vec::new(),
        mode: ScenarioMode::Match,
        name: "Seat presentation".into(),
        map: map
            .into_iter()
            .map(|row| row.into_iter().collect())
            .collect(),
        players: (0..count)
            .map(|seat| PlayerSpec {
                name: format!("Seat {seat}"),
                team: team(seat),
                scrap: 0,
                bot: seat != 0,
                bot_config: None,
            })
            .collect(),
        units: vec![],
        buildings: vec![],
        meta: None,
    }
}

fn rgb(color: Color) -> (u8, u8, u8) {
    (
        numeric::to_u8((color.r * 255.0).round()),
        numeric::to_u8((color.g * 255.0).round()),
        numeric::to_u8((color.b * 255.0).round()),
    )
}

#[test]
fn every_supported_ffa_seat_has_a_distinct_hostile_identity_for_every_viewer() {
    for count in 2..=MAX_PLAYERS {
        let state = scenario(count, |_| None).build().unwrap();
        for viewer in 0..count {
            let styles = SeatStyles::new(&state, PlayerId(viewer.fit::<u8>()), false);
            let mut colors = Vec::new();
            for owner in 0..count {
                let seat = PlayerId(owner.fit::<u8>());
                let style = styles.get(seat);
                let cue = AllegianceCue::of(&state, PlayerId(viewer.fit::<u8>()), seat);
                if owner == viewer {
                    assert_eq!(cue, AllegianceCue::Mine);
                    assert_eq!(style, self_color(false));
                } else {
                    assert_eq!(cue, AllegianceCue::Hostile);
                    let color = rgb(style);
                    assert!(
                        !colors.contains(&color),
                        "seat {owner} repeats an identity for viewer {viewer}"
                    );
                    colors.push(color);
                }
            }
        }
    }
}

#[test]
fn large_allied_team_preserves_identity_families_and_refreshes_the_viewer() {
    let scenario = scenario(MAX_PLAYERS, |seat| Some(u8::from(seat == MAX_PLAYERS - 1)));
    let state = scenario.build().unwrap();
    let allied = SeatStyles::new(&state, PlayerId(0), false);
    let enemy = SeatStyles::new(&state, PlayerId(15), false);
    let mut colors = Vec::new();
    for owner in 1..15 {
        let color = allied.get(PlayerId(owner));
        assert_eq!(
            AllegianceCue::of(&state, PlayerId(0), PlayerId(owner)),
            AllegianceCue::Ally
        );
        assert!(!colors.contains(&rgb(color)));
        colors.push(rgb(color));
        assert!(color.b > color.r, "allies stay cool");
        assert_eq!(
            AllegianceCue::of(&state, PlayerId(15), PlayerId(owner)),
            AllegianceCue::Hostile
        );
    }
    assert_eq!(enemy.get(PlayerId(15)), self_color(false));
    assert_eq!(rgb(allied.get(PlayerId(1))), (110, 190, 250));
    assert_eq!(rgb(enemy.get(PlayerId(0))), (228, 44, 58));
}

#[test]
fn team_colors_give_each_relationship_one_color() {
    let state = scenario(8, |seat| Some(u8::from(seat >= 4)))
        .build()
        .unwrap();
    let styles = SeatStyles::new(&state, PlayerId(0), true);
    assert_eq!(styles.get(PlayerId(0)), self_color(true));
    for ally in 1..4 {
        assert_eq!(styles.get(PlayerId(ally)), TEAM_ALLY);
    }
    for enemy in 4..8 {
        assert_eq!(styles.get(PlayerId(enemy)), TEAM_ENEMY);
    }
    assert!(
        chroma(self_color(true)) < 0.05,
        "the self color stays achromatic"
    );
}

#[test]
fn live_and_replay_views_share_prepared_styles_after_viewer_changes() {
    let scenario = scenario(MAX_PLAYERS, |seat| Some((seat / 8).fit::<u8>()));
    let viewport = macroquad::prelude::vec2(1100.0, 720.0);
    let mut game = crate::game::Game::with_viewport(scenario.clone(), viewport).unwrap();
    let mut playback = crate::screens::playback::PlaybackSession::from_replay(
        aftermine_kit::GameReplay::new(aftermine_sim::SIM_VERSION, "test", scenario),
    )
    .unwrap();
    for viewer in [0, 8, 15] {
        game.presentation.human = PlayerId(viewer);
        playback.presentation.human = PlayerId(viewer);
        let live = game.view();
        let replay = playback.view();
        for owner in 0..16 {
            let owner = PlayerId(owner);
            assert_eq!(
                crate::render::seat_identity_color(&live, owner),
                crate::render::seat_identity_color(&replay, owner)
            );
            assert_eq!(
                crate::render::seat_identity_color(&live, owner) == crate::render::self_color(),
                owner == PlayerId(viewer)
            );
        }
    }
}

fn hue(color: Color) -> f32 {
    let (r, g, b) = (color.r, color.g, color.b);
    let max = r.max(g).max(b);
    let chroma = max - r.min(g).min(b);
    let sector = if max == r {
        ((g - b) / chroma).rem_euclid(6.0)
    } else if max == g {
        (b - r) / chroma + 2.0
    } else {
        (r - g) / chroma + 4.0
    };
    sector * 60.0
}

fn chroma(color: Color) -> f32 {
    color.r.max(color.g).max(color.b) - color.r.min(color.g).min(color.b)
}

#[test]
fn each_enemy_team_wears_one_family_in_order_of_its_first_seat() {
    let teams = [0, 1, 2, 0, 1, 2];
    let styles = SeatStyles::from_teams(&teams, 0, false);
    assert_eq!(styles.get(PlayerId(3)), ALLIES[0]);
    assert_eq!(styles.get(PlayerId(1)), REDS[0]);
    assert_eq!(styles.get(PlayerId(4)), REDS[1]);
    assert_eq!(styles.get(PlayerId(2)), YELLOWS[0]);
    assert_eq!(styles.get(PlayerId(5)), YELLOWS[1]);
}

#[test]
fn every_enemy_of_an_eight_seat_free_for_all_has_its_own_hue() {
    let enemies = WITHOUT_ALLIES.map(|family| family[0]);
    for (rank, enemy) in enemies[..7].iter().enumerate() {
        for other in &enemies[rank + 1..7] {
            let apart = (hue(*enemy) - hue(*other)).abs();
            assert!(
                apart.min(360.0 - apart) >= 20.0,
                "{:?} and {:?} share a hue",
                rgb(*enemy),
                rgb(*other)
            );
        }
    }
}

#[test]
fn the_self_color_never_reads_as_a_friend_or_a_foe() {
    let mine = self_color(false);
    for color in ALLIES
        .into_iter()
        .chain(WITHOUT_ALLIES.into_iter().flatten())
    {
        let apart = (hue(mine) - hue(color)).abs();
        assert!(
            apart.min(360.0 - apart) >= 30.0,
            "{:?} sits within 30 degrees of the self color",
            rgb(color)
        );
    }
}

/// A seat color as `OKLab` under simulated red-weak vision (Machado 2009), at the
/// half brightness structure panels draw it.
fn red_weak_oklab(color: Color) -> [f32; 3] {
    let linear = |channel: f32| {
        let drawn = channel * 0.5;
        if drawn <= 0.040_45 {
            drawn / 12.92
        } else {
            ((drawn + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (linear(color.r), linear(color.g), linear(color.b));
    let (r, g, b) = (
        (0.152_286 * r + 1.052_583 * g - 0.204_868 * b).clamp(0.0, 1.0),
        (0.114_503 * r + 0.786_281 * g + 0.099_216 * b).clamp(0.0, 1.0),
        (-0.003_882 * r - 0.048_116 * g + 1.051_998 * b).clamp(0.0, 1.0),
    );
    let long = (0.412_221 * r + 0.536_333 * g + 0.051_446 * b).cbrt();
    let medium = (0.211_903 * r + 0.680_700 * g + 0.107_397 * b).cbrt();
    let short = (0.088_302 * r + 0.281_719 * g + 0.629_979 * b).cbrt();
    [
        0.210_454 * long + 0.793_618 * medium - 0.004_072 * short,
        1.977_998 * long - 2.428_592 * medium + 0.450_594 * short,
        0.025_904 * long + 0.782_772 * medium - 0.808_676 * short,
    ]
}

fn red_weak_distance(a: Color, b: Color) -> f32 {
    let (a, b) = (red_weak_oklab(a), red_weak_oklab(b));
    let squares: f32 = a.iter().zip(&b).map(|(a, b)| (a - b).powi(2)).sum();
    100.0 * squares.sqrt()
}

fn closest_under_red_weak(ours: &[Color], theirs: &[Color]) -> f32 {
    ours.iter()
        .flat_map(|ours| {
            theirs
                .iter()
                .map(|theirs| red_weak_distance(*ours, *theirs))
        })
        .fold(f32::INFINITY, f32::min)
}

/// A 4v4, the largest teams a shipped map has, shows three allies and the four
/// reds of one enemy team.
#[test]
fn seat_colors_hold_apart_under_red_weak_vision() {
    let allies = &ALLIES[..3];
    let enemies = &REDS;
    let mine = [self_color(false)];
    assert!(closest_under_red_weak(allies, enemies) >= 11.0);
    assert!(closest_under_red_weak(&mine, enemies) >= 10.0);
    assert!(closest_under_red_weak(&mine, allies) >= 10.0);
    assert!(
        red_weak_distance(mine[0], color_u8!(245, 175, 40, 255)) < 5.0,
        "a bright warm color collapses onto the self green"
    );
    let team_mine = self_color(true);
    assert!(red_weak_distance(TEAM_ALLY, TEAM_ENEMY) >= 15.0);
    assert!(red_weak_distance(team_mine, TEAM_ENEMY) >= 15.0);
    assert!(red_weak_distance(team_mine, TEAM_ALLY) >= 15.0);
}
