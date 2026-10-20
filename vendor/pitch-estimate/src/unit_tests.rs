//! In-crate gates: the FFT path against the time-domain formulas, and the
//! named formula-level unit checks.

use crate::reference::{self, Lcg};
use crate::yin::cmndf_in_place;
use crate::{McLeodDetector, PitchDetector, YinDetector};

fn assert_close(actual: &[f64], expected: &[f64], tol: f64, what: &str) {
    assert_eq!(actual.len(), expected.len(), "{what}: length");
    for (tau, (a, e)) in actual.iter().zip(expected).enumerate() {
        let scale = e.abs().max(1.0);
        assert!(
            (a - e).abs() <= tol * scale,
            "{what}: tau={tau} fft={a} time-domain={e}"
        );
    }
}

/// FFT path vs the time-domain formulas, 20 seeded random frames, 1e-6
/// relative on both curves.
#[test]
fn differential_fft_vs_time_domain() {
    let mut lcg = Lcg(0x5eed_0001);
    for round in 0..20 {
        let frame_len = [1024usize, 1000, 2048, 777][round % 4];
        let tau_max = frame_len / 2;
        let frame = lcg.frame(frame_len);

        let mut mpm = McLeodDetector::<f64>::new(frame_len, tau_max).unwrap();
        let _ = mpm.detect(&frame, 44_100);
        assert_eq!(mpm.valid_len(), tau_max + 1);
        let expected = reference::nsdf(&frame, tau_max);
        assert_close(mpm.nsdf(), &expected, 1e-6, &format!("nsdf round {round}"));

        let window = frame_len - tau_max;
        let mut yin = YinDetector::<f64>::new(frame_len, window).unwrap();
        let _ = yin.detect(&frame, 44_100);
        assert_eq!(yin.valid_len(), tau_max + 1);
        let d = reference::difference(&frame, window, tau_max);
        let expected = reference::cmndf(&d);
        assert_close(
            yin.cmndf(),
            &expected,
            1e-6,
            &format!("cmndf round {round}"),
        );
        // The difference function itself is compared relative to the frame
        // energy, the scale of the cancellation inside d(tau).
        let energy: f64 = frame.iter().map(|v| v * v).sum();
        for (tau, (a, e)) in yin.difference().iter().zip(&d).enumerate() {
            assert!(
                (a - e).abs() <= 1e-9 * energy,
                "difference round {round}: tau={tau} fft={a} time-domain={e}"
            );
        }
    }
}

/// The f32 path tracks the time-domain formulas to single precision.
#[test]
fn differential_f32_tracks_time_domain() {
    let mut lcg = Lcg(0x5eed_0002);
    let frame_len = 2048;
    let tau_max = 1024;
    let frame = lcg.frame(frame_len);
    let frame32: Vec<f32> = frame.iter().map(|&v| v as f32).collect();

    let mut mpm = McLeodDetector::<f32>::new(frame_len, tau_max).unwrap();
    let _ = mpm.detect(&frame32, 44_100);
    let expected = reference::nsdf(&frame, tau_max);
    let actual: Vec<f64> = mpm.nsdf().iter().map(|&v| f64::from(v)).collect();
    assert_close(&actual, &expected, 1e-3, "nsdf f32");

    let mut yin = YinDetector::<f32>::new(frame_len, tau_max).unwrap();
    let _ = yin.detect(&frame32, 44_100);
    let d = reference::difference(&frame, tau_max, tau_max);
    let expected = reference::cmndf(&d);
    let actual: Vec<f64> = yin.cmndf().iter().map(|&v| f64::from(v)).collect();
    assert_close(&actual, &expected, 1e-3, "cmndf f32");
}

