//! The score: the game's music, cut into sections it strings together.
//!
//! A match takes the next of its songs in turn. The song plays its intro,
//! then eight-bar sections in an order picked as it goes, never the same kind
//! twice running, and after a while gives way to the next song at a calm
//! moment. Every section of a song has a calm and a fight layer that play in
//! step, and the fight layer comes up with the size of the fight within
//! earshot, so a skirmish leaves the music calm. The match also calls for sections:
//! siege while the local seat's structures are under sustained attack, final
//! push while its army fights among hostile structures late in the match, and
//! the song's lift in a big battle. The menu theme cycles its sections in
//! order, and an ending plays its phrase once and then loops.
//!
//! Each section rings past its end under the next one, which starts on the
//! audio clock the moment the last one's body ends. Sections decode on a
//! worker thread shortly before they are due, so only a few are ever held.

mod catalog;
mod conductor;

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use kira::Frame;

use crate::audio_engine::{self, SCORE_RATE, ScoreTrack, ScoreVoice};
use catalog::{Catalog, FINAL_PUSH, Role, SIEGE};
use conductor::Conductor;
pub(crate) use conductor::Cues;

/// The presentation context the score reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scene {
    /// Home, setup, the saves and replays shelf, or Settings opened from Home.
    Menu,
    /// A live or replayed match.
    Match,
    /// An ongoing match behind Pause or Settings.
    Pause,
    /// A drawn match.
    Result,
    /// The local player's team won.
    Victory,
    /// Another team won.
    Defeat,
}

/// The scene to score this frame, and whether a match screen should start
/// its music over. A holding screen, `None`, keeps the last scene a screen
/// named; a newly installed match restarts once a screen shows it.
fn follow(held: &mut Scene, fresh: &mut bool, scene: Option<Scene>) -> (Scene, bool) {
    match scene {
        None => (*held, false),
        Some(scene) => {
            *held = scene;
            (scene, std::mem::take(fresh) && scene.role() == Role::Match)
        }
    }
}

impl Scene {
    fn role(self) -> Role {
        match self {
            Self::Menu => Role::Menu,
            Self::Match | Self::Pause => Role::Match,
            Self::Result => Role::Draw,
            Self::Victory => Role::Victory,
            Self::Defeat => Role::Defeat,
        }
    }
}

/// Every section's gain: a match song's calm layer plays at about
/// -31 LUFS, well under the battle, and the other pieces keep their level
/// beside it.
const SCORE_GAIN_DB: f32 = -12.8;
/// How long the music on its way out takes to fade when the screen calls
/// for another piece.
const HANDOFF: Duration = Duration::from_secs(2);
/// How far ahead of its start the score picks and decodes a section. The
/// pick reads the match as it is then.
const LEAD_SECS: u64 = 6;
/// The least notice a section starts with, a tenth of a second, so both
/// layers are queued on the audio thread before the clock reaches them.
const MARGIN_TICKS: u64 = SCORE_RATE as u64 / 10;
/// How long before a special section the fight layer starts to come up:
/// long enough for it to be fully up as the section starts.
const SPECIAL_RISE_SECS: u64 = 3;

/// Guns heard lately up to which the music stays calm, and from which the
/// fight layer has it all; between them the two layers blend.
const CALM_GUNS: usize = 3;
const FIGHT_GUNS: usize = 10;
/// How fast the fight layer's share rises and falls, per second: over about
/// three seconds up and six down, so a short exchange does not flip it.
const FIGHT_RISE: f32 = 1.0 / 3.0;
const FIGHT_FALL: f32 = 1.0 / 6.0;
/// The music behind Pause or Settings, as a gain.
const PAUSE_DUCK: f32 = 0.316;
const DUCK_RATE: f32 = 1.0;

/// The pure crossfade between the calm and fight layers, and the duck
/// behind Pause.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Layers {
    /// The fight layer's share, 0 to 1; the calm layer has the rest.
    fight: f32,
    duck: f32,
}

impl Default for Layers {
    fn default() -> Self {
        Self {
            fight: 0.0,
            duck: 1.0,
        }
    }
}

