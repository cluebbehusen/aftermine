//! Map-audit gates: every shipped scenario must keep its size and wealth
//! badges, spawn fairness, artillery pressure, passage widths, contestable
//! frame clusters, and mirrored authoring, as measured by `driver map-audit`.
//!
//! Set `AFTERMINE_MAP_GATES_DIR` to a directory of drafts, relative to the
//! repository root, to hold them to the same gates before promotion.

use aftermine_driver::audit::{MapAudit, RouteAudit, audit};
use aftermine_sim::map::Map;
use aftermine_sim::scenario::BotConfig;
use aftermine_sim::{BuildingKind, PlayerId, Scenario, State};
use chassis::grid::TilePos;
use chassis::grid::as_index;
use std::collections::{BTreeSet, VecDeque};
use std::path::PathBuf;

/// Nearest enemy routes, in effective steps, that set what a map owes
/// beyond its badges. Close quarters may share the second frame claim and
/// accept closer artillery; from the remote-value route on, a map needs
/// frames beyond every starting pocket, and from the longer two it needs a
/// remote cluster of two or three frames to expand into.
const CLOSE_QUARTERS: usize = 28;
const REMOTE_VALUE_FROM: usize = 53;
const REMOTE_PAIR_FROM: usize = 91;
const REMOTE_TRIO_FROM: usize = 151;

/// The shortest route between hostile Foundries by the cheapest mover.
fn nearest_enemy(report: &MapAudit) -> usize {
    report
        .routes
        .iter()
        .filter_map(RouteAudit::effective_steps)
        .min()
        .expect("a hostile pair routes")
}

fn shipped() -> Vec<(String, Scenario)> {
    let drafts = std::env::var("AFTERMINE_MAP_GATES_DIR").ok();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(drafts.as_deref().unwrap_or("scenarios"));
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).expect("scenarios dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("utf-8 name")
            .to_string();
        out.push((name, Scenario::load(&path).expect("shipped maps load")));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(
        drafts.is_some() || out.len() >= 10,
        "the shipped roster is present"
    );
    out
}

#[test]
fn every_map_seats_a_human_and_live_opponents() {
    // Seat 0 is the human chair; every other seat must actually play.
    for (name, scenario) in shipped() {
        assert_eq!(
            scenario.mode,
            aftermine_sim::scenario::ScenarioMode::Match,
            "{name}: shipped maps use match rules"
        );
        assert!(
            !scenario.players[0].bot,
            "{name}: seat 0 is the human chair"
        );
        assert!(
            scenario.players[0].bot_config.is_none(),
            "{name}: human chair must not carry a bot configuration"
        );
        for (i, seat) in scenario.players.iter().enumerate().skip(1) {
            assert!(seat.bot, "{name}: seat {i} is a dead chair (bot: false)");
            assert!(
                seat.bot_config == Some(BotConfig::default()),
                "{name}: seat {i} is not the shipped scripted opponent"
            );
        }
    }
}

#[test]
fn every_map_carries_complete_metadata() {
    for (name, scenario) in shipped() {
        let meta = scenario
            .meta
            .as_ref()
            .unwrap_or_else(|| panic!("{name}: shipped maps carry metadata"));
        assert!(!meta.hook.is_empty(), "{name}: hook missing");
        assert!(meta.size.is_some(), "{name}: size badge missing");
        assert!(!meta.mode.is_empty(), "{name}: mode missing");
        assert!(meta.wealth.is_some(), "{name}: wealth badge missing");
        assert!(!meta.theme.is_empty(), "{name}: theme missing");
    }
}

#[test]
fn routes_connect_and_badges_hold() {
    // The badges are measured claims. Size reads the nearest enemy route in
    // effective steps between Foundry doorsteps: weighted ground steps, or
    // the air detour on island pairs that no ground route serves (the sim's
    // connectivity gate already guarantees some mover connects every pair).
    // Wealth reads scrap and frames per player.
    for (name, scenario) in shipped() {
        let report = audit(&scenario).expect("audit builds");
        let meta = scenario.meta.as_ref().unwrap();
        assert!(!report.routes.is_empty(), "{name}: no hostile pair routed");
        for route in &report.routes {
            route
                .air_tiles
                .unwrap_or_else(|| panic!("{name}: sky sealed"));
            route
                .effective_steps()
                .unwrap_or_else(|| panic!("{name}: no mover routes the pair"));
        }
        assert_eq!(
            meta.size,
            report.size_badge,
            "{name}: the size badge disagrees with a nearest enemy route of {} steps",
            nearest_enemy(&report)
        );
        assert_eq!(
            meta.wealth,
            Some(report.wealth_badge),
            "{name}: the wealth badge disagrees with {} per player",
            report.wealth_per_player
        );
    }
}

