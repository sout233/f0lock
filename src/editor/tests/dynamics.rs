use super::*;

#[test]
fn dynamics_indicator_and_label_toggle_the_same_parameter() {
    let mut f = Fixture::new();
    f.host.events.lock().unwrap().clear();
    f.click(Pos2::new(383.0, 373.0), egui::PointerButton::Primary);
    assert!(f.editor.params.band(0).dynamics.enabled.value());
    f.click_text("Dynamics");
    assert!(!f.editor.params.band(0).dynamics.enabled.value());
    assert_eq!(
        *f.host.events.lock().unwrap(),
        vec!["begin", "set", "end", "begin", "set", "end"]
    );
}

#[test]
fn dynamics_switch_preserves_all_controls_and_edits_only_the_selected_point() {
    let mut f = Fixture::new();
    f.add();
    f.frame(vec![]);
    let labels = [
        "Gain",
        "Q",
        "F0 Ratio",
        "Threshold",
        "Range",
        "Dyn.Attack",
        "Dyn.Release",
    ];
    let before = labels.map(|label| {
        f.shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(t) if t.galley.job.text == label => {
                    Some(shape.shape.visual_bounding_rect())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("Missing control while dynamics is off: {label}"))
    });
    f.host.events.lock().unwrap().clear();
    f.click_text("Dynamics");
    f.frame(vec![]);

    for (label, bounds) in labels.into_iter().zip(before) {
        let after = f
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(t) if t.galley.job.text == label => {
                    Some(shape.shape.visual_bounding_rect())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("Missing control while dynamics is on: {label}"));
        assert_eq!(bounds, after, "{label} moved when enabling dynamics");
    }

    for label in [
        "Threshold",
        "Range",
        "Dyn.Attack",
        "Dyn.Release",
        "Attack",
        "Release",
    ] {
        assert!(
            f.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(t) if t.galley.job.text == label)),
            "Missing {label}"
        );
    }
    assert_eq!(*f.host.events.lock().unwrap(), vec!["begin", "set", "end"]);
    assert!(f.editor.params.band(1).dynamics.enabled.value());
    assert!(!f.editor.params.band(0).dynamics.enabled.value());
    assert!(!f.editor.params.sidechain.dynamics[1].enabled.value());
    assert!(!f.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(t) if t.galley.job.text == "On")));

    f.click_text("Point 1");
    f.frame(vec![]);
    assert!(f.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(t) if t.galley.job.text == "F0 Ratio")));
    f.click_text("Point 2");
    f.frame(vec![]);

    f.click(Pos2::new(680.0, 465.0), egui::PointerButton::Secondary);
    f.frame(vec![]);
    f.frame(vec![egui::Event::Text("33.25".to_owned())]);
    f.click_text("Apply");
    assert!((f.editor.params.band(1).dynamics.attack.value() - 33.25).abs() < 0.001);
    assert_eq!(f.editor.params.active().attack.value(), 10.0);
    assert_eq!(f.editor.params.active().release.value(), 100.0);
    f.click_text("Dynamics");
    f.frame(vec![]);
    assert!(f.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(t) if t.galley.job.text == "F0 Ratio")));
    assert_eq!(f.editor.params.band(1).gain.value(), 0.0);
    assert!(!f.editor.params.band(1).dynamics.enabled.value());
    assert_eq!(
        *f.host.events.lock().unwrap(),
        vec![
            "begin", "set", "end", "begin", "set", "end", "begin", "set", "end"
        ]
    );

    f.click_text("Point 1");
    f.click_text("Peak");
    f.click_text("Low Cut");
    f.click_text("Dynamics");
    f.frame(vec![]);
    assert!(!f.editor.params.band(0).dynamics.enabled.value());
}
