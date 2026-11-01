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

#[test]
fn poly_mode_uses_host_gestures_and_shows_each_points_own_pitch() {
    use crate::{
        engine::{Controls, Engine},
        midi::{MidiMode, PitchSource},
    };
    use nice_plug::midi::{Channel, Key, VoiceID};
    let mut f = Fixture::new();
    f.add();
    let setter = f.editor.gui.as_ref().unwrap().param_setter();
    set_value(&setter, &f.editor.params.pitch_source, PitchSource::Midi);
    set_value(&setter, f.editor.params.band(1).ratio, 1.0);
    f.click(Pos2::new(860.0, 32.0), egui::PointerButton::Primary);
    f.click_text("Audio Settings");
    f.frame(vec![]);
    f.host.events.lock().unwrap().clear();
    f.click_text("Mono");
    f.click_text(MidiMode::Poly.label());
    assert_eq!(f.editor.params.midi_mode.value(), MidiMode::Poly);
    assert_eq!(*f.host.events.lock().unwrap(), vec!["begin", "set", "end"]);
    f.editor.audio_open = false;
    let mut engine = Engine::new(48000.0);
    engine.configure_pitch_source(PitchSource::Midi);
    engine.configure_midi_mode(MidiMode::Poly);
    engine.configure_midi_points(0b11);
    for key in [57, 81] {
        engine.handle_midi(NoteEvent::NoteOn {
            timing: 0,
            voice_id: VoiceID::Wildcard,
            channel: Channel::Number(0),
            key: Key::Number(key),
            velocity: 1.0,
        });
    }
    let mut controls = Controls::default();
    controls.bands[1].exists = true;
    engine.process([0.1; 2], controls);
    f.editor.telemetry.publish(&engine);
    f.frame(vec![]);
    let marker = |label: &str| {
        f.shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == label => {
                    Some(t.pos.x + t.galley.size().x * 0.5)
                }
                _ => None,
            })
            .expect("Rendered point marker missing")
    };
    let distance = marker("2") - marker("1");
    assert!((distance - 4.0_f32.ln() / 1000.0_f32.ln() * 786.0).abs() < 0.01);
    assert!(f.shapes.iter().any(|s| matches!(&s.shape,
        egui::Shape::Text(t) if t.galley.job.text == "MIDI Poly · 2 notes")));
    f.add();
    assert!((f.editor.params.band(2).ratio.value() - 1.0).abs() < 1e-5);
}
