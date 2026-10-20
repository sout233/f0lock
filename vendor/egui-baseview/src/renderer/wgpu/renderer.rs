use std::sync::Arc;

use baseview::{WindowContext, dpi::PhysicalSize};
use egui::FullOutput;
use egui_wgpu::{
    RenderState, RendererOptions, ScreenDescriptor, WgpuError,
    wgpu::{
        Color, CommandEncoderDescriptor, Extent3d, RenderPassColorAttachment, RenderPassDescriptor,
        Surface, SurfaceConfiguration, TextureDescriptor, TextureDimension, TextureUsages,
        TextureView, TextureViewDescriptor,
    },
};

pub use egui_wgpu::WgpuConfiguration;

#[derive(Debug, Clone)]
pub struct GraphicsConfig {
    /// Controls whether to apply dithering to minimize banding artifacts.
    ///
    /// Dithering assumes an sRGB output and thus will apply noise to any input value that lies between
    /// two 8bit values after applying the sRGB OETF function, i.e. if it's not a whole 8bit value in "gamma space".
    /// This means that only inputs from texture interpolation and vertex colors should be affected in practice.
    ///
    /// Defaults to true.
    pub dithering: bool,

    /// Configures wgpu instance/device/adapter/surface creation and renderloop.
    pub wgpu_options: WgpuConfiguration,

    /// Additional options for the wgpu renderer.
    pub renderer_options: RendererOptions,
}

impl Default for GraphicsConfig {
    fn default() -> Self {
        Self {
            dithering: true,
            wgpu_options: Default::default(),
            renderer_options: Default::default(),
        }
    }
}

pub struct Renderer {
    #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
    pub(crate) timing: std::cell::RefCell<crate::diagnostics::GpuTiming>,
    render_state: Arc<RenderState>,
    instance: wgpu::Instance,
    surface: Surface<'static>,
    config: GraphicsConfig,
    msaa_texture_view: Option<TextureView>,
    msaa_samples: u32,
    width: u32,
    height: u32,
}

impl Renderer {
    pub fn new(window: WindowContext, config: GraphicsConfig) -> Result<Self, WgpuError> {
        #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
        let started = std::time::Instant::now();
        let mut setup = config.wgpu_options.wgpu_setup.clone();
        if let egui_wgpu::WgpuSetup::CreateNew(create_new) = &mut setup {
            create_new.display_handle = Some(Box::new(window.platform_handle()));
        }
        let instance = pollster::block_on(setup.new_instance());

        let surface = instance.create_surface(window.platform_handle()).unwrap();

        let msaa_samples = config.renderer_options.msaa_samples;

        let state = Arc::new(pollster::block_on(RenderState::create(
            &config.wgpu_options,
            &instance,
            Some(&surface),
            config.renderer_options,
        ))?);

        #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
        crate::diagnostics::window_event("gpu_created", &window, format_args!("adapter={:?} msaa={} present={:?} initialization_ms={}", state.adapter.get_info(), msaa_samples, config.wgpu_options.surface.present_mode, started.elapsed().as_secs_f64() * 1000.0));

        Ok(Self {
            #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
            timing: Default::default(),
            render_state: state,
            instance,
            surface,
            config,
            msaa_texture_view: None,
            msaa_samples,
            width: 0,
            height: 0,
        })
    }

    pub fn max_texture_side(&self) -> usize {
        self.render_state
            .as_ref()
            .device
            .limits()
            .max_texture_dimension_2d as usize
    }

    fn configure_surface(&self, width: u32, height: u32) {
        #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
        {
            self.timing.borrow_mut().counters.configurations += 1;
        }
        let usage = TextureUsages::RENDER_ATTACHMENT;

        let mut surf_config = SurfaceConfiguration {
            usage,
            format: self.render_state.target_format,
            present_mode: self.config.wgpu_options.surface.present_mode,
            view_formats: vec![self.render_state.target_format],
            ..self
                .surface
                .get_default_config(&self.render_state.adapter, width, height)
                .expect("Unsupported surface")
        };

        if let Some(desired_maximum_frame_latency) = self
            .config
            .wgpu_options
            .surface
            .desired_maximum_frame_latency
        {
            surf_config.desired_maximum_frame_latency = desired_maximum_frame_latency;
        }

        self.surface
            .configure(&self.render_state.device, &surf_config);
    }

