use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use baseview::dpi::{LogicalPosition, LogicalSize, Size};
use baseview::{
    Event, EventStatus, HandlerError, ParentWindowHandle, Window, WindowContext, WindowHandler,
    WindowSettings, WindowSize,
};
use copypasta::ClipboardProvider;
use egui::{FullOutput, Pos2, Rect, Rgba, ViewportCommand, pos2, vec2};
use keyboard_types::Modifiers;
use raw_window_handle::HasWindowHandle;

use crate::App;
use crate::{GraphicsConfig, renderer::Renderer};

#[cfg(all(feature = "log", not(feature = "tracing")))]
use log::{error, warn};
#[cfg(feature = "tracing")]
use tracing::{error, warn};

/// A realtime-safe handle to request a update & repaint for an egui app.
///
/// This can be used, for example, to notify the GUI that the value of a decibel
/// meter has changed.
#[derive(Debug, Clone)]
pub struct RepaintNotifier {
    repaint: Arc<AtomicBool>,
}

impl RepaintNotifier {
    pub fn new() -> Self {
        Self {
            repaint: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn request_repaint(&self) {
        self.repaint.store(true, Ordering::Relaxed);
    }

    pub fn request_repaint_with(&self, repaint: bool) {
        if repaint {
            self.repaint.store(true, Ordering::Relaxed);
        }
    }

    fn repaint_requested(&self) -> bool {
        self.repaint.swap(false, Ordering::Relaxed)
    }
}

impl Default for RepaintNotifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Settings used when creating a new window
#[derive(Debug, Clone)]
pub struct EguiWindowSettings {
    /// The window title.
    pub title: String,

    /// The size of the window, either in physical or logical coordinates.
    pub size: Size,

    /// The minimum window size. Set to `None` for no minimum size.
    pub min_size: Option<Size>,
    /// The maximum window size. Set to `None` for no maximum size.
    pub max_size: Option<Size>,

    /// Whether the window can be resized.
    pub resizable: bool,

    /// The amount of zoom (scaling) to apply. This is applied on top of the
    /// system's native scaling factor.
    ///
    /// The zoom factor can also be changed later with
    /// [`Context::set_zoom_factor()`](egui::Context::set_zoom_factor).
    pub zoom_factor: f32,

    /// The graphics configuration
    pub graphics: GraphicsConfig,

    /// If the window is to be embedded in a parent window, the handle to that window.
    ///
    /// If `None`, the window will be standalone.
    pub parent: Option<ParentWindowHandle>,

    /// If the window expects to have a parent when first displayed.
    ///
    /// Setting this will delay the actual creation of the window until the parent is set (unless
    /// the window is shown first).
    ///
    /// If the `parent` field is already set, this does nothing and is ignored.
    pub wait_for_parent: bool,

    /// A fallback scale factor, if Baseview couldn't get one from the platform.
    ///
    /// If the platform does already provide an accurate scaling factor, this doesn't do anything.
    ///
    /// If the given fallback scale factor is actually useful and different from the current one
    /// (1.0 by default), this will resize and redraw the window accordingly.
    ///
    /// # Platform compatibility notes.
    ///
    /// On Win32, this value is used if running on early versions of Windows 10 (or earlier).
    ///
    /// On X11, this value is used if no `Xft.dpi`setting is set.
    ///
    /// On macOS, this function is always a no-op.
    pub fallback_scale_factor: Option<f64>,

    /// A realtime-safe handle to request an update & repaint for an egui app.
    ///
    /// This can be used, for example, to notify the GUI that the value of a decibel
    /// meter has changed.
    pub repaint_notifier: Option<RepaintNotifier>,

    /// The rate at which the [`repaint_notifier`](EguiWindowSettings::repaint_notifier) will be
    /// polled.
    ///
    /// Defaults to `1.0 / 64.0` seconds.
    pub repaint_poll_interval: Duration,
}

impl EguiWindowSettings {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// The window title.
    #[inline]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// The size of the window, either in physical or logical coordinates.
    #[inline]
    pub fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }

    /// Sets whether the window can be resized.
    ///
    /// Defaults to `true`.
    #[inline]
    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// The minimum window size. Set to `None` for no minimum size.
    ///
    /// Defaults to `None`.
    #[inline]
    pub fn with_min_size<S: Into<Size>>(mut self, min_size: Option<S>) -> Self {
        self.min_size = min_size.map(|s| s.into());
        self
    }

    /// The maximum window size. Set to `None` for no maximum size.
    ///
    /// Defaults to `None`.
    #[inline]
    pub fn with_max_size<S: Into<Size>>(mut self, max_size: Option<S>) -> Self {
        self.max_size = max_size.map(|s| s.into());
        self
    }

    /// The amount of zoom (scaling) to apply. This is applied on top of the
    /// system's native scaling factor.
    ///
    /// This is ignored if [`size`](EguiWindowSettings::size) is in physical units.
    ///
    /// The zoom factor can also be changed later with
    /// [`Context::set_zoom_factor()`](egui::Context::set_zoom_factor).
    #[inline]
    pub fn with_zoom_factor(mut self, zoom_factor: f32) -> Self {
        self.zoom_factor = zoom_factor;
        self
    }

    /// If the window is to be embedded in a parent window, the handle to that window.
    ///
    /// If `None`, the window will be standalone.
    #[inline]
    pub fn with_parent<'a, P: HasWindowHandle + 'a>(
        mut self,
        parent: impl Into<Option<&'a P>>,
    ) -> Self {
        self.parent = parent.into().map(ParentWindowHandle::from_window);
        self
    }

    /// Sets [`wait_for_parent`](Self::wait_for_parent) to the given value.
    pub fn with_wait_for_parent(mut self, wait_for_parent: bool) -> Self {
        self.wait_for_parent = wait_for_parent;
        self
    }

    /// Sets [`wait_for_parent`](Self::wait_for_parent) to `true`.
    #[inline]
    pub fn wait_for_parent(mut self) -> Self {
        self.wait_for_parent = true;
        self
    }

    /// Sets [`fallback_scale_factor`](Self::fallback_scale_factor) to the given value.
    #[inline]
    pub fn with_fallback_scale_factor(mut self, scale_factor: impl Into<Option<f64>>) -> Self {
        self.fallback_scale_factor = scale_factor.into();
        self
    }

    /// The graphics configuration
    #[inline]
    pub fn with_graphics_config(mut self, config: GraphicsConfig) -> Self {
        self.graphics = config;
        self
    }

    /// A clone of the realtime-safe handle to request a update & repaint for an egui app.
    ///
    /// This can be used, for example, to notify the GUI that the value of a decibel
    /// meter has changed.
    #[inline]
    pub fn with_repaint_notifier(mut self, repaint_notifier: RepaintNotifier) -> Self {
        self.repaint_notifier = Some(repaint_notifier);
        self
    }

    /// The rate at which the [`repaint_notifier`](EguiWindowSettings::repaint_notifier) will be
    /// polled in seconds.
    ///
    /// Defaults to `1.0 / 64.0` seconds.
    #[inline]
    pub fn with_repaint_poll_interval(mut self, interval: Duration) -> Self {
        self.repaint_poll_interval = interval;
        self
    }
}

impl Default for EguiWindowSettings {
    fn default() -> Self {
        Self {
            title: String::new(),
            size: Size::Logical(LogicalSize {
                width: 300.0,
                height: 200.0,
            }),
            min_size: None,
            max_size: None,
            resizable: true,
            zoom_factor: 1.0,
            graphics: GraphicsConfig::default(),
            parent: None,
            wait_for_parent: false,
            fallback_scale_factor: None,
            repaint_notifier: None,
            repaint_poll_interval: Duration::from_secs_f64(1.0 / 64.0),
        }
    }
}

/// Represents the surroundings of your app.
pub struct Frame {
    renderer: Renderer,
    window: WindowContext,
    clear_color: Rgba,
    key_capture: KeyCapture,
}

impl Frame {
    #[cfg(all(feature = "dpi-trace", feature = "wgpu", target_os = "windows"))]
    pub fn gpu_counters(&self) -> crate::diagnostics::GpuCounters {
        self.renderer.timing.borrow().counters
    }
    fn new(renderer: Renderer, window: WindowContext) -> Self {
        Self {
            renderer,
            window,
            clear_color: Rgba::BLACK,
            key_capture: Default::default(),
        }
    }

