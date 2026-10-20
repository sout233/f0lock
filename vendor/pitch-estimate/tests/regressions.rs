//! Named regressions: low-pitch NSDF seed, YIN clarity bounds, the no-panic
//! edge suite, overflow safety, and the AC-over-DC / high-lag NSDF guards.
//! Each test is named so a reintroduced defect is named in CI output.

mod common;

use std::fmt::Debug;

use common::{cents, generate, load_signals, sample_rates, to_f32, Lcg};
use pitch_estimate::{
    ConfigError, Float, McLeodDetector, Pitch, PitchDetector, YinDetector, MAX_FRAME_LEN,
};

/// Every reported pitch must be finite with clarity in `[0, 1]`.
fn in_contract<T: Float + Debug>(p: Option<Pitch<T>>, what: &str) -> Option<Pitch<T>> {
    if let Some(p) = p {
        assert!(
            p.frequency.is_finite(),
            "{what}: frequency {:?}",
            p.frequency
        );
        assert!(
            p.clarity.is_finite() && p.clarity >= T::ZERO && p.clarity <= T::ONE,
            "{what}: clarity {:?}",
            p.clarity
        );
    }
    p
}

fn sine<T: Float>(len: usize, freq: f64, sample_rate: u32) -> Vec<T> {
    (0..len)
        .map(|i| {
            let t = i as f64 / f64::from(sample_rate);
            T::from_f64_lossy((2.0 * std::f64::consts::PI * freq * t).sin())
        })
        .collect()
}

fn scaled_sine<T: Float>(
    len: usize,
    freq: f64,
    sample_rate: u32,
    amplitude: f64,
    dc: f64,
) -> Vec<T> {
    (0..len)
        .map(|i| {
            let t = i as f64 / f64::from(sample_rate);
            T::from_f64_lossy(dc + amplitude * (2.0 * std::f64::consts::PI * freq * t).sin())
        })
        .collect()
}

fn assert_yin_pitch<T: Float + Debug>(frame: &[T], sample_rate: u32, expected: f64, what: &str) {
    let mut yin = YinDetector::<T>::new(frame.len(), frame.len() / 2).unwrap();
    let pitch = in_contract(yin.detect(frame, sample_rate), what)
        .unwrap_or_else(|| panic!("{what}: dropped a pitched frame"));
    let error = cents(pitch.frequency.to_f64(), expected);
    assert!(
        error.abs() <= 10.0,
        "{what}: {} Hz ({error:.2} cents)",
        pitch.frequency.to_f64()
    );
    assert!(
        pitch.clarity >= T::from_f64_lossy(0.9) && pitch.clarity <= T::ONE,
        "{what}: clarity {:?}",
        pitch.clarity
    );
}

fn alternating<T: Float>(len: usize, amp: T) -> Vec<T> {
    (0..len)
        .map(|i| if i % 2 == 0 { amp } else { T::ZERO - amp })
        .collect()
}

fn noise<T: Float>(lcg: &mut Lcg, len: usize, amp: T) -> Vec<T> {
    (0..len)
        .map(|_| T::from_f64_lossy(lcg.next_unit()) * amp)
        .collect()
}

/// sine_55 at both rates passes MPM at clarity threshold 0.9, within 10
/// cents, with clarity at least 0.95. An NSDF seed from an unnormalized FFT
/// reports clarity near 0.81 here and is rejected.
#[test]
fn low_pitch_regression() {
    let suite = load_signals();
    let rp = &suite.params;
    let mut checked = 0;
    for s in suite.signals.iter().filter(|s| s.id == "sine_55") {
        let x = generate(&suite, s);
        let x32 = to_f32(&x);
        let label = s.label();
        for k in [0.9_f64, rp.mpm_k] {
            let mut mpm = McLeodDetector::<f64>::new(rp.mpm_window, rp.mpm_tau_max)
                .unwrap()
                .with_k(k)
                .with_clarity_threshold(0.9);
            let p = mpm
                .detect(&x, s.sample_rate)
                .unwrap_or_else(|| panic!("{label} k={k}: MPM rejected a clean 55 Hz sine"));
            let err = cents(p.frequency, 55.0);
            assert!(
                err.abs() <= 10.0,
                "{label} k={k}: {} Hz ({err:.2} cents)",
                p.frequency
            );
            assert!(p.clarity >= 0.95, "{label} k={k}: clarity {}", p.clarity);

            let mut mpm32 = McLeodDetector::<f32>::new(rp.mpm_window, rp.mpm_tau_max)
                .unwrap()
                .with_k(k as f32)
                .with_clarity_threshold(0.9);
            let p = mpm32
                .detect(&x32, s.sample_rate)
                .unwrap_or_else(|| panic!("{label} k={k} f32: MPM rejected a clean 55 Hz sine"));
            let err = cents(f64::from(p.frequency), 55.0);
            assert!(err.abs() <= 10.0, "{label} k={k} f32: {} Hz", p.frequency);
            assert!(
                p.clarity >= 0.95,
                "{label} k={k} f32: clarity {}",
                p.clarity
            );
        }
        checked += 1;
    }
    assert_eq!(checked, 2, "sine_55 entries at both sample rates");
}

