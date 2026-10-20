use super::*;
#[test]
fn live_body_frames_have_cached_production_contact_sources() {
    let manifest: HashMap<String, [f32; 4]> =
        serde_json::from_str(include_str!("../../../assets/sprites/atlas.json")).unwrap();
    let sources = contact_sources(&manifest);
    let loaded_sources = entity_sources(&manifest);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/sprites");
    let bytes = std::fs::read(root.join("atlas.png")).unwrap();
    let first = Image::from_file_with_format(&bytes, Some(ImageFormat::Png)).unwrap();
    let page_height = u32::from(first.height);
    let mut pages = vec![first];
    // Every idle building body and rig base too: a hit on one lands on its art.
    for name in [
        "rig_harvester_body_cargo5",
        "rig_excavator_body_move2",
        "rig_excavator_body_move1",
        "rig_scuttler_body_move2",
        "rig_buzzard_hull_move2",
        "bombard_action2",
        "rig_spire_foundation",
        "rig_lattice_base",
    ]
    .into_iter()
    .chain(
        oxide_sim::BuildingKind::ALL
            .iter()
            .map(|&kind| crate::assets::building_stem(kind)),
    ) {
        let key = manifest[name].map(numeric::to_u32);
        assert!(sources.contains(&key), "uncached live body frame: {name}");
        assert!(
            loaded_sources.contains(&key),
            "body frame omitted from load: {name}"
        );
        let page = (key[1] / page_height) as usize;
        while pages.len() <= page {
            let bytes = std::fs::read(root.join(format!("atlas_{}.png", pages.len()))).unwrap();
            pages.push(Image::from_file_with_format(&bytes, Some(ImageFormat::Png)).unwrap());
        }
        let mask = crate::sprite_contact::SpriteContact::capture(
            &pages[page],
            [key[0], key[1] % page_height, key[2], key[3]],
        );
        assert!(
            mask.contact(vec2(0.0, -2.0), Vec2::ZERO, Vec2::splat(-0.5), Vec2::ONE)
                .is_some(),
            "empty body mask: {name}"
        );
    }
}

#[test]
fn layered_units_omit_unused_full_poses_but_keep_portraits_and_fallbacks() {
    let mut manifest = HashMap::from([
        ("buzzard".to_owned(), [0.0, 0.0, 128.0, 128.0]),
        ("buzzard_move1".to_owned(), [128.0, 0.0, 128.0, 128.0]),
        ("buzzard_action2".to_owned(), [256.0, 0.0, 128.0, 128.0]),
        (
            "buzzard_accent_move1".to_owned(),
            [640.0, 0.0, 128.0, 128.0],
        ),
    ]);
    assert_eq!(entity_sources(&manifest).len(), 4);
    manifest.insert("rig_buzzard_hull".to_owned(), [384.0, 0.0, 128.0, 128.0]);
    manifest.insert(
        "rig_buzzard_mount_action2".to_owned(),
        [512.0, 0.0, 128.0, 128.0],
    );
    assert_eq!(
        entity_sources(&manifest),
        BTreeSet::from([[0, 0, 128, 128], [384, 0, 128, 128], [512, 0, 128, 128],])
    );
}

/// The production atlas pages and their height.
fn atlas_pages() -> (Vec<Image>, u32) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/sprites");
    let mut pages = Vec::new();
    for index in 0.. {
        let name = if index == 0 {
            "atlas.png".to_owned()
        } else {
            format!("atlas_{index}.png")
        };
        let Ok(bytes) = std::fs::read(root.join(name)) else {
            break;
        };
        pages.push(Image::from_file_with_format(&bytes, Some(ImageFormat::Png)).unwrap());
    }
    let height = u32::from(pages[0].height);
    (pages, height)
}

