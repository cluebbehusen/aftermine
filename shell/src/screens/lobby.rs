//! A LAN match before it starts: the Join form (your name and the host's
//! address), then the lobby's status and Cancel while machines gather.
//! The caller starts joins and installs the match once the lobby starts
//! it.

use crate::game::{Game, SoundKind};
use crate::menu::{Label, Line, Menu, PreviewCache};
use crate::netplay::{Link, Lobby, address_char, parse_join_address};
use crate::numeric;
use crate::press::{Fed, Press};
use crate::screens::wizard::{self, CardController, CardView};
use crate::text_field::{Edit, Field, Form, Keyboard};
use crate::theme::Type;
use macroquad::prelude::Vec2;
use macroquad::prelude::{Rect, draw_rectangle, draw_rectangle_lines, draw_text, measure_text};
use oxide_net::Scanner;
use oxide_net::discovery::{Heard, Sighting};
use oxide_net::{NameError, PlayerName};
use oxide_net::{RoomSeat, SeatHolder};
use oxide_protocol::{Key, RawEvent};
use oxide_sim::{PlayerId, Scenario};
use std::net::SocketAddr;
use std::time::Duration;

/// Longest host address the field accepts: the longest DNS name and a port.
const ADDRESS_MAX: usize = 253 + ":65535".len();

/// What a lobby frame decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    /// Keep going.
    Stay,
    /// Join the host at `address`, which names a port, as `name`.
    Join { name: PlayerName, address: String },
    /// Leave for the screen underneath.
    Cancel,
}

enum Face {
    /// Asking who and where; `failure` says why the last join did not.
    Join {
        form: Form,
        failure: Option<String>,
        /// Listening for lobbies on the local network.
        scan: Scan,
        /// The lobbies heard lately.
        sightings: Vec<Sighting>,
        /// The listed lobby the keyboard is on, once Down leaves the last
        /// field.
        cursor: Option<SocketAddr>,
        /// The listed lobby a press armed, by address: the list can reorder
        /// under a finger.
        press: Press<SocketAddr>,
    },
    /// Machines gathering.
    Waiting(Lobby),
}

/// Whether the Join form listens for lobbies.
enum Scan {
    /// Not asked to (tests stay off the shared discovery port).
    Off,
    /// The platform refused the socket.
    Failed,
    Listening(Box<Scanner>),
}

/// The lobby screen.
pub struct LobbyScreen {
    face: Face,
    /// Whether forms this screen opens listen for lobbies.
    discover: bool,
    /// The form's fields, or the roster and Cancel, for the UI report.
    pub menu: Menu<usize>,
    /// The corner Back button over a joiner's copy of the setup screen.
    back: crate::button::BackButton,
}

fn name_check(name: &str) -> Result<(), &'static str> {
    PlayerName::new(name).map(drop).map_err(NameError::reason)
}

fn address_check(address: &str) -> Result<(), &'static str> {
    parse_join_address(address).map(drop)
}

impl LobbyScreen {
    /// The Join form, prefilled with the saved `name` and `address`;
    /// focus starts on whichever the player still has to type.
    pub fn join(name: Option<&str>, address: &str) -> Self {
        let name = name.unwrap_or_default();
        let name_field = Field::new(name, PlayerName::MAX_CHARS, PlayerName::accepts)
            .labeled("Your name", "")
            .validated(name_check);
        let address_field = Field::new(address, ADDRESS_MAX, address_char)
            .labeled("Host address", "e.g. 192.168.1.20")
            .validated(address_check)
            .keyboard(Keyboard::Address)
            .weight(2.0);
        let focus = usize::from(PlayerName::new(name).is_ok());
        let form = Form::new("JOIN MATCH", "JOIN", vec![name_field, address_field], focus);
        Self::face(Face::Join {
            form,
            failure: None,
            scan: Scan::Off,
            sightings: Vec::new(),
            cursor: None,
            press: Press::default(),
        })
    }

