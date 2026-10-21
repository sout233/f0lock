use super::*;
use crate::{
    detector::Detector,
    engine::{Controls, Engine},
};
use std::f32::consts::TAU;

#[test]
fn melody_tracks_the_selected_voice_through_octave_and_non_octave_harmonies() {
    for lower in [201.0, 267.0] {
        for target in [
            Target {
                mode: TargetMode::Range,
                minimum: 360.0,
                maximum: 460.0,
                ..Target::default()
            },
            Target {
                mode: TargetMode::Reference,
                reference: 400.0,
                tolerance: 2.0,
                ..Target::default()
            },
        ] {
            let mut detector = Detector::new(48000.0);
            detector.mode = DetectionMode::SpectralMelody;
            detector.target = target;
            let mut frames = 0;

            nice_assert_no_alloc::assert_no_alloc(|| {
                for i in 0..96000 {
                    let t = i as f32 / 48000.0;
                    // A known lead with vibrato, plus a separate overlapping voice.
                    let phase = TAU * 400.0 * t + 0.5 * (TAU * 5.0 * t).sin();
                    let lead: f32 = (1..=8)
                        .map(|h| 0.15 / h as f32 * (h as f32 * phase).sin())
                        .sum();
                    let harmony: f32 = if t > 0.3 {
                        (1..=8)
                            .map(|h| 0.2 / h as f32 * (TAU * lower * h as f32 * t).sin())
                            .sum()
                    } else {
                        0.0
                    };

                    if detector.push([lead + harmony; 2]) {
                        let estimate = detector.analyze(-55.0, 0.85);

                        if t > 0.5 {
                            assert!(
                                (estimate.frequency - 400.0).abs() < 8.0,
                                "lower={lower}, t={t}, {estimate:?}"
                            );
                            frames += 1;
                        }
                    }
                }
            });

            assert!(frames > 100);
        }
    }
}

#[test]
fn melody_mode_and_wideband_history_switch_without_realtime_allocations() {
    let mut engine = Engine::new(48000.0);

    nice_assert_no_alloc::assert_no_alloc(|| {
        for detection in [
            DetectionMode::SpectralMelody,
            DetectionMode::Mpm,
            DetectionMode::SpectralMelody,
        ] {
            let controls = Controls {
                detection,
                ..Controls::default()
            };

            for i in 0..24000 {
                let sample = 0.2 * (TAU * 220.0 * i as f32 / 48000.0).sin();
                engine.process([sample; 2], controls);
            }

            assert!((engine.estimate.frequency - 220.0).abs() < 1.0);
        }
    });
}

/// Fixture is external: interleaved stereo little-endian f32, at 44.1 kHz.
/// No copyrighted audio or inferred ground-truth labels are checked in.
#[test]
#[ignore = "requires F0LOCK_MELODY_FIXTURE pointing to an external 44.1 kHz stereo f32 fixture"]
fn analyze_external_harmony_fixture() {
    let path = std::env::var("F0LOCK_MELODY_FIXTURE").expect("Set F0LOCK_MELODY_FIXTURE");
    let bytes = std::fs::read(&path).expect("Read melody fixture");
    assert_eq!(bytes.len() % 8, 0, "Fixture must contain stereo f32 frames");
    let samples: Vec<[f32; 2]> = bytes
        .chunks_exact(8)
        .map(|frame| {
            std::array::from_fn(|ch| {
                f32::from_le_bytes(frame[ch * 4..ch * 4 + 4].try_into().unwrap())
            })
        })
        .collect();
    assert!(samples.iter().flatten().all(|x| x.is_finite()));
    assert!(!samples.is_empty());
    let mut csv = String::from("mode,time_seconds,hz,clarity\n");

    for mode in DetectionMode::ALL {
        let mut detector = Detector::new(44100.0);
        detector.mode = mode;
        detector.target = Target {
            mode: TargetMode::Range,
            minimum: 250.0,
            maximum: 600.0,
            ..Target::default()
        };
        let mut voiced = 0;
        let mut frames = 0;
        let start = std::time::Instant::now();

        for (i, &sample) in samples.iter().enumerate() {
            if detector.push(sample) {
                let estimate = detector.analyze(-55.0, 0.85);
                assert!(estimate.frequency.is_finite() && estimate.clarity.is_finite());
                voiced += usize::from(estimate.frequency > 0.0);
                frames += 1;
                use std::fmt::Write;
                writeln!(
                    csv,
                    "{mode:?},{:.6},{:.3},{:.6}",
                    i as f64 / 44100.0,
                    estimate.frequency,
                    estimate.clarity
                )
                .unwrap();
            }
        }

        println!(
            "{mode:?}: voiced={voiced}/{frames} ({:.1}%), analysis+CSV={:?}; coverage is NOT pitch accuracy",
            voiced as f32 / frames as f32 * 100.0,
            start.elapsed()
        );
    }

    std::fs::write(format!("{path}.csv"), csv.as_bytes()).expect("Write UTF-8 melody analysis CSV");
}
