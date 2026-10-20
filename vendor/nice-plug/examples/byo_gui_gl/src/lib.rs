//! This plugin demonstrates how to "bring your own GUI toolkit" using a raw OpenGL context.

use baseview::{
    HandlerError, WindowContext, WindowSettings,
    dpi::LogicalSize,
    gl::{GlConfig, GlContext},
};
use crossbeam::atomic::AtomicCell;
use glow::Context;
use nice_plug::editor::{EditorHandle, HostMethods, SpawnedEditor, dpi::NativeSize};
use nice_plug::{context::gui::GuiContext, prelude::*};
use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crate::OpenGlError::GlError;

const MIN_SIZE: LogicalSize<f32> = LogicalSize::new(400.0, 300.0);
const RESIZE_HINT: ResizeHint = ResizeHint::resizable().with_min_logical_size(MIN_SIZE);

/// Helper for parsing and interpreting the OpenGL shader version. This will
/// help ensure maximum compatibility with systems.
/// (borrowed and modified from
/// https://github.com/emilk/egui/blob/main/crates/egui_glow/src/shader_version.rs)
fn get_shader_version_string(gl: &Arc<Context>) -> &'static str {
    use glow::HasContext as _;

    #[cfg(not(target_arch = "wasm32"))]
    if gl.version().major < 2 {
        // this checks on desktop that we are not using opengl 1.1 microsoft sw rendering context.
        // ShaderVersion::get fn will segfault due to SHADING_LANGUAGE_VERSION (added in gl2.0)
        panic!("OpenGL 2.0+ is not supported on this device.");
    }

    let glsl_ver = unsafe { gl.get_parameter_string(glow::SHADING_LANGUAGE_VERSION) };

    let shader_version = {
        let start = glsl_ver.find(|c| char::is_ascii_digit(&c)).unwrap();
        let es = glsl_ver[..start].contains(" ES ");
        let ver = glsl_ver[start..]
            .split_once(' ')
            .map_or(&glsl_ver[start..], |x| x.0);
        let [maj, min]: [u8; 2] = ver
            .splitn(3, '.')
            .take(2)
            .map(|x| x.parse().unwrap_or_default())
            .collect::<Vec<u8>>()
            .try_into()
            .unwrap();

        // Put your supported shader versions here
        if es {
            if maj >= 3 {
                "#version 300 es"
            } else {
                "#version 100"
            }
        } else if maj > 1 || (maj == 1 && min >= 40) {
            "#version 140"
        } else {
            "#version 120"
        }
    };

    nice_log!("Shader version: {shader_version} ({glsl_ver:?})");

    shader_version
}

#[derive(Debug, thiserror::Error)]
pub enum OpenGlError {
    #[error("Failed to get baseview's GL context")]
    NoContext,
    #[error("GL program: {0}")]
    GlError(String),
    #[error("{0}")]
    Baseview(#[from] baseview::Error),
}

impl From<String> for OpenGlError {
    fn from(err: String) -> Self {
        Self::GlError(err)
    }
}

// A small gaurd that makes sure the OpenGL context is released if an error
// is returned.
struct ContextGaurd<'a> {
    gl_context: &'a GlContext,
}

impl<'a> ContextGaurd<'a> {
    pub unsafe fn new(gl_context: &'a GlContext) -> Result<Self, OpenGlError> {
        unsafe {
            gl_context.make_current()?;
        }
        Ok(Self { gl_context })
    }
}

impl<'a> Drop for ContextGaurd<'a> {
    fn drop(&mut self) {
        unsafe {
            self.gl_context.make_not_current().unwrap();
        }
    }
}

pub struct GlWindow {
    _gui_context: GuiContext,
    gl: Arc<glow::Context>,
    window: WindowContext,
    poll_timer_handle: baseview::TimerHandle,

    vertex_array: glow::NativeVertexArray,
    program: glow::NativeProgram,

    #[allow(unused)]
    params: Arc<MyPluginParams>,
    editor_state: Arc<GlEditorState>,
    redraw_requested: Arc<AtomicBool>,
}

impl Drop for GlWindow {
    fn drop(&mut self) {
        use glow::HasContext as _;

        unsafe {
            self.gl.delete_program(self.program);
            self.gl.delete_vertex_array(self.vertex_array);
        }
    }
}

impl GlWindow {
    fn new(
        window: WindowContext,
        gui_context: GuiContext,
        params: Arc<MyPluginParams>,
        editor_state: Arc<GlEditorState>,
        redraw_requested: Arc<AtomicBool>,
        poll_timer_handle: baseview::TimerHandle,
    ) -> Result<Self, OpenGlError> {
        use glow::HasContext as _;

        let gl_context = window.gl_context().ok_or(OpenGlError::NoContext)?;

        let (gl, vertex_array, program) = unsafe {
            let _context_gaurd = ContextGaurd::new(&gl_context);

            #[allow(clippy::arc_with_non_send_sync)]
            let gl = Arc::new(glow::Context::from_loader_function_cstr(|s| {
                gl_context.get_proc_address(s)
            }));

            let shader_version = get_shader_version_string(&gl);

            let vertex_array = gl.create_vertex_array()?;
            gl.bind_vertex_array(Some(vertex_array));

            let program = gl.create_program()?;

            let (vertex_shader_source, fragment_shader_source) = (
                r#"const vec2 verts[3] = vec2[3](
                    vec2(0.5f, 1.0f),
                    vec2(0.0f, 0.0f),
                    vec2(1.0f, 0.0f)
                );
                out vec2 vert;
                void main() {
                    vert = verts[gl_VertexID];
                    gl_Position = vec4(vert - 0.5, 0.0, 1.0);
                }"#,
                r#"precision mediump float;
                in vec2 vert;
                out vec4 color;
                void main() {
                    color = vec4(vert, 0.5, 1.0);
                }"#,
            );

            let shader_sources = [
                (glow::VERTEX_SHADER, vertex_shader_source),
                (glow::FRAGMENT_SHADER, fragment_shader_source),
            ];

            let mut shaders = Vec::with_capacity(shader_sources.len());

            for (shader_type, shader_source) in shader_sources.iter() {
                let shader = gl
                    .create_shader(*shader_type)
                    .expect("Cannot create shader");
                gl.shader_source(shader, &format!("{}\n{}", shader_version, shader_source));
                gl.compile_shader(shader);
                if !gl.get_shader_compile_status(shader) {
                    return Err(GlError(gl.get_shader_info_log(shader)));
                }
                gl.attach_shader(program, shader);
                shaders.push(shader);
            }

            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                return Err(GlError(gl.get_program_info_log(program)));
            }