/// The NSDF of a short hand-computed vector equals the time-domain formula
/// to 1e-9, and n(tau) <= 1 + 1e-9 on random frames.
#[test]
fn nsdf_unit() {
    // x = [1, 2, 3, 4, 3, 2, 1, 0], tau_max 4.
    // tau=1: r = 2+6+12+12+6+2+0 = 40, m' = (1+4+9+16+9+4+1) + (4+9+16+9+4+1+0) = 44+43 = 87
    // tau=2: r = 3+8+9+8+3+0 = 31, m' = (1+4+9+16+9+4) + (9+16+9+4+1+0) = 43+39 = 82
    // tau=3: r = 4+6+6+4+0 = 20, m' = (1+4+9+16+9) + (16+9+4+1+0) = 39+30 = 69
    // tau=4: r = 3+4+3+0 = 10, m' = (1+4+9+16) + (9+4+1+0) = 30+14 = 44
    let x = [1.0f64, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0, 0.0];
    let expected = [
        1.0,
        2.0 * 40.0 / 87.0,
        2.0 * 31.0 / 82.0,
        2.0 * 20.0 / 69.0,
        2.0 * 10.0 / 44.0,
    ];
    let mut mpm = McLeodDetector::<f64>::new(8, 4).unwrap();
    let _ = mpm.detect(&x, 8000);
    assert_eq!(mpm.valid_len(), 5);
    for (tau, (a, e)) in mpm.nsdf().iter().zip(&expected).enumerate() {
        assert!((a - e).abs() <= 1e-9, "tau={tau}: {a} vs {e}");
    }
    let expected_td = reference::nsdf(&x, 4);
    assert_close(mpm.nsdf(), &expected_td, 1e-9, "hand vector vs time-domain");

    let mut lcg = Lcg(0xd1);
    let mut mpm = McLeodDetector::<f64>::new(4096, 2048).unwrap();
    for _ in 0..10 {
        let frame = lcg.frame(4096);
        let _ = mpm.detect(&frame, 44_100);
        for (tau, &n) in mpm.nsdf().iter().enumerate() {
            assert!((-1.0 - 1e-9..=1.0 + 1e-9).contains(&n), "tau={tau}: n={n}");
        }
    }
}

/// d = [0, 6, 14] gives exactly [1, 1, 1.4]. An off-by-one tau multiplier
/// yields [1, 2, 2.1].
#[test]
fn cmndf_index_unit() {
    let d = [0.0f64, 6.0, 14.0];
    let mut out = [0.0f64; 3];
    assert_eq!(cmndf_in_place(&d, &mut out), 3);
    assert_eq!(out, [1.0, 1.0, 1.4]);

    let d32 = [0.0f32, 6.0, 14.0];
    let mut out32 = [0.0f32; 3];
    assert_eq!(cmndf_in_place(&d32, &mut out32), 3);
    assert_eq!(out32, [1.0, 1.0, 1.4]);
}

/// d'(1) == 1 for 100 random nonzero frames.
#[test]
fn cmndf_d1_is_one_for_random_frames() {
    let mut lcg = Lcg(0xd2);
    let mut yin = YinDetector::<f64>::new(512, 256).unwrap();
    for _ in 0..100 {
        let frame = lcg.frame(512);
        let _ = yin.detect(&frame, 44_100);
        assert!(yin.valid_len() > 1);
        assert_eq!(yin.cmndf()[1], 1.0);
        assert_eq!(yin.cmndf()[0], 1.0);
    }
}

/// A zero running sum gives the no-dip value 1, and a non-finite entry ends
/// the finite prefix.
#[test]
fn cmndf_guards() {
    let d = [0.0f64, 0.0, 0.0, 5.0];
    let mut out = [9.0f64; 4];
    assert_eq!(cmndf_in_place(&d, &mut out), 4);
    assert_eq!(out, [1.0, 1.0, 1.0, 3.0]);

    let d = [0.0f64, 2.0, f64::INFINITY, 1.0];
    let mut out = [9.0f64; 4];
    assert_eq!(cmndf_in_place(&d, &mut out), 2);
    assert_eq!(&out[..2], &[1.0, 1.0]);

    let d = [0.0f64, f64::NAN];
    let mut out = [9.0f64; 2];
    assert_eq!(cmndf_in_place(&d, &mut out), 1);

    let mut out: [f64; 0] = [];
    assert_eq!(cmndf_in_place(&[], &mut out), 0);
}

#[test]
fn config_errors_display() {
    use crate::ConfigError;
    let e = McLeodDetector::<f64>::new(0, 1).unwrap_err();
    assert_eq!(e, ConfigError::ZeroSize { what: "frame_len" });
    assert_eq!(e.to_string(), "frame_len must be at least 1, got 0");
    let e = McLeodDetector::<f64>::new(16, 17).unwrap_err();
    assert_eq!(e.to_string(), "tau_max must lie in 1..=16, got 17");
    let e = YinDetector::<f32>::new(16, 16).unwrap_err();
    assert_eq!(e.to_string(), "window must lie in 1..=15, got 16");
    let e = YinDetector::<f32>::new(16, 0).unwrap_err();
    assert_eq!(e, ConfigError::ZeroSize { what: "window" });
    let e = McLeodDetector::<f64>::new(1, 1).unwrap_err();
    assert!(matches!(
        e,
        ConfigError::OutOfRange {
            what: "frame_len",
            ..
        }
    ));
    let e = McLeodDetector::<f64>::new(crate::MAX_FRAME_LEN + 1, 1).unwrap_err();
    assert!(matches!(
        e,
        ConfigError::OutOfRange {
            what: "frame_len",
            ..
        }
    ));

    let mut set = std::collections::HashSet::new();
    set.insert(ConfigError::ZeroSize { what: "frame_len" });
    set.insert(e);
    assert_eq!(set.len(), 2);
}

