#[cfg(all(feature = "opengl", not(feature = "wgpu")))]
mod opengl;
#[cfg(all(feature = "opengl", not(feature = "wgpu")))]
pub use opengl::renderer::{GraphicsConfig, Renderer};

#[cfg(feature = "wgpu")]
mod wgpu;
#[cfg(feature = "wgpu")]
pub use wgpu::renderer::{GraphicsConfig, Renderer};
