//! An [`Editor`] implementation for egui.

use crate::EguiEditorState;
use crate::NiceEguiApp;
use egui_baseview::EguiWindowSettings;
use egui_baseview::RepaintNotifier;
use egui_baseview::baseview;
use egui_baseview::baseview::HandlerError;
use egui_baseview::{EguiWindow, GraphicsConfig};
use nice_plug_core::context::gui::GuiContext;
use nice_plug_core::editor::SizeConstraints;
use nice_plug_core::editor::dpi::NativeSize;
use nice_plug_core::editor::dpi::Size;
use nice_plug_core::editor::{
    Editor, EditorHandle, HostMethods, Modifiers, ParentWindowHandle, ResizeHint, SpawnedEditor,
    VirtualKeyCode,
};
use parking_lot::Mutex;
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

pub struct EguiNiceSettings {
    /// The window title
    pub title: String,

    /// The graphics configuration
    pub graphics: GraphicsConfig,

    /// Describes whether and how a host may resize an [`Editor`], returned from
    /// [`nice_plug_core::editor::Editor::resize_hint()`].
    ///
    /// The default is non-resizable (`can_resize: false`), matching the previous
    /// fixed-size behavior. To make an editor resizable, return a hint with
    /// `can_resize: true`; the per-axis flags and aspect-ratio fields refine how.
    pub resize_hint: ResizeHint,

    /// The rate at which the [`repaint_notifier`](egui-baseview::repaint_notifier)
    /// will be polled.
    ///
    /// Defaults to `1.0 / 64.0` seconds.
    pub repaint_poll_interval: Duration,
}

impl Default for EguiNiceSettings {
    fn default() -> Self {
        Self {
            title: Default::default(),
            graphics: Default::default(),
            resize_hint: Default::default(),
            repaint_poll_interval: Duration::from_secs_f64(1.0 / 64.0),
        }
    }
}

impl EguiNiceSettings {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Use the given window title
    #[inline]
    pub fn with_tile(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Use the given graphics configuration
    #[inline]
    pub fn with_graphics_config(mut self, config: GraphicsConfig) -> Self {
        self.graphics = config;
        self
    }

    /// Describes whether and how a host may resize an [`Editor`], returned from
    /// [`Editor::resize_hint()`].
    ///
    /// The default is non-resizable (`can_resize: false`), matching the previous
    /// fixed-size behavior. To make an editor resizable, return a hint with
    /// `can_resize: true`; the per-axis flags and aspect-ratio fields refine how.
    #[inline]
    pub fn with_resize_hint(mut self, resize_hint: ResizeHint) -> Self {
        self.resize_hint = resize_hint;
        self
    }

    /// The rate at which the [`repaint_notifier`](egui-baseview::repaint_notifier)
    /// will be polled.
    ///
    /// Defaults to `1.0 / 64.0` seconds.
    #[inline]
    pub fn with_repaint_poll_interval(mut self, interval: Duration) -> Self {
        self.repaint_poll_interval = interval;
        self
    }
}

struct UserAppWrapper<A: NiceEguiApp> {
    user_app: Arc<Mutex<A>>,
    gui_context: GuiContext,
    egui_ctx: Arc<Mutex<Option<egui::Context>>>,
    egui_state: Arc<EguiEditorState>,
}

impl<A: NiceEguiApp> Drop for UserAppWrapper<A> {
    fn drop(&mut self) {
        #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
        egui_baseview::diagnostics::event(
            "nice_closed",
            format_args!("state={:?}", self.egui_state),
        );
        *self.egui_ctx.lock() = None;
        self.user_app.lock().editor_closed();
    }
}

impl<A: NiceEguiApp> egui_baseview::App for UserAppWrapper<A> {
    fn build(
        &mut self,
        egui_ctx: egui::Context,
        frame: &mut egui_baseview::Frame,
    ) -> Result<(), baseview::HandlerError> {
        *self.egui_ctx.lock() = Some(egui_ctx.clone());
        self.egui_state
            .system_scale_factor
            .store(Some(frame.baseview_window().size().scale_factor as f32));

        self.user_app
            .lock()
            .build(egui_ctx, self.gui_context.clone(), frame)
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut egui_baseview::Frame) {
        self.user_app.lock().ui(ui, frame);
    }

    fn resized(&mut self, size: baseview::WindowSize) {
        self.egui_state.system_scale_factor.store(Some(
            size.scale_factor as f32 / self.egui_state.zoom_factor.load(),
        ));

        let current_size = self.egui_state.size();
        let new_size = match current_size {
            Size::Logical(_) => Size::Logical(size.logical),
            Size::Physical(_) => Size::Physical(size.physical),
        };
        self.egui_state.size.store(new_size);

        self.user_app.lock().resized(size);
    }

