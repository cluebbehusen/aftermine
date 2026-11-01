//! Role and allegiance cues at strategic camera scales.

use crate::game::Scene;
use crate::numeric;
use crate::render::prim::{fill_circle, stroke_circle};
use aftermine_sim::Unit;
use macroquad::prelude::*;

thread_local! {
    static PREFS: std::cell::Cell<crate::config::MarkerPrefs> =
        std::cell::Cell::new(crate::config::MarkerPrefs::default());
}

pub(crate) fn set_prefs(prefs: crate::config::MarkerPrefs) {
    PREFS.set(prefs.clamped());
}

pub(crate) fn prefs() -> crate::config::MarkerPrefs {
    PREFS.get()
}

pub(crate) fn marker_alpha(zoom: f32) -> f32 {
    let p = prefs();
    transition(zoom, p.start, p.end)
}

/// Draws one marker's art around its screen position. The art is authored as
/// offsets at 100% and takes the player's marker size here rather than from a
/// camera: `set_camera` flushes the frame's draw batch, and that mid-frame
/// re-upload stalls in the GL driver.
#[derive(Clone, Copy)]
struct Pen {
    center: Vec2,
    scale: f32,
}

impl Pen {
    fn at(self, offset: (f32, f32)) -> Vec2 {
        self.center + vec2(offset.0, offset.1) * self.scale
    }

    fn line(self, a: (f32, f32), b: (f32, f32), thickness: f32, color: Color) {
        let (a, b) = (self.at(a), self.at(b));
        draw_line(a.x, a.y, b.x, b.y, thickness * self.scale, color);
    }

    fn rect(self, corner: (f32, f32), size: (f32, f32), color: Color) {
        let corner = self.at(corner);
        draw_rectangle(
            corner.x,
            corner.y,
            size.0 * self.scale,
            size.1 * self.scale,
            color,
        );
    }

    fn rect_lines(self, corner: (f32, f32), size: (f32, f32), thickness: f32, color: Color) {
        let corner = self.at(corner);
        draw_rectangle_lines(
            corner.x,
            corner.y,
            size.0 * self.scale,
            size.1 * self.scale,
            thickness * self.scale,
            color,
        );
    }

    fn poly(self, sides: u8, radius: f32, rotation: f32, color: Color) {
        draw_poly(
            self.center.x,
            self.center.y,
            sides,
            radius * self.scale,
            rotation,
            color,
        );
    }

    fn poly_lines(self, sides: u8, radius: f32, rotation: f32, thickness: f32, color: Color) {
        draw_poly_lines(
            self.center.x,
            self.center.y,
            sides,
            radius * self.scale,
            rotation,
            thickness * self.scale,
            color,
        );
    }

    fn circle(self, offset: (f32, f32), radius: f32, color: Color) {
        fill_circle(self.at(offset), radius * self.scale, color);
    }

    fn circle_lines(self, offset: (f32, f32), radius: f32, thickness: f32, color: Color) {
        stroke_circle(
            self.at(offset),
            radius * self.scale,
            thickness * self.scale,
            color,
        );
    }

    fn triangle(self, a: (f32, f32), b: (f32, f32), c: (f32, f32), color: Color) {
        draw_triangle(self.at(a), self.at(b), self.at(c), color);
    }
}

fn transition(zoom: f32, start: f32, end: f32) -> f32 {
    ((start - zoom) / (start - end)).clamp(0.0, 1.0)
}

pub(crate) fn replaces_units(zoom: f32) -> bool {
    marker_alpha(zoom) >= 1.0
}

pub(crate) fn visible(game: &Scene<'_>, unit: &Unit) -> bool {
    unit.player == game.presentation.human
        || game.presentation.all_seeing()
        || game.my_vision().sees_unit(unit)
}

