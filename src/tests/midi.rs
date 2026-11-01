use crate::{
    engine::{Controls, Engine},
    lookahead::LookaheadTime,
    midi::PitchSource,
    mode::OperatingMode,
};
use nice_plug::{
    midi::{Channel, Key, VoiceID},
    prelude::*,
};
use std::f32::consts::TAU;

fn on(key: u8) -> NoteEvent<()> {
    NoteEvent::NoteOn {
        timing: 0,
        voice_id: VoiceID::Wildcard,
        channel: Channel::Number(0),
        key: Key::Number(key),
        velocity: 1.0,
    }
}

#[test]
fn polyphonic_points_filter_distinct_notes_and_release_independently() {
    use crate::midi::MidiMode;
    let mut engine = Engine::new(48000.0);
    engine.configure_pitch_source(PitchSource::Midi);
    engine.configure_midi_mode(MidiMode::Poly);
    engine.configure_midi_points(0b11);
    let mut controls = Controls {
        release_ms: 1.0,
        ..Controls::default()
    };
    controls.bands[1].exists = true;
    controls.bands[1].gain = -9.0;
    controls.bands[0].q = 6.0;
    controls.bands[1].q = 6.0;
    let mut components = [[0.0_f64; 2]; 2];
    nice_assert_no_alloc::assert_no_alloc(|| {
        engine.handle_midi(on(57));
        engine.handle_midi(on(81));
        for i in 0..48000 {
            let phases = [220.0, 880.0].map(|hz| TAU * hz * i as f32 / 48000.0);
            let input = 0.1 * (phases[0].sin() + phases[1].sin());
            let out = engine.process([input; 2], controls);
            if i >= 24000 {
                for (j, phase) in phases.into_iter().enumerate() {
                    components[j][0] += f64::from(out[0] * phase.sin());
                    components[j][1] += f64::from(out[0] * phase.cos());
                }
            }
        }
        assert!((engine.bands[0].frequency - 220.0).abs() < 0.01);
        assert!((engine.bands[1].frequency - 880.0).abs() < 0.01);
        engine.handle_midi(NoteEvent::NoteOff {
            timing: 0,
            voice_id: VoiceID::Wildcard,
            channel: Channel::Number(0),
            key: Key::Number(57),
            velocity: 0.0,
        });
        for _ in 0..1000 {
            engine.process([0.1; 2], controls);
        }
        assert_eq!(engine.bands[0].wet, 0.0);
        assert_eq!(engine.bands[1].wet, 1.0);
        assert!((engine.bands[1].frequency - 880.0).abs() < 0.01);
    });
    for (i, expected) in [-6.0, -9.0].into_iter().enumerate() {
        let gain = 20.0 * (components[i][0].hypot(components[i][1]) / 1200.0).log10();
        assert!((gain - expected).abs() < 0.1, "point {i}: {gain} dB");
    }
}

#[test]
fn polyphonic_note_on_and_off_remain_aligned_with_lookahead() {
    use crate::midi::MidiMode;
    let mut engine = Engine::new(48000.0);
    engine.configure_pitch_source(PitchSource::Midi);
    engine.configure_midi_mode(MidiMode::Poly);
    engine.configure_midi_points(0b11);
    engine.configure_lookahead(true, LookaheadTime::Ms150);
    let mut controls = Controls::default();
    controls.bands[1].exists = true;
    nice_assert_no_alloc::assert_no_alloc(|| {
        engine.handle_midi(on(57));
        engine.handle_midi(on(81));
        for i in 0..7300 {
            if i == 30 {
                engine.handle_midi(NoteEvent::NoteOff {
                    timing: 0,
                    voice_id: VoiceID::Wildcard,
                    channel: Channel::Number(0),
                    key: Key::Number(57),
                    velocity: 0.0,
                });
            }
            engine.process([0.1; 2], controls);
            let expected = if i < 7200 {
                0
            } else if i < 7230 {
                0b11
            } else {
                0b10
            };
            assert_eq!(engine.midi_active_mask(), expected, "sample {i}");
        }
    });
}