#[test]
fn trafficked_passages_are_at_least_three_tiles_wide() {
    // Groups crowd and even one unit reads badly squeezing through a gap a
    // tile or two wide, and an experimental needs three tiles to pass. The
    // audit counts only ground where hostile seats meet, so islands keep
    // their authored shape.
    for (name, scenario) in shipped() {
        let report = audit(&scenario).expect("audit builds");
        let spots: Vec<String> = report
            .narrow_passages
            .iter()
            .map(|p| format!("({},{})..({},{})", p.min.0, p.min.1, p.max.0, p.max.1))
            .collect();
        assert!(
            spots.is_empty(),
            "{name}: traffic squeezes through passages under three tiles wide at {}",
            spots.join(", ")
        );
    }
}

#[test]
fn no_frame_cluster_locks_a_match() {
    // A big cluster of frames no base owns, which rival seats reach about
    // as soon as each other and little ground can shell, pays whoever lands
    // first for the rest of the match.
    for (name, scenario) in shipped() {
        let report = audit(&scenario).expect("audit builds");
        let locked: Vec<String> = report
            .prizes
            .iter()
            .filter(|prize| prize.locked())
            .map(|prize| {
                format!(
                    "{} frames at {:?}, shelled from {} tiles",
                    prize.frames.len(),
                    prize.frames,
                    prize.shelling
                )
            })
            .collect();
        assert!(
            locked.is_empty(),
            "{name}: a contested prize no artillery can answer: {}",
            locked.join("; ")
        );
    }
}

#[test]
fn artillery_pressure_stays_bounded() {
    // The caps encode minimum ground routes (about 14.6 steps in close
    // quarters, 19 elsewhere) for the longest artillery reach; a change to
    // that reach rescales the caps, not the maps. A tier-three siege piece
    // in close quarters is a late commitment, not an opening threat.
    for (name, scenario) in shipped() {
        let report = audit(&scenario).expect("audit builds");
        let cap = if nearest_enemy(&report) <= CLOSE_QUARTERS {
            0.96
        } else {
            0.74
        };
        for route in &report.routes {
            let Some(pressure) = route.artillery_pressure else {
                continue;
            };
            assert!(
                pressure <= cap,
                "{name}: artillery pressure {pressure:.2} above the {cap} cap"
            );
        }
    }
}

#[test]
fn spawns_are_fair_to_every_seat() {
    for (name, scenario) in shipped() {
        let report = audit(&scenario).expect("audit builds");
        let seats = &report.seats;
        let symmetry = scenario
            .meta
            .as_ref()
            .map(|m| m.symmetry.clone())
            .unwrap_or_default();
        if symmetry == "metric" {
            metric_fairness(&name, seats);
            continue;
        }
        let first = &seats[0];
        for seat in seats.iter().skip(1) {
            assert_eq!(
                seat.reachable_tiles, first.reachable_tiles,
                "{name}: seat {} rooms differently",
                seat.seat
            );
            assert_eq!(
                seat.nearest_enemy_route, first.nearest_enemy_route,
                "{name}: seat {} meets the enemy on a different clock",
                seat.seat
            );
        }
        // Scrap distance. Duels: the mirror seat measures identically.
        // 4p: same-parity seats hold strictly; across the parity split a
        // one-tile lean is tolerated on frozen terrain, and rebuilt maps
        // should close it to zero.
        let gap = |a: usize, b: usize| (seats[a].nearest_scrap - seats[b].nearest_scrap).abs();
        match seats.len() {
            2 => assert!(gap(0, 1) <= 1e-9, "{name}: duel seats disagree on scrap"),
            4 => {
                assert!(gap(0, 2) <= 1e-9, "{name}: seats 0/2 disagree on scrap");
                assert!(gap(1, 3) <= 1e-9, "{name}: seats 1/3 disagree on scrap");
                assert!(
                    gap(0, 1) <= 1.5,
                    "{name}: scrap leans {:.2} across the team split",
                    gap(0, 1)
                );
            }
            // Mirrored six- and eight-seat maps are built from identical
            // lanes, so every seat measures scrap identically.
            6 | 8 => {
                for i in 1..seats.len() {
                    assert!(
                        gap(0, i) <= 1e-9,
                        "{name}: seat {i} disagrees on scrap with seat 0"
                    );
                }
            }
            // Other seat counts: mirrored maps hold room and clock exactly
            // (asserted above); scrap holds within the cross-parity lean
            // the 4p rule tolerates.
            _ => {
                for i in 1..seats.len() {
                    assert!(
                        gap(0, i) <= 1.5,
                        "{name}: scrap leans {:.2} against seat {i}",
                        gap(0, i)
                    );
                }
            }
        }
    }
}

