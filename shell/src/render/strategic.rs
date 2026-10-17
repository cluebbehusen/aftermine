//! Spires, Lattices and the warheads between them: the silo deck at its
//! simulation heading with the round assembling on it, the battery's two
//! pods and reload arm, and each warhead's climb, cruise and dive over the
//! line it flies.

use super::*;
use crate::assets::{LatticeRig, SpireRig};
use crate::numeric::Fit;
use crate::render::prim::{fill_circle, line_between, stroke_circle};
use crate::render::sky::{
    Flight, INTERCEPTOR_TAIL, InterceptorPath, SCALE_PER_TILE, SHADOW_PER_TILE, WARHEAD_TAIL,
};
use oxide_sim::Building;

/// Where a Lattice pod turns, as a share of the footprint.
const POD_PIVOTS: [Vec2; 2] = [vec2(0.25, 62.0 / 128.0), vec2(0.75, 62.0 / 128.0)];

/// Seconds a pod holds its shot's bearing, then spends swinging north.
const POD_HOLD: f32 = 0.6;
const POD_RETURN: f32 = 0.8;

/// A pod's turning point in world coordinates. Pod 0 is the left one.
pub(crate) fn lattice_pod_pivot(b: &Building, pod: u8) -> Vec2 {
    let (w, h) = b.kind.size();
    vec2(b.anchor.x as f32, b.anchor.y as f32)
        + POD_PIVOTS[usize::from(pod.min(1))] * vec2(w as f32, h as f32)
}

/// The art faces north at zero and turns clockwise; the compass starts
/// east and turns clockwise too.
/// Ready ticks at which the Spire's upper and lower locks start to
/// release; each takes four.
const UPPER_RELEASE: usize = 2;
const LOWER_RELEASE: usize = 6;

fn deck_rotation(heading: u8) -> f32 {
    f32::from(heading) / 256.0 * std::f32::consts::TAU + std::f32::consts::FRAC_PI_2
}

fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// A closing lock or clamp's step over `from..to` of a round's progress.
fn closing_step(progress: f32, from: f32, to: f32) -> usize {
    numeric::to_usize((4.0 * smoothstep((progress - from) / (to - from))).round()).min(4)
}

/// One layer of a building's art: its source and tint, drawn over the
/// footprint or a sub-rectangle of it, turned about `pivot`.
struct Layer {
    source: Rect,
    tint: Color,
    origin: Vec2,
    size: Vec2,
    rotation: f32,
    pivot: Vec2,
}

fn draw_layers(sprites: &Sprites, zoom: f32, layers: &[Layer]) {
    for layer in layers {
        sprites.draw_building(
            layer.origin.x,
            layer.origin.y,
            layer.tint,
            DrawTextureParams {
                dest_size: Some(layer.size),
                source: Some(layer.source),
                rotation: layer.rotation,
                pivot: Some(layer.pivot),
                ..Default::default()
            },
            zoom,
        );
    }
}

/// A completed Spire: the fixed foundation, then the deck and everything
/// riding it, turned to the simulation heading.
pub(super) fn draw_spire(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    b: &Building,
    screen: Vec2,
    dest: Vec2,
    accent: Color,
) -> bool {
    let (Some(rigs), Some(magazine), Some(launcher), Some(stats)) = (
        sprites.strategic_rigs(),
        b.magazine,
        b.launcher,
        b.kind.magazine(),
    ) else {
        return false;
    };
    let rig: &SpireRig = &rigs.spire;
    let zoom = game.presentation.camera.zoom;
    let pivot = screen + dest * 0.5;
    let rotation = deck_rotation(launcher.heading);
    let mut layers = Vec::new();
    let mut push = |row: Rect, tint: Color, rotation: f32| {
        layers.push(Layer {
            source: row,
            tint,
            origin: screen,
            size: dest,
            rotation,
            pivot,
        });
    };
    push(rig.foundation[0], WHITE, 0.0);
    push(rig.foundation[1], accent, 0.0);
    push(rig.deck[0], WHITE, rotation);
    push(rig.deck[1], accent, rotation);
    let assembling = (magazine.ordered > 0 && magazine.loaded == 0)
        .then(|| magazine.progress as f32 / stats.round_ticks.max(1) as f32);
    let (lower, upper) = if magazine.loaded > 0 {
        // Once the deck stands aligned on its strike, the upper lock
        // releases, then the lower, and the round is free to fly.
        let ready = usize::from(launcher.ready);
        let release = |from: usize| 4 - ready.saturating_sub(from).min(4);
        (release(LOWER_RELEASE), release(UPPER_RELEASE))
    } else if let Some(p) = assembling {
        (
            closing_step(p, 1.0 / 3.0, 0.4),
            closing_step(p, 0.689, 0.756),
        )
    } else {
        (0, 0)
    };
    if magazine.loaded > 0 {
        push(rig.round[0], WHITE, rotation);
        push(rig.round[1], accent, rotation);
        push(rig.gantry_parked, WHITE, rotation);
    } else if let Some(p) = assembling {
        let carry = rig.gantry.len().saturating_sub(4).max(1);
        let frame = if p < 8.0 / 9.0 {
            (1 + numeric::to_usize((p * 9.0 / 8.0 * carry as f32).floor())).min(carry)
        } else {
            (carry + 1 + numeric::to_usize(((p - 8.0 / 9.0) * 36.0).floor())).min(rig.gantry.len())
        };
        let seated = if frame <= carry {
            (frame - 1) / 8
        } else {
            rig.stacks.len() + 1
        };
        if seated > rig.stacks.len() {
            push(rig.round[0], WHITE, rotation);
            push(rig.round[1], accent, rotation);
        } else if seated > 0 {
            push(rig.stacks[seated - 1], WHITE, rotation);
        }
        let nose_from = carry + 1 - rig.noses.len();
        if (nose_from..=carry).contains(&frame) {
            let nose = rig.noses[frame - nose_from];
            push(nose[0], WHITE, rotation);
            push(nose[1], accent, rotation);
        }
        push(rig.gantry[frame - 1], WHITE, rotation);
    } else {
        push(rig.gantry_parked, WHITE, rotation);
    }
    push(rig.cover[0], WHITE, rotation);
    push(rig.cover[1], accent, rotation);
    push(rig.lock_lower[lower], WHITE, rotation);
    push(rig.lock_upper[upper], WHITE, rotation);
    draw_layers(sprites, zoom, &layers);
    true
}

