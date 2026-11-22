use super::*;

impl Staffing {
    /// The worked nodes with the Harvesters each wants.
    pub(crate) fn crews(&self) -> &[(TilePos, usize)] {
        &self.worked
    }
}

impl Routes<'_> {
    /// Where a route steps from `tile`, as from a source or past one: as
    /// [`step`](Self::step) takes it, and as ranking every closer tile of a
    /// footprint's ring took it.
    pub(crate) fn steps_from(&self, tile: TilePos, source: bool) -> [Option<(u16, TilePos)>; 2] {
        let distance = self.distances.get(tile).copied().unwrap_or(UNREACHABLE);
        let open = |tile: TilePos| self.map.component(tile).is_some();
        let legal = |next: &TilePos| {
            let (dx, dy) = (next.x - tile.x, next.y - tile.y);
            source || dx == 0 || dy == 0 || (open(tile.offset(dx, 0)) && open(tile.offset(0, dy)))
        };
        let from = doubled(tile);
        let ranked = ring(tile, (1, 1))
            .filter(legal)
            .map(|next| {
                (
                    self.distances.get(next).copied().unwrap_or(UNREACHABLE),
                    next,
                )
            })
            .filter(|(closer, _)| *closer < distance)
            .min_by_key(|(closer, next)| (*closer, self.frame.rank(from, doubled(*next))));
        [self.step(tile, distance, source), ranked]
    }
}

#[test]
fn a_crew_under_unseen_fire_beside_its_only_foundry_still_leaves_the_site() {
    // A West site stands beside West's only Foundry, hit by fire nobody
    // sees, so where it landed covers every tile beside the Foundry. The
    // crew still leaves the site for a tile out of known fire.
    use aftermine_sim::PlayerId;
    let scenario = crate::tests::match_on(
        crate::tests::strip(false),
        &["West", "East"],
        &[(0, UnitKind::Harvester, 4, 4)],
    );
    let state = scenario.build().unwrap();
    let mut observation = ObservationData::fog_honest(&state, PlayerId(0));
    let map = MapModel::from_scenario(&scenario).unwrap();
    let frame = HomeFrame::of(&observation, &map).unwrap();
    let foundry = observation
        .my_buildings
        .iter()
        .find(|building| building.kind == BuildingKind::Foundry)
        .unwrap()
        .clone();
    let anchor = foundry.anchor.offset(foundry.kind.size().0, 0);
    let site = BuildingObs {
        provisional: false,
        id: aftermine_sim::BuildingId(77),
        player: PlayerId(0),
        kind: BuildingKind::Turret,
        anchor,
        hp: 1,
        built: false,
        seen: true,
        tier: 0,
    };
    observation.my_buildings.push(site);
    observation.salvage_incidents.push(anchor);
    let crew = observation
        .my_units
        .iter_mut()
        .find(|unit| unit.kind == UnitKind::Harvester)
        .unwrap();
    crew.site = Some(aftermine_sim::BuildingId(77));
    let crew = crew.id;
    assert!(
        ring(foundry.anchor, foundry.kind.size())
            .all(|tile| tile.chebyshev(anchor) <= HARVEST_INCIDENT_DANGER_RADIUS),
        "premise: the incident covers every tile beside the Foundry"
    );
    let mut ledger = Ledger::new(PlayerId(0), 0, 8);
    let pulled = withdraw(
        &observation,
        &map,
        frame,
        &Memory::default(),
        (&[], &[aftermine_sim::BuildingId(77)]),
        &mut ledger,
    );
    assert_eq!(pulled, [aftermine_sim::BuildingId(77)]);
    assert!(ledger.employs(crew));
}
