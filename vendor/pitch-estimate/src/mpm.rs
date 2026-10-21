//! McLeod pitch method: linear autocorrelation, NSDF, key-maximum picking.

use crate::error::ConfigError;
use crate::fft::Correlator;
use crate::float::Float;
use crate::interp::refine;
use crate::stats::{check_frame_len, or_default, screen};
use crate::{Pitch, PitchDetector};

/// Default relative-peak constant `k`.
pub(crate) const DEFAULT_K: f64 = 0.9;
/// Default clarity threshold for [`McLeodDetector`].
pub(crate) const DEFAULT_MPM_CLARITY_THRESHOLD: f64 = 0.6;

/// Pitch detector using the McLeod pitch method: normalized square
/// difference function (NSDF), key-maximum picking, parabolic refinement.
///
/// Construct once with [`McLeodDetector::new`], tune with the `with_*`
/// setters, then call [`PitchDetector::detect`] per frame. Every buffer is
/// allocated by the constructor, and `detect` does not allocate.
pub struct McLeodDetector<T> {
    frame_len: usize,
    tau_max: usize,
    k: T,
    clarity_threshold: T,
    power_threshold: T,
    correlator: Correlator<T>,
    acf: Vec<T>,
    nsdf: Vec<T>,
    #[cfg(test)]
    valid: usize,
}

impl<T: Float> McLeodDetector<T> {
    /// Visit all refined positive NSDF maxima, without the relative-peak cutoff.
    /// This allows a caller to track competing periods across audio frames.
    /// Uses the detector's preallocated transform buffers.
    pub fn candidates(&mut self, frame: &[T], sample_rate: u32, mut visit: impl FnMut(Pitch<T>)) {
        if frame.len() < self.frame_len {
            return;
        }

        let x = &frame[..self.frame_len];
        let Some(stats) = screen(x, self.power_threshold) else {
            return;
        };
        let len = self.compute_nsdf(x, stats.sum_sq);

        Self::for_each_key_maximum(&self.nsdf, len, |lag, value| {
            let frequency = T::from_u32_lossy(sample_rate) / lag;
            let clarity = value.clamp_to(T::ZERO, T::ONE);

            if frequency.is_finite() && clarity.is_finite() {
                visit(Pitch { frequency, clarity });
            }

            false
        });
    }

    /// Create a detector for frames of `frame_len` samples that examines
    /// lags `1..=tau_max`.
    ///
    /// `tau_max` must satisfy `1 <= tau_max <= frame_len`, and `frame_len`
    /// must lie in `2..=MAX_FRAME_LEN`. Lags `1..=tau_max` are examined. A
    /// key maximum needs a negative-going zero crossing after its peak, so a
    /// final positive NSDF run cut off by `tau_max` is not a candidate. A
    /// period at lag `tau_max` is never reported. Size `tau_max` larger than
    /// the longest period of interest, with room to close that peak's lobe.
    /// Any length is accepted. The FFT is padded to the next power of two, so
    /// nearby lengths often share a transform size. Returns `Err` for any size
    /// outside those bounds.
    pub fn new(frame_len: usize, tau_max: usize) -> Result<Self, ConfigError> {
        check_frame_len(frame_len)?;
        if tau_max == 0 {
            return Err(ConfigError::ZeroSize { what: "tau_max" });
        }
        if tau_max > frame_len {
            return Err(ConfigError::OutOfRange {
                what: "tau_max",
                value: tau_max,
                min: 1,
                max: frame_len,
            });
        }
        Ok(Self {
            frame_len,
            tau_max,
            k: T::from_f64_lossy(DEFAULT_K),
            clarity_threshold: T::from_f64_lossy(DEFAULT_MPM_CLARITY_THRESHOLD),
            power_threshold: T::ZERO,
            correlator: Correlator::new(frame_len + tau_max),
            acf: vec![T::ZERO; tau_max + 1],
            nsdf: vec![T::ZERO; tau_max + 1],
            #[cfg(test)]
            valid: 0,
        })
    }

    /// Set the relative-peak constant `k`, clamped to `[0.8, 1.0]`. NaN
    /// selects the default 0.9.
    #[must_use]
    pub fn with_k(mut self, k: T) -> Self {
        self.k =
            or_default(k, T::from_f64_lossy(DEFAULT_K)).clamp_to(T::from_f64_lossy(0.8), T::ONE);
        self
    }

