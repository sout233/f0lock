//! YIN: difference function, CMNDF, absolute-threshold dip picking.

use crate::error::ConfigError;
use crate::fft::Correlator;
use crate::float::Float;
use crate::interp::refine;
use crate::stats::{check_frame_len, or_default, screen};
use crate::{Pitch, PitchDetector};

/// Default absolute threshold on the CMNDF for [`YinDetector`].
pub(crate) const DEFAULT_ABSOLUTE_THRESHOLD: f64 = 0.15;

/// Pitch detector using YIN: difference function over a fixed integration
/// window, cumulative mean normalized difference function (CMNDF), absolute
/// threshold with descent to the local minimum, parabolic refinement.
///
/// Construct once with [`YinDetector::new`], tune with the `with_*` setters,
/// then call [`PitchDetector::detect`] per frame. Every buffer is allocated by
/// the constructor, and `detect` does not allocate.
pub struct YinDetector<T> {
    frame_len: usize,
    window: usize,
    tau_max: usize,
    absolute_threshold: T,
    power_threshold: T,
    frame_scratch: Vec<T>,
    correlator: Correlator<T>,
    xcorr: Vec<T>,
    diff: Vec<T>,
    cmndf: Vec<T>,
    #[cfg(test)]
    valid: usize,
}

impl<T: Float> YinDetector<T> {
    /// Create a detector for frames of `frame_len` samples with integration
    /// window `window`. Lags `1..=frame_len - window` are examined.
    ///
    /// `window` must satisfy `1 <= window < frame_len`, and `frame_len` must
    /// lie in `2..=MAX_FRAME_LEN`. The lowest detectable frequency is
    /// `sample_rate / (frame_len - window)`. A window of half the frame is the
    /// usual choice. Any length is accepted. The FFT is padded to the next
    /// power of two, so nearby lengths often share a transform size. Returns
    /// `Err` for any size outside those bounds.
    pub fn new(frame_len: usize, window: usize) -> Result<Self, ConfigError> {
        check_frame_len(frame_len)?;
        if window == 0 {
            return Err(ConfigError::ZeroSize { what: "window" });
        }
        if window >= frame_len {
            return Err(ConfigError::OutOfRange {
                what: "window",
                value: window,
                min: 1,
                max: frame_len - 1,
            });
        }
        let tau_max = frame_len - window;
        Ok(Self {
            frame_len,
            window,
            tau_max,
            absolute_threshold: T::from_f64_lossy(DEFAULT_ABSOLUTE_THRESHOLD),
            power_threshold: T::ZERO,
            frame_scratch: vec![T::ZERO; frame_len],
            correlator: Correlator::new(frame_len),
            xcorr: vec![T::ZERO; tau_max + 1],
            diff: vec![T::ZERO; tau_max + 1],
            cmndf: vec![T::ONE; tau_max + 1],
            #[cfg(test)]
            valid: 0,
        })
    }

    /// Set the absolute threshold on the CMNDF directly. The paper's range is
    /// 0.1 to 0.2, and the default is 0.15. NaN selects the default.
    #[must_use]
    pub fn with_absolute_threshold(mut self, threshold: T) -> Self {
        self.absolute_threshold =
            or_default(threshold, T::from_f64_lossy(DEFAULT_ABSOLUTE_THRESHOLD));
        self
    }

    /// Set the threshold as a clarity: `absolute_threshold = 1 - clarity`.
    /// A clarity threshold of 0.85 is the default 0.15 absolute threshold.
    /// NaN selects the default.
    #[must_use]
    pub fn with_clarity_threshold(self, clarity: T) -> Self {
        if clarity.is_nan() {
            self.with_absolute_threshold(clarity)
        } else {
            self.with_absolute_threshold(T::ONE - clarity)
        }
    }

    /// Set the power threshold: a frame whose mean square is below it yields
    /// no pitch. Default 0. NaN selects the default.
    #[must_use]
    pub fn with_power_threshold(mut self, threshold: T) -> Self {
        self.power_threshold = or_default(threshold, T::ZERO);
        self
    }

    /// Frame length in samples.
    pub fn frame_len(&self) -> usize {
        self.frame_len
    }

    /// Integration window `W` in samples.
    pub fn window(&self) -> usize {
        self.window
    }

    /// Largest lag examined, `frame_len - window`.
    pub fn tau_max(&self) -> usize {
        self.tau_max
    }

    /// Absolute threshold on the CMNDF.
    pub fn absolute_threshold(&self) -> T {
        self.absolute_threshold
    }

    /// The threshold expressed as a clarity, `1 - absolute_threshold`.
    pub fn clarity_threshold(&self) -> T {
        T::ONE - self.absolute_threshold
    }

    /// Power threshold.
    pub fn power_threshold(&self) -> T {
        self.power_threshold
    }

    /// Transform length used by the FFT path.
    #[cfg(test)]
    pub(crate) fn fft_len(&self) -> usize {
        self.correlator.len()
    }