/// Packs every production entity sprite's levels as loading does, with
/// the region each level takes.
fn pack_production() -> (Packer, Vec<(Source, usize, Region)>) {
    let manifest: HashMap<String, [f32; 4]> =
        serde_json::from_str(include_str!("../../../assets/sprites/atlas.json")).unwrap();
    let (pages, height) = atlas_pages();
    let areas: BTreeMap<_, _> = entity_sources(&manifest)
        .into_iter()
        .map(|key| {
            let local = [key[0], key[1] % height, key[2], key[3]];
            (key, packed_area(&pages[(key[1] / height) as usize], local))
        })
        .collect();
    let (packer, regions) = pack(&areas, &unit_sources(&manifest), |_, area, level| {
        let factor = LEVELS[level].fit::<u32>();
        Image::gen_image_color(
            (area[2] / factor).fit::<u16>(),
            (area[3] / factor).fit::<u16>(),
            WHITE,
        )
    });
    assert_eq!(regions.len(), areas.len() * LEVELS.len());
    (packer, regions)
}

#[test]
fn production_mips_fit_seven_pages_without_discarding_levels() {
    let (packer, regions) = pack_production();
    for (_, _, region) in regions {
        assert!(region.rect.right() < PAGE as f32);
        assert!(region.rect.bottom() < PAGE as f32);
    }
    assert!(
        packer.images.len() <= 7,
        "{} mip pages exceed the seven-page budget",
        packer.images.len()
    );
}

#[test]
fn each_coarse_level_of_a_pass_keeps_to_one_page() {
    let manifest: HashMap<String, [f32; 4]> =
        serde_json::from_str(include_str!("../../../assets/sprites/atlas.json")).unwrap();
    let units = unit_sources(&manifest);
    let (_, regions) = pack_production();
    // A draw samples two adjacent levels; with each coarse level on one
    // page, every sprite a pass draws at one zoom shares its batch.
    for unit in [false, true] {
        for level in 1..LEVELS.len() {
            let pages: BTreeSet<usize> = regions
                .iter()
                .filter(|(key, packed, _)| *packed == level && units.contains(key) == unit)
                .map(|(_, _, region)| region.page)
                .collect();
            assert_eq!(pages.len(), 1, "units {unit} level {level} spans {pages:?}");
        }
    }
}

#[test]
fn cradles_pack_with_the_works_and_rigged_hulls_with_the_units() {
    assert!(!is_unit_source("maw_cradle"));
    assert!(!is_unit_source("rig_maw_cradle_frame"));
    assert!(!is_unit_source("roc_cradle_accent"));
    assert!(is_unit_source("maw"));
    assert!(is_unit_source("rig_roc_hull"));
    assert!(!is_unit_source("turret"));
}

#[test]
fn mip_vertex_data_preserves_flip_pivot_rotation_and_tint() {
    let params = DrawTextureParams {
        dest_size: Some(vec2(40.0, 20.0)),
        flip_x: true,
        flip_y: true,
        pivot: Some(vec2(10.0, 20.0)),
        rotation: std::f32::consts::FRAC_PI_2,
        ..Default::default()
    };
    let a = Rect::new(128.0, 256.0, 64.0, 32.0);
    let b = Rect::new(64.0, 512.0, 32.0, 16.0);
    let tint = Color::new(0.3, 0.6, 0.9, 0.4);
    let whole = Rect::new(0.0, 0.0, 1.0, 1.0);
    let vertices = sprite_vertices(vec2(10.0, 20.0), tint, &params, whole, [a, b], 0.6);
    for (vertex, expected) in vertices.iter().zip([
        vec2(-10.0, 60.0),
        vec2(-10.0, 20.0),
        vec2(10.0, 20.0),
        vec2(10.0, 60.0),
    ]) {
        assert!(vertex.position.truncate().distance(expected) < 0.0001);
        assert_eq!(vertex.color, <[u8; 4]>::from(tint));
        assert_eq!(vertex.normal.z, 0.6);
        assert_eq!(vertex.normal.w, 1.0);
    }
    assert_eq!(vertices[0].uv, vec2(128.0, 256.0) / PAGE as f32);
    assert_eq!(vertices[2].uv, vec2(192.0, 288.0) / PAGE as f32);
    assert_eq!(
        vertices[0].normal.truncate().truncate(),
        vec2(64.0, 512.0) / PAGE as f32
    );
    assert_eq!(
        vertices[2].normal.truncate().truncate(),
        vec2(96.0, 528.0) / PAGE as f32
    );
}