    /// Lists the lobbies on the local network under the Join form, now and
    /// on every return to it.
    pub fn discovering(mut self) -> Self {
        self.discover = true;
        if let Face::Join { scan, .. } = &mut self.face {
            *scan = match Scanner::bind(&crate::build_identity().revision) {
                Ok(scanner) => Scan::Listening(Box::new(scanner)),
                Err(_) => Scan::Failed,
            };
        }
        self
    }

    /// Listens with `scanner` instead of the shared discovery port.
    #[cfg(test)]
    pub fn with_scanner(mut self, scanner: Scanner) -> Self {
        if let Face::Join { scan, .. } = &mut self.face {
            *scan = Scan::Listening(Box::new(scanner));
        }
        self
    }

    /// The Join form showing why the last join failed.
    pub fn failed(name: &str, address: &str, reason: String) -> Self {
        let mut screen = Self::join(Some(name), address);
        if let Face::Join { failure, .. } = &mut screen.face {
            *failure = Some(reason);
        }
        screen
    }

    /// Waits on `lobby`.
    pub fn waiting(lobby: Lobby) -> Self {
        Self::face(Face::Waiting(lobby))
    }

    fn face(face: Face) -> Self {
        let menu = match &face {
            Face::Join { form, .. } => form.menu(|index| index),
            Face::Waiting(lobby) => Menu::new("LAN MATCH", waiting_lines(lobby)),
        };
        Self {
            face,
            discover: false,
            menu,
            back: crate::button::BackButton::default(),
        }
    }

    /// Polls the lobby; returns the match once it starts. A join that
    /// gives up comes back to the form, with the reason.
    pub fn poll(&mut self, now: Duration, viewport: Vec2) -> Option<(Game, Link)> {
        if let Face::Join {
            form,
            scan,
            sightings,
            cursor,
            ..
        } = &mut self.face
        {
            if let Scan::Listening(scanner) = scan {
                *sightings = scanner.poll(now).to_vec();
            }
            self.menu = join_menu(form, sightings, *cursor);
            return None;
        }
        let Face::Waiting(lobby) = &mut self.face else {
            return None;
        };
        let started = lobby.poll(now, viewport);
        if let Some((name, address, reason)) = lobby.failure() {
            let form = Self::failed(name.as_str(), address, reason.to_owned());
            *self = if self.discover {
                form.discovering()
            } else {
                form
            };
        } else {
            self.menu.set_lines(waiting_lines(lobby));
        }
        started
    }

    /// The keyboard the focused field asks for, while the form is up.
    pub fn keyboard(&self) -> Option<Keyboard> {
        match &self.face {
            Face::Join { form, .. } => Some(form.keyboard()),
            Face::Waiting(_) => None,
        }
    }

    /// Consumes a request to raise the on-screen keyboard again.
    pub fn take_keyboard_request(&mut self) -> bool {
        match &mut self.face {
            Face::Join { form, .. } => form.take_keyboard_request(),
            Face::Waiting(_) => false,
        }
    }