    /// Set the clear color of the renderer.
    pub fn set_clear_color(&mut self, clear_color: Rgba) {
        self.clear_color = clear_color;
    }

    /// Set how to handle capturing key events from the host.
    pub fn set_key_capture(&mut self, key_capture: KeyCapture) {
        self.key_capture = key_capture;
    }

    /// Access the internal baseview window.
    pub fn baseview_window(&self) -> &WindowContext {
        &self.window
    }

    /// A reference to the underlying
    /// [glow](https://docs.rs/glow/0.17.0/x86_64-unknown-linux-gnu/glow/index.html)
    /// (OpenGL) context.
    ///
    /// This can be used, for instance, to:
    ///
    /// * Render things to offscreen buffers.
    /// * Read the pixel buffer from the previous frame (glow::Context::read_pixels).
    /// * Render things behind the egui windows.
    ///
    /// Note that all egui painting is deferred to after the call to App::ui
    /// (egui only collects egui::Shapes and then egui-baseview paints them all in
    /// one go later on).
    #[cfg(feature = "opengl")]
    pub fn gl(&self) -> &std::sync::Arc<egui_glow::glow::Context> {
        &self.renderer.glow_context
    }
}

/// Describes how to handle capturing key events from the host.
#[derive(Default, Debug, Clone, PartialEq)]
pub enum KeyCapture {
    #[default]
    /// All keys will be captured from the host.
    CaptureAll,
    /// No keys will be captured from the host.
    IgnoreAll,
    /// Only the given keys will be captured from the host.
    CaptureKeys(Vec<keyboard_types::Key>),
    /// All keys except the given ones will be captured from the host.
    IgnoreKeys(Vec<keyboard_types::Key>),
}

struct EguiWindowInner<A: App> {
    user_app: A,
    egui_ctx: egui::Context,
    clipboard_ctx: Option<copypasta::ClipboardContext>,
    frame: Frame,
}

/// Handles an egui-baseview application
pub struct EguiWindow<A: App> {
    inner: RefCell<EguiWindowInner<A>>,
    egui_input: RefCell<egui::RawInput>,
    viewport_id: egui::ViewportId,
    start_time: Instant,
    system_scale_factor: Cell<f64>,
    zoom_factor: Cell<f32>,
    pointer_logical_pos: Cell<Option<egui::Pos2>>,
    current_cursor_icon: Cell<baseview::MouseCursor>,
    repaint_notifier: Option<RepaintNotifier>,
    modifiers: Cell<egui::Modifiers>,
    poll_repaint_timer_handle: baseview::TimerHandle,

