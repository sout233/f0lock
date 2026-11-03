use super::*;

/// Centre of the Listen strip control, left of Active.
fn listen_button() -> Pos2 {
    Pos2::new(690.0, 298.0)
}

#[test]
fn listen_button_auditions_the_selected_point_only() {
    let mut f = Fixture::new();
    assert_eq!(f.auditioned(), None);

    f.host.events.lock().unwrap().clear();
    f.click(listen_button(), egui::PointerButton::Primary);
    f.frame(vec![]);

    assert_eq!(
        f.auditioned(),
        Some(0),
        "the first point is selected on open"
    );
    assert!(
        f.host.events.lock().unwrap().is_empty(),
        "audition is view state, it must not reach the host"
    );

    f.add();
    f.click_text("Point 2");
    f.frame(vec![]);
    assert_eq!(
        f.auditioned(),
        Some(1),
        "audition follows the selected point"
    );

    f.click(listen_button(), egui::PointerButton::Primary);
    f.frame(vec![]);
    assert_eq!(f.auditioned(), None);
}

/// Peak level of a steady tone at `tone_hz`, after the chain settles.
fn peak(tone_hz: f32, gain: f32, audition: Option<usize>, half_width: f32) -> f32 {
    use crate::engine::{Controls, Engine};
    use crate::midi::PitchSource;
    use nice_plug::midi::{Channel, Key, NoteEvent, VoiceID};

    let mut engine = Engine::new(48000.0);
    engine.configure_pitch_source(PitchSource::Midi);
    // A3, so point 0 sits at 220 Hz with its default ratio of 1.
    engine.handle_midi(NoteEvent::NoteOn {
        timing: 0,
        voice_id: VoiceID::Wildcard,
        channel: Channel::Number(0),
        key: Key::Number(57),
        velocity: 1.0,
    });

    let mut controls = Controls::default();
    controls.bands[0].exists = true;
    controls.bands[0].gain = gain;
    controls.audition = audition;
    controls.listen_half_width = half_width;

    let mut peak = 0.0_f32;
    for i in 0..96000 {
        let x = 0.1 * (std::f32::consts::TAU * tone_hz * i as f32 / 48000.0).sin();
        let output = engine.process([x; 2], controls);
        if i > 72000 {
            peak = peak.max(output[0].abs());
        }
    }
    peak
}

#[test]
fn audition_hears_the_processed_band_around_the_point() {
    let decibels = |peak: f32| 20.0 * (peak / 0.1).log10();

    // Point 0 cuts 12 dB at 220 Hz. Listening on it must play the processed
    // band, not the dry signal.
    let listening = decibels(peak(220.0, -12.0, Some(0), 50.0));
    assert!(
        (listening + 12.0).abs() < 1.5,
        "the point should be heard through the chain at -12 dB, got {listening:.1} dB"
    );

    // Away from the point the window closes, even though the chain itself
    // barely touches 1 kHz.
    let outside = decibels(peak(1000.0, -12.0, Some(0), 50.0));
    assert!(
        outside < -15.0,
        "1 kHz should fall outside a 220 Hz +/- 50 Hz window, got {outside:.1} dB"
    );

    let without = decibels(peak(1000.0, -12.0, None, 50.0));
    assert!(
        without > -3.0,
        "without listening 1 kHz should pass almost untouched, got {without:.1} dB"
    );

    // Widening the window lets the same tone through, which is what the drag on
    // the strip is for.
    let wide = decibels(peak(1000.0, -12.0, Some(0), 500.0));
    assert!(
        wide > outside + 6.0 && wide > -8.0,
        "a 500 Hz window should pass 1 kHz, got {wide:.1} dB against {outside:.1} dB"
    );
}

#[test]
fn dragging_the_listen_control_sets_the_window_width() {
    let mut f = Fixture::new();
    assert!((f.editor.audition.half_width() - 50.0).abs() < 0.01);

    // 50 px at 2 Hz per pixel: 50 -> 150 Hz.
    f.drag(listen_button(), listen_button() + Vec2::new(50.0, 0.0));
    assert!(
        (f.editor.audition.half_width() - 150.0).abs() < 2.0,
        "width is {} Hz",
        f.editor.audition.half_width()
    );
    assert_eq!(f.auditioned(), None, "a drag must not start the audition");

    // The strip stays draggable while the audition runs.
    f.click(listen_button(), egui::PointerButton::Primary);
    f.frame(vec![]);
    assert_eq!(f.auditioned(), Some(0));
    f.drag(listen_button(), listen_button() - Vec2::new(100.0, 0.0));
    assert!((f.editor.audition.half_width() - 10.0).abs() < 0.01);
    assert_eq!(
        f.auditioned(),
        Some(0),
        "dragging must not stop the audition"
    );
}

/// X position of a frequency in the spectrum plot: the outer rect is
/// (20, 78)-(880, 278) and the plot inset is 37 px horizontally.
fn plot_x(hz: f32) -> f32 {
    let left = 57.0;
    let width = 786.0;
    left + (hz / 20.0_f32).ln() / (20000.0_f32 / 20.0).ln() * width
}

fn audition_window(f: &Fixture) -> Option<Rect> {
    f.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Rect(rect) if rect.rect.height() > 120.0 && rect.fill.a() > 0 => {
            Some(rect.rect)
        }
        _ => None,
    })
}

fn marker_color(f: &Fixture, label: &str) -> Option<egui::Color32> {
    f.shapes.iter().rev().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text) if text.galley.job.text == label => Some(text.fallback_color),
        _ => None,
    })
}

#[test]
fn spectrum_draws_the_audition_window_and_dims_other_points() {
    let mut f = Fixture::new();
    f.add();
    f.click_text("Point 1");
    f.frame(vec![]);

    assert!(
        audition_window(&f).is_none(),
        "the window must only show while listening"
    );
    assert_eq!(marker_color(&f, "2"), Some(POINT_COLORS[1]));

    f.click(listen_button(), egui::PointerButton::Primary);
    f.frame(vec![]);

    // The window spans the point frequency +/- the strip width.
    let pitch = f.editor.telemetry.values()[2];
    let center = if pitch > 0.0 { pitch } else { 150.0 };
    let window = audition_window(&f).expect("the audition window is missing");
    assert!(
        (window.left() - plot_x(center - 50.0)).abs() < 1.5,
        "left edge at {} instead of {}",
        window.left(),
        plot_x(center - 50.0)
    );
    assert!(
        (window.right() - plot_x(center + 50.0)).abs() < 1.5,
        "right edge at {} instead of {}",
        window.right(),
        plot_x(center + 50.0)
    );

    assert_eq!(marker_color(&f, "1"), Some(POINT_COLORS[0]));
    assert_eq!(
        marker_color(&f, "2"),
        Some(egui::Color32::from_gray(72)),
        "points outside the audition should dim"
    );
}
