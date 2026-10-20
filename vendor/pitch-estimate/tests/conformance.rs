//! Accuracy on every `expect_pitch: true` entry, both detectors, both sample
//! rates, f32 and f64, with the reference picker parameters. Signals are
//! regenerated from the frozen JSON and validated against the stored
//! fingerprints.

mod common;

use common::{cents, generate, load_signals, to_f32, Signal, SignalSuite};
use pitch_estimate::{McLeodDetector, Pitch, PitchDetector, YinDetector};

fn check(label: &str, detector: &str, s: &Signal, p: Option<Pitch<f64>>) {
    let p = p.unwrap_or_else(|| panic!("{label} {detector}: expected Some, got None"));
    let err = cents(p.frequency, s.expected.f0_hz);
    assert!(
        err.abs() <= s.expected.tol_cents,
        "{label} {detector}: f0 {} expected {} ({err:.2} cents, tol {})",
        p.frequency,
        s.expected.f0_hz,
        s.expected.tol_cents
    );
    assert!(
        p.clarity.is_finite() && (0.0..=1.0).contains(&p.clarity),
        "{label} {detector}: clarity {} out of range",
        p.clarity
    );
    assert!(
        p.clarity >= s.expected.clarity_min,
        "{label} {detector}: clarity {} below {}",
        p.clarity,
        s.expected.clarity_min
    );
}

fn widen(p: Option<Pitch<f32>>) -> Option<Pitch<f64>> {
    p.map(|p| Pitch {
        frequency: f64::from(p.frequency),
        clarity: f64::from(p.clarity),
    })
}

fn run(suite: &SignalSuite, f32_path: bool) {
    let rp = &suite.params;
    let mut mpm64 = McLeodDetector::<f64>::new(rp.mpm_window, rp.mpm_tau_max)
        .unwrap()
        .with_k(rp.mpm_k);
    let mut yin64 = YinDetector::<f64>::new(rp.yin_frame, rp.yin_window)
        .unwrap()
        .with_absolute_threshold(rp.yin_threshold);
    let mut mpm32 = McLeodDetector::<f32>::new(rp.mpm_window, rp.mpm_tau_max)
        .unwrap()
        .with_k(rp.mpm_k as f32);
    let mut yin32 = YinDetector::<f32>::new(rp.yin_frame, rp.yin_window)
        .unwrap()
        .with_absolute_threshold(rp.yin_threshold as f32);
    let mut checked = 0;
    for s in suite.signals.iter().filter(|s| s.expected.expect_pitch) {
        let x = generate(suite, s);
        let label = s.label();
        if f32_path {
            let x32 = to_f32(&x);
            check(
                &label,
                "mpm/f32",
                s,
                widen(mpm32.detect(&x32, s.sample_rate)),
            );
            check(
                &label,
                "yin/f32",
                s,
                widen(yin32.detect(&x32, s.sample_rate)),
            );
        } else {
            check(&label, "mpm/f64", s, mpm64.detect(&x, s.sample_rate));
            check(&label, "yin/f64", s, yin64.detect(&x, s.sample_rate));
        }
        checked += 1;
    }
    assert_eq!(checked, 22, "pitched conformance entries");
}

#[test]
fn conformance_accuracy_f64() {
    let suite = load_signals();
    run(&suite, false);
}

#[test]
fn conformance_accuracy_f32() {
    let suite = load_signals();
    run(&suite, true);
}

/// Every roster entry regenerates to its stored fingerprint.
#[test]
fn fingerprints_regenerate() {
    let suite = load_signals();
    for s in &suite.signals {
        let x = generate(&suite, s);
        assert_eq!(x.len(), s.num_samples);
    }
}

/// The `expect_pitch: false` entries report nothing above clarity 0.5.
#[test]
fn unpitched_entries_report_none() {
    let suite = load_signals();
    let rp = &suite.params;
    let mut mpm = McLeodDetector::<f64>::new(rp.mpm_window, rp.mpm_tau_max)
        .unwrap()
        .with_k(rp.mpm_k)
        .with_clarity_threshold(0.5);
    let mut yin = YinDetector::<f64>::new(rp.yin_frame, rp.yin_window)
        .unwrap()
        .with_clarity_threshold(0.5);
    let mut checked = 0;
    for s in suite.signals.iter().filter(|s| !s.expected.expect_pitch) {
        let x = generate(&suite, s);
        assert_eq!(mpm.detect(&x, s.sample_rate), None, "{} mpm", s.label());
        assert_eq!(yin.detect(&x, s.sample_rate), None, "{} yin", s.label());
        let x32 = to_f32(&x);
        let mut mpm32 = McLeodDetector::<f32>::new(rp.mpm_window, rp.mpm_tau_max)
            .unwrap()
            .with_clarity_threshold(0.5);
        let mut yin32 = YinDetector::<f32>::new(rp.yin_frame, rp.yin_window)
            .unwrap()
            .with_clarity_threshold(0.5);
        assert_eq!(
            mpm32.detect(&x32, s.sample_rate),
            None,
            "{} mpm/f32",
            s.label()
        );
        assert_eq!(
            yin32.detect(&x32, s.sample_rate),
            None,
            "{} yin/f32",
            s.label()
        );
        checked += 1;
    }
    assert_eq!(checked, 6);
}

/// With every gate open, a candidate reported on noise has in-range clarity,
/// and it stays far below any sane gate (measured: 0.063 for MPM).
#[test]
fn noise_candidate_stays_in_range() {
    let suite = load_signals();
    let rp = &suite.params;
    let mut mpm = McLeodDetector::<f64>::new(rp.mpm_window, rp.mpm_tau_max)
        .unwrap()
        .with_k(rp.mpm_k)
        .with_clarity_threshold(0.0);
    for s in suite.signals.iter().filter(|s| s.id == "noise_only") {
        let x = generate(&suite, s);
        if let Some(p) = mpm.detect(&x, s.sample_rate) {
            assert!(p.clarity.is_finite() && (0.0..=1.0).contains(&p.clarity));
            assert!(
                p.clarity < 0.3,
                "{}: noise clarity {}",
                s.label(),
                p.clarity
            );
            assert!(p.frequency.is_finite());
        }
    }
}
