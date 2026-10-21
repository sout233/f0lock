use crate::{
    engine::{Controls, Engine},
    mode::OperatingMode,
    pitch::DetectionMode,
};
use std::f32::consts::TAU;

#[test]
fn sidechain_pitch_controls_main_audio_and_does_not_leak_into_the_output() {
    for detection in DetectionMode::ALL {
        let mut engine = Engine::new(48000.0);
        let mut controls = Controls {
            mode: OperatingMode::Sidechain,
            detection,
            ..Controls::default()
        };
        controls.bands[0].ratio = 4.0;
        let mut power_in = 0.0_f64;
        let mut power_out = 0.0_f64;

        nice_assert_no_alloc::assert_no_alloc(|| {
            for i in 0..48000 {
                let main = 0.1 * (TAU * 440.0 * i as f32 / 48000.0).sin();
                let sidechain = 0.2 * (TAU * 110.0 * i as f32 / 48000.0).sin();
                let output =
                    engine.process_with_sidechain([main, -main], Some([sidechain; 2]), controls);
                if i > 24000 {
                    power_in += f64::from(main).powi(2);
                    power_out += f64::from(output[0]).powi(2);
                }
                assert!((output[0] + output[1]).abs() < 1e-6);
            }
        });

        let db = 10.0 * (power_out / power_in).log10();
        assert!(
            (engine.estimate.frequency - 110.0).abs() < 0.5,
            "{detection:?}"
        );
        assert!((engine.bands[0].frequency - 440.0).abs() < 1.0);
        assert!((db + 6.0).abs() < 0.1, "{detection:?}: {db} dB");

        for i in 0..24000 {
            let main = 0.1 * (TAU * 440.0 * i as f32 / 48000.0).sin();
            let output = engine.process_with_sidechain([main; 2], None, controls);
            if i > 20000 {
                assert_eq!(output, [main; 2]);
            }
        }
        assert_eq!(
            engine.estimate.frequency, 0.0,
            "Unrouted sidechain used main input"
        );
        assert_eq!(engine.bands[0].wet, 0.0);

        engine.reset();
        for i in 0..24000 {
            let sidechain = 0.2 * (TAU * 110.0 * i as f32 / 48000.0).sin();
            assert_eq!(
                engine.process_with_sidechain([0.0; 2], Some([sidechain; 2]), controls),
                [0.0; 2]
            );
        }
        assert!(engine.estimate.frequency > 0.0);
    }
}

#[test]
fn switching_detection_sources_discards_the_previous_pitch_and_keeps_main_spectrum() {
    let mut engine = Engine::new(48000.0);
    engine.visualize = true;
    let mut controls = Controls::default();

    for i in 0..24000 {
        let main = 0.2 * (TAU * 440.0 * i as f32 / 48000.0).sin();
        engine.process_with_sidechain([main; 2], Some([0.0; 2]), controls);
    }
    assert!((engine.estimate.frequency - 440.0).abs() < 0.5);
    controls.mode = OperatingMode::Sidechain;
    engine.process_with_sidechain([0.1; 2], Some([0.1; 2]), controls);
    assert_eq!(engine.estimate.frequency, 0.0);

    for i in 0..48000 {
        let main = 0.2 * (TAU * 440.0 * i as f32 / 48000.0).sin();
        let sidechain = 0.2 * (TAU * 110.0 * i as f32 / 48000.0).sin();
        engine.process_with_sidechain([main; 2], Some([sidechain; 2]), controls);
    }
    assert!((engine.estimate.frequency - 110.0).abs() < 0.5);
    let peak = engine
        .analyzer
        .display
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .unwrap()
        .0;
    let hz = crate::analyzer::bin_frequency(peak, crate::analyzer::max_frequency(48000.0));
    assert!(
        (hz - 440.0).abs() < 30.0,
        "Spectrum incorrectly displays sidechain: {hz}"
    );

    controls.mode = OperatingMode::Track;
    engine.process_with_sidechain([0.1; 2], Some([0.1; 2]), controls);
    assert_eq!(engine.estimate.frequency, 0.0);
}

#[test]
fn parameter_banks_retain_original_track_ids_and_expose_distinct_sidechain_ids() {
    use nice_plug::prelude::*;
    let params = crate::params::F0Params::default();
    let root = params.param_map();
    for (id, _, _) in params.track.param_map() {
        assert!(
            root.iter().any(|(root_id, _, _)| *root_id == id),
            "Old ID missing: {id}"
        );
        assert!(
            root.iter()
                .any(|(root_id, _, _)| *root_id == format!("sc_{id}")),
            "Sidechain ID missing: {id}"
        );
    }
    assert_eq!(params.mode.value(), OperatingMode::Track);
    assert_ne!(
        std::ptr::addr_of!(params.track.gain),
        std::ptr::addr_of!(params.sidechain.gain)
    );
}