/// The score tracks' gains for one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Volumes {
    pub calm: f32,
    pub fight: f32,
    pub solo: f32,
}

impl Layers {
    /// Advances the crossfade toward the size of the fight within earshot,
    /// `guns` heard lately; a special section holds the fight layer up.
    pub(crate) fn update(&mut self, scene: Scene, guns: usize, special: bool, dt: f32) -> Volumes {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.25)
        } else {
            0.0
        };
        let fight = if special {
            1.0
        } else if scene == Scene::Match || scene == Scene::Pause {
            fight_share(guns)
        } else {
            0.0
        };
        let rate = if fight > self.fight {
            FIGHT_RISE
        } else {
            FIGHT_FALL
        };
        approach(&mut self.fight, fight, rate * dt);
        let duck = if scene == Scene::Pause {
            PAUSE_DUCK
        } else {
            1.0
        };
        approach(&mut self.duck, duck, DUCK_RATE * dt);
        Volumes {
            calm: (1.0 - self.fight) * self.duck,
            fight: self.fight * self.duck,
            solo: self.duck,
        }
    }

    /// Whether the fight layer has gone quiet.
    fn calm(self) -> bool {
        self.fight < 0.1
    }
}

/// The fight layer's share for `guns` heard lately.
fn fight_share(guns: usize) -> f32 {
    let over = guns.saturating_sub(CALM_GUNS).min(FIGHT_GUNS - CALM_GUNS);
    over as f32 / (FIGHT_GUNS - CALM_GUNS) as f32
}

fn approach(value: &mut f32, target: f32, step: f32) {
    if *value < target {
        *value = (*value + step).min(target);
    } else {
        *value = (*value - step).max(target);
    }
}

/// How long one hit keeps counting toward a siege or a final push, as the
/// time constant of its decay.
const HEAT_SECS: f32 = 6.0;
/// The decayed hit count that makes a siege or a final push: a gun or two
/// firing for several seconds, not one stray shot.
const PRESSURE_HITS: f32 = 5.0;
/// Distinct guns heard firing within `BATTLE_SECS` that make a big battle.
const BATTLE_GUNS: usize = 20;
/// How long a gun counts toward the size of a fight after it last fired.
const BATTLE_SECS: f32 = 8.0;
/// When a match is late enough for a final push, in simulation ticks.
pub(crate) const FINAL_PUSH_AFTER: aftermine_sim::Tick =
    10 * 60 * aftermine_sim::TICKS_PER_SECOND as aftermine_sim::Tick;

/// A count of recent hits that decays while nothing lands.
#[derive(Debug, Default, Clone, Copy)]
struct Heat {
    value: f32,
    at: f32,
}

impl Heat {
    fn read(self, now: f32) -> f32 {
        self.value * (-(now - self.at).max(0.0) / HEAT_SECS).exp()
    }

    fn hit(&mut self, now: f32) {
        *self = Self {
            value: self.read(now) + 1.0,
            at: now,
        };
    }
}

/// What a match's events have told the score lately. The presentation
/// feeds it only events its seat could know of.
#[derive(Debug, Default, Clone)]
pub(crate) struct Pressure {
    siege: Heat,
    push: Heat,
    /// Each gun heard firing, a unit or a structure, and when it last did.
    guns: HashMap<aftermine_sim::Target, f32>,
}

impl Pressure {
    /// A hit on one of the local seat's structures.
    pub(crate) fn structure_hit(&mut self, now: f32) {
        self.siege.hit(now);
    }

    /// A hit by the local seat's army on a hostile structure it can see,
    /// late in the match.
    pub(crate) fn push_hit(&mut self, now: f32) {
        self.push.hit(now);
    }

    /// A gun heard firing: a unit's weapon, a turret or an artillery piece.
    pub(crate) fn gun_heard(&mut self, gun: aftermine_sim::Target, now: f32) {
        if self.guns.len() > 4 * BATTLE_GUNS {
            self.guns.retain(|_, &mut at| now - at < BATTLE_SECS);
        }
        self.guns.insert(gun, now);
    }

