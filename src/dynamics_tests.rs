use crate::{
    band::{Band, BandControl},
    dynamics::{DynamicControl, Dynamics},
    filter::FilterKind,
};
use std::f32::consts::TAU;

#[test]
fn dynamics_detects_the_selected_band_and_links_stereo_power() {
    for (frequency, amplitude, antiphase, expected) in [
        (1000.0, 0.5, false, -6.0),
        (1000.0, 0.5, true, -6.0),
        (1000.0, 0.001, false, 0.0),
        (8000.0, 0.5, false, 0.0),
    ] {
        let mut dynamic = Dynamics::new(48000.0);
        for i in 0..48000 {
            let x = amplitude * (TAU * frequency * i as f32 / 48000.0).sin();
            dynamic.process(
                [x, if antiphase { -x } else { x }],
                DynamicControl {
                    enabled: true,
                    ..Default::default()
                },
                48000.0,
                1000.0,
                3.0,
                FilterKind::Peak,
            );
        }
        assert!(
            (dynamic.gain - expected).abs() < 0.02,
            "{frequency} Hz: {}",
            dynamic.gain
        );
    }
}

#[test]
fn dynamic_attack_release_and_disable_preserve_continuity() {
    let mut dynamic = Dynamics::new(48000.0);
    let mut control = DynamicControl {
        enabled: true,
        attack_ms: 50.0,
        release_ms: 200.0,
        ..Default::default()
    };
    for i in 0..48000 {
        let x = 0.5 * (TAU * 1000.0 * i as f32 / 48000.0).sin();
        dynamic.process([x; 2], control, 48000.0, 1000.0, 3.0, FilterKind::Peak);
        if i == 2400 {
            assert!(dynamic.gain < -2.5 && dynamic.gain > -4.5);
        }
    }
    let previous = dynamic.gain;
    control.enabled = false;
    dynamic.process([0.0; 2], control, 48000.0, 1000.0, 3.0, FilterKind::Peak);
    assert!((dynamic.gain - previous).abs() < 0.001);
    for _ in 0..9600 {
        dynamic.process([0.0; 2], control, 48000.0, 1000.0, 3.0, FilterKind::Peak);
    }
    assert!((dynamic.gain - previous / std::f32::consts::E).abs() < 0.02);
}

#[test]
fn dynamic_gain_is_applied_to_audio_and_reported_in_the_response() {
    let mut band = Band::new(48000.0);
    let control = BandControl {
        exists: true,
        dynamics: DynamicControl {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut input_power = 0.0;
    let mut output_power = 0.0;
    for i in 0..48000 {
        let x = 0.5 * (TAU * 1000.0 * i as f32 / 48000.0).sin();
        let y = band.process(
            [x; 2],
            [x; 2],
            control,
            1000.0,
            true,
            48000.0,
            1.0 - (-1.0_f32 / 480.0).exp(),
            [1.0; 2],
        )[0];
        if i >= 36000 {
            input_power += x * x;
            output_power += y * y;
        }
    }
    assert!((10.0_f32 * (output_power / input_power).log10() + 6.0).abs() < 0.05);
    assert!((band.values()[9] + 6.0).abs() < 0.02);
    assert!((20.0 * band.coefficients.response(48000.0, 1000.0).norm().log10() + 6.0).abs() < 0.05);
}

#[test]
fn dynamic_detector_uses_the_lookahead_aligned_source() {
    use crate::{
        engine::{Controls, Engine},
        lookahead::LookaheadTime,
        mode::OperatingMode,
    };
    for duration in [None, Some(LookaheadTime::Ms200)] {
        let mut engine = Engine::new(48000.0);
        if let Some(duration) = duration {
            engine.configure_lookahead(true, duration);
        }
        let mut controls = Controls {
            mode: OperatingMode::Sidechain,
            ..Default::default()
        };
        controls.bands[0].gain = 0.0;
        controls.bands[0].dynamics.enabled = true;
        for i in 0..48000 {
            let sc = 0.5 * (TAU * 440.0 * i as f32 / 48000.0).sin();
            engine.process_with_sidechain([0.0; 2], Some([sc; 2]), controls);
        }
        assert!((engine.bands[0].values()[9] + 6.0).abs() < 0.1);
    }
}

#[test]
fn shelves_detect_their_own_region_and_gainless_filters_ignore_dynamics() {
    for (kind, hz, expected) in [
        (FilterKind::LowShelf, 100.0, -6.0),
        (FilterKind::LowShelf, 8000.0, 0.0),
        (FilterKind::HighShelf, 8000.0, -6.0),
        (FilterKind::HighShelf, 100.0, 0.0),
        (FilterKind::Notch, 1000.0, 0.0),
        (FilterKind::Highpass, 1000.0, 0.0),
        (FilterKind::Lowpass, 1000.0, 0.0),
    ] {
        let mut dynamic = Dynamics::new(48000.0);
        for i in 0..48000 {
            let x = 0.5 * (TAU * hz * i as f32 / 48000.0).sin();
            dynamic.process(
                [x; 2],
                DynamicControl {
                    enabled: true,
                    ..Default::default()
                },
                48000.0,
                1000.0,
                3.0,
                kind,
            );
        }
        assert!(
            (dynamic.gain - expected).abs() < 0.02,
            "{kind:?} at {hz}: {}",
            dynamic.gain
        );
    }
}

#[test]
fn eight_dynamic_points_remain_finite_without_realtime_allocations() {
    use crate::engine::{Controls, Engine};
    let mut engine = Engine::new(48000.0);
    let mut controls = Controls::default();
    for (i, band) in controls.bands.iter_mut().enumerate() {
        band.exists = true;
        band.ratio = (i + 1) as f32;
        band.dynamics.enabled = true;
    }
    nice_assert_no_alloc::assert_no_alloc(|| {
        for i in 0..48000 {
            let x = 0.5 * (TAU * 440.0 * i as f32 / 48000.0).sin();
            if i % 8000 == 0 {
                for band in &mut controls.bands {
                    band.kind = FilterKind::ALL[i / 8000];
                }
            }
            let y = engine.process([x, -x], controls);
            assert!(y.iter().all(|s| s.is_finite() && s.abs() < 10.0));
        }
    });
}
