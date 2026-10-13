use super::*;

fn crucible(x: i32, y: i32) -> BuildingObs {
    BuildingObs {
        provisional: false,
        id: BuildingId(1),
        player: oxide_sim::PlayerId(1),
        kind: BuildingKind::Crucible,
        anchor: TilePos::new(x, y),
        hp: BuildingKind::Crucible.base_stats().max_hp,
        built: true,
        seen: true,
        tier: 0,
    }
}

#[test]
fn a_volley_takes_what_the_simulation_s_falloff_deals_and_no_more_than_is_left() {
    let mark = Mark::of(&crucible(10, 10));
    let paid = u64::from(BuildingKind::Crucible.invested_cost(0));
    let max = u64::from(BuildingKind::Crucible.base_stats().max_hp);
    let peak = u64::from(WARHEAD.peak_damage);
    assert_eq!(volley(&[mark], TilePos::new(10, 10), 1), paid * peak / max);
    // A second warhead takes only what the first left.
    assert_eq!(
        volley(&[mark], TilePos::new(10, 10), 2),
        paid * peak / max + paid * (max - peak) / max
    );
    // Four tiles from its nearest edge, nothing.
    assert_eq!(volley(&[mark], TilePos::new(16, 10), 1), 0);
}

#[test]
fn a_foundry_counts_its_resistance() {
    let mut foundry = crucible(10, 10);
    foundry.kind = BuildingKind::Foundry;
    foundry.hp = BuildingKind::Foundry.base_stats().max_hp;
    let mark = Mark::of(&foundry);
    let taken = u64::from(BuildingKind::Foundry.warhead_taken(WARHEAD.peak_damage));
    let paid = u64::from(BuildingKind::Foundry.invested_cost(0));
    let max = u64::from(BuildingKind::Foundry.base_stats().max_hp);
    assert_eq!(volley(&[mark], TilePos::new(11, 11), 1), paid * taken / max);
}
