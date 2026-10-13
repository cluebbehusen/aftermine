//! Spires, Lattices and the warheads between them: the silo deck at its
//! simulation heading with the round assembling on it, the battery's two
//! pods and reload arm, and each warhead's climb, cruise and dive over the
//! line it flies.

use super::*;
use crate::assets::{LatticeRig, SpireRig};
use crate::numeric::Fit;
use crate::render::prim::{fill_circle, line_between, stroke_circle};
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
        // The locks spring open during the last of the swing, the upper
        // one first, so the round is free as the deck lines up.
        let remaining = launcher.target.map_or(u32::MAX, |target| {
            let desired = chassis::compass::heading_of(target.center() - b.center());
            u32::from(
                desired
                    .wrapping_sub(launcher.heading)
                    .cast_signed()
                    .unsigned_abs(),
            ) / u32::from(b.kind.silo().map_or(1, |silo| silo.turn_rate.max(1)))
        });
        (
            if remaining <= 4 { 0 } else { 4 },
            if remaining <= 8 { 0 } else { 4 },
        )
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
    for pod in 0..2u8 {
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
        let clamp = if pod < magazine.loaded {
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
/// tracks the warhead its forecast assigns.
fn pod_aims(game: &crate::game::Scene<'_>, b: &Building, loaded: u8) -> [f32; 2] {
    let now = game.presentation.fx_time();
    let mut aims = [0.0; 2];
    for (pod, aim) in aims.iter_mut().enumerate() {
        let Some(&(angle, stamp)) = game.presentation.pod_shots.get(&(b.id.0, pod.fit::<u8>()))
        else {
            continue;
        };
        let age = now - stamp;
        let back = smoothstep((age - POD_HOLD) / POD_RETURN);
        let swing =
            (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        *aim = swing * (1.0 - back);
    }
    if loaded > 0 && !game.state.hostile(game.presentation.human, b.player) {
        let ready = usize::from(loaded - 1);
        let tick =
            game.state.current_tick().saturating_sub(1) as f32 + game.presentation.tick_fraction();
        if let Some(target) = game
            .state
            .known_warheads(game.presentation.human)
            .into_iter()
            .filter(|w| w.interceptor == Some(b.id))
            .filter_map(|w| w.line)
            .min_by_key(|line| line.arrival)
        {
            let ground = Flight::of(target).ground(tick);
            let d = ground - lattice_pod_pivot(b, ready.fit::<u8>());
            if d.length_squared() > 1e-4 {
                let angle = d.y.atan2(d.x) + std::f32::consts::FRAC_PI_2;
                aims[ready] = (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                    - std::f32::consts::PI;
            }
        }
    }
    aims
}

/// A warhead's whole flight in presentation terms.
#[derive(Clone, Copy)]
struct Flight {
    launch: Vec2,
    impact: Vec2,
    launched_at: f32,
    arrival: f32,
}

impl Flight {
    fn of(line: oxide_sim::strike::WarheadLine) -> Self {
        let ticks = oxide_sim::stats::WARHEAD.flight_ticks(line.launch, line.impact);
        Self {
            launch: crate::game::world_vec(line.launch),
            impact: crate::game::world_vec(line.impact),
            launched_at: line.arrival.saturating_sub(ticks) as f32,
            arrival: line.arrival as f32,
        }
    }

    fn progress(self, tick: f32) -> f32 {
        ((tick - self.launched_at) / (self.arrival - self.launched_at).max(1.0)).clamp(0.0, 1.0)
    }

    fn ground(self, tick: f32) -> Vec2 {
        self.launch + (self.impact - self.launch) * self.progress(tick)
    }

    /// How high the warhead stands, in tiles of screen lift: a steep
    /// boost, a long high cruise, and a steep dive onto the target.
    fn apex(self) -> f32 {
        (self.launch.distance(self.impact) * 0.22).clamp(2.0, 9.0)
    }
}

/// Height as a share of the apex, and its slope, along the flight.
fn altitude(p: f32) -> (f32, f32) {
    const SHAPE: f32 = 0.55;
    let s = (std::f32::consts::PI * p).sin().max(0.02);
    let height = s.powf(SHAPE);
    let slope =
        SHAPE * s.powf(SHAPE - 1.0) * (std::f32::consts::PI * p).cos() * std::f32::consts::PI;
    (height, slope)
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
fn sky_pose(camera: &crate::camera::Camera, flight: Flight, tick: f32) -> (Vec2, f32, Vec2) {
    let zoom = camera.zoom;
    let p = flight.progress(tick);
    let (height, slope) = altitude(p);
    let apex = flight.apex();
    let body = camera.to_screen(flight.ground(tick)) - vec2(0.0, apex * zoom * height);
    let velocity = (flight.impact - flight.launch) * zoom - vec2(0.0, apex * zoom * slope);
    (body, height, velocity)
}

/// One warhead in flight at `tick`: its ground shadow, the smoke trail
/// along the arc it has flown, the body under power or coasting, and the
/// re-entry glow over its last moments.
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
    let (body, height, velocity) = sky_pose(camera, flight, tick);
    // Its shadow keeps to the ground, fading as the warhead climbs.
    let ground = camera.to_screen(flight.ground(tick));
    let shadow_size = Vec2::splat(zoom * 1.1 * (1.0 - 0.45 * height));
    sprites.draw(
        ground.x - shadow_size.x * 0.5,
        ground.y - shadow_size.y * 0.5,
        Color::new(1.0, 1.0, 1.0, 0.55 * (1.0 - 0.75 * height)),
        DrawTextureParams {
            dest_size: Some(shadow_size),
            source: Some(sprites.air_shadow()),
            ..Default::default()
        },
    );
    // A smoke trail along the arc it has flown.
    let span = (flight.arrival - flight.launched_at).max(1.0);
    let mut previous = body;
    for step in 1..=14 {
        let back = tick - step as f32 * 0.012 * span;
        if back <= flight.launched_at {
            break;
        }
        let (point, _, _) = sky_pose(camera, flight, back);
        let fade = 1.0 - step as f32 / 15.0;
        line_between(
            previous,
            point,
            (0.16 * zoom * fade).max(1.0),
            Color::new(0.86, 0.85, 0.82, 0.32 * fade),
        );
        previous = point;
    }
    let remaining = (flight.arrival - tick) * crate::game::TICK_DT;
    let glow = (1.0 - remaining / REENTRY).clamp(0.0, 1.0);
    let heading = velocity.normalize_or_zero();
    let size = Vec2::splat(2.5 * zoom * (1.0 + 0.5 * height));
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
    let rotation = velocity.y.atan2(velocity.x) + std::f32::consts::FRAC_PI_2;
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
    // Interceptors launched ahead of their forecast stops, climbing to
    // meet their warheads where the simulation stops them.
    for launch in &game.presentation.prelaunches {
        let climbed = launch.climbed(tick);
        let hostile = state.hostile(game.presentation.human, launch.battery_player);
        if !super::sky::missile_seen(game, hostile, launch.from.lerp(launch.at, climbed)) {
            continue;
        }
        let (sky, _, _) = sky_pose(camera, Flight::of(launch.line), launch.stop);
        draw_interceptor(
            game,
            sprites,
            camera.to_screen(launch.from),
            sky,
            seat_identity_color(game, launch.battery_player),
            climbed,
        );
    }
    let completed = state.current_tick();
    let fraction = game.presentation.tick_fraction();
    for fx in &game.presentation.fx {
        if let crate::game::EffectKind::Interception {
            line,
            warhead_player,
            warhead_hostile,
            pod,
            battery_player,
            at,
            seed,
            completed_tick,
            early,
            ..
        } = fx.kind
        {
            draw_interception(
                game,
                sprites,
                Interception {
                    line,
                    tint: warhead_tint(game, warhead_player, warhead_hostile),
                    pod,
                    battery: seat_identity_color(game, battery_player),
                    at,
                    seed,
                    stopped: completed_tick.saturating_sub(1) as f32,
                    early,
                    warhead_hostile,
                    battery_hostile: state.hostile(game.presentation.human, battery_player),
                },
                fx.age_at(completed, fraction),
            );
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

/// What an interception draws from.
#[derive(Clone, Copy)]
struct Interception {
    line: Option<oxide_sim::strike::WarheadLine>,
    tint: Color,
    pod: Option<Vec2>,
    battery: Color,
    at: Vec2,
    seed: u32,
    /// The tick the simulation stopped the warhead on.
    stopped: f32,
    /// Whether the interceptor launched ahead and met the warhead at its
    /// stop.
    early: bool,
    /// Whether the warhead is hostile to this viewer.
    warhead_hostile: bool,
    /// Whether the interceptor is.
    battery_hostile: bool,
}

/// Tiles above its stop an unknown warhead is taken to fly when this seat
/// never knew its line.
const UNKNOWN_LIFT: f32 = 3.0;

/// A deterministic share in `0..1` for fragment `index` of seed `seed`.
fn scatter(seed: u32, index: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ index.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65_535.0
}

/// A stopped warhead flying on as a ghost while the interceptor climbs out
/// of its pod on a curve to meet it in the sky; then a burst up there, its
/// fragments falling and fading before they reach the ground.
fn draw_interception(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    meeting: Interception,
    age: f32,
) {
    let camera = &game.presentation.camera;
    let zoom = camera.zoom;
    let meet = crate::game::INTERCEPT_MEET;
    let flight = meeting.line.map(Flight::of);
    let meet_tick = if meeting.early {
        meeting.stopped
    } else {
        meeting.stopped + meet / crate::game::TICK_DT
    };
    // A hostile missile shows only over ground this seat sees.
    let warhead_seen =
        |ground: Vec2| super::sky::missile_seen(game, meeting.warhead_hostile, ground);
    let interceptor_seen =
        |ground: Vec2| super::sky::missile_seen(game, meeting.battery_hostile, ground);
    // Where they meet, high over the warhead's ground track, and how far
    // the ground lies below.
    let (sky, drop) = match flight {
        Some(flight) => {
            let (body, height, _) = sky_pose(camera, flight, meet_tick);
            (body, flight.apex() * zoom * height)
        }
        None => (
            camera.to_screen(meeting.at) - vec2(0.0, UNKNOWN_LIFT * zoom),
            UNKNOWN_LIFT * zoom,
        ),
    };
    let below = flight.map_or(meeting.at, |flight| flight.ground(meet_tick));
    if age < meet {
        if let Some(flight) = flight {
            let tick = meeting.stopped + age / crate::game::TICK_DT;
            if tick < flight.arrival && warhead_seen(flight.ground(tick)) {
                draw_flight(game, sprites, flight, tick, meeting.tint);
            }
        }
        if let Some(pod) = meeting.pod
            && interceptor_seen(pod.lerp(below, age / meet))
        {
            draw_interceptor(
                game,
                sprites,
                camera.to_screen(pod),
                sky,
                meeting.battery,
                age / meet,
            );
        }
        return;
    }
    // Out of sight, a seat whose missile it was sees only the puff of smoke
    // it ended in; a seat with neither missile sees nothing.
    let in_sight = super::sky::ground_seen_by(game.presentation, game.state, below);
    if !in_sight && meeting.warhead_hostile && meeting.battery_hostile {
        return;
    }
    let after = age - meet;
    // The flash where they met: a white core, then a halo racing out.
    if in_sight && after < 0.3 {
        let k = after / 0.3;
        let core = (after / 0.1).min(1.0);
        fill_circle(
            sky,
            zoom * (0.25 + 0.6 * core) * (1.0 - 0.5 * k),
            Color::new(1.0, 1.0, 1.0, 0.95 * (1.0 - k)),
        );
        stroke_circle(
            sky,
            zoom * (0.4 + 1.4 * k),
            0.25 * zoom * (1.0 - k) + 1.0,
            Color::new(HEAT.r, HEAT.g, HEAT.b, 0.8 * (1.0 - k)),
        );
    }
    // Ragged smoke that spreads and drifts up there.
    if after < 1.2 {
        let k = after / 1.2;
        for puff in 0..6u32 {
            let angle = scatter(meeting.seed, puff + 40) * std::f32::consts::TAU;
            let reach = zoom * (0.2 + 0.7 * scatter(meeting.seed, puff + 50)) * (0.3 + k);
            let centre =
                sky + vec2(angle.cos(), angle.sin() * 0.7) * reach - vec2(0.0, zoom * 0.35 * k);
            let shade = 0.42 + 0.14 * scatter(meeting.seed, puff + 60);
            fill_circle(
                centre,
                zoom * (0.22 + 0.45 * k) * (0.7 + 0.6 * scatter(meeting.seed, puff + 70)),
                Color::new(
                    shade,
                    shade * 0.98,
                    shade * 0.95,
                    0.38 * (1.0 - k) * (1.0 - k),
                ),
            );
        }
    }
    if !in_sight {
        return;
    }
    // Fragments arc out and fall on smoking trails, gone before the ground.
    let fall_time = meet + 1.0;
    let gravity = 2.0 * drop.max(zoom) / (fall_time * fall_time);
    for index in 0..8 {
        let angle = scatter(meeting.seed, index) * std::f32::consts::TAU;
        let speed = zoom * (1.0 + 2.0 * scatter(meeting.seed, index + 11));
        let out = vec2(angle.cos(), angle.sin() * 0.6 - 0.5) * speed;
        let at = |t: f32| sky + out * t + vec2(0.0, 0.5 * gravity * t * t);
        let fade = (1.0 - after).clamp(0.0, 1.0);
        if fade <= 0.0 {
            continue;
        }
        let mut previous = at(after);
        for step in 1..=5 {
            let back = after - step as f32 * 0.05;
            if back <= 0.0 {
                break;
            }
            let point = at(back);
            let trail = fade * (1.0 - step as f32 / 6.0);
            line_between(
                previous,
                point,
                2.0,
                Color::new(0.5, 0.49, 0.47, 0.6 * trail),
            );
            previous = point;
        }
        let hot = (1.0 - after / 0.5).clamp(0.0, 1.0);
        fill_circle(
            at(after),
            2.4,
            Color::new(
                0.25 + 0.75 * hot,
                0.24 + 0.71 * hot,
                0.23 + 0.59 * hot,
                fade,
            ),
        );
    }
}

/// The interceptor climbing out of its pod at `from` on a curve to `to`,
/// `t` of the way there.
fn draw_interceptor(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    from: Vec2,
    to: Vec2,
    tint: Color,
    t: f32,
) {
    let zoom = game.presentation.camera.zoom;
    // Straight up out of the pod first, then over onto the meeting point.
    let control = from - vec2(0.0, (from.distance(to) * 0.7).max(2.0 * zoom));
    let t = (0.3 * t + 0.7 * t * t).clamp(0.0, 1.0);
    let point = |t: f32| from * (1.0 - t) * (1.0 - t) + control * 2.0 * (1.0 - t) * t + to * t * t;
    let at = point(t);
    let tangent = (control - from) * 2.0 * (1.0 - t) + (to - control) * 2.0 * t;
    // Exhaust along the curve it has climbed.
    let mut previous = at;
    for step in 1..=10 {
        let back = t - step as f32 * 0.04;
        if back <= 0.0 {
            break;
        }
        let point = point(back);
        let fade = 1.0 - step as f32 / 11.0;
        line_between(
            previous,
            point,
            (0.12 * zoom * fade).max(1.0),
            Color::new(0.9, 0.9, 0.86, 0.35 * fade),
        );
        previous = point;
    }
    let Some(rows) = sprites.strategic_rigs().map(|rigs| &rigs.lattice.launch) else {
        fill_circle(at, 0.15 * zoom, WHITE);
        return;
    };
    let rotation = tangent.y.atan2(tangent.x) + std::f32::consts::FRAC_PI_2;
    let flicker = numeric::to_usize((game.presentation.fx_time() * 30.0).floor());
    let row = rows[1 + flicker % (rows.len() - 1).max(1)];
    let size = Vec2::splat(2.0 * zoom * (1.0 + 0.4 * t));
    for (source, color) in [(row[0], WHITE), (row[1], tint)] {
        sprites.draw(
            at.x - size.x * 0.5,
            at.y - size.y * 0.5,
            color,
            DrawTextureParams {
                dest_size: Some(size),
                source: Some(source),
                rotation,
                pivot: Some(at),
                ..Default::default()
            },
        );
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

/// The blast a strike would bring: under the cursor while a Launch is
/// armed, and at each selected own Spire's ordered tile, with a line from
/// the silo.
pub(crate) fn draw_strike_preview(game: &crate::game::Scene<'_>, input: &InputState) {
    let camera = &game.presentation.camera;
    let zoom = camera.zoom;
    let blast = oxide_sim::stats::WARHEAD;
    let human = game.presentation.human;
    let own = seat_identity_color(game, human);
    let ring = |center: Vec2, alpha: f32| {
        let color = Color::new(own.r, own.g, own.b, alpha);
        stroke_circle(
            center,
            blast.outer_radius.to_num::<f32>() * zoom,
            2.0,
            color,
        );
        stroke_circle(
            center,
            blast.full_radius.to_num::<f32>() * zoom,
            1.5,
            Color::new(color.r, color.g, color.b, alpha * 0.8),
        );
    };
    let silo_line = |b: &Building, to: Vec2, alpha: f32| {
        line_between(
            camera.to_screen(crate::game::world_vec(b.center())),
            to,
            1.5,
            Color::new(own.r, own.g, own.b, alpha),
        );
    };
    if !input.striking.is_empty() && input.last_pointer == crate::input::Pointer::Mouse {
        let tile = crate::input::ground_tile(game.state, camera.to_world(input.mouse));
        let center = camera.to_screen(crate::input::tile_center(tile));
        ring(center, 0.75);
        for b in input
            .striking
            .iter()
            .filter_map(|id| game.state.building(*id))
        {
            silo_line(b, center, 0.4);
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
