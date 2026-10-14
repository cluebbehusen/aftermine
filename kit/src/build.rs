//! Identity of the executable that recorded or wrote a file.

#[cfg(test)]
mod tests;

use serde::Serialize;

/// Build provenance does not participate in replay compatibility or state hashes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BuildIdentity {
    /// Package version.
    pub version: String,
    /// Recording commit, or `unknown` for a source archive.
    pub revision: String,
    /// Whether the executable's package/dependency trees and shared build inputs
    /// had changes: `true`, `false`, or `unknown` when source could not be inspected.
    pub dirty: String,
    /// Build operating system.
    pub os: String,
    /// Build architecture.
    pub architecture: String,
}
impl Default for BuildIdentity {
    fn default() -> Self {
        Self::new(env!("CARGO_PKG_VERSION"), "unknown", "unknown")
    }
}

impl BuildIdentity {
    /// The commit, marked `-dirty` when the build had local changes.
    pub fn label(&self) -> String {
        if self.dirty == "true" {
            format!("{}-dirty", self.revision)
        } else {
            self.revision.clone()
        }
    }

    /// Identity captured by the host executable's build script.
    pub fn new(version: &str, revision: &str, dirty: &str) -> Self {
        Self {
            version: version.into(),
            revision: revision.into(),
            dirty: dirty.into(),
            os: std::env::consts::OS.into(),
            architecture: std::env::consts::ARCH.into(),
        }
    }
}
