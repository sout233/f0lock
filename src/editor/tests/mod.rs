use super::*;
use nice_plug::{
    context::{PluginApi, gui::GuiContextInner},
    params::internals::ParamPtr,
    prelude::PluginState,
};
use std::sync::Mutex;

mod about;
mod dynamics;
mod gestures;
mod knobs;
mod modes;
#[cfg(target_os = "windows")]
mod native;
mod points;
mod preview;
mod scale;
mod spectrum;
mod target;

#[derive(Default)]
pub(super) struct RecordingHost {
    pub(super) events: Mutex<Vec<&'static str>>,
}
impl GuiContextInner for RecordingHost {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Clap
    }

    unsafe fn raw_begin_set_parameter(&self, _: ParamPtr) {
        self.events.lock().unwrap().push("begin");
    }

    unsafe fn raw_set_parameter_normalized(&self, param: ParamPtr, value: f32) {
        self.events.lock().unwrap().push("set");
        unsafe {
            param._internal_set_normalized_value(value);
        }
    }

    unsafe fn raw_end_set_parameter(&self, _: ParamPtr) {
        self.events.lock().unwrap().push("end");
    }

    fn get_state(&self) -> PluginState {
        panic!("Unexpected state read");
    }

    fn set_state(&self, _: PluginState) {
        panic!("Unexpected state write");
    }

    fn request_restart(&self) {
        panic!("Unexpected restart");
    }
}

struct Fixture {
    ctx: egui::Context,
    editor: F0Editor,
    host: Arc<RecordingHost>,
    shapes: Vec<egui::epaint::ClippedShape>,
    viewport_commands: Vec<egui::ViewportCommand>,
    opened_urls: Vec<String>,
    time: f64,
}

impl Fixture {
    fn new() -> Self {
        let ctx = egui::Context::default();
        configure_style(&ctx);

        let telemetry = Arc::new(Telemetry::default());
        let mut engine = crate::engine::Engine::new(48000.0);

        for i in 0..24000 {
            let x = 0.1 * (std::f32::consts::TAU * 220.0 * i as f32 / 48000.0).sin();
            engine.process([x; 2], crate::engine::Controls::default());
        }

        telemetry.publish(&engine);

        let mut editor = F0Editor::new(Arc::new(F0Params::default()), telemetry);
        let host = Arc::new(RecordingHost::default());
        editor.gui = Some(GuiContext::new(host.clone()));

        let mut fixture = Self {
            ctx,
            editor,
            host,
            shapes: Vec::new(),
            viewport_commands: Vec::new(),
            opened_urls: Vec::new(),
            time: 0.0,
        };

        fixture.frame(vec![]);
        fixture
    }

    fn frame(&mut self, events: Vec<egui::Event>) {
        self.time += 1.0 / 60.0;
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 724.0))),
                events,
                time: Some(self.time),
                ..Default::default()
            },
            |ui| self.editor.draw(ui),
        );
        output.textures_delta.clear();
        self.opened_urls.extend(
            output
                .platform_output
                .commands
                .iter()
                .filter_map(|command| match command {
                    egui::OutputCommand::OpenUrl(url) => Some(url.url.clone()),
                    _ => None,
                }),
        );
        self.viewport_commands = output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .clone();
        self.shapes = output.shapes;
    }

    fn click(&mut self, pos: Pos2, button: egui::PointerButton) {
        self.frame(vec![egui::Event::PointerMoved(pos)]);
        self.frame(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        self.frame(vec![egui::Event::PointerButton {
            pos,
            button,
            pressed: false,
            modifiers: Default::default(),
        }]);
    }

    fn click_text(&mut self, text: &str) {
        self.frame(vec![]);

        let position = self
            .shapes
            .iter()
            .rev()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(t) if t.galley.job.text == text => {
                    Some(shape.shape.visual_bounding_rect().center())
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!(
                    "Menu item missing: {text}; target={:?}; labels={:?}",
                    self.editor.context_point,
                    self.shapes
                        .iter()
                        .filter_map(|s| match &s.shape {
                            egui::Shape::Text(t) => Some(t.galley.job.text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                )
            });
        self.click(position, egui::PointerButton::Primary);
    }

    fn add(&mut self) {
        self.click_text("+");
    }

    fn handle(&self, index: usize) -> Pos2 {
        let v = self.editor.telemetry.values();
        let b = self.editor.params.band(index);
        let f0 = if v[2] > 0.0 { v[2] } else { PREVIEW_F0 };
        let hz = f0 * b.ratio.value();
        let gain = if b.kind.value().has_gain() {
            b.gain.value()
        } else {
            0.0
        };
        Pos2::new(
            57.0 + (hz / 20.0).ln() / 1000.0_f32.ln() * 786.0,
            172.5 - gain / 48.0 * 155.0,
        )
    }

    fn knob_position(&self, center: Pos2) -> f32 {
        let scale = 0.85;
        let direction = self
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::LineSegment { points, stroke }
                    if stroke.width == 3.5 * scale && points[1].distance(center) < 35.0 =>
                {
                    Some(points[1] - points[0])
                }
                _ => None,
            })
            .expect("Knob pointer was not rendered");
        let mut angle = direction.y.atan2(direction.x);

        if angle < std::f32::consts::PI * 0.75 {
            angle += std::f32::consts::TAU;
        }
        (angle / std::f32::consts::PI - 0.75) / 1.5
    }
}