#[test]
fn mip_quad_rotates_about_the_destination_center_by_default() {
    let params = DrawTextureParams {
        dest_size: Some(vec2(40.0, 20.0)),
        rotation: std::f32::consts::PI,
        ..Default::default()
    };
    let source = Rect::new(0.0, 0.0, 64.0, 64.0);
    let whole = Rect::new(0.0, 0.0, 1.0, 1.0);
    let vertices = sprite_vertices(
        vec2(10.0, 20.0),
        WHITE,
        &params,
        whole,
        [source, source],
        0.0,
    );
    assert!(vertices[0].position.truncate().distance(vec2(50.0, 40.0)) < 0.0001);
    assert!(vertices[2].position.truncate().distance(vec2(10.0, 20.0)) < 0.0001);
}

#[test]
fn reduction_keeps_color_premultiplied_through_transparent_edges() {
    let image = Image {
        width: 2,
        height: 2,
        bytes: vec![255, 80, 10, 255, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0],
    };
    assert_eq!(reduce(&image, [0, 0, 2, 2], 2).bytes, [63, 20, 2, 63]);
    let full = reduce(&image, [0, 0, 2, 2], 1);
    assert_eq!(&full.bytes[4..8], &[0, 0, 0, 0]);
}
#[test]
fn physical_size_and_both_axes_select_detail_biased_levels() {
    let (a, b, blend) = lod_mix(vec2(128.0, 128.0), vec2(32.0, 32.0));
    assert_eq!((a, b), (1, 2));
    assert!((blend - 0.6).abs() < 0.0001);
    assert_eq!(lod_mix(vec2(128.0, 64.0), vec2(128.0, 16.0)), (a, b, blend),);
    let (a, b, blend) = lod_mix(vec2(128.0, 128.0), vec2(64.0, 64.0));
    assert_eq!((a, b), (0, 1));
    assert!((blend - 0.6).abs() < 0.0001);
    assert_eq!(lod_mix(vec2(128.0, 128.0), vec2(256.0, 256.0)), (0, 0, 0.0));
}
#[test]
fn levels_change_continuously_at_every_boundary() {
    for width in 8000..129_000 {
        let weights = |w| {
            let (a, b, t) = lod_mix(vec2(128.0, 128.0), vec2(w, w));
            let mut out = [0.0; 4];
            out[a] += 1.0 - t;
            out[b] += t;
            out
        };
        let a = weights(width as f32 / 1000.0);
        let b = weights((width + 1) as f32 / 1000.0);
        for (a, b) in a.into_iter().zip(b) {
            assert!((a - b).abs() < 0.001);
        }
    }
}
#[test]
fn building_layers_and_resources_enter_the_bank_but_terrain_does_not() {
    for key in [
        "foundry",
        "flak_turret",
        "flak_mount_accent",
        "rig_array_t1_rotor",
        "repair_bay",
        "scuttle_charge_accent",
        "scrap_full",
        "wreck_pile",
        "harvester_cargo0",
    ] {
        assert!(is_entity_source(key), "{key}");
    }
    for key in ["ground_0", "rock_slab_0_0", "scorch"] {
        assert!(!is_entity_source(key));
    }
}
#[test]
fn packed_borders_do_not_import_adjacent_sprites() {
    let mut packer = Packer::new();
    let red = packer.insert(&Image::gen_image_color(2, 2, RED));
    packer.insert(&Image::gen_image_color(2, 2, BLUE));
    for y in numeric::to_u32(red.rect.y) - 1..=numeric::to_u32(red.rect.y) + 2 {
        for x in numeric::to_u32(red.rect.x) - 1..=numeric::to_u32(red.rect.x) + 2 {
            assert_eq!(
                packer.images[red.page].get_pixel(x, y),
                packer.images[red.page].get_pixel(2, 2)
            );
        }
    }
    packer.x = PAGE - 2;
    packer.y = PAGE - 2;
    assert_eq!(packer.insert(&Image::gen_image_color(2, 2, GREEN)).page, 1);
}

