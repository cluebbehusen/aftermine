//! A player's display name: what other players see in the lobby and what
//! replaces the seat's name in the match.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A trimmed name of printable ASCII, at most [`PlayerName::MAX_CHARS`]
/// long. Decoding checks it too, so a name from the wire is as valid as a
/// typed one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PlayerName(String);

/// Why a name was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    /// Nothing but spaces.
    Empty,
    /// Longer than [`PlayerName::MAX_CHARS`].
    TooLong,
    /// A character outside printable ASCII.
    Character,
}

impl PlayerName {
    /// The longest name, in characters.
    pub const MAX_CHARS: usize = 16;

    /// `raw` trimmed, if what remains is a valid name.
    pub fn new(raw: &str) -> Result<Self, NameError> {
        let name = raw.trim();
        if name.is_empty() {
            Err(NameError::Empty)
        } else if name.chars().count() > Self::MAX_CHARS {
            Err(NameError::TooLong)
        } else if !name.chars().all(Self::accepts) {
            Err(NameError::Character)
        } else {
            Ok(Self(name.to_owned()))
        }
    }

    /// Whether a name may contain `ch`: printable ASCII, spaces included.
    pub fn accepts(ch: char) -> bool {
        ch == ' ' || ch.is_ascii_graphic()
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for PlayerName {
    type Error = NameError;

    fn try_from(raw: String) -> Result<Self, NameError> {
        Self::new(&raw)
    }
}

impl From<PlayerName> for String {
    fn from(name: PlayerName) -> Self {
        name.0
    }
}

impl fmt::Display for PlayerName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl NameError {
    /// The refusal as a sentence for the player.
    pub fn reason(self) -> &'static str {
        match self {
            NameError::Empty => "Enter your name",
            NameError::TooLong => "Names are at most 16 characters",
            NameError::Character => "Names use letters, digits, spaces, and punctuation",
        }
    }
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reason())
    }
}

impl std::error::Error for NameError {}

#[cfg(test)]
mod tests;
