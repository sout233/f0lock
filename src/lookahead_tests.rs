use crate::engine::{Controls, Engine};
use std::f32::consts::TAU;

#[test]
fn first_pitch_initializes_frequency_before_attack_instead_of_sweeping_from_150hz() {
    let mut engine = Engine::new(48000.0);
    let controls = Controls {
        tracking_ms: 300.0,
        ..Controls::default()
    };

    for i in 0..12000 {
        let sample = 0.1 * (TAU * 880.0 * i as f32 / 48000.0).sin();
        engine.process([sample; 2], controls);

        if engine.estimate.frequency > 0.0 {
            assert!(
                (engine.frequency - engine.estimate.frequency).abs() < 0.01,
                "First pitch: {}, filter frequency: {}",
                engine.estimate.frequency,
                engine.frequency
            );
            assert!((engine.bands[0].frequency - engine.estimate.frequency).abs() < 0.01);
            assert!((engine.bands[0].wet - 1.0 / 480.0).abs() < 1e-6);
            return;
        }
    }

    panic!("No first pitch detected");
}

#[test]
fn lookahead_applies_eq_to_the_start_of_the_delayed_note() {
    for detection in crate::pitch::DetectionMode::ALL {
        for enabled in [false, true] {
            let mut engine = Engine::new(48000.0);
            engine.set_lookahead(enabled);
            let delay = engine.latency_samples();
            let mut controls = Controls {
                detection,
                tracking_ms: 300.0,
                ..Controls::default()
            };
            controls.bands[0].gain = -12.0;
            let mut power = [0.0_f64; 2];

            nice_assert_no_alloc::assert_no_alloc(|| {
                for i in 0..24000 {
                    let sample = 0.1 * (TAU * 880.0 * i as f32 / 48000.0).sin();
                    let output = engine.process([sample; 2], controls);

                    if i < delay {
                        assert_eq!(output, [0.0; 2]);
                    }

                    if (delay + 1056..delay + 2016).contains(&i) {
                        let reference = 0.1 * (TAU * 880.0 * (i - delay) as f32 / 48000.0).sin();
                        power[0] += f64::from(reference).powi(2);
                        power[1] += f64::from(output[0]).powi(2);
                    }
                }
            });

            let gain = 10.0 * (power[1] / power[0]).log10();
            let expected = if enabled { -12.0 } else { 0.0 };
            assert!(
                (gain - expected).abs() < 0.2,
                "{detection:?}, lookahead={enabled}, onset gain={gain}"
            );
        }
    }
}

#[test]
fn delay_keeps_dry_bypass_and_stereo_automation_aligned_and_reset_discards_history() {
    for rate in [8000.0, 44100.0, 48000.0, 192000.0] {
        for bypass in [false, true] {
            let mut engine = Engine::new(rate);
            engine.set_lookahead(true);
            let delay = engine.latency_samples();
            assert!(delay >= (rate * 0.15).round() as usize);
            assert!(delay >= engine.detector.maximum_decision_age_samples());
            let controls = Controls {
                bypass,
                mix: 0.0,
                ..Controls::default()
            };

            nice_assert_no_alloc::assert_no_alloc(|| {
                for i in 0..delay + 32 {
                    let x = if i == 7 { 0.25 } else { 0.0 };
                    let output = engine.process([x, -x], controls);
                    let expected = if i == delay + 7 {
                        [0.25, -0.25]
                    } else {
                        [0.0; 2]
                    };
                    assert_eq!(output, expected, "rate={rate}, bypass={bypass}, sample={i}");
                }

                engine.process([0.5; 2], controls);
                engine.reset();

                for _ in 0..delay + 10 {
                    assert_eq!(engine.process([0.0; 2], controls), [0.0; 2]);
                }

                engine.set_lookahead(false);
                assert_eq!(engine.latency_samples(), 0);
                assert_eq!(engine.process([0.25, -0.25], controls), [0.25, -0.25]);
            });
        }
    }
}

