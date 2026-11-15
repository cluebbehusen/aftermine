//! Controller cursor: open arcs, eased target brackets and a feathered paint edge.

use crate::assets::Sprites;
use crate::game::Scene;
use crate::input::{InputState, PaintCursor, Pointer};
use crate::numeric;
use anyhow::{Context, Result};
use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation};
use macroquad::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Frame {
    center: Vec2,
    half: Vec2,
    round: f32,
    focus: f32,
    paint: f32,
}

/// Transient cursor interpolation, discarded with the match's presentation.
#[derive(Default)]
pub(crate) struct Motion {
    previous: Option<(f64, Frame)>,
}

impl Motion {
    fn clear(&mut self) {
        self.previous = None;
    }

    fn sample(&mut self, target: Frame, now: f64, immediate: bool) -> Frame {
        let frame = match self.previous {
            Some((at, from)) if !immediate && (0.0..=0.25).contains(&(now - at)) => {
                let dt = numeric::to_f32(now - at).min(0.1);
                let ease = 1.0 - (-dt * 22.0).exp();
                // A growing brush must draw the center and radius the input catches with.
                let shape = if target.paint > 0.0 { 1.0 } else { ease };
                Frame {
                    center: from.center.lerp(target.center, shape),
                    half: from.half.lerp(target.half, shape),
                    round: from.round + (target.round - from.round) * shape,
                    focus: from.focus + (target.focus - from.focus) * ease,
                    paint: from.paint + (target.paint - from.paint) * ease,
                }
            }
            _ => target,
        };
        self.previous = Some((now, frame));
        frame
    }
}

fn target(game: &Scene<'_>, input: &InputState, scale: f32) -> Frame {
    let cursor = crate::input::paint_cursor(input);
    let (radius, painting) = match cursor {
        PaintCursor::Idle => (11.0 * scale, false),
        PaintCursor::Held(radius) => (radius, false),
        PaintCursor::Sweeping(radius) => (radius, true),
    };
    let mut frame = Frame {
        center: input.reticle.point(),
        half: Vec2::splat(radius),
        round: radius,
        focus: 0.0,
        paint: if painting { 1.0 } else { 0.0 },
    };
    if cursor != PaintCursor::Idle {
        return frame;
    }
    let Some(pick) = input.reticle.snap().filter(|pick| pick.seen(game)) else {
        return frame;
    };
    let camera = &game.presentation.camera;
    match pick {
        crate::input::reticle::Picked::Unit(id, _) => {
            let Some(unit) = game.state.unit(id) else {
                return frame;
            };
            frame.center = camera.to_screen(game.presentation.draw_pos(
                id,
                unit.pos,
                game.clock.render_alpha(),
            ));
            frame.half =
                Vec2::splat(crate::input::unit_pick_radius(unit.kind) * camera.zoom + 6.0 * scale);
        }
        crate::input::reticle::Picked::Building(id, _) => {
            let Some(building) = game.state.building(id) else {
                return frame;
            };
            let (w, h) = building.kind.size();
            let min = vec2(building.anchor.x as f32, building.anchor.y as f32);
            let a = camera.to_screen(min);
            let b = camera.to_screen(min + vec2(w as f32, h as f32));
            frame.center = (a + b) * 0.5;
            frame.half = (b - a) * 0.5 + Vec2::splat(5.0 * scale);
        }
    }
    frame.round = 4.0 * scale;
    frame.focus = 1.0;
    frame
}

/// Draws only over live play, above the HUD like the mouse cursor.
pub(crate) fn draw(game: &Scene<'_>, sprites: &Sprites, input: &InputState) {
    let mut motion = game.presentation.reticle_motion.borrow_mut();
    if input.last_pointer != Pointer::Pad || input.ring.is_some() {
        motion.clear();
        return;
    }
    let point = input.reticle.point();
    let scale = super::ui_scale();
    if input.placing().is_some() {
        motion.clear();
    } else {
        let frame = motion.sample(
            target(game, input, scale),
            input.now,
            super::reduced_motion(),
        );
        sprites.reticle_ink().draw(frame, scale);
    }
    draw_circle(
        point.x,
        point.y,
        2.8 * scale,
        Color::new(0.03, 0.03, 0.04, 0.7),
    );
    draw_circle(
        point.x,
        point.y,
        1.25 * scale,
        Color::new(0.91, 0.895, 0.855, 0.95),
    );
}

