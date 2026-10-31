//! Text entry for the menus: a title, a row of one or more fields, and
//! Cancel plus a confirm button.

use crate::game::SoundKind;
use crate::menu::Menu;
use crate::numeric;
use crate::press::{Fed, Press};
use crate::theme::{
    BORDER_STRONG, MIN_TOUCH_TARGET, SURFACE_CARD, SURFACE_PANEL, Stroke, TEXT_ACCENT, TEXT_DANGER,
    TEXT_DISABLED, TEXT_PRIMARY, TEXT_SECONDARY, TEXT_TITLE, Type,
};
use aftermine_protocol::{Key, RawEvent};
use macroquad::prelude::{
    Color, Rect, Vec2, draw_rectangle, draw_rectangle_lines, draw_text, measure_text,
};

/// The on-screen keyboard a field asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyboard {
    /// The standard text keyboard.
    Text,
    /// Numbers and punctuation first, for host addresses.
    Address,
}

impl Keyboard {
    /// The platform layout behind this keyboard.
    pub fn native(self) -> macroquad::miniquad::KeyboardType {
        match self {
            Keyboard::Text => macroquad::miniquad::KeyboardType::Default,
            Keyboard::Address => macroquad::miniquad::KeyboardType::NumbersAndPunctuation,
        }
    }
}

/// One field: its value and what it takes.
pub struct Field {
    label: &'static str,
    placeholder: &'static str,
    value: String,
    max: usize,
    accepts: fn(char) -> bool,
    validate: fn(&str) -> Result<(), &'static str>,
    keyboard: Keyboard,
    /// The field's share of the row's width.
    weight: f32,
}

/// The default check: anything but blank.
fn not_blank(value: &str) -> Result<(), &'static str> {
    if value.is_empty() {
        Err("type something first")
    } else {
        Ok(())
    }
}

impl Field {
    /// An unlabeled field holding at most `max` characters that `accepts`
    /// takes, prefilled with the ones `value` has.
    pub fn new(value: &str, max: usize, accepts: fn(char) -> bool) -> Self {
        Self {
            label: "",
            placeholder: "",
            value: value.chars().filter(|&ch| accepts(ch)).take(max).collect(),
            max,
            accepts,
            validate: not_blank,
            keyboard: Keyboard::Text,
            weight: 1.0,
        }
    }

    /// Names the field and says what it expects while empty.
    pub fn labeled(mut self, label: &'static str, placeholder: &'static str) -> Self {
        self.label = label;
        self.placeholder = placeholder;
        self
    }

    /// Replaces the not-blank check; `validate` sees the trimmed value.
    pub fn validated(mut self, validate: fn(&str) -> Result<(), &'static str>) -> Self {
        self.validate = validate;
        self
    }

    /// Asks for `keyboard` while the field has focus.
    pub fn keyboard(mut self, keyboard: Keyboard) -> Self {
        self.keyboard = keyboard;
        self
    }

    /// Takes `weight` shares of the row's width.
    pub fn weight(mut self, weight: f32) -> Self {
        self.weight = weight;
        self
    }
}

/// The form's pointer targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Zone {
    Field(usize),
    Confirm,
    Cancel,
}

/// Where the form's face sits. It hugs the top of the window: an
/// on-screen keyboard covers roughly the bottom half in landscape, and
/// the platform does not resize the canvas around it.
#[derive(Debug, Clone)]
pub struct Layout {
    pub title_y: f32,
    pub hint_y: f32,
    /// The labels' baseline, when the fields have labels.
    pub label_y: f32,
    pub fields: Vec<Rect>,
    pub cancel: Rect,
    pub confirm: Rect,
}

impl Layout {
    fn zone_at(&self, p: Vec2) -> Option<Zone> {
        if let Some(index) = self.fields.iter().position(|rect| rect.contains(p)) {
            return Some(Zone::Field(index));
        }
        [(self.confirm, Zone::Confirm), (self.cancel, Zone::Cancel)]
            .into_iter()
            .find(|(rect, _)| rect.contains(p))
            .map(|(_, zone)| zone)
    }
}