/// With every gate open, both detectors report finite clarity in `[0, 1]` on
/// all 28 roster entries (noise, DC and zero frames included) and on 1000
/// seeded random frames, for f32 and f64. Pure tones report clarity of at
/// least 0.9.
#[test]
fn clarity_bounds() {
    let suite = load_signals();
    let rp = &suite.params;
    let mut mpm = McLeodDetector::<f64>::new(rp.mpm_window, rp.mpm_tau_max)
        .unwrap()
        .with_k(rp.mpm_k)
        .with_clarity_threshold(0.0);
    let mut yin = YinDetector::<f64>::new(rp.yin_frame, rp.yin_window)
        .unwrap()
        .with_absolute_threshold(1.0);
    let mut mpm32 = McLeodDetector::<f32>::new(rp.mpm_window, rp.mpm_tau_max)
        .unwrap()
        .with_k(rp.mpm_k as f32)
        .with_clarity_threshold(0.0);
    let mut yin32 = YinDetector::<f32>::new(rp.yin_frame, rp.yin_window)
        .unwrap()
        .with_absolute_threshold(1.0);
    let mut checked = 0;
    for s in &suite.signals {
        let x = generate(&suite, s);
        let x32 = to_f32(&x);
        let sr = s.sample_rate;
        let label = s.label();
        let results = [
            in_contract(mpm.detect(&x, sr), &format!("{label} mpm/f64")).map(|p| p.clarity),
            in_contract(yin.detect(&x, sr), &format!("{label} yin/f64")).map(|p| p.clarity),
            in_contract(mpm32.detect(&x32, sr), &format!("{label} mpm/f32"))
                .map(|p| f64::from(p.clarity)),
            in_contract(yin32.detect(&x32, sr), &format!("{label} yin/f32"))
                .map(|p| f64::from(p.clarity)),
        ];
        if s.is_pure_tone() {
            for (i, c) in results.iter().enumerate() {
                let c = c.unwrap_or_else(|| panic!("{label} [{i}]: no pitch on a pure tone"));
                assert!(c >= 0.9, "{label} [{i}]: pure-tone clarity {c}");
            }
        }
        checked += 1;
    }
    assert_eq!(checked, 28, "conformance roster");

    let mut lcg = Lcg::new(0xc1a7_1e55);
    let mut mpm = McLeodDetector::<f64>::new(1024, 512)
        .unwrap()
        .with_clarity_threshold(0.0);
    let mut yin = YinDetector::<f64>::new(1024, 512)
        .unwrap()
        .with_absolute_threshold(1.0);
    let mut mpm32 = McLeodDetector::<f32>::new(1024, 512)
        .unwrap()
        .with_clarity_threshold(0.0);
    let mut yin32 = YinDetector::<f32>::new(1024, 512)
        .unwrap()
        .with_absolute_threshold(1.0);
    for i in 0..1000 {
        let frame = lcg.frame(1024);
        let frame32 = to_f32(&frame);
        in_contract(mpm.detect(&frame, 44_100), &format!("random {i} mpm/f64"));
        in_contract(yin.detect(&frame, 44_100), &format!("random {i} yin/f64"));
        in_contract(
            mpm32.detect(&frame32, 48_000),
            &format!("random {i} mpm/f32"),
        );
        in_contract(
            yin32.detect(&frame32, 48_000),
            &format!("random {i} yin/f32"),
        );
    }
}

