use super::*;

fn enter_number(fixture: &mut Fixture, center: Pos2, text: &str) {
    fixture.click(center, egui::PointerButton::Secondary);
    fixture.frame(vec![]);
    fixture.frame(vec![egui::Event::Text(text.to_owned())]);
}

#[test]
fn right_click_input_preserves_precision_and_displays_two_decimals() {
    let mut f = Fixture::new();
    enter_number(&mut f, Pos2::new(90.0, 583.0), "47.529938");
    f.click_text("Apply");
    f.frame(vec![]);

    assert!((f.editor.params.tracking.value() - 47.529938).abs() < 0.00002);
    assert_eq!(*f.host.events.lock().unwrap(), vec!["begin", "set", "end"]);
    assert!(f.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.job.text == "47.53 ms")));
}

#[test]
fn clarity_accepts_percentages_and_enter_confirms_input() {
    let mut f = Fixture::new();
    enter_number(&mut f, Pos2::new(450.0, 583.0), "82.345");
    f.frame(vec![egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }]);
    f.frame(vec![]);

    assert!((f.editor.params.clarity.value() - 0.82345).abs() < 0.000001);
    assert_eq!(*f.host.events.lock().unwrap(), vec!["begin", "set", "end"]);
    assert!(f.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.job.text == "82.35%")));
}

#[test]
fn invalid_input_and_cancel_never_write_parameters() {
    for (input, error) in [
        ("abc", "Enter a valid number"),
        ("NaN", "Enter a finite number"),
        ("inf", "Enter a finite number"),
        ("301", "Range: 5.00 to 300.00"),
    ] {
        let mut f = Fixture::new();
        enter_number(&mut f, Pos2::new(90.0, 583.0), input);
        f.click_text("Apply");
        f.frame(vec![]);

        assert_eq!(f.editor.params.tracking.value(), 35.0);
        assert!(f.host.events.lock().unwrap().is_empty());
        assert!(f.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.job.text == error)));

        f.click_text("Cancel");
        f.frame(vec![]);

        assert!(f.host.events.lock().unwrap().is_empty());
        assert!(!f.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.job.text == "Apply")));
    }
}

#[test]
fn numeric_entry_targets_only_the_selected_point_and_ends_switch_animation() {
    let mut f = Fixture::new();
    f.add();
    f.host.events.lock().unwrap().clear();

    let center = Pos2::new(562.5, 424.5);
    enter_number(&mut f, center, "2.126");
    f.click_text("Apply");
    f.frame(vec![]);

    let ratio = f.editor.params.band(1).ratio;

    assert!((ratio.value() - 2.126).abs() < 0.00001);
    assert_eq!(f.editor.params.band(0).ratio.value(), 1.0);
    assert!((f.knob_position(center) - ratio.modulated_normalized_value()).abs() < 0.00001);
    assert_eq!(*f.host.events.lock().unwrap(), vec!["begin", "set", "end"]);
    assert!(f.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.job.text == "× 2.13")));
}

#[test]
fn escape_and_tab_switch_discard_unconfirmed_input() {
    let mut f = Fixture::new();
    f.add();
    f.host.events.lock().unwrap().clear();

    enter_number(&mut f, Pos2::new(112.5, 424.5), "6.75");
    f.click_text("Point 1");
    f.frame(vec![]);

    assert_eq!(f.editor.params.band(0).gain.value(), -6.0);
    assert_eq!(f.editor.params.band(1).gain.value(), 0.0);
    assert!(f.host.events.lock().unwrap().is_empty());

    enter_number(&mut f, Pos2::new(90.0, 583.0), "48");
    f.frame(vec![egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }]);
    f.frame(vec![]);

    assert_eq!(f.editor.params.tracking.value(), 35.0);
    assert!(f.host.events.lock().unwrap().is_empty());
    assert!(!f.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.job.text == "Apply")));
}

#[test]
fn all_knob_readouts_use_two_decimal_places() {
    let mut f = Fixture::new();
    let gui = f.editor.gui.as_ref().unwrap();
    let setter = gui.param_setter();
    set_value(&setter, &f.editor.params.gate, -57.68801);
    set_value(&setter, &f.editor.params.mix, 94.55998);
    f.frame(vec![]);

    for value in [
        "-6.00 dB",
        "3.00",
        "× 1.00",
        "35.00 ms",
        "-57.69 dBFS",
        "85.00%",
        "94.56 %",
        "0.00 dB",
    ] {
        assert!(
            f.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.job.text == value)),
            "Missing readout: {value}"
        );
    }
}