/// One analytic cursor shader, loaded with the shell's GPU assets.
pub(crate) struct Ink(Material);

impl Ink {
    pub(crate) fn load() -> Result<Self> {
        load_material(
            ShaderSource::Glsl {
                vertex: VERTEX,
                fragment: FRAGMENT,
            },
            MaterialParams {
                uniforms: vec![
                    UniformDesc::new("Center", UniformType::Float2),
                    UniformDesc::new("Shape", UniformType::Float4),
                    UniformDesc::new("Look", UniformType::Float2),
                ],
                pipeline_params: PipelineParams {
                    color_blend: Some(BlendState::new(
                        Equation::Add,
                        BlendFactor::Value(BlendValue::SourceAlpha),
                        BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
                    )),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .map(Self)
        .context("loading the controller cursor shader")
    }

    fn draw(&self, frame: Frame, scale: f32) {
        self.0.set_uniform("Center", frame.center);
        self.0.set_uniform(
            "Shape",
            (
                frame.half.x,
                frame.half.y,
                frame.round.min(frame.half.min_element()),
                (frame.half.min_element() * 0.5).clamp(6.0 * scale, 14.0 * scale),
            ),
        );
        self.0.set_uniform("Look", (frame.paint, frame.focus));
        gl_use_material(&self.0);
        let extent = frame.half + Vec2::splat(9.0);
        draw_rectangle(
            frame.center.x - extent.x,
            frame.center.y - extent.y,
            extent.x * 2.0,
            extent.y * 2.0,
            WHITE,
        );
        gl_use_default_material();
    }
}

const VERTEX: &str = r"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
uniform mat4 Model;
uniform mat4 Projection;
varying vec2 at;
void main() { at=position.xy; gl_Position=Projection*Model*vec4(position,1.0); }
";

const FRAGMENT: &str = r"#version 100
precision highp float;
varying vec2 at;
uniform vec2 Center;
uniform vec4 Shape;
uniform vec2 Look;
void main() {
    vec2 p=at-Center;
    vec2 q=abs(p)-Shape.xy+Shape.z;
    float d=length(max(q,0.0))+min(max(q.x,q.y),0.0)-Shape.z;
    float rim=1.0-smoothstep(0.45,1.25,abs(d));
    float shade=(1.0-smoothstep(1.0,3.0,abs(d)))*0.48;
    float corner=smoothstep(Shape.x-Shape.w-0.6,Shape.x-Shape.w+0.6,abs(p.x))
                *smoothstep(Shape.y-Shape.w-0.6,Shape.y-Shape.w+0.6,abs(p.y));
    float angle=abs(mod(atan(p.y,p.x)+0.785398,1.570796)-0.785398);
    float arcs=1.0-smoothstep(0.48,0.51,angle);
    float gate=mix(arcs,corner,Look.y);
    float inside=1.0-smoothstep(-0.8,0.8,d);
    float featherGate=mix(1.0-smoothstep(0.46,0.53,angle),corner,Look.y);
    float soft=(1.0-smoothstep(0.0,4.5,abs(d+1.8)))*0.12*Look.x*featherGate;
    float fill=0.03*Look.x*inside;
    rim*=mix(1.0,0.82,Look.x);
    rim*=gate*0.86;
    shade*=gate;
    float bone=clamp(rim+fill+soft,0.0,1.0);
    float alpha=bone+shade*(1.0-bone);
    vec3 color=(vec3(0.91,0.895,0.855)*bone+vec3(0.04)*shade*(1.0-bone))/max(alpha,0.001);
    gl_FragColor=vec4(color,alpha);
}
";

#[cfg(test)]
mod tests;