    // Re-use the allocations from the previous output.
    full_output: RefCell<FullOutput>,

    pub window: WindowContext,
}

impl<A: App> EguiWindow<A> {
    fn new(
        window: WindowContext,
        title: String,
        graphics_config: GraphicsConfig,
        mut user_app: A,
        zoom_factor: f32,
        repaint_notifier: Option<RepaintNotifier>,
        repaint_poll_interval: Duration,
    ) -> Result<EguiWindow<A>, HandlerError> {
        let renderer = Renderer::new(window.clone(), graphics_config)?;
        let egui_ctx = egui::Context::default();

        egui_ctx.set_zoom_factor(zoom_factor);

        {
            let waker = window.waker();
            egui_ctx.set_request_repaint_callback(move |request_repaint_info| {
                waker.request_redraw_after(request_repaint_info.delay)
            });
        }

        let size = window.size();

        let system_scale_factor = size.scale_factor;
        let total_scale_factor = system_scale_factor * zoom_factor as f64;

        let logical_size: LogicalSize<f64> = size.physical.to_logical(total_scale_factor);

        let screen_rect = Rect::from_min_size(
            Pos2::new(0f32, 0f32),
            vec2(logical_size.width as f32, logical_size.height as f32),
        );

        let viewport_info = egui::ViewportInfo {
            parent: None,
            title: Some(title),
            native_pixels_per_point: Some(system_scale_factor as f32),
            focused: Some(true),
            inner_rect: Some(screen_rect),
            ..Default::default()
        };
        let viewport_id = egui::ViewportId::default();

        let mut egui_input = egui::RawInput {
            max_texture_side: Some(renderer.max_texture_side()),
            screen_rect: Some(screen_rect),
            ..Default::default()
        };
        let _ = egui_input.viewports.insert(viewport_id, viewport_info);

        let mut frame = Frame::new(renderer, window.clone());

        user_app.build(egui_ctx.clone(), &mut frame)?;

        #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
        crate::diagnostics::window_event("egui_created", &window, format_args!("zoom={zoom_factor} system_factor={system_scale_factor} initial_input={screen_rect:?}"));

        let clipboard_ctx = match copypasta::ClipboardContext::new() {
            Ok(clipboard_ctx) => Some(clipboard_ctx),
            Err(e) => {
                #[cfg(any(feature = "tracing", feature = "log"))]
                error!("Failed to initialize clipboard: {}", e);

                #[cfg(not(any(feature = "tracing", feature = "log")))]
                let _ = e;

                None
            }
        };

        let poll_repaint_timer_handle = window.create_timer(repaint_poll_interval)?;

        let start_time = Instant::now();

        Ok(Self {
            inner: RefCell::new(EguiWindowInner {
                user_app,
                egui_ctx,
                clipboard_ctx,
                frame,
            }),
            viewport_id,
            start_time,
            egui_input: egui_input.into(),
            pointer_logical_pos: None.into(),
            current_cursor_icon: baseview::MouseCursor::Default.into(),
            system_scale_factor: system_scale_factor.into(),
            zoom_factor: zoom_factor.into(),
            window,
            repaint_notifier,
            modifiers: Cell::new(egui::Modifiers::default()),
            full_output: RefCell::new(FullOutput::default()),
            poll_repaint_timer_handle,
        })
    }

