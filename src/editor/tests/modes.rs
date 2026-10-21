use super::*;
use crate::{mode::OperatingMode, pitch::DetectionMode};

#[test]
fn processing_bar_mode_switch_restores_independent_point_and_global_parameters() {
    let mut f = Fixture::new();
    f.add();
    assert_eq!(f.editor.selected, Some(1));
    f.host.events.lock().unwrap().clear();
    f.click_text("Track EQ");
    f.click_text("Sidechain EQ");
    f.frame(vec![]);
    assert_eq!(f.editor.params.mode.value(), OperatingMode::Sidechain);
    assert_eq!(f.editor.selected, Some(0));
    assert!(!f.editor.params.band(1).exists.value());
    assert_eq!(*f.host.events.lock().unwrap(), vec!["begin", "set", "end"]);

    let setter = f.editor.gui.as_ref().unwrap().param_setter();
    set_value(&setter, &f.editor.params.active().gain, -12.0);
    set_value(&setter, &f.editor.params.active().attack, 40.0);
    set_value(
        &setter,
        &f.editor.params.active().detection,
        DetectionMode::Pyin,
    );
    f.frame(vec![]);
    assert!(
        f.shapes.iter().any(
            |s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text == "Sidechain EQ")
        )
    );
    f.click_text("Sidechain EQ");
    f.click_text("Track EQ");
    f.frame(vec![]);

    assert_eq!(f.editor.selected, Some(1));
    assert!(f.editor.params.band(1).exists.value());
    assert_eq!(f.editor.params.active().gain.value(), -6.0);
    assert_eq!(f.editor.params.active().attack.value(), 10.0);
    assert_eq!(
        f.editor.params.active().detection.value(),
        DetectionMode::SpectralMelody
    );

    f.click_text("Track EQ");
    f.click_text("Sidechain EQ");
    f.frame(vec![]);
    assert_eq!(f.editor.params.active().gain.value(), -12.0);
    assert_eq!(f.editor.params.active().attack.value(), 40.0);
    assert_eq!(
        f.editor.params.active().detection.value(),
        DetectionMode::Pyin
    );
}
