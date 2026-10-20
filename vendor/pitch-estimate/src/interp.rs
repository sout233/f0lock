//! Parabolic refinement of a discrete extremum.

use crate::float::Float;

/// Fit a parabola through `(-1, y_prev)`, `(0, y0)`, `(+1, y_next)` and return
/// the vertex offset `delta` together with the vertex value.
///
/// For a true discrete extremum `delta` lies in `[-1/2, 1/2]`. When the three
/// points are collinear or equal the denominator vanishes and the discrete
/// point is returned unchanged.
#[inline]
pub(crate) fn parabolic<T: Float>(y_prev: T, y0: T, y_next: T) -> (T, T) {
    let two = T::ONE + T::ONE;
    let four = two + two;
    let denom = two * (y_prev - two * y0 + y_next);
    if denom == T::ZERO {
        return (T::ZERO, y0);
    }
    let diff = y_prev - y_next;
    let delta = (diff / denom).clamp_to(-T::HALF, T::HALF);
    let value = y0 - diff * delta / four;
    (delta, value)
}

/// Refine the extremum at index `i` of `curve`, using the neighbours when both
/// exist inside `curve[..len]`. At a boundary the discrete point is returned.
///
/// Returns `(lag, value)` with `lag = i + delta`.
#[inline]
pub(crate) fn refine<T: Float>(curve: &[T], len: usize, i: usize) -> (T, T) {
    let lag = T::from_usize_lossy(i);
    if i == 0 || i + 1 >= len {
        return (lag, curve[i]);
    }
    let (delta, value) = parabolic(curve[i - 1], curve[i], curve[i + 1]);
    (lag + delta, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symmetric_peak_has_zero_offset() {
        let (delta, value) = parabolic(0.5f64, 1.0, 0.5);
        assert_eq!(delta, 0.0);
        assert_eq!(value, 1.0);
    }

    #[test]
    fn recovers_a_known_parabola() {
        // y = 1 - (x - 0.3)^2 sampled at -1, 0, 1.
        let f = |x: f64| 1.0 - (x - 0.3) * (x - 0.3);
        let (delta, value) = parabolic(f(-1.0), f(0.0), f(1.0));
        assert!((delta - 0.3).abs() < 1e-12);
        assert!((value - 1.0).abs() < 1e-12);
    }

    #[test]
    fn flat_triplet_falls_back_to_discrete() {
        let (delta, value) = parabolic(2.0f32, 2.0, 2.0);
        assert_eq!(delta, 0.0);
        assert_eq!(value, 2.0);
    }

    #[test]
    fn boundary_is_not_interpolated() {
        let c = [0.1f64, 0.9, 0.2, 0.8];
        assert_eq!(refine(&c, 4, 0), (0.0, 0.1));
        assert_eq!(refine(&c, 4, 3), (3.0, 0.8));
        assert_eq!(refine(&c, 3, 2), (2.0, 0.2));
        let (lag, _) = refine(&c, 4, 1);
        assert!(lag > 0.5 && lag < 1.5);
    }
}
