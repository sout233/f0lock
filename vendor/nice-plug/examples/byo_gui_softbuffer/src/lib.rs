//! This plugin demonstrates how to "bring your own GUI toolkit" using a raw Softbuffer rendering context.

use baseview::{HandlerError, WindowContext, WindowSettings, dpi::LogicalSize};
use crossbeam::atomic::AtomicCell;
use nice_plug::{
    context::gui::GuiContext,
    editor::{EditorHandle, HostMethods, SpawnedEditor, dpi::NativeSize},
    prelude::*,
};
use softbuffer::SoftBufferError;
use std::{
    cell::RefCell,
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const MIN_SIZE: LogicalSize<f32> = LogicalSize::new(200.0, 150.0);
const RESIZE_HINT: ResizeHint = ResizeHint::resizable().with_min_logical_size(MIN_SIZE);

pub struct SoftbufferWindow {
    _gui_context: GuiContext,
    window: WindowContext,
    poll_timer_handle: baseview::TimerHandle,

    surface: RefCell<Surface>,

    #[allow(unused)]
    params: Arc<MyPluginParams>,
    editor_state: Arc<SoftbufferEditorState>,
    redraw_requested: Arc<AtomicBool>,
}

struct Surface {
    _sb_context: softbuffer::Context<WindowContext>,
    sb_surface: softbuffer::Surface<WindowContext, WindowContext>,
}

impl SoftbufferWindow {
    fn new(
        window: WindowContext,
        gui_context: GuiContext,
        params: Arc<MyPluginParams>,
        editor_state: Arc<SoftbufferEditorState>,
        redraw_requested: Arc<AtomicBool>,
        poll_timer_handle: baseview::TimerHandle,
    ) -> Result<Self, SoftBufferError> {
        let size = window.size();

        let sb_context = softbuffer::Context::new(window.clone())?;
        let mut sb_surface = softbuffer::Surface::new(&sb_context, window.clone())?;

        sb_surface.resize(
            NonZeroU32::new(size.physical.width).unwrap(),
            NonZeroU32::new(size.physical.height).unwrap(),
        )?;

        Ok(Self {
            _gui_context: gui_context,
            window,
            surface: RefCell::new(Surface {
                _sb_context: sb_context,
                sb_surface,
            }),
            params,
            editor_state,
            redraw_requested,
            poll_timer_handle,
        })
    }
}

impl baseview::WindowHandler for SoftbufferWindow {
    fn poll(&self) {
        if self.redraw_requested.swap(false, Ordering::Relaxed) {
            self.window.request_redraw();
        }
    }

    fn draw(&self) -> Result<(), HandlerError> {
        // Do rendering here.

        let mut surface = self.surface.borrow_mut();
        let Surface {
            _sb_context,
            sb_surface,
        } = &mut *surface;

        let mut buffer = sb_surface.buffer_mut()?;

        let width = buffer.width().get();
        let height = buffer.height().get();

        for y in 0..height {
            for x in 0..width {
                let red = x % 255;
                let green = y % 255;
                let blue = (x * y) % 255;
                let alpha = 255;

                let index = (y as usize * width as usize) + x as usize;
                buffer[index] = blue | (green << 8) | (red << 16) | (alpha << 24);
            }
        }

        if let Err(e) = buffer.present() {
            nice_plug::nice_error!("{}", e);
        }

        Ok(())
    }

    fn on_event(&self, event: baseview::Event) -> baseview::EventStatus {
        // Do event processing here.
        #[allow(clippy::single_match)]
        #[allow(clippy::collapsible_match)]
        match &event {
            baseview::Event::Window(event) => match event {
                baseview::WindowEvent::Focused => {
                    self.redraw_requested.store(true, Ordering::Relaxed);
                }
                _ => {}
            },
            _ => {}
        }

        baseview::EventStatus::Captured
    }

    fn resized(&self, new_size: baseview::WindowSize) -> Result<(), HandlerError> {
        self.surface.borrow_mut().sb_surface.resize(
            NonZeroU32::new(new_size.physical.width).unwrap(),
            NonZeroU32::new(new_size.physical.height).unwrap(),
        )?;

        self.editor_state.scale_factor.store(new_size.scale_factor);
        self.editor_state
            .logical_size
            .store(new_size.logical.cast());

        self.redraw_requested.store(true, Ordering::Relaxed);

        Ok(())
    }

    fn on_timer(&self, timer: &baseview::TimerHandle) {
        if timer == self.poll_timer_handle {
            self.poll();
        }
    }
}

pub struct SoftbufferEditor {
    params: Arc<MyPluginParams>,
    editor_state: Arc<SoftbufferEditorState>,
}

impl Editor for SoftbufferEditor {
    type Handle = SoftbufferEditorHandle;

    fn spawn(
        &self,
        parent: Option<ParentWindowHandle>,
        wait_for_parent: bool,
        fallback_scale_factor: Option<f64>,
        gui_context: GuiContext,
        host: Option<HostMethods>,
    ) -> Result<SpawnedEditor<Self::Handle>, Box<dyn Error>> {
        let params = Arc::clone(&self.params);
        let editor_state = Arc::clone(&self.editor_state);

        let redraw_requested = Arc::new(AtomicBool::new(true));
        let redraw_requested_2 = Arc::clone(&redraw_requested);

        // The host is a re-implementation of
        // [`baseview::host::HostCallbacks`](https://docs.rs/baseview/latest/baseview/host/trait.HostCallbacks.html)
        // and
        // [`baseview::host::HostMainThreadCaller`](https://docs.rs/baseview/latest/baseview/host/trait.HostMainThreadCaller.html)
        // to avoid `nice-plug-core` from depending on `baseview` until it is stabilized.
        //
        // Create a small wrapper to adapt it to baseview's HostCallbacks traits.
        let host = {
            struct HostCallbackAdapter {
                host: Box<dyn nice_plug::editor::HostCallbacks>,
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
                host: Box<dyn nice_plug::editor::HostMainThreadCaller>,
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

        let window = baseview::Window::create_with_host(
            WindowSettings::new()
                .with_title("Softbuffer Window")
                .with_size(self.editor_state.logical_size())
                .with_min_size::<LogicalSize<f32>>(Some(MIN_SIZE))
                .with_resizable(RESIZE_HINT.can_resize)
                .with_parent(parent.as_ref())
                .with_wait_for_parent(wait_for_parent)
                .with_fallback_scale_factor(fallback_scale_factor),
            move |window: WindowContext| -> Result<SoftbufferWindow, HandlerError> {
                editor_state.scale_factor.store(window.size().scale_factor);

                let poll_timer_handle = window.create_timer(Duration::from_secs_f64(1.0 / 64.0))?;

                SoftbufferWindow::new(
                    window,
                    gui_context,
                    params,
                    editor_state,
                    redraw_requested_2,
                    poll_timer_handle,
                )
                .map_err(|e| e.into())
            },
            host,
        )?;

        self.editor_state.open.store(true, Ordering::Release);

        Ok(SpawnedEditor {
            handle: SoftbufferEditorHandle {
                state: Arc::clone(&self.editor_state),
                redraw_requested,
            },
            window,
        })
    }

    fn size(&self) -> NativeSize<u32> {
        NativeSize::from_size(
            self.editor_state.logical_size().into(),
            self.editor_state.scale_factor(),
        )
    }

    fn resize_hint(&self) -> ResizeHint {
        RESIZE_HINT
    }

    unsafe fn assume_standalone_in_process(&self) {
        // Safety: Editor::assume_standalone_in_process() has the same invariants as
        // baseview::assume_standalone_in_process()
        unsafe {
            baseview::assume_standalone_in_process();
        }
    }
}

/// A handle to a spawned instance of our Editor.
pub struct SoftbufferEditorHandle {
    state: Arc<SoftbufferEditorState>,
    redraw_requested: Arc<AtomicBool>,
}

impl EditorHandle for SoftbufferEditorHandle {
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
        self.redraw_requested.store(true, Ordering::Relaxed);

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
        window.resize(new_size)
    }

    fn set_fallback_scale_factor(
        &self,
        scale_factor: f64,
        window: &Self::Window,
    ) -> Result<(), Self::Error> {
        window.suggest_fallback_scale_factor(scale_factor)
    }

    /// Return the closest supported size.
    fn adjust_size(
        &self,
        new_size: NativeSize<u32>,
        window: &Self::Window,
    ) -> Option<NativeSize<u32>> {
        let current_size = window.size();
        Some(RESIZE_HINT.adjust_size(
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
        false
    }

    fn state_changed(&self) {
        self.redraw_requested.store(true, Ordering::Relaxed);
    }

    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {
        // The UI should generally be redrawn when a param is changed.
        self.redraw_requested.store(true, Ordering::Relaxed);
    }

    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {
        // The UI should generally be redrawn when a param is changed.
        self.redraw_requested.store(true, Ordering::Relaxed);
    }
}

impl Drop for SoftbufferEditorHandle {
    fn drop(&mut self) {
        self.state.open.store(false, Ordering::Release);
    }
}

#[derive(Debug)]
pub struct SoftbufferEditorState {
    logical_size: AtomicCell<LogicalSize<f32>>,
    /// Whether the editor's window is currently open.
    open: AtomicBool,
    scale_factor: AtomicCell<f64>,
}

impl SoftbufferEditorState {
    pub fn from_size(size: LogicalSize<f32>) -> Self {
        Self {
            logical_size: AtomicCell::new(size),
            open: AtomicBool::new(false),
            scale_factor: AtomicCell::new(1.0),
        }
    }

    pub fn logical_size(&self) -> LogicalSize<f32> {
        self.logical_size.load()
    }

    pub fn scale_factor(&self) -> f64 {
        self.scale_factor.load()
    }

    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::Acquire)
    }
}

// ---------------------------------------------------------------------------------------------------

/// This is mostly identical to the gain example, minus some fluff, and with a GUI.
pub struct MyPlugin {
    params: Arc<MyPluginParams>,
    editor_state: Arc<SoftbufferEditorState>,
}

#[derive(Params)]
pub struct MyPluginParams {
    #[id = "gain"]
    pub gain: FloatParam,

    #[id = "foobar"]
    pub some_int: IntParam,
}

impl Default for MyPlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(MyPluginParams::default()),
            editor_state: Arc::new(SoftbufferEditorState::from_size(MIN_SIZE)),
        }
    }
}

