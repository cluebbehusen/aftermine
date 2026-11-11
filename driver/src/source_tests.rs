//! Test code lives in its own files, which cargo-llvm-cov's default filter
//! skips, so the coverage floors measure production code alone.

use std::path::{Path, PathBuf};

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("readable source directory") {
        let path = entry.expect("readable directory entry").path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Whether cargo-llvm-cov's default filename filter skips `path`, given
/// relative to the workspace root.
fn coverage_skips(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    name == "tests.rs"
        || name.ends_with("_tests.rs")
        || name.ends_with("-tests.rs")
        || path.parent().is_some_and(|dir| {
            dir.components().any(|part| {
                matches!(
                    part.as_os_str().to_str(),
                    Some("tests" | "examples" | "benches")
                )
            })
        })
}

/// Test-only items a production file keeps on purpose, by file and the start
/// of the item: seams a test reaches inside a production type or function,
/// which a test file cannot hold. Imports need no entry; coverage counts no
/// line of them.
const SEAMS: &[(&str, &str)] = &[
    // Counts the ground routes the salvage danger cache searches.
    ("sim/src/vision.rs", "route_searches: Cell<usize>,"),
    ("sim/src/vision.rs", "route_searches: Cell::new(0),"),
    ("sim/src/vision.rs", "self.route_searches.set("),
    // Apple's touch-surface swipe, compiled on other hosts for its tests.
    ("shell/src/pad.rs", "Swipe {"),
    ("shell/src/pad.rs", "Reading::Swipe {"),
];

/// The file that `mod name;` in `file` loads, honoring a `#[path]` override.
fn module_file(file: &Path, name: &str, path_override: Option<&str>) -> PathBuf {
    let dir = file.parent().expect("a source file sits in a directory");
    if let Some(path) = path_override {
        return dir.join(path);
    }
    let stem = file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let base = if matches!(stem, "lib" | "main" | "mod") {
        dir.to_path_buf()
    } else {
        dir.join(stem)
    };
    let flat = base.join(format!("{name}.rs"));
    if flat.exists() {
        flat
    } else {
        base.join(name).join("mod.rs")
    }
}

/// Whether a `#[cfg(...)]` attribute compiles its item for tests, alone or
/// combined with other predicates; anything under `not(...)` does not count.
fn cfg_mentions_test(attribute: &str) -> bool {
    let Some(predicate) = attribute
        .strip_prefix("#[cfg(")
        .and_then(|rest| rest.strip_suffix(")]"))
    else {
        return false;
    };
    let mut kept = String::new();
    let mut in_string = false;
    let mut negated_depth = None;
    let mut depth = 0usize;
    for (at, ch) in predicate.char_indices() {
        if in_string {
            in_string = ch != '"';
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                if negated_depth == Some(depth) {
                    negated_depth = None;
                }
                depth -= 1;
            }
            _ if negated_depth.is_none() && predicate[at..].starts_with("not(") => {
                negated_depth = Some(depth + 1);
            }
            _ => {}
        }
        if negated_depth.is_none() {
            kept.push(ch);
        }
    }
    kept.split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .any(|word| word == "test")
}