pub fn draw_markers(game: &Scene<'_>, alpha: f32) {
    let opacity = marker_alpha(game.presentation.camera.zoom);
    if opacity <= 0.0 {
        return;
    }
    let scale = prefs().scale;
    let (lo, hi) = game.presentation.camera.world_rect();
    // Draw all backplates before any glyph so adjacent markers cannot erase roles.
    for backplates in [true, false] {
        for unit in game
            .state
            .units()
            .iter()
            .filter(|u| !game.presentation.selection.units.contains(&u.id))
            .chain(
                game.state
                    .units()
                    .iter()
                    .filter(|u| game.presentation.selection.units.contains(&u.id)),
            )
        {
            if !visible(game, unit) {
                continue;
            }
            let pos = game.presentation.draw_pos(unit.id, unit.pos, alpha);
            if pos.x < lo.x - 2.0 || pos.y < lo.y - 2.0 || pos.x > hi.x + 2.0 || pos.y > hi.y + 2.0
            {
                continue;
            }
            let pen = Pen {
                center: game.presentation.camera.to_screen(pos),
                scale,
            };
            let tint = crate::render::seat_identity_color(game, unit.player);
            let color = Color { a: opacity, ..tint };
            let fill = Color::new(0.065, 0.065, 0.075, opacity);
            let ink = Color::new(0.91, 0.89, 0.83, opacity);
            let selected = game.presentation.selection.units.contains(&unit.id);
            let air = unit.domain() == aftermine_sim::stats::Domain::Air;
            if backplates {
                if air {
                    pen.poly(4, 10.0, 0.0, fill);
                    pen.poly_lines(4, 10.0, 0.0, 1.6, color);
                } else {
                    pen.rect((-7.0, -7.0), (14.0, 14.0), fill);
                    pen.rect_lines((-7.0, -7.0), (14.0, 14.0), 1.6, color);
                }
                continue;
            }
            let line = |a, b| pen.line(a, b, 1.4, ink);
            match crate::look::unit(unit.kind).marker {
                crate::look::MarkerRole::Worker => {
                    line((-3.0, -3.0), (-3.0, 3.0));
                    line((3.0, -3.0), (3.0, 3.0));
                    line((-3.0, 3.0), (3.0, 3.0));
                }
                crate::look::MarkerRole::Gun => {
                    pen.circle_lines((0.0, 1.0), 2.4, 1.3, ink);
                    line((0.0, 0.0), (0.0, -4.5));
                }
                crate::look::MarkerRole::Siege => {
                    line((-3.5, 2.0), (0.0, -2.0));
                    line((0.0, -2.0), (3.5, 2.0));
                    line((-3.5, 4.0), (3.5, 4.0));
                }
                crate::look::MarkerRole::AntiAir => {
                    line((-3.5, 1.5), (0.0, -2.0));
                    line((0.0, -2.0), (3.5, 1.5));
                    line((0.0, -2.0), (0.0, 4.0));
                }
                crate::look::MarkerRole::PointDefense => {
                    for y in [-3.0, 0.0, 3.0] {
                        line((-3.0, y), (3.0, y));
                    }
                }
                crate::look::MarkerRole::Scout => {
                    pen.circle_lines((0.0, 0.0), 3.0, 1.3, ink);
                    pen.circle((0.0, 0.0), 1.0, ink);
                }
                crate::look::MarkerRole::Transport => {
                    line((-3.0, -2.0), (-3.0, 2.0));
                    line((3.0, -2.0), (3.0, 2.0));
                    line((-3.0, 2.0), (3.0, 2.0));
                    line((-3.0, -2.0), (3.0, -2.0));
                }
                crate::look::MarkerRole::Demolition => {
                    line((-3.0, -3.0), (3.0, 3.0));
                    line((-3.0, 3.0), (3.0, -3.0));
                }
            }
            if game.state.hostile(game.presentation.human, unit.player) {
                pen.triangle((-3.0, 8.0), (3.0, 8.0), (0.0, 11.0), color);
            } else if unit.player != game.presentation.human {
                pen.line((-3.0, 9.0), (3.0, 9.0), 1.5, ink);
            }
            if selected {
                pen.circle_lines((0.0, 0.0), 12.0, 1.3, ink);
            }
            if unit.hp < unit.kind.stats().max_hp {
                pen.rect((-7.0, -12.0), (14.0, 2.0), fill);
                pen.rect(
                    (-7.0, -12.0),
                    (14.0 * unit.hp as f32 / unit.kind.stats().max_hp as f32, 2.0),
                    color,
                );
            }
        }
    }
}

pub(crate) fn building_visible(game: &Scene<'_>, b: &aftermine_sim::Building) -> bool {
    b.player == game.presentation.human
        || game.presentation.all_seeing()
        || (b.tiles().any(|t| game.my_vision().visible(t))
            && game.state.building_apparent(game.presentation.human, b))
}

