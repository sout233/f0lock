use crate::{
    band::{Band, BandControl},
    engine::{Controls, Engine},
    filter::FilterKind,
};
use std::f32::consts::TAU;

#[test]
fn infinite_release_keeps_low_cut_after_pitch_loss_and_resumes_normal_release_when_disabled() {
    use crate::{filter::CutSlope, lookahead::LookaheadTime, mode::OperatingMode};

    for mode in [OperatingMode::Track, OperatingMode::Sidechain] {
        for lookahead in [false, true] {
            let mut engine = Engine::new(48000.0);
            engine.configure_lookahead(lookahead, LookaheadTime::Ms150);
            let mut controls = Controls {
                mode,
                infinite_release: true,
                ..Controls::default()
            };
            controls.bands[0].kind = FilterKind::Highpass;
            controls.bands[0].slope = CutSlope::Db24;
            controls.bands[0].q = std::f32::consts::FRAC_1_SQRT_2;

            // No detection yet: infinite release must not enable a placeholder EQ.
            for _ in 0..12000 {
                engine.process_with_sidechain([0.0; 2], Some([0.0; 2]), controls);
            }
            assert_eq!(engine.bands[0].wet, 0.0);

            for i in 0..48000 {
                let x = 0.02 * (TAU * 440.0 * i as f32 / 48000.0).sin();
                engine.process_with_sidechain([x; 2], Some([x; 2]), controls);
            }
            assert_eq!(engine.bands[0].wet, 1.0);
            controls.gate_db = -20.0;

            let mut input_power = 0.0_f64;
            let mut output_power = 0.0_f64;
            for i in 0..48000 {
                let x = 0.005 * (TAU * 110.0 * i as f32 / 48000.0).sin();
                let y = engine.process_with_sidechain([x; 2], Some([0.0; 2]), controls);
                if i > 24000 {
                    input_power += f64::from(x).powi(2);
                    output_power += f64::from(y[0]).powi(2);
                }
            }
            assert_eq!(engine.estimate.frequency, 0.0);
            assert_eq!(engine.bands[0].wet, 1.0);
            assert!((engine.frequency - 440.0).abs() < 1.0);
            assert!(
                10.0 * (output_power / input_power).log10() < -40.0,
                "Held Low Cut must still process audio: {mode:?}, lookahead={lookahead}"
            );

            // A new note continues tracking instead of freezing the pitch forever.
            controls.gate_db = -55.0;
            for i in 0..48000 {
                let x = 0.02 * (TAU * 660.0 * i as f32 / 48000.0).sin();
                engine.process_with_sidechain([x; 2], Some([x; 2]), controls);
            }
            assert!((engine.frequency - 660.0).abs() < 1.0);

            controls.gate_db = -20.0;
            controls.infinite_release = false;
            for _ in 0..24000 {
                engine.process_with_sidechain([0.001; 2], Some([0.0; 2]), controls);
            }
            assert_eq!(engine.bands[0].wet, 0.0);
        }
    }
}

#[test]
fn infinite_release_does_not_prevent_explicit_band_disable_delete_or_reset() {
    for action in 0..3 {
        let mut engine = Engine::new(48000.0);
        let mut controls = Controls {
            infinite_release: true,
            ..Controls::default()
        };
        for i in 0..24000 {
            let x = 0.1 * (TAU * 220.0 * i as f32 / 48000.0).sin();
            engine.process([x; 2], controls);
        }
        assert_eq!(engine.bands[0].wet, 1.0);
        match action {
            0 => controls.bands[0].exists = false,
            1 => controls.bands[0].enabled = false,
            _ => engine.reset(),
        }
        for _ in 0..12000 {
            engine.process([0.0; 2], controls);
        }
        assert_eq!(engine.bands[0].wet, 0.0);
    }
}

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
                band.process([0.1; 2], [0.1; 2], control, 220.0, true, rate, alpha, steps);
            }

            assert!((band.wet - 0.5).abs() < 0.0003, "{kind:?} at {rate}");

            for _ in 0..(0.03 * rate) as usize {
                band.process([0.1; 2], [0.1; 2], control, 220.0, true, rate, alpha, steps);
            }

            assert_eq!(band.wet, 1.0);

            for _ in 0..(0.1 * rate) as usize {
                band.process(
                    [0.1; 2], [0.1; 2], control, 220.0, false, rate, alpha, steps,
                );
            }

            assert!((band.wet - 0.5).abs() < 0.001, "{kind:?} at {rate}");
            assert_eq!(band.gain, -12.0, "Release must not also fade filter gain");

            let previous = band.wet;
            band.process([0.1; 2], [0.1; 2], control, 220.0, true, rate, alpha, steps);
            assert!((band.wet - previous - steps[0]).abs() < 1e-6);

            for _ in 0..(0.21 * rate) as usize {
                band.process(
                    [0.1; 2], [0.1; 2], control, 220.0, false, rate, alpha, steps,
                );
            }

            assert_eq!(band.wet, 0.0);
            assert_eq!(
                band.process(
                    [0.1; 2], [0.1; 2], control, 220.0, false, rate, alpha, steps
                ),
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
        let hz = if i < 12000 { 440.0 } else { 660.0 };
        let x = 0.02 * (TAU * hz * i as f32 / 48000.0).sin();
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
