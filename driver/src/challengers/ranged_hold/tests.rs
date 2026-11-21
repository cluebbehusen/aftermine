use super::super::{Challenger, Family, Plan};
use aftermine_sim::scenario::UnitSpec;
use aftermine_sim::{Command, PlayerId, Scenario, UnitKind};
use chassis::grid::TilePos;

#[test]
fn an_aircraft_at_the_line_draws_only_the_units_that_can_hit_it() {
    let mut scenario = Scenario::skirmish();
    for seat in &mut scenario.players {
        seat.bot = false;
        seat.bot_config = None;
    }
    let plain = Challenger::new(Family::RangedHold, PlayerId(0), 5, &scenario).unwrap();
    let Plan::RangedHold(params) = &plain.plan else {
        unreachable!("a ranged hold's plan");
    };
    let line = plain
        .map
        .approach(plain.map.route_length() * params.line / 100, 4, params.lane);
    // Open tiles nearest the line, with room around them for a unit.
    let (map, _) = scenario.parse_map_and_anchors().unwrap();
    let roomy = |tile: TilePos| {
        (-1..=1).all(|dy| (-1..=1).all(|dx| map.terrain_passable(tile.offset(dx, dy))))
    };
    let mut near: Vec<TilePos> = (-4..=4)
        .flat_map(|dy| (-4..=4).map(move |dx| line.offset(dx, dy)))
        .filter(|tile| roomy(*tile))
        .collect();
    near.sort_by_key(|tile| (tile.chebyshev(line), tile.y, tile.x));
    let kinds = [
        UnitKind::Lancer,
        UnitKind::Lancer,
        UnitKind::Warden,
        UnitKind::Flakhound,
    ];
    scenario.units.extend(
        kinds
            .iter()
            .zip(near.iter().step_by(2))
            .map(|(kind, tile)| UnitSpec {
                player: 0,
                kind: *kind,
                x: tile.x,
                y: tile.y,
            }),
    );
    scenario.units.push(UnitSpec {
        player: 1,
        kind: UnitKind::Buzzard,
        x: line.x,
        y: line.y,
    });
    let mut state = scenario.build().unwrap();
    let flakhound = state
        .units()
        .iter()
        .find(|unit| unit.player == PlayerId(0) && unit.kind == UnitKind::Flakhound)
        .unwrap()
        .id;
    let mut challenger = Challenger::new(Family::RangedHold, PlayerId(0), 5, &scenario).unwrap();
    let mut hunted = None;
    while state.current_tick() < 200 && hunted.is_none() {
        let commands = challenger.act(&state);
        hunted = commands.iter().find_map(|order| match &order.command {
            Command::Hunt { units, .. } if units.contains(&flakhound) => Some(units.clone()),
            _ => None,
        });
        state.tick(&commands);
    }
    assert_eq!(hunted, Some(vec![flakhound]));
}
