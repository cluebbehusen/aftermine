//! How the Roster writes the names of units and structures.

/// A simulation name as players read it: each word capitalized, so
/// "repair bay" becomes "Repair Bay".
pub fn entity_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut start = true;
    for c in name.chars() {
        if start {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
        start = c == ' ';
    }
    out
}

/// The plural of an entity name: "Foundry" to "Foundries", "Repair Bay"
/// to "Repair Bays", and a name already ending in s, like "Airworks",
/// unchanged.
pub fn plural_name(name: &str) -> String {
    let mut letters = name.chars().rev();
    match (letters.next(), letters.next()) {
        (Some('s'), _) => name.to_string(),
        (Some('y'), Some(before)) if !"aeiou".contains(before) => {
            format!("{}ies", &name[..name.len() - 1])
        }
        _ => format!("{name}s"),
    }
}

/// "x", "x and y", or "x, y, and z".
pub fn series(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [first, second] => format!("{first} and {second}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

/// A name with its article: "a Crucible", "an Airworks".
pub(super) fn article(name: &str) -> String {
    let name = entity_name(name);
    let vowel = name.starts_with(['A', 'E', 'I', 'O', 'U']);
    format!("{} {name}", if vowel { "an" } else { "a" })
}