#[test]
fn midi_sets_pitch_immediately_without_audio_detection_or_target_limits() {
    let mut engine = Engine::new(48000.0);
    engine.configure_pitch_source(PitchSource::Midi);
    let mut controls = Controls::default();
    controls.target.mode = crate::pitch::TargetMode::Range;
    controls.target.minimum = 1000.0;
    controls.target.maximum = 2000.0;
    controls.gate_db = 0.0;
    controls.clarity = 1.0;
    nice_assert_no_alloc::assert_no_alloc(|| {
        engine.handle_midi(on(23)); // B0, below even the extended audio detector.
        engine.process([0.0; 2], controls);
        assert!((engine.estimate.frequency - 30.8677).abs() < 0.001);
        engine.handle_midi(on(0));
        engine.process([0.0; 2], controls);
        assert!((engine.estimate.frequency - 8.175799).abs() < 0.001);
        engine.handle_midi(on(127));
        engine.process([0.0; 2], controls);
        assert!((engine.estimate.frequency - 12543.854).abs() < 0.01);
        assert!(engine.bands.iter().all(|band| band.frequency.is_finite()));
    });
}

#[test]
fn midi_pitch_drives_eq_and_note_off_releases_it_on_both_audio_paths() {
    for mode in OperatingMode::ALL {
        let mut engine = Engine::new(48000.0);
        engine.configure_pitch_source(PitchSource::Midi);
        let controls = Controls {
            mode,
            release_ms: 10.0,
            ..Controls::default()
        };
        engine.handle_midi(on(69));
        let mut powers = [0.0_f64; 2];
        nice_assert_no_alloc::assert_no_alloc(|| {
            for i in 0..24000 {
                let sample = 0.2 * (TAU * 440.0 * i as f32 / 48000.0).sin();
                let output = engine.process_with_sidechain([sample; 2], None, controls);
                if i >= 12000 {
                    powers[0] += f64::from(sample).powi(2);
                    powers[1] += f64::from(output[0]).powi(2);
                }
            }
            engine.handle_midi(NoteEvent::NoteOff {
                timing: 0,
                voice_id: VoiceID::Wildcard,
                channel: Channel::Number(0),
                key: Key::Number(69),
                velocity: 0.0,
            });
            for _ in 0..1000 {
                engine.process_with_sidechain([0.1; 2], None, controls);
            }
            assert_eq!(engine.estimate.frequency, 0.0);
            assert_eq!(engine.bands[0].wet, 0.0);
            assert_eq!(
                engine.process_with_sidechain([0.1; 2], None, controls),
                [0.1; 2]
            );
        });
        let gain = 10.0 * (powers[1] / powers[0]).log10();
        assert!((gain + 6.0).abs() < 0.1, "{mode:?}: {gain} dB");
    }
}

#[test]
fn midi_events_align_with_lookahead_audio_and_configuration_changes_keep_held_notes() {
    let mut engine = Engine::new(48000.0);
    engine.configure_pitch_source(PitchSource::Midi);
    engine.configure_lookahead(true, LookaheadTime::Minimum);
    assert_eq!(engine.latency_samples(), 0);
    engine.handle_midi(on(69));
    engine.configure_lookahead(true, LookaheadTime::Ms150);
    assert_eq!(engine.latency_samples(), 7200);
    nice_assert_no_alloc::assert_no_alloc(|| {
        for i in 0..7250 {
            let input = if i == 0 { [0.25; 2] } else { [0.0; 2] };
            let controls = Controls {
                bypass: true,
                ..Controls::default()
            };
            engine.process(input, controls);
            if i < 7200 {
                assert_eq!(engine.estimate.frequency, 0.0);
            } else {
                assert_eq!(engine.estimate.frequency, 440.0);
            }
        }
        engine.configure_pitch_source(PitchSource::Audio);
        assert_eq!(engine.latency_samples(), 7200);
        assert_eq!(engine.estimate.frequency, 0.0);
        engine.configure_pitch_source(PitchSource::Midi);
        engine.configure_lookahead(false, LookaheadTime::Minimum);
        engine.process([0.0; 2], Controls::default());
        assert_eq!(engine.estimate.frequency, 440.0);
        engine.reset();
        engine.process([0.1; 2], Controls::default());
        assert_eq!(engine.estimate.frequency, 0.0);
    });
}