    fn resize_and_generate_msaa_view(&mut self, width: u32, height: u32) {
        let render_state = self.render_state.as_ref();

        self.width = width;
        self.height = height;

        self.configure_surface(width, height);

        let texture_format = render_state.target_format;

        if self.msaa_samples > 1 {
            self.msaa_texture_view = Some(
                render_state
                    .device
                    .create_texture(&TextureDescriptor {
                        label: Some("egui_msaa_texture"),
                        size: Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: self.msaa_samples.max(1),
                        dimension: TextureDimension::D2,
                        format: texture_format,
                        usage: TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[texture_format],
                    })
                    .create_view(&TextureViewDescriptor::default()),
            );
        }
    }

    pub fn render(
        &mut self,
        window: &baseview::WindowContext,
        clear_color: egui::Rgba,
        physical_size: PhysicalSize<u32>,
        pixels_per_point: f32,
        egui_ctx: &mut egui::Context,
        full_output: &mut FullOutput,
    ) {
        #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
        let started = std::time::Instant::now();
        let shapes = std::mem::take(&mut full_output.shapes);

        let clipped_primitives = egui_ctx.tessellate(shapes, pixels_per_point);

        let mut encoder =
            self.render_state
                .device
                .create_command_encoder(&CommandEncoderDescriptor {
                    label: Some("encoder"),
                });

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [physical_size.width, physical_size.height],
            pixels_per_point,
        };

        let user_cmd_bufs = {
            let mut renderer = self.render_state.renderer.write();
            for (id, image_deltas) in full_output.textures_delta.set.drain() {
                for image_delta in image_deltas {
                    renderer.update_texture(
                        &self.render_state.device,
                        &self.render_state.queue,
                        id,
                        &image_delta,
                    );
                }
            }

            renderer.update_buffers(
                &self.render_state.device,
                &self.render_state.queue,
                &mut encoder,
                &clipped_primitives,
                &screen_descriptor,
            )
        };

        if self.width != physical_size.width
            || self.height != physical_size.height
            || (self.msaa_samples > 1 && self.msaa_texture_view.is_none())
        {
            self.resize_and_generate_msaa_view(physical_size.width, physical_size.height);
        }

        let mut recreate_surface = false;
        #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
        let acquire_started = std::time::Instant::now();
        let output_frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => Some(texture),
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => return,
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

        #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
        let acquire_ms = acquire_started.elapsed().as_secs_f64() * 1000.0;

        let Some(output_frame) = output_frame else {
            if recreate_surface {
                self.surface = self
                    .instance
                    .create_surface(window.platform_handle())
                    .unwrap();
            }

            self.configure_surface(self.width, self.height);
            return;
        };

        {
            let renderer = self.render_state.renderer.read();
            let frame_view = output_frame
                .texture
                .create_view(&TextureViewDescriptor::default());

            let (view, resolve_target) = if let Some(msaa_view) = &self.msaa_texture_view {
                (msaa_view, Some(&frame_view))
            } else {
                (&frame_view, None)
            };

            let render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("egui_render"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target,
                    ops: egui_wgpu::wgpu::Operations {
                        load: egui_wgpu::wgpu::LoadOp::Clear(Color {
                            r: clear_color[0] as f64,
                            g: clear_color[1] as f64,
                            b: clear_color[2] as f64,
                            a: clear_color[3] as f64,
                        }),
                        store: egui_wgpu::wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            // Forgetting the pass' lifetime means that we are no longer compile-time protected from
            // runtime errors caused by accessing the parent encoder before the render pass is dropped.
            // Since we don't pass it on to the renderer, we should be perfectly safe against this mistake here!
            renderer.render(
                &mut render_pass.forget_lifetime(),
                &clipped_primitives,
                &screen_descriptor,
            );
        }

        {
            let mut renderer = self.render_state.renderer.write();
            for id in &full_output.textures_delta.free {
                renderer.free_texture(id);
            }
        }

        let encoded = encoder.finish();

        #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
        let submit_started = std::time::Instant::now();

        self.render_state
            .queue
            .submit(user_cmd_bufs.into_iter().chain([encoded]));

        #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
        let submit_ms = submit_started.elapsed().as_secs_f64() * 1000.0;
        #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
        let present_started = std::time::Instant::now();

        self.render_state.queue.present(output_frame);

        #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
        self.timing.borrow_mut().frame(window, [started.elapsed().as_secs_f64() * 1000.0, acquire_ms, submit_ms, present_started.elapsed().as_secs_f64() * 1000.0], egui_ctx);
    }
}