#[test]
fn pitch_displays_frequency_and_clarity() {
    let p = crate::Pitch {
        frequency: 440.0,
        clarity: 0.99,
    };
    assert_eq!(p.to_string(), "440 Hz, clarity 0.99");
}

#[test]
fn setters_clamp_and_default() {
    let m = McLeodDetector::<f64>::new(64, 32).unwrap();
    assert_eq!(m.k(), 0.9);
    assert_eq!(m.clarity_threshold(), 0.6);
    let m = m.with_k(0.1);
    assert_eq!(m.k(), 0.8);
    let m = m.with_k(7.0);
    assert_eq!(m.k(), 1.0);
    let m = m.with_k(f64::NAN).with_clarity_threshold(f64::NAN);
    assert_eq!(m.k(), 0.9);
    assert_eq!(m.clarity_threshold(), 0.6);
    let m = m.with_power_threshold(f64::NAN);
    assert_eq!(m.power_threshold(), 0.0);
    assert_eq!(m.fft_len(), 128);

    let y = YinDetector::<f32>::new(64, 32).unwrap();
    assert!((y.absolute_threshold() - 0.15).abs() < 1e-6);
    let y = y.with_clarity_threshold(0.9);
    assert!((y.absolute_threshold() - 0.1).abs() < 1e-6);
    assert!((y.clarity_threshold() - 0.9).abs() < 1e-6);
    let y = y.with_clarity_threshold(f32::NAN);
    assert!((y.absolute_threshold() - 0.15).abs() < 1e-6);
    assert_eq!(y.tau_max(), 32);
    assert_eq!(y.window(), 32);
    assert_eq!(y.frame_len(), 64);
    assert_eq!(y.fft_len(), 64);
}

/// Two identical 24-sample end segments with a large unrelated centre
/// transient. Exact n(40) = 1. A floor of m'(tau) against m'(0) used to
/// write n(40) = 0 and drop the pitch.
#[test]
fn separated_matching_segments_keep_nsdf_peak() {
    let mut state = 0x1234_5678_9abc_def0u64;
    let mut pattern = [0.0f64; 24];
    for value in &mut pattern {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *value = if state >> 63 == 0 { -1.0 } else { 1.0 };
    }

    let mut frame = [0.0f64; 64];
    frame[..24].copy_from_slice(&pattern);
    frame[40..].copy_from_slice(&pattern);
    frame[32] = 1e8;

    let mut detector = McLeodDetector::<f64>::new(64, 48)
        .unwrap()
        .with_clarity_threshold(0.9);
    let pitch = detector
        .detect(&frame, 48_000)
        .expect("matching end segments are a period");
    let tau = 40usize;
    let r: f64 = (0..64 - tau).map(|j| frame[j] * frame[j + tau]).sum();
    let m: f64 = (0..64 - tau)
        .map(|j| frame[j] * frame[j] + frame[j + tau] * frame[j + tau])
        .sum();
    let expected = 2.0 * r / m;
    assert!(
        (expected - 1.0).abs() < 1e-12,
        "time-domain n(40)={expected}"
    );
    let n40 = detector.nsdf()[tau];
    assert!(
        n40 > 0.99,
        "nsdf[40]={n40} was zeroed or collapsed by a floor on m'(0)"
    );
    assert!(pitch.clarity > 0.9);
    assert!((pitch.frequency - 1200.0).abs() < 50.0);
}

/// Overflowing 1024-sample frames leave only the d'(0)/n(0) = 1 prefix.
#[test]
fn overflow_frame_finite_prefix_is_one() {
    let frame64: Vec<f64> = (0..1024)
        .map(|i| if i % 2 == 0 { 1e152 } else { -1e152 })
        .collect();
    let frame32: Vec<f32> = (0..1024)
        .map(|i| if i % 2 == 0 { 1e17 } else { -1e17 })
        .collect();
    let mut yin = YinDetector::<f64>::new(1024, 512).unwrap();
    assert_eq!(yin.detect(&frame64, 44_100), None);
    assert_eq!(yin.valid_len(), 1);
    let mut yin = YinDetector::<f32>::new(1024, 512).unwrap();
    assert_eq!(yin.detect(&frame32, 44_100), None);
    assert_eq!(yin.valid_len(), 1);
    let mut mpm = McLeodDetector::<f64>::new(1024, 512).unwrap();
    assert_eq!(mpm.detect(&frame64, 44_100), None);
    assert_eq!(mpm.valid_len(), 1);
    let mut mpm = McLeodDetector::<f32>::new(1024, 512).unwrap();
    assert_eq!(mpm.detect(&frame32, 44_100), None);
    assert_eq!(mpm.valid_len(), 1);
}