pub(crate) fn draw_buildings(game: &Scene<'_>) {
    let alpha = marker_alpha(game.presentation.camera.zoom);
    if alpha <= 0.0 {
        return;
    }
    let scale = prefs().scale;
    let (lo, hi) = game.presentation.camera.world_rect();
    // A Spire's deck art is too small to read here, so its marker says
    // whether it holds a warhead, as far as the viewer may know.
    let loaded: Vec<_> = game
        .state
        .known_magazines(game.presentation.human)
        .into_iter()
        .filter(|known| known.heading.is_some() && known.loaded > 0)
        .map(|known| known.building)
        .collect();
    let mut markers = Vec::new();
    for b in game
        .state
        .buildings()
        .iter()
        .filter(|b| building_visible(game, b))
    {
        markers.push((
            b.kind,
            b.player,
            b.anchor,
            b.built(),
            false,
            game.presentation.selection.buildings.contains(&b.id),
            if game.presentation.all_seeing() {
                b.launcher.is_some() && b.magazine.is_some_and(|m| m.loaded > 0)
            } else {
                loaded.contains(&b.id)
            },
            1.0,
        ));
    }
    if !game.presentation.all_seeing() {
        for g in game.my_vision().ghosts() {
            let observed = game
                .state
                .buildings_at(g.anchor)
                .any(|b| b.kind == g.kind && b.player == g.owner && building_visible(game, b));
            if observed {
                continue;
            }
            let age = game
                .presentation
                .last_seen
                .borrow()
                .get(&(g.anchor.x, g.anchor.y))
                .map_or(0.0, |stamp| game.presentation.fx_time() - stamp);
            markers.push((
                g.kind,
                g.owner,
                g.anchor,
                g.built,
                true,
                false,
                false,
                0.55 * (1.0 - crate::render::staleness_fade(age)),
            ));
        }
    }
    for (kind, owner, anchor, built, memory, selected, armed, fade) in markers {
        let (w, h) = kind.size();
        let world = vec2(
            anchor.x as f32 + w as f32 * 0.5,
            anchor.y as f32 + h as f32 * 0.5,
        );
        if world.x < lo.x - 3.0
            || world.y < lo.y - 3.0
            || world.x > hi.x + 3.0
            || world.y > hi.y + 3.0
        {
            continue;
        }
        let pen = Pen {
            center: game.presentation.camera.to_screen(world),
            scale,
        };
        let opacity = alpha * fade;
        let identity = crate::render::seat_identity_color(game, owner);
        let edge = Color {
            a: opacity,
            ..identity
        };
        let ink = Color::new(0.91, 0.89, 0.83, opacity);
        let fill = Color::new(0.065, 0.065, 0.075, opacity * 0.94);
        pen.poly(8, 10.0, 22.5, fill);
        pen.poly_lines(8, 10.0, 22.5, 1.4, edge);
        building_glyph(kind, pen, ink);
        if !built {
            pen.line((-6.0, 6.0), (6.0, -6.0), 1.3, ink);
        }
        if memory {
            pen.circle((9.0, -9.0), 2.0, ink);
        }
        if armed {
            let warhead = Color {
                a: opacity,
                ..crate::theme::TEXT_ACCENT
            };
            pen.circle((-8.5, -8.5), 2.6, warhead);
        }
        if selected {
            pen.poly_lines(8, 13.0, 22.5, 1.4, ink);
        }
        if game.state.hostile(game.presentation.human, owner) {
            pen.triangle((-3.0, 11.0), (3.0, 11.0), (0.0, 14.0), edge);
        } else if owner != game.presentation.human {
            pen.line((-3.0, 12.0), (3.0, 12.0), 1.4, ink);
        }
    }
}