    /// The debug protocol's stable mode name for the current face.
    pub fn mode_name(&self) -> &'static str {
        match self.face {
            Face::Join { .. } => "join_match",
            Face::Waiting(_) => "lobby",
        }
    }

    /// The line under the title: the lobby's status, or why the last join
    /// failed.
    pub fn subtitle(&self) -> String {
        match &self.face {
            Face::Join { failure, .. } => failure.clone().unwrap_or_default(),
            Face::Waiting(lobby) => lobby.status(),
        }
    }

    /// Applies a frame's events.
    pub fn update(
        &mut self,
        events: &[RawEvent],
        mouse: &mut Vec2,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
    ) -> Out {
        match &mut self.face {
            Face::Join {
                form,
                sightings,
                cursor,
                press,
                ..
            } => {
                let rows = match_rows(form, sightings.len());
                let shown = &sightings[..rows.len()];
                let joinable: Vec<SocketAddr> =
                    shown.iter().filter_map(Sighting::address).collect();
                if cursor.is_some_and(|at| !joinable.contains(&at)) {
                    *cursor = None;
                }
                let mut picked = None;
                let mut form_events = Vec::with_capacity(events.len());
                for event in events {
                    let fed = press.feed(event, |p, _| {
                        let row = rows.iter().position(|rect| rect.contains(p))?;
                        shown[row].address()
                    });
                    if let Fed::Activated(address) = fed {
                        picked = Some(address);
                        continue;
                    }
                    match (*cursor, event) {
                        (None, RawEvent::KeyDown { key: Key::Down })
                            if form.on_last_field() && !joinable.is_empty() =>
                        {
                            *cursor = joinable.first().copied();
                        }
                        (Some(at), RawEvent::KeyDown { key }) => {
                            let index = joinable.iter().position(|&listed| listed == at);
                            let index = index.unwrap_or_default();
                            match key {
                                Key::Down => {
                                    *cursor = Some(joinable[(index + 1).min(joinable.len() - 1)]);
                                }
                                Key::Up => *cursor = index.checked_sub(1).map(|up| joinable[up]),
                                Key::Tab => *cursor = None,
                                Key::Enter => picked = Some(at),
                                Key::Escape => form_events.push(*event),
                                _ => {}
                            }
                        }
                        // Typing goes nowhere while the list has the keyboard.
                        (Some(_), RawEvent::Text { .. }) => {}
                        (Some(_), RawEvent::MouseDown { .. } | RawEvent::TouchDown { .. }) => {
                            *cursor = None;
                            form_events.push(*event);
                        }
                        _ => form_events.push(*event),
                    }
                }
                if let Some(address) = picked {
                    sounds.push((SoundKind::Click, None));
                    match PlayerName::new(form.value(0)) {
                        Ok(name) => {
                            return Out::Join {
                                name,
                                address: address.to_string(),
                            };
                        }
                        Err(error) => {
                            *cursor = None;
                            form.refuse(0, error.reason(), sounds);
                        }
                    }
                }
                let edit = form.update(&form_events, sounds);
                self.menu = join_menu(form, sightings, *cursor);
                match edit {
                    Edit::Commit => {
                        match (
                            PlayerName::new(form.value(0)),
                            parse_join_address(form.value(1)),
                        ) {
                            (Ok(name), Ok(address)) => Out::Join { name, address },
                            // The form refuses a commit either check fails.
                            _ => Out::Stay,
                        }
                    }
                    Edit::Cancel => Out::Cancel,
                    Edit::Stay => Out::Stay,
                }
            }
            Face::Waiting(lobby) => {
                let escaped = events
                    .iter()
                    .any(|event| matches!(event, RawEvent::KeyDown { key: Key::Escape }));
                if lobby.room_view().is_some() {
                    // The copy of the setup screen draws no rows, so only
                    // Back and Escape leave.
                    let (back, _) = self.back.route(events);
                    if back {
                        sounds.push((SoundKind::Click, None));
                    }
                    return if back || escaped {
                        Out::Cancel
                    } else {
                        Out::Stay
                    };
                }
                if self.menu.handle(events, mouse).is_some() {
                    sounds.push((SoundKind::Click, None));
                    return Out::Cancel;
                }
                if escaped { Out::Cancel } else { Out::Stay }
            }
        }
    }

    /// Draws the form; or, waiting, a joiner's copy of the host's setup
    /// once it arrives, else the status, roster, and Cancel.
    pub fn draw(&self, mouse: Vec2, previews: &mut PreviewCache) {
        if let Face::Waiting(lobby) = &self.face
            && let Some((map, seats, you)) = lobby.room_view()
        {
            draw_room(map, seats, you, &self.subtitle(), previews);
            crate::button::draw_back(mouse);
            return;
        }
        match &self.face {
            Face::Join {
                form,
                failure,
                scan,
                sightings,
                cursor,
                ..
            } => {
                let color = if failure.is_some() {
                    crate::theme::TEXT_DANGER
                } else {
                    crate::theme::TEXT_SECONDARY
                };
                form.draw(&self.subtitle(), color, mouse);
                if !matches!(scan, Scan::Off) {
                    draw_matches(form, scan, sightings, *cursor, mouse);
                }
            }
            Face::Waiting(_) => self.menu.draw(&self.subtitle()),
        }
    }
}

