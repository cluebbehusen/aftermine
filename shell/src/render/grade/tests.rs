use super::*;

const THEMES: [&str; 6] = [
    "rusted-yard",
    "cold-circuitry",
    "quarry-dust",
    "basalt",
    "slag",
    "verdigris",
];

#[test]
fn every_shipped_map_has_a_graded_theme() {
    for entry in std::fs::read_dir("../scenarios").expect("the maps read") {
        let path = entry.expect("the map reads").path();
        let text = std::fs::read_to_string(&path).expect("the map reads");
        let map: serde_json::Value = serde_json::from_str(&text).expect("the map parses");
        let theme = map["meta"]["theme"].as_str().expect("the map has a theme");
        assert!(THEMES.contains(&theme), "{path:?} uses {theme}");
    }
}

#[test]
fn themes_look_different_from_each_other() {
    for (index, theme) in THEMES.iter().enumerate() {
        let wash = Grade::of(theme).floor_wash().expect("a theme has a wash");
        assert!(wash.a < 0.2, "{theme} would stop being gentle");
        for other in &THEMES[index + 1..] {
            assert_ne!(Grade::of(theme), Grade::of(other));
        }
    }
}

#[test]
fn no_theme_changes_nothing() {
    let grade = Grade::of("");
    assert_eq!(grade.floor_wash(), None);
    assert_eq!(grade.rock(), WHITE);
    let color = Color::new(0.3, 0.4, 0.5, 1.0);
    assert_eq!(grade.wall(color), color);
}

#[test]
fn a_wall_moves_toward_its_theme_and_stays_opaque() {
    let grey = Color::new(0.25, 0.25, 0.25, 1.0);
    let slag = Grade::of("slag").wall(grey);
    assert!(slag.r > slag.b, "{slag:?}");
    assert_eq!(slag.a, 1.0);
}

#[test]
fn rocks_lean_toward_the_theme_without_brightening() {
    for theme in THEMES {
        let rock = Grade::of(theme).rock();
        for channel in [rock.r, rock.g, rock.b] {
            assert!((0.5..=1.0).contains(&channel), "{theme}: {rock:?}");
        }
    }
    let rust = Grade::of("rusted-yard").rock();
    assert!(rust.r > rust.b);
    let cold = Grade::of("cold-circuitry").rock();
    assert!(cold.b > cold.r);
}
