//! Realtime monophonic pitch detection with the McLeod pitch method (MPM)
//! and YIN.
//!
//! Both detectors take one frame of `f32` or `f64` samples and return the
//! fundamental frequency with a clarity score in `[0, 1]`, or `None` when the
//! frame has no pitch that clears the configured gates. Estimation is per
//! frame and stateless: a detector keeps no history between calls and applies
//! no smoothing across frames.
//!
//! ```
//! use pitch_estimate::{McLeodDetector, PitchDetector, YinDetector};
//!
//! let sample_rate = 44_100;
//! let frame: Vec<f64> = (0..4096)
//!     .map(|i| (2.0 * std::f64::consts::PI * 440.0 * i as f64 / sample_rate as f64).sin())
//!     .collect();
//!
//! let mut mpm = McLeodDetector::<f64>::new(4096, 2048)?;
//! let pitch = mpm.detect(&frame, sample_rate).expect("a clean sine has a pitch");
//! assert!((pitch.frequency - 440.0).abs() < 0.5);
//! assert!(pitch.clarity > 0.99);
//!
//! let mut yin = YinDetector::<f64>::new(4096, 2048)?;
//! let pitch = yin.detect(&frame, sample_rate).expect("a clean sine has a pitch");
//! assert!((pitch.frequency - 440.0).abs() < 0.5);
//! # Ok::<(), pitch_estimate::ConfigError>(())
//! ```
//!
//! # Contract
//!
//! For every finite frame, `detect` returns either `None` or a `Pitch` whose
//! `frequency` is finite and whose `clarity` is finite and in `[0, 1]`. That
//! holds when the frame's own arithmetic overflows inside the FFT: a
//! non-finite autocorrelation, difference, or CMNDF entry is discarded before
//! any candidate is chosen. Frames with a non-finite sample, an empty or too
//! short frame, an all-zero frame, and a constant (DC) frame return `None`
//! and never panic.
//!
//! The detectors allocate their scratch buffers once, in the constructor.
//! `detect` performs no heap allocation.
//!
//! The crate uses the standard library (the FFT layer needs it) and contains
//! no `unsafe` code.
//!
//! # Algorithms
//!
//! [`McLeodDetector`] computes the linear autocorrelation `r(tau)` through a
//! zero-padded real FFT, divides the product by the transform length so the
//! values are in time-domain units, seeds the two-segment squared-sum term
//! `m'(tau)` from the time-domain sum of squares, and forms the NSDF
//! `n(tau) = 2 r(tau) / m'(tau)`, which lies in `[-1, 1]`. Key maxima between
//! zero crossings are refined by parabolic interpolation. The first one at or
//! above `k` times the largest is the period, and its height is the clarity.
//!
//! [`YinDetector`] computes the difference function `d(tau)` over a fixed
//! integration window through a cross-correlation FFT, normalizes it to the
//! CMNDF `d'(tau) = d(tau) * tau / (d(1) + ... + d(tau))` (so `d'(1) = 1`),
//! takes the first lag at which `d'` drops below the absolute threshold,
//! descends to the local minimum, refines it parabolically, and reports
//! `clarity = 1 - d'_min` clamped to `[0, 1]`.
//!
//! Neither detector applies a window function. YIN subtracts the frame mean
//! before its transform because its difference function is DC-invariant. Remove
//! DC before using MPM. A constant offset pushes the NSDF toward 1 at every lag
//! and hides the zero crossings MPM needs.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::fmt;

mod error;
mod fft;
mod float;
mod interp;
mod mpm;
mod stats;
mod yin;

#[cfg(test)]
mod reference;
#[cfg(test)]
mod unit_tests;

pub use error::{ConfigError, MAX_FRAME_LEN};
pub use float::Float;
pub use mpm::McLeodDetector;
pub use yin::YinDetector;

/// Result of a detection pass over one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pitch<T> {
    /// Estimated fundamental, Hz.
    pub frequency: T,
    /// Confidence in `[0, 1]`. Never NaN, never outside `[0, 1]`.
    pub clarity: T,
}

impl<T: fmt::Display> fmt::Display for Pitch<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} Hz, clarity {}", self.frequency, self.clarity)
    }
}

/// A frame-by-frame pitch detector.
pub trait PitchDetector<T: Float> {
    /// Detect the pitch of `frame` sampled at `sample_rate` Hz.
    ///
    /// Returns `None` when no pitch clears the configured gates (power floor,
    /// clarity or absolute threshold). Degenerate input returns `None` without
    /// a panic: an empty frame, a frame shorter than the configured frame
    /// length, an all-zero frame, a constant (DC) frame, or a frame containing
    /// a non-finite sample. A frame longer than the
    /// configured length is analysed over its first `frame_len` samples.
    ///
    /// `sample_rate == 0` returns a finite 0.0 Hz pitch, which is in
    /// contract (finite, clarity in `[0, 1]`) but odd.
    fn detect(&mut self, frame: &[T], sample_rate: u32) -> Option<Pitch<T>>;
}
