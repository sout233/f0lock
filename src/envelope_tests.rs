use crate::{
    band::{Band, BandControl},
    engine::{Controls, Engine},
    filter::FilterKind,
};
use std::f32::consts::TAU;

#[test]
fn every_filter_uses_attack_and_release_for_its_processing_amount() {
    for rate in [8000.0, 48000.0, 192000.0] {
        for kind in FilterKind::ALL {
            let mut band = Band::new(rate);
            let control = BandControl {
                exists: true,
                gain: -12.0,
                kind,
                ..BandControl::default()
            };
            let alpha = 1.0 - (-1.0_f32 / (0.01 * rate)).exp();
            let steps = [1.0 / (0.05 * rate), 1.0 / (0.2 * rate)];

            for _ in 0..(0.025 * rate) as usize {
                band.process([0.1; 2], control, 220.0, true, rate, alpha, steps);
            }

            assert!((band.wet - 0.5).abs() < 0.0003, "{kind:?} at {rate}");

            for _ in 0..(0.03 * rate) as usize {
                band.process([0.1; 2], control, 220.0, true, rate, alpha, steps);
            }

            assert_eq!(band.wet, 1.0);

            for _ in 0..(0.1 * rate) as usize {
                band.process([0.1; 2], control, 220.0, false, rate, alpha, steps);
            }

            assert!((band.wet - 0.5).abs() < 0.001, "{kind:?} at {rate}");
            assert_eq!(band.gain, -12.0, "Release must not also fade filter gain");

            let previous = band.wet;
            band.process([0.1; 2], control, 220.0, true, rate, alpha, steps);
            assert!((band.wet - previous - steps[0]).abs() < 1e-6);

            for _ in 0..(0.21 * rate) as usize {
                band.process([0.1; 2], control, 220.0, false, rate, alpha, steps);
            }

            assert_eq!(band.wet, 0.0);
            assert_eq!(
                band.process([0.1; 2], control, 220.0, false, rate, alpha, steps),
                [0.1; 2]
            );
        }
    }
}

#[test]
fn losing_pitch_freezes_frequency_while_release_continues_processing_audio() {
    let mut engine = Engine::new(48000.0);
    let mut controls = Controls {
        tracking_ms: 300.0,
        attack_ms: 20.0,
        release_ms: 200.0,
        ..Controls::default()
    };

    for i in 0..24000 {
        let x = 0.02 * (TAU * 660.0 * i as f32 / 48000.0).sin();
        engine.process([x; 2], controls);
    }

    assert!(engine.estimate.frequency > 0.0);
    assert_eq!(engine.bands[0].wet, 1.0);
    controls.gate_db = -20.0;

    for _ in 0..1024 {
        engine.process([0.02; 2], controls);
        if engine.estimate.frequency == 0.0 {
            break;
        }
    }

    assert_eq!(engine.estimate.frequency, 0.0);
    let frequency = engine.frequency;
    let wet = engine.bands[0].wet;
    assert!(
        (frequency - 660.0).abs() > 1.0,
        "Tracking must still be moving at dropout"
    );

    for _ in 0..2400 {
        engine.process([0.02; 2], controls);
        assert_eq!(engine.frequency, frequency);
    }

    assert!((wet - engine.bands[0].wet - 0.25).abs() < 0.001);
    assert!(engine.bands[0].wet > 0.0);
    assert_eq!(engine.bands[0].gain, -6.0);
    engine.reset();
    assert_eq!(engine.bands[0].wet, 0.0);
}