/// A completed Lattice: the base, each pod turned to its own bearing with
/// its interceptor or the parts of one, the reload arm serving the pod
/// being refilled, and the cover plate.
pub(super) fn draw_lattice(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    b: &Building,
    screen: Vec2,
    dest: Vec2,
    accent: Color,
) -> bool {
    let (Some(rigs), Some(magazine), Some(stats)) =
        (sprites.strategic_rigs(), b.magazine, b.kind.magazine())
    else {
        return false;
    };
    let rig: &LatticeRig = &rigs.lattice;
    let zoom = game.presentation.camera.zoom;
    let mut layers = Vec::new();
    let footprint = |source: Rect, tint: Color| Layer {
        source,
        tint,
        origin: screen,
        size: dest,
        rotation: 0.0,
        pivot: screen + dest * 0.5,
    };
    layers.push(footprint(rig.base, WHITE));
    // The pod being refilled, and how far along its round is.
    let rebuilding = (magazine.ordered > 0 && magazine.loaded < stats.capacity).then(|| {
        (
            magazine.loaded,
            magazine.progress as f32 / stats.round_ticks.max(1) as f32,
        )
    });
    let arm_carry = rig.arms[0].len().saturating_sub(2).max(1);
    let arm_frame = rebuilding.map(|(_, p)| {
        if p < 0.9 {
            (1 + numeric::to_usize((p / 0.9 * arm_carry as f32).floor())).min(arm_carry)
        } else {
            (arm_carry + 1 + numeric::to_usize(((p - 0.9) * 20.0).floor())).min(rig.arms[0].len())
        }
    });
    let aim = pod_aims(game, b, magazine.loaded);
    let tick = sky_tick(game);
    for pod in 0..2u8 {
        let firing = game
            .presentation
            .prelaunches
            .iter()
            .find(|launch| launch.battery == b.id && launch.pod == pod);
        let pivot = game
            .presentation
            .camera
            .to_screen(lattice_pod_pivot(b, pod));
        let size = Vec2::splat(zoom);
        let origin = pivot - size * 0.5;
        let rotation = aim[usize::from(pod)];
        let layer = |source: Rect, tint: Color| Layer {
            source,
            tint,
            origin,
            size,
            rotation,
            pivot,
        };
        layers.push(layer(rig.pod[0], WHITE));
        layers.push(layer(rig.pod[1], accent));
        let clamp = if let Some(launch) = firing {
            // The round is the interceptor; it leaves the pod empty.
            if !launch.launched(tick) {
                layers.push(layer(rig.interceptor[0], WHITE));
                layers.push(layer(rig.interceptor[1], accent));
            }
            4 - numeric::to_usize((4.0 * launch.opened(tick)).round())
        } else if pod < magazine.loaded {
            layers.push(layer(rig.interceptor[0], WHITE));
            layers.push(layer(rig.interceptor[1], accent));
            4
        } else if let (Some((filling, p)), Some(frame)) = (rebuilding, arm_frame)
            && filling == pod
        {
            if frame > arm_carry {
                layers.push(layer(rig.interceptor[0], WHITE));
                layers.push(layer(rig.interceptor[1], accent));
            } else if let Some(&parts) = rig.stacks.get(((frame - 1) / 4).wrapping_sub(1)) {
                layers.push(layer(parts, WHITE));
            }
            closing_step(p, 0.9, 1.0)
        } else {
            0
        };
        layers.push(layer(rig.clamp[clamp], WHITE));
    }
    if let (Some((pod, _)), Some(frame)) = (rebuilding, arm_frame) {
        let side = usize::from(pod.min(1));
        let noses = &rig.noses[side];
        let nose_from = arm_carry + 1 - noses.len();
        if (nose_from..=arm_carry).contains(&frame) {
            let nose = noses[frame - nose_from];
            layers.push(footprint(nose[0], WHITE));
            layers.push(footprint(nose[1], accent));
        }
        layers.push(footprint(rig.arms[side][frame - 1], WHITE));
    }
    layers.push(footprint(rig.cover, WHITE));
    draw_layers(sprites, zoom, &layers);
    true
}