    /// The cues as of `now`. A spectator defends no seat, so only the
    /// fight's size reaches the score from a replay.
    pub(crate) fn cues(&self, now: f32, spectating: bool) -> Cues {
        let seat = !spectating;
        let guns = self
            .guns
            .values()
            .filter(|&&at| now - at < BATTLE_SECS)
            .count();
        Cues {
            siege: seat && self.siege.read(now) >= PRESSURE_HITS,
            final_push: seat && self.push.read(now) >= PRESSURE_HITS,
            big_battle: guns >= BATTLE_GUNS,
            guns,
        }
    }
}

struct Request {
    generation: u64,
    piece: usize,
    section: usize,
    frames: u64,
    files: Vec<(ScoreTrack, String)>,
}

struct Decoded {
    generation: u64,
    piece: usize,
    section: usize,
    layers: Result<Vec<(ScoreTrack, Arc<[Frame]>)>>,
}

/// Decodes requested sections off the frame thread.
fn spawn_decoder() -> Result<(Sender<Request>, Receiver<Decoded>)> {
    let (requests, inbox) = std::sync::mpsc::channel::<Request>();
    let (outbox, decoded) = std::sync::mpsc::channel();
    let root = crate::assets::resource_root().join("assets/music");
    std::thread::Builder::new()
        .name("score decoder".into())
        .spawn(move || {
            while let Ok(request) = inbox.recv() {
                let reply = Decoded {
                    generation: request.generation,
                    piece: request.piece,
                    section: request.section,
                    layers: decode_section(&root, &request),
                };
                if outbox.send(reply).is_err() {
                    return;
                }
            }
        })?;
    Ok((requests, decoded))
}

fn decode_section(
    root: &std::path::Path,
    request: &Request,
) -> Result<Vec<(ScoreTrack, Arc<[Frame]>)>> {
    request
        .files
        .iter()
        .map(|(track, file)| {
            let path = root.join(file);
            let bytes =
                std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            let frames = audio_engine::decode_ogg(&bytes)
                .with_context(|| format!("decoding {}", path.display()))?;
            ensure!(
                frames.len() as u64 == request.frames,
                "{} decodes to the wrong length",
                path.display()
            );
            Ok((*track, frames))
        })
        .collect()
}

/// A section queued or playing, for the fight layer's rise.
struct Placed {
    start: u64,
    end: u64,
    special: bool,
}

/// The score as it plays: the conductor's choices, the sections decoding
/// and playing, and the layer crossfade.
pub(crate) struct Soundtrack {
    catalog: Catalog,
    conductor: Conductor,
    requests: Sender<Request>,
    decoded: Receiver<Decoded>,
    /// Bumped on every change of piece, so a decode on its way for the old
    /// one is dropped.
    generation: u64,
    role: Option<Role>,
    /// A section is decoding.
    awaiting: bool,
    /// When the next section starts, once the last one is queued; `None`
    /// starts a new piece as soon as it can.
    next_start: Option<u64>,
    placed: VecDeque<Placed>,
    voices: Vec<ScoreVoice>,
    layers: Layers,
    /// A section failed to load; the score is silent until the next piece.
    failed: bool,
    /// The scene a holding screen keeps playing.
    held: Scene,
    /// A new match was installed; its music starts from the top once a
    /// match screen shows it.
    fresh: bool,
}

