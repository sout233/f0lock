//! This plugin demonstrates how to "bring your own GUI toolkit" using a raw WGPU context.

use baseview::{HandlerError, WindowContext, WindowSettings};
use crossbeam::atomic::AtomicCell;
use nice_plug::context::gui::GuiContext;
use nice_plug::editor::dpi::{LogicalSize, NativeSize};
use nice_plug::editor::{EditorHandle, HostMethods, SpawnedEditor};
use nice_plug::prelude::*;
use std::cell::RefCell;
use std::error::Error;
use std::time::Duration;
use std::{
    borrow::Cow,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const MIN_SIZE: LogicalSize<f32> = LogicalSize::new(400.0, 300.0);
const RESIZE_HINT: ResizeHint = ResizeHint::resizable().with_min_logical_size(MIN_SIZE);

pub struct WgpuWindow {
    _gui_context: GuiContext,
    window: WindowContext,
    poll_timer_handle: baseview::TimerHandle,

    surface: RefCell<Surface>,

    #[allow(unused)]
    params: Arc<MyPluginParams>,
    editor_state: Arc<WgpuEditorState>,
    redraw_requested: Arc<AtomicBool>,
}

struct Surface {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
}

impl WgpuWindow {
    fn new(
        window: WindowContext,
        gui_context: GuiContext,
        params: Arc<MyPluginParams>,
        editor_state: Arc<WgpuEditorState>,
        redraw: Arc<AtomicBool>,
    ) -> Result<Self, HandlerError> {
        pollster::block_on(Self::create(
            window,
            gui_context,
            params,
            editor_state,
            redraw,
        ))
    }

    async fn create(
        window: WindowContext,
        gui_context: GuiContext,
        params: Arc<MyPluginParams>,
        editor_state: Arc<WgpuEditorState>,
        redraw: Arc<AtomicBool>,
    ) -> Result<Self, HandlerError> {
        let size = window.size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

        let surface = instance.create_surface(window.platform_handle())?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                force_fallback_adapter: false,
                // Request an adapter which can render to our surface
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;

        // Create the logical device and command queue
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                ..Default::default()
            })
            .await?;

        const SHADER: &str = "
            const VERTS = array(
                vec2<f32>(0.5, 1.0),
                vec2<f32>(0.0, 0.0),
                vec2<f32>(1.0, 0.0)
            );

            struct VertexOutput {
                @builtin(position) clip_position: vec4<f32>,
                @location(0) position: vec2<f32>,
            };

            @vertex
            fn vs_main(
                @builtin(vertex_index) in_vertex_index: u32,
            ) -> VertexOutput {
                var out: VertexOutput;
                out.position = VERTS[in_vertex_index];
                out.clip_position = vec4<f32>(out.position - 0.5, 0.0, 1.0);
                return out;
            }

            @fragment
            fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
                return vec4<f32>(in.position, 0.5, 1.0);
            }
            ";

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER)),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[],
            immediate_size: 0,
        });

        let swapchain_capabilities = surface.get_capabilities(&adapter);
        let swapchain_format = swapchain_capabilities.formats[0];

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(swapchain_format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let mut surface_config = surface
            .get_default_config(&adapter, size.physical.width, size.physical.height)
            .unwrap(); // TODO

        // IMPORTANT! Plugins should not use vsync as it blocks the host's main thread.
        surface_config.present_mode = wgpu::PresentMode::AutoNoVsync;

        surface.configure(&device, &surface_config);

        // Note, for some reason wgpu doesn't work if this timer is created before the
        // surface is created.
        let poll_timer_handle = window.create_timer(Duration::from_secs_f64(1.0 / 64.0))?;

        Ok(Self {
            _gui_context: gui_context,
            window,
            surface: RefCell::new(Surface {
                device,
                queue,
                pipeline,
                surface,
                surface_config,
            }),
            params,
            editor_state,
            redraw_requested: redraw,
            poll_timer_handle,
        })
    }
}

