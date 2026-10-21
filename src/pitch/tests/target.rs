use super::*;
use crate::{
    detector::Detector,
    engine::{Controls, Engine},
};
use std::f32::consts::TAU;

fn render(mode: DetectionMode, target: Target, signal: impl Fn(f32) -> f32) -> Vec<f32> {
    let mut detector = Detector::new(48000.0);
    detector.mode = mode;
    detector.target = target;
    let mut results = Vec::new();

    for i in 0..24000 {
        let x = signal(i as f32 / 48000.0);
        if detector.push([x; 2]) {
            let estimate = detector.analyze(-55.0, 0.85);
            if i > 12000 {
                results.push(estimate.frequency);
            }
        }
    }
    results
}

#[test]
fn range_tracks_the_allowed_voice_and_rejects_unsupported_targets() {
    let range = Target {
        mode: TargetMode::Range,
        minimum: 360.0,
        maximum: 520.0,
        ..Target::default()
    };

    for mode in DetectionMode::ALL {
        let allowed = render(mode, range, |t| {
            0.3 * (TAU * 440.0 * t).sin() + 0.04 * (TAU * 220.0 * t).sin()
        });
        assert!(
            allowed.iter().all(|hz| (*hz - 440.0).abs() < 3.0),
            "{mode:?}: {allowed:?}"
        );

        let excluded = render(mode, range, |t| 0.3 * (TAU * 200.0 * t).sin());
        assert!(
            excluded.iter().all(|hz| *hz == 0.0),
            "Fabricated target in {mode:?}: {excluded:?}"
        );

        let invalid = Target {
            minimum: 520.0,
            maximum: 360.0,
            ..range
        };
        assert!(
            render(mode, invalid, |t| 0.3 * (TAU * 440.0 * t).sin())
                .iter()
                .all(|hz| *hz == 0.0)
        );
    }
}

#[test]
fn range_is_applied_before_yin_threshold_candidate_selection() {
    let mut curve = [1.0; 32];
    curve[5] = 0.12;
    curve[10] = 0.02;
    let target = Target {
        mode: TargetMode::Range,
        minimum: 180.0,
        maximum: 220.0,
        ..Target::default()
    };
    let mut observations = Observations::default();
    pyin_observations(&curve, &curve, 1000.0, &mut observations, target);
    let mass: f32 = observations.probability.iter().sum();

    assert!(
        mass > 0.2,
        "Outside candidate swallowed inside candidate: {mass}"
    );
    for i in 0..BINS {
        if observations.probability[i] > 0.0 {
            assert!((observations.frequency[i] - 200.0).abs() < 0.1);
        }
    }
}

#[test]
fn reference_biases_candidates_without_inventing_voicing_or_fixing_the_output_pitch() {
    let reference = Target {
        mode: TargetMode::Reference,
        reference: 400.0,
        tolerance: 6.0,
        ..Target::default()
    };
    let mut observations = Observations::default();
    observations.add(300.0, 0.95, 0.4);
    observations.add(400.0, 0.95, 0.4);
    reference.apply_preference(&mut observations);
    assert!((observations.probability.iter().sum::<f32>() - 0.8).abs() < 1e-6);
    let best = (0..BINS)
        .max_by(|&a, &b| observations.probability[a].total_cmp(&observations.probability[b]))
        .unwrap();
    assert_eq!(observations.frequency[best], 400.0);

    for mode in DetectionMode::ALL {
        for tone in [350.0, 450.0] {
            let result = render(mode, reference, |t| 0.3 * (TAU * tone * t).sin());
            assert!(
                result.iter().all(|hz| (*hz - tone).abs() < 3.0),
                "{mode:?} reference fixed the output: {result:?}"
            );
        }

        assert!(render(mode, reference, |_| 0.0).iter().all(|hz| *hz == 0.0));
        let unsupported = Target {
            reference: 500.0,
            tolerance: 1.0,
            ..reference
        };
        assert!(
            render(mode, unsupported, |t| 0.3 * (TAU * 220.0 * t).sin())
                .iter()
                .all(|hz| *hz == 0.0)
        );
    }
}

#[test]
fn target_automation_and_mode_changes_do_not_allocate_or_return_old_pending_pitches() {
    let mut engine = Engine::new(48000.0);
    let mut controls = Controls {
        detection: DetectionMode::Pyin,
        ..Controls::default()
    };

    nice_assert_no_alloc::assert_no_alloc(|| {
        for i in 0..48000 {
            if i == 24000 {
                controls.target = Target {
                    mode: TargetMode::Range,
                    minimum: 500.0,
                    maximum: 600.0,
                    ..Target::default()
                };
            }
            if i == 36000 {
                controls.target = Target {
                    mode: TargetMode::Reference,
                    reference: 220.0,
                    tolerance: 3.0,
                    ..Target::default()
                };
            }

            if i >= 36000 {
                controls.target.reference = 220.0 + (i - 36000) as f32 / 1200.0;
            }

            let x = 0.3 * (TAU * 220.0 * i as f32 / 48000.0).sin();
            engine.process([x; 2], controls);
            if (25024..36000).contains(&i) {
                assert_eq!(engine.estimate.frequency, 0.0);
            }
        }
    });
    assert!((engine.estimate.frequency - 220.0).abs() < 1.0);
}
