use super::*;
use crate::detector::{Detector, Estimate};
use std::f32::consts::TAU;

fn render(
    detector: &mut Detector,
    rate: f32,
    seconds: f32,
    signal: impl Fn(f32) -> f32,
) -> Estimate {
    let mut last = Estimate::default();

    nice_assert_no_alloc::assert_no_alloc(|| {
        for i in 0..(rate * seconds) as usize {
            let sample = signal(i as f32 / rate);

            if detector.push([sample, -sample]) {
                last = detector.analyze(-55.0, 0.85);
            }
        }
    });

    last
}

#[test]
fn bass_all_backends_cover_twenty_to_three_hundred_hz() {
    for rate in [8000.0, 23999.0, 44100.0, 48000.0, 192000.0] {
        for mode in DetectionMode::ALL {
            let mut detector = Detector::new(rate);
            detector.configure_range(DetectionRange::Bass);
            detector.mode = mode;

            for hz in [20.0, 27.5, 55.0, 110.0, 220.0, 300.0] {
                detector.reset();
                let result = render(&mut detector, rate, 0.8, |t| 0.2 * (TAU * hz * t).sin());
                let cents = 1200.0 * (result.frequency / hz).log2();

                assert!(
                    cents.abs() < 12.0,
                    "{mode:?}, rate={rate}, hz={hz}: {result:?}"
                );
                assert!((BASS_MIN_HZ..=BASS_MAX_HZ).contains(&result.frequency));
            }
        }
    }
}

#[test]
fn bass_target_modes_include_low_fundamentals_and_exclude_outside_ranges() {
    for mode in DetectionMode::ALL {
        for target_mode in TargetMode::ALL {
            let mut detector = Detector::new(48000.0);
            detector.configure_range(DetectionRange::Bass);
            detector.mode = mode;
            detector.target = Target {
                mode: target_mode,
                minimum: 20.0,
                maximum: 35.0,
                reference: 27.5,
                tolerance: 2.0,
            };
            let result = render(&mut detector, 48000.0, 0.8, |t| {
                0.2 * (TAU * 27.5 * t).sin()
            });
            assert!(
                (result.frequency - 27.5).abs() < 0.2,
                "{mode:?}, {target_mode:?}: {result:?}"
            );

            detector.reset();
            detector.target.mode = TargetMode::Range;
            detector.target.minimum = 500.0;
            detector.target.maximum = 1000.0;
            let result = render(&mut detector, 48000.0, 0.6, |t| {
                0.2 * (TAU * 55.0 * t).sin()
            });
            assert_eq!(
                result.frequency, 0.0,
                "Out-of-profile target must not select a pitch"
            );
        }
    }
}

#[test]
fn bass_psi_retains_harmonics_above_the_pitch_ceiling() {
    let mut detector = Detector::new(48000.0);
    detector.configure_range(DetectionRange::Bass);
    let result = render(&mut detector, 48000.0, 0.8, |t| {
        let phase = TAU * 110.0 * t;
        0.01 * phase.sin()
            + 0.12 * (phase * 3.0).sin()
            + 0.10 * (phase * 4.0).sin()
            + 0.08 * (phase * 5.0).sin()
    });

    assert!((result.frequency - 110.0).abs() < 1.0, "{result:?}");
}

#[test]
fn bass_anti_alias_filter_rejects_high_frequency_images() {
    for mode in DetectionMode::ALL {
        let mut detector = Detector::new(48000.0);
        detector.configure_range(DetectionRange::Bass);
        detector.mode = mode;
        // At the 4 kHz analysis rate, an unfiltered 3945 Hz signal aliases to 55 Hz.
        let result = render(&mut detector, 48000.0, 0.8, |t| {
            0.2 * (TAU * 3945.0 * t).sin()
        });

        assert_eq!(result.frequency, 0.0, "{mode:?}: {result:?}");
    }
}

#[test]
fn bass_switch_updates_analysis_rate_and_lookahead_without_allocations() {
    use crate::{engine::Engine, lookahead::LookaheadTime};

    let mut engine = Engine::new(48000.0);
    engine.configure_lookahead(true, LookaheadTime::Minimum);

    nice_assert_no_alloc::assert_no_alloc(|| {
        for (range, rate, age) in [
            (DetectionRange::Bass, 4000.0, 14591),
            (DetectionRange::Low, 12000.0, 9727),
            (DetectionRange::Standard, 12000.0, 5631),
            (DetectionRange::Bass, 4000.0, 14591),
        ] {
            engine.configure_detection_range(range);
            assert_eq!(engine.detector.rate, rate);
            assert_eq!(engine.latency_samples(), age);
            assert_eq!(engine.estimate.frequency, 0.0);
        }
    });
}

#[test]
fn bass_glides_do_not_freeze_the_temporal_trackers() {
    for mode in [
        DetectionMode::MpmTracking,
        DetectionMode::Pyin,
        DetectionMode::SpectralMelody,
    ] {
        let mut detector = Detector::new(48000.0);
        detector.configure_range(DetectionRange::Bass);
        detector.mode = mode;
        // Descending 808-like pitch, followed by a sustained low note.
        let result = render(&mut detector, 48000.0, 1.2, |t| {
            let phase = if t < 0.7 {
                TAU * (110.0 * t - 55.0 * t * t / 1.4)
            } else {
                TAU * (57.75 + 55.0 * (t - 0.7))
            };
            0.2 * phase.sin() + 0.06 * (2.0 * phase).sin()
        });

        assert!(
            (result.frequency - 55.0).abs() < 1.0,
            "{mode:?}: {result:?}"
        );
    }
}

#[test]
fn bass_ratio_can_apply_eq_above_three_hundred_hz_with_lookahead_and_sidechain() {
    use crate::{
        engine::{Controls, Engine},
        lookahead::LookaheadTime,
        mode::OperatingMode,
    };

    for mode in [OperatingMode::Track, OperatingMode::Sidechain] {
        let mut engine = Engine::new(48000.0);
        engine.configure_detection_range(DetectionRange::Bass);
        engine.configure_lookahead(true, LookaheadTime::Ms150);
        let mut controls = Controls {
            mode,
            ..Controls::default()
        };
        let hz = if mode == OperatingMode::Track {
            110.0
        } else {
            55.0
        };
        controls.bands[0].ratio = 440.0 / hz;
        let mut input = [0.0_f64; 2];
        let mut output = [0.0_f64; 2];

        nice_assert_no_alloc::assert_no_alloc(|| {
            for i in 0..72000 {
                let t = i as f32 / 48000.0;
                let phase = TAU * 440.0 * t;
                let sample = 0.2 * (TAU * 110.0 * t).sin() + 0.1 * phase.sin();
                let sidechain = 0.2 * (TAU * hz * t).sin();
                let wet =
                    engine.process_with_sidechain([sample; 2], Some([sidechain; 2]), controls);

                if i >= 48000 {
                    for (channel, basis) in [phase.sin(), phase.cos()].into_iter().enumerate() {
                        input[channel] += f64::from(sample * basis);
                        output[channel] += f64::from(wet[0] * basis);
                    }
                }
            }
        });

        assert!(
            (engine.estimate.frequency - hz).abs() < 0.5,
            "{mode:?}: {:?}",
            engine.estimate
        );
        assert!((engine.bands[0].frequency - 440.0).abs() < 2.0);
        let db = 20.0 * (output[0].hypot(output[1]) / input[0].hypot(input[1])).log10();
        assert!((db + 6.0).abs() < 0.25, "{mode:?}: {db} dB");
    }
}