impl baseview::WindowHandler for WgpuWindow {
    fn poll(&self) {
        if self.redraw_requested.swap(false, Ordering::Relaxed) {
            self.window.request_redraw();
        }
    }

    fn draw(&self) -> Result<(), HandlerError> {
        // Do rendering here.
        let mut surface = self.surface.borrow_mut();
        let Surface {
            device,
            queue,
            pipeline,
            surface,
            surface_config,
        } = &mut *surface;

        let mut recreate_surface = false;
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => Some(texture),
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Suboptimal(_) | wgpu::CurrentSurfaceTexture::Outdated => {
                None
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                unreachable!("No error scope registered, so validation errors will panic")
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                recreate_surface = true;
                None
            }
        };

        let Some(frame) = frame else {
            if recreate_surface {
                let instance =
                    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

                *surface = instance
                    .create_surface(self.window.platform_handle())
                    .unwrap();
            }

            surface.configure(device, surface_config);
            self.window.request_redraw();
            return Ok(());
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            rpass.set_pipeline(pipeline);
            rpass.draw(0..3, 0..1);
        }

        queue.submit(Some(encoder.finish()));
        queue.present(frame);

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
        self.editor_state.scale_factor.store(new_size.scale_factor);
        self.editor_state
            .logical_size
            .store(new_size.logical.cast());

        {
            let mut surface = self.surface.borrow_mut();
            let Surface {
                device,
                queue: _,
                pipeline: _,
                surface,
                surface_config,
            } = &mut *surface;

            surface_config.width = new_size.physical.width;
            surface_config.height = new_size.physical.height;

            surface.configure(device, surface_config);
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

pub struct WgpuEditor {
    params: Arc<MyPluginParams>,
    editor_state: Arc<WgpuEditorState>,
}

impl Editor for WgpuEditor {
    type Handle = WgpuEditorHandle;

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
                .with_title("Wgpu Window")
                .with_size(self.editor_state.logical_size())
                .with_min_size::<LogicalSize<f32>>(Some(MIN_SIZE))
                .with_resizable(RESIZE_HINT.can_resize)
                .with_parent(parent.as_ref())
                .with_wait_for_parent(wait_for_parent)
                .with_fallback_scale_factor(fallback_scale_factor),
            move |window: WindowContext| -> Result<WgpuWindow, HandlerError> {
                WgpuWindow::new(
                    window,
                    gui_context,
                    params,
                    editor_state,
                    redraw_requested_2,
                )
            },
            host,
        )?;

        self.editor_state.open.store(true, Ordering::Release);

        Ok(SpawnedEditor {
            handle: WgpuEditorHandle {
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
pub struct WgpuEditorHandle {
    state: Arc<WgpuEditorState>,
    redraw_requested: Arc<AtomicBool>,
}

impl EditorHandle for WgpuEditorHandle {
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

impl Drop for WgpuEditorHandle {
    fn drop(&mut self) {
        self.state.open.store(false, Ordering::Release);
    }
}

#[derive(Debug)]
pub struct WgpuEditorState {
    logical_size: AtomicCell<LogicalSize<f32>>,
    /// Whether the editor's window is currently open.
    open: AtomicBool,
    scale_factor: AtomicCell<f64>,
}

impl WgpuEditorState {
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
    editor_state: Arc<WgpuEditorState>,
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
            editor_state: Arc::new(WgpuEditorState::from_size(MIN_SIZE)),
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
    const NAME: &'static str = "BYO GUI Example (WGPU)";
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

    type Editor = WgpuEditor;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        Some(WgpuEditor {
            params: Arc::clone(&self.params),
            editor_state: Arc::clone(&self.editor_state),
        })
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
                // Do stuff
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for MyPlugin {
    const CLAP_ID: &'static str = "com.moist-plugins-gmbh.byo-gui-wgpu";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("A simple example plugin with a raw WGPU context for rendering");
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
    const VST3_CLASS_ID: [u8; 16] = *b"ByoGuiWGPUWooooo";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Tools];
}

nice_export_clap!(MyPlugin);
nice_export_vst3!(MyPlugin);