    /// Open a new window.
    ///
    /// * `settings` - The settings of the window.
    /// * `app` - The application to run.
    pub fn create(settings: EguiWindowSettings, app: A) -> Result<Window, baseview::Error> {
        Self::create_with_host(settings, app, None)
    }

    /// Open a new window.
    ///
    /// * `settings` - The settings of the window.
    /// * `app` - The application to run.
    /// * `host` - The baseview ['Host'](baseview::host::Host) callbacks.
    pub fn create_with_host(
        settings: EguiWindowSettings,
        app: A,
        host: Option<baseview::host::Host>,
    ) -> Result<Window, baseview::Error> {
        let size = match settings.size {
            Size::Logical(size) => Size::Logical(LogicalSize {
                width: size.width as f64 * settings.zoom_factor as f64,
                height: size.height as f64 * settings.zoom_factor as f64,
            }),
            Size::Physical(size) => Size::Physical(size),
        };

        let mut options = WindowSettings::new()
            .with_title(settings.title.clone())
            .with_size(size)
            .with_min_size::<Size>(settings.min_size)
            .with_max_size::<Size>(settings.max_size)
            .with_resizable(settings.resizable)
            .with_wait_for_parent(settings.wait_for_parent)
            .with_fallback_scale_factor(settings.fallback_scale_factor);

        options.parent = settings.parent;

        #[cfg(feature = "opengl")]
        let options = { options.with_gl_config(Some(settings.graphics.gl_config.clone())) };

        Window::create_with_host(
            options,
            move |window| {
                EguiWindow::new(
                    window,
                    settings.title,
                    settings.graphics,
                    app,
                    settings.zoom_factor,
                    settings.repaint_notifier,
                    settings.repaint_poll_interval,
                )
            },
            host,
        )
    }

