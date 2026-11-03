use super::*;

fn expander(f: &Fixture) -> Pos2 {
    Pos2::new(
        f.text_rect("Q").expect("Q label is visible").center().x + 36.0,
        422.0,
    )
}

#[test]
fn expander_stays_attached_to_q_when_the_layout_changes() {
    let mut f = Fixture::new();
    let mut centers = Vec::new();
    for _ in 0..2 {
        let q = f.text_rect("Q").expect("Q label is visible");
        let center = f
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Circle(circle) if circle.radius == 9.0 => Some(circle.center),
                _ => None,
            })
            .expect("Q Follow button is visible");
        assert!(
            (center.x - q.center().x - 36.0).abs() < 1.0,
            "button must follow Q horizontally: button={center:?}, Q={q:?}"
        );
        assert!(
            (center.y - 422.0).abs() < 1.0,
            "button must sit beside Q's outer ring: {center:?}"
        );
        centers.push(center);
        f.click(center, egui::PointerButton::Primary);
        f.frame(vec![]);
    }
    assert!((centers[1].x - centers[0].x - 10.0).abs() < 1.0);
}

fn knobs(f: &Fixture, expected: &[(&str, f32)]) {
    for (label, x) in expected {
        let rect = f
            .text_rect(label)
            .unwrap_or_else(|| panic!("missing knob label: {label}"));
        assert!(
            (rect.center().x - x).abs() < 1.0,
            "{label} sits at {} instead of {x}",
            rect.center().x
        );
    }
}

#[test]
fn expander_hides_and_restores_the_q_follow_knob() {
    let mut f = Fixture::new();
    knobs(
        &f,
        &[
            ("Gain", 55.0),
            ("Q", 165.0),
            ("Q Follow", 275.0),
            ("F0 Ratio", 385.0),
            ("Threshold", 505.0),
        ],
    );

    f.host.events.lock().unwrap().clear();
    f.click(expander(&f), egui::PointerButton::Primary);
    f.frame(vec![]);

    assert!(f.text_rect("Q Follow").is_none());
    knobs(
        &f,
        &[
            ("Gain", 65.0),
            ("Q", 175.0),
            ("F0 Ratio", 285.0),
            ("Threshold", 420.0),
            ("Dyn.Release", 810.0),
        ],
    );
    assert!(
        f.host.events.lock().unwrap().is_empty(),
        "the expander is a view toggle, it must not touch parameters"
    );

    f.click(expander(&f), egui::PointerButton::Primary);
    f.frame(vec![]);
    assert!(f.text_rect("Q Follow").is_some());
    knobs(
        &f,
        &[("Gain", 55.0), ("Threshold", 505.0), ("Dyn.Release", 835.0)],
    );
}

#[test]
fn collapsed_by_default_until_the_point_uses_q_follow() {
    let mut f = Fixture::new();
    f.editor.q_follow_open = [None; BAND_COUNT];
    f.frame(vec![]);
    assert!(f.text_rect("Q Follow").is_none());

    {
        let setter = f.editor.gui.as_ref().unwrap().param_setter();
        set_value(&setter, f.editor.params.band(0).q_follow, 1.0);
    }
    f.frame(vec![]);
    assert!(
        f.text_rect("Q Follow").is_some(),
        "a non-zero Q Follow value must open the knob without a click"
    );

    f.frame(vec![]);
    f.click(expander(&f), egui::PointerButton::Primary);
    f.frame(vec![]);
    assert!(f.text_rect("Q Follow").is_none());
}

#[test]
fn expander_clears_every_label_in_both_layouts() {
    for expanded in [Some(true), Some(false)] {
        let mut f = Fixture::new();
        f.editor.q_follow_open = [expanded; BAND_COUNT];
        f.frame(vec![]);

        let hit = Rect::from_center_size(expander(&f), Vec2::splat(26.0));
        for shape in &f.shapes {
            if let egui::Shape::Text(text) = &shape.shape {
                let rect = shape.shape.visual_bounding_rect();
                assert!(
                    !rect.intersects(hit),
                    "\"{}\" at {rect:?} collides with the expander at {hit:?} (open={expanded:?})",
                    text.galley.job.text
                );
            }
        }
    }
}

fn divider(f: &Fixture) -> f32 {
    f.shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::LineSegment { points, .. }
                if (points[0].x - points[1].x).abs() < 0.01
                    && ((points[0].y - points[1].y).abs() - 180.0).abs() < 2.0 =>
            {
                Some(points[0].x)
            }
            _ => None,
        })
        .expect("the EQ column divider is missing")
}

/// The EQ divider and the Dynamics header have to move with the strip. When the
/// fourth knob is hidden the row is the original 350 px one, so a divider left
/// at 450 would cut straight through the Threshold knob and its label.
#[test]
fn header_follows_the_eq_column_width() {
    for (open, expected) in [(true, 450.0), (false, 350.0)] {
        let mut f = Fixture::new();
        f.editor.q_follow_open = [Some(open); BAND_COUNT];
        f.frame(vec![]);

        let line = divider(&f);
        assert!(
            (line - expected).abs() < 0.5,
            "divider at {line} instead of {expected} (open={open})"
        );

        for shape in &f.shapes {
            let egui::Shape::Text(text) = &shape.shape else {
                continue;
            };
            let rect = shape.shape.visual_bounding_rect();
            if rect.center().y < 360.0 || rect.center().y > 545.0 {
                continue;
            }
            assert!(
                rect.left() > line - 1.0 || rect.right() < line + 1.0,
                "\"{}\" at {rect:?} straddles the divider at {line} (open={open})",
                text.galley.job.text
            );
        }
    }
}