/// Degenerate frames. The flag says whether the frame must yield `None`
/// (`true`) or merely a finite-or-`None` result (`false`): a frame of
/// alternating tiny values is a valid Nyquist tone for a sample type in
/// which the squares do not underflow.
fn edge_frames<T: Float>(frame_len: usize, sample_rate: u32) -> Vec<(&'static str, bool, Vec<T>)> {
    let tone: Vec<T> = sine(frame_len, 440.0, sample_rate);
    let with = |name: &'static str, at: usize, v: T| {
        let mut f = tone.clone();
        f[at] = v;
        (name, true, f)
    };
    let inf = T::from_f64_lossy(f64::INFINITY);
    let nan = T::from_f64_lossy(f64::NAN);
    let tiny = T::from_f64_lossy(f64::from(f32::MIN_POSITIVE));
    let f32_subnormal = T::from_f64_lossy(1e-40);
    let f64_subnormal = T::from_f64_lossy(1e-310);
    vec![
        ("empty", true, Vec::new()),
        ("single sample", true, vec![T::ONE]),
        ("one short", true, tone[..frame_len - 1].to_vec()),
        ("all zero", true, vec![T::ZERO; frame_len]),
        ("dc", true, vec![T::HALF; frame_len]),
        ("negative dc", true, vec![T::ZERO - T::ONE; frame_len]),
        ("f32 min positive", true, vec![tiny; frame_len]),
        ("f32 subnormal", true, vec![f32_subnormal; frame_len]),
        ("f64 subnormal", true, vec![f64_subnormal; frame_len]),
        (
            "alternating f64 subnormal",
            true,
            alternating(frame_len, f64_subnormal),
        ),
        (
            "alternating f32 min positive",
            false,
            alternating(frame_len, tiny),
        ),
        (
            "alternating f32 subnormal",
            false,
            alternating(frame_len, f32_subnormal),
        ),
        with("+inf at 0", 0, inf),
        with("-inf at end", frame_len - 1, T::ZERO - inf),
        with("nan in the middle", frame_len / 2, nan),
        ("all inf", true, vec![inf; frame_len]),
        ("all nan", true, vec![nan; frame_len]),
        ("alternating inf", true, alternating(frame_len, inf)),
    ]
}

fn edge_suite<T: Float + Debug>() {
    for &(frame_len, sub) in &[(1024usize, 512usize), (64, 32), (2, 1)] {
        for &sr in &[44_100u32, 48_000, 8_000, 1, 0] {
            let mut mpm = McLeodDetector::<T>::new(frame_len, sub)
                .unwrap()
                .with_clarity_threshold(T::ZERO);
            let mut yin = YinDetector::<T>::new(frame_len, sub)
                .unwrap()
                .with_absolute_threshold(T::ONE);
            for (name, must_be_none, frame) in edge_frames::<T>(frame_len, sr) {
                let what = format!("{name} ({frame_len}/{sub} @ {sr})");
                let m = in_contract(mpm.detect(&frame, sr), &format!("mpm {what}"));
                let y = in_contract(yin.detect(&frame, sr), &format!("yin {what}"));
                if must_be_none {
                    assert_eq!(m, None, "mpm {what}");
                    assert_eq!(y, None, "yin {what}");
                }
            }
        }
    }

    // A longer frame is analysed over its first `frame_len` samples, and
    // `sample_rate == 0` yields a finite 0.0 Hz pitch.
    let tone: Vec<T> = sine(4096 + 7, 440.0, 44_100);
    let mut mpm = McLeodDetector::<T>::new(4096, 2048).unwrap();
    let mut yin = YinDetector::<T>::new(4096, 2048).unwrap();
    let p = in_contract(mpm.detect(&tone, 44_100), "mpm long frame").expect("mpm long frame");
    assert!((p.frequency.to_f64() - 440.0).abs() < 1.0);
    let p = in_contract(yin.detect(&tone, 44_100), "yin long frame").expect("yin long frame");
    assert!((p.frequency.to_f64() - 440.0).abs() < 1.0);
    let p = in_contract(mpm.detect(&tone, 0), "mpm sr 0").expect("mpm sr 0");
    assert_eq!(p.frequency.to_f64(), 0.0);
    let p = in_contract(yin.detect(&tone, 0), "yin sr 0").expect("yin sr 0");
    assert_eq!(p.frequency.to_f64(), 0.0);
}