    /// Set the clarity threshold: a frame whose chosen peak has a clarity
    /// below it yields no pitch. Default 0.6. NaN selects the default.
    #[must_use]
    pub fn with_clarity_threshold(mut self, threshold: T) -> Self {
        self.clarity_threshold =
            or_default(threshold, T::from_f64_lossy(DEFAULT_MPM_CLARITY_THRESHOLD));
        self
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

    /// Largest lag examined.
    pub fn tau_max(&self) -> usize {
        self.tau_max
    }

    /// Relative-peak constant `k`.
    pub fn k(&self) -> T {
        self.k
    }

    /// Clarity threshold.
    pub fn clarity_threshold(&self) -> T {
        self.clarity_threshold
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

    /// The NSDF curve `n(0..=tau_max)` from the most recent `detect` call
    /// that reached the transform stage.
    #[cfg(test)]
    pub(crate) fn nsdf(&self) -> &[T] {
        &self.nsdf
    }

    /// Number of leading NSDF entries that took part in the most recent peak
    /// search. 0 before the first call and after a frame that was screened out.
    #[cfg(test)]
    pub(crate) fn valid_len(&self) -> usize {
        self.valid
    }

    /// Fill `self.nsdf` from the frame `x` (length `frame_len`) and return
    /// the number of leading finite entries.
    fn compute_nsdf(&mut self, x: &[T], sum_sq: T) -> usize {
        let tau_max = self.tau_max;
        self.correlator.autocorrelate(x, &mut self.acf[..=tau_max]);
        let two = T::ONE + T::ONE;
        // m'(0) is seeded from the time-domain sum of squares, so the taper
        // term and the autocorrelation share one scale.
        let m0 = two * sum_sq;
        let mut m = m0;
        let w = x.len();
        for tau in 0..=tau_max {
            // n(tau) = 2 r(tau) / m'(tau) whenever m'(tau) > 0, and 0 when
            // m'(tau) = 0. A floor relative to m'(0) would zero a high-lag
            // peak whose overlapping energy is small next to energy elsewhere
            // in the frame.
            let n = if tau == 0 {
                T::ONE
            } else if m <= T::ZERO {
                T::ZERO
            } else {
                two * self.acf[tau] / m
            };
            if !n.is_finite() {
                for v in &mut self.nsdf[tau..] {
                    *v = T::ZERO;
                }
                return tau;
            }
            self.nsdf[tau] = n;
            if tau < w {
                let head = x[tau];
                let tail = x[w - 1 - tau];
                m = m - head * head - tail * tail;
            }
        }
        tau_max + 1
    }

    /// Visit every key maximum of `n[..len]` in lag order, passing the
    /// refined `(lag, value)` pair. Returns early when `f` returns `true`.
    fn for_each_key_maximum(n: &[T], len: usize, mut f: impl FnMut(T, T) -> bool) {
        let mut tau = 1;
        // Skip the initial lobe: everything until n first drops below zero.
        while tau < len && n[tau] >= T::ZERO {
            tau += 1;
        }
        loop {
            // Positive-going zero crossing.
            while tau < len && n[tau] <= T::ZERO {
                tau += 1;
            }
            if tau >= len {
                return;
            }
            let mut best = tau;
            while tau < len && n[tau] > T::ZERO {
                if n[tau] > n[best] {
                    best = tau;
                }
                tau += 1;
            }
            if tau >= len {
                // A final positive run cut off by tau_max yields no maximum.
                return;
            }
            let (lag, value) = refine(n, len, best);
            if f(lag, value) {
                return;
            }
        }
    }
}

impl<T: Float> std::fmt::Debug for McLeodDetector<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("McLeodDetector");
        s.field("frame_len", &self.frame_len)
            .field("tau_max", &self.tau_max)
            .field("k", &self.k)
            .field("clarity_threshold", &self.clarity_threshold)
            .field("power_threshold", &self.power_threshold)
            .field("fft_len", &self.correlator.len());
        #[cfg(test)]
        s.field("valid", &self.valid);
        s.finish()
    }
}

impl<T: Float> PitchDetector<T> for McLeodDetector<T> {
    fn detect(&mut self, frame: &[T], sample_rate: u32) -> Option<Pitch<T>> {
        #[cfg(test)]
        {
            self.valid = 0;
        }
        if frame.len() < self.frame_len {
            return None;
        }
        let x = &frame[..self.frame_len];
        let stats = screen(x, self.power_threshold)?;
        let len = self.compute_nsdf(x, stats.sum_sq);
        #[cfg(test)]
        {
            self.valid = len;
        }

        let mut n_max = T::ZERO;
        Self::for_each_key_maximum(&self.nsdf, len, |_, value| {
            if value > n_max {
                n_max = value;
            }
            false
        });
        if n_max <= T::ZERO {
            return None;
        }
        let cutoff = self.k * n_max;
        let mut chosen = None;
        Self::for_each_key_maximum(&self.nsdf, len, |lag, value| {
            if value >= cutoff {
                chosen = Some((lag, value));
                true
            } else {
                false
            }
        });
        let (lag, value) = chosen?;
        let clarity = value.clamp_to(T::ZERO, T::ONE);
        if clarity < self.clarity_threshold {
            return None;
        }
        let frequency = T::from_u32_lossy(sample_rate) / lag;
        if !frequency.is_finite() || !clarity.is_finite() {
            return None;
        }
        Some(Pitch { frequency, clarity })
    }
}
