use super::*;
use crate::{
    detector::Detector,
    engine::{Controls, Engine},
};
use std::f32::consts::TAU;

fn feed(detector: &mut Detector, seconds: f32, signal: impl Fn(f32) -> f32) -> Vec<f32> {
    let mut pitches = Vec::new();
    let rate = 48000.0;

    for i in 0..(seconds * rate) as usize {
        let sample = signal(i as f32 / rate);

        if detector.push([sample, -sample]) {
            pitches.push(detector.analyze(-55.0, 0.85).frequency);
        }
    }

    pitches
}

#[test]
fn probabilistic_thresholding_retains_competing_yin_minima() {
    let mut curve = [1.0; 32];
    curve[5] = 0.22;
    curve[10] = 0.08;
    let mut observations = Observations::default();
    pyin_observations(&curve, &curve, 1000.0, &mut observations, Target::default());

    let active: Vec<_> = observations
        .probability
        .iter()
        .enumerate()
        .filter(|(_, p)| **p > 0.0)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(active.len(), 2);
    assert!(observations.frequency[active[0]] < observations.frequency[active[1]]);
    assert!(observations.probability[active[0]] > observations.probability[active[1]]);

    let total: f32 = observations.probability.iter().sum();
    assert!(total > 0.4 && total < 1.0, "Voiced probability: {total}");
}

#[test]
fn temporal_evidence_rejects_transient_octave_errors_but_accepts_new_notes() {
    for ratio in [0.25, 0.5, 2.0, 4.0] {
        let original = 220.0;
        let target = original * ratio;
        let mut tracker = Tracker::<PERIODIC_BINS>::new(128.0 / 12000.0);
        let mut stable = Observations::default();
        stable.add(original, 0.99, 0.99);

        for _ in 0..12 {
            tracker.process(&stable, 0.85, 0);
        }

        let mut ambiguous = Observations::default();
        ambiguous.add(original, 0.93, 0.25);
        ambiguous.add(target, 0.99, 0.74);

        for _ in 0..2 {
            let (hz, _) = tracker.process(&ambiguous, 0.85, 0);
            assert!((hz - original).abs() < 0.1, "Transient ratio {ratio}: {hz}");
        }

        let mut changed = Observations::default();
        changed.add(target, 0.99, 0.99);
        let mut hz = 0.0;

        for _ in 0..12 {
            hz = tracker.process(&changed, 0.85, 0).0;
        }

        assert!(
            (hz - target).abs() < 0.1,
            "Real note change ratio {ratio}: {hz}"
        );
    }
}

#[test]
fn detector_modes_track_tones_harmonics_and_reacquire_after_silence() {
    for mode in DetectionMode::ALL {
        let mut detector = Detector::new(48000.0);
        detector.mode = mode;
        let pitches = feed(&mut detector, 0.4, |t| {
            let phase = TAU * 110.0 * t;
            0.08 * phase.sin() + 0.2 * (2.0 * phase).sin() + 0.1 * (3.0 * phase).sin()
        });
        assert!(
            (pitches.last().unwrap() - 110.0).abs() < 1.0,
            "{mode:?}: {pitches:?}"
        );

        let silence = feed(&mut detector, 0.25, |_| 0.0);
        assert_eq!(*silence.last().unwrap(), 0.0);

        let restarted = feed(&mut detector, 0.35, |t| 0.2 * (TAU * 440.0 * t).sin());
        assert!(
            (restarted.last().unwrap() - 440.0).abs() < 2.0,
            "{mode:?}: {restarted:?}"
        );
    }
}

#[test]
fn enhanced_modes_follow_real_two_octave_note_changes_without_silence() {
    for mode in [
        DetectionMode::MpmTracking,
        DetectionMode::Pyin,
        DetectionMode::SpectralMelody,
    ] {
        let mut detector = Detector::new(48000.0);
        detector.mode = mode;

        for hz in [110.0, 440.0, 110.0, 349.2] {
            let pitches = feed(&mut detector, 0.4, |t| 0.2 * (TAU * hz * t).sin());
            assert!(
                (pitches.last().unwrap() - hz).abs() < 2.0,
                "{mode:?}, {hz}: {pitches:?}"
            );
        }
    }
}