/// Degenerate frames return `None` without panicking, for both detectors and
/// both sample types, and the constructors reject zero or absurd sizes with
/// `Err`.
#[test]
fn no_panic_edge_suite() {
    edge_suite::<f64>();
    edge_suite::<f32>();

    assert_eq!(
        McLeodDetector::<f64>::new(0, 1).unwrap_err(),
        ConfigError::ZeroSize { what: "frame_len" }
    );
    assert_eq!(
        McLeodDetector::<f64>::new(16, 0).unwrap_err(),
        ConfigError::ZeroSize { what: "tau_max" }
    );
    assert_eq!(
        YinDetector::<f32>::new(0, 1).unwrap_err(),
        ConfigError::ZeroSize { what: "frame_len" }
    );
    assert_eq!(
        YinDetector::<f32>::new(16, 0).unwrap_err(),
        ConfigError::ZeroSize { what: "window" }
    );
    for absurd in [MAX_FRAME_LEN + 1, usize::MAX / 2, usize::MAX] {
        assert!(matches!(
            McLeodDetector::<f64>::new(absurd, 1).unwrap_err(),
            ConfigError::OutOfRange {
                what: "frame_len",
                ..
            }
        ));
        assert!(matches!(
            YinDetector::<f32>::new(absurd, 1).unwrap_err(),
            ConfigError::OutOfRange {
                what: "frame_len",
                ..
            }
        ));
    }
    assert!(matches!(
        McLeodDetector::<f32>::new(1, 1).unwrap_err(),
        ConfigError::OutOfRange {
            what: "frame_len",
            ..
        }
    ));
    assert!(matches!(
        McLeodDetector::<f64>::new(16, 17).unwrap_err(),
        ConfigError::OutOfRange {
            what: "tau_max",
            ..
        }
    ));
    assert!(matches!(
        McLeodDetector::<f64>::new(16, usize::MAX).unwrap_err(),
        ConfigError::OutOfRange {
            what: "tau_max",
            ..
        }
    ));
    assert!(matches!(
        YinDetector::<f64>::new(16, 16).unwrap_err(),
        ConfigError::OutOfRange { what: "window", .. }
    ));
    assert!(matches!(
        YinDetector::<f64>::new(16, usize::MAX).unwrap_err(),
        ConfigError::OutOfRange { what: "window", .. }
    ));
    assert!(McLeodDetector::<f64>::new(2, 1).is_ok());
    assert!(McLeodDetector::<f64>::new(16, 16).is_ok());
    assert!(YinDetector::<f64>::new(2, 1).is_ok());
    assert!(YinDetector::<f64>::new(16, 15).is_ok());
}

/// A finite alternating frame whose cross-spectrum overflows yields `None`
/// or a finite in-range pitch, with no NaN in the output. Configs (1024, 512)
/// and (64, 32), both roster sample rates, f64 at 1e152 and f32 at 1e17, at
/// the default and the fully open threshold. The 1024-sample configs are
/// pinned to `None`.
#[test]
fn yin_rejects_overflow_frame() {
    let rates = sample_rates(&load_signals());
    assert_eq!(rates, [44_100, 48_000]);
    for &(frame_len, window) in &[(1024usize, 512usize), (64, 32)] {
        let frame64: Vec<f64> = alternating(frame_len, 1e152);
        let frame32: Vec<f32> = alternating(frame_len, 1e17);
        for &sr in &rates {
            for threshold in [0.15_f64, 1.0] {
                let what = format!("({frame_len}, {window}) @ {sr} threshold {threshold}");
                let mut yin = YinDetector::<f64>::new(frame_len, window)
                    .unwrap()
                    .with_absolute_threshold(threshold);
                let p = in_contract(yin.detect(&frame64, sr), &format!("f64 {what}"));
                if frame_len == 1024 {
                    assert_eq!(p, None, "f64 {what}");
                }
                let mut yin = YinDetector::<f32>::new(frame_len, window)
                    .unwrap()
                    .with_absolute_threshold(threshold as f32);
                let p = in_contract(yin.detect(&frame32, sr), &format!("f32 {what}"));
                if frame_len == 1024 {
                    assert_eq!(p, None, "f32 {what}");
                }
            }
        }
    }
}