/// The free-for-all fairness class: no tile mirror to lean on, so every
/// seat's measured position must sit inside a tolerance of the field's
/// mean — room within 5%, first-contact clock within 15%, scrap within
/// 2.5 tiles, and (when the map has frames at all) extractor access
/// within 20%.
fn metric_fairness(name: &str, seats: &[aftermine_driver::audit::SeatAudit]) {
    let mean = |xs: &[f64]| xs.iter().sum::<f64>() / xs.len() as f64;
    let rooms: Vec<f64> = seats.iter().map(|s| s.reachable_tiles as f64).collect();
    let room_mean = mean(&rooms);
    for s in seats {
        let dev = (s.reachable_tiles as f64 - room_mean).abs() / room_mean;
        assert!(
            dev <= 0.05,
            "{name}: seat {} rooms {:.1}% off the field",
            s.seat,
            dev * 100.0
        );
    }
    let clocks: Vec<f64> = seats
        .iter()
        .map(|s| {
            s.nearest_enemy_route
                .unwrap_or_else(|| panic!("{name}: seat {} sealed off", s.seat)) as f64
        })
        .collect();
    let clock_mean = mean(&clocks);
    for (s, clock) in seats.iter().zip(&clocks) {
        let dev = (clock - clock_mean).abs() / clock_mean;
        assert!(
            dev <= 0.15,
            "{name}: seat {} meets the enemy {:.1}% off the field's clock",
            s.seat,
            dev * 100.0
        );
    }
    let scrap_mean = mean(&seats.iter().map(|s| s.nearest_scrap).collect::<Vec<_>>());
    for s in seats {
        assert!(
            (s.nearest_scrap - scrap_mean).abs() <= 2.5,
            "{name}: seat {} digs {:.2} tiles off the field's scrap",
            s.seat,
            (s.nearest_scrap - scrap_mean).abs()
        );
    }
    if seats.iter().all(|s| s.nearest_extractor.is_some()) {
        let frames: Vec<f64> = seats.iter().map(|s| s.nearest_extractor.unwrap()).collect();
        let frame_mean = mean(&frames);
        for (s, d) in seats.iter().zip(&frames) {
            let dev = (d - frame_mean).abs() / frame_mean.max(1.0);
            assert!(
                dev <= 0.20,
                "{name}: seat {} reaches its extractor {:.1}% off the field",
                s.seat,
                dev * 100.0
            );
        }
    }
}

#[test]
fn tiered_maps_follow_the_level_rules() {
    for (name, scenario) in shipped() {
        let faults = aftermine_driver::audit::level_faults(&scenario)
            .unwrap_or_else(|error| panic!("{name}: {error:#}"));
        assert!(faults.is_empty(), "{name}: {}", faults.join("; "));
    }
}

