//! A controller's left stick as the menu arrows.
//!
//! Menus read canonical arrow keys, so a stick pushed toward an edge
//! presses that arrow once, and held there presses it again at a steady
//! pace, as a held key would. Only menus see this: gameplay screens get
//! the stick's raw position, and the conversion is reset while one is up
//! so a stick still held there never presses into the next menu.

use aftermine_protocol::{Key, RawEvent, Stick};

/// Deflection along the dominant axis that presses an arrow.
const ENGAGE: f32 = 0.5;
/// Deflection along the held arrow under which it lets go. Lower than
/// [`ENGAGE`], so a stick resting near the threshold never chatters.
const RELEASE: f32 = 0.3;
/// Seconds a held arrow waits before it first presses again.
const FIRST_REPEAT: f32 = 0.4;
/// Seconds between later presses of a held arrow.
const REPEAT: f32 = 0.12;

/// The left stick's state between menu frames.
#[derive(Debug, Default)]
pub(crate) struct MenuStick {
    /// The arrow the stick holds and the seconds until it presses again.
    held: Option<(Key, f32)>,
}

impl MenuStick {
    /// Replaces the frame's left-stick events with the arrow presses they
    /// mean, plus any repeat `dt` seconds of holding have earned. Other
    /// events pass through in order.
    pub(crate) fn arrows(&mut self, events: &[RawEvent], dt: f32) -> Vec<RawEvent> {
        let mut out = Vec::with_capacity(events.len());
        if let Some((key, wait)) = &mut self.held {
            *wait -= dt.max(0.0);
            if *wait <= 0.0 {
                // One press a frame however long the frame took, so a
                // hitch never fires a burst.
                *wait = REPEAT;
                press(*key, &mut out);
            }
        }
        for event in events {
            match *event {
                RawEvent::PadStick {
                    stick: Stick::Left,
                    x,
                    y,
                    ..
                } => self.moved(x, y, &mut out),
                other => out.push(other),
            }
        }
        out
    }

    /// Forgets a held arrow, while a screen that is not a menu is up.
    pub(crate) fn reset(&mut self) {
        self.held = None;
    }

    fn moved(&mut self, x: f32, y: f32, out: &mut Vec<RawEvent>) {
        let pointing = pointing(x, y);
        let held = self.held.map(|(key, _)| key);
        let keeps = held.is_some_and(|key| along(key, x, y) >= RELEASE);
        match (pointing, held) {
            (Some(key), Some(current)) if key == current => {}
            (Some(key), _) => {
                self.held = Some((key, FIRST_REPEAT));
                press(key, out);
            }
            (None, Some(_)) if keeps => {}
            (None, _) => self.held = None,
        }
    }
}

/// The arrow a stick position presses: the dominant axis, once it is past
/// [`ENGAGE`]. A perfect diagonal reads vertical, since most menus are
/// lists.
fn pointing(x: f32, y: f32) -> Option<Key> {
    if x.abs().max(y.abs()) < ENGAGE {
        return None;
    }
    Some(if y.abs() >= x.abs() {
        if y < 0.0 { Key::Up } else { Key::Down }
    } else if x < 0.0 {
        Key::Left
    } else {
        Key::Right
    })
}

/// How far a stick position reaches toward `key`'s edge.
fn along(key: Key, x: f32, y: f32) -> f32 {
    match key {
        Key::Up => -y,
        Key::Down => y,
        Key::Left => -x,
        Key::Right => x,
        _ => 0.0,
    }
}

fn press(key: Key, out: &mut Vec<RawEvent>) {
    out.push(RawEvent::KeyDown { key });
    out.push(RawEvent::KeyUp { key });
}

#[cfg(test)]
mod tests;
