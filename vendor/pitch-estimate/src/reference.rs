//! Pure time-domain NSDF, difference, and CMNDF, used only as the in-crate
//! check against the FFT path.

/// Linear autocorrelation, m'(tau), and the NSDF.
pub(crate) fn nsdf(x: &[f64], tau_max: usize) -> Vec<f64> {
    let w = x.len();
    (0..=tau_max)
        .map(|tau| {
            if tau == 0 {
                return 1.0;
            }
            let mut r = 0.0;
            let mut m = 0.0;
            for j in 0..w.saturating_sub(tau) {
                r += x[j] * x[j + tau];
                m += x[j] * x[j] + x[j + tau] * x[j + tau];
            }
            if m > 0.0 {
                2.0 * r / m
            } else {
                0.0
            }
        })
        .collect()
}

/// Difference function over window `w`.
pub(crate) fn difference(x: &[f64], w: usize, tau_max: usize) -> Vec<f64> {
    (0..=tau_max)
        .map(|tau| {
            (0..w)
                .map(|j| {
                    let e = x[j] - x[j + tau];
                    e * e
                })
                .sum()
        })
        .collect()
}

/// CMNDF with multiplier `tau` and `d'(0) = 1`.
pub(crate) fn cmndf(d: &[f64]) -> Vec<f64> {
    let mut out = Vec::with_capacity(d.len());
    let mut running = 0.0;
    for (tau, &value) in d.iter().enumerate() {
        if tau == 0 {
            out.push(1.0);
            continue;
        }
        running += value;
        out.push(if running > 0.0 {
            value * tau as f64 / running
        } else {
            1.0
        });
    }
    out
}

/// MMIX linear congruential generator matching the frozen generator spec.
pub(crate) struct Lcg(pub u64);

impl Lcg {
    pub(crate) fn next_unit(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
    }

    pub(crate) fn frame(&mut self, len: usize) -> Vec<f64> {
        (0..len).map(|_| self.next_unit()).collect()
    }
}
