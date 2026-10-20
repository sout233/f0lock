pub use dpi::*;

/// A size represented in the platform's native pixels.
///
/// This size is represented in physical pixels on Windows and Linux, and in logical pixels on macOS.
///
/// (This is a re-implementation of
/// [`baseview::dpi::NativeSize`](https://github.com/RustAudio/baseview/blob/e95471e23d111d64db2e86e3b33ba619984bb96e/src/dpi.rs#L8)
/// to avoid directly depending on `baseview` until it is stabilized.)
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NativeSize<P> {
    pub width: P,
    pub height: P,
}

impl<P> NativeSize<P> {
    #[inline]
    pub const fn new(width: P, height: P) -> Self {
        NativeSize { width, height }
    }
}

impl<P: Pixel> NativeSize<P> {
    #[inline]
    pub fn from_size(size: Size, scale_factor: f64) -> Self {
        #[cfg(target_os = "macos")]
        {
            let size = size.to_logical(scale_factor);
            Self {
                width: size.width,
                height: size.height,
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            let size = size.to_physical(scale_factor);
            Self {
                width: size.width,
                height: size.height,
            }
        }
    }

    #[inline]
    pub fn from_logical_or_physical(
        logical_size: LogicalSize<f64>,
        physical_size: PhysicalSize<u32>,
    ) -> Self {
        #[cfg(target_os = "macos")]
        {
            let _ = physical_size;

            Self {
                width: logical_size.width.cast(),
                height: logical_size.height.cast(),
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = logical_size;

            Self {
                width: physical_size.width.cast(),
                height: physical_size.height.cast(),
            }
        }
    }

    #[inline]
    pub fn cast<X: Pixel>(&self) -> NativeSize<X> {
        NativeSize {
            width: self.width.cast(),
            height: self.height.cast(),
        }
    }

    #[inline]
    pub fn to_physical(self, scale_factor: f64) -> PhysicalSize<P> {
        #[cfg(target_os = "macos")]
        {
            let size = LogicalSize {
                width: self.width,
                height: self.height,
            };
            size.to_physical(scale_factor)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = scale_factor;
            PhysicalSize {
                width: self.width,
                height: self.height,
            }
        }
    }

    #[inline]
    pub fn to_logical(self, scale_factor: f64) -> LogicalSize<P> {
        #[cfg(target_os = "macos")]
        {
            let _ = scale_factor;
            LogicalSize {
                width: self.width,
                height: self.height,
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let size = PhysicalSize {
                width: self.width,
                height: self.height,
            };
            size.to_logical(scale_factor)
        }
    }
}

impl<P: Pixel> From<NativeSize<P>> for Size {
    #[inline]
    fn from(size: NativeSize<P>) -> Self {
        #[cfg(target_os = "macos")]
        {
            Size::Logical(LogicalSize::new(size.width, size.height).cast())
        }
        #[cfg(not(target_os = "macos"))]
        {
            Size::Physical(PhysicalSize::new(size.width, size.height).cast())
        }
    }
}
