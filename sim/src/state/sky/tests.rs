use super::*;
use std::cell::Cell;

fn route(to: TilePos) -> Option<Vec<TilePos>> {
    (to.x >= 0).then(|| vec![to])
}

#[test]
fn a_repeated_air_route_searches_once() {
    let routes = AirRoutes::default();
    let searches = Cell::new(0);
    let (from, to) = (TilePos::new(1, 2), TilePos::new(9, 4));
    let search = || {
        searches.set(searches.get() + 1);
        route(to)
    };
    assert_eq!(routes.route(from, to, search), route(to));
    assert_eq!(routes.route(from, to, search), route(to));
    assert_eq!(searches.get(), 1);

    let refused = TilePos::new(3, 3);
    assert_eq!(
        routes.route(from, refused, || {
            searches.set(searches.get() + 1);
            None
        }),
        None
    );
    assert_eq!(
        routes.route(from, refused, search),
        None,
        "a refusal is kept"
    );
    assert_eq!(searches.get(), 2);
}

#[test]
fn air_routes_keep_only_the_most_recent_searches() {
    let routes = AirRoutes::default();
    let searches = Cell::new(0);
    let from = TilePos::new(0, 0);
    let goal = |i: usize| TilePos::new(i32::try_from(i).expect("small"), 5);
    let ask = |i: usize| {
        routes.route(from, goal(i), || {
            searches.set(searches.get() + 1);
            route(goal(i))
        })
    };
    for i in 0..=AIR_ROUTES {
        assert_eq!(ask(i), route(goal(i)));
    }
    assert_eq!(searches.get(), AIR_ROUTES + 1);
    for i in 2..=AIR_ROUTES {
        ask(i);
    }
    assert_eq!(searches.get(), AIR_ROUTES + 1, "the recent ones are kept");
    ask(0);
    assert_eq!(searches.get(), AIR_ROUTES + 2, "the oldest one went first");
}

#[test]
fn a_clone_starts_its_air_routes_afresh() {
    let routes = AirRoutes::default();
    let searches = Cell::new(0);
    let (from, to) = (TilePos::new(1, 2), TilePos::new(9, 4));
    let search = || {
        searches.set(searches.get() + 1);
        route(to)
    };
    routes.route(from, to, search);
    let copy = routes.clone();
    assert_eq!(copy.route(from, to, search), route(to));
    assert_eq!(searches.get(), 2);
    assert_eq!(copy, routes, "derived routes never set two states apart");
}

fn peaks() -> crate::State {
    let mut map = vec![".".repeat(20); 12];
    map[1].replace_range(1..2, "1");
    map[10].replace_range(18..19, "2");
    for row in &mut map[2..10] {
        row.replace_range(9..11, "^^");
    }
    map[5].replace_range(4..7, "^^^");
    crate::Scenario {
        map,
        units: Vec::new(),
        buildings: Vec::new(),
        ..crate::Scenario::skirmish()
    }
    .build()
    .expect("peaks build")
}

fn fresh_labels(state: &crate::State) -> Vec<u32> {
    chassis::path::cardinal_components(state.map.width(), state.map.height(), |tile| {
        state.passable_for(crate::stats::Domain::Air, tile)
    })
}

#[test]
fn a_state_carries_its_sky_labels_through_assembly_load_and_clone() {
    let state = peaks();
    let labels = fresh_labels(&state);
    assert!(labels.contains(&0), "peaks close the sky");
    assert_eq!(state.sky_labels.labels(), labels.as_slice());

    let json = serde_json::to_string(&state).expect("state serializes");
    let loaded: crate::State = serde_json::from_str(&json).expect("state loads");
    assert_eq!(loaded.sky_labels.labels(), labels.as_slice());

    let copy = state.clone();
    assert!(Arc::ptr_eq(&copy.sky_labels.0, &state.sky_labels.0));
}