#[test]
fn buffered_pitch_and_parameters_share_the_source_timestamp() {
    use crate::{
        detector::Estimate,
        lookahead::{Decision, Lookahead},
    };

    let mut queue = Lookahead::new(48000.0, 5631);
    queue.set_enabled(true);
    let delay = queue.latency_samples();

    nice_assert_no_alloc::assert_no_alloc(|| {
        for i in 0..delay + 100 {
            let mut controls = Controls::default();
            controls.bands[0].gain = if i == 37 { -12.0 } else { -6.0 };
            if i == 37 + 5631 {
                assert_eq!(queue.analysis_controls(5631, controls).bands[0].gain, -12.0);
            }
            let decision = (i == 37 + 5631).then_some(Decision {
                dated: true,
                age: 5631,
                prelock_age: None,
                estimate: Estimate {
                    frequency: 880.0,
                    clarity: 0.99,
                    rms: 0.1,
                },
            });
            let input = if i == 37 { [0.25; 2] } else { [0.0; 2] };

            if let Some(frame) = queue.push(input, input, controls, decision, false) {
                if i - delay == 37 {
                    assert_eq!(frame.audio, [0.25; 2]);
                    assert_eq!(frame.controls.bands[0].gain, -12.0);
                    assert_eq!(frame.estimate.unwrap().frequency, 880.0);
                } else {
                    assert_eq!(frame.audio, [0.0; 2]);
                    assert!(frame.estimate.is_none());
                }
            }
        }
    });
}

#[test]
fn lookahead_waits_for_main_or_sidechain_onset_before_starting_attack() {
    use crate::mode::OperatingMode;

    for mode in [OperatingMode::Track, OperatingMode::Sidechain] {
        let mut engine = Engine::new(48000.0);
        engine.set_lookahead(true);
        let delay = engine.latency_samples();
        let mut controls = Controls {
            mode,
            ..Controls::default()
        };
        controls.bands[0].ratio = if mode == OperatingMode::Sidechain {
            4.0
        } else {
            1.0
        };
        let onset = 4000;

        for i in 0..onset + delay + 12000 {
            let t = i.saturating_sub(onset) as f32 / 48000.0;
            let main = if i >= onset {
                0.1 * (TAU * 440.0 * t).sin()
            } else {
                0.0
            };
            let sidechain = if i >= onset {
                0.2 * (TAU * 110.0 * t).sin()
            } else {
                0.0
            };
            engine.process_with_sidechain([main; 2], Some([sidechain; 2]), controls);

            if i <= onset + delay {
                assert_eq!(
                    engine.bands[0].wet, 0.0,
                    "Attack spent on silence in {mode:?}"
                );
            }

            if i == onset + delay + 241 {
                assert!(
                    (engine.bands[0].wet - 241.0 / 480.0).abs() < 0.003,
                    "{mode:?}: wet={}",
                    engine.bands[0].wet
                );
            }
        }

        assert!(
            (engine.bands[0].frequency - 440.0).abs() < 2.0,
            "{mode:?}: {}",
            engine.bands[0].frequency
        );
        assert_eq!(engine.bands[0].wet, 1.0);
    }
}

#[test]
fn subsequent_note_changes_keep_tracking_smoothing() {
    let mut engine = Engine::new(48000.0);
    let controls = Controls {
        tracking_ms: 300.0,
        ..Controls::default()
    };
    let mut changed = false;

    for i in 0..72000 {
        let hz = if i < 24000 { 880.0 } else { 440.0 };
        let input = 0.1 * (TAU * hz * i as f32 / 48000.0).sin();
        let previous = engine.frequency;
        engine.process([input; 2], controls);

        if i > 24000
            && engine.estimate.frequency > 400.0
            && engine.estimate.frequency < 500.0
            && !changed
        {
            assert!(
                engine.frequency > 600.0,
                "Tracking was skipped: {}",
                engine.frequency
            );
            assert!((engine.frequency - previous).abs() < 1.0);
            changed = true;
        }
    }

    assert!(changed);
}
