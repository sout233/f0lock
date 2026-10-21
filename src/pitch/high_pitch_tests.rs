use super::*;
use crate::{
    detector::Detector,
    engine::{Controls, Engine},
};
use std::f32::consts::TAU;

#[test]
fn psi_tracks_high_notes_instead_of_their_subharmonics() {
    for rate in [8000.0, 22050.0, 44100.0, 48000.0, 96000.0, 192000.0] {
        for hz in [1001.0, 1760.0, 1995.0, 2000.0] {
            let mut engine = Engine::new(rate);
            let controls = Controls::default();

            nice_assert_no_alloc::assert_no_alloc(|| {
                for i in 0..(rate * 0.5) as usize {
                    let phase = TAU * hz * i as f32 / rate;
                    let sample = 0.12 * phase.sin()
                        + if 2.0 * hz < rate * 0.45 {
                            0.18 * (2.0 * phase).sin()
                        } else {
                            0.0
                        };
                    engine.process([sample; 2], controls);
                }
            });

            let cents = 1200.0 * (engine.estimate.frequency / hz).log2();
            assert!(
                cents.abs() < 12.0,
                "rate={rate}, hz={hz}, estimate={:?}",
                engine.estimate
            );
        }
    }
}

#[test]
fn high_targets_are_available_to_psi_but_do_not_extend_periodicity_backends() {
    let target = Target {
        mode: TargetMode::Range,
        minimum: 1500.0,
        maximum: 2000.0,
        ..Target::default()
    };

    for mode in DetectionMode::ALL {
        let mut detector = Detector::new(48000.0);
        detector.mode = mode;
        detector.target = target;
        let mut last = crate::detector::Estimate::default();

        for i in 0..24000 {
            let sample = 0.2 * (TAU * 1760.0 * i as f32 / 48000.0).sin();

            if detector.push([sample; 2]) {
                last = detector.analyze(-55.0, 0.85);
            }
        }

        if mode == DetectionMode::SpectralMelody {
            assert!((last.frequency - 1760.0).abs() < 3.0, "{last:?}");
        } else {
            assert_eq!(last.frequency, 0.0, "{mode:?}: {last:?}");
        }
    }
}

#[test]
fn psi_applies_eq_at_the_high_fundamental() {
    let mut engine = Engine::new(44100.0);
    let mut input = [0.0_f64; 2];
    let mut output = [0.0_f64; 2];

    for i in 0..88200 {
        let phase = TAU * 1760.0 * i as f32 / 44100.0;
        let sample = 0.12 * phase.sin() + 0.18 * (2.0 * phase).sin();
        let processed = engine.process([sample; 2], Controls::default());

        if i >= 44100 {
            for (ch, basis) in [phase.sin(), phase.cos()].into_iter().enumerate() {
                input[ch] += f64::from(sample * basis);
                output[ch] += f64::from(processed[0] * basis);
            }
        }
    }

    let gain = 20.0 * (output[0].hypot(output[1]) / input[0].hypot(input[1])).log10();
    assert!((engine.estimate.frequency - 1760.0).abs() < 3.0);
    assert!((gain + 6.0).abs() < 0.2, "High-note EQ: {gain} dB");
}

/// External 44.1 kHz stereo f32 export of the user's high sustain synth.
#[test]
#[ignore = "requires F0LOCK_HIGH_SYNTH_FIXTURE; copyrighted sample stays outside the repository"]
fn external_high_synth_tracks_the_upper_octave_in_auto() {
    let path = std::env::var("F0LOCK_HIGH_SYNTH_FIXTURE").expect("Set F0LOCK_HIGH_SYNTH_FIXTURE");
    let bytes = std::fs::read(&path).expect("Read high synth fixture");
    assert!(!bytes.is_empty() && bytes.len() % 8 == 0);
    let mut detector = Detector::new(44100.0);
    let mut results = Vec::new();

    for (i, frame) in bytes.chunks_exact(8).enumerate() {
        let stereo = std::array::from_fn(|ch| {
            f32::from_le_bytes(frame[ch * 4..ch * 4 + 4].try_into().unwrap())
        });
        assert!(stereo.iter().all(|v| v.is_finite()));

        if detector.push(stereo) {
            let estimate = detector.analyze(-55.0, 0.85);

            if i > 22050 {
                results.push(estimate.frequency);
            }
        }
    }

    let high = results
        .iter()
        .filter(|&hz| (1650.0..1900.0).contains(hz))
        .count();
    let low = results
        .iter()
        .filter(|&hz| (825.0..950.0).contains(hz))
        .count();
    println!(
        "Auto: high={high}, octave_down={low}, frames={}",
        results.len()
    );
    assert!(
        high as f32 / results.len() as f32 > 0.95,
        "High-note tracking coverage below 95%"
    );
    assert_eq!(
        low, 0,
        "The sustained synth still locks to the lower octave"
    );
}