impl Soundtrack {
    /// Loads the music's manifest and starts its decoder. Nothing plays
    /// until the first [`Self::update`].
    pub fn start() -> Result<Self> {
        let catalog = Catalog::load()?;
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |since| u64::from(since.subsec_nanos()) ^ since.as_secs());
        let conductor = Conductor::new(&catalog, seed);
        let (requests, decoded) = spawn_decoder()?;
        Ok(Self {
            catalog,
            conductor,
            requests,
            decoded,
            generation: 0,
            role: None,
            awaiting: false,
            next_start: None,
            placed: VecDeque::new(),
            voices: Vec::new(),
            layers: Layers::default(),
            failed: false,
            held: Scene::Menu,
            fresh: false,
        })
    }

    /// Starts the next match screen's music from a new song's intro, for a
    /// match that replaced the one before it.
    pub fn new_match(&mut self) {
        self.fresh = true;
    }

    /// Follows the screen and the match, and queues sections as they come
    /// due. `None` holds the music already playing, as a loading screen
    /// does.
    pub fn update(&mut self, scene: Option<Scene>, cues: Cues, dt: f32) {
        self.voices.retain(|voice| !voice.finished());
        let Some(now) = audio_engine::score_tick() else {
            return;
        };
        let (scene, restart) = follow(&mut self.held, &mut self.fresh, scene);
        let role = scene.role();
        if self.role != Some(role) || restart {
            self.change_piece(role);
        } else if !self.awaiting
            && !self.failed
            && let Some(start) = self.next_start
            && start <= now + LEAD_SECS * u64::from(SCORE_RATE)
        {
            let (piece, section) = self.conductor.next(&self.catalog, cues, self.layers.calm());
            self.request(piece, section);
        }
        while let Ok(decoded) = self.decoded.try_recv() {
            if decoded.generation == self.generation {
                self.place(decoded, now);
            }
        }
        let rise = SPECIAL_RISE_SECS * u64::from(SCORE_RATE);
        while self.placed.front().is_some_and(|placed| placed.end <= now) {
            self.placed.pop_front();
        }
        let special = self
            .placed
            .iter()
            .any(|placed| placed.special && placed.start <= now + rise);
        let volumes = self.layers.update(scene, cues.guns, special, dt);
        audio_engine::set_score_volumes(volumes.calm, volumes.fight, volumes.solo);
    }

    fn change_piece(&mut self, role: Role) {
        self.role = Some(role);
        self.generation += 1;
        for voice in &mut self.voices {
            voice.fade_out(HANDOFF);
        }
        self.placed.clear();
        self.next_start = None;
        self.failed = false;
        let (piece, section) = self.conductor.begin(&self.catalog, role);
        self.request(piece, section);
    }

    fn request(&mut self, piece: usize, section: usize) {
        let at = &self.catalog.pieces[piece].sections[section];
        let files = at
            .layers()
            .into_iter()
            .map(|(track, file)| (track, file.to_owned()))
            .collect();
        let request = Request {
            generation: self.generation,
            piece,
            section,
            frames: at.frames,
            files,
        };
        if self.requests.send(request).is_ok() {
            self.awaiting = true;
        } else {
            self.failed = true;
        }
    }

    /// Queues a decoded section to start when the last one's body ends, or
    /// as soon as it can if it came too late for that.
    fn place(&mut self, decoded: Decoded, now: u64) {
        if !std::mem::take(&mut self.awaiting) {
            return;
        }
        let layers = match decoded.layers {
            Ok(layers) => layers,
            Err(error) => {
                eprintln!("music stopped: {error:#}");
                self.failed = true;
                return;
            }
        };
        let earliest = now + MARGIN_TICKS;
        let start = self
            .next_start
            .filter(|&start| start >= earliest)
            .unwrap_or(earliest);
        let section = &self.catalog.pieces[decoded.piece].sections[decoded.section];
        for (track, frames) in layers {
            if let Some(voice) = audio_engine::play_score_layer(track, frames, SCORE_GAIN_DB, start)
            {
                self.voices.push(voice);
            }
        }
        self.next_start = Some(start + section.body);
        self.placed.push_back(Placed {
            start,
            end: start + section.body,
            special: matches!(section.kind.as_str(), SIEGE | FINAL_PUSH),
        });
    }
}

/// The score for a match in `game`'s state, from its bound seat's side.
pub(crate) fn match_scene(game: &crate::game::Scene<'_>, paused: bool) -> Scene {
    use crate::game::Verdict;
    match Verdict::of(game.state, game.presentation.human) {
        Some(Verdict::Draw) => Scene::Result,
        Some(Verdict::Victory) => Scene::Victory,
        Some(Verdict::Defeat | Verdict::Surrendered) => Scene::Defeat,
        None if paused => Scene::Pause,
        None => Scene::Match,
    }
}

#[cfg(test)]
mod tests;