/// A 64-pixel sprite whose only painted texels span `x0..x1`, `y0..y1`.
fn painted(x0: u32, x1: u32, y0: u32, y1: u32) -> Image {
    let mut image = Image::gen_image_color(64, 64, BLANK);
    for y in y0..y1 {
        for x in x0..x1 {
            image.set_pixel(x, y, Color::new(0.8, 0.4, 0.2, 0.5));
        }
    }
    image
}

#[test]
fn a_packed_area_keeps_a_transparent_coarse_texel_around_the_paint_within_the_sprite() {
    assert_eq!(
        packed_area(&painted(20, 30, 33, 40), [0, 0, 64, 64]),
        [8, 24, 32, 24]
    );
    // Paint at the sprite's edge keeps that edge, as the whole sprite does.
    assert_eq!(
        packed_area(&painted(0, 5, 60, 64), [0, 0, 64, 64]),
        [0, 48, 16, 16]
    );
    assert_eq!(
        packed_area(&painted(0, 0, 0, 0), [0, 0, 64, 64]),
        [0, 0, 8, 8]
    );
    // Any alpha counts, however faint.
    let mut faint = painted(0, 0, 0, 0);
    faint.set_pixel(40, 9, Color::from_rgba(255, 255, 255, 1));
    assert_eq!(packed_area(&faint, [0, 0, 64, 64]), [32, 0, 24, 24]);
}

#[test]
fn every_level_of_a_packed_area_matches_that_part_of_the_whole_sprite() {
    let image = painted(20, 30, 33, 40);
    let area = packed_area(&image, [0, 0, 64, 64]);
    for factor in LEVELS {
        let whole = reduce(&image, [0, 0, 64, 64], factor);
        let part = reduce(&image, area, factor);
        let f = factor.fit::<u32>();
        for y in 0..u32::from(part.height) {
            for x in 0..u32::from(part.width) {
                assert_eq!(
                    part.get_pixel(x, y),
                    whole.get_pixel(area[0] / f + x, area[1] / f + y)
                );
            }
        }
        // The area's edge texels are transparent at every level, so linear
        // sampling there blends with nothing, as it does in the whole sprite.
        let (w, h) = (u32::from(part.width), u32::from(part.height));
        for x in 0..w {
            assert_eq!(part.get_pixel(x, 0).a, 0.0);
            assert_eq!(part.get_pixel(x, h - 1).a, 0.0);
        }
        for y in 0..h {
            assert_eq!(part.get_pixel(0, y).a, 0.0);
            assert_eq!(part.get_pixel(w - 1, y).a, 0.0);
        }
    }
}

#[test]
fn a_packed_area_draws_where_it_lies_in_the_whole_flipped_and_turned_sprite() {
    let params = DrawTextureParams {
        dest_size: Some(vec2(40.0, 20.0)),
        flip_x: true,
        rotation: std::f32::consts::PI,
        ..Default::default()
    };
    let region = Rect::new(0.0, 0.0, 16.0, 16.0);
    // The area's left quarter, upper half: flipped, it covers the right
    // quarter; turned a half turn about the whole quad's centre, the left
    // quarter's lower half.
    let area = Rect::new(0.0, 0.0, 0.25, 0.5);
    let vertices = sprite_vertices(
        vec2(10.0, 20.0),
        WHITE,
        &params,
        area,
        [region, region],
        0.0,
    );
    for (vertex, expected) in vertices.iter().zip([
        vec2(10.0, 40.0),
        vec2(20.0, 40.0),
        vec2(20.0, 30.0),
        vec2(10.0, 30.0),
    ]) {
        assert!(
            vertex.position.truncate().distance(expected) < 0.0001,
            "{vertex:?} {expected}"
        );
    }
}
