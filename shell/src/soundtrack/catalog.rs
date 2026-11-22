//! The music on disk: `assets/music/manifest.json` and the Ogg Vorbis file
//! of every section layer it names.

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::audio_engine::{SCORE_RATE, ScoreTrack};

/// What a piece is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Role {
    /// One of the songs a match rotates through.
    Match,
    /// The menu theme.
    Menu,
    /// The local player's team won.
    Victory,
    /// Another team won, or the local player surrendered.
    Defeat,
    /// Every team's last Foundry fell together.
    Draw,
}

/// One section: what it is, when the next one starts, and its files.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Section {
    /// Its kind; sections of one kind share their music, such as A and A
    /// again.
    pub kind: String,
    /// Frames until the next section starts.
    pub body: u64,
    /// Frames in its files: the body plus the tail that rings under the
    /// next section.
    pub frames: u64,
    files: Files,
}

#[derive(Debug, Clone, Deserialize)]
struct Files {
    calm: Option<String>,
    fight: Option<String>,
    main: Option<String>,
}

impl Section {
    /// Each layer's track and its file under `assets/music`.
    pub(crate) fn layers(&self) -> Vec<(ScoreTrack, &str)> {
        [
            (ScoreTrack::Calm, &self.files.calm),
            (ScoreTrack::Fight, &self.files.fight),
            (ScoreTrack::Solo, &self.files.main),
        ]
        .into_iter()
        .filter_map(|(track, file)| file.as_deref().map(|file| (track, file)))
        .collect()
    }
}

/// One piece of music, its sections in the order they were written.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Piece {
    pub id: String,
    pub role: Role,
    pub sections: Vec<Section>,
}

/// Every piece the score plays.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Catalog {
    sample_rate: u32,
    pub pieces: Vec<Piece>,
}

/// A match song's opening, played once as it starts.
pub(crate) const INTRO: &str = "I";
/// A match song's lift, which a big battle asks for.
pub(crate) const LIFT: &str = "C";
/// Played while the local seat's structures are under sustained attack.
pub(crate) const SIEGE: &str = "S";
/// Played while the local seat's army fights among hostile structures late
/// in a match.
pub(crate) const FINAL_PUSH: &str = "F";
/// An ending's loop, after its phrase.
pub(crate) const ENDING_LOOP: &str = "L";

impl Catalog {
    /// Parses and checks a manifest.
    pub(crate) fn parse(json: &str) -> Result<Self> {
        let catalog: Self = serde_json::from_str(json).context("parsing the music manifest")?;
        ensure!(
            catalog.sample_rate == SCORE_RATE,
            "the music must be at {SCORE_RATE} Hz"
        );
        for role in [Role::Menu, Role::Victory, Role::Defeat, Role::Draw] {
            ensure!(
                catalog.pieces.iter().filter(|p| p.role == role).count() == 1,
                "the music needs one {role:?} piece"
            );
        }
        ensure!(
            catalog.pieces.iter().any(|p| p.role == Role::Match),
            "the music needs a match song"
        );
        for piece in &catalog.pieces {
            let id = &piece.id;
            ensure!(!piece.sections.is_empty(), "{id} has no sections");
            for section in &piece.sections {
                ensure!(
                    0 < section.body && section.body < section.frames,
                    "{id} has a section without a tail"
                );
                let tracks: Vec<_> = section.layers().iter().map(|(t, _)| *t).collect();
                let expected: &[ScoreTrack] = if piece.role == Role::Match {
                    &[ScoreTrack::Calm, ScoreTrack::Fight]
                } else {
                    &[ScoreTrack::Solo]
                };
                ensure!(tracks == expected, "{id} has the wrong layers");
            }
            match piece.role {
                Role::Match => {
                    ensure!(
                        piece.sections[0].kind == INTRO,
                        "{id} must open on its intro"
                    );
                    ensure!(
                        piece.sections.iter().any(|s| s.kind == LIFT)
                            && piece.sections.iter().any(|s| s.kind == SIEGE)
                            && piece.sections.iter().any(|s| s.kind == FINAL_PUSH),
                        "{id} needs a lift, a siege and a final push"
                    );
                    ensure!(
                        piece.sections.iter().filter(|s| is_free(&s.kind)).count() >= 2,
                        "{id} needs two kinds of section to alternate"
                    );
                }
                Role::Victory | Role::Defeat | Role::Draw => ensure!(
                    piece.sections.last().is_some_and(|s| s.kind == ENDING_LOOP),
                    "{id} must end on its loop"
                ),
                Role::Menu => {}
            }
        }
        Ok(catalog)
    }

    /// Loads the bundled manifest.
    pub(crate) fn load() -> Result<Self> {
        let path = crate::assets::resource_root().join("assets/music/manifest.json");
        let json = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&json)
    }

    /// The one piece for `role`; match songs go through [`Self::songs`].
    pub(crate) fn piece(&self, role: Role) -> usize {
        self.pieces
            .iter()
            .position(|piece| piece.role == role)
            .unwrap_or_default()
    }

    /// The match songs, in manifest order.
    pub(crate) fn songs(&self) -> Vec<usize> {
        (0..self.pieces.len())
            .filter(|&index| self.pieces[index].role == Role::Match)
            .collect()
    }
}

/// Whether a match song plays this kind of its own accord, rather than only
/// as it opens or when the match calls for it.
pub(crate) fn is_free(kind: &str) -> bool {
    !matches!(kind, INTRO | SIEGE | FINAL_PUSH)
}

#[cfg(test)]
mod tests;