    fn poll_repaint_timer(&self) {
        if let Some(repaint_notifier) = &self.repaint_notifier
            && repaint_notifier.repaint_requested()
        {
            self.window.request_redraw();
        }
    }
}

/// Update the pressed key modifiers when a mouse event has sent a new set of modifiers.
fn update_modifiers(
    old_modifiers: &Cell<egui::Modifiers>,
    new_modifiers: &Modifiers,
    egui_input: &mut egui::RawInput,
) {
    let (new_mac_cmd, new_command) = if cfg!(target_os = "macos") {
        let m = new_modifiers.meta();
        (m, m)
    } else {
        (false, new_modifiers.ctrl())
    };

    let new_modifiers = egui::Modifiers {
        alt: new_modifiers.alt(),
        ctrl: new_modifiers.ctrl(),
        shift: new_modifiers.shift(),
        mac_cmd: new_mac_cmd,
        command: new_command,
    };

    if old_modifiers.get() != new_modifiers {
        old_modifiers.set(new_modifiers);
        egui_input
            .events
            .push(egui::Event::ModifiersChanged(new_modifiers));
    }
}

impl<A: App> WindowHandler for EguiWindow<A> {
    fn poll(&self) {
        self.poll_repaint_timer();
    }

    fn draw(&self) -> Result<(), HandlerError> {
        #[cfg(all(feature = "dpi-trace", feature = "wgpu", target_os = "windows"))]
        let ui_started = Instant::now();
        let (egui_input, logical_size) = {
            let mut egui_input = self.egui_input.borrow_mut();
            egui_input.time = Some(self.start_time.elapsed().as_secs_f64());

            let zoom_factor = self.zoom_factor.get();

            let size = self.window.size();
            let logical_size: LogicalSize<f32> = size
                .physical
                .to_logical(size.scale_factor * zoom_factor as f64);

            let screen_rect = Rect::from_min_size(
                Pos2::new(0f32, 0f32),
                vec2(logical_size.width, logical_size.height),
            );

            egui_input.screen_rect = Some(screen_rect);
            (egui_input.take(), logical_size)
        };

        // Re-use the allocations from the previous output.
        let mut full_output = self.full_output.borrow_mut();

        *full_output = {
            let mut inner = self.inner.borrow_mut();
            let EguiWindowInner {
                user_app,
                egui_ctx,
                clipboard_ctx: _,
                frame,
            } = &mut *inner;

            egui_ctx.run_ui(egui_input, |ui| user_app.ui(ui, frame))
        };

        let Some(viewport_output) = full_output.viewport_output.get(&self.viewport_id) else {
            // The main window was closed by egui.
            self.window.request_close();
            return Ok(());
        };

        let mut new_size = None;
        let new_zoom = { self.inner.borrow().egui_ctx.zoom_factor() };

        if self.zoom_factor.get() != new_zoom {
            self.zoom_factor.set(new_zoom);

            let mut inner = self.inner.borrow_mut();

            inner.egui_ctx.set_zoom_factor(new_zoom);
            inner.user_app.zoom_factor_changed(new_zoom);

            new_size = Some(Size::Logical(LogicalSize::new(
                (logical_size.width * new_zoom) as f64,
                (logical_size.height * new_zoom) as f64,
            )));
        }

        for command in viewport_output.commands.iter() {
            match command {
                ViewportCommand::Close => {
                    self.window.request_close();
                }
                ViewportCommand::InnerSize(size) => {
                    new_size = Some(Size::Logical(LogicalSize::new(
                        (size.x * new_zoom) as f64,
                        (size.y * new_zoom) as f64,
                    )));
                }
                ViewportCommand::Focus => {
                    self.window.focus()?;
                }
                _ => {}
            }
        }

        if let Some(new_size) = new_size {
            if let Err(e) = self.window.resize(new_size) {
                error!("Failed to resize window: {}", e);
            }
        }

        {
            let mut inner = self.inner.borrow_mut();
            let EguiWindowInner {
                user_app: _,
                egui_ctx,
                clipboard_ctx,
                frame,
            } = &mut *inner;

            let size = self.window.size();
            #[cfg(all(feature = "dpi-trace", feature = "wgpu", target_os = "windows"))]
            {
                frame.renderer.timing.borrow_mut().ui_ms = ui_started.elapsed().as_secs_f64() * 1000.0;
            }
            frame.renderer.render(
                &self.window,
                frame.clear_color,
                size.physical,
                size.scale_factor as f32 * egui_ctx.zoom_factor(),
                egui_ctx,
                &mut full_output,
            );

            for command in full_output.platform_output.commands.drain(..) {
                match command {
                    egui::OutputCommand::CopyText(text) => {
                        if let Some(clipboard_ctx) = clipboard_ctx.as_mut()
                            && let Err(err) = clipboard_ctx.set_contents(text)
                        {
                            #[cfg(any(feature = "tracing", feature = "log"))]
                            error!("Copy/Cut error: {}", err);

                            #[cfg(not(any(feature = "tracing", feature = "log")))]
                            let _ = err;
                        }
                    }
                    egui::OutputCommand::CopyImage(_) => {
                        #[cfg(any(feature = "tracing", feature = "log"))]
                        warn!("Copying images is not supported in egui_baseview.");
                    }
                    egui::OutputCommand::OpenUrl(open_url) => {
                        if let Err(err) = open::that_detached(&open_url.url) {
                            #[cfg(any(feature = "tracing", feature = "log"))]
                            error!("Open error: {}", err);

                            #[cfg(not(any(feature = "tracing", feature = "log")))]
                            let _ = err;
                        }
                    }
                    // TODO?
                    egui::OutputCommand::TextSelectionSettled(_) => {}
                }
            }
        }

        let cursor_icon =
            crate::translate::translate_cursor_icon(full_output.platform_output.cursor_icon);
        if self.current_cursor_icon.get() != cursor_icon {
            self.current_cursor_icon.set(cursor_icon);

            self.window.set_mouse_cursor(cursor_icon)?;
        }

        // A temporary workaround for keyboard input not working sometimes.
        // See https://codeberg.org/RustAudio/egui-baseview/issues/20
        #[cfg(feature = "keyboard_focus_workaround")]
        {
            if !full_output.platform_output.events.is_empty()
                || full_output.platform_output.ime.is_some()
            {
                self.window.focus();
            }
        }

        Ok(())
    }