/// The Join form's rows for the UI report: its fields, then one line per
/// lobby heard, a row for each joinable one.
fn join_menu(form: &Form, sightings: &[Sighting], cursor: Option<SocketAddr>) -> Menu<usize> {
    let fields = form.menu(|index| index);
    let mut lines = fields.lines().to_vec();
    let first = lines.len();
    lines.extend(sightings.iter().enumerate().map(|(index, sighting)| {
        let label = match_label(sighting);
        if sighting.address().is_some() {
            Line::Row(Label::Text(label), first + index)
        } else {
            Line::Header(label)
        }
    }));
    let mut menu = Menu::new("JOIN MATCH", lines);
    let listed = cursor.and_then(|at| {
        sightings
            .iter()
            .position(|sighting| sighting.address() == Some(at))
    });
    menu.select(listed.map_or(fields.selected, |index| first + index));
    menu
}

/// One lobby as the list names it.
fn match_label(sighting: &Sighting) -> String {
    match &sighting.heard {
        Heard::Joinable(beacon) => format!(
            "Match: {} | {} | {} of {} players",
            beacon.map, beacon.host, beacon.players, beacon.seats
        ),
        Heard::OtherBuild { host, .. } => format!("Match: {host} | different build"),
    }
}

/// Where the match list's rows sit, under the form's buttons.
fn match_rows(form: &Form, count: usize) -> Vec<Rect> {
    let s = crate::render::ui_scale();
    let layout = form.layout(crate::render::viewport(), s);
    let (Some(first), Some(last)) = (layout.fields.first(), layout.fields.last()) else {
        return Vec::new();
    };
    let height = crate::theme::MIN_TOUCH_TARGET * s;
    let top = layout.confirm.y + layout.confirm.h + 46.0 * s;
    (0..count.min(4))
        .map(|row| {
            Rect::new(
                first.x,
                top + row as f32 * (height + 6.0 * s),
                last.x + last.w - first.x,
                height,
            )
        })
        .collect()
}

/// Draws "Matches on your network" and a row per lobby heard.
fn draw_matches(
    form: &Form,
    scan: &Scan,
    sightings: &[Sighting],
    cursor: Option<SocketAddr>,
    mouse: Vec2,
) {
    let s = crate::render::ui_scale();
    let layout = form.layout(crate::render::viewport(), s);
    let Some(first) = layout.fields.first() else {
        return;
    };
    let heading_y = layout.confirm.y + layout.confirm.h + 34.0 * s;
    draw_text(
        "MATCHES ON YOUR NETWORK",
        first.x,
        heading_y,
        Type::Small.at(s),
        crate::theme::TEXT_SECONDARY,
    );
    let empty = match scan {
        Scan::Failed => Some("Can't search this network"),
        _ if sightings.is_empty() => Some("No matches found"),
        _ => None,
    };
    if let Some(line) = empty {
        draw_text(
            line,
            first.x,
            heading_y + 30.0 * s,
            Type::Label.at(s),
            crate::theme::TEXT_DISABLED,
        );
        return;
    }
    for (rect, sighting) in match_rows(form, sightings.len()).iter().zip(sightings) {
        let joinable = sighting.address().is_some();
        let hovered = joinable && (rect.contains(mouse) || sighting.address() == cursor);
        draw_rectangle(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            if hovered {
                crate::theme::SURFACE_CARD
            } else {
                crate::theme::SURFACE_PANEL
            },
        );
        draw_rectangle_lines(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            crate::theme::Stroke::Hairline.at(s),
            if hovered {
                crate::theme::TEXT_ACCENT
            } else {
                crate::theme::BORDER_STRONG
            },
        );
        let (left, right) = match &sighting.heard {
            Heard::Joinable(beacon) => (
                beacon.map.clone(),
                format!(
                    "{} - {} of {} players",
                    beacon.host, beacon.players, beacon.seats
                ),
            ),
            Heard::OtherBuild { host, .. } => (host.clone(), "different build".to_owned()),
        };
        let size = Type::Body.at(s);
        let y = rect.y + rect.h * 0.5 + size * 0.35;
        let (strong, quiet) = if joinable {
            (crate::theme::TEXT_PRIMARY, crate::theme::TEXT_SECONDARY)
        } else {
            (crate::theme::TEXT_DISABLED, crate::theme::TEXT_DISABLED)
        };
        draw_text(&left, rect.x + 12.0 * s, y, size, strong);
        let small = Type::Label.at(s);
        let width = measure_text(&right, None, numeric::font_size(small), 1.0).width;
        draw_text(&right, rect.x + rect.w - width - 12.0 * s, y, small, quiet);
    }
}

