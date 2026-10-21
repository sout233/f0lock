use super::*;
use crate::pitch::TargetMode;

fn drag(f: &mut Fixture, start: Pos2, end: Pos2) {
    f.frame(vec![egui::Event::PointerMoved(start)]);
    f.frame(vec![egui::Event::PointerButton {
        pos: start,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    }]);
    f.frame(vec![egui::Event::PointerMoved(end)]);
    f.frame(vec![egui::Event::PointerButton {
        pos: end,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    }]);
    let events = f.host.events.lock().unwrap();
    assert_eq!(events.first(), Some(&"begin"));
    assert_eq!(events.last(), Some(&"end"));
    assert!(events.len() >= 3 && events[1..events.len() - 1].iter().all(|e| *e == "set"));
}

#[test]
fn target_modes_and_spectrum_handles_use_host_gestures_without_editing_eq_points() {
    let mut f = Fixture::new();
    f.click_text("Target: Auto");
    f.click_text("Range");
    f.frame(vec![]);
    assert_eq!(
        f.editor.params.active().target_mode.value(),
        TargetMode::Range
    );
    assert_eq!(*f.host.events.lock().unwrap(), vec!["begin", "set", "end"]);
    f.click(Pos2::new(600.0, 300.0), egui::PointerButton::Primary);
    f.host.events.lock().unwrap().clear();

    let x = |hz: f32| 57.0 + (hz / 20.0).ln() / 1000.0_f32.ln() * 786.0;
    drag(
        &mut f,
        Pos2::new(x(100.0), 220.0),
        Pos2::new(x(180.0), 220.0),
    );
    assert!((f.editor.params.active().target_min.value() - 180.0).abs() < 0.1);
    assert_eq!(f.editor.params.active().gain.value(), -6.0);
    assert_eq!(f.editor.params.active().ratio.value(), 1.0);

    f.host.events.lock().unwrap().clear();
    f.click_text("Target: Range");
    f.click_text("Reference");
    f.click(Pos2::new(600.0, 300.0), egui::PointerButton::Primary);
    assert_eq!(
        f.editor.params.active().target_mode.value(),
        TargetMode::Reference
    );
    f.host.events.lock().unwrap().clear();
    drag(
        &mut f,
        Pos2::new(x(300.0), 220.0),
        Pos2::new(x(400.0), 220.0),
    );
    assert!((f.editor.params.active().target_reference.value() - 400.0).abs() < 0.1);

    f.host.events.lock().unwrap().clear();
    let upper = 400.0 * 2.0_f32.sqrt();
    drag(
        &mut f,
        Pos2::new(x(upper), 220.0),
        Pos2::new(x(600.0), 220.0),
    );
    assert!(
        (f.editor.params.active().target_tolerance.value() - 12.0 * 1.5_f32.log2()).abs() < 0.01
    );
}

#[test]
fn range_handles_stop_at_the_opposite_boundary() {
    for (start, end, expected_min, expected_max) in
        [(100.0, 900.0, 600.0, 600.0), (600.0, 60.0, 100.0, 100.0)]
    {
        let mut f = Fixture::new();
        let setter = f.editor.gui.as_ref().unwrap().param_setter();
        set_value(
            &setter,
            &f.editor.params.active().target_mode,
            TargetMode::Range,
        );
        f.frame(vec![]);
        f.host.events.lock().unwrap().clear();

        let x = |hz: f32| 57.0 + (hz / 20.0).ln() / 1000.0_f32.ln() * 786.0;
        drag(&mut f, Pos2::new(x(start), 220.0), Pos2::new(x(end), 220.0));

        let bank = f.editor.params.active();
        assert!((bank.target_min.value() - expected_min).abs() < 0.1);
        assert!((bank.target_max.value() - expected_max).abs() < 0.1);
    }
}

