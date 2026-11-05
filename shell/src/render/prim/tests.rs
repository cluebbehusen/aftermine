use super::*;

/// Whether `v` falls on a device pixel boundary.
fn on_pixel(v: f32, dpi: f32) -> bool {
    ((v * dpi) - (v * dpi).round()).abs() < 1e-3
}

#[test]
fn every_edge_lands_on_whole_device_pixels_at_any_position() {
    for dpi in [1.0, 1.5, 2.0, 3.0] {
        for offset in [0.0, 0.25, 0.4, 0.5, 0.6, 0.75] {
            for thickness in [1.0, 1.5, 2.0, 3.0] {
                let rect = Rect::new(100.0 + offset, 50.0 + offset, 40.0, 40.0);
                let bands = outline_bands(rect, thickness, dpi)
                    .map(|band| band.expect("a box this size keeps its four edges"));
                for band in bands {
                    for v in [band.x, band.y, band.x + band.w, band.y + band.h] {
                        assert!(on_pixel(v, dpi), "{band:?} at dpi {dpi}");
                    }
                    assert!(
                        band.w.min(band.h) * dpi >= 1.0 - 1e-3,
                        "{band:?} is at least one device pixel"
                    );
                }
                // Top, bottom, left, right: each hugs its own side.
                let [top, bottom, left, right] = [bands[0], bands[1], bands[2], bands[3]];
                assert!((top.y - rect.y).abs() <= 0.5 / dpi + 1e-3);
                assert!((bottom.y + bottom.h - (rect.y + rect.h)).abs() <= 0.5 / dpi + 1e-3);
                assert!((left.x - rect.x).abs() <= 0.5 / dpi + 1e-3);
                assert!((right.x + right.w - (rect.x + rect.w)).abs() <= 0.5 / dpi + 1e-3);
            }
        }
    }
}

#[test]
fn edges_keep_half_the_stroke_as_macroquad_draws_it() {
    let bands = outline_bands(Rect::new(10.0, 10.0, 40.0, 40.0), 3.0, 2.0);
    assert!(bands.iter().flatten().all(|band| band.w.min(band.h) == 1.5));
    let hairline = outline_bands(Rect::new(10.0, 10.0, 40.0, 40.0), 1.0, 1.0);
    assert!(
        hairline
            .iter()
            .flatten()
            .all(|band| band.w.min(band.h) == 1.0)
    );
}

#[test]
fn a_box_too_small_for_its_edges_fills() {
    let bands = outline_bands(Rect::new(10.0, 10.0, 2.0, 2.0), 3.0, 1.0);
    assert_eq!(
        bands,
        [Some(Rect::new(10.0, 10.0, 2.0, 2.0)), None, None, None]
    );
}