/// Draws a joiner's read-only copy of the host's setup screen: the seat
/// cards grouped by team, the map with every chair marked, and the status.
fn draw_room(
    map: &Scenario,
    seats: &[RoomSeat],
    you: PlayerId,
    status: &str,
    previews: &mut PreviewCache,
) {
    let view = crate::render::viewport();
    let ui = crate::render::ui_scale();
    let you = usize::from(you.0);
    // People and open chairs wear one wide chip, as on the host's screen.
    let wide: Vec<bool> = seats
        .iter()
        .map(|seat| !matches!(seat.holder, SeatHolder::Bot { .. }))
        .collect();
    let order = wizard::seat_display_order(map);
    let mine = order.iter().position(|&seat| seat == you).unwrap_or(0);
    let mut layout = wizard::setup_layout_page(map, you, &wide, view, ui, 0);
    for page in 1..layout.page_count {
        if layout.cards.get(mine).is_some_and(Option::is_some) {
            break;
        }
        layout = wizard::setup_layout_page(map, you, &wide, view, ui, page);
    }

    let centered = |text: &str, y: f32, size: f32, color| {
        let width = measure_text(text, None, numeric::font_size(size), 1.0).width;
        draw_text(text, (view.x - width) * 0.5, y, size, color);
    };
    centered(
        "MATCH SETUP",
        64.0 * ui,
        Type::Title.at(ui),
        crate::theme::TEXT_TITLE,
    );
    let line = format!("{} - {status}", map.name);
    let size = crate::typography::fit(&line, Type::Label.at(ui), view.x - 32.0 * ui, 10.0);
    centered(&line, 92.0 * ui, size, crate::theme::TEXT_SECONDARY);

    wizard::draw_team_headings(&layout, ui);
    let styles = wizard::seat_styles(seats.len(), you, |seat| {
        seats.get(seat).and_then(|seat| seat.team)
    });
    for (pos, card) in layout.cards.iter().enumerate() {
        let Some(card) = card else {
            continue;
        };
        let seat = order[pos];
        let controller = if seat == you {
            CardController::Own
        } else {
            match &seats[seat].holder {
                SeatHolder::Host { name } => CardController::Person {
                    name: name.as_str(),
                    host: true,
                },
                SeatHolder::Player { name } => CardController::Person {
                    name: name.as_str(),
                    host: false,
                },
                SeatHolder::Bot { difficulty, stance } => CardController::Bot {
                    difficulty: *difficulty,
                    stance: *stance,
                },
                SeatHolder::Open => CardController::Open,
                SeatHolder::Reserved { name } => CardController::Reserved {
                    name: name.as_str(),
                },
            }
        };
        let team = seats[seat].team.map_or_else(
            || "FFA".to_owned(),
            |team| format!("Team {}", u16::from(team) + 1),
        );
        let card_view = CardView {
            seat,
            accent: styles.get(oxide_sim::PlayerId::from_index(seat)),
            name: map.players[seat].name.as_str(),
            controller,
            team,
            focus: None,
            moving: false,
        };
        wizard::draw_seat_card(card, &card_view, ui);
    }
    if let Some(tex) = previews.get_map(map) {
        let theme = map.meta.as_ref().map_or("", |meta| meta.theme.as_str());
        let drawn = wizard::draw_preview(tex, theme, layout.preview, ui);
        wizard::draw_seat_markers(map, &styles, drawn, you, None, ui);
    }
}

/// The waiting face's lines: the roster, which the cursor skips, then
/// Cancel.
fn waiting_lines(lobby: &Lobby) -> Vec<Line<usize>> {
    lobby
        .roster()
        .into_iter()
        .map(Line::Header)
        .chain([Line::Row(Label::from("Cancel"), 0)])
        .collect()
}

#[cfg(test)]
mod tests;
