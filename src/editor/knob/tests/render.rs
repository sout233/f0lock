use super::super::*;

#[test]
fn disc_edges_have_partial_pixel_coverage_at_supported_scales() {
    for scale in [0.75, 1.0, 1.25, 1.5, 2.0] {
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(scale);
        let center = Pos2::new(75.37, 75.13);
        let radius = 30.0;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(200.0))),
                ..Default::default()
            },
            |ui| disc(ui.painter(), center, radius, Color32::WHITE, Color32::WHITE),
        );
        output.textures_delta.clear();
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        let scale = output.pixels_per_point;
        let mut partial_pixels = 0;

        for y in
            ((center.y - radius - 2.0) * scale) as i32..((center.y + radius + 2.0) * scale) as i32
        {
            for x in ((center.x - radius - 2.0) * scale) as i32
                ..((center.x + radius + 2.0) * scale) as i32
            {
                let point = Pos2::new(x as f32 + 0.5, y as f32 + 0.5) / scale;
                let mut coverage = 0.0_f32;

                for primitive in &primitives {
                    let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive else {
                        panic!("Unexpected paint callback");
                    };

                    for indices in mesh.indices.chunks_exact(3) {
                        let [a, b, c] = [indices[0], indices[1], indices[2]]
                            .map(|index| mesh.vertices[index as usize]);
                        let cross = |u: Vec2, v: Vec2| u.x * v.y - u.y * v.x;
                        let denominator = cross(b.pos - a.pos, c.pos - a.pos);

                        if denominator.abs() < 1e-8 {
                            continue;
                        }

                        let wb = cross(point - a.pos, c.pos - a.pos) / denominator;
                        let wc = cross(b.pos - a.pos, point - a.pos) / denominator;
                        let wa = 1.0 - wb - wc;

                        if wa >= -1e-6 && wb >= -1e-6 && wc >= -1e-6 {
                            coverage = coverage.max(
                                (wa * f32::from(a.color.a())
                                    + wb * f32::from(b.color.a())
                                    + wc * f32::from(c.color.a()))
                                    / 255.0,
                            );
                        }
                    }
                }

                if (0.02..0.98).contains(&coverage) {
                    partial_pixels += 1;
                }
            }
        }

        assert!(
            partial_pixels as f32 > radius * scale * 2.0,
            "Circle edge has insufficient antialiasing at {scale}x: {partial_pixels} partial pixels"
        );
    }
}