fn building_glyph(kind: aftermine_sim::BuildingKind, pen: Pen, ink: Color) {
    use aftermine_sim::BuildingKind::{
        Airworks, Array, Barricade, Bastion, Crucible, Extractor, Fabricator, FlakTurret, Foundry,
        Lattice, MawCradle, Reclaimer, RepairBay, RocCradle, ScuttleCharge, Spire, Turret,
    };
    let line = |a, b| pen.line(a, b, 1.35, ink);
    match kind {
        Foundry | Fabricator | Crucible => {
            line((-4.0, 4.0), (4.0, 4.0));
            line((-4.0, 4.0), (-4.0, -2.0));
            line((-4.0, -2.0), (-1.0, 0.0));
            line((-1.0, 0.0), (2.0, -2.0));
            line((2.0, -2.0), (4.0, -2.0));
            line((4.0, -2.0), (4.0, 4.0));
            line((-4.0, -2.0), (-4.0, -5.0));
            if kind != Foundry {
                line((0.0, -3.0), (0.0, -5.0));
            }
            if kind == Crucible {
                line((4.0, -3.0), (4.0, -5.0));
            }
        }
        Airworks => {
            line((-5.0, 1.0), (5.0, 1.0));
            line((0.0, -5.0), (0.0, 5.0));
            line((-3.0, 4.0), (3.0, 4.0));
        }
        Turret => {
            pen.circle_lines((0.0, 1.0), 2.8, 1.3, ink);
            line((0.0, 0.0), (0.0, -5.0));
        }
        FlakTurret => {
            line((-4.0, 1.0), (0.0, -3.0));
            line((0.0, -3.0), (4.0, 1.0));
            line((0.0, -3.0), (0.0, 5.0));
        }
        Bastion => {
            line((-4.0, 1.0), (0.0, -3.0));
            line((0.0, -3.0), (4.0, 1.0));
            line((-4.0, 4.0), (4.0, 4.0));
        }
        Array => {
            pen.circle_lines((0.0, 0.0), 4.0, 1.1, ink);
            line((0.0, 0.0), (3.0, -4.0));
            pen.circle((0.0, 0.0), 1.4, ink);
        }
        RepairBay => {
            line((-4.0, 0.0), (4.0, 0.0));
            line((0.0, -4.0), (0.0, 4.0));
        }
        Reclaimer => {
            line((-4.0, -4.0), (4.0, -4.0));
            line((4.0, -4.0), (4.0, 0.0));
            line((4.0, 0.0), (0.0, 4.0));
            line((0.0, 4.0), (-4.0, 0.0));
            line((-4.0, 0.0), (-4.0, -4.0));
        }
        Extractor => {
            line((-4.0, -3.0), (0.0, 2.0));
            line((0.0, 2.0), (4.0, -3.0));
            line((0.0, 2.0), (0.0, 5.0));
        }
        Barricade => {
            for x in [-4.0, 0.0, 4.0] {
                line((x, -4.0), (x, 4.0));
            }
        }
        ScuttleCharge => {
            line((-3.0, -3.0), (3.0, 3.0));
            line((-3.0, 3.0), (3.0, -3.0));
        }
        Spire => {
            line((0.0, -5.0), (-2.5, -1.0));
            line((0.0, -5.0), (2.5, -1.0));
            line((-2.5, -1.0), (-2.5, 4.0));
            line((2.5, -1.0), (2.5, 4.0));
            line((-4.0, 4.0), (4.0, 4.0));
        }
        Lattice => {
            pen.circle_lines((-2.5, 0.0), 2.2, 1.3, ink);
            pen.circle_lines((2.5, 0.0), 2.2, 1.3, ink);
            line((-5.0, 4.5), (5.0, 4.5));
        }
        RocCradle => {
            // A gantry frame over a winged hull taking shape.
            line((-5.0, 5.0), (-5.0, -4.0));
            line((5.0, 5.0), (5.0, -4.0));
            line((-5.0, -4.0), (5.0, -4.0));
            line((-3.5, 1.5), (3.5, 1.5));
            line((0.0, -1.0), (0.0, 4.0));
        }
        MawCradle => {
            // A gantry frame over a hull taking shape.
            line((-5.0, 5.0), (-5.0, -4.0));
            line((5.0, 5.0), (5.0, -4.0));
            line((-5.0, -4.0), (5.0, -4.0));
            line((-2.5, 3.0), (2.5, 3.0));
            line((-2.5, 3.0), (-2.5, 0.0));
            line((2.5, 3.0), (2.5, 0.0));
        }
    }
}

pub(crate) fn extractor_frame_visible(game: &Scene<'_>, frame: chassis::grid::TilePos) -> bool {
    let known = game.presentation.all_seeing()
        || (0..2).any(|dy| (0..2).any(|dx| game.my_vision().explored(frame.offset(dx, dy))));
    let claimed = if game.presentation.all_seeing() {
        game.state
            .buildings()
            .iter()
            .any(|building| building.hp > 0 && building.anchor == frame)
    } else {
        game.state
            .extractor_frame_claim_known(game.presentation.human, frame)
    };
    known && !claimed
}

