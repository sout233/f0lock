use super::OpenGlError;
use baseview::WindowContext;
use baseview::dpi::PhysicalSize;
use baseview::gl::GlConfig;
use egui::FullOutput;
use egui_glow::Painter;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct GraphicsConfig {
    pub gl_config: GlConfig,

    /// Controls whether to apply dithering to minimize banding artifacts.
    ///
    /// Dithering assumes an sRGB output and thus will apply noise to any input value that lies between
    /// two 8bit values after applying the sRGB OETF function, i.e. if it's not a whole 8bit value in "gamma space".
    /// This means that only inputs from texture interpolation and vertex colors should be affected in practice.
    ///
    /// Defaults to true.
    pub dithering: bool,

    /// Needed for cross compiling for VirtualBox VMSVGA driver with OpenGL ES 2.0 and OpenGL 2.1 which doesn't support SRGB texture.
    /// See <https://github.com/emilk/egui/pull/1993>.
    ///
    /// For OpenGL ES 2.0: set this to [`egui_glow::ShaderVersion::Es100`] to solve blank texture problem (by using the "fallback shader").
    pub shader_version: Option<egui_glow::ShaderVersion>,
}

impl Default for GraphicsConfig {
    fn default() -> Self {
        Self {
            gl_config: GlConfig::default(),
            shader_version: None,
            dithering: true,
        }
    }
}

pub struct Renderer {
    pub glow_context: Arc<egui_glow::glow::Context>,
    painter: Painter,
}

impl Renderer {
    pub fn new(window: WindowContext, config: GraphicsConfig) -> Result<Self, OpenGlError> {
        let context = window.gl_context().ok_or(OpenGlError::NoContext)?;
        unsafe {
            context.make_current()?;
        }

        let glow_context = Arc::new(unsafe {
            egui_glow::glow::Context::from_loader_function_cstr(|s| context.get_proc_address(s))
        });

        let painter = egui_glow::Painter::new(
            Arc::clone(&glow_context),
            "",
            config.shader_version,
            config.dithering,
        )
        .map_err(OpenGlError::CreatePainter)?;

        unsafe {
            context.make_not_current()?;
        }

        Ok(Self {
            glow_context,
            painter,
        })
    }

    pub fn max_texture_side(&self) -> usize {
        self.painter.max_texture_side()
    }

    pub fn render(
        &mut self,
        window: &WindowContext,
        clear_color: egui::Rgba,
        physical_size: PhysicalSize<u32>,
        pixels_per_point: f32,
        egui_ctx: &mut egui::Context,
        full_output: &mut FullOutput,
    ) {
        let PhysicalSize {
            width: canvas_width,
            height: canvas_height,
        } = physical_size;

        let shapes = std::mem::take(&mut full_output.shapes);
        let textures_delta = &mut full_output.textures_delta;

        let context = window
            .gl_context()
            .expect("failed to get baseview gl context");
        unsafe {
            context.make_current().unwrap();
        }

        unsafe {
            use egui_glow::glow::HasContext as _;
            self.glow_context.clear_color(
                clear_color.r(),
                clear_color.g(),
                clear_color.b(),
                clear_color.a(),
            );
            self.glow_context.clear(egui_glow::glow::COLOR_BUFFER_BIT);
        }

        #[expect(clippy::iter_over_hash_type)] // Order doesn't matter here
        for (id, image_deltas) in textures_delta.set.drain() {
            for image_delta in image_deltas {
                self.painter.set_texture(id, &image_delta);
            }
        }

        let clipped_primitives = egui_ctx.tessellate(shapes, pixels_per_point);
        let dimensions: [u32; 2] = [canvas_width, canvas_height];

        self.painter
            .paint_primitives(dimensions, pixels_per_point, &clipped_primitives);

        #[expect(clippy::iter_over_hash_type)] // Order doesn't matter here
        for id in textures_delta.free.drain() {
            self.painter.free_texture(id);
        }

        unsafe {
            context.swap_buffers().unwrap();
            context.make_not_current().unwrap();
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.painter.destroy()
    }
}
