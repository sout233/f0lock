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
    assert_eq!(f.editor.params.target_mode.value(), TargetMode::Range);
    assert_eq!(*f.host.events.lock().unwrap(), vec!["begin", "set", "end"]);
    f.click(Pos2::new(600.0, 300.0), egui::PointerButton::Primary);
    f.host.events.lock().unwrap().clear();

    let x = |hz: f32| 57.0 + (hz / 20.0).ln() / 1000.0_f32.ln() * 786.0;
    drag(
        &mut f,
        Pos2::new(x(100.0), 220.0),
        Pos2::new(x(180.0), 220.0),
    );
    assert!((f.editor.params.target_min.value() - 180.0).abs() < 0.1);
    assert_eq!(f.editor.params.gain.value(), -6.0);
    assert_eq!(f.editor.params.ratio.value(), 1.0);

    f.host.events.lock().unwrap().clear();
    f.click_text("Target: Range");
    f.click_text("Reference");
    f.click(Pos2::new(600.0, 300.0), egui::PointerButton::Primary);
    assert_eq!(f.editor.params.target_mode.value(), TargetMode::Reference);
    f.host.events.lock().unwrap().clear();
    drag(
        &mut f,
        Pos2::new(x(300.0), 220.0),
        Pos2::new(x(400.0), 220.0),
    );
    assert!((f.editor.params.target_reference.value() - 400.0).abs() < 0.1);

    f.host.events.lock().unwrap().clear();
    let upper = 400.0 * 2.0_f32.sqrt();
    drag(
        &mut f,
        Pos2::new(x(upper), 220.0),
        Pos2::new(x(600.0), 220.0),
    );
    assert!((f.editor.params.target_tolerance.value() - 12.0 * 1.5_f32.log2()).abs() < 0.01);
}

#[test]
fn invalid_range_is_visible_even_when_the_target_menu_is_closed() {
    let mut f = Fixture::new();
    let setter = f.editor.gui.as_ref().unwrap().param_setter();
    set_value(&setter, &f.editor.params.target_mode, TargetMode::Range);
    set_value(&setter, &f.editor.params.target_min, 700.0);
    set_value(&setter, &f.editor.params.target_max, 300.0);
    f.frame(vec![]);
    assert!(f.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(t) if t.galley.job.text == "Invalid target range")));
}
