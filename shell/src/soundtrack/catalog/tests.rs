use super::*;

fn bundled() -> (Catalog, std::path::PathBuf) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/music");
    let json = std::fs::read_to_string(root.join("manifest.json")).expect("music manifest");
    (Catalog::parse(&json).expect("a valid manifest"), root)
}

#[test]
fn the_bundled_music_names_every_piece_and_file() {
    let (catalog, root) = bundled();
    assert_eq!(catalog.songs().len(), 4, "four match songs take turns");
    for piece in &catalog.pieces {
        for section in &piece.sections {
            for (_, file) in section.layers() {
                assert!(root.join(file).is_file(), "{file} is missing");
            }
        }
    }
}

#[test]
fn every_file_decodes_to_the_length_its_manifest_gives() {
    let (catalog, root) = bundled();
    for section in catalog.pieces.iter().flat_map(|piece| &piece.sections) {
        for (_, file) in section.layers() {
            let bytes = std::fs::read(root.join(file)).expect("music file");
            let frames = crate::audio_engine::decode_ogg(&bytes).expect("decodes");
            assert_eq!(
                frames.len() as u64,
                section.frames,
                "{file} must decode to its whole slot, so sections join on time"
            );
            assert!(
                frames.iter().any(|frame| frame.left.abs() > 0.01),
                "{file} decodes to sound"
            );
        }
    }
}

fn manifest(pieces: &str) -> String {
    format!(r#"{{"sample_rate": 44100, "pieces": [{pieces}]}}"#)
}

fn section(kind: &str, layers: &str) -> String {
    format!(
        r#"{{"name": "{kind}", "kind": "{kind}", "body": 10, "frames": 20, "files": {layers}}}"#
    )
}

fn solo(id: &str, role: &str, kinds: &[&str]) -> String {
    let sections: Vec<String> = kinds
        .iter()
        .map(|kind| section(kind, r#"{"main": "x.ogg"}"#))
        .collect();
    format!(
        r#"{{"id": "{id}", "role": "{role}", "sections": [{}]}}"#,
        sections.join(",")
    )
}

fn song(kinds: &[&str]) -> String {
    let sections: Vec<String> = kinds
        .iter()
        .map(|kind| section(kind, r#"{"calm": "c.ogg", "fight": "f.ogg"}"#))
        .collect();
    format!(
        r#"{{"id": "song", "role": "match", "sections": [{}]}}"#,
        sections.join(",")
    )
}

fn others() -> String {
    [
        solo("menu", "menu", &["M1", "M2"]),
        solo("victory", "victory", &["I", "L"]),
        solo("defeat", "defeat", &["I", "L"]),
        solo("draw", "draw", &["I", "L"]),
    ]
    .join(",")
}

#[test]
fn a_manifest_the_score_cannot_follow_is_refused() {
    let good = song(&["I", "A", "B", "C", "S", "F"]);
    assert!(Catalog::parse(&manifest(&format!("{good},{}", others()))).is_ok());
    for (broken, why) in [
        (song(&["A", "B", "C", "S", "F"]), "intro"),
        (song(&["I", "A", "B", "S", "F"]), "lift"),
        (song(&["I", "A", "B", "C", "F"]), "siege"),
        (song(&["I", "C", "S", "F"]), "two kinds"),
    ] {
        let json = manifest(&format!("{broken},{}", others()));
        let error = Catalog::parse(&json).expect_err(why).to_string();
        assert!(error.contains(why), "{error}");
    }
    let no_menu = manifest(&format!(
        "{good},{},{},{}",
        solo("victory", "victory", &["I", "L"]),
        solo("defeat", "defeat", &["I", "L"]),
        solo("draw", "draw", &["I", "L"]),
    ));
    assert!(Catalog::parse(&no_menu).is_err());
    let wrong_layers = manifest(&format!(
        "{},{}",
        good.replace(r#""fight": "f.ogg""#, r#""main": "f.ogg""#),
        others()
    ));
    assert!(Catalog::parse(&wrong_layers).is_err());
    let no_tail = manifest(&format!(
        "{},{}",
        good.replace(r#""frames": 20"#, r#""frames": 10"#),
        others()
    ));
    assert!(Catalog::parse(&no_tail).is_err());
}