/// MPM on the same overflow frames. Its power spectrum saturates, the
/// autocorrelation goes non-finite at lag 1, and the 1024-sample frames
/// collapse to `None`.
#[test]
fn mpm_overflow_spot_check() {
    let rates = sample_rates(&load_signals());
    for &(frame_len, tau_max) in &[(1024usize, 512usize), (64, 32)] {
        let frame64: Vec<f64> = alternating(frame_len, 1e152);
        let frame32: Vec<f32> = alternating(frame_len, 1e17);
        for &sr in &rates {
            for threshold in [0.6_f64, 0.0] {
                let what = format!("({frame_len}, {tau_max}) @ {sr} threshold {threshold}");
                let mut mpm = McLeodDetector::<f64>::new(frame_len, tau_max)
                    .unwrap()
                    .with_clarity_threshold(threshold);
                let p = in_contract(mpm.detect(&frame64, sr), &format!("f64 {what}"));
                if frame_len == 1024 {
                    assert_eq!(p, None, "f64 {what}");
                }
                let mut mpm = McLeodDetector::<f32>::new(frame_len, tau_max)
                    .unwrap()
                    .with_clarity_threshold(threshold as f32);
                let p = in_contract(mpm.detect(&frame32, sr), &format!("f32 {what}"));
                if frame_len == 1024 {
                    assert_eq!(p, None, "f32 {what}");
                }
            }
        }
    }
}

fn amplitude_sweep<T: Float + Debug>(exponents: impl Iterator<Item = i32> + Clone, seed: u64) {
    let mut lcg = Lcg::new(seed);
    for &(frame_len, sub) in &[(1024usize, 512usize), (1000, 500), (64, 32)] {
        let mut mpm = McLeodDetector::<T>::new(frame_len, sub)
            .unwrap()
            .with_clarity_threshold(T::ZERO);
        let mut yin = YinDetector::<T>::new(frame_len, sub)
            .unwrap()
            .with_absolute_threshold(T::ONE);
        let mut mpm_default = McLeodDetector::<T>::new(frame_len, sub).unwrap();
        let mut yin_default = YinDetector::<T>::new(frame_len, sub).unwrap();
        for k in exponents.clone() {
            let amp = T::from_f64_lossy(10f64.powi(k));
            let tone: Vec<T> = sine::<T>(frame_len, 440.0, 44_100)
                .iter()
                .map(|&v| v * amp)
                .collect();
            let frames: [(&str, Vec<T>); 3] = [
                ("noise", noise(&mut lcg, frame_len, amp)),
                ("alternating", alternating(frame_len, amp)),
                ("tone", tone),
            ];
            for (name, frame) in &frames {
                let what = format!("{name} 1e{k} ({frame_len}/{sub})");
                in_contract(mpm.detect(frame, 44_100), &format!("mpm open {what}"));
                in_contract(yin.detect(frame, 44_100), &format!("yin open {what}"));
                in_contract(mpm_default.detect(frame, 48_000), &format!("mpm {what}"));
                in_contract(yin_default.detect(frame, 48_000), &format!("yin {what}"));
            }
        }
    }
}

/// Noise, Nyquist-alternating and tone frames swept from amplitude 1 up
/// through the overflow regime and past the point where the input itself is
/// non-finite. Both detectors, both sample types, open and default gates:
/// every result is finite or `None`.
#[test]
fn large_amplitude_frames_are_finite_or_none() {
    amplitude_sweep::<f64>((0..=320).step_by(4), 2_000_064);
    amplitude_sweep::<f32>(0..=40, 2_000_032);
}