#[test]
fn every_map_offers_a_home_extractor_and_expansion_value() {
    const NATURAL_EXTENSION: i32 = 8;

    let support_radius = aftermine_sim::stats::EXTRACTOR_SUPPORT_RADIUS;
    for (name, scenario) in shipped() {
        // The nearest enemy route measures contact timing, not total
        // acreage. Large FFA basins can start rivals close while still
        // needing value beyond their starting pockets.
        const REMOTE_VALUE_AREA_FLOOR: i32 = 7_000;
        let (map, foundries) = scenario
            .parse_map_and_anchors()
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let frames = map.extractor_frames();
        assert!(
            frames.len() >= scenario.players.len(),
            "{name}: every seat needs its own opening Extractor claim"
        );
        for (index, &frame) in frames.iter().enumerate() {
            for &other in &frames[index + 1..] {
                assert!(
                    !rects_overlap(
                        frame,
                        BuildingKind::Extractor.size(),
                        other,
                        BuildingKind::Extractor.size(),
                    ),
                    "{name}: Extractor frames at ({}, {}) and ({}, {}) overlap",
                    frame.x,
                    frame.y,
                    other.x,
                    other.y
                );
            }
        }

        let state = scenario.build().expect("shipped map builds");
        let air_reachability: Vec<_> = foundries
            .iter()
            .map(|(_, foundry)| reachable_air_from_foundry(&map, *foundry))
            .collect();
        for &frame in frames {
            assert!(
                state.units().iter().all(|unit| {
                    !rect_contains(frame, BuildingKind::Extractor.size(), unit.tile())
                }),
                "{name}: Extractor frame ({}, {}) overlaps a starting unit",
                frame.x,
                frame.y
            );
            assert!(
                state.buildings().iter().all(|building| {
                    !(0..BuildingKind::Extractor.size().1).any(|dy| {
                        (0..BuildingKind::Extractor.size().0)
                            .any(|dx| building.contains(frame.offset(dx, dy)))
                    })
                }),
                "{name}: Extractor frame ({}, {}) overlaps a starting building",
                frame.x,
                frame.y
            );
        }
        let nearest = nearest_enemy(&audit(&scenario).expect("audit builds"));
        let mut opening_candidates = Vec::with_capacity(foundries.len());
        let mut seat_distances = Vec::with_capacity(foundries.len());
        for &(seat, foundry) in &foundries {
            let mut distances: Vec<(i32, TilePos)> = frames
                .iter()
                .copied()
                .map(|frame| (extractor_foundry_distance(foundry, frame), frame))
                .collect();
            distances.sort_unstable();
            let home: Vec<_> = distances
                .iter()
                .filter(|(distance, _)| *distance <= support_radius)
                .map(|(_, frame)| *frame)
                .collect();
            let usable_home: Vec<_> = home
                .iter()
                .copied()
                .filter(|frame| {
                    (0..2)
                        .all(|dy| (0..2).all(|dx| state.vision(seat).visible(frame.offset(dx, dy))))
                        && {
                            let reachable =
                                reachable_builder_ground(&map, &state, seat, Some(*frame));
                            reachable_rect_perimeter(
                                &map,
                                *frame,
                                BuildingKind::Extractor.size(),
                                &reachable,
                            )
                        }
                })
                .collect();
            assert!(
                !usable_home.is_empty(),
                "{name}: seat {} needs a fully visible, builder-reachable frame inside starting Foundry support: {distances:?}",
                seat.0
            );
            opening_candidates.push((seat, usable_home));
            seat_distances.push((seat, distances));
        }
        assert!(
            distinct_opening_claims_exist(&opening_candidates),
            "{name}: starting seats cannot be matched to distinct visible, reachable home Extractor frames: {opening_candidates:?}"
        );

        let all_home_candidates: BTreeSet<_> = opening_candidates
            .iter()
            .flat_map(|(_, frames)| frames.iter().copied())
            .collect();
        for (seat, distances) in &seat_distances {
            // Close quarters may make the second claim a shared contested
            // center. Every longer map gives each seat a nearby natural
            // beyond all viable opening claims so expansion adds renewable
            // value. A natural may still be shared by teammates.
            if nearest <= CLOSE_QUARTERS {
                continue;
            }
            assert!(
                distances.iter().any(|(distance, frame)| {
                    *distance > support_radius
                        && *distance <= support_radius + NATURAL_EXTENSION
                        && !all_home_candidates.contains(frame)
                        && {
                            let reachable =
                                reachable_builder_ground(&map, &state, *seat, Some(*frame));
                            reachable_rect_perimeter(
                                &map,
                                *frame,
                                BuildingKind::Extractor.size(),
                                &reachable,
                            ) && supportable_foundry_anchor(
                                &map,
                                &state,
                                *frame,
                                &reachable,
                                support_radius,
                            )
                            .is_some()
                        }
                }),
                "{name}: seat {} needs a reachable additional natural and a legal supporting Foundry site: {distances:?}",
                seat.0
            );
        }

        // Every authored claim must be reachable by at least one starting
        // builder. Unless that builder's own Foundry supports the frame, the
        // claim also needs construction room for a supporting Foundry.
        for &frame in frames {
            let ground_usable = foundries.iter().any(|(seat, foundry)| {
                let reachable = reachable_builder_ground(&map, &state, *seat, Some(frame));
                reachable_rect_perimeter(&map, frame, BuildingKind::Extractor.size(), &reachable)
                    && (extractor_foundry_distance(*foundry, frame) <= support_radius
                        || supportable_foundry_anchor(
                            &map,
                            &state,
                            frame,
                            &reachable,
                            support_radius,
                        )
                        .is_some())
            });
            let transport_usable = air_reachability.iter().any(|air_reachable| {
                reachable_ground_rect_perimeter(
                    &map,
                    &state,
                    frame,
                    BuildingKind::Extractor.size(),
                    air_reachable,
                ) && supportable_foundry_anchor_from_air(
                    &map,
                    &state,
                    frame,
                    air_reachable,
                    support_radius,
                )
                .is_some()
            });
            assert!(
                ground_usable || transport_usable,
                "{name}: Extractor frame ({}, {}) is neither a ground-reachable claim nor a viable transport expansion",
                frame.x,
                frame.y
            );
        }

        let physically_large = map.width() * map.height() >= REMOTE_VALUE_AREA_FLOOR;
        if physically_large || nearest >= REMOTE_VALUE_FROM {
            let remote: Vec<_> = frames
                .iter()
                .copied()
                .filter(|frame| {
                    foundries.iter().all(|(_, foundry)| {
                        extractor_foundry_distance(*foundry, *frame)
                            > support_radius + NATURAL_EXTENSION
                    })
                })
                .collect();
            assert!(
                !remote.is_empty(),
                "{name}: a map with footprint {}x{} and rivals {nearest} steps apart needs Extractor value beyond every starting pocket",
                map.width(),
                map.height()
            );

            let cluster_floor = match name.as_str() {
                // Its remote ground is a set of tiny transport-only islands;
                // preserving legible landing room matters more than packing
                // overlapping frames onto one component.
                "skyhook-anchorage" => 1,
                _ if nearest >= REMOTE_TRIO_FROM => 3,
                _ if nearest >= REMOTE_PAIR_FROM => 2,
                _ => 1,
            };
            let reachability: Vec<_> = foundries
                .iter()
                .map(|(seat, _)| reachable_builder_ground(&map, &state, *seat, None))
                .collect();
            let cluster = largest_supportable_cluster(
                &map,
                &state,
                &remote,
                &reachability,
                &air_reachability,
                support_radius,
            );
            assert!(
                cluster >= cluster_floor,
                "{name}: a map with footprint {}x{} and rivals {nearest} steps apart needs a reachable remote cluster of at least {cluster_floor} frame(s), found {cluster}",
                map.width(),
                map.height()
            );
        }
    }
}

