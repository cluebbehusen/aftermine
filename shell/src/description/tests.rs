use super::*;

/// Words that put a figure in prose. "One" is left out: it reads as a
/// pronoun as often as a count ("one at a time").
const NUMBER_WORDS: [&str; 15] = [
    "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve",
    "twice", "half", "double", "triple",
];

fn typed_figures(words: &str) -> Vec<String> {
    let mut found: Vec<String> = words
        .split(|c: char| !c.is_ascii_digit())
        .filter(|run| !run.is_empty())
        .map(str::to_string)
        .collect();
    found.extend(
        words
            .split(|c: char| !c.is_ascii_alphabetic())
            .map(str::to_ascii_lowercase)
            .filter(|word| NUMBER_WORDS.contains(&word.as_str())),
    );
    found
}

fn texts() -> Vec<(String, Text)> {
    UnitKind::ALL
        .into_iter()
        .map(|kind| (kind.name().to_string(), unit_text(kind)))
        .chain(
            BuildingKind::ALL
                .into_iter()
                .map(|kind| (kind.name().to_string(), building_text(kind))),
        )
        .collect()
}

#[test]
fn typed_words_hold_no_figures() {
    for (name, text) in texts() {
        assert_eq!(
            typed_figures(text.words),
            Vec::<String>::new(),
            "{name}: put the figure in the stats table and fill a {{}} from it"
        );
    }
}

#[test]
fn every_gap_gets_one_figure() {
    for (name, text) in texts() {
        assert_eq!(
            text.words.matches("{}").count(),
            text.figures.len(),
            "{name}: {}",
            text.words
        );
        assert!(!text.fill().contains("{}"), "{name}");
    }
}

#[test]
fn figures_come_from_the_stats_table() {
    let sapper = UnitKind::Sapper
        .stats()
        .demolition
        .expect("a Sapper carries a charge");
    assert!(unit_description(UnitKind::Sapper).contains(&format!(
        "Deals {} damage to a structure and {} to enemy ground units",
        sapper.structure_damage, sapper.splash_damage
    )));
    let drones = UnitKind::Roc
        .stats()
        .bay
        .expect("a Roc carries drones")
        .capacity;
    assert!(unit_description(UnitKind::Roc).contains(&count_word(u32::from(drones))));
    let ring = BuildingKind::Lattice
        .battery()
        .expect("a Lattice is a battery")
        .ring;
    assert!(
        building_description(BuildingKind::Lattice).contains(&format!("{} tiles", tiles(ring)))
    );
}

#[test]
fn tiles_drop_a_whole_number_s_tenths() {
    assert_eq!(tiles(Fx::lit("15.0")), "15");
    assert_eq!(tiles(Fx::lit("1.5")), "1.5");
}

#[test]
fn small_counts_read_as_words() {
    assert_eq!(count_word(6), "six");
    assert_eq!(count_word(40), "40");
}