/// A 220 Hz sine keeps its pitch under large amplitude scaling and large DC
/// offsets. Scales that overflow YIN's intermediate values return `None`.
#[test]
fn large_amplitude_yin_reports_correct_pitch() {
    let n = 4096;
    let sr = 44_100u32;
    let frequency = 220.0;

    for amplitude in [1.0_f64, 1e3, 1e6, 1e8] {
        let frame = scaled_sine::<f32>(n, frequency, sr, amplitude, 0.0);
        assert_yin_pitch(&frame, sr, frequency, &format!("f32 amplitude {amplitude}"));
    }
    for dc in [1e2_f64, 1e3, 1e4] {
        let frame = scaled_sine::<f32>(n, frequency, sr, 1.0, dc);
        assert_yin_pitch(&frame, sr, frequency, &format!("f32 dc {dc}"));
    }
    for amplitude in [1e6_f64, 1e8, 1e150] {
        let frame = scaled_sine::<f64>(n, frequency, sr, amplitude, 0.0);
        assert_yin_pitch(&frame, sr, frequency, &format!("f64 amplitude {amplitude}"));
    }
    for dc in [1e6_f64, 1e7, 1e8] {
        let frame = scaled_sine::<f64>(n, frequency, sr, 1.0, dc);
        assert_yin_pitch(&frame, sr, frequency, &format!("f64 dc {dc}"));
    }

    for amplitude in [1e16_f64, 1e18] {
        let frame = scaled_sine::<f32>(n, frequency, sr, amplitude, 0.0);
        let mut yin = YinDetector::<f32>::new(n, n / 2).unwrap();
        assert_eq!(
            in_contract(yin.detect(&frame, sr), &format!("f32 overflow {amplitude}")),
            None
        );
    }
    let frame = scaled_sine::<f64>(n, frequency, sr, 1e152, 0.0);
    let mut yin = YinDetector::<f64>::new(n, n / 2).unwrap();
    assert_eq!(
        in_contract(yin.detect(&frame, sr), "f64 overflow 1e152"),
        None
    );
}

/// A unit 440 Hz sine plus DC 400 for both sample types, and plus DC 1e7 for
/// f64. YIN's difference function is DC-invariant, so it reports the sine.
/// MPM does not remove DC, so it may still return `None`.
#[test]
fn low_ac_over_dc_sine_is_detected() {
    let n = 4096;
    let sr = 44_100u32;
    let dc = 400.0;
    let frame64 = scaled_sine::<f64>(n, 440.0, sr, 1.0, dc);
    let frame32: Vec<f32> = frame64.iter().map(|&v| v as f32).collect();

    assert_yin_pitch(&frame32, sr, 440.0, "yin/f32 dc 400");
    assert_yin_pitch(&frame64, sr, 440.0, "yin/f64 dc 400");
    let high_dc_frame = scaled_sine::<f64>(n, 440.0, sr, 1.0, 1e7);
    assert_yin_pitch(&high_dc_frame, sr, 440.0, "yin/f64 dc 1e7");

    let mut mpm32 = McLeodDetector::<f32>::new(n, n / 2).unwrap();
    let mut mpm64 = McLeodDetector::<f64>::new(n, n / 2).unwrap();
    in_contract(mpm32.detect(&frame32, sr), "mpm/f32");
    in_contract(mpm64.detect(&frame64, sr), "mpm/f64");
}

/// Two identical 24-sample end segments and a 1e8 centre transient. Exact
/// n(40) = 1. A floor of m'(tau) against m'(0) used to write n(40) = 0 and
/// return `None`.
#[test]
fn separated_matching_segments_are_not_erased_by_unrelated_center_energy() {
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
    assert!(pitch.clarity > 0.9, "clarity {}", pitch.clarity);
    assert!(
        (pitch.frequency - 1200.0).abs() < 50.0,
        "frequency {}",
        pitch.frequency
    );
}

