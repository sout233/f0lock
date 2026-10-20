use super::*;

#[test]
fn rendered_spectrum_reaches_right_edge_and_shows_8khz_peak() {
    let mut f = Fixture::new();
    let mut engine = crate::engine::Engine::new(48000.0);
    engine.visualize = true;

    for i in 0..24000 {
        let x = 0.25 * (std::f32::consts::TAU * 8000.0 * i as f32 / 48000.0).sin();
        engine.process([x; 2], crate::engine::Controls::default());
    }

    f.editor.telemetry.publish(&engine);
    f.frame(vec![]);

    let points = f.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Path(path) if path.stroke.width == 1.3
            && matches!(path.stroke.color, egui::epaint::ColorMode::Solid(c) if c == Color32::from_gray(128)) => Some(&path.points),
        _ => None,
    }).expect("Input spectrum path missing");

    assert!((points.first().unwrap().x - 57.0).abs() < 0.01);
    assert!(
        (points.last().unwrap().x - 843.0).abs() < 0.01,
        "Spectrum was truncated before the plot's right edge"
    );

    let eight_khz_x = 57.0 + (8000.0_f32 / 20.0).ln() / 1000.0_f32.ln() * 786.0;

    assert!(
        points
            .iter()
            .any(|p| (p.x - eight_khz_x).abs() < 6.0 && p.y < 135.0),
        "8 kHz input was not rendered at its actual frequency and level"
    );
}
