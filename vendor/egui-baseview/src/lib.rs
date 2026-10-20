mod renderer;
mod translate;
mod window;

#[cfg(all(feature = "dpi-trace", target_os = "windows"))]
pub mod diagnostics;

pub use baseview;
use baseview::WindowSize;
pub use keyboard_types::Key;
pub use renderer::GraphicsConfig;
pub use window::{EguiWindow, EguiWindowSettings, Frame, KeyCapture, RepaintNotifier};

/// Implement this trait to run an app with egui-baseview.
pub trait App: Send + 'static {
    /// Called once before the first frame. Setup code such as `egui_ctx.set_fonts()`
    /// can be done here.
    ///
    /// If an error is returned, then the window will be closed.
    fn build(
        &mut self,
        egui_ctx: egui::Context,
        frame: &mut Frame,
    ) -> Result<(), baseview::HandlerError> {
        let _ = egui_ctx;
        let _ = frame;
        Ok(())
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut Frame);

    /// Called when the window has been resized.
    ///
    /// The given size takes the zoom factor into account.
    fn resized(&mut self, size: WindowSize) {
        let _ = size;
    }

    /// Called when the zoom factor has changed.
    fn zoom_factor_changed(&mut self, zoom_factor: f32) {
        let _ = zoom_factor;
    }
}
