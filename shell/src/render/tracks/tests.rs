use super::*;
use crate::numeric;

fn idle_body(kind: UnitKind) -> Image {
    let sprites = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/sprites");
    let name = kind.name();
    let path = [
        format!("rig_{name}_hull"),
        format!("rig_{name}_body"),
        format!("rig_{name}_body_cargo0"),
        name.to_owned(),
    ]
    .into_iter()
    .map(|stem| sprites.join(format!("{stem}.png")))
    .find(|path| path.exists())
    .unwrap_or_else(|| panic!("{kind:?} has no body art"));
    Image::from_file_with_format(&std::fs::read(&path).unwrap(), Some(ImageFormat::Png)).unwrap()
}

#[test]
fn belts_stay_on_their_track_casings() {
    for kind in UnitKind::ALL.into_iter().filter(|&kind| supported(kind)) {
        let body = idle_body(kind);
        let width = usize::from(body.width);
        // Runs are in 128-px sprite space, which spans the whole canvas.
        let pixel = |at: f32| numeric::to_usize(at * f32::from(body.width) / 128.0);
        for &[x0, y0, x1, y1] in runs(kind) {
            for y in pixel(y0)..pixel(y1) {
                for x in pixel(x0)..pixel(x1) {
                    assert_eq!(
                        body.bytes[(y * width + x) * 4 + 3],
                        u8::MAX,
                        "{kind:?} belt leaves its casing at ({x}, {y})"
                    );
                }
            }
        }
    }
}
