#![cfg_attr(feature = "simd", feature(portable_simd))]
#![allow(clippy::type_complexity)]

pub mod audio_setup;
pub mod buffer;
pub mod context;
pub mod formatters;
pub mod midi;
pub mod params;
pub mod plugin;
pub mod util;

#[cfg(feature = "editor")]
pub mod editor;

// These macros are also in the crate root and in the prelude, but having the module itself be pub
// as well makes it easy to import _just_ the macros without using `#[macro_use] extern crate nice-plug-core;`
#[macro_use]
pub mod debug;

/// A re-export of the `tracing` crate for use in the debug macros. This should not be used directly.
#[doc(no_inline)]
pub use tracing;
