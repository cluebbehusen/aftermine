use super::*;

fn catalog() -> Catalog {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/music/manifest.json");
    Catalog::parse(&std::fs::read_to_string(path).expect("music manifest")).expect("manifest")
}

fn kind(catalog: &Catalog, (piece, section): (usize, usize)) -> &str {
    &catalog.pieces[piece].sections[section].kind
}

const QUIET: Cues = Cues {
    siege: false,
    final_push: false,
    big_battle: false,
    guns: 0,
};

#[test]
fn matches_open_on_an_intro_and_songs_take_turns() {
    let catalog = catalog();
    let songs = catalog.songs();
    for seed in 0..20 {
        let mut conductor = Conductor::new(&catalog, seed);
        let mut heard = Vec::new();
        for _ in 0..songs.len() {
            let first = conductor.begin(&catalog, Role::Match);
            assert_eq!(kind(&catalog, first), "I");
            heard.push(first.0);
        }
        heard.sort_unstable();
        assert_eq!(heard, songs, "every song plays before any repeats");
    }
}

#[test]
fn a_song_never_plays_one_kind_twice_running_or_an_unasked_special() {
    let catalog = catalog();
    let mut conductor = Conductor::new(&catalog, 7);
    let mut last = conductor.begin(&catalog, Role::Match);
    for _ in 0..500 {
        let next = conductor.next(&catalog, QUIET, false);
        let (before, after) = (kind(&catalog, last), kind(&catalog, next));
        assert_ne!(before, after, "the same kind twice running");
        assert!(!matches!(after, "I" | "S" | "F"), "{after} played unasked");
        assert_eq!(next.0, last.0, "a fight keeps its song");
        last = next;
    }
}

#[test]
fn the_match_calls_for_siege_final_push_and_the_lift() {
    let catalog = catalog();
    let mut conductor = Conductor::new(&catalog, 3);
    conductor.begin(&catalog, Role::Match);
    let siege = Cues {
        siege: true,
        final_push: true,
        big_battle: true,
        guns: 30,
    };
    let walk: Vec<String> = (0..4)
        .map(|_| kind(&catalog, conductor.next(&catalog, siege, false)).to_owned())
        .collect();
    assert_eq!(
        walk,
        ["S", "C", "S", "C"],
        "a siege outranks the rest and alternates with the lift"
    );

    let push = Cues {
        final_push: true,
        ..QUIET
    };
    assert_eq!(kind(&catalog, conductor.next(&catalog, push, false)), "F");
    let battle = Cues {
        big_battle: true,
        ..QUIET
    };
    assert_eq!(kind(&catalog, conductor.next(&catalog, battle, false)), "C");
    assert_ne!(
        kind(&catalog, conductor.next(&catalog, battle, false)),
        "C",
        "a long battle does not repeat the lift back to back"
    );
}

#[test]
fn a_song_gives_way_only_at_a_calm_boundary_after_its_turn() {
    let catalog = catalog();
    let mut conductor = Conductor::new(&catalog, 11);
    let (song, _) = conductor.begin(&catalog, Role::Match);
    for _ in 0..SONG_SECTIONS {
        assert_eq!(conductor.next(&catalog, QUIET, true).0, song);
    }
    let fighting = conductor.next(&catalog, QUIET, false);
    assert_eq!(fighting.0, song, "a fight keeps its song");
    let cued = conductor.next(
        &catalog,
        Cues {
            big_battle: true,
            ..QUIET
        },
        true,
    );
    assert_eq!(cued.0, song, "a cue keeps its song");
    let next = conductor.next(&catalog, QUIET, true);
    assert_ne!(next.0, song);
    assert_eq!(
        kind(&catalog, next),
        "I",
        "the next song opens on its intro"
    );
}

#[test]
fn the_menu_cycles_in_order_and_an_ending_loops_after_its_phrase() {
    let catalog = catalog();
    let mut conductor = Conductor::new(&catalog, 1);
    let menu = conductor.begin(&catalog, Role::Menu);
    let count = catalog.pieces[menu.0].sections.len();
    let order: Vec<usize> = (0..2 * count)
        .map(|_| conductor.next(&catalog, QUIET, true).1)
        .collect();
    let expected: Vec<usize> = (1..=2 * count).map(|step| step % count).collect();
    assert_eq!(order, expected);

    for role in [Role::Victory, Role::Defeat, Role::Draw] {
        let first = conductor.begin(&catalog, role);
        assert_eq!(kind(&catalog, first), "I");
        for _ in 0..3 {
            assert_eq!(kind(&catalog, conductor.next(&catalog, QUIET, true)), "L");
        }
    }
}
