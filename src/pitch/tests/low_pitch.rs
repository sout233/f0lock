use super::*;
use crate::detector::{Detector, Estimate};
use std::f32::consts::TAU;

#[test]
fn all_backends_track_sub_bass_across_sample_rates_and_target_modes() {
    for rate in [
        8000.0, 22050.0, 23999.0, 44100.0, 48000.0, 96000.0, 192000.0,
    ] {
        for mode in DetectionMode::ALL {
            for target_mode in [TargetMode::Auto, TargetMode::Range, TargetMode::Reference] {
                let mut detector = Detector::new(rate);
                detector.mode = mode;
                detector.configure_range(DetectionRange::Low);

                // Near the lower boundary, low A, five-string bass B, and low E.
                for hz in [25.5, 27.5, 30.8677, 41.2034] {
                    detector.reset();
                    detector.target = Target {
                        mode: target_mode,
                        minimum: EXTENDED_MIN_HZ,
                        maximum: 45.0,
                        reference: hz,
                        ..Target::default()
                    };
                    let mut last = Estimate::default();
                    nice_assert_no_alloc::assert_no_alloc(|| {
                        for i in 0..(rate * 0.6) as usize {
                            let sample = 0.2 * (TAU * hz * i as f32 / rate).sin();
                            if detector.push([sample, -sample]) {
                                last = detector.analyze(-55.0, 0.85);
                            }
                        }
                    });
                    let cents = 1200.0 * (last.frequency / hz).log2();
                    assert!(
                        cents.abs() < 10.0,
                        "{mode:?}, {target_mode:?}, rate={rate}, hz={hz}: {last:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn pitch_grid_covers_both_frequency_limits() {
    let mut observations = candidates::Observations::<760>::new(EXTENDED_MIN_HZ, MAX_HZ);
    observations.add(EXTENDED_MIN_HZ, 1.0, 1.0);
    observations.add(MAX_HZ, 1.0, 1.0);
    assert_eq!(observations.frequency[0], EXTENDED_MIN_HZ);
    assert_eq!(observations.frequency[759], MAX_HZ);
    let periodic_top = (120.0 * (PERIODIC_MAX_HZ / EXTENDED_MIN_HZ).log2()).round() as usize;
    assert!(periodic_top < 640);
}

#[test]
fn extended_range_keeps_the_upper_pitch_limit() {
    for rate in [8000.0, 23999.0, 48000.0, 192000.0] {
        let mut detector = Detector::new(rate);
        detector.configure_range(DetectionRange::Low);
        let mut last = Estimate::default();
        for i in 0..(rate * 0.5) as usize {
            let sample = 0.2 * (TAU * MAX_HZ * i as f32 / rate).sin();
            if detector.push([sample; 2]) {
                last = detector.analyze(-55.0, 0.85);
            }
        }
        let cents = 1200.0 * (last.frequency / MAX_HZ).log2();
        assert!(cents.abs() < 12.0, "rate={rate}: {last:?}");
    }
}

#[test]
fn range_switches_restore_standard_window_and_latency_without_realtime_allocations() {
    use crate::{engine::Engine, lookahead::LookaheadTime};

    for (rate, standard_age, low_age) in [(8000.0, 1407, 2431), (48000.0, 5631, 9727)] {
        let mut engine = Engine::new(rate);
        assert_eq!(engine.detector.window_samples(), 1024);
        engine.configure_lookahead(true, LookaheadTime::Minimum);
        assert_eq!(engine.latency_samples(), standard_age);
        nice_assert_no_alloc::assert_no_alloc(|| {
            for range in [
                DetectionRange::Low,
                DetectionRange::Standard,
                DetectionRange::Low,
            ] {
                engine.configure_detection_range(range);
                assert_eq!(engine.estimate.frequency, 0.0);
                assert_eq!(
                    engine.detector.window_samples(),
                    if range == DetectionRange::Low {
                        2048
                    } else {
                        1024
                    }
                );
                assert_eq!(
                    engine.latency_samples(),
                    if range == DetectionRange::Low {
                        low_age
                    } else {
                        standard_age
                    }
                );
                let sample = [0.1; 2];
                assert_eq!(engine.process(sample, Default::default()), [0.0; 2]);
            }
            engine.configure_lookahead(false, LookaheadTime::Minimum);
            assert_eq!(engine.latency_samples(), 0);
        });
    }
}

#[test]
fn low_range_applies_eq_to_sub_bass_on_main_and_sidechain_inputs() {
    use crate::{
        engine::{Controls, Engine},
        mode::OperatingMode,
    };

    for mode in [OperatingMode::Track, OperatingMode::Sidechain] {
        let mut engine = Engine::new(48000.0);
        engine.configure_detection_range(DetectionRange::Low);
        let controls = Controls {
            mode,
            ..Controls::default()
        };
        let mut powers = [0.0_f64; 2];
        nice_assert_no_alloc::assert_no_alloc(|| {
            for i in 0..48000 {
                let phase = TAU * 30.8677 * i as f32 / 48000.0;
                let sample = 0.2 * phase.sin();
                let output =
                    engine.process_with_sidechain([sample; 2], Some([sample; 2]), controls);
                if i >= 24000 {
                    powers[0] += f64::from(sample).powi(2);
                    powers[1] += f64::from(output[0]).powi(2);
                }
            }
        });
        assert!((engine.estimate.frequency - 30.8677).abs() < 0.1);
        let gain = 10.0 * (powers[1] / powers[0]).log10();
        assert!((gain + 6.0).abs() < 0.2, "{mode:?}: {gain} dB");
    }
}
