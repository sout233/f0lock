//! Macros for logging and debug assertions. [`nice_dbg!()`], [`nice_trace!()`], and the
//! `nice_debug_assert_*!()` macros are compiled out during release builds, so they can be used for
//! asserting adiditonal invariants in debug builds. Check the [`nice_log!()`] macro for more
//! information on nice-plug's logger. None of the logging functions are realtime-safe, and you
//! should avoid using them during release builds in any of the functions that may be called from an
//! audio thread.

// NOTE: Exporting macros in Rust is a bit weird. `#[macro_export]` causes them to be exported to
//       the crate root, but that makes it difficult to include just the macros without using
//       `#[macro_use] extern crate nice-plug;`. That's why the macros are also re-exported from this
//       module.

/// Write something to the logger. This defaults to STDERR unless the user is running Windows and a
/// debugger has been attached, in which case `OutputDebugString()` will be used instead.
///
/// The logger's behavior can be controlled by setting the `NICE_LOG` environment variable to:
///
/// - `stderr`, in which case the log output always gets written to STDERR.
/// - `windbg` (only on Windows), in which case the output always gets logged using
///   `OutputDebugString()`.
/// - A file path, in which case the output gets appended to the end of that file which will be
///   created if necessary.
#[macro_export]
macro_rules! nice_log {
    ($($args:tt)*) => (
        $crate::tracing::info!($($args)*)
    );
}
#[doc(inline)]
pub use nice_log;

/// Similar to `nice_log!()`, but less subtle. Used for printing warnings.
#[macro_export]
macro_rules! nice_warn {
    ($($args:tt)*) => (
        $crate::tracing::warn!($($args)*)
    );
}
#[doc(inline)]
pub use nice_warn;

/// Similar to `nice_log!()`, but more scream-y. Used for printing fatal errors.
#[macro_export]
macro_rules! nice_error {
    ($($args:tt)*) => (
        $crate::tracing::error!($($args)*)
    );
}
#[doc(inline)]
pub use nice_error;

/// The same as `nice_log!()`, but with source and thread information. Like the
/// `nice_debug_assert*!()` macros, this is only shown when compiling in debug mode.
#[macro_export]
macro_rules! nice_trace {
    ($($args:tt)*) => (
        $crate::util::permit_alloc(|| $crate::tracing::trace!($($args)*))
    );
}
#[doc(inline)]
pub use nice_trace;

/// Analogues to the `dbg!()` macro, but respecting the `NICE_LOG` environment variable and with all
/// of the same logging features as the other `nice_*!()` macros. Like the `nice_debug_assert*!()`
/// macros, this is only shown when compiling in debug mode, but the macro will still return the
/// value in non-debug modes.
#[macro_export]
macro_rules! nice_dbg {
    () => {
        $crate::util::permit_alloc(|| $crate::tracing::debug!(""));
    };
    ($val:expr $(,)?) => {
        // Match here acts as a let-binding: https://stackoverflow.com/questions/48732263/why-is-rusts-assert-eq-implemented-using-a-match/48732525#48732525
        match $val {
            tmp => {
                $crate::util::permit_alloc(|| $crate::tracing::debug!("{} = {:#?}", stringify!($val), &tmp));
                tmp
            }
        }
    };
    ($($val:expr),+ $(,)?) => { ($($crate::nice_dbg!($val)),+,) };
}
#[doc(inline)]
pub use nice_dbg;

/// A `debug_assert!()` analogue that prints the error with line number information instead of
/// panicking. During tests this is upgraded to a regular panicking `debug_assert!()`.
///
/// TODO: Detect if we're running under a debugger, and trigger a break if we are
#[macro_export]
macro_rules! nice_debug_assert {
    ($cond:expr $(,)?) => (
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if cfg!(test) {
           debug_assert!($cond);
        } else if cfg!(debug_assertions) && !$cond {
            $crate::util::permit_alloc(|| $crate::tracing::warn!(concat!("Debug assertion failed: ", stringify!($cond))));
        }
    );
    ($cond:expr, $format:expr $(, $($args:tt)*)?) => (
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if cfg!(test) {
           debug_assert!($cond, $format, $($($args)*)?);
        } else if cfg!(debug_assertions) && !$cond {
            $crate::util::permit_alloc(|| $crate::tracing::warn!(concat!("Debug assertion failed: ", stringify!($cond), ", ", $format), $($($args)*)?));
        }
    );
}
#[doc(inline)]
pub use nice_debug_assert;

/// An unconditional debug assertion failure, for if the condition has already been checked
/// elsewhere. See [`nice_debug_assert!()`] for more information.
#[macro_export]
macro_rules! nice_debug_assert_failure {
    () => (
        if cfg!(test) {
           debug_assert!(false, "Debug assertion failed");
        } else if cfg!(debug_assertions) {
            $crate::util::permit_alloc(|| $crate::tracing::warn!("Debug assertion failed"));
        }
    );
    ($format:expr $(, $($args:tt)*)?) => (
        if cfg!(test) {
           debug_assert!(false, concat!("Debug assertion failed: ", $format), $($($args)*)?);
        } else if cfg!(debug_assertions) {
            $crate::util::permit_alloc(|| $crate::tracing::warn!(concat!("Debug assertion failed: ", $format), $($($args)*)?));
        }
    );
}
#[doc(inline)]
pub use nice_debug_assert_failure;

/// A `debug_assert_eq!()` analogue that prints the error with line number information instead of
/// panicking. See [`nice_debug_assert!()`] for more information.
#[macro_export]
macro_rules! nice_debug_assert_eq {
    ($left:expr, $right:expr $(,)?) => (
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if cfg!(test) {
           debug_assert_eq!($left, $right);
        } else if cfg!(debug_assertions) && $left != $right {
            $crate::util::permit_alloc(|| $crate::tracing::warn!(concat!("Debug assertion failed: ", stringify!($left), " != ", stringify!($right))));
        }
    );
    ($left:expr, $right:expr, $format:expr $(, $($args:tt)*)?) => (
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if cfg!(test) {
           debug_assert_eq!($left, $right, $format, $($($args)*)?);
        } else if cfg!(debug_assertions) && $left != $right {
            $crate::util::permit_alloc(|| $crate::tracing::warn!(concat!("Debug assertion failed: ", stringify!($left), " != ", stringify!($right), ", ", $format), $($($args)*)?));
        }
    );
}
#[doc(inline)]
pub use nice_debug_assert_eq;

/// A `debug_assert_ne!()` analogue that prints the error with line number information instead of
/// panicking. See [`nice_debug_assert!()`] for more information.
#[macro_export]
macro_rules! nice_debug_assert_ne {
    ($left:expr, $right:expr $(,)?) => (
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if cfg!(test) {
           debug_assert_ne!($left, $right);
        } else if cfg!(debug_assertions) && $left == $right {
            $crate::util::permit_alloc(|| $crate::tracing::warn!(concat!("Debug assertion failed: ", stringify!($left), " == ", stringify!($right))));
        }
    );
    ($left:expr, $right:expr, $format:expr $(, $($args:tt)*)?) => (
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if cfg!(test) {
           debug_assert_ne!($left, $right, $format, $($($args)*)?);
        } else if cfg!(debug_assertions) && $left == $right  {
            $crate::util::permit_alloc(|| $crate::tracing::warn!(concat!("Debug assertion failed: ", stringify!($left), " == ", stringify!($right), ", ", $format), $($($args)*)?));
        }
    );
}
#[doc(inline)]
pub use nice_debug_assert_ne;
