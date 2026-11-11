//! Guards the input funnel by reading the shell's own source: only the
//! hardware adapter in `input.rs` polls macroquad's keyboard, mouse and
//! touch state, so injected input reaches every screen the way the hardware
//! does.

use std::path::{Path, PathBuf};

/// Macroquad's polled keyboard, mouse and touch reads.
const HARDWARE_READS: &[&str] = &[
    "get_char_pressed",
    "get_keys_down",
    "get_keys_pressed",
    "get_keys_released",
    "get_last_key_pressed",
    "is_key_down",
    "is_key_pressed",
    "is_key_released",
    "is_mouse_button_down",
    "is_mouse_button_pressed",
    "is_mouse_button_released",
    "mouse_delta_position",
    "mouse_position",
    "mouse_position_local",
    "mouse_wheel",
    "touches",
    "touches_local",
];

/// Every production source file in the shell; unit tests are not.
fn production_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("source directory reads") {
        let path = entry.expect("directory entry reads").path();
        let name = path
            .file_name()
            .map_or("", |name| name.to_str().expect("source names are UTF-8"));
        if path.is_dir() {
            if name != "tests" {
                production_sources(&path, out);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && name != "tests.rs"
            && !name.ends_with("_tests.rs")
        {
            out.push(path);
        }
    }
}

/// Whether `code` calls the free function `name`, bare or by path, rather
/// than a method or a longer name that ends the same way.
fn calls(code: &str, name: &str) -> bool {
    code.match_indices(name).any(|(at, _)| {
        let before = &code[..at];
        let joined = before
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.');
        code[at + name.len()..].starts_with('(') && !joined && !before.trim_end().ends_with("fn")
    })
}

#[test]
fn only_the_hardware_adapter_polls_the_keyboard_mouse_and_touch() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let adapter = root.join("input.rs");
    let mut sources = Vec::new();
    production_sources(&root, &mut sources);
    sources.sort();
    let mut reads = Vec::new();
    for path in sources.iter().filter(|path| **path != adapter) {
        let source = std::fs::read_to_string(path).expect("source reads");
        for (line, text) in source.lines().enumerate() {
            let code = text.split("//").next().unwrap_or_default();
            for name in HARDWARE_READS.iter().filter(|name| calls(code, name)) {
                let file = path.strip_prefix(&root).expect("under src").display();
                reads.push(format!("{file}:{}: {name}", line + 1));
            }
        }
    }
    assert!(
        reads.is_empty(),
        "read input from the frame's events or the tracked pointer instead: {reads:#?}"
    );
}

#[test]
fn the_scan_recognizes_what_it_guards() {
    assert!(calls(
        "let at: Vec2 = mouse_position().into();",
        "mouse_position"
    ));
    assert!(calls("if mq::is_key_down(code) {", "is_key_down"));
    assert!(!calls("let held = self.touches();", "touches"));
    assert!(!calls("fn touches(&self) -> usize {", "touches"));
    assert!(!calls("last_mouse_position(event)", "mouse_position"));
    assert!(!calls("let mouse_position = point;", "mouse_position"));
}