fn reachable_rect_perimeter(
    map: &Map,
    anchor: TilePos,
    size: (i32, i32),
    reachable: &[bool],
) -> bool {
    (anchor.y - 1..=anchor.y + size.1).any(|y| {
        (anchor.x - 1..=anchor.x + size.0).any(|x| {
            let inside =
                x >= anchor.x && x < anchor.x + size.0 && y >= anchor.y && y < anchor.y + size.1;
            !inside
                && x >= 0
                && y >= 0
                && x < map.width()
                && y < map.height()
                && reachable[as_index(y * map.width() + x)]
        })
    })
}

fn reachable_ground_rect_perimeter(
    map: &Map,
    state: &State,
    anchor: TilePos,
    size: (i32, i32),
    reachable: &[bool],
) -> bool {
    (anchor.y - 1..=anchor.y + size.1).any(|y| {
        (anchor.x - 1..=anchor.x + size.0).any(|x| {
            let tile = TilePos::new(x, y);
            let inside =
                x >= anchor.x && x < anchor.x + size.0 && y >= anchor.y && y < anchor.y + size.1;
            !inside
                && map.terrain_passable(tile)
                && state
                    .buildings()
                    .iter()
                    .all(|building| !building.contains(tile))
                && reachable[as_index(y * map.width() + x)]
        })
    })
}

fn extractor_foundry_distance(foundry: TilePos, extractor: TilePos) -> i32 {
    aftermine_sim::geometry::footprint_distance(
        foundry,
        BuildingKind::Foundry.size(),
        extractor,
        BuildingKind::Extractor.size(),
    )
}

fn distinct_opening_claims_exist(candidates: &[(PlayerId, Vec<TilePos>)]) -> bool {
    fn assign(
        seats: &[(PlayerId, Vec<TilePos>)],
        index: usize,
        claimed: &mut Vec<TilePos>,
    ) -> bool {
        let Some((_, frames)) = seats.get(index) else {
            return true;
        };
        for &frame in frames {
            if claimed.contains(&frame) {
                continue;
            }
            claimed.push(frame);
            if assign(seats, index + 1, claimed) {
                return true;
            }
            claimed.pop();
        }
        false
    }

    let mut constrained_first = candidates.to_vec();
    constrained_first.sort_by_key(|(seat, frames)| (frames.len(), *seat));
    assign(&constrained_first, 0, &mut Vec::new())
}

fn reachable_builder_ground(
    map: &Map,
    state: &State,
    seat: PlayerId,
    restored_frame: Option<TilePos>,
) -> Vec<bool> {
    let index = |tile: TilePos| as_index(tile.y * map.width() + tile.x);
    let blocked = |tile: TilePos| {
        restored_frame
            .is_some_and(|frame| rect_contains(frame, BuildingKind::Extractor.size(), tile))
            || state
                .buildings()
                .iter()
                .any(|building| building.contains(tile))
    };
    let passable = |tile: TilePos| map.terrain_passable(tile) && !blocked(tile);
    let mut reachable = vec![false; as_index(map.width() * map.height())];
    let mut queue = VecDeque::new();
    for unit in state
        .units()
        .iter()
        .filter(|unit| unit.player == seat && unit.kind.stats().harvest.is_some())
    {
        let tile = unit.tile();
        if passable(tile) && !reachable[index(tile)] {
            reachable[index(tile)] = true;
            queue.push_back(tile);
        }
    }
    assert!(
        !queue.is_empty(),
        "seat {} needs a live starting builder for the forward-economy proof",
        seat.0
    );

    while let Some(tile) = queue.pop_front() {
        for (dx, dy) in chassis::grid::CARDINALS {
            let next = tile.offset(dx, dy);
            if passable(next) && !reachable[index(next)] {
                reachable[index(next)] = true;
                queue.push_back(next);
            }
        }
    }
    reachable
}