    /// The difference function `d(0..=tau_max)` from the most recent `detect`
    /// call that reached the transform stage.
    #[cfg(test)]
    pub(crate) fn difference(&self) -> &[T] {
        &self.diff
    }

    /// The CMNDF curve `d'(0..=tau_max)` from the most recent `detect` call
    /// that reached the transform stage.
    #[cfg(test)]
    pub(crate) fn cmndf(&self) -> &[T] {
        &self.cmndf
    }

    /// Number of leading CMNDF entries that took part in the most recent dip
    /// search. 0 before the first call and after a frame that was screened out.
    #[cfg(test)]
    pub(crate) fn valid_len(&self) -> usize {
        self.valid
    }

    /// Fill `self.diff` and `self.cmndf` from the frame `x` (length
    /// `frame_len`) and return the number of leading finite entries.
    fn compute_curves(&mut self, x: &[T]) -> usize {
        let w = self.window;
        let tau_max = self.tau_max;
        let mut sum = T::ZERO;
        for &value in x {
            sum = sum + value;
        }
        let mean = sum / T::from_usize_lossy(self.frame_len);
        for (value, &sample) in self.frame_scratch.iter_mut().zip(x) {
            *value = sample - mean;
        }
        let x = &self.frame_scratch;
        self.correlator
            .cross_correlate(&x[..w], x, &mut self.xcorr[..=tau_max]);

        let mut e0 = T::ZERO;
        for &v in &x[..w] {
            e0 = e0 + v * v;
        }
        let two = T::ONE + T::ONE;
        let mut e_tau = e0;
        let mut limit = tau_max + 1;
        self.diff[0] = T::ZERO;
        for tau in 1..=tau_max {
            let leaving = x[tau - 1];
            let entering = x[tau + w - 1];
            e_tau = e_tau + entering * entering - leaving * leaving;
            let d = e0 + e_tau - two * self.xcorr[tau];
            if !d.is_finite() {
                limit = tau;
                break;
            }
            self.diff[tau] = if d < T::ZERO { T::ZERO } else { d };
        }
        let limit = cmndf_in_place(&self.diff[..limit], &mut self.cmndf);
        for v in &mut self.diff[limit..] {
            *v = T::ZERO;
        }
        for v in &mut self.cmndf[limit..] {
            *v = T::ONE;
        }
        limit
    }
}

/// Compute the CMNDF of `d` into `out[..d.len()]` and return the number of
/// leading entries that are finite.
///
/// `out[0] = 1`, and `out[tau] = d[tau] * tau / S(tau)` with the running sum
/// `S(tau) = d[1] + ... + d[tau]`. `S(tau) = 0` gives 1 (no dip). A non-finite
/// `d[tau]` or `S(tau)` ends the finite prefix: that entry and everything
/// after it carry no information and are not written.
pub(crate) fn cmndf_in_place<T: Float>(d: &[T], out: &mut [T]) -> usize {
    if d.is_empty() {
        return 0;
    }
    out[0] = T::ONE;
    let mut running = T::ZERO;
    for tau in 1..d.len() {
        let value = d[tau];
        running = running + value;
        if !value.is_finite() || !running.is_finite() {
            return tau;
        }
        out[tau] = if running <= T::ZERO {
            T::ONE
        } else {
            (value / running) * T::from_usize_lossy(tau)
        };
    }
    d.len()
}

impl<T: Float> std::fmt::Debug for YinDetector<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("YinDetector");
        s.field("frame_len", &self.frame_len)
            .field("window", &self.window)
            .field("tau_max", &self.tau_max)
            .field("absolute_threshold", &self.absolute_threshold)
            .field("power_threshold", &self.power_threshold)
            .field("fft_len", &self.correlator.len());
        #[cfg(test)]
        s.field("valid", &self.valid);
        s.finish()
    }
}

impl<T: Float> PitchDetector<T> for YinDetector<T> {
    fn detect(&mut self, frame: &[T], sample_rate: u32) -> Option<Pitch<T>> {
        #[cfg(test)]
        {
            self.valid = 0;
        }
        if frame.len() < self.frame_len {
            return None;
        }
        let x = &frame[..self.frame_len];
        screen(x, self.power_threshold)?;
        let len = self.compute_curves(x);
        #[cfg(test)]
        {
            self.valid = len;
        }

        let curve = &self.cmndf[..len];
        let threshold = self.absolute_threshold;
        let mut tau = (2..len).find(|&tau| curve[tau] < threshold)?;
        while tau + 1 < len && curve[tau + 1] < curve[tau] {
            tau += 1;
        }
        let (lag, d_min) = refine(curve, len, tau);
        let clarity = (T::ONE - d_min).clamp_to(T::ZERO, T::ONE);
        let frequency = T::from_u32_lossy(sample_rate) / lag;
        if !frequency.is_finite() || !clarity.is_finite() {
            return None;
        }
        Some(Pitch { frequency, clarity })
    }
}
