//! Guards the player-text rules in `AGENTS.md` by reading the shell's own
//! source: screen titles and small labels are all caps, everything else is
//! sentence case, and sizes come from the type scale. A new all-caps literal
//! fails here until it is added to the role it plays below.

use std::path::{Path, PathBuf};

/// Screen and dialog titles.
const TITLES: &[&str] = &[
    "AFTERMINE",
    "CLOSE THE LOBBY?",
    "COULD NOT SAVE",
    "DEFEAT",
    "DROP ?",
    "JOIN MATCH",
    "LAN MATCH",
    "LEFT-HANDED BINDINGS?",
    "LOADING MATCH",
    "MAPS",
    "MATCH RESULT",
    "MATCH SETUP",
    "MENU",
    "MUTUAL DESTRUCTION",
    "PAUSED",
    "REMOVE ?",
    "REPLACE",
    "RESET BINDINGS?",
    "ROSTER",
    "SAVE MATCH",
    "SAVES & REPLAYS",
    "SAVING MATCH",
    "SETTINGS",
    "SURRENDERED",
    "TUTORIAL",
    "THIS PLAYER",
    "VICTORY",
    "WAITING",
    "YOUR NAME",
];

/// Small labels over a value, column, section or card, and compact tags.
const LABELS: &[&str] = &[
    "AGGRO",
    "AIRWORKS",
    "ARMY VALUE",
    "AUTO",
    "BAL",
    "BUILD",
    "BUILT",
    "COLLECTED",
    "CONSTRUCTION",
    "CRADLES",
    "CRUCIBLE",
    "DONE",
    "DRONE BAYS",
    "ELIMINATED - SPECTATING",
    "FABRICATOR",
    "FOUNDRY",
    "HEAP",
    "HOST",
    "LAN AUTO",
    "LESSON :   |",
    "LOST",
    "MATCHES ON YOUR NETWORK",
    "PEAK",
    "PAUSED []",
    "PEAK ARMY",
    "PLAYER",
    "PRIME",
    "SCRAP",
    "SELECTED STRUCTURES",
    "SELECTED UNITS",
    "STD",
    "STEP  OF",
    "STRUCTURES",
    "SURRENDERED - SPECTATING",
    "TEAM",
    "TURTLE",
    "UNITS",
    "VALUE",
    "VET",
];

/// Acronyms, which keep their capitals anywhere.
const ACRONYMS: &[&str] = &["-- FPS", "FFA", "FPS", "LAN"];

/// Text whose every word is capitalized because each is a name, or that only
/// tools read.
const NAMED: &[&str] = &[
    ".  | Difficulty  | Stance  |",
    "Idle Harvesters",
    "Unclaimed Extractor",
];