#[test]
fn mode_switches_and_all_backends_are_allocation_free_in_audio_processing() {
    let mut engine = Engine::new(48000.0);

    nice_assert_no_alloc::assert_no_alloc(|| {
        for mode in [
            DetectionMode::MpmTracking,
            DetectionMode::Pyin,
            DetectionMode::Mpm,
        ] {
            let controls = Controls {
                detection: mode,
                ..Controls::default()
            };

            for i in 0..16000 {
                let x = 0.1 * (TAU * 220.0 * i as f32 / 48000.0).sin();
                let output = engine.process([x, -x], controls);
                assert!(output.iter().all(|x| x.is_finite()));
            }

            assert!((engine.estimate.frequency - 220.0).abs() < 1.0, "{mode:?}");
        }
    });
}

#[test]
fn enhanced_backends_track_across_sample_rates_and_pitch_limits() {
    for rate in [8000.0, 22050.0, 44100.0, 48000.0, 96000.0, 192000.0] {
        for mode in [
            DetectionMode::MpmTracking,
            DetectionMode::Pyin,
            DetectionMode::SpectralMelody,
        ] {
            let mut engine = Engine::new(rate);
            let controls = Controls {
                detection: mode,
                ..Controls::default()
            };

            for hz in [51.0, 87.3, 349.2, 990.0] {
                engine.reset();

                for i in 0..(rate * 0.4) as usize {
                    let x = 0.2 * (TAU * hz * i as f32 / rate).sin();
                    engine.process([x; 2], controls);
                }

                let cents = 1200.0 * (engine.estimate.frequency / hz).log2();
                assert!(
                    cents.abs() < 10.0,
                    "{mode:?}, {rate}, {hz}: {:?}",
                    engine.estimate
                );
            }
        }
    }
}

#[test]
fn enhanced_backends_recover_a_missing_fundamental_at_startup() {
    for mode in [
        DetectionMode::MpmTracking,
        DetectionMode::Pyin,
        DetectionMode::SpectralMelody,
    ] {
        let mut detector = Detector::new(48000.0);
        detector.mode = mode;
        let pitches = feed(&mut detector, 0.4, |t| {
            let phase = TAU * 110.0 * t;
            0.2 * (2.0 * phase).sin() + 0.17 * (3.0 * phase).sin()
        });

        assert!(
            (pitches.last().unwrap() - 110.0).abs() < 1.0,
            "{mode:?}: {pitches:?}"
        );
    }
}

#[test]
fn enhanced_backends_do_not_lock_to_dc_noise_or_below_gate_audio() {
    for mode in [
        DetectionMode::MpmTracking,
        DetectionMode::Pyin,
        DetectionMode::SpectralMelody,
    ] {
        for kind in 0..3 {
            let mut engine = Engine::new(48000.0);
            let controls = Controls {
                detection: mode,
                ..Controls::default()
            };
            let mut seed = 7_u32;

            for i in 0..24000 {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let x = match kind {
                    0 => 0.1,
                    1 => 0.2 * (seed as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32,
                    _ => 1e-5 * (TAU * 220.0 * i as f32 / 48000.0).sin(),
                };
                engine.process([x; 2], controls);

                if i > 12000 {
                    assert_eq!(engine.estimate.frequency, 0.0, "{mode:?}, kind={kind}");
                }
            }
        }
    }
}

#[test]
fn pyin_commits_three_hops_later_and_reset_discards_pending_frames() {
    let mut tracker = Tracker::<PERIODIC_BINS>::new(128.0 / 12000.0);
    let mut observation = Observations::default();
    observation.add(220.0, 0.99, 1.0);

    for _ in 0..LOOKAHEAD {
        assert_eq!(tracker.process(&observation, 0.85, LOOKAHEAD).0, 0.0);
    }

    assert_eq!(tracker.process(&observation, 0.85, LOOKAHEAD).0, 220.0);
    tracker.reset();
    assert_eq!(tracker.process(&observation, 0.85, LOOKAHEAD).0, 0.0);
}
