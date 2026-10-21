use crate::{
    detector::Detector,
    engine::{Controls, Engine},
    filter::{Coefficients, Svf, response_db},
};
use std::f32::consts::TAU;

#[test]
fn mpm_tracks_tones_across_sample_rates() {
    for rate in [8000.0, 22050.0, 44100.0, 48000.0, 96000.0, 192000.0] {
        for hz in [51.0, 55.0, 110.0, 220.0, 440.0, 880.0, 990.0] {
            let mut detector = Detector::new(rate);
            detector.mode = crate::pitch::DetectionMode::Mpm;
            let mut last = Default::default();

            for i in 0..(rate * 0.3) as usize {
                let sample = 0.2 * (TAU * hz * i as f32 / rate).sin();

                if detector.push([sample, sample]) {
                    last = detector.analyze(-55.0, 0.85);
                }
            }

            let cents = 1200.0 * (last.frequency / hz).log2();

            assert!(
                cents.abs() < 8.0,
                "rate={rate}, hz={hz}, estimate={last:?}, cents={cents}"
            );
            assert!(last.clarity > 0.95, "{last:?}");
        }
    }
}

#[test]
fn stronger_harmonics_and_antiphase_stereo_keep_fundamental() {
    let rate = 48000.0;
    let mut e = Engine::new(rate);

    for i in 0..24000 {
        let phase = TAU * 110.0 * i as f32 / rate;
        let sample = 0.2 * phase.sin() + 0.5 * (2.0 * phase).sin() + 0.2 * (3.0 * phase).sin();
        e.process([sample, -sample], Controls::default());
    }

    assert!(
        (e.estimate.frequency - 110.0).abs() < 1.0,
        "{:?}",
        e.estimate
    );
    assert!((e.frequency - 110.0).abs() < 1.0);
}

#[test]
fn silence_noise_dc_and_gate_do_not_lock() {
    let mut d = Detector::new(48000.0);
    let mut seed = 7u32;

    for kind in 0..4 {
        d.reset();

        for i in 0..16000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let sample = match kind {
                0 => 0.0,
                1 => 0.1,
                2 => 0.2 * (seed as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32,
                _ => 1e-5 * (TAU * 220.0 * i as f32 / 48000.0).sin(),
            };

            if d.push([sample; 2]) && i > 8000 {
                let estimate = d.analyze(-55.0, 0.85);

                assert_eq!(estimate.frequency, 0.0, "kind={kind}, {estimate:?}");
            }
        }
    }
}

#[test]
fn bell_center_gain_matches_requested_decibels() {
    for rate in [8000.0, 44100.0, 48000.0, 192000.0] {
        for hz in [55.0, 220.0, 990.0] {
            for q in [0.3, 3.0, 16.0] {
                for db in [-24.0, -6.0, 0.0, 12.0, 24.0] {
                    let mut filter = Svf::default();
                    let c = Coefficients::bell(rate, hz, q, db);
                    let mut input_power = 0.0_f64;
                    let mut output_power = 0.0_f64;
                    // High-Q boosted bells need a longer settling interval.
                    let settle = (rate
                        * (10.0 * q * 10.0_f32.powf(db / 40.0) / (std::f32::consts::PI * hz))
                            .max(0.3)) as usize;

                    for i in 0..settle + rate as usize / 2 {
                        let input = 0.1 * (TAU * hz * i as f32 / rate).sin();
                        let output = filter.bell(input, c);

                        if i >= settle {
                            input_power += f64::from(input).powi(2);
                            output_power += f64::from(output).powi(2);
                        }
                    }

                    let measured = (10.0 * (output_power / input_power).log10()) as f32;

                    assert!(
                        (measured - db).abs() < 0.06,
                        "rate={rate}, f={hz}, q={q}, requested={db}, actual={measured}"
                    );
                    assert!((response_db(rate, hz, hz, q, db) - measured).abs() < 0.06);
                }
            }
        }
    }
}

#[test]
fn engine_applies_cut_and_boost_at_detected_pitch() {
    for db in [-12.0, 12.0] {
        let mut e = Engine::new(48000.0);

        let mut controls = Controls::default();
        controls.bands[0].gain = db;
        let mut input_power = 0.0_f64;
        let mut output_power = 0.0_f64;

        for i in 0..48000 {
            let input = 0.1 * (TAU * 220.0 * i as f32 / 48000.0).sin();
            let output = e.process([input; 2], controls)[0];

            if i > 24000 {
                input_power += f64::from(input).powi(2);
                output_power += f64::from(output).powi(2);
            }
        }

        let measured = (10.0 * (output_power / input_power).log10()) as f32;

        assert!((measured - db).abs() < 0.1, "{db} => {measured}");
    }
}

