use egui::{Color32, Mesh, Painter, Pos2, Vec2};
use std::f32::consts::{PI, TAU};

pub(super) fn arc(
    painter: &Painter,
    center: Pos2,
    radius: f32,
    range: [f32; 2],
    scale: f32,
    color: Color32,
    strength: f32,
) {
    if range[0] == range[1] {
        return;
    }

    let first = PI * (0.75 + 1.5 * range[0].min(range[1]));
    let last = PI * (0.75 + 1.5 * range[0].max(range[1]));
    let mut mesh = Mesh::default();

    for layer in (1..=10).rev() {
        let t = layer as f32 / 10.0;
        let half_width = (3.0 + 40.0 * t) * scale * 0.5;
        let tint = color.gamma_multiply(strength * (0.004 + 0.10 * (1.0 - t).powi(2)));
        let base = mesh.vertices.len() as u32;

        for i in 0..=64 {
            let angle = first + (last - first) * i as f32 / 64.0;
            let normal = Vec2::angled(angle);
            mesh.colored_vertex(center + normal * (radius - half_width), tint);
            mesh.colored_vertex(center + normal * (radius + half_width), tint);
            if i > 0 {
                let current = base + i * 2;
                mesh.add_triangle(current - 2, current - 1, current);
                mesh.add_triangle(current - 1, current + 1, current);
            }
        }

        // Each semicircle meets the strip at its diameter, without overlapping it.
        for (angle, offset) in [(first, PI), (last, 0.0)] {
            let end = center + Vec2::angled(angle) * radius;
            let base = mesh.vertices.len() as u32;
            mesh.colored_vertex(end, tint);
            for i in 0..=16 {
                let normal = Vec2::angled(angle + offset + TAU * 0.5 * i as f32 / 16.0);
                mesh.colored_vertex(end + normal * half_width, tint);
                if i > 0 {
                    mesh.add_triangle(base, base + i, base + i + 1);
                }
            }
        }
    }

    painter.add(egui::Shape::mesh(mesh));
}