    fn zoom_factor_changed(&mut self, zoom_factor: f32) {
        self.egui_state.zoom_factor.store(zoom_factor);

        self.user_app.lock().zoom_factor_changed(zoom_factor);
    }
}

/// An [`Editor`] implementation that calls an egui draw loop.
pub struct EguiEditor<A: NiceEguiApp> {
    pub(crate) egui_state: Arc<EguiEditorState>,
    pub(crate) user_app: Arc<Mutex<A>>,
    pub(crate) settings: Arc<EguiNiceSettings>,
    pub(crate) repaint_notifier: RepaintNotifier,
}

impl<A: NiceEguiApp> Editor for EguiEditor<A> {
    type Handle = EguiEditorHandle;

    fn spawn(
        &self,
        parent: Option<ParentWindowHandle>,
        wait_for_parent: bool,
        fallback_scale_factor: Option<f64>,
        gui_context: GuiContext,
        host: Option<HostMethods>,
    ) -> Result<SpawnedEditor<Self::Handle>, Box<dyn Error>> {
        let egui_state = self.egui_state.clone();
        let user_app = self.user_app.clone();
        let size = egui_state.size();
        let zoom_factor = egui_state.zoom_factor.load();

        #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
        egui_baseview::diagnostics::event(
            "nice_spawn",
            format_args!(
                "size={size:?} zoom={zoom_factor} fallback={fallback_scale_factor:?} \
                 parent={parent:?} state={egui_state:?}"
            ),
        );

        let (min_size, max_size): (Option<Size>, Option<Size>) =
            match self.settings.resize_hint.size_constraints {
                SizeConstraints::Logical { min_size, max_size } => {
                    (min_size.map(|s| s.into()), max_size.map(|s| s.into()))
                }
                SizeConstraints::Physical { min_size, max_size } => {
                    (min_size.map(|s| s.into()), max_size.map(|s| s.into()))
                }
            };

        let settings = EguiWindowSettings::new()
            .with_title(self.settings.title.clone())
            .with_size(size)
            .with_min_size(min_size)
            .with_max_size(max_size)
            .with_resizable(self.settings.resize_hint.can_resize)
            .with_zoom_factor(zoom_factor)
            .with_graphics_config(self.settings.graphics.clone())
            .with_parent(parent.as_ref())
            .with_fallback_scale_factor(fallback_scale_factor)
            .with_wait_for_parent(wait_for_parent)
            .with_repaint_notifier(self.repaint_notifier.clone())
            .with_repaint_poll_interval(self.settings.repaint_poll_interval);

        let egui_ctx = Arc::new(Mutex::new(None));

        // The host is a re-implementation of
        // [`baseview::host::HostCallbacks`](https://docs.rs/baseview/latest/baseview/host/trait.HostCallbacks.html)
        // and
        // [`baseview::host::HostMainThreadCaller`](https://docs.rs/baseview/latest/baseview/host/trait.HostMainThreadCaller.html)
        // to avoid `nice-plug-core` from depending on `baseview` until it is stabilized.
        //
        // Create a small wrapper to adapt it to baseview's HostCallbacks traits.
        let host = {
            struct HostCallbackAdapter {
                host: Box<dyn nice_plug_core::editor::HostCallbacks>,
            }
            impl baseview::host::HostCallbacks for HostCallbackAdapter {
                fn request_resize(
                    &mut self,
                    new_size: baseview::WindowSize,
                ) -> Result<(), HandlerError> {
                    self.host
                        .request_resize(new_size.physical.into(), new_size.scale_factor)
                        .map_err(HandlerError::from_boxed)
                }

                fn destroyed(&mut self) {
                    self.host.destroyed();
                }
            }
            struct HostMainThreadCallerAdapter {
                host: Box<dyn nice_plug_core::editor::HostMainThreadCaller>,
            }
            impl baseview::host::HostMainThreadCaller for HostMainThreadCallerAdapter {
                fn call_main_thread(&mut self) {
                    self.host.call_main_thread();
                }
            }
            host.map(|host| {
                baseview::host::Host::new()
                    .with_callbacks(HostCallbackAdapter {
                        host: host.callbacks,
                    })
                    .with_main_thread(HostMainThreadCallerAdapter {
                        host: host.main_thread_caller,
                    })
            })
        };
        let window = EguiWindow::create_with_host(
            settings,
            UserAppWrapper {
                user_app,
                gui_context,
                egui_ctx,
                egui_state: egui_state.clone(),
            },
            host,
        )?;

        self.egui_state.open.store(true, Ordering::Release);
        self.repaint_notifier.request_repaint();

        Ok(SpawnedEditor {
            handle: EguiEditorHandle {
                egui_state: self.egui_state.clone(),
                resize_hint: self.settings.resize_hint,
                repaint_notifier: self.repaint_notifier.clone(),
            },
            window,
        })
    }

