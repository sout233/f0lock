//! The sealed sample-type trait.

use realfft::FftNum;

mod sealed {
    pub trait Sealed {}
    impl Sealed for f32 {}
    impl Sealed for f64 {}
}

/// Sample type accepted by the detectors: `f32` or `f64`.
///
/// The trait is sealed. It adds the conversions and constants the detectors
/// need. `realfft::FftNum` is a public supertrait because the FFT layer
/// requires it. Callers never implement `Float`. They only name it as the
/// bound on [`crate::PitchDetector`].
pub trait Float: sealed::Sealed + FftNum + PartialOrd {
    /// Zero.
    const ZERO: Self;
    /// One.
    const ONE: Self;
    /// One half.
    const HALF: Self;

    /// Convert a lag or length. Exact for every value the detectors use.
    fn from_usize_lossy(n: usize) -> Self;
    /// Convert a sample rate.
    fn from_u32_lossy(n: u32) -> Self;
    /// Convert a tuning constant.
    fn from_f64_lossy(v: f64) -> Self;
    /// Widen to `f64`.
    fn to_f64(self) -> f64;
    /// `true` for every value that is neither infinite nor NaN.
    fn is_finite(self) -> bool;
    /// `true` for NaN.
    fn is_nan(self) -> bool;
    /// Clamp into `[lo, hi]`. NaN passes through unchanged, and both
    /// detectors test the clamped value with `is_finite` before reporting it.
    fn clamp_to(self, lo: Self, hi: Self) -> Self {
        if self < lo {
            lo
        } else if self > hi {
            hi
        } else {
            self
        }
    }
}

impl Float for f32 {
    const ZERO: Self = 0.0;
    const ONE: Self = 1.0;
    const HALF: Self = 0.5;

    #[inline]
    fn from_usize_lossy(n: usize) -> Self {
        n as f32
    }
    #[inline]
    fn from_u32_lossy(n: u32) -> Self {
        n as f32
    }
    #[inline]
    fn from_f64_lossy(v: f64) -> Self {
        v as f32
    }
    #[inline]
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
    #[inline]
    fn is_finite(self) -> bool {
        f32::is_finite(self)
    }
    #[inline]
    fn is_nan(self) -> bool {
        f32::is_nan(self)
    }
}

impl Float for f64 {
    const ZERO: Self = 0.0;
    const ONE: Self = 1.0;
    const HALF: Self = 0.5;

    #[inline]
    fn from_usize_lossy(n: usize) -> Self {
        n as f64
    }
    #[inline]
    fn from_u32_lossy(n: u32) -> Self {
        f64::from(n)
    }
    #[inline]
    fn from_f64_lossy(v: f64) -> Self {
        v
    }
    #[inline]
    fn to_f64(self) -> f64 {
        self
    }
    #[inline]
    fn is_finite(self) -> bool {
        f64::is_finite(self)
    }
    #[inline]
    fn is_nan(self) -> bool {
        f64::is_nan(self)
    }
}
