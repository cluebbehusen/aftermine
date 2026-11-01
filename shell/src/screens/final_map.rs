//! Frozen inspection of the already-final live battlefield.

use crate::action::{Action, ActionEvent, ActionResolver, BindingMap, Context};
use crate::camera::controls::{MinimapPoint, ViewerHands, event_point, held_pan, pan_toward};
use crate::game::Game;
use crate::numeric;
use crate::{render, theme};
#[cfg(test)]
use aftermine_protocol::Key;
use aftermine_protocol::RawEvent;
use macroquad::prelude::*;

/// Camera-only state for the final battlefield view.
#[derive(Default)]
pub struct FinalMapScreen {
    resolver: ActionResolver,
    /// The corner Back button.
    back: crate::button::BackButton,
    /// Middle-drag, wheel, minimap and finger camera control.
    hands: ViewerHands,
}

impl FinalMapScreen {
    /// Opens a frozen map inspector.
    pub fn open() -> Self {
        Self::default()
    }

    /// Applies camera input. Returns true when the report should reopen.
    pub fn update(
        &mut self,
        bindings: &BindingMap,
        events: &[RawEvent],
        dt: f32,
        camera_prefs: crate::config::CameraPrefs,
        mouse: &mut Vec2,
        game: &mut Game,
    ) -> bool {
        let ui = render::ui_scale();
        let (back, events) = self.back.route(events);
        if back {
            return true;
        }
        for event in &events {
            match event {
                RawEvent::KeyDown { key } => {
                    if self
                        .resolver
                        .key_edge_in(bindings, *key, true, Context::FinalMap)
                        == Some(ActionEvent::Pressed(Action::Back))
                    {
                        return true;
                    }
                }
                RawEvent::KeyUp { key } => {
                    self.resolver
                        .key_edge_in(bindings, *key, false, Context::FinalMap);
                }
                _ => {
                    let minimap =
                        event_point(event).map_or_else(MinimapPoint::default, |p| MinimapPoint {
                            under: render::minimap_world_at(&game.view(), p),
                            clamped: render::minimap_world_clamped(&game.view(), p),
                        });
                    self.hands.pointer(
                        event,
                        minimap,
                        &mut game.presentation.camera,
                        mouse,
                        camera_prefs,
                        ui,
                    );
                }
            }
        }
        pan_toward(
            &mut game.presentation.camera,
            held_pan(&self.resolver),
            camera_prefs,
            dt,
        );
        false
    }

    /// The camera line, for the hands the player is using.
    fn coaching(bindings: &BindingMap, hands: crate::platform::Hands) -> String {
        match (hands.touch(), hands.keys) {
            (false, true) => format!(
                "Pan with the keys, minimap or middle drag | wheel to zoom | {} back",
                bindings.label(Action::Back)
            ),
            (false, false) => "Click the minimap to pan | scroll to zoom".to_string(),
            (true, true) => format!(
                "Drag to pan | pinch to zoom | {} back",
                bindings.label(Action::Back)
            ),
            (true, false) => "Drag to pan | pinch to zoom".to_string(),
        }
    }

    /// Draws the Back button and the compact camera-help strip over the
    /// battlefield.
    pub fn draw_hud(bindings: &BindingMap, mouse: Vec2) {
        let scale = render::ui_scale();
        crate::button::draw_back(mouse);
        let size = crate::theme::Type::Body.at(scale);
        if !crate::hints::showing() {
            return;
        }
        let line = Self::coaching(bindings, crate::platform::hands());
        let width = measure_text(&line, None, numeric::font_size(size), 1.0).width;
        let x = (screen_width() - width) * 0.5;
        let y = screen_height() - 14.0 * scale;
        draw_rectangle(
            x - 10.0 * scale,
            y - size,
            width + 20.0 * scale,
            size + 10.0 * scale,
            theme::SURFACE_CAPTION,
        );
        draw_text(&line, x, y, size, theme::TEXT_PRIMARY);
    }
}

#[cfg(test)]
mod tests;
