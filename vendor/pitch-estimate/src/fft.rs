//! Linear correlation through a zero-padded real FFT.
//!
//! Every product leaving this module is divided by the transform length, so
//! the values equal the time-domain sums to rounding. Nothing downstream ever
//! sees an FFT-scaled number.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::num_traits::Zero;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

use crate::float::Float;

/// Owns the FFT plans and every scratch buffer needed to correlate frames
/// without allocating.
pub(crate) struct Correlator<T> {
    len: usize,
    forward: Arc<dyn RealToComplex<T>>,
    inverse: Arc<dyn ComplexToReal<T>>,
    input: Vec<T>,
    spectrum_a: Vec<Complex<T>>,
    spectrum_b: Vec<Complex<T>>,
    scratch: Vec<Complex<T>>,
    output: Vec<T>,
}

impl<T: Float> Correlator<T> {
    /// Build a correlator whose transform is the smallest power of two that is
    /// at least `min_len`, so lags `0..out.len()` of a correlation between
    /// slices `a` and `b` with `a.len() + out.len() - 1 <= min_len` and
    /// `b.len() <= min_len` never wrap around.
    pub(crate) fn new(min_len: usize) -> Self {
        let len = min_len.max(2).next_power_of_two();
        let mut planner = RealFftPlanner::<T>::new();
        let forward = planner.plan_fft_forward(len);
        let inverse = planner.plan_fft_inverse(len);
        let scratch_len = forward.get_scratch_len().max(inverse.get_scratch_len());
        Self {
            len,
            input: vec![T::ZERO; len],
            spectrum_a: vec![Complex::zero(); len / 2 + 1],
            spectrum_b: vec![Complex::zero(); len / 2 + 1],
            scratch: vec![Complex::zero(); scratch_len],
            output: vec![T::ZERO; len],
            forward,
            inverse,
        }
    }

    /// Transform length M.
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Forward transform of `x` zero-padded to M, written into `self.output`'s
    /// spectral companion selected by `which` (0 = a, 1 = b).
    fn transform(&mut self, x: &[T], which: usize) {
        debug_assert!(x.len() <= self.len);
        self.input[..x.len()].copy_from_slice(x);
        for v in &mut self.input[x.len()..] {
            *v = T::ZERO;
        }
        let spectrum = if which == 0 {
            &mut self.spectrum_a
        } else {
            &mut self.spectrum_b
        };
        // The only error realfft can report here is a length mismatch, which
        // the constructor rules out.
        let _ = self
            .forward
            .process_with_scratch(&mut self.input, spectrum, &mut self.scratch);
    }

    /// Inverse-transform `spectrum_a`, divide by M, and copy lags `0..out.len()`
    /// into `out`.
    fn finish(&mut self, out: &mut [T]) {
        debug_assert!(out.len() <= self.len);
        // Packed real spectra carry no imaginary part at DC and Nyquist. The
        // products formed by the callers keep that property up to the sign of
        // zero, and a non-finite spectrum is passed through on purpose so the
        // detectors can observe it. realfft still performs the transform when
        // it flags those slots, so the result is ignored either way.
        self.spectrum_a[0].im = T::ZERO;
        let last = self.spectrum_a.len() - 1;
        self.spectrum_a[last].im = T::ZERO;
        let _ = self.inverse.process_with_scratch(
            &mut self.spectrum_a,
            &mut self.output,
            &mut self.scratch,
        );
        let scale = T::from_usize_lossy(self.len);
        for (o, v) in out.iter_mut().zip(&self.output) {
            *o = *v / scale;
        }
    }

    /// Linear autocorrelation: `out[tau] = sum_j x[j] * x[j + tau]` over every
    /// `j` with `j + tau < x.len()`.
    ///
    /// Requires `x.len() + out.len() - 1 <= M`.
    pub(crate) fn autocorrelate(&mut self, x: &[T], out: &mut [T]) {
        debug_assert!(x.len() + out.len() <= self.len + 1);
        self.transform(x, 0);
        for c in &mut self.spectrum_a {
            let power = c.re * c.re + c.im * c.im;
            *c = Complex::new(power, T::ZERO);
        }
        self.finish(out);
    }

    /// Linear cross-correlation: `out[tau] = sum_j a[j] * b[j + tau]` over
    /// every `j < a.len()` with `j + tau < b.len()`.
    ///
    /// Requires `a.len() + out.len() - 1 <= M` and `b.len() <= M`.
    pub(crate) fn cross_correlate(&mut self, a: &[T], b: &[T], out: &mut [T]) {
        debug_assert!(a.len() + out.len() <= self.len + 1);
        debug_assert!(b.len() <= self.len);
        self.transform(a, 1);
        self.transform(b, 0);
        for (x, y) in self.spectrum_a.iter_mut().zip(&self.spectrum_b) {
            *x = *x * y.conj();
        }
        self.finish(out);
    }
}