/// A 1e-6-amplitude 220 Hz sine at `power_threshold` 0 is a real pitched
/// frame. Both detectors report it, both sample types, with the right f0
/// and clarity in `[0, 1]`. A 1e-20 AC floor used to drop a still-smaller
/// 1e-12 tone that the mean-square gate would keep.
#[test]
fn low_amplitude_tone_detected_at_zero_threshold() {
    let n = 4096;
    let sr = 44_100u32;
    let freq = 220.0;
    for &amp in &[1e-6_f64, 1e-12] {
        let frame64: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64 / f64::from(sr);
                amp * (2.0 * std::f64::consts::PI * freq * t).sin()
            })
            .collect();
        let frame32: Vec<f32> = frame64.iter().map(|&v| v as f32).collect();
        let what = format!("amp {amp}");

        let mut mpm = McLeodDetector::<f64>::new(n, n / 2)
            .unwrap()
            .with_power_threshold(0.0);
        let mut yin = YinDetector::<f64>::new(n, n / 2)
            .unwrap()
            .with_power_threshold(0.0);
        let p = in_contract(mpm.detect(&frame64, sr), &format!("mpm/f64 {what}"))
            .unwrap_or_else(|| panic!("mpm/f64 {what}: dropped a pitched tone"));
        assert!(
            cents(p.frequency, freq).abs() <= 10.0,
            "mpm/f64 {what}: {} Hz",
            p.frequency
        );
        assert!((0.0..=1.0).contains(&p.clarity));
        let p = in_contract(yin.detect(&frame64, sr), &format!("yin/f64 {what}"))
            .unwrap_or_else(|| panic!("yin/f64 {what}: dropped a pitched tone"));
        assert!(
            cents(p.frequency, freq).abs() <= 10.0,
            "yin/f64 {what}: {} Hz",
            p.frequency
        );
        assert!((0.0..=1.0).contains(&p.clarity));

        let mut mpm = McLeodDetector::<f32>::new(n, n / 2)
            .unwrap()
            .with_power_threshold(0.0);
        let mut yin = YinDetector::<f32>::new(n, n / 2)
            .unwrap()
            .with_power_threshold(0.0);
        let p = in_contract(mpm.detect(&frame32, sr), &format!("mpm/f32 {what}"))
            .unwrap_or_else(|| panic!("mpm/f32 {what}: dropped a pitched tone"));
        assert!(
            cents(f64::from(p.frequency), freq).abs() <= 10.0,
            "mpm/f32 {what}: {} Hz",
            p.frequency
        );
        let p = in_contract(yin.detect(&frame32, sr), &format!("yin/f32 {what}"))
            .unwrap_or_else(|| panic!("yin/f32 {what}: dropped a pitched tone"));
        assert!(
            cents(f64::from(p.frequency), freq).abs() <= 10.0,
            "yin/f32 {what}: {} Hz",
            p.frequency
        );
    }
}

/// Silence, a constant frame, and a DC offset return `None`. They pass the
/// mean-square gate at threshold 0 and then fail to produce a key maximum
/// (MPM) or a CMNDF dip (YIN).
#[test]
fn dc_and_constant_still_none() {
    let n = 4096;
    let sr = 44_100u32;
    let frames64: [(&str, Vec<f64>); 4] = [
        ("silence", vec![0.0; n]),
        ("constant", vec![0.5; n]),
        ("negative constant", vec![-1.0; n]),
        ("dc", vec![400.0; n]),
    ];
    let mut mpm = McLeodDetector::<f64>::new(n, n / 2)
        .unwrap()
        .with_power_threshold(0.0)
        .with_clarity_threshold(0.0);
    let mut yin = YinDetector::<f64>::new(n, n / 2)
        .unwrap()
        .with_power_threshold(0.0)
        .with_absolute_threshold(1.0);
    let mut mpm32 = McLeodDetector::<f32>::new(n, n / 2)
        .unwrap()
        .with_power_threshold(0.0)
        .with_clarity_threshold(0.0);
    let mut yin32 = YinDetector::<f32>::new(n, n / 2)
        .unwrap()
        .with_power_threshold(0.0)
        .with_absolute_threshold(1.0);
    for (name, frame) in &frames64 {
        let frame32: Vec<f32> = frame.iter().map(|&v| v as f32).collect();
        assert_eq!(
            in_contract(mpm.detect(frame, sr), &format!("mpm/f64 {name}")),
            None,
            "mpm/f64 {name}"
        );
        assert_eq!(
            in_contract(yin.detect(frame, sr), &format!("yin/f64 {name}")),
            None,
            "yin/f64 {name}"
        );
        assert_eq!(
            in_contract(mpm32.detect(&frame32, sr), &format!("mpm/f32 {name}")),
            None,
            "mpm/f32 {name}"
        );
        assert_eq!(
            in_contract(yin32.detect(&frame32, sr), &format!("yin/f32 {name}")),
            None,
            "yin/f32 {name}"
        );
    }
}

/// Two identical frames in a row yield the same pitch. The detector keeps no
/// history between calls.
#[test]
fn detect_is_stateless() {
    let frame: Vec<f64> = sine(1024, 220.0, 44_100);
    let mut mpm = McLeodDetector::<f64>::new(1024, 512).unwrap();
    let mut yin = YinDetector::<f64>::new(1024, 512).unwrap();
    let a = mpm.detect(&frame, 44_100);
    let b = mpm.detect(&frame, 44_100);
    assert_eq!(a, b);
    let a = yin.detect(&frame, 44_100);
    let b = yin.detect(&frame, 44_100);
    assert_eq!(a, b);
    assert!(a.is_some());
}