            for shader in shaders {
                gl.detach_shader(program, shader);
                gl.delete_shader(shader);
            }

            gl.use_program(Some(program));

            (gl, vertex_array, program)
        };

        Ok(Self {
            _gui_context: gui_context,
            gl,
            vertex_array,
            program,
            params,
            editor_state,
            window,
            redraw_requested,
            poll_timer_handle,
        })
    }
}

impl baseview::WindowHandler for GlWindow {
    fn poll(&self) {
        if self.redraw_requested.swap(false, Ordering::Relaxed) {
            self.window.request_redraw();
        }
    }

    fn draw(&self) -> Result<(), HandlerError> {
        // Do rendering here.

        use glow::HasContext as _;

        let gl_context = self.window.gl_context().unwrap();

        unsafe {
            let _context_gaurd = ContextGaurd::new(&gl_context);

            self.gl.clear_color(0.05, 0.05, 0.05, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);

            self.gl.draw_arrays(glow::TRIANGLES, 0, 3);

            if let Err(e) = gl_context.swap_buffers() {
                nice_plug::nice_error!("{}", e);
            }
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
        use glow::HasContext as _;

        self.editor_state.scale_factor.store(new_size.scale_factor);
        self.editor_state
            .logical_size
            .store(new_size.logical.cast());

        let gl_context = self.window.gl_context().unwrap();

        unsafe {
            let _context_gaurd = ContextGaurd::new(&gl_context);

            self.gl.viewport(
                0,
                0,
                new_size.physical.width as i32,
                new_size.physical.height as i32,
            );
        }

        self.redraw_requested.store(true, Ordering::Relaxed);

        Ok(())
    }

    fn on_timer(&self, timer: &baseview::TimerHandle) {
        if timer == self.poll_timer_handle {
            self.poll();
        }
    }
}

pub struct GlEditor {
    params: Arc<MyPluginParams>,
    editor_state: Arc<GlEditorState>,
}

impl Editor for GlEditor {
    type Handle = GlEditorHandle;

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
                .with_title("OpenGL Window")
                .with_size(self.editor_state.logical_size())
                .with_min_size::<LogicalSize<f32>>(Some(MIN_SIZE))
                .with_resizable(RESIZE_HINT.can_resize)
                .with_parent(parent.as_ref())
                .with_wait_for_parent(wait_for_parent)
                .with_fallback_scale_factor(fallback_scale_factor)
                .with_gl_config(Some(GlConfig {
                    version: (3, 2),
                    red_bits: 8,
                    blue_bits: 8,
                    green_bits: 8,
                    alpha_bits: 8,
                    depth_bits: 24,
                    stencil_bits: 8,
                    samples: None,
                    srgb: true,
                    ..Default::default()
                })),
            move |window: WindowContext| -> Result<GlWindow, HandlerError> {
                editor_state.scale_factor.store(window.size().scale_factor);

                let poll_timer_handle = window.create_timer(Duration::from_secs_f64(1.0 / 64.0))?;

                GlWindow::new(
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
            handle: GlEditorHandle {
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
pub struct GlEditorHandle {
    state: Arc<GlEditorState>,
    redraw_requested: Arc<AtomicBool>,
}

impl EditorHandle for GlEditorHandle {
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

impl Drop for GlEditorHandle {
    fn drop(&mut self) {
        self.state.open.store(false, Ordering::Release);
    }
}

#[derive(Debug)]
pub struct GlEditorState {
    /// The window's size in logical pixels before applying `scale_factor`.
    logical_size: AtomicCell<LogicalSize<f32>>,
    /// Whether the editor's window is currently open.
    open: AtomicBool,
    scale_factor: AtomicCell<f64>,
}

impl GlEditorState {
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
    editor_state: Arc<GlEditorState>,
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
            editor_state: Arc::new(GlEditorState::from_size(MIN_SIZE)),
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
    const NAME: &'static str = "BYO GUI Example (OpenGL)";
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

    type Editor = GlEditor;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        Some(GlEditor {
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
                // Put stuff here
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for MyPlugin {
    const CLAP_ID: &'static str = "com.moist-plugins-gmbh.byo-gui-gl";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("A simple example plugin with a raw OpenGL context for rendering");
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
    const VST3_CLASS_ID: [u8; 16] = *b"ByoGuiOpenGLWooo";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Tools];
}

nice_export_clap!(MyPlugin);
nice_export_vst3!(MyPlugin);