    fn resized(&self, new_size: WindowSize) -> Result<(), HandlerError> {
        #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
        crate::diagnostics::window_event("egui_resized", &self.window, format_args!("requested={new_size:?}"));
        let zoom_factor = self.inner.borrow().egui_ctx.zoom_factor();

        let total_scale_factor = new_size.scale_factor * zoom_factor as f64;
        let logical_size: LogicalSize<f64> = new_size.physical.to_logical(total_scale_factor);

        let screen_rect = Rect::from_min_size(
            Pos2::new(0f32, 0f32),
            vec2(logical_size.width as f32, logical_size.height as f32),
        );

        let mut egui_input = self.egui_input.borrow_mut();

        egui_input.screen_rect = Some(screen_rect);

        let viewport_info = egui_input.viewports.get_mut(&self.viewport_id).unwrap();
        viewport_info.native_pixels_per_point = Some(new_size.scale_factor as f32);
        viewport_info.inner_rect = Some(screen_rect);

        self.system_scale_factor.set(new_size.scale_factor);

        let mut inner = self.inner.borrow_mut();

        inner.user_app.resized(WindowSize {
            physical: new_size.physical,
            logical: logical_size,
            scale_factor: total_scale_factor,
        });

        Ok(())
    }

    fn on_event(&self, event: Event) -> EventStatus {
        let mut return_status = EventStatus::Captured;

        // Parent/embedded windows do not always gain keyboard focus
        // Automatically on click. Request focus explicitly before forwarding the event.
        //
        // TODO: Check if this is still necessary.
        if matches!(
            event,
            baseview::Event::Mouse(baseview::MouseEvent::ButtonPressed { .. })
        ) && !self.window.has_focus()
        {
            self.window.focus().unwrap();
        }

        let mut egui_input = self.egui_input.borrow_mut();

        let mut do_repaint = true;

        match &event {
            baseview::Event::Mouse(event) => match event {
                baseview::MouseEvent::CursorMoved {
                    position,
                    modifiers,
                } => {
                    update_modifiers(&self.modifiers, modifiers, &mut egui_input);

                    let logical_pos: LogicalPosition<f32> = position.to_logical(
                        self.system_scale_factor.get()
                            * self.inner.borrow().egui_ctx.zoom_factor() as f64,
                    );
                    let pos = pos2(logical_pos.x, logical_pos.y);

                    self.pointer_logical_pos.set(Some(pos));
                    egui_input.events.push(egui::Event::PointerMoved(pos));
                }
                baseview::MouseEvent::ButtonPressed { button, modifiers } => {
                    update_modifiers(&self.modifiers, modifiers, &mut egui_input);

                    if let Some(pos) = self.pointer_logical_pos.get()
                        && let Some(button) = crate::translate::translate_mouse_button(*button)
                    {
                        egui_input.events.push(egui::Event::PointerButton {
                            pos,
                            button,
                            pressed: true,
                            modifiers: self.modifiers.get(),
                        });
                    }
                }
                baseview::MouseEvent::ButtonReleased { button, modifiers } => {
                    update_modifiers(&self.modifiers, modifiers, &mut egui_input);

                    if let Some(pos) = self.pointer_logical_pos.get()
                        && let Some(button) = crate::translate::translate_mouse_button(*button)
                    {
                        egui_input.events.push(egui::Event::PointerButton {
                            pos,
                            button,
                            pressed: false,
                            modifiers: self.modifiers.get(),
                        });
                    }
                }
                baseview::MouseEvent::WheelScrolled {
                    delta: scroll_delta,
                    modifiers,
                } => {
                    update_modifiers(&self.modifiers, modifiers, &mut egui_input);

                    #[allow(unused_mut)]
                    let (unit, mut delta) = match scroll_delta {
                        baseview::ScrollDelta::Lines { x, y } => {
                            (egui::MouseWheelUnit::Line, egui::vec2(*x, *y))
                        }

                        baseview::ScrollDelta::Pixels { x, y } => (
                            egui::MouseWheelUnit::Point,
                            egui::vec2(*x, *y) * self.window.scale_factor() as f32,
                        ),
                    };

                    if cfg!(target_os = "macos") {
                        // This is still buggy in winit despite
                        // https://github.com/rust-windowing/winit/issues/1695 being closed
                        //
                        // TODO: See if this is an issue in baseview as well.
                        delta.x *= -1.0;
                    }

                    egui_input.events.push(egui::Event::MouseWheel {
                        unit,
                        delta,
                        modifiers: self.modifiers.get(),
                        phase: egui::TouchPhase::Move,
                        source: egui::MouseWheelSource::Unknown,
                    });
                }
                baseview::MouseEvent::CursorLeft => {
                    self.pointer_logical_pos.set(None);
                    egui_input.events.push(egui::Event::PointerGone);
                }
                _ => do_repaint = false,
            },
            baseview::Event::Keyboard(event) => {
                update_modifiers(&self.modifiers, &event.modifiers, &mut egui_input);

                let pressed = event.state == keyboard_types::KeyState::Down;

                let modifiers = self.modifiers.get();

                if let Some(key) = crate::translate::translate_virtual_key(&event.key) {
                    egui_input.events.push(egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed,
                        repeat: event.repeat,
                        modifiers,
                    });
                }

                if pressed {
                    // VirtualKeyCode::Paste etc in winit are broken/untrustworthy,
                    // so we detect these things manually:
                    //
                    // TODO: See if this is an issue in baseview as well.
                    if is_cut_command(modifiers, event.code) {
                        egui_input.events.push(egui::Event::Cut);
                    } else if is_copy_command(modifiers, event.code) {
                        egui_input.events.push(egui::Event::Copy);
                    } else if is_paste_command(modifiers, event.code) {
                        if let Some(clipboard_ctx) = self.inner.borrow_mut().clipboard_ctx.as_mut()
                        {
                            match clipboard_ctx.get_contents() {
                                Ok(contents) => egui_input.events.push(egui::Event::Text(contents)),
                                Err(err) => {
                                    #[cfg(any(feature = "tracing", feature = "log"))]
                                    error!("Paste error: {}", err);

                                    #[cfg(not(any(feature = "tracing", feature = "log")))]
                                    let _ = err;
                                }
                            }
                        }
                    } else if let keyboard_types::Key::Character(written) = &event.key
                        && !modifiers.ctrl
                        && !modifiers.command
                    {
                        egui_input.events.push(egui::Event::Text(written.clone()));
                    }
                }

                match &self.inner.borrow().frame.key_capture {
                    KeyCapture::CaptureAll => {}
                    KeyCapture::IgnoreAll => return_status = EventStatus::Ignored,
                    KeyCapture::CaptureKeys(keys) => {
                        if !keys.contains(&event.key) {
                            return_status = EventStatus::Ignored
                        }
                    }
                    KeyCapture::IgnoreKeys(keys) => {
                        if keys.contains(&event.key) {
                            return_status = EventStatus::Ignored
                        }
                    }
                }
            }
            baseview::Event::Window(event) => match event {
                baseview::WindowEvent::Focused => {
                    egui_input.events.push(egui::Event::WindowFocused(true));
                    egui_input
                        .viewports
                        .get_mut(&self.viewport_id)
                        .unwrap()
                        .focused = Some(true);
                }
                baseview::WindowEvent::Unfocused => {
                    egui_input.events.push(egui::Event::WindowFocused(false));
                    egui_input
                        .viewports
                        .get_mut(&self.viewport_id)
                        .unwrap()
                        .focused = Some(false);
                }
                baseview::WindowEvent::WillClose => {}
                _ => {}
            },
            _ => do_repaint = false,
        }

