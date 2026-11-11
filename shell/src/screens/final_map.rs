//! Frozen inspection of the already-final live battlefield.

use crate::action::{Action, ActionResolver, BindingMap, Context};
use crate::camera::controls::{
    MinimapPoint, RemoteCamera, ViewerHands, ViewerPads, event_point, held_pan, pan_toward,
};
use crate::game::Game;
use crate::hint_line::HintLine;
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
    /// Controller sticks and the Siri Remote's camera modes.
    remote: RemoteCamera,
}

impl FinalMapScreen {
    /// Opens a frozen map inspector.
    pub fn open() -> Self {
        Self::default()
    }

    /// Moves the camera by a frame's sticks and takes the Siri Remote's camera
    /// input. Returns the events left for [`Self::update`].
    pub(crate) fn route_pads(
        &mut self,
        events: &[RawEvent],
        pads: &ViewerPads,
        dt: f32,
        game: &mut Game,
    ) -> Vec<RawEvent> {
        self.remote
            .route(events, &mut game.presentation.camera, pads, dt)
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
                    if self.resolver.press(bindings, *key, Context::FinalMap) == Some(Action::Back)
                    {
                        return true;
                    }
                }
                RawEvent::KeyUp { key } => self.resolver.release(*key),
                // The controller's back button, by position like the menus'.
                RawEvent::PadDown {
                    button: crate::pad::BACK,
                    ..
                } => return true,
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
    fn coaching(&self, bindings: &BindingMap, hands: crate::platform::Hands) -> HintLine {
        use aftermine_protocol::{PadButton, PadFamily};
        if hands.pad == Some(PadFamily::Remote) {
            return HintLine::from(format!("{} | Back to the report", self.remote.hint()));
        }
        if let Some(family) = hands.pad {
            let icon = |button| crate::pad::button_icon(family, button);
            return HintLine::default()
                .pad(icon(PadButton::LeftStick))
                .text(" pan | ")
                .pad(icon(PadButton::RightStick))
                .text(" zoom | ")
                .pad(icon(crate::pad::BACK))
                .text(" back to the report");
        }
        HintLine::from(match (hands.touch(), hands.keys) {
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
        })
    }

    /// Draws the Back button and the compact camera-help strip over the
    /// battlefield.
    pub fn draw_hud(&self, sprites: &crate::assets::Sprites, bindings: &BindingMap, mouse: Vec2) {
        let scale = render::ui_scale();
        crate::button::draw_back(mouse);
        let size = crate::theme::Type::Body.at(scale);
        let hands = crate::platform::hands();
        // Which mode the remote's swipes are in is state, not coaching.
        let remote = hands.pad == Some(aftermine_protocol::PadFamily::Remote);
        if !remote && !crate::hints::showing() {
            return;
        }
        let line = self.coaching(bindings, hands);
        let width = line.width(size);
        let x = (screen_width() - width) * 0.5;
        let y = screen_height() - 14.0 * scale;
        draw_rectangle(
            x - 10.0 * scale,
            y - size,
            width + 20.0 * scale,
            size + 10.0 * scale,
            theme::SURFACE_CAPTION,
        );
        line.draw_at(sprites, x, y, size, theme::TEXT_PRIMARY, WHITE);
        if remote {
            crate::tabs::draw_remote_modes(
                sprites,
                self.remote,
                screen_width() * 0.5,
                y - size - 8.0 * scale,
                mouse,
            );
        }
    }
}

#[cfg(test)]
mod tests;
