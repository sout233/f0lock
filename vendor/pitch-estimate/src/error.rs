//! Constructor-time configuration errors.

use std::fmt;

/// Largest accepted frame length, in samples.
///
/// A frame of this length with `tau_max` equal to `frame_len` uses an FFT
/// of `2^25` points. Scratch for `f64` is about 1.3 GB plus the FFT
/// planner's workspace. Larger values are rejected as a configuration
/// mistake.
pub const MAX_FRAME_LEN: usize = 1 << 24;

/// Why a detector could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigError {
    /// A size that must be positive was zero.
    ZeroSize {
        /// The parameter name: `"frame_len"`, `"tau_max"`, or `"window"`.
        what: &'static str,
    },
    /// A size was outside the range the other sizes allow.
    OutOfRange {
        /// The parameter name: `"frame_len"`, `"tau_max"`, or `"window"`.
        what: &'static str,
        /// The rejected value.
        value: usize,
        /// The smallest accepted value.
        min: usize,
        /// The largest accepted value.
        max: usize,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::ZeroSize { what } => write!(f, "{what} must be at least 1, got 0"),
            ConfigError::OutOfRange {
                what,
                value,
                min,
                max,
            } => write!(f, "{what} must lie in {min}..={max}, got {value}"),
        }
    }
}

impl std::error::Error for ConfigError {}
