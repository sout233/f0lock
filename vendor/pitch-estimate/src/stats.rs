//! Frame screening shared by both detectors.

use crate::float::Float;

/// Time-domain energy summary of the analysed samples.
pub(crate) struct FrameStats<T> {
    /// Sum of squares.
    pub sum_sq: T,
}

/// Screen `x` before any transform runs.
///
/// Returns `None` when any sample is non-finite, the sum of squares is
/// non-finite, or the mean square is below `power_threshold`. That
/// mean-square floor is the only amplitude gate. A frame whose samples
/// are all equal (silence, constant, DC) also returns `None`. A
/// low-amplitude pitched frame whose samples are not all equal is kept.
pub(crate) fn screen<T: Float>(x: &[T], power_threshold: T) -> Option<FrameStats<T>> {
    let mut sum_sq = T::ZERO;
    for &v in x {
        sum_sq = sum_sq + v * v;
    }
    if !sum_sq.is_finite() {
        return None;
    }
    let n = T::from_usize_lossy(x.len());
    let mean_sq = sum_sq / n;
    if mean_sq < power_threshold {
        return None;
    }
    let first = x[0];
    if x.iter().all(|&v| v == first) {
        return None;
    }
    Some(FrameStats { sum_sq })
}

/// Validate `frame_len` against the accepted range.
pub(crate) fn check_frame_len(frame_len: usize) -> Result<(), crate::ConfigError> {
    use crate::error::{ConfigError, MAX_FRAME_LEN};
    if frame_len == 0 {
        return Err(ConfigError::ZeroSize { what: "frame_len" });
    }
    if !(2..=MAX_FRAME_LEN).contains(&frame_len) {
        return Err(ConfigError::OutOfRange {
            what: "frame_len",
            value: frame_len,
            min: 2,
            max: MAX_FRAME_LEN,
        });
    }
    Ok(())
}

/// Replace a NaN tuning value with its default.
#[inline]
pub(crate) fn or_default<T: Float>(value: T, default: T) -> T {
    if value.is_nan() {
        default
    } else {
        value
    }
}