pub(crate) fn draw_extractor_frames(game: &Scene<'_>) {
    let prefs = prefs();
    let alpha = marker_alpha(game.presentation.camera.zoom);
    if alpha <= 0.0 {
        return;
    }
    let scale = prefs.scale;
    let (lo, hi) = game.presentation.camera.world_rect();
    for &frame in game.state.map().extractor_frames() {
        let pos = vec2(frame.x as f32 + 1.0, frame.y as f32 + 1.0);
        if pos.x < lo.x - 2.0
            || pos.y < lo.y - 2.0
            || pos.x > hi.x + 2.0
            || pos.y > hi.y + 2.0
            || !extractor_frame_visible(game, frame)
        {
            continue;
        }
        let pen = Pen {
            center: game.presentation.camera.to_screen(pos),
            scale,
        };
        let ink = Color::new(0.83, 0.66, 0.36, alpha);
        pen.rect(
            (-8.0, -8.0),
            (16.0, 16.0),
            Color::new(0.065, 0.065, 0.075, alpha * 0.9),
        );
        for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            pen.line((x * 8.0, y * 8.0), (x * 4.0, y * 8.0), 1.4, ink);
            pen.line((x * 8.0, y * 8.0), (x * 8.0, y * 4.0), 1.4, ink);
        }
        building_glyph(aftermine_sim::BuildingKind::Extractor, pen, ink);
    }
}

pub(crate) fn known_resource(game: &Scene<'_>, pos: chassis::grid::TilePos) -> u32 {
    if game.presentation.all_seeing() || game.my_vision().visible(pos) {
        game.state
            .map()
            .tile(pos)
            .map_or(0, |t| t.scrap.saturating_add(t.wreck))
    } else {
        game.my_vision()
            .remembered_scrap(pos)
            .saturating_add(game.my_vision().remembered_wreck(pos))
    }
}
fn resource_marker_opacity(game: &Scene<'_>, pos: chassis::grid::TilePos) -> f32 {
    let age_opacity = crate::render::resource_memory_opacity(game, pos);
    age_opacity
        * if game.presentation.all_seeing() || game.my_vision().visible(pos) {
            1.0
        } else {
            0.4
        }
}
pub(crate) fn draw_resources(game: &Scene<'_>) {
    let p = prefs();
    let opacity = transition(
        game.presentation.camera.zoom,
        (p.start - 4.0).max(p.end - 1.0),
        p.end - 2.0,
    ) * 0.65;
    if opacity <= 0.0 {
        return;
    }
    let mut groups = std::collections::BTreeMap::<(i32, i32), (Vec2, u32, f32)>::new();
    let (lo, hi) = game.presentation.camera.world_rect();
    // World-anchored cells keep summaries stable while the camera pans.
    for y in (numeric::to_i32(lo.y.floor()).div_euclid(4) * 4).max(0)
        ..((numeric::to_i32(hi.y.ceil()).div_euclid(4) + 1) * 4).min(game.state.map().height())
    {
        for x in (numeric::to_i32(lo.x.floor()).div_euclid(4) * 4).max(0)
            ..((numeric::to_i32(hi.x.ceil()).div_euclid(4) + 1) * 4).min(game.state.map().width())
        {
            let pos = chassis::grid::TilePos::new(x, y);
            if known_resource(game, pos) == 0 {
                continue;
            }
            let group =
                groups
                    .entry((x.div_euclid(4), y.div_euclid(4)))
                    .or_insert((Vec2::ZERO, 0, 0.0));
            group.0 += vec2(x as f32 + 0.5, y as f32 + 0.5);
            group.1 += 1;
            group.2 += resource_marker_opacity(game, pos);
        }
    }
    for (_, (sum, count, evidence)) in groups {
        let p = game.presentation.camera.to_screen(sum / count as f32);
        // Average tile evidence so an old memory cannot dim every live node in a cell.
        let alpha = opacity * evidence / count as f32;
        let scale = prefs().scale;
        let size = if count > 1 { 4.0 } else { 2.8 } * scale;
        draw_poly(
            p.x,
            p.y,
            4,
            size + 1.5,
            0.0,
            Color::new(0.04, 0.04, 0.05, alpha),
        );
        draw_poly_lines(
            p.x,
            p.y,
            4,
            size,
            0.0,
            1.3 * scale,
            Color::new(0.70, 0.53, 0.26, alpha),
        );
    }
}
#[cfg(test)]
mod tests;