fn reachable_air_from_foundry(map: &Map, foundry: TilePos) -> Vec<bool> {
    let index = |tile: TilePos| as_index(tile.y * map.width() + tile.x);
    let passable = |tile: TilePos| {
        map.tile(tile)
            .is_some_and(|map_tile| !map_tile.terrain.blocks_air())
    };
    let mut reachable = vec![false; as_index(map.width() * map.height())];
    let mut queue = VecDeque::new();
    if passable(foundry) {
        reachable[index(foundry)] = true;
        queue.push_back(foundry);
    }

    while let Some(tile) = queue.pop_front() {
        for (dx, dy) in chassis::grid::CARDINALS {
            let next = tile.offset(dx, dy);
            if passable(next) && !reachable[index(next)] {
                reachable[index(next)] = true;
                queue.push_back(next);
            }
        }
    }
    reachable
}

fn largest_supportable_cluster(
    map: &Map,
    state: &State,
    frames: &[TilePos],
    reachability: &[Vec<bool>],
    air_reachability: &[Vec<bool>],
    support_radius: i32,
) -> usize {
    let mut largest = 0;
    for y in 0..map.height() {
        for x in 0..map.width() {
            let anchor = TilePos::new(x, y);
            if !foundry_site_is_legal(map, state, anchor) {
                continue;
            }
            let supported = frames
                .iter()
                .filter(|frame| extractor_foundry_distance(anchor, **frame) <= support_radius)
                .count();
            if reachability
                .iter()
                .any(|reachable| foundry_doorstep_reached(map, anchor, reachable))
            {
                largest = largest.max(supported);
            }
            for air_reachable in air_reachability {
                if !foundry_ground_doorstep_reached(map, state, anchor, air_reachable) {
                    continue;
                }
                let accessible = frames
                    .iter()
                    .filter(|frame| {
                        extractor_foundry_distance(anchor, **frame) <= support_radius
                            && reachable_ground_rect_perimeter(
                                map,
                                state,
                                **frame,
                                BuildingKind::Extractor.size(),
                                air_reachable,
                            )
                    })
                    .count();
                largest = largest.max(accessible);
            }
        }
    }
    largest
}

fn supportable_foundry_anchor(
    map: &Map,
    state: &State,
    frame: TilePos,
    reachable: &[bool],
    support_radius: i32,
) -> Option<TilePos> {
    for y in 0..map.height() {
        for x in 0..map.width() {
            let anchor = TilePos::new(x, y);
            if extractor_foundry_distance(anchor, frame) > support_radius
                || !foundry_site_is_legal(map, state, anchor)
            {
                continue;
            }
            if foundry_doorstep_reached(map, anchor, reachable) {
                return Some(anchor);
            }
        }
    }
    None
}

fn supportable_foundry_anchor_from_air(
    map: &Map,
    state: &State,
    frame: TilePos,
    air_reachable: &[bool],
    support_radius: i32,
) -> Option<TilePos> {
    for y in 0..map.height() {
        for x in 0..map.width() {
            let anchor = TilePos::new(x, y);
            if extractor_foundry_distance(anchor, frame) <= support_radius
                && foundry_site_is_legal(map, state, anchor)
                && foundry_ground_doorstep_reached(map, state, anchor, air_reachable)
            {
                return Some(anchor);
            }
        }
    }
    None
}

fn foundry_doorstep_reached(map: &Map, anchor: TilePos, reachable: &[bool]) -> bool {
    let foundry_size = BuildingKind::Foundry.size();
    (anchor.y - 1..=anchor.y + foundry_size.1).any(|door_y| {
        (anchor.x - 1..=anchor.x + foundry_size.0).any(|door_x| {
            let inside = door_x >= anchor.x
                && door_x < anchor.x + foundry_size.0
                && door_y >= anchor.y
                && door_y < anchor.y + foundry_size.1;
            !inside
                && door_x >= 0
                && door_y >= 0
                && door_x < map.width()
                && door_y < map.height()
                && reachable[as_index(door_y * map.width() + door_x)]
        })
    })
}

