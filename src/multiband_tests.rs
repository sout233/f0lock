use crate::{
    band::{BAND_COUNT, BandControl},
    engine::{Controls, Engine},
    filter::{Coefficients, FilterKind, Svf},
};
use std::f32::consts::TAU;

#[test]
fn all_filter_types_match_their_complex_frequency_response() {
    let rate = 48000.0;

    for kind in FilterKind::ALL {
        for db in [-12.0, 12.0] {
            for hz in [40.0, 300.0, 3000.0, 19000.0] {
                let c = Coefficients::for_kind(rate, 300.0, 0.70710677, db, kind);
                let mut filter = Svf::default();
                let mut input = 0.0_f64;

                let mut output = 0.0_f64;

                for i in 0..48000 {
                    let x = 0.1 * (TAU * hz * i as f32 / rate).sin();
                    let y = filter.process(x, c);

                    if i > 24000 {
                        input += f64::from(x).powi(2);
                        output += f64::from(y).powi(2);
                    }
                }

                let measured = (output / input).sqrt() as f32;
                let expected = c.response(rate, hz).norm();

                assert!(
                    (measured - expected).abs() < 0.003 * expected.max(1.0),
                    "{kind:?}, gain={db}, test frequency={hz}: expected {expected}, measured {measured}"
                );
            }
        }
    }
}

#[test]
fn shelf_and_cut_filter_asymptotes_are_correct() {
    let make = |kind| Coefficients::for_kind(48000.0, 500.0, 0.70710677, 12.0, kind);
    let gain = 10.0_f32.powf(12.0 / 20.0);

    assert!((make(FilterKind::LowShelf).response(48000.0, 0.0).norm() - gain).abs() < 1e-5);
    assert!((make(FilterKind::HighShelf).response(48000.0, 0.0).norm() - 1.0).abs() < 1e-5);
    assert!(make(FilterKind::Highpass).response(48000.0, 0.0).norm() < 1e-6);
    assert!((make(FilterKind::Lowpass).response(48000.0, 0.0).norm() - 1.0).abs() < 1e-6);
    assert!(make(FilterKind::Notch).response(48000.0, 500.0).norm() < 1e-6);
}

#[test]
fn two_tracking_bands_cascade_and_keep_their_ratio() {
    let mut engine = Engine::new(48000.0);

    let mut controls = Controls::default();
    controls.bands[1] = BandControl {
        exists: true,
        gain: -6.0,
        ..BandControl::default()
    };

    let mut input_power = 0.0_f64;
    let mut output_power = 0.0_f64;

    for i in 0..48000 {
        let x = 0.1 * (TAU * 220.0 * i as f32 / 48000.0).sin();
        let y = engine.process([x; 2], controls)[0];

        if i > 24000 {
            input_power += f64::from(x).powi(2);
            output_power += f64::from(y).powi(2);
        }
    }

    let db = 10.0 * (output_power / input_power).log10();

    assert!((db + 12.0).abs() < 0.1, "Expected -12 dB, got {db}");
    controls.bands[1].ratio = 3.0;

    for i in 0..48000 {
        let x = 0.1 * (TAU * 110.0 * i as f32 / 48000.0).sin();
        engine.process([x; 2], controls);
    }

    assert!((engine.frequency - 110.0).abs() < 0.1);
    assert!((engine.bands[1].frequency / engine.frequency - 3.0).abs() < 0.001);
}

#[test]
fn all_eight_bands_and_type_changes_are_allocation_free_and_release_to_dry() {
    let mut engine = Engine::new(48000.0);

    let mut controls = Controls {
        bands: std::array::from_fn(|i| BandControl {
            exists: true,
            ratio: (i + 1) as f32,
            gain: -3.0,
            q: 0.70710677,
            kind: FilterKind::ALL[i % 6],
            ..BandControl::default()
        }),
        ..Controls::default()
    };
    nice_assert_no_alloc::assert_no_alloc(|| {
        for i in 0..96000 {
            if i % 4000 == 0 {
                for (j, b) in controls.bands.iter_mut().enumerate() {
                    b.kind = FilterKind::ALL[(i / 4000 + j) % 6];
                }
            }

            let x = 0.1 * (TAU * 220.0 * i as f32 / 48000.0).sin();
            let y = engine.process([x, -x], controls);

            assert!(y.iter().all(|v| v.is_finite() && v.abs() < 10.0));
        }

        for _ in 0..24000 {
            engine.process([0.0; 2], controls);
        }
    });
    assert!(engine.bands.iter().all(|b| b.wet == 0.0));
    controls.bands.fill(BandControl::default());

    assert_eq!(BAND_COUNT, 8);

    for i in 0..4000 {
        let input = [i as f32 * 0.0001, -0.01];

        assert_eq!(engine.process(input, controls), input);
    }
}
