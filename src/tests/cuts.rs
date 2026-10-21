use crate::{
    band::{Band, BandControl},
    filter::{Coefficients, CutSlope, FilterKind},
};
use std::f32::consts::{PI, TAU};

#[test]
fn default_cut_slope_preserves_legacy_resonance() {
    for kind in [FilterKind::Highpass, FilterKind::Lowpass] {
        for q in [0.3, 3.0, 16.0] {
            let mut band = Band::new(48000.0);
            let control = BandControl {
                exists: true,
                kind,
                q,
                ..BandControl::default()
            };
            band.process(
                [0.0; 2], [0.0; 2], control, 440.0, true, 48000.0, 0.002, [1.0; 2],
            );
            let previous = Coefficients::for_kind(48000.0, 440.0, q, 0.0, kind);
            for hz in [100.0, 440.0, 2000.0] {
                assert!(
                    (band.coefficients.response(48000.0, hz) - previous.response(48000.0, hz))
                        .norm()
                        < 1e-5
                );
            }
        }
    }
}

#[test]
fn every_cut_slope_matches_butterworth_response_in_audio_and_telemetry() {
    for rate in [44100.0, 96000.0] {
        for kind in [FilterKind::Highpass, FilterKind::Lowpass] {
            for slope in CutSlope::ALL {
                for hz in [400.0, 800.0, 1600.0] {
                    let mut band = Band::new(rate);
                    let control = BandControl {
                        exists: true,
                        q: std::f32::consts::FRAC_1_SQRT_2,
                        kind,
                        slope,
                        ..BandControl::default()
                    };
                    let mut input_power = 0.0_f64;
                    let mut output_power = 0.0_f64;

                    for i in 0..rate as usize {
                        let x =
                            0.1 * (TAU as f64 * hz as f64 * i as f64 / rate as f64).sin() as f32;
                        let y = band.process(
                            [x, -x],
                            [x, -x],
                            control,
                            800.0,
                            true,
                            rate,
                            0.002,
                            [1.0; 2],
                        );
                        assert!((y[0] + y[1]).abs() < 1e-6);

                        if i > rate as usize / 2 {
                            input_power += f64::from(x).powi(2);
                            output_power += f64::from(y[0]).powi(2);
                        }
                    }

                    let r = ((PI as f64 * hz as f64 / rate as f64).tan()
                        / (PI as f64 * 800.0 / rate as f64).tan())
                        as f32;
                    let exponent =
                        if kind == FilterKind::Lowpass { 2 } else { -2 } * slope.order() as i32;
                    let expected = 1.0 / (1.0 + r.powi(exponent)).sqrt();
                    let measured = (output_power / input_power).sqrt() as f32;
                    let measured_db_error = 20.0 * (measured / expected).log10();
                    assert!(
                        measured_db_error.abs() < 0.1,
                        "rate={rate}, {kind:?}, {slope:?}, hz={hz}: measured={measured}, expected={expected}"
                    );

                    let values = band.values();
                    let mut response =
                        Coefficients::new(values[4], values[5], [values[6], values[7], values[8]])
                            .response(rate, hz);
                    for section in values[10..].chunks_exact(6) {
                        if section[5] > 0.0 {
                            response *= Coefficients::new(
                                section[0],
                                section[1],
                                [section[2], section[3], section[4]],
                            )
                            .response(rate, hz);
                        }
                    }
                    assert!(
                        (response.norm() - expected).abs() < 2e-5,
                        "Telemetry does not match audio: {kind:?}, {slope:?}, hz={hz}"
                    );
                }
            }
        }
    }
}

#[test]
fn cut_type_and_slope_automation_remain_finite_and_allocation_free() {
    for rate in [8000.0, 48000.0, 192000.0] {
        let mut band = Band::new(rate);
        let alpha = 1.0 - (-1.0 / (0.01 * rate)).exp();
        nice_assert_no_alloc::assert_no_alloc(|| {
            for i in 0..100000 {
                let control = BandControl {
                    exists: true,
                    q: 16.0,
                    slope: CutSlope::ALL[(i / 4000) % 9],
                    kind: FilterKind::ALL[(i / 7000) % 6],
                    ..BandControl::default()
                };
                let hz = 50.0 * 20.0_f32.powf((i % 5000) as f32 / 5000.0);
                let x = 0.01 * (i as f32 * 0.1).sin();
                let y = band.process([x; 2], [x; 2], control, hz, true, rate, alpha, [0.01; 2]);
                assert!(
                    y.iter().all(|x| x.is_finite() && x.abs() < 100.0),
                    "rate={rate}, sample={i}, output={y:?}"
                );
            }
        });
    }
}
