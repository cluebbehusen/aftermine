use super::*;

fn positions(ink: &Ink) -> Vec<[f32; 2]> {
    ink.mesh
        .vertices
        .iter()
        .map(|vertex| [vertex.position.x, vertex.position.y])
        .collect()
}

#[test]
fn a_poly_queues_what_draw_poly_would() {
    let mut ink = Ink::default();
    ink.rect(0.0, 0.0, 1.0, 1.0, WHITE);
    let (x, y, sides, radius, rotation) = (10.5, 20.25, 8_u8, 13.0, 22.5_f32);
    ink.poly(x, y, sides, radius, rotation, WHITE);
    let rot = rotation.to_radians();
    let mut expected = vec![[x, y]];
    let mut indices = Vec::new();
    for i in 0..=sides {
        let rx = (f32::from(i) / f32::from(sides) * std::f32::consts::PI * 2. + rot).cos();
        let ry = (f32::from(i) / f32::from(sides) * std::f32::consts::PI * 2. + rot).sin();
        expected.push([x + radius * rx, y + radius * ry]);
        if i != sides {
            indices.extend([4, u16::from(i) + 5, u16::from(i) + 6]);
        }
    }
    assert_eq!(positions(&ink)[4..], expected);
    assert_eq!(ink.mesh.indices[6..], indices);
}

#[test]
fn a_ring_queues_what_draw_arc_would() {
    let mut ink = Ink::default();
    let (x, y, radius, thickness) = (3.0, 4.0, 12.0, 1.3);
    ink.circle_lines(vec2(x, y), radius, thickness, WHITE);
    let part = 360.0_f32.to_radians();
    let sides = (30.0 * part / std::f32::consts::TAU).ceil().max(1.0);
    let span = part / sides;
    let mut expected = Vec::new();
    for i in 0..crate::numeric::to_usize(sides) {
        let start = i as f32 * span;
        let end = start + span;
        for (angle, radius) in [
            (start, radius),
            (start, radius + thickness),
            (end, radius),
            (end, radius + thickness),
        ] {
            let point = Vec2::new(x, y) + radius * Vec2::from_angle(angle);
            expected.push([point.x, point.y]);
        }
    }
    assert_eq!(positions(&ink), expected);
    assert_eq!(ink.mesh.indices.len(), crate::numeric::to_usize(sides) * 6);
    assert_eq!(ink.mesh.indices[6..12], [4, 5, 6, 6, 5, 7]);
}