#[test]
fn range_numeric_input_stops_at_the_opposite_boundary() {
    for (initial, input, expected_min, expected_max) in
        [("100", "900", 600.0, 600.0), ("600", "60", 100.0, 100.0)]
    {
        let mut f = Fixture::new();
        f.click_text("Target: Auto");
        f.click_text("Range");
        f.frame(vec![]);
        if !f.shapes.iter().any(|shape| {
            matches!(&shape.shape,
            egui::Shape::Text(t) if t.galley.job.text == "Minimum")
        }) {
            f.click_text("Target: Range");
            f.frame(vec![]);
        }

        let label = if initial == "100" {
            "Minimum"
        } else {
            "Maximum"
        };
        let row_y = f
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(t) if t.galley.job.text == label => {
                    Some(shape.shape.visual_bounding_rect().center().y)
                }
                _ => None,
            })
            .expect("Range field label missing");

        let position = f
            .shapes
            .iter()
            .rev()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(t)
                    if t.galley.job.text.starts_with(initial)
                        && (shape.shape.visual_bounding_rect().center().y - row_y).abs() < 5.0 =>
                {
                    Some(shape.shape.visual_bounding_rect().center())
                }
                _ => None,
            })
            .expect("Range numeric input missing");
        f.click(position, egui::PointerButton::Primary);
        f.frame(vec![]);
        f.frame(vec![egui::Event::Text(input.to_owned())]);
        f.frame(vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }]);
        f.frame(vec![]);

        let bank = f.editor.params.active();
        assert!(
            (bank.target_min.value() - expected_min).abs() < 0.1,
            "minimum: {}",
            bank.target_min.value()
        );
        assert!(
            (bank.target_max.value() - expected_max).abs() < 0.1,
            "maximum: {}",
            bank.target_max.value()
        );
    }
}

#[test]
fn invalid_range_is_visible_even_when_the_target_menu_is_closed() {
    let mut f = Fixture::new();
    let setter = f.editor.gui.as_ref().unwrap().param_setter();
    set_value(
        &setter,
        &f.editor.params.active().target_mode,
        TargetMode::Range,
    );
    set_value(&setter, &f.editor.params.active().target_min, 700.0);
    set_value(&setter, &f.editor.params.active().target_max, 300.0);
    f.frame(vec![]);
    assert!(f.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(t) if t.galley.job.text == "Invalid target range")));
}

#[test]
fn target_handles_and_both_parameter_banks_support_two_khz() {
    use crate::mode::OperatingMode;

    let mut f = Fixture::new();

    for mode in [OperatingMode::Track, OperatingMode::Sidechain] {
        let bank = f.editor.params.for_mode(mode);

        for param in [&bank.target_min, &bank.target_max, &bank.target_reference] {
            assert_eq!(param.preview_plain(1.0), 2000.0);
        }
    }

    let setter = f.editor.gui.as_ref().unwrap().param_setter();
    set_value(
        &setter,
        &f.editor.params.active().target_mode,
        TargetMode::Range,
    );
    set_value(&setter, &f.editor.params.active().target_max, 1200.0);
    f.frame(vec![]);
    f.host.events.lock().unwrap().clear();
    let x = |hz: f32| 57.0 + (hz / 20.0).ln() / 1000.0_f32.ln() * 786.0;
    drag(
        &mut f,
        Pos2::new(x(1200.0), 220.0),
        Pos2::new(x(2000.0), 220.0),
    );
    assert!((f.editor.params.active().target_max.value() - 2000.0).abs() < 0.1);

    let setter = f.editor.gui.as_ref().unwrap().param_setter();
    set_value(
        &setter,
        &f.editor.params.active().target_mode,
        TargetMode::Reference,
    );
    set_value(&setter, &f.editor.params.active().target_reference, 1500.0);
    f.frame(vec![]);
    f.host.events.lock().unwrap().clear();
    drag(
        &mut f,
        Pos2::new(x(1500.0), 220.0),
        Pos2::new(x(1800.0), 220.0),
    );
    assert!((f.editor.params.active().target_reference.value() - 1800.0).abs() < 0.1);
}