/// Each pod's bearing: a pod that just fired holds toward its shot and
/// swings back north; for the battery's own team, the next pod to fire
/// tracks the warhead its forecast assigns; a pod whose stop is under way
/// swings onto its interceptor's path.
fn pod_aims(game: &crate::game::Scene<'_>, b: &Building, loaded: u8) -> [f32; 2] {
    let now = game.presentation.fx_time();
    let wrap = |angle: f32| {
        (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
    };
    let bearing = |d: Vec2| d.y.atan2(d.x) + std::f32::consts::FRAC_PI_2;
    let mut aims = [0.0; 2];
    for (pod, aim) in aims.iter_mut().enumerate() {
        let Some(&(angle, stamp)) = game.presentation.pod_shots.get(&(b.id.0, pod.fit::<u8>()))
        else {
            continue;
        };
        let age = now - stamp;
        let back = smoothstep((age - POD_HOLD) / POD_RETURN);
        *aim = wrap(angle) * (1.0 - back);
    }
    let tick = sky_tick(game);
    if loaded > 0 && !game.state.hostile(game.presentation.human, b.player) {
        let ready = usize::from(loaded - 1);
        if let Some(target) = game
            .state
            .known_warheads(game.presentation.human)
            .into_iter()
            .filter(|w| w.interceptor == Some(b.id))
            .filter_map(|w| w.line)
            .min_by_key(|line| line.arrival)
        {
            let d = Flight::of(target).ground(tick) - lattice_pod_pivot(b, ready.fit::<u8>());
            if d.length_squared() > 1e-4 {
                aims[ready] = wrap(bearing(d));
            }
        }
    }
    for launch in game
        .presentation
        .prelaunches
        .iter()
        .filter(|launch| launch.battery == b.id && !launch.launched(tick))
    {
        let pod = usize::from(launch.pod.min(1));
        let target = wrap(bearing(launch.path.heading()));
        let turn = wrap(target - aims[pod]);
        aims[pod] = wrap(aims[pod] + turn * smoothstep(launch.aimed(tick)));
    }
    aims
}

/// The presentation tick the sky draws at.
fn sky_tick(game: &crate::game::Scene<'_>) -> f32 {
    game.state.current_tick().saturating_sub(1) as f32 + game.presentation.tick_fraction()
}

/// Seconds before landing a warhead glows with re-entry and its strike
/// ring tightens onto the target.
const REENTRY: f32 = 0.4;

/// Warm white for heat and blast light: read by brightness, never hue.
const HEAT: Color = Color::new(1.0, 0.95, 0.82, 1.0);

/// A warhead's identity tint: its team's colour for the viewer's own and
/// allied warheads, the danger colour for a hostile one.
fn warhead_tint(
    game: &crate::game::Scene<'_>,
    player: Option<oxide_sim::PlayerId>,
    hostile: bool,
) -> Color {
    match player {
        Some(player) if !hostile => seat_identity_color(game, player),
        _ => DANGER,
    }
}

/// Where a flight stands in the sky on screen at `tick`, its height as a
/// share of the apex, and the screen direction it is moving.
/// One warhead in flight at `tick`, on the straight line from its deck to
/// its target: its shadow falling away as it climbs, the smoke trail
/// behind its nozzle, the body under power or coasting, and the re-entry
/// glow over its last moments.
fn draw_flight(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    flight: Flight,
    tick: f32,
    tint: Color,
) {
    let camera = &game.presentation.camera;
    let zoom = camera.zoom;
    let p = flight.progress(tick);
    let at = flight.ground(tick);
    let height = flight.altitude(tick);
    let scale = flight.scale(tick);
    let heading = flight.heading();
    let body = camera.to_screen(at);
    if at.distance(flight.launch) > 0.6 {
        draw_air_shadow(sprites, camera, at, height);
    }
    // Smoke from the nozzle, back along the line it has flown.
    let nozzle = |tick: f32| {
        camera.to_screen(flight.ground(tick) - heading * WARHEAD_TAIL * flight.scale(tick))
    };
    draw_trail(
        zoom,
        (1..=18).map(|step| tick - step as f32),
        flight.launched_at,
        nozzle,
        tick,
        0.16,
    );
    let remaining = (flight.arrival - tick) * crate::game::TICK_DT;
    let glow = (1.0 - remaining / REENTRY).clamp(0.0, 1.0);
    let size = Vec2::splat(2.5 * zoom * scale);
    if glow > 0.0 {
        // Re-entry: a hot streak behind the nose and a halo around it.
        let nose = body + heading * size.y * 0.22;
        line_between(
            nose,
            nose - heading * size.y * (0.35 + 0.5 * glow),
            0.28 * zoom * glow + 1.0,
            Color::new(HEAT.r, HEAT.g, HEAT.b, 0.55 * glow),
        );
        fill_circle(
            nose,
            0.55 * zoom * glow,
            Color::new(HEAT.r, HEAT.g, HEAT.b, 0.28 * glow),
        );
        fill_circle(
            nose,
            0.25 * zoom * glow,
            Color::new(1.0, 1.0, 1.0, 0.7 * glow),
        );
    }
    let rotation = heading.y.atan2(heading.x) + std::f32::consts::FRAC_PI_2;
    let Some(rows) = sprites.strategic_rigs().map(|rigs| &rigs.spire.warhead) else {
        fill_circle(body, 0.25 * zoom, tint);
        return;
    };
    // Under power through the boost and cruise, coasting in the dive.
    let row = if p > 0.86 || rows.len() < 2 {
        rows[0]
    } else {
        let flicker = numeric::to_usize((game.presentation.fx_time() * 18.0).floor());
        rows[1 + flicker % (rows.len() - 1)]
    };
    for (source, color) in [(row[0], WHITE), (row[1], tint)] {
        sprites.draw(
            body.x - size.x * 0.5,
            body.y - size.y * 0.5,
            color,
            DrawTextureParams {
                dest_size: Some(size),
                source: Some(source),
                rotation,
                pivot: Some(body),
                ..Default::default()
            },
        );
    }
}

/// A missile's shadow on the ground under `at`, falling away down and to
/// the right as it climbs `height` tiles, the way an aircraft's does.
fn draw_air_shadow(sprites: &Sprites, camera: &crate::camera::Camera, at: Vec2, height: f32) {
    let zoom = camera.zoom;
    let ground = camera.to_screen(at + SHADOW_PER_TILE * height);
    let size = Vec2::splat(zoom * 1.0 * (1.0 - 0.05 * height.min(5.0)));
    sprites.draw(
        ground.x - size.x * 0.5,
        ground.y - size.y * 0.5,
        Color::new(1.0, 1.0, 1.0, 0.5 - 0.05 * height.min(5.0)),
        DrawTextureParams {
            dest_size: Some(size),
            source: Some(sprites.air_shadow()),
            ..Default::default()
        },
    );
}

/// A smoke trail through the nozzle positions at `ticks`, newest first,
/// none before `start`, joining the nozzle at `now`; `width` in tiles.
fn draw_trail(
    zoom: f32,
    ticks: impl Iterator<Item = f32>,
    start: f32,
    nozzle: impl Fn(f32) -> Vec2,
    now: f32,
    width: f32,
) {
    let mut previous = nozzle(now);
    let ticks: Vec<f32> = ticks.collect();
    let count = ticks.len() as f32 + 1.0;
    for (index, back) in ticks.into_iter().enumerate() {
        if back < start {
            break;
        }
        let point = nozzle(back);
        let fade = 1.0 - (index as f32 + 1.0) / count;
        line_between(
            previous,
            point,
            (width * zoom * fade).max(1.0),
            Color::new(0.86, 0.85, 0.82, 0.34 * fade),
        );
        previous = point;
    }
}

/// Every warhead this seat may know of, drawn above the fog: a tracked
/// or own warhead on its full climb, cruise and dive with its shadow on
/// the ground and the strike ring where it will land; an untracked hostile
/// one as a contact marker filling toward lock. Interceptions in progress
/// draw here too, since they happen in the same sky.
pub(crate) fn draw_warheads(game: &crate::game::Scene<'_>, sprites: &Sprites) {
    let state = game.state;
    let human = game.presentation.human;
    let warheads = if game.presentation.all_seeing() {
        state.omniscient_warheads(human)
    } else {
        state.known_warheads(human)
    };
    let camera = &game.presentation.camera;
    let zoom = camera.zoom;
    let tick = state.current_tick().saturating_sub(1) as f32 + game.presentation.tick_fraction();
    for warhead in warheads {
        let owner = warhead_tint(game, warhead.player, warhead.hostile);
        let Some(line) = warhead.line else {
            // Everyone sees missiles true to fog: a hostile warhead whose
            // line this seat has not tracked shows as the missile it is
            // while over ground in sight, else as its radar contact.
            let ground = crate::game::world_vec(warhead.position);
            let seen = super::sky::missile_seen(game, warhead.hostile, ground).then(|| {
                state
                    .warheads()
                    .iter()
                    .find(|flying| flying.id == warhead.id)
            });
            if let Some(Some(flying)) = seen {
                let line = oxide_sim::strike::WarheadLine {
                    launch: flying.launch,
                    impact: flying.impact,
                    arrival: flying.arrival,
                };
                draw_flight(game, sprites, Flight::of(line), tick, owner);
            } else {
                draw_contact(camera.to_screen(ground), zoom, warhead.exposure, DANGER);
            }
            continue;
        };
        let flight = Flight::of(line);
        // The strike ring: where it falls and how far the blast reaches.
        let impact = camera.to_screen(flight.impact);
        let blast = oxide_sim::stats::WARHEAD;
        let outer = blast.outer_radius.to_num::<f32>() * zoom;
        let full = blast.full_radius.to_num::<f32>() * zoom;
        let ring = Color::new(
            owner.r,
            owner.g,
            owner.b,
            if warhead.hostile { 0.85 } else { 0.5 },
        );
        stroke_circle(impact, outer, 2.0, ring);
        stroke_circle(
            impact,
            full,
            1.5,
            Color::new(ring.r, ring.g, ring.b, ring.a * 0.7),
        );
        if warhead.hostile {
            let tick_len = 0.6 * zoom;
            for dir in [
                vec2(1.0, 0.0),
                vec2(-1.0, 0.0),
                vec2(0.0, 1.0),
                vec2(0.0, -1.0),
            ] {
                line_between(
                    impact + dir * (outer - tick_len),
                    impact + dir * (outer + tick_len),
                    2.0,
                    ring,
                );
            }
        }
        // Over its last moments a ring closes from beyond the blast onto
        // the full-damage core.
        let remaining = (flight.arrival - tick) * crate::game::TICK_DT;
        let closing = (1.0 - remaining / REENTRY).clamp(0.0, 1.0);
        if closing > 0.0 {
            let radius = outer * 1.6 + (full - outer * 1.6) * smoothstep(closing);
            stroke_circle(
                impact,
                radius,
                2.5,
                Color::new(ring.r, ring.g, ring.b, 0.35 + 0.55 * closing),
            );
        }
        // The ground track still to fly.
        let from = camera.to_screen(flight.ground(tick));
        let track = Color::new(owner.r, owner.g, owner.b, 0.28);
        let span = impact - from;
        let dashes = numeric::to_usize((span.length() / (0.5 * zoom)).floor()).min(400);
        for dash in (0..dashes).step_by(2) {
            let a = from + span * (dash as f32 / dashes.max(1) as f32);
            let b = from + span * ((dash + 1) as f32 / dashes.max(1) as f32);
            line_between(a, b, 1.5, track);
        }
        let ground = flight.ground(tick);
        if super::sky::missile_seen(game, warhead.hostile, ground) {
            draw_flight(game, sprites, flight, tick, owner);
        } else {
            // Its line is known, but the missile is over ground out of
            // sight: a contact marker where the line puts it.
            draw_contact(camera.to_screen(ground), zoom, warhead.exposure, owner);
        }
    }
    // Interceptors climbing ahead of their forecast stops to meet their
    // warheads nose to nose where the simulation stops them.
    for launch in &game.presentation.prelaunches {
        if !launch.launched(tick) || tick >= launch.stop {
            continue;
        }
        let hostile = state.hostile(game.presentation.human, launch.battery_player);
        if super::sky::missile_seen(game, hostile, launch.path.at(tick)) {
            draw_interceptor(
                game,
                sprites,
                &launch.path,
                tick,
                seat_identity_color(game, launch.battery_player),
            );
        }
    }
    for fx in &game.presentation.fx {
        if let crate::game::EffectKind::Interception {
            line,
            warhead_player,
            warhead_hostile,
            path,
            battery_player,
            contact,
            contact_tick,
            seed,
            ..
        } = fx.kind
        {
            let battery_hostile = state.hostile(game.presentation.human, battery_player);
            let flying_on = tick < contact_tick + crate::game::INTERCEPT_OVERLAP_TICKS;
            // Both fly on into the burst, which swallows them.
            if let Some(line) = line
                && flying_on
            {
                let flight = Flight::of(line);
                if tick < flight.arrival
                    && super::sky::missile_seen(game, warhead_hostile, flight.ground(tick))
                {
                    draw_flight(
                        game,
                        sprites,
                        flight,
                        tick,
                        warhead_tint(game, warhead_player, warhead_hostile),
                    );
                }
            }
            if let Some(path) = path
                && flying_on
                && super::sky::missile_seen(game, battery_hostile, path.at(tick))
            {
                draw_interceptor(
                    game,
                    sprites,
                    &path,
                    tick,
                    seat_identity_color(game, battery_player),
                );
            }
            if tick >= contact_tick {
                // Out of sight, a seat whose missile it was sees only the
                // puff of smoke it ended in; anyone else sees nothing.
                let in_sight = super::sky::ground_seen_by(game.presentation, game.state, contact);
                if in_sight || !warhead_hostile || !battery_hostile {
                    let drift = line.map_or(Vec2::ZERO, |line| Flight::of(line).heading());
                    draw_sky_burst(game, contact, tick - contact_tick, drift, seed, in_sight);
                }
            }
        }
    }
}

/// A hostile warhead held in sight or radar but not yet tracked: a hollow
/// chevron with a ring filling toward lock.
fn draw_contact(center: Vec2, zoom: f32, exposure: Option<u8>, tint: Color) {
    let r = 0.45 * zoom;
    for (a, b) in [
        (vec2(0.0, -r), vec2(r * 0.8, r * 0.7)),
        (vec2(r * 0.8, r * 0.7), vec2(0.0, r * 0.25)),
        (vec2(0.0, r * 0.25), vec2(-r * 0.8, r * 0.7)),
        (vec2(-r * 0.8, r * 0.7), vec2(0.0, -r)),
    ] {
        line_between(center + a, center + b, 2.0, tint);
    }
    let held = f32::from(exposure.unwrap_or(0)) / f32::from(oxide_sim::stats::WARHEAD_TRACK_TICKS);
    crate::render::prim::stroke_arc(center, r * 1.6, 2.0, 360.0 * held.clamp(0.0, 1.0), tint);
}

/// An interceptor at `tick` on its straight climb: its shadow falling away,
/// smoke from its nozzle back to its pod, and the round under power.
fn draw_interceptor(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    path: &InterceptorPath,
    tick: f32,
    tint: Color,
) {
    let camera = &game.presentation.camera;
    let zoom = camera.zoom;
    let at = path.at(tick);
    let height = path.altitude_at(tick);
    let heading = path.heading();
    if at.distance(path.from) > 0.4 {
        draw_air_shadow(sprites, camera, at, height);
    }
    let scale = |tick: f32| 1.0 + SCALE_PER_TILE * path.altitude_at(tick);
    let nozzle =
        |tick: f32| camera.to_screen(path.at(tick) - heading * INTERCEPTOR_TAIL * scale(tick));
    draw_trail(
        zoom,
        (1..=12).map(|step| tick - step as f32),
        path.launch,
        nozzle,
        tick,
        0.12,
    );
    let body = camera.to_screen(at);
    let Some(rows) = sprites.strategic_rigs().map(|rigs| &rigs.lattice.launch) else {
        fill_circle(body, 0.15 * zoom, WHITE);
        return;
    };
    let rotation = heading.y.atan2(heading.x) + std::f32::consts::FRAC_PI_2;
    let flicker = numeric::to_usize((game.presentation.fx_time() * 30.0).floor());
    let row = rows[1 + flicker % (rows.len() - 1).max(1)];
    let size = Vec2::splat(2.0 * zoom * scale(tick));
    for (source, color) in [(row[0], WHITE), (row[1], tint)] {
        sprites.draw(
            body.x - size.x * 0.5,
            body.y - size.y * 0.5,
            color,
            DrawTextureParams {
                dest_size: Some(size),
                source: Some(source),
                rotation,
                pivot: Some(body),
                ..Default::default()
            },
        );
    }
}

/// A deterministic share in `0..1` for fragment `index` of seed `seed`.
fn scatter(seed: u32, index: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ index.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65_535.0
}

/// A soft disc: fine layered circles fading toward the rim.
fn soft_disc(centre: Vec2, radius: f32, color: Color) {
    if radius < 0.5 || color.a <= 0.005 {
        return;
    }
    for layer in 0..8u8 {
        fill_circle(
            centre,
            radius * (1.0 - f32::from(layer) / 9.0),
            Color::new(color.r, color.g, color.b, color.a * 0.2),
        );
    }
}

/// The sky burst where an interceptor met a warhead, `age` ticks after
/// contact at `at`: a white flash big enough to swallow both noses, a
/// fireball rolling on along the warhead's `drift`, smoke that spreads and
/// drifts, and fragments falling away on short streaks. Out of sight only
/// the smoke shows.
fn draw_sky_burst(
    game: &crate::game::Scene<'_>,
    at: Vec2,
    age: f32,
    drift: Vec2,
    seed: u32,
    in_sight: bool,
) {
    let camera = &game.presentation.camera;
    let zoom = camera.zoom;
    let px = zoom / 64.0;
    let roll = |rate: f32| camera.to_screen(at + drift * rate * age * 1.4 / 64.0);
    if age < 34.0 {
        let k = age / 34.0;
        let build = ((age + 1.0) / 4.0).min(1.0) * (1.0 - k);
        let centre = roll(0.6) - vec2(0.0, 14.0 * k * px);
        soft_disc(
            centre,
            (20.0 + 24.0 * k) * px,
            Color::new(0.41, 0.4, 0.38, 0.8 * build),
        );
        for puff in 0..7u32 {
            let angle = puff as f32 * 0.9 + 0.4 + scatter(seed, puff) * 0.3;
            let reach = (6.0 + 18.0 * k * (0.6 + 0.4 * scatter(seed, puff + 10))) * px;
            soft_disc(
                centre + vec2(angle.cos(), angle.sin() * 0.7) * reach,
                (10.0 + 12.0 * k) * px,
                Color::new(0.43, 0.42, 0.4, 0.32 * build),
            );
        }
    }
    if !in_sight {
        return;
    }
    if age < 12.0 {
        let k = age / 12.0;
        let centre = roll(0.8);
        soft_disc(
            centre,
            (24.0 + 14.0 * k) * px,
            Color::new(0.91, 0.5, 0.2, 0.9 * (1.0 - k).powf(0.8)),
        );
        soft_disc(
            centre,
            (13.0 + 6.0 * k) * px,
            Color::new(1.0, 0.77, 0.43, 0.85 * (1.0 - k)),
        );
    }
    let centre = camera.to_screen(at);
    if age < 5.0 {
        let k = age / 5.0;
        soft_disc(
            centre,
            (54.0 + 18.0 * k) * px,
            Color::new(1.0, 0.93, 0.8, 0.75 * (1.0 - k)),
        );
        fill_circle(
            centre,
            (22.0 - 8.0 * k) * px,
            Color::new(1.0, 1.0, 1.0, 1.0 - 0.7 * k),
        );
    }
    if (5.0..23.0).contains(&age) {
        let t = age - 2.0;
        let fade = 1.0 - (age - 5.0) / 18.0;
        let push = drift * 1.4 * 0.5;
        for index in 0..6u32 {
            let angle = (scatter(seed, index + 20) * 360.0 - 180.0).to_radians();
            let speed = 1.4 + 2.7 * scatter(seed, index + 30);
            let fall = 0.4 + 0.45 * scatter(seed, index + 40);
            let head = centre
                + vec2(
                    angle.cos() * speed * 1.3 * t + push.x * t,
                    angle.sin() * speed * 0.8 * t + fall * t * t * 0.5 + push.y * t,
                ) * px;
            let velocity = vec2(angle.cos() * speed, angle.sin() * speed * 0.8 + fall * t);
            let tail =
                head - velocity.normalize_or_zero() * (4.0 + 3.0 * scatter(seed, index + 50)) * px;
            line_between(
                tail,
                head,
                (2.0 * px).max(1.0),
                Color::new(0.59, 0.56, 0.52, 0.67 * fade),
            );
            fill_circle(
                head,
                (1.2 * px).max(0.75),
                Color::new(0.86, 0.8, 0.7, 0.86 * fade),
            );
        }
    }
}

/// A warhead's ground burst `age` seconds in: a white flash, a shockwave
/// out to the blast's edge with dust behind it, a fireball that cools from
/// white through orange to soot as it rolls upward, and a smoke column
/// rising high over the crater.
pub(super) fn draw_warhead_burst(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    at: Vec2,
    seed: u32,
    age: f32,
) {
    let camera = &game.presentation.camera;
    let zoom = camera.zoom;
    let center = camera.to_screen(at);
    let blast = oxide_sim::stats::WARHEAD;
    let outer = blast.outer_radius.to_num::<f32>() * zoom;
    let full = blast.full_radius.to_num::<f32>() * zoom;
    let life = crate::game::WARHEAD_BURST;
    // Dust kicked up behind the shockwave, settling low.
    if (0.1..1.6).contains(&age) {
        let k = (age - 0.1) / 1.5;
        let radius = outer * (0.6 + 0.5 * (1.0 - (1.0 - k) * (1.0 - k)));
        stroke_circle(
            center,
            radius,
            zoom * 0.5 * (1.0 - k) + 1.0,
            Color::new(0.45, 0.4, 0.34, 0.3 * (1.0 - k)),
        );
    }
    // The column, so the fireball and flash sit over its foot.
    if age > 0.15 {
        let k = ((age - 0.15) / (life - 0.15)).clamp(0.0, 1.0);
        let rise = 1.0 - (1.0 - k) * (1.0 - k);
        let thin = (1.0 - k).powf(1.4);
        for puff in 0..12u32 {
            let level = puff as f32 / 11.0;
            let drift = (scatter(seed, puff) - 0.5) * zoom * (0.4 + 0.8 * level) * rise;
            let height = zoom * 5.0 * level * rise + zoom * 0.3;
            let radius =
                zoom * (0.3 + 0.55 * level * rise) * (0.8 + 0.4 * scatter(seed, puff + 80));
            let shade = 0.3 + 0.16 * level + 0.06 * scatter(seed, puff + 90);
            fill_circle(
                center + vec2(drift, -height),
                radius,
                Color::new(shade, shade * 0.97, shade * 0.93, 0.5 * thin),
            );
        }
        // The cap spreads wide at the top.
        for puff in 0..4u32 {
            let side = scatter(seed, puff + 100) - 0.5;
            let cap = center + vec2(side * zoom * 2.6 * rise, -zoom * (5.0 * rise + 0.5));
            fill_circle(
                cap,
                zoom * (0.4 + 0.6 * rise),
                Color::new(0.5, 0.49, 0.47, 0.4 * thin),
            );
        }
    }
    // The fireball, rolling upward as it cools.
    if age < 1.3 {
        let k = age / 1.3;
        let swell = 1.0 - (1.0 - (age / 0.25).min(1.0)).powi(2);
        let heat = (1.0 - k).max(0.0);
        for blob in 0..9u32 {
            let rank = blob as f32 / 8.0;
            let angle = scatter(seed, blob + 20) * std::f32::consts::TAU;
            let reach = full * (0.15 + 0.45 * scatter(seed, blob + 30)) * swell;
            let offset = vec2(angle.cos(), angle.sin() * 0.8) * reach - vec2(0.0, zoom * 1.2 * k);
            // Outer blobs cool first; the heart stays hot longest.
            let t = (heat * (1.0 - 0.45 * rank)).clamp(0.0, 1.0);
            let color = if t > 0.5 {
                let w = (t - 0.5) * 2.0;
                Color::new(1.0, 0.62 + 0.35 * w, 0.25 + 0.55 * w, 0.8)
            } else {
                let w = t * 2.0;
                Color::new(0.3 + 0.7 * w, 0.24 + 0.38 * w, 0.2 + 0.05 * w, 0.75)
            };
            fill_circle(
                center + offset,
                full * (0.32 + 0.22 * (1.0 - rank)) * (0.6 + 0.4 * swell),
                Color::new(color.r, color.g, color.b, color.a * (1.0 - k * k)),
            );
        }
        // White-hot heart.
        fill_circle(
            center - vec2(0.0, zoom * 1.2 * k),
            full * 0.35 * (1.0 - k).powi(2),
            Color::new(1.0, 0.98, 0.9, 0.9 * (1.0 - k)),
        );
        super::destruction::draw_hit(
            sprites,
            center,
            zoom,
            outer / zoom * 0.7,
            (age / 0.6).min(1.0),
        );
    }
    // The shockwave racing out to the blast's edge.
    if age < 0.55 {
        let k = age / 0.55;
        let radius = outer * (1.0 - (1.0 - k) * (1.0 - k));
        stroke_circle(
            center,
            radius,
            zoom * 0.3 * (1.0 - k) + 1.0,
            Color::new(HEAT.r, HEAT.g, HEAT.b, 0.85 * (1.0 - k)),
        );
    }
    // The flash.
    if age < 0.15 {
        let k = age / 0.15;
        fill_circle(
            center,
            full * (0.8 + 1.6 * k),
            Color::new(1.0, 1.0, 1.0, 0.9 * (1.0 - k)),
        );
    }
}

/// Screen pixels between a strike preview's warning stripes, before UI
/// scale: the stripes keep their pitch at every zoom rather than turning to
/// moiré when zoomed out.
const HAZARD_STEP: f32 = 12.0;
/// How fast the stripes crawl, in unscaled pixels a second.
const HAZARD_DRIFT: f32 = 10.0;

/// The 45-degree stripes, every `step` pixels shifted by `phase`, that
/// cover the ring between radii `inner` and `outer` without entering the
/// inner disc, as segments relative to the centre.
fn hazard_stripes(inner: f32, outer: f32, step: f32, phase: f32) -> Vec<(Vec2, Vec2)> {
    let (along, across) = (vec2(1.0, -1.0).normalize(), vec2(1.0, 1.0).normalize());
    let half = |radius: f32, offset: f32| (radius * radius - offset * offset).max(0.0).sqrt();
    let lines = numeric::to_usize((outer * 2.0 / step).ceil()).min(400) + 1;
    let mut stripes = Vec::with_capacity(lines * 2);
    for k in 0..lines {
        let offset = -outer + phase + k as f32 * step;
        if offset.abs() >= outer {
            continue;
        }
        let (base, reach) = (across * offset, half(outer, offset));
        if offset.abs() < inner {
            let gap = half(inner, offset);
            stripes.push((base - along * reach, base - along * gap));
            stripes.push((base + along * gap, base + along * reach));
        } else {
            stripes.push((base - along * reach, base + along * reach));
        }
    }
    stripes
}

/// The blast a strike would bring: at the touch aim or under the cursor
/// while a Launch is armed, and at each selected own Spire's ordered tile, with a line from
/// the silo.
pub(crate) fn draw_strike_preview(game: &crate::game::Scene<'_>, input: &InputState) {
    let camera = &game.presentation.camera;
    let zoom = camera.zoom;
    let blast = oxide_sim::stats::WARHEAD;
    let human = game.presentation.human;
    let own = seat_identity_color(game, human);
    let (s, motion) = (ui_scale(), !super::reduced_motion());
    let drift = if motion {
        game.presentation.fx_time() * HAZARD_DRIFT * s
    } else {
        0.0
    };
    // The full-damage core is a solid disc; warning stripes fill the band
    // the blast fades across. `alpha` is how strongly this ring speaks.
    let ring = |center: Vec2, alpha: f32| {
        let tint = |a: f32| Color::new(own.r, own.g, own.b, a * alpha);
        let (outer, full) = (
            blast.outer_radius.to_num::<f32>() * zoom,
            blast.full_radius.to_num::<f32>() * zoom,
        );
        fill_circle(center, outer, tint(0.16));
        let step = HAZARD_STEP * s;
        // Inset a pixel each side so a stripe's width stays within the edges.
        for (a, b) in hazard_stripes(full + 1.0, outer - 1.0, step, drift % step) {
            line_between(center + a, center + b, step * 0.42, tint(0.55));
        }
        fill_circle(center, full, tint(0.5));
        stroke_circle(center, outer, 2.5, tint(1.25));
        stroke_circle(center, full, 2.0, Color::new(0.92, 0.94, 0.98, 1.1 * alpha));
    };
    let silo_line = |b: &Building, to: Vec2, alpha: f32| {
        line_between(
            camera.to_screen(crate::game::world_vec(b.center())),
            to,
            1.5,
            Color::new(own.r, own.g, own.b, alpha),
        );
    };
    if let Some(strike) = &input.striking {
        // A touch aim, once down, is where it lands; else the mouse's hover.
        let tile = strike.aim().or_else(|| {
            (input.last_pointer == crate::input::Pointer::Mouse)
                .then(|| crate::input::ground_tile(game.state, camera.to_world(input.mouse)))
        });
        if let Some(tile) = tile {
            let center = camera.to_screen(crate::input::tile_center(tile));
            ring(center, 0.75);
            for b in strike
                .spires
                .iter()
                .filter_map(|id| game.state.building(*id))
            {
                silo_line(b, center, 0.4);
            }
        }
    }
    for b in game
        .presentation
        .selection
        .buildings
        .iter()
        .filter_map(|id| game.state.building(*id))
        .filter(|b| b.player == human)
    {
        if let Some(target) = b.launcher.and_then(|launcher| launcher.target) {
            let center = camera.to_screen(crate::input::tile_center(target));
            ring(center, 0.55);
            silo_line(b, center, 0.35);
        }
    }
}

#[cfg(test)]
mod tests;