/// The test-only code in `file` that coverage would count: inline test
/// modules, out-of-line ones whose file the filter keeps, and, in a file the
/// filter keeps, any other test-only item outside [`SEAMS`].
fn counted_test_code(root: &Path, file: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(file).expect("readable source file");
    let lines: Vec<&str> = text.lines().collect();
    let shown = file.strip_prefix(root).unwrap_or(file);
    let production = !coverage_skips(shown);
    let mut found = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let start = index;
        index += 1;
        if !lines[start].trim().starts_with("#[cfg(") {
            continue;
        }
        // rustfmt splits a long attribute across lines.
        let mut attribute = lines[start].trim().to_owned();
        let mut item = start + 1;
        while attribute.matches('[').count() > attribute.matches(']').count() {
            let Some(next) = lines.get(item) else { break };
            attribute.push_str(next.trim());
            item += 1;
        }
        if !cfg_mentions_test(&attribute) {
            continue;
        }
        let mut path_override = None;
        while let Some(next) = lines.get(item).map(|line| line.trim()) {
            if let Some(path) = next
                .strip_prefix("#[path = \"")
                .and_then(|rest| rest.strip_suffix("\"]"))
            {
                path_override = Some(path);
            } else if !next.starts_with("#[") && !next.starts_with("//") {
                break;
            }
            item += 1;
        }
        let Some(declaration) = lines.get(item).map(|line| line.trim()) else {
            continue;
        };
        let declaration = match declaration.strip_prefix("pub(") {
            Some(rest) => rest.split_once(") ").map_or(declaration, |(_, item)| item),
            None => declaration.strip_prefix("pub ").unwrap_or(declaration),
        };
        let Some(module) = declaration.strip_prefix("mod ") else {
            let seam = SEAMS
                .iter()
                .any(|(path, start)| Path::new(path) == shown && declaration.starts_with(start));
            if production && !declaration.starts_with("use ") && !seam {
                found.push(format!(
                    "{}:{}: test-only `{declaration}`",
                    shown.display(),
                    item + 1
                ));
            }
            continue;
        };
        if let Some(name) = module.strip_suffix(" {") {
            found.push(format!(
                "{}:{}: inline `mod {name}`",
                shown.display(),
                item + 1
            ));
        } else if let Some(name) = module.strip_suffix(';') {
            let loaded = module_file(file, name, path_override);
            let relative = loaded.strip_prefix(root).unwrap_or(&loaded);
            if !coverage_skips(relative) {
                found.push(format!(
                    "{}:{}: `mod {name}` loads {}, which coverage counts",
                    shown.display(),
                    item + 1,
                    relative.display()
                ));
            }
        }
    }
    found
}

#[test]
fn test_code_lives_in_files_that_coverage_skips() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the driver sits in the workspace");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root).expect("readable workspace root") {
        let member = entry.expect("readable workspace entry").path();
        if member.join("Cargo.toml").is_file() && member.join("src").is_dir() {
            sources(&member.join("src"), &mut files);
        }
    }
    sources(&root.join("build-support"), &mut files);
    files.sort();
    let found: Vec<String> = files
        .iter()
        .flat_map(|file| counted_test_code(root, file))
        .collect();
    assert!(
        found.is_empty(),
        "move each test module and test-only item into a child `tests.rs` file, \
         or list a real seam in SEAMS:\n{}",
        found.join("\n")
    );
}

#[test]
fn compound_test_cfgs_count_and_negated_ones_do_not() {
    assert!(cfg_mentions_test("#[cfg(test)]"));
    assert!(cfg_mentions_test(
        r#"#[cfg(all(test, target_os = "linux"))]"#
    ));
    assert!(cfg_mentions_test(
        r#"#[cfg(any(test, feature = "fixtures"))]"#
    ));
    assert!(cfg_mentions_test("#[cfg(all(test,unix))]"));
    assert!(!cfg_mentions_test("#[cfg(not(test))]"));
    assert!(!cfg_mentions_test("#[cfg(all(unix, not(test)))]"));
    assert!(!cfg_mentions_test(r#"#[cfg(feature = "test")]"#));
    assert!(!cfg_mentions_test("#[cfg(unix)]"));
}

#[test]
fn a_test_only_item_counts_in_a_production_file_and_not_in_a_test_file() {
    let root = std::env::temp_dir().join(format!("aftermine-source-guard-{}", std::process::id()));
    let src = root.join("member").join("src");
    std::fs::create_dir_all(&src).unwrap();
    let production = src.join("lib.rs");
    std::fs::write(
        &production,
        "#[cfg(test)]\nuse std::fmt;\n\n#[cfg(test)]\npub(crate) fn helper() {}\n\n#[cfg(test)]\nmod tests;\n",
    )
    .unwrap();
    let tests = src.join("tests.rs");
    std::fs::write(&tests, "#[cfg(test)]\nfn fixture() {}\n").unwrap();
    let found = counted_test_code(&root, &production);
    let in_tests = counted_test_code(&root, &tests);
    std::fs::remove_dir_all(&root).ok();
    let shown = Path::new("member").join("src").join("lib.rs");
    assert_eq!(
        found,
        [format!(
            "{}:5: test-only `fn helper() {{}}`",
            shown.display()
        )]
    );
    assert!(in_tests.is_empty(), "{in_tests:?}");
}
