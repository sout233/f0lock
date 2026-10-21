use super::*;
use crate::engine::{Controls, Engine};
use std::f32::consts::TAU;

#[test]
fn tracking_preserves_sustained_periodic_observations_after_a_clean_attack() {
    for lag in [0, LOOKAHEAD] {
        let mut tracker = Tracker::<PERIODIC_BINS>::new(128.0 / 12000.0);
        let mut onset = Observations::default();
        onset.add(220.0, 0.99, 1.0);

        for _ in 0..20 {
            tracker.process(&onset, 0.85, lag);
        }

        let mut sustained = Observations::default();
        sustained.add(220.0, 0.93, 0.9);

        for frame in 0..100 {
            let (hz, clarity) = tracker.process(&sustained, 0.85, lag);
            assert!(
                hz > 0.0,
                "Voiced sustain lost at frame {frame}, lag={lag}, clarity={clarity}"
            );
        }
    }
}

#[test]
fn sustained_noisy_tone_keeps_eq_engaged_in_enhanced_modes() {
    let rate = 48000.0;
    let mut failed = Vec::new();

    for mode in DetectionMode::ALL {
        let mut engine = Engine::new(rate);
        let controls = Controls {
            detection: mode,
            ..Controls::default()
        };
        let mut seed = 7_u32;
        let mut lost = 0;
        let mut applied = 0;
        let mut voiced_clarity = 0.0;
        let mut input_projection = [0.0_f64; 2];
        let mut output_projection = [0.0_f64; 2];

        for i in 0..96000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = (seed as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32;
            let phase = TAU * 220.0 * i as f32 / rate;
            let x = 0.2 * phase.sin() + if i > 14400 { 0.3 * noise } else { 0.0 };
            let output = engine.process([x; 2], controls);

            if i > 24000 {
                lost += usize::from(engine.estimate.frequency == 0.0);
                applied += usize::from(engine.bands[0].wet > 0.99);
                voiced_clarity += engine.estimate.clarity;

                for (channel, basis) in [phase.sin(), phase.cos()].into_iter().enumerate() {
                    input_projection[channel] += f64::from(x * basis);
                    output_projection[channel] += f64::from(output[0] * basis);
                }
            }
        }

        let samples = 71999.0;
        let amplitude = |p: [f64; 2]| p[0].hypot(p[1]);
        let gain = 20.0 * (amplitude(output_projection) / amplitude(input_projection)).log10();
        println!(
            "{mode:?}: lost={:.1}%, wet={:.1}%, mean clarity={:.3}, gain={gain:.2} dB",
            lost as f32 / samples * 100.0,
            applied as f32 / samples * 100.0,
            voiced_clarity / samples
        );
        if lost != 0 || applied <= 71000 || (gain + 6.0).abs() > 0.2 {
            failed.push(mode);
        }
    }

    assert!(failed.is_empty(), "Sustained EQ dropped out: {failed:?}");
}
