use super::*;

#[test]
fn every_profile_has_consistent_human_readable_labels() {
    let difficulties = [
        (BotDifficulty::Scrapheap, "Scrapheap", "HEAP"),
        (BotDifficulty::Standard, "Standard", "STD"),
        (BotDifficulty::Veteran, "Veteran", "VET"),
        (BotDifficulty::Prime, "Prime", "PRIME"),
    ];
    let stances = [
        (BotStance::Turtle, "Turtle", "TURTLE"),
        (BotStance::Balanced, "Balanced", "BAL"),
        (BotStance::Aggressive, "Aggressive", "AGGRO"),
    ];

    for (difficulty, difficulty_label, compact_difficulty) in difficulties {
        assert_eq!(difficulty_name(difficulty), difficulty_label);
        for (stance, stance_label, compact_stance) in stances {
            assert_eq!(stance_name(stance), stance_label);
            let controller = bot_label(difficulty, stance, BotLabelStyle::Controller);
            let compact = bot_label(difficulty, stance, BotLabelStyle::Compact);

            assert_eq!(
                controller,
                format!("{difficulty_label} / {stance_label} bot")
            );
            assert_eq!(compact, format!("{compact_difficulty}/{compact_stance}"));
        }
    }
}

#[test]
fn default_profile_is_no_longer_ambiguous_about_difficulty() {
    assert_eq!(
        bot_label(
            BotDifficulty::Standard,
            BotStance::Balanced,
            BotLabelStyle::Controller,
        ),
        "Standard / Balanced bot"
    );
    assert_eq!(
        bot_label(
            BotDifficulty::Standard,
            BotStance::Balanced,
            BotLabelStyle::Compact,
        ),
        "STD/BAL"
    );
}