    fn size(&self) -> NativeSize<u32> {
        let size = NativeSize::from_size(
            self.egui_state.size(),
            self.egui_state.system_scale_factor() as f64
                * self.egui_state.user_scale_factor() as f64,
        );
        #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
        egui_baseview::diagnostics::event(
            "nice_size_query",
            format_args!("reported={size:?} state={:?}", self.egui_state),
        );
        size
    }

    fn resize_hint(&self) -> nice_plug_core::editor::ResizeHint {
        self.settings.resize_hint
    }

    fn track_info_updated(&self, info: nice_plug_core::plugin::TrackInfo) {
        self.user_app.lock().track_info_changed(info);
        self.repaint_notifier.request_repaint();
    }

    unsafe fn assume_standalone_in_process(&self) {
        // Safety: Editor::assume_standalone_in_process() has the same invariants as
        // baseview::assume_standalone_in_process()
        unsafe {
            baseview::assume_standalone_in_process();
        }
    }
}

/// A handle to a spawned instance of an [`EguiEditor`].
pub struct EguiEditorHandle {
    egui_state: Arc<EguiEditorState>,
    repaint_notifier: RepaintNotifier,
    resize_hint: ResizeHint,
}

impl EditorHandle for EguiEditorHandle {
    type Window = baseview::Window;
    type Error = baseview::Error;

    fn run_until_closed(window: Self::Window) -> Result<(), Self::Error> {
        window.run_until_closed()
    }

    fn set_parent(
        &self,
        parent: ParentWindowHandle,
        window: &Self::Window,
    ) -> Result<(), Self::Error> {
        window.set_parent(&parent)
    }

    fn show(&self, window: &Self::Window) -> Result<(), Self::Error> {
        let res = window.show();

        // TODO: This might be a baseview bug. the `baseview::WindowEvent::Focused`
        // event does not always get called after `window.show()` is called. Either
        // that, or baseview needs to add a `baseview::WindowEvent::Shown` event.
        //
        // For now, we must manually trigger a redraw.
        self.repaint_notifier.request_repaint();

        res
    }

    fn hide(&self, window: &Self::Window) -> Result<(), Self::Error> {
        window.hide()
    }

    fn host_main_thread_callback(&self, window: &Self::Window) {
        window.host_main_thread_callback();
    }

    fn set_size(
        &self,
        new_size: NativeSize<u32>,
        window: &Self::Window,
    ) -> Result<(), Self::Error> {
        #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
        egui_baseview::diagnostics::event(
            "nice_host_size",
            format_args!("requested={new_size:?} current={:?}", window.size()),
        );
        window.resize(new_size)
    }

    fn set_fallback_scale_factor(
        &self,
        scale_factor: f64,
        window: &Self::Window,
    ) -> Result<(), Self::Error> {
        #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
        egui_baseview::diagnostics::event(
            "nice_host_scale",
            format_args!(
                "suggested={scale_factor} current={:?} state={:?}",
                window.size(),
                self.egui_state
            ),
        );
        window.suggest_fallback_scale_factor(scale_factor)
    }

    /// Return the closest supported size.
    fn adjust_size(
        &self,
        new_size: NativeSize<u32>,
        window: &Self::Window,
    ) -> Option<NativeSize<u32>> {
        let current_size = window.size();
        Some(self.resize_hint.adjust_size(
            new_size,
            NativeSize::from_logical_or_physical(current_size.logical, current_size.physical),
            current_size.scale_factor,
        ))
    }

    fn on_virtual_key_from_host(
        &self,
        _key_code: VirtualKeyCode,
        _is_down: bool,
        _modifiers: Modifiers,
    ) -> bool {
        // TODO
        false
    }

    fn state_changed(&self) {
        self.repaint_notifier.request_repaint();
    }

    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {
        self.repaint_notifier.request_repaint();
    }

    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {
        self.repaint_notifier.request_repaint();
    }
}

impl Drop for EguiEditorHandle {
    fn drop(&mut self) {
        self.egui_state.open.store(false, Ordering::Release);
    }
}