/// Lays out fields sharing one row by `weights`, with room above them for
/// labels when `labeled`, and the buttons beneath.
pub fn layout(view: Vec2, s: f32, weights: &[f32], labeled: bool) -> Layout {
    let gap = 12.0 * s;
    let height = MIN_TOUCH_TARGET * s;
    let row_w = if weights.len() > 1 { 640.0 } else { 420.0 } * s;
    let width = row_w.min(view.x - 32.0 * s);
    let label_h = if labeled { 24.0 * s } else { 0.0 };
    let top = 92.0 * s;
    let field_y = top + label_h;
    let room = width - gap * weights.len().saturating_sub(1) as f32;
    let total: f32 = weights.iter().sum();
    let mut x = (view.x - width) * 0.5;
    let fields = weights
        .iter()
        .map(|weight| {
            let w = room * weight / total;
            let rect = Rect::new(x, field_y, w, height);
            x += w + gap;
            rect
        })
        .collect();
    let buttons_w = (420.0 * s).min(width);
    let button_w = (buttons_w - gap) * 0.5;
    let buttons_x = (view.x - buttons_w) * 0.5;
    let buttons_y = field_y + height + gap;
    Layout {
        title_y: 56.0 * s,
        hint_y: 80.0 * s,
        label_y: top + label_h * 0.7,
        fields,
        cancel: Rect::new(buttons_x, buttons_y, button_w, height),
        confirm: Rect::new(buttons_x + button_w + gap, buttons_y, button_w, height),
    }
}

/// What a frame did with the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// Still editing.
    Stay,
    /// Enter or the confirm button with every field valid; read them with
    /// [`Form::value`].
    Commit,
    /// Escape or Cancel.
    Cancel,
}

/// The form: its fields, which one has focus, and the presses on it.
pub struct Form {
    title: &'static str,
    confirm: &'static str,
    fields: Vec<Field>,
    focus: usize,
    /// Why the last commit was refused, shown until the next edit.
    error: Option<&'static str>,
    press: Press<Zone>,
    /// A tap on a field asked for the on-screen keyboard again (the
    /// player may have dismissed it); the frame loop takes this.
    keyboard_request: bool,
}

impl Form {
    /// A form titled `title` whose confirm button reads `confirm`, with
    /// focus on field `focus`.
    pub fn new(
        title: &'static str,
        confirm: &'static str,
        fields: Vec<Field>,
        focus: usize,
    ) -> Self {
        assert!(!fields.is_empty(), "a form needs a field");
        Self {
            title,
            confirm,
            focus: focus.min(fields.len() - 1),
            fields,
            error: None,
            press: Press::default(),
            keyboard_request: false,
        }
    }

    /// Refuses a commit the caller checked itself: plays Denied, shows
    /// `reason`, and focuses `field`.
    pub fn refuse(
        &mut self,
        field: usize,
        reason: &'static str,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
    ) {
        self.focus = field.min(self.fields.len() - 1);
        self.error = Some(reason);
        sounds.push((SoundKind::Denied, None));
    }

    /// Field `index`'s value, trimmed.
    pub fn value(&self, index: usize) -> &str {
        self.fields[index].value.trim()
    }

    /// The keyboard the focused field asks for.
    pub fn keyboard(&self) -> Keyboard {
        self.fields[self.focus].keyboard
    }

    /// Whether the focused field is the last.
    pub fn on_last_field(&self) -> bool {
        self.focus + 1 == self.fields.len()
    }

    /// Consumes a request to raise the on-screen keyboard again.
    pub fn take_keyboard_request(&mut self) -> bool {
        std::mem::take(&mut self.keyboard_request)
    }

    /// The form's layout in `view` at UI scale `s`.
    pub fn layout(&self, view: Vec2, s: f32) -> Layout {
        let weights: Vec<f32> = self.fields.iter().map(|field| field.weight).collect();
        let labeled = self.fields.iter().any(|field| !field.label.is_empty());
        layout(view, s, &weights, labeled)
    }

    /// The fields as menu rows, for the UI report, the focused one
    /// selected. The caret is a static underscore, never a blink, so
    /// reduced motion holds and the shots suite stays deterministic.
    pub fn menu<R: Clone>(&self, row: impl Fn(usize) -> R) -> Menu<R> {
        let rows = self.fields.iter().enumerate().map(|(index, field)| {
            let caret = if index == self.focus { "_" } else { "" };
            let text = if field.label.is_empty() {
                format!("{}{caret}", field.value)
            } else {
                format!("{}: {}{caret}", field.label, field.value)
            };
            (crate::menu::Label::Text(text), row(index))
        });
        let mut menu = Menu::rows(self.title, rows);
        menu.select(self.focus);
        menu
    }

