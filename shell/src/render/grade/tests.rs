use super::*;

const THEMES: [Theme; 6] = [
    Theme::RustedYard,
    Theme::ColdCircuitry,
    Theme::QuarryDust,
    Theme::Basalt,
    Theme::Slag,
    Theme::Verdigris,
];

#[test]
fn themes_look_different_from_each_other() {
    for (index, &theme) in THEMES.iter().enumerate() {
        let wash = Grade::of(Some(theme))
            .floor_wash()
            .expect("a theme has a wash");
        assert!(wash.a < 0.2, "{theme:?} would stop being gentle");
        for other in &THEMES[index + 1..] {
            assert_ne!(Grade::of(Some(theme)), Grade::of(Some(*other)));
        }
    }
}

#[test]
fn no_theme_changes_nothing() {
    let grade = Grade::of(None);
    assert_eq!(grade.floor_wash(), None);
    assert_eq!(grade.rock(), WHITE);
    let color = Color::new(0.3, 0.4, 0.5, 1.0);
    assert_eq!(grade.wall(color), color);
}

#[test]
fn a_wall_moves_toward_its_theme_and_stays_opaque() {
    let grey = Color::new(0.25, 0.25, 0.25, 1.0);
    let slag = Grade::of(Some(Theme::Slag)).wall(grey);
    assert!(slag.r > slag.b, "{slag:?}");
    assert_eq!(slag.a, 1.0);
}

#[test]
fn rocks_lean_toward_the_theme_without_brightening() {
    for theme in THEMES {
        let rock = Grade::of(Some(theme)).rock();
        for channel in [rock.r, rock.g, rock.b] {
            assert!((0.5..=1.0).contains(&channel), "{theme:?}: {rock:?}");
        }
    }
    let rust = Grade::of(Some(Theme::RustedYard)).rock();
    assert!(rust.r > rust.b);
    let cold = Grade::of(Some(Theme::ColdCircuitry)).rock();
    assert!(cold.b > cold.r);
}