        if do_repaint {
            self.window.request_redraw();
        }

        // For keyboard events, also check if egui actually wants keyboard input
        // This allows DAW shortcuts (spacebar, etc.) to pass through when no text field is focused
        match &event {
            baseview::Event::Keyboard(_) => {
                let egui_ctx = &self.inner.borrow().egui_ctx;
                if return_status == EventStatus::Captured && !egui_ctx.egui_wants_keyboard_input() {
                    EventStatus::Ignored
                } else {
                    return_status
                }
            }
            baseview::Event::Mouse(_) => {
                let egui_ctx = &self.inner.borrow().egui_ctx;
                if egui_ctx.egui_is_using_pointer() || egui_ctx.egui_wants_pointer_input() {
                    EventStatus::Captured
                } else {
                    EventStatus::Ignored
                }
            }
            baseview::Event::Window(_) => EventStatus::Captured,
            _ => EventStatus::Ignored,
        }
    }

    fn on_timer(&self, timer: &baseview::TimerHandle) {
        if timer == self.poll_repaint_timer_handle {
            self.poll_repaint_timer();
        }
    }
}

fn is_cut_command(modifiers: egui::Modifiers, keycode: keyboard_types::Code) -> bool {
    (modifiers.command && keycode == keyboard_types::Code::KeyX)
        || (cfg!(target_os = "windows")
            && modifiers.shift
            && keycode == keyboard_types::Code::Delete)
}

fn is_copy_command(modifiers: egui::Modifiers, keycode: keyboard_types::Code) -> bool {
    (modifiers.command && keycode == keyboard_types::Code::KeyC)
        || (cfg!(target_os = "windows")
            && modifiers.ctrl
            && keycode == keyboard_types::Code::Insert)
}

fn is_paste_command(modifiers: egui::Modifiers, keycode: keyboard_types::Code) -> bool {
    (modifiers.command && keycode == keyboard_types::Code::KeyV)
        || (cfg!(target_os = "windows")
            && modifiers.shift
            && keycode == keyboard_types::Code::Insert)
}
