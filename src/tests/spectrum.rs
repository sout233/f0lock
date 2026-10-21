use crate::{
    Telemetry,
    analyzer::{Analyzer, bin_frequency, max_frequency},
    engine::{Controls, Engine},
};
use std::f32::consts::TAU;

#[test]
fn input_spectrum_displays_a_tone_above_2khz() {
    let mut engine = Engine::new(48000.0);
    engine.visualize = true;

    for i in 0..24000 {
        let sample = 0.25 * (TAU * 8000.0 * i as f32 / 48000.0).sin();
        engine.process([sample; 2], Controls::default());
    }

    let telemetry = Telemetry::default();
    telemetry.publish(&engine);
    let display = telemetry.spectrum();
    let peak = display
        .iter()
        .enumerate()
        .filter_map(|(i, &db)| {
            let hz = 20.0 * 1000.0_f32.powf(i as f32 / (display.len() - 1) as f32);
            (7000.0..=9000.0).contains(&hz).then_some(db)
        })
        .fold(f32::NEG_INFINITY, f32::max);

    assert!(
        peak > -16.0,
        "8 kHz input tone is missing from the full-band spectrum: {peak} dBFS"
    );
}

#[test]
fn full_band_spectrum_covers_sample_rates_and_preserves_stereo_content() {
    for rate in [8000.0, 22050.0, 44100.0, 48000.0, 96000.0, 192000.0] {
        for hz in [3000.0, 8000.0, 12000.0, 18000.0, 20000.0] {
            if hz > max_frequency(rate) {
                continue;
            }

            let mut analyzer = Analyzer::new(rate);

            nice_assert_no_alloc::assert_no_alloc(|| {
                for i in 0..(rate * 0.3) as usize {
                    let sample = 0.25 * (TAU * hz * i as f32 / rate).sin();
                    analyzer.push([sample, -sample], true);
                }
            });

            let (peak_index, &peak) = analyzer
                .display
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .unwrap();
            let displayed_hz = bin_frequency(peak_index, max_frequency(rate));

            assert!(
                (displayed_hz / hz - 1.0).abs() < 0.04,
                "rate={rate}, input={hz}, display={displayed_hz}"
            );
            assert!(
                (-16.0..=-11.0).contains(&peak),
                "rate={rate}, hz={hz}, level={peak}"
            );
        }
    }
}

#[test]
fn quieter_channel_high_frequencies_remain_visible() {
    let mut analyzer = Analyzer::new(48000.0);

    for i in 0..24000 {
        let phase = TAU * i as f32 / 48000.0;
        analyzer.push(
            [0.5 * (110.0 * phase).sin(), 0.25 * (8000.0 * phase).sin()],
            true,
        );
    }

    for (hz, threshold) in [(110.0, -10.0), (8000.0, -16.0)] {
        let peak = analyzer
            .display
            .iter()
            .enumerate()
            .filter_map(|(i, &db)| {
                let frequency = bin_frequency(i, 20000.0);
                ((frequency / hz - 1.0).abs() < 0.04).then_some(db)
            })
            .fold(f32::NEG_INFINITY, f32::max);

        assert!(
            peak > threshold,
            "Stereo channel content missing at {hz} Hz: {peak}"
        );
    }
}

#[test]
fn spectrum_resets_on_close_and_does_not_replay_old_audio() {
    let mut analyzer = Analyzer::new(48000.0);

    for i in 0..12000 {
        let sample = 0.25 * (TAU * 8000.0 * i as f32 / 48000.0).sin();
        analyzer.push([sample; 2], true);
    }

    assert!(analyzer.display.iter().any(|&db| db > -16.0));
    analyzer.push([0.0; 2], false);
    assert!(analyzer.display.iter().all(|&db| db < -80.0));

    for _ in 0..12000 {
        analyzer.push([0.0; 2], true);
    }

    assert!(analyzer.display.iter().all(|&db| db < -80.0));
}

#[test]
fn spectrum_analysis_does_not_change_audio_or_pitch_detection() {
    let mut visible = Engine::new(48000.0);
    let mut hidden = Engine::new(48000.0);
    visible.visualize = true;

    for i in 0..24000 {
        let phase = TAU * i as f32 / 48000.0;
        let input = [0.25 * (220.0 * phase).sin() + 0.1 * (8000.0 * phase).sin(); 2];
        assert_eq!(
            visible.process(input, Controls::default()),
            hidden.process(input, Controls::default())
        );
    }

    assert_eq!(visible.estimate.frequency, hidden.estimate.frequency);
    assert_eq!(visible.estimate.clarity, hidden.estimate.clarity);
}