impl Default for MyPluginParams {
    fn default() -> Self {
        Self {
            // See the main gain example for more details
            gain: FloatParam::new(
                "Gain",
                util::db_to_gain(0.0),
                FloatRange::gain_range(-30.0, 30.0),
            )
            .with_smoother(SmoothingStyle::Logarithmic(50.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(2))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
            some_int: IntParam::new("Something", 3, IntRange::Linear { min: 0, max: 3 }),
        }
    }
}

impl Plugin for MyPlugin {
    const NAME: &'static str = "BYO GUI Example (Softbuffer)";
    const VENDOR: &'static str = "Moist Plugins GmbH";
    const URL: &'static str = "https://youtu.be/dQw4w9WgXcQ";
    const EMAIL: &'static str = "info@example.com";

    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type Editor = SoftbufferEditor;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        Some(SoftbufferEditor {
            params: Arc::clone(&self.params),
            editor_state: Arc::clone(&self.editor_state),
        })
    }

    fn activate(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        _buffer_config: &BufferConfig,
        _context: &mut impl ActivateContext<Self>,
    ) -> bool {
        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        for channel_samples in buffer.iter_samples() {
            let gain = self.params.gain.smoothed.next();
            for sample in channel_samples {
                *sample *= gain;
            }

            // To save resources, a plugin can (and probably should!) only perform expensive
            // calculations that are only displayed on the GUI while the GUI is open
            if self.editor_state.is_open() {
                // Do things
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for MyPlugin {
    const CLAP_ID: &'static str = "com.moist-plugins-gmbh.byo-gui-softbuffer";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("A simple example plugin with a raw Softbuffer context for rendering");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Utility,
    ];
}

impl Vst3Plugin for MyPlugin {
    const VST3_CLASS_ID: [u8; 16] = *b"ByoGuiSoftbuffer";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Tools];
}

nice_export_clap!(MyPlugin);
nice_export_vst3!(MyPlugin);