    /// Applies a frame's events. The form owns the frame: typed characters
    /// edit the focused field, Backspace deletes, Tab and Down move focus
    /// on and Up back, a tap focuses a field, Enter advances to the next
    /// field or commits from the last, the confirm button commits, and
    /// Escape or Cancel abandons. A commit with an invalid field plays
    /// Denied, shows why, and focuses that field instead.
    pub fn update(
        &mut self,
        events: &[RawEvent],
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
    ) -> Edit {
        let layout = self.layout(crate::render::viewport(), crate::render::ui_scale());
        let mut pressed = None;
        for event in events {
            if let Fed::Activated(zone) = self.press.feed(event, |p, _| layout.zone_at(p)) {
                pressed = Some(zone);
            }
        }
        if let Some(Zone::Field(index)) = pressed {
            self.focus = index;
            self.keyboard_request = true;
        }
        let last = self.fields.len() - 1;
        let mut enter = false;
        for event in events {
            match *event {
                RawEvent::Text { ch } => {
                    let field = &mut self.fields[self.focus];
                    if (field.accepts)(ch) && field.value.chars().count() < field.max {
                        field.value.push(ch);
                        self.error = None;
                    }
                }
                RawEvent::KeyDown {
                    key: Key::Backspace,
                } => {
                    self.fields[self.focus].value.pop();
                    self.error = None;
                }
                RawEvent::KeyDown {
                    key: Key::Tab | Key::Down,
                } => self.focus = (self.focus + 1) % self.fields.len(),
                RawEvent::KeyDown { key: Key::Up } => {
                    self.focus = self.focus.checked_sub(1).unwrap_or(last);
                }
                RawEvent::KeyDown { key: Key::Enter } => enter = true,
                _ => {}
            }
        }
        if enter && self.focus < last {
            self.focus += 1;
        } else if enter || pressed == Some(Zone::Confirm) {
            match self.first_invalid() {
                Some((index, reason)) => {
                    self.focus = index;
                    self.error = Some(reason);
                    sounds.push((SoundKind::Denied, None));
                }
                None => return Edit::Commit,
            }
        }
        let escaped = events
            .iter()
            .any(|e| matches!(e, RawEvent::KeyDown { key: Key::Escape }));
        if pressed == Some(Zone::Cancel) || escaped {
            self.press.cancel();
            return Edit::Cancel;
        }
        Edit::Stay
    }

    /// The first field that fails its check, and why.
    fn first_invalid(&self) -> Option<(usize, &'static str)> {
        self.fields.iter().enumerate().find_map(|(index, field)| {
            (field.validate)(field.value.trim())
                .err()
                .map(|reason| (index, reason))
        })
    }

    /// Draws the title, `hint` under it in `hint_color` (or why the last
    /// commit was refused, in danger red), the fields, and the buttons.
    /// Coaching passes a faded color; a notice such as a failure passes a
    /// plain one, since warnings never wait.
    pub fn draw(&self, hint: &str, hint_color: Color, mouse: Vec2) {
        let s = crate::render::ui_scale();
        let view = crate::render::viewport();
        let layout = self.layout(view, s);
        let centered = |text: &str, y: f32, size: f32, color: Color| {
            let width = measure_text(text, None, numeric::font_size(size), 1.0).width;
            draw_text(text, (view.x - width) * 0.5, y, size, color);
        };
        centered(self.title, layout.title_y, Type::Title.at(s), TEXT_TITLE);
        match self.error {
            Some(reason) => centered(reason, layout.hint_y, Type::Label.at(s), TEXT_DANGER),
            None => centered(hint, layout.hint_y, Type::Label.at(s), hint_color),
        }
        for (index, (field, rect)) in self.fields.iter().zip(&layout.fields).enumerate() {
            draw_field(field, *rect, index == self.focus, layout.label_y, s);
        }
        crate::button::draw(layout.cancel, "CANCEL", layout.cancel.contains(mouse), s);
        crate::button::draw(
            layout.confirm,
            self.confirm,
            layout.confirm.contains(mouse),
            s,
        );
    }
}

/// Draws one field's label, box, and value (or its placeholder while
/// empty), the caret on the focused field.
fn draw_field(field: &Field, rect: Rect, focused: bool, label_y: f32, s: f32) {
    if !field.label.is_empty() {
        draw_text(
            field.label.to_uppercase(),
            rect.x,
            label_y,
            Type::Small.at(s),
            if focused { TEXT_ACCENT } else { TEXT_SECONDARY },
        );
    }
    let (fill, stroke, edge) = if focused {
        (SURFACE_CARD, Stroke::Focus, TEXT_ACCENT)
    } else {
        (SURFACE_PANEL, Stroke::Hairline, BORDER_STRONG)
    };
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, fill);
    draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, stroke.at(s), edge);
    // The focus outline marks where typing goes, so an empty field shows
    // its placeholder alone rather than a caret glued to it.
    let (text, color) = if field.value.is_empty() && !field.placeholder.is_empty() {
        (field.placeholder.to_owned(), TEXT_DISABLED)
    } else if focused {
        (format!("{}_", field.value), TEXT_PRIMARY)
    } else {
        (field.value.clone(), TEXT_PRIMARY)
    };
    let room = rect.w - 24.0 * s;
    let mut size = Type::Heading.at(s);
    let width = measure_text(&text, None, numeric::font_size(size), 1.0).width;
    if width > room {
        size = (size * room / width).max(10.0);
    }
    draw_text(
        &text,
        rect.x + 12.0 * s,
        rect.y + rect.h * 0.66,
        size,
        color,
    );
}

#[cfg(test)]
mod tests;