#[test]
fn lost_pitch_releases_gain_and_preserves_center() {
    let mut e = Engine::new(48000.0);

    for i in 0..24000 {
        let x = 0.2 * (TAU * 110.0 * i as f32 / 48000.0).sin();
        e.process([x; 2], Controls::default());
    }

    assert!(e.gain < -5.9);

    for _ in 0..24000 {
        e.process([0.0; 2], Controls::default());
    }

    assert_eq!(e.estimate.frequency, 0.0);
    assert!(e.gain.abs() < 0.001);
    assert!((e.frequency - 110.0).abs() < 1.0);
    e.reset();

    assert_eq!(e.estimate.frequency, 0.0);
    assert_eq!(e.gain, 0.0);
    assert_eq!(e.process([0.0; 2], Controls::default()), [0.0; 2]);
}

#[test]
fn bypass_reaches_exact_dry_signal() {
    let mut e = Engine::new(48000.0);

    let controls = Controls {
        output_db: 12.0,
        bypass: true,
        ..Controls::default()
    };

    for i in 0..8000 {
        let x = 0.1 * (TAU * 220.0 * i as f32 / 48000.0).sin();
        let y = e.process([x, -x], controls);

        if i > 500 {
            assert!((y[0] - x).abs() < 1e-7 && (y[1] + x).abs() < 1e-7);
        }
    }
}

#[test]
fn rapid_filter_modulation_stays_finite() {
    for rate in [8000.0, 48000.0, 192000.0] {
        let mut filter = Svf::default();

        for i in 0..200000 {
            let t = (i % 2000) as f32 / 1999.0;
            let hz = 50.0 * 20.0_f32.powf(t);
            let db = 24.0 * (TAU * t).sin();
            let c = Coefficients::bell(rate, hz, 16.0, db);
            let y = filter.bell(0.1 * (i as f32 * 0.075).sin(), c);

            assert!(
                y.is_finite() && y.abs() < 10.0,
                "rate={rate}, sample={i}, output={y}"
            );
        }
    }
}

#[test]
fn default_plugin_exports_unique_parameter_ids() {
    use nice_plug::prelude::*;
    let plugin = crate::F0Lock::default();
    let map = plugin.params.param_map();

    assert_eq!(map.len(), 206);
    let mut ids: Vec<_> = map.iter().map(|v| &v.0).collect();
    ids.sort();
    ids.dedup();

    assert_eq!(ids.len(), 206);
}

#[test]
fn psi_is_the_default_algorithm_in_both_parameter_banks_and_dsp() {
    use crate::{mode::OperatingMode, pitch::DetectionMode};

    let params = crate::params::F0Params::default();

    for mode in [OperatingMode::Track, OperatingMode::Sidechain] {
        assert_eq!(
            params.for_mode(mode).detection.value(),
            DetectionMode::SpectralMelody
        );
    }

    assert_eq!(DetectionMode::default(), DetectionMode::SpectralMelody);
    assert_eq!(Controls::default().detection, DetectionMode::SpectralMelody);
    assert_eq!(Detector::new(48000.0).mode, DetectionMode::SpectralMelody);
    assert_eq!(DetectionMode::default().label(), "F0LOCK Ψ");
}

#[test]
fn realtime_dsp_does_not_allocate() {
    let mut e = Engine::new(48000.0);
    e.visualize = true;
    nice_assert_no_alloc::assert_no_alloc(|| {
        for i in 0..24000 {
            let x = 0.1 * (TAU * 220.0 * i as f32 / 48000.0).sin();
            let y = e.process([x, -x], Controls::default());

            assert!(y.iter().all(|v| v.is_finite()));
        }
    });
}

#[test]
fn dc_in_one_channel_does_not_hide_pitch_in_the_other() {
    let mut e = Engine::new(48000.0);

    for i in 0..24000 {
        let x = 0.02 * (TAU * 220.0 * i as f32 / 48000.0).sin();
        e.process([0.8, x], Controls::default());
    }

    assert!((e.estimate.frequency - 220.0).abs() < 1.0);
}

#[test]
fn slow_tracking_at_high_sample_rate_converges_without_roundoff_stall() {
    let mut e = Engine::new(192000.0);

    let controls = Controls {
        tracking_ms: 300.0,
        ..Controls::default()
    };

    for i in 0..768000 {
        let x = 0.1 * (TAU * 440.0 * i as f32 / 192000.0).sin();
        e.process([x; 2], controls);
    }

    let cents = 1200.0 * (e.frequency / e.estimate.frequency).log2();

    assert!(
        cents.abs() < 0.1,
        "Frequency smoothing stalled at {cents} cents"
    );
}
