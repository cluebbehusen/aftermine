//! Player-facing names for configured rules-based opponents.

use aftermine_sim::scenario::{BotDifficulty, BotStance};

/// Where a bot description is being shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotLabelStyle {
    /// The descriptive label in the selection panel and the post-match roster.
    Controller,
    /// The shortened label used by dense cards and post-match rosters.
    Compact,
}

/// Formats the visible skill and stance of one opponent.
///
/// Personality seeds and resolved specialties are intentionally absent: they
/// are replay provenance, not information the ordinary UI should reveal.
pub fn bot_label(difficulty: BotDifficulty, stance: BotStance, style: BotLabelStyle) -> String {
    let difficulty = difficulty_name(difficulty);
    let stance = stance_name(stance);
    match style {
        BotLabelStyle::Controller => format!("{difficulty} / {stance} bot"),
        BotLabelStyle::Compact => format!(
            "{}/{}",
            compact_difficulty(difficulty),
            compact_stance(stance)
        ),
    }
}

/// Human-readable difficulty name used by controls that show one dial.
pub const fn difficulty_name(difficulty: BotDifficulty) -> &'static str {
    match difficulty {
        BotDifficulty::Scrapheap => "Scrapheap",
        BotDifficulty::Standard => "Standard",
        BotDifficulty::Veteran => "Veteran",
        BotDifficulty::Prime => "Prime",
    }
}

/// Human-readable stance name used by controls that show one dial.
pub const fn stance_name(stance: BotStance) -> &'static str {
    match stance {
        BotStance::Turtle => "Turtle",
        BotStance::Balanced => "Balanced",
        BotStance::Aggressive => "Aggressive",
    }
}

fn compact_difficulty(name: &'static str) -> &'static str {
    match name {
        "Scrapheap" => "HEAP",
        "Standard" => "STD",
        "Veteran" => "VET",
        "Prime" => "PRIME",
        _ => name,
    }
}

fn compact_stance(name: &'static str) -> &'static str {
    match name {
        "Turtle" => "TURTLE",
        "Balanced" => "BAL",
        "Aggressive" => "AGGRO",
        _ => name,
    }
}

#[cfg(test)]
mod tests;