fn foundry_ground_doorstep_reached(
    map: &Map,
    state: &State,
    anchor: TilePos,
    reachable: &[bool],
) -> bool {
    let foundry_size = BuildingKind::Foundry.size();
    (anchor.y - 1..=anchor.y + foundry_size.1).any(|door_y| {
        (anchor.x - 1..=anchor.x + foundry_size.0).any(|door_x| {
            let tile = TilePos::new(door_x, door_y);
            let inside = door_x >= anchor.x
                && door_x < anchor.x + foundry_size.0
                && door_y >= anchor.y
                && door_y < anchor.y + foundry_size.1;
            !inside
                && map.terrain_passable(tile)
                && state
                    .buildings()
                    .iter()
                    .all(|building| !building.contains(tile))
                && reachable[as_index(door_y * map.width() + door_x)]
        })
    })
}

fn foundry_site_is_legal(map: &Map, state: &State, anchor: TilePos) -> bool {
    let (width, height) = BuildingKind::Foundry.size();
    (0..height).all(|dy| {
        (0..width).all(|dx| {
            let tile = anchor.offset(dx, dy);
            map.terrain_passable(tile)
                && !map.tile_in_extractor_frame(tile)
                && state
                    .buildings()
                    .iter()
                    .all(|building| !building.contains(tile))
        })
    })
}

fn rect_contains(anchor: TilePos, size: (i32, i32), tile: TilePos) -> bool {
    tile.x >= anchor.x
        && tile.x < anchor.x + size.0
        && tile.y >= anchor.y
        && tile.y < anchor.y + size.1
}

fn rects_overlap(a: TilePos, a_size: (i32, i32), b: TilePos, b_size: (i32, i32)) -> bool {
    a.x < b.x + b_size.0 && b.x < a.x + a_size.0 && a.y < b.y + b_size.1 && b.y < a.y + a_size.1
}

#[test]
fn rotated_seat_partners_carry_opposite_compass_names() {
    // A seat is named for where it spawns, and the seat its Foundry
    // rotates onto spawns across the map, so the two names are compass
    // opposites: North West faces South East. This holds for the metric
    // class too wherever its seats happen to rotate onto each other; a
    // seat whose anchor rotates onto no seat has no partner to check.
    let (fw, fh) = BuildingKind::Foundry.size();
    let mut mismatched = Vec::new();
    for (name, scenario) in shipped() {
        let (map, anchors) = scenario
            .parse_map_and_anchors()
            .unwrap_or_else(|e| panic!("{name}: map parses ({e})"));
        for (seat, at) in &anchors {
            let image = TilePos {
                x: map.width() - fw - at.x,
                y: map.height() - fh - at.y,
            };
            let Some((partner, _)) = anchors.iter().find(|(_, a)| *a == image) else {
                continue;
            };
            let mine = &scenario.players[seat.0 as usize].name;
            let theirs = &scenario.players[partner.0 as usize].name;
            if *theirs != compass_opposite(mine) {
                mismatched.push(format!(
                    "{name}: seat {} ({mine}) rotates onto seat {} ({theirs})",
                    seat.0, partner.0
                ));
            }
        }
    }
    assert!(mismatched.is_empty(), "{mismatched:#?}");
}

