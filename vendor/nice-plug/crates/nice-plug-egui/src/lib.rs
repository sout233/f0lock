//! [egui](https://github.com/emilk/egui) editor support for nice-plug.
//!
//! TODO: Proper usage example, for now check out the gain_gui example

// See the comment in the main `nice-plug` crate
#![allow(clippy::type_complexity)]

use crossbeam::atomic::AtomicCell;
use egui_baseview::baseview::WindowSize;
use nice_plug_core::context::gui::GuiContext;
use nice_plug_core::editor::dpi::{LogicalSize, PhysicalSize, Size};
use nice_plug_core::plugin::TrackInfo;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(not(any(feature = "opengl", feature = "wgpu")))]
compile_error!("There's currently no software rendering support for egui");

/// Re-export for convenience.
pub use egui_baseview::*;

#[cfg(all(feature = "opengl", not(feature = "wgpu")))]
pub use baseview::gl::{GlConfig, Profile};

pub use crate::editor::{EguiEditor, EguiEditorHandle, EguiNiceSettings};

mod editor;
pub mod resizable_window;
pub mod widgets;

/// Create an [`Editor`](nice_plug_core::editor::Editor) instance using an [`egui`] GUI. Using the
/// user state parameter is optional, but it can be useful for keeping track of some temporary
/// GUI-only settings. See the `nice-plug_gain_egui` example for more information on how to use this.
/// The [`EguiEditorState`] passed to this function contains the GUI's intitial size, and this is kept in
/// sync whenever the GUI gets resized. You can also use this to know if the GUI is open, so you can
/// avoid performing potentially expensive calculations while the GUI is not open.
///
/// See [`EguiEditorState::from_size()`].
pub fn create_egui_editor<A: NiceEguiApp>(
    egui_state: Arc<EguiEditorState>,
    repaint_notifier: RepaintNotifier,
    settings: EguiNiceSettings,
    app: A,
) -> Option<EguiEditor<A>> {
    Some(EguiEditor {
        egui_state,
        user_app: Arc::new(Mutex::new(app)),
        settings: Arc::new(settings),
        repaint_notifier,
    })
}

/// Implement this trait to run an app with nice-plug-egui.
pub trait NiceEguiApp: Send + 'static {
    /// Called when a new editor is opened. Setup code such as `egui_ctx.set_fonts()`
    /// can be done here.
    ///
    /// This may be called again after a call to [`NiceEguiApp::editor_closed()`].
    ///
    /// If an error is returned, then the window will be closed.
    fn build(
        &mut self,
        egui_ctx: egui::Context,
        nice_gui_ctx: GuiContext,
        frame: &mut Frame,
    ) -> Result<(), baseview::HandlerError> {
        let _ = egui_ctx;
        let _ = nice_gui_ctx;
        let _ = frame;
        Ok(())
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    ///
    /// This will only ever be called while an editor window is open.
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut Frame);

    /// Called when the window has been resized.
    fn resized(&mut self, new_size: WindowSize) {
        let _ = new_size;
    }

    /// Called when the zoom factor has changed.
    fn zoom_factor_changed(&mut self, zoom_factor: f32) {
        let _ = zoom_factor;
    }

    /// Called when the editor is closed. This is needed because the plugin editor window
    /// can be opened and closed multiple times.
    ///
    /// If your app holds onto an `egui::Context` object, then it should be dropped here so that
    /// egui can probably be cleaned up.
    fn editor_closed(&mut self) {}

    /// Called when the track information has changed.
    fn track_info_changed(&mut self, info: TrackInfo) {
        let _ = info;
    }
}

/// State for an `nice-plug-egui` editor.
#[derive(Debug)]
pub struct EguiEditorState {
    size: AtomicCell<Size>,
    zoom_factor: AtomicCell<f32>,

    /// The scaling factor reported by the host, if any. On macOS this will never be set and we
    /// should use the system scaling factor instead.
    pub(crate) fallback_scale_factor: AtomicCell<Option<f32>>,

    pub(crate) system_scale_factor: AtomicCell<Option<f32>>,

    /// Whether the editor's window is currently open.
    open: AtomicBool,
}

impl EguiEditorState {
    /// Create a new state for egui's editor.
    pub fn from_size(size: impl Into<Size>, zoom_factor: f32) -> Arc<Self> {
        assert!(zoom_factor > 0.0);

        Arc::new(Self {
            size: AtomicCell::new(size.into()),
            zoom_factor: AtomicCell::new(zoom_factor),
            open: AtomicBool::new(false),
            fallback_scale_factor: AtomicCell::new(None),
            system_scale_factor: AtomicCell::new(None),
        })
    }

    pub fn size(&self) -> Size {
        self.size.load()
    }

    pub fn set_size(&self, size: impl Into<Size>) {
        self.size.store(size.into());
    }

    pub fn set_zoom_factor(&self, zoom_factor: f32) {
        assert!(zoom_factor > 0.0);
        self.zoom_factor.store(zoom_factor);
    }

    fn system_scale_factor(&self) -> f32 {
        let scale_factor = self.system_scale_factor.load();
        let fallback_scale_factor = self.fallback_scale_factor.load();

        scale_factor.unwrap_or_else(|| fallback_scale_factor.unwrap_or(1.0))
    }

    pub fn logical_size(&self) -> LogicalSize<f32> {
        let size = self.size.load();
        let scale_factor = self.system_scale_factor();

        size.to_logical(scale_factor as f64)
    }

    pub fn physical_size(&self) -> PhysicalSize<u32> {
        let size = self.size.load();
        let scale_factor = self.system_scale_factor();

        size.to_physical(scale_factor as f64)
    }

    /// The current user zoom (scale) factor. This is applied on top of the
    /// system's scale factor.
    pub fn user_scale_factor(&self) -> f32 {
        self.zoom_factor.load()
    }

    /// Whether the GUI is currently visible.
    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::Acquire)
    }
}
