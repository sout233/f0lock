# Changelog

Notable changes to this crate are recorded here. The format follows Keep a Changelog, and the crate follows semantic versioning.

## 0.1.0 - 2026-08-22

Initial release.

### Added

- `McLeodDetector`: the McLeod pitch method. Linear autocorrelation through a zero-padded real FFT in time-domain units, the normalized square difference function seeded from the time-domain sum of squares, key-maximum picking with the relative constant `k`, and parabolic refinement.
- `YinDetector`: YIN. Difference function over a fixed integration window, cumulative mean normalized difference function with the `tau` multiplier so that `d'(1) = 1`, absolute-threshold dip selection with descent to the local minimum, and parabolic refinement. Clarity is `1 - d'_min` clamped to `[0, 1]`.
- `PitchDetector` trait, `Pitch` result, `ConfigError`, and the sealed `Float` trait for `f32` and `f64`.
- Finite-input guarantee: for every finite frame, a reported pitch has finite frequency and clarity in `[0, 1]`, including frames whose intermediate values overflow. Degenerate frames return `None` without panicking.
- Allocation-free `detect`, pinned by a counting-allocator test.
- A 28-signal conformance replay, named regression tests for each defect class, and a CI gate on Rust 1.70.
- The only amplitude gate is the mean-square `power_threshold`, default 0. A low-amplitude pitched frame is detected when that floor is 0. Constant and silent frames return `None` from the detector: MPM finds no key maximum, and YIN finds no CMNDF dip.
- NSDF is zero only when `m'(tau)` is zero. A high-lag peak is kept when energy elsewhere in the frame is large.
