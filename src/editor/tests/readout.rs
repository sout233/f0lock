use super::*;

/// The EQ frequency readout: the only 11 pt monospace "… Hz" text in the
/// control strip.
fn eq_readout(f: &Fixture) -> egui::Rect {
    f.shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(t)
                if t.galley.job.text.ends_with(" Hz")
                    && t.galley.job.sections[0].format.font_id.size == 11.0 =>
            {
                Some(shape.shape.visual_bounding_rect())
            }
            _ => None,
        })
        .find(|rect| (360.0..410.0).contains(&rect.center().y))
        .expect("EQ frequency readout was not rendered")
}

#[test]
fn eq_frequency_readout_is_right_aligned_on_the_column_divider() {
    let mut f = Fixture::new();
    let narrow = eq_readout(&f);
    // Divider sits at x = 450; the readout keeps a 10 px margin.
    assert!(
        (narrow.right() - 440.0).abs() < 1.5,
        "readout right edge is {} instead of 440",
        narrow.right()
    );

    let setter = f.editor.gui.as_ref().unwrap().param_setter();
    set_value(&setter, f.editor.params.band(0).ratio, 8.0);
    f.frame(vec![]);

    let wide = eq_readout(&f);
    assert!(
        wide.width() > narrow.width() + 1.0,
        "test needs a readout with one more digit: {} vs {}",
        wide.width(),
        narrow.width()
    );
    assert!(
        (wide.right() - narrow.right()).abs() < 0.5,
        "longer readout moved its right edge from {} to {}",
        narrow.right(),
        wide.right()
    );
}
