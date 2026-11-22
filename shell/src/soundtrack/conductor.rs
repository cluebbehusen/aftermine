//! Which section plays next: pure choices over the catalog, so a test can
//! walk a match without an output device.

use super::catalog::{Catalog, FINAL_PUSH, LIFT, Role, SIEGE, is_free};

/// Sections a match song plays after its intro before the score moves on to
/// the next song, at the first calm boundary.
pub(crate) const SONG_SECTIONS: u32 = 8;

/// What the match on screen asks of the score, from the local seat's side
/// and only from what it can see.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cues {
    /// Its structures are under sustained attack.
    pub siege: bool,
    /// Late in the match, its army is fighting among hostile structures it
    /// can see.
    pub final_push: bool,
    /// Many guns are firing within earshot.
    pub big_battle: bool,
    /// Distinct guns heard firing lately, which sizes the fight.
    pub guns: usize,
}

/// The score's position in its music and the choices that move it on.
pub(crate) struct Conductor {
    piece: usize,
    section: usize,
    /// Match songs in the order they take turns, across matches too.
    songs: Vec<usize>,
    turn: usize,
    /// Sections the current match song has played since its intro.
    played: u32,
    rng: u64,
}

impl Conductor {
    /// A conductor whose song order and section picks follow `seed`.
    pub(crate) fn new(catalog: &Catalog, seed: u64) -> Self {
        let mut conductor = Self {
            piece: 0,
            section: 0,
            songs: catalog.songs(),
            turn: 0,
            played: 0,
            rng: seed | 1,
        };
        for index in (1..conductor.songs.len()).rev() {
            let other = conductor.below(index + 1);
            conductor.songs.swap(index, other);
        }
        conductor
    }

    /// xorshift64*: presentation only, so its stream needs no recording.
    fn below(&mut self, bound: usize) -> usize {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let value = self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32;
        usize::try_from(value).unwrap_or_default() % bound.max(1)
    }

    /// The first section of `role`'s music: a match takes the next song in
    /// turn from its intro.
    pub(crate) fn begin(&mut self, catalog: &Catalog, role: Role) -> (usize, usize) {
        self.piece = if role == Role::Match {
            self.next_song()
        } else {
            catalog.piece(role)
        };
        self.section = 0;
        self.played = 0;
        (self.piece, self.section)
    }

    fn next_song(&mut self) -> usize {
        let song = self.songs[self.turn % self.songs.len()];
        self.turn = (self.turn + 1) % self.songs.len();
        song
    }

    /// The section after the last one chosen. `calm` says the match is not
    /// fighting, so a song that has had its turn may give way.
    pub(crate) fn next(&mut self, catalog: &Catalog, cues: Cues, calm: bool) -> (usize, usize) {
        let piece = &catalog.pieces[self.piece];
        let count = piece.sections.len();
        self.section = match piece.role {
            Role::Menu => (self.section + 1) % count,
            Role::Victory | Role::Defeat | Role::Draw => (self.section + 1).min(count - 1),
            Role::Match => {
                let quiet = calm && !cues.siege && !cues.final_push && !cues.big_battle;
                if quiet && self.played >= SONG_SECTIONS && self.songs.len() > 1 {
                    return self.begin(catalog, Role::Match);
                }
                self.played += 1;
                self.pick(catalog, cues)
            }
        };
        (self.piece, self.section)
    }

    /// A match song's next section: what the match asks for, else any free
    /// section, but never the kind that just played.
    fn pick(&mut self, catalog: &Catalog, cues: Cues) -> usize {
        let sections = &catalog.pieces[self.piece].sections;
        let last = sections[self.section].kind.as_str();
        let wanted = if cues.siege {
            Some(SIEGE)
        } else if cues.final_push {
            Some(FINAL_PUSH)
        } else if cues.big_battle {
            Some(LIFT)
        } else {
            None
        };
        // A special moment that outlasts its section alternates with the
        // lift rather than repeating.
        let kind = match wanted {
            Some(kind) if kind != last => Some(kind),
            Some(_) if last != LIFT => Some(LIFT),
            _ => None,
        };
        let choices: Vec<usize> = (0..sections.len())
            .filter(|&index| {
                let candidate = sections[index].kind.as_str();
                match kind {
                    Some(kind) => candidate == kind,
                    None => is_free(candidate) && candidate != last,
                }
            })
            .collect();
        let pick = self.below(choices.len());
        choices.get(pick).copied().unwrap_or(self.section)
    }
}

#[cfg(test)]
mod tests;