/// A seat name with North and South, and East and West, exchanged,
/// inside compound words too ("Northwest" becomes "Southeast").
fn compass_opposite(name: &str) -> String {
    const SWAPS: [(&str, &str); 8] = [
        ("North", "South"),
        ("South", "North"),
        ("East", "West"),
        ("West", "East"),
        ("north", "south"),
        ("south", "north"),
        ("east", "west"),
        ("west", "east"),
    ];
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(c) = rest.chars().next() {
        if let Some((from, to)) = SWAPS.iter().find(|(from, _)| rest.starts_with(from)) {
            out.push_str(to);
            rest = &rest[from.len()..];
        } else {
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

#[test]
fn every_map_mirrors_its_paired_seats_entry_by_entry() {
    // The metric class opts out: its fairness is measured, not
    // mirrored (see `metric_fairness`).
    // The authoring rule: a paired seat's
    // starting units must be the entry-by-entry 180-degree image of its
    // partner's, because ids are handed out in list order and every
    // id-order tie-break downstream inherits that order.
    //
    // The pairing is derived from the map, not assumed: rotating a
    // Foundry anchor 180 degrees lands on exactly one other anchor, and
    // that relation is an involution. It reads {0<->1} on duels,
    // {0<->3, 1<->2} or {0<->2, 1<->3} on the 4p maps, and
    // {i <-> n-1-i} on the 6p/8p lane stacks.
    for (name, scenario) in shipped() {
        if scenario
            .meta
            .as_ref()
            .is_some_and(|m| m.symmetry == "metric")
        {
            continue;
        }
        let (map, anchors) = scenario
            .parse_map_and_anchors()
            .unwrap_or_else(|e| panic!("{name}: map parses ({e})"));
        let (w, h) = (map.width(), map.height());

        // Anchor digits are already ground by the time Map::parse is
        // done, so this is the terrain symmetry the pairing rests on —
        // scrap amounts and cosmetic rubble included.
        for (pos, tile) in map.iter() {
            let image = TilePos {
                x: w - 1 - pos.x,
                y: h - 1 - pos.y,
            };
            assert_eq!(
                map.tile(image),
                Some(tile),
                "{name}: tile ({}, {}) is not the image of its mirror",
                pos.x,
                pos.y
            );
            assert_eq!(
                map.elevation(image),
                map.elevation(pos),
                "{name}: tile ({}, {}) stands at another level than its mirror",
                pos.x,
                pos.y
            );
        }
        let (ew, eh) = BuildingKind::Extractor.size();
        for frame in map.extractor_frames() {
            let image = TilePos {
                x: w - ew - frame.x,
                y: h - eh - frame.y,
            };
            assert!(
                map.extractor_frames().contains(&image),
                "{name}: Extractor frame ({}, {}) has no rotated frame at ({}, {})",
                frame.x,
                frame.y,
                image.x,
                image.y
            );
        }

        let (fw, fh) = BuildingKind::Foundry.size();
        let anchor = |seat: PlayerId| {
            anchors.iter().find(|(p, _)| *p == seat).map_or_else(
                || panic!("{name}: seat {} has no Foundry anchor", seat.0),
                |(_, at)| *at,
            )
        };
        // An anchor names the footprint's top-left, so its image sits a
        // footprint in from the rotated corner.
        let partner = |seat: PlayerId| {
            let at = anchor(seat);
            let image = TilePos {
                x: w - fw - at.x,
                y: h - fh - at.y,
            };
            anchors.iter().find(|(_, a)| *a == image).map_or_else(
                || {
                    panic!(
                        "{name}: seat {}'s anchor rotates onto ({}, {}), where no seat sits",
                        seat.0, image.x, image.y
                    )
                },
                |(p, _)| *p,
            )
        };

        let state = scenario.build().expect("shipped maps build");
        for index in 0..scenario.players.len() {
            let seat = PlayerId::from_index(index);
            let mirror = partner(seat);
            assert_ne!(mirror, seat, "{name}: seat {index} is its own mirror");
            assert_eq!(
                partner(mirror),
                seat,
                "{name}: the anchor pairing is not an involution at seat {index}"
            );
            // A seat mirroring its own teammate would hand one team both
            // halves of the map's symmetry and leave the other pair
            // fighting across it.
            assert!(
                state.hostile(seat, mirror),
                "{name}: seat {index} mirrors teammate seat {}",
                mirror.0
            );
            assert_eq!(
                state.players()[index].scrap,
                state.players()[mirror.0 as usize].scrap,
                "{name}: seat {index} banks differently from its mirror"
            );

            let mine: Vec<_> = scenario
                .units
                .iter()
                .filter(|u| u.player == seat.0)
                .collect();
            let theirs: Vec<_> = scenario
                .units
                .iter()
                .filter(|u| u.player == mirror.0)
                .collect();
            assert_eq!(
                mine.len(),
                theirs.len(),
                "{name}: seat {index} starts {} units against its mirror's {}",
                mine.len(),
                theirs.len()
            );
            for (k, (a, b)) in mine.iter().zip(&theirs).enumerate() {
                assert_eq!(
                    (b.x, b.y),
                    (w - 1 - a.x, h - 1 - a.y),
                    "{name}: seat {index}'s unit #{k} is not placed opposite its mirror's",
                );
                assert_eq!(
                    a.kind, b.kind,
                    "{name}: seat {index}'s unit #{k} is a different kind than its mirror's"
                );
            }

            let mine: Vec<_> = scenario
                .buildings
                .iter()
                .filter(|b| b.player == seat.0)
                .collect();
            let theirs: Vec<_> = scenario
                .buildings
                .iter()
                .filter(|b| b.player == mirror.0)
                .collect();
            assert_eq!(
                mine.len(),
                theirs.len(),
                "{name}: seat {index} starts a different number of structures than its mirror"
            );
            for (k, (a, b)) in mine.iter().zip(&theirs).enumerate() {
                assert_eq!(
                    a.kind, b.kind,
                    "{name}: seat {index}'s structure #{k} differs in kind from its mirror's"
                );
                let (bw, bh) = a.kind.size();
                assert_eq!(
                    (b.x, b.y),
                    (w - bw - a.x, h - bh - a.y),
                    "{name}: seat {index}'s structure #{k} is not placed opposite its mirror's"
                );
            }
        }
    }
}
