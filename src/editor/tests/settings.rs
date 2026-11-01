use super::*;
use crate::pitch::DetectionRange;

#[test]
fn audio_settings_switches_detection_range_through_host_gestures() {
    let mut f = Fixture::new();
    assert_eq!(
        f.editor.params.detection_range.value(),
        DetectionRange::Standard
    );
    f.click(Pos2::new(860.0, 32.0), egui::PointerButton::Primary);
    f.click_text("Audio Settings");
    f.frame(vec![]);
    f.host.events.lock().unwrap().clear();
    f.click_text(DetectionRange::Standard.label());
    f.click_text(DetectionRange::Low.label());
    f.frame(vec![]);
    assert_eq!(f.editor.params.detection_range.value(), DetectionRange::Low);
    assert_eq!(*f.host.events.lock().unwrap(), vec!["begin", "set", "end"]);
    assert!(f.shapes.iter().any(|s| matches!(&s.shape,
        egui::Shape::Text(t) if t.galley.job.text.contains("responds more slowly"))));

    f.time += 1.0;
    f.host.events.lock().unwrap().clear();
    f.click_text(DetectionRange::Low.label());
    f.click_text(DetectionRange::Standard.label());
    assert_eq!(
        f.editor.params.detection_range.value(),
        DetectionRange::Standard
    );
    assert_eq!(*f.host.events.lock().unwrap(), vec!["begin", "set", "end"]);
}

#[test]
fn midi_source_can_be_selected_from_the_processing_bar_and_settings() {
    use crate::midi::PitchSource;

    let mut f = Fixture::new();
    assert_eq!(f.editor.params.pitch_source.value(), PitchSource::Audio);
    f.click_text("Pitch: Audio");
    f.click_text("MIDI");
    f.frame(vec![]);
    assert_eq!(f.editor.params.pitch_source.value(), PitchSource::Midi);
    assert_eq!(*f.host.events.lock().unwrap(), vec!["begin", "set", "end"]);
    assert!(f.shapes.iter().any(|s| matches!(&s.shape,
        egui::Shape::Text(t) if t.galley.job.text == "MIDI Tracking")));
    assert!(!f.shapes.iter().any(|s| matches!(&s.shape,
        egui::Shape::Text(t) if t.galley.job.text == "Target: Auto")));
    f.click(Pos2::new(860.0, 32.0), egui::PointerButton::Primary);
    f.click_text("Audio Settings");
    f.frame(vec![]);
    assert!(f.shapes.iter().any(|s| matches!(&s.shape,
        egui::Shape::Text(t) if t.galley.job.text == "MIDI notes")));
    f.host.events.lock().unwrap().clear();
    f.click_text("MIDI");
    f.click_text("Audio");
    assert_eq!(f.editor.params.pitch_source.value(), PitchSource::Audio);
    assert_eq!(*f.host.events.lock().unwrap(), vec!["begin", "set", "end"]);
}
