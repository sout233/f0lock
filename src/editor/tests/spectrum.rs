use super::*;

#[test]
fn cut_slope_menu_is_conditional_and_edits_the_selected_banks_parameter() {
    use crate::filter::CutSlope;

    let mut f = Fixture::new();
    let has_text = |f: &Fixture, text: &str| {
        f.shapes
            .iter()
            .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text == text))
    };
    assert!(!has_text(&f, "12 dB/oct"));
    f.click_text("Peak");
    f.click_text("Low Cut");
    f.frame(vec![]);
    f.click_text("12 dB/oct");
    f.frame(vec![]);

    for slope in CutSlope::ALL {
        assert!(has_text(&f, &slope.label()), "Missing {}", slope.label());
    }

    f.host.events.lock().unwrap().clear();
    f.click_text("96 dB/oct");
    assert_eq!(f.editor.params.band(0).slope.value(), CutSlope::Db96);
    assert_eq!(
        f.editor.params.sidechain.band(0).slope.value(),
        CutSlope::Db12
    );
    assert_eq!(*f.host.events.lock().unwrap(), vec!["begin", "set", "end"]);

    f.click_text("Low Cut");
    f.click_text("High Cut");
    f.frame(vec![]);
    assert!(has_text(&f, "96 dB/oct"));
    f.click_text("96 dB/oct");
    f.click_text("6 dB/oct");
    f.click(Pos2::new(175.0, 465.0), egui::PointerButton::Secondary);
    f.frame(vec![]);
    assert!(
        !has_text(&f, "Apply"),
        "First-order cuts must disable the unused Q control"
    );
    f.click_text("High Cut");
    f.click_text("Peak");
    f.frame(vec![]);
    assert!(!has_text(&f, "96 dB/oct"));
}

#[test]
fn cut_response_leaves_the_plot_instead_of_flattening_at_the_floor() {
    let mut f = Fixture::new();
    let mut engine = crate::engine::Engine::new(48000.0);
    let mut controls = crate::engine::Controls::default();
    controls.bands[0].kind = FilterKind::Highpass;

    for i in 0..24000 {
        let x = 0.25 * (std::f32::consts::TAU * 440.0 * i as f32 / 48000.0).sin();
        engine.process([x; 2], controls);
    }

    f.editor.telemetry.publish(&engine);
    f.frame(vec![]);

    let (shape, path) = f
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Path(path) if path.stroke.width == 2.0 && path.points.len() == 480 => {
                Some((shape, path))
            }
            _ => None,
        })
        .expect("EQ response path missing");

    assert!(
        path.points[0].y > 250.0,
        "Response was flattened at the graph floor"
    );
    assert!((shape.clip_rect.bottom() - 250.0).abs() < 0.01);
}

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
