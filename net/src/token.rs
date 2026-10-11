//! Random 128-bit identifiers: the secret a host gives each client at Go,
//! which proves the client's seat when it rejoins the running match, and
//! the identifier of an installation, which helps a host seat a returning
//! player.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// A client's rejoin secret: 128 random bits, chosen by the host's caller
/// and sent only to that client.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Token([u8; 16]);

impl Token {
    /// A token holding `bits`.
    pub const fn new(bits: u128) -> Self {
        Self(bits.to_be_bytes())
    }

    /// The token as 32 lowercase hex digits, as it travels and is stored.
    pub fn to_hex(self) -> String {
        hex(&self.0)
    }

    /// Reads 32 hex digits; `None` for anything else.
    pub fn from_hex(text: &str) -> Option<Self> {
        unhex(text).map(Self)
    }
}

impl fmt::Debug for Token {
    // Logs may name a token, never spell it out.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Token(..)")
    }
}

impl Serialize for Token {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Token {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::from_hex(&text).ok_or_else(|| serde::de::Error::custom("a token is 32 hex digits"))
    }
}

/// An installation of the game: 128 random bits it keeps in its settings and
/// sends only to a host it joins. Two copies running on one machine share
/// it, so it identifies a player only when no one else in the room has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallId([u8; 16]);

impl InstallId {
    /// An identifier holding `bits`.
    pub const fn new(bits: u128) -> Self {
        Self(bits.to_be_bytes())
    }

    /// The identifier as 32 lowercase hex digits, as it travels and is
    /// stored.
    pub fn to_hex(self) -> String {
        hex(&self.0)
    }

    /// Reads 32 hex digits; `None` for anything else.
    pub fn from_hex(text: &str) -> Option<Self> {
        unhex(text).map(Self)
    }
}

impl Serialize for InstallId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for InstallId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::from_hex(&text)
            .ok_or_else(|| serde::de::Error::custom("an install id is 32 hex digits"))
    }
}

/// 32 hex digits as 16 bytes; `None` for anything else.
fn unhex(text: &str) -> Option<[u8; 16]> {
    if text.len() != 32 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = [0; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * index..2 * index + 2], 16).ok()?;
    }
    Some(bytes)
}

/// `bytes` as lowercase hex, two digits each.
pub(crate) fn hex(bytes: &[u8]) -> String {
    use fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(2 * bytes.len()), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

#[cfg(test)]
mod tests;
