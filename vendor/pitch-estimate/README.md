# pitch-estimate

Per-frame monophonic pitch detection for `f32` and `f64` samples, with the McLeod pitch method (MPM) and YIN. One estimate per frame, stateless: no history, no smoothing, no tracking across frames. For a streaming f0 tracker see pitch-core.

## Usage

```rust
use pitch_estimate::{McLeodDetector, PitchDetector, YinDetector};

fn main() -> Result<(), pitch_estimate::ConfigError> {
    let sample_rate = 44_100;
    let frame: Vec<f64> = (0..4096)
        .map(|i| (2.0 * std::f64::consts::PI * 440.0 * i as f64 / sample_rate as f64).sin())
        .collect();

    // Frame of 4096 samples, lags up to 2048. Size tau_max past the longest
    // period of interest so the NSDF peak can close.
    let mut mpm = McLeodDetector::<f64>::new(4096, 2048)?;
    if let Some(pitch) = mpm.detect(&frame, sample_rate) {
        println!("{:.2} Hz, clarity {:.3}", pitch.frequency, pitch.clarity);
    }

    // Frame of 4096 samples with a 2048-sample integration window.
    let mut yin = YinDetector::<f64>::new(4096, 2048)?.with_absolute_threshold(0.1);
    if let Some(pitch) = yin.detect(&frame, sample_rate) {
        println!("{:.2} Hz, clarity {:.3}", pitch.frequency, pitch.clarity);
    }
    Ok(())
}
```

Construct a detector once and reuse it. The constructor allocates every scratch buffer, and `detect` allocates nothing.

## Contract

- For every finite frame, `detect` returns either `None` or a `Pitch` whose `frequency` is finite and whose `clarity` is finite and in `[0, 1]`. This holds when the frame's own arithmetic overflows inside the FFT: a non-finite autocorrelation, difference, or CMNDF entry is discarded before any candidate is chosen.
- An empty frame, a frame shorter than the configured length, an all-zero frame, a constant (DC) frame, and a frame with a non-finite sample return `None`. Nothing panics.
- A frame longer than the configured length is analysed over its first `frame_len` samples.
- `sample_rate == 0` returns a finite 0.0 Hz pitch.
- Neither detector applies a window function. YIN subtracts the frame mean before its transform because its difference function is DC-invariant. Remove DC before using MPM.
- The constructors return `Err(ConfigError)` for a zero size, a lag range that does not fit the frame, or a frame longer than `MAX_FRAME_LEN`.

## Detectors

`McLeodDetector::new(frame_len, tau_max)` examines lags `1..=tau_max`. A key maximum needs a negative-going zero crossing after its peak, so a final positive NSDF run cut off by `tau_max` is not a candidate and a period at lag `tau_max` is never reported. Size `tau_max` larger than the longest period of interest, with room to close that peak's lobe. The detector computes the linear autocorrelation through a zero-padded real FFT, divides the product by the transform length so every value is in time-domain units, seeds the two-segment squared-sum term from the time-domain sum of squares, and forms the normalized square difference function, which lies in `[-1, 1]`. Key maxima between zero crossings are refined by parabolic interpolation. The first one at or above `k` times the largest is the period, and its height is the clarity.

- `with_k`: relative-peak constant, default 0.9, clamped to `[0.8, 1.0]`.
- `with_clarity_threshold`: peaks below it yield `None`, default 0.6.
- `with_power_threshold`: mean-square floor, default 0.

`YinDetector::new(frame_len, window)` examines lags `1..=frame_len - window`. It computes the difference function over the fixed integration window, normalizes it to the cumulative mean normalized difference function `d'(tau) = d(tau) * tau / (d(1) + ... + d(tau))` so that `d'(1) = 1`, takes the first lag at which `d'` drops below the absolute threshold, descends to the local minimum, refines it parabolically, and reports `clarity = 1 - d'_min` clamped to `[0, 1]`.

- `with_absolute_threshold`: default 0.15. The paper's range is 0.1 to 0.2.
- `with_clarity_threshold(c)`: sets the absolute threshold to `1 - c`.
- `with_power_threshold`: mean-square floor, default 0.

## Verification

Named tests in the crate's suite:

- `conformance_accuracy_f64`, `conformance_accuracy_f32`: 28 signals (14 specs at 44100 and 48000 Hz: sines from 55 to 880 Hz, harmonic complexes, a noisy tone, an octave-ambiguous tone, noise, DC, and silence) regenerated from a frozen generator spec and checked against stored fingerprints. Both detectors land within the per-entry cents tolerance with clarity at or above the per-entry floor.
- `low_pitch_regression`: the 55 Hz sine passes MPM at clarity threshold 0.9 with clarity at least 0.95, at both rates and both sample types.
- `nsdf_unit`, `cmndf_index_unit`, `cmndf_d1_is_one_for_random_frames`: the formulas on hand-computed vectors, and `d'(1) = 1` on random frames.
- `clarity_bounds`: with every gate open, clarity is finite and in `[0, 1]` on all 28 signals and 1000 seeded random frames, and pure tones report at least 0.9.
- `no_panic_edge_suite`: the degenerate frames listed under Contract, for both detectors, both sample types, several sizes and sample rates including 0.
- `yin_rejects_overflow_frame`, `mpm_overflow_spot_check`, `large_amplitude_frames_are_finite_or_none`: finite frames whose intermediates overflow, at the named amplitudes and across a sweep up to non-finite input.
- `large_amplitude_yin_reports_correct_pitch`: a 220 Hz sine stays within 10 cents under large amplitude scaling and DC offsets for both sample types. Overflow scales return `None`.
- `detect_is_zero_alloc`: a counting allocator wraps the system allocator and sees zero allocations across `detect` calls for both detectors, `f32` and `f64`, frame lengths 2048 and 1000, on sine, noise, silent, and overflow frames.
- `differential_fft_vs_time_domain`: the FFT path matches a direct time-domain evaluation of both curves to 1e-6 relative on 20 seeded frames.
- `low_amplitude_tone_detected_at_zero_threshold`: a 1e-6-amplitude 220 Hz sine at `power_threshold` 0 is reported by both detectors, both sample types, with clarity in `[0, 1]`.
- `dc_and_constant_still_none`: silence, a constant frame, and a DC offset return `None`.
- `low_ac_over_dc_sine_is_detected`: a 440 Hz sine plus DC 400 is reported by YIN at `f32` and `f64`.
- `separated_matching_segments_are_not_erased_by_unrelated_center_energy`: matching end segments keep their NSDF peak when a large transient sits between them.
- `detect_is_stateless`: two identical frames in a row yield the same pitch.

CI runs these on stable in debug and release, with clippy at `-D warnings` and rustfmt, and builds the library and its doctests on Rust 1.70.

## Requirements

Rust 1.70 or newer. The standard library is required because the FFT layer needs it. One dependency, `realfft`. The crate is `#![forbid(unsafe_code)]`.

## License

MIT