/// Every production source file in the shell. Unit tests and the debug
/// server, which speaks to tools and not players, are not player text, and
/// neither are the environment variable names in `paths.rs`.
fn production_sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("source directory reads") {
            let path = entry.expect("directory entry reads").path();
            let name = path
                .file_name()
                .map_or("", |name| name.to_str().expect("source names are UTF-8"));
            if path.is_dir() {
                if name != "tests" {
                    walk(&path, out);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs")
                && name != "tests.rs"
                && !name.ends_with("_tests.rs")
                && name != "debug_server.rs"
                && name != "paths.rs"
            {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
    out.sort();
    out
}

/// The source with its `#[cfg(test)]` modules and comments left in place but
/// its string literals pulled out: each literal's text and the line it is on.
/// Byte strings and code inside comments are skipped.
fn string_literals(source: &str) -> Vec<(usize, String)> {
    let chars: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let (mut at, mut line) = (0, 1);
    while at < chars.len() {
        match chars[at] {
            '\n' => line += 1,
            '/' if chars.get(at + 1) == Some(&'/') => {
                while at < chars.len() && chars[at] != '\n' {
                    at += 1;
                }
                continue;
            }
            '\'' if chars.get(at + 1) == Some(&'"') => at += 2,
            '\'' if chars.get(at + 1) == Some(&'\\') => at += 3,
            '"' => {
                let byte = at > 0 && chars[at - 1] == 'b';
                let start = line;
                let mut text = String::new();
                at += 1;
                while at < chars.len() && chars[at] != '"' {
                    if chars[at] == '\\' {
                        at += 1;
                    }
                    if chars[at] == '\n' {
                        line += 1;
                    }
                    text.push(chars[at]);
                    at += 1;
                }
                if !byte {
                    out.push((start, text));
                }
            }
            _ => {}
        }
        at += 1;
    }
    out
}

/// `text` without its `{}` format holes, which hold no casing of their own.
fn without_holes(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in text.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

fn words(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_alphabetic())
        .filter(|word| !word.is_empty())
        .collect()
}

/// Whether `text` is a run of capitals long enough to be a word, not a key
/// name such as "Q" or an identifier such as an environment variable's name.
fn is_all_caps(text: &str) -> bool {
    let words = words(text);
    !text.contains('_')
        && words.iter().any(|word| word.chars().count() >= 3)
        && words
            .iter()
            .all(|word| word.chars().all(char::is_uppercase))
}

/// Whether `text` capitalizes every word, as "Save Game" does. One-word
/// strings say nothing; neither do strings holding an unbroken format hole.
fn is_title_case(text: &str) -> bool {
    let words = words(text);
    words.len() >= 2
        && words.iter().all(|word| {
            let mut letters = word.chars();
            letters.next().is_some_and(char::is_uppercase) && letters.any(char::is_lowercase)
        })
}

#[test]
fn all_caps_text_plays_a_declared_role() {
    let mut strays = Vec::new();
    for path in production_sources() {
        let source = std::fs::read_to_string(&path).expect("source reads");
        for (line, literal) in string_literals(&source) {
            let text = without_holes(&literal);
            let text = text.trim();
            if is_all_caps(text)
                && !TITLES.contains(&text)
                && !LABELS.contains(&text)
                && !ACRONYMS.contains(&text)
            {
                strays.push(format!("{}:{line}: {text}", path.display()));
            }
        }
    }
    assert!(
        strays.is_empty(),
        "buttons, rows and sentences are sentence case; a title or small label \
         joins TITLES or LABELS:\n{}",
        strays.join("\n")
    );
}

#[test]
fn player_text_is_never_title_case() {
    let mut strays = Vec::new();
    for path in production_sources() {
        let source = std::fs::read_to_string(&path).expect("source reads");
        for (line, literal) in string_literals(&source) {
            let text = without_holes(&literal);
            let text = text.trim();
            if is_title_case(text) && !NAMED.contains(&text) {
                strays.push(format!("{}:{line}: {text}", path.display()));
            }
        }
    }
    assert!(
        strays.is_empty(),
        "use sentence case, or add a true name to NAMED:\n{}",
        strays.join("\n")
    );
}

#[test]
fn only_the_menu_chooses_a_title_size() {
    let mut strays = Vec::new();
    for path in production_sources() {
        if path.ends_with("menu.rs") || path.ends_with("theme.rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("source reads");
        for (index, line) in source.lines().enumerate() {
            if line.contains("Type::Title") || line.contains("Type::Display") {
                strays.push(format!("{}:{}", path.display(), index + 1));
            }
        }
    }
    assert!(
        strays.is_empty(),
        "draw titles with crate::menu::draw_title or draw_page_heading:\n{}",
        strays.join("\n")
    );
}

/// Drawn text takes its size from `theme::Type`, and `typography::fit` will
/// only shrink it to a `Type`, which the compiler holds. This reads what the
/// compiler can't: a number in the draw call, or a local it traces back to.
/// Struct fields and parameters are not traced.
#[test]
fn drawn_text_sizes_come_from_the_type_scale() {
    let mut strays = Vec::new();
    for path in production_sources() {
        let source = std::fs::read_to_string(&path).expect("source reads");
        for call in ["draw_text(", "typography::draw("] {
            for (found, _) in source.match_indices(call) {
                let line = source[..found].lines().count();
                let before = &source[..found];
                let Some(size) = fourth_argument(&source[found + call.len()..]) else {
                    continue;
                };
                if comes_from_a_raw_number(before, &size) {
                    strays.push(format!("{}:{line}: {size}", path.display()));
                }
            }
        }
    }
    assert!(
        strays.is_empty(),
        "use a theme::Type size:\n{}",
        strays.join("\n")
    );
}

/// Whether `size`, as written just after `before`, is a number that never
/// passes through the type scale, following plain locals within the function.
fn comes_from_a_raw_number(before: &str, size: &str) -> bool {
    if size.contains("Type::") || size.contains("fit(") {
        return false;
    }
    if size.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    if !size.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return false;
    }
    let function = before.rfind(" fn ").map_or(before, |at| &before[at..]);
    let binding = format!(" {size} = ");
    let Some(at) = function.rfind(&binding) else {
        return false;
    };
    let rest = &function[at + binding.len()..];
    let value = rest.split(';').next().unwrap_or_default().trim();
    comes_from_a_raw_number(&function[..at], value)
}

/// The fourth comma-separated argument of the call whose text starts `call`.
fn fourth_argument(call: &str) -> Option<String> {
    let mut depth = 0;
    let mut in_string = false;
    let mut previous = ' ';
    let mut index = 0;
    let mut current = String::new();
    for c in call.chars() {
        if c == '"' && previous != '\\' {
            in_string = !in_string;
        }
        previous = c;
        if !in_string {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' if depth == 0 => {
                    return (index == 3).then(|| current.trim().to_string());
                }
                ')' | ']' | '}' => depth -= 1,
                ',' if depth == 0 => {
                    if index == 3 {
                        return Some(current.trim().to_string());
                    }
                    index += 1;
                    current.clear();
                    continue;
                }
                _ => {}
            }
        }
        current.push(c);
    }
    None
}

#[test]
fn the_rules_recognize_what_they_guard() {
    assert!(is_all_caps("WATCH REPLAY"));
    assert!(!is_all_caps("Q"));
    assert!(!is_all_caps("Watch replay"));
    assert!(is_title_case("Save Game"));
    assert!(!is_title_case("Save game"));
    assert!(!is_title_case("Settings"));
    assert_eq!(without_holes("REPLACE {}"), "REPLACE ");
    assert_eq!(
        string_literals("let a = \"ONE\"; // \"TWO\"\nlet b = b\"THREE\";\nlet c = \"FOUR\";"),
        vec![(1, "ONE".to_string()), (3, "FOUR".to_string())]
    );
    assert!(comes_from_a_raw_number("", "14.0 * s"));
    assert!(comes_from_a_raw_number(
        " fn draw() { let raw = 14.0 * s; let size = raw;",
        "size"
    ));
    assert!(!comes_from_a_raw_number(
        " fn draw() { let size = Type::Body.at(s);",
        "size"
    ));
    assert!(!comes_from_a_raw_number(
        " fn other() { let size = 14.0; }\n fn draw(size: f32) {",
        "size"
    ));
    assert_eq!(
        fourth_argument("text, x, y, size(1, 2), color);"),
        Some("size(1, 2)".to_string())
    );
}
