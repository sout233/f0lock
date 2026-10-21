use crate::{Telemetry, band::BAND_COUNT, filter::FilterKind, params::F0Params};
use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use nice_plug::{
    context::gui::{GuiContext, ParamSetter},
    prelude::*,
};
use nice_plug_egui::NiceEguiApp;
use std::{sync::Arc, time::Duration};

#[path = "controls.rs"]
mod controls;
#[path = "credits.rs"]
mod credits;
#[path = "knob.rs"]
mod knob;
#[path = "mode_ui.rs"]
mod mode_ui;
#[path = "point_tabs.rs"]
mod point_tabs;
#[path = "settings.rs"]
mod settings;
#[path = "spectrum.rs"]
mod spectrum;
#[path = "target_ui.rs"]
mod target_ui;
use knob::{RED, gradient_rect};
#[cfg(test)]
#[path = "editor_points_tests.rs"]
mod point_tests;
#[cfg(test)]
#[path = "editor_tests.rs"]
mod tests;

const TEXT: Color32 = Color32::from_rgb(213, 214, 214);
const MUTED: Color32 = Color32::from_rgb(145, 148, 150);
const GRID: Color32 = Color32::from_rgb(54, 57, 59);
const GLOBAL_ACCENT: Color32 = Color32::from_rgb(103, 108, 113);
const GLOBAL_FONT: &str = "global-bold";

pub struct F0Editor {
    params: Arc<F0Params>,
    telemetry: Arc<Telemetry>,
    gui: Option<GuiContext>,
    selected: Option<usize>,
    context_point: Option<usize>,
    notice: Option<Notice>,
    about_open: bool,
    about_credits: bool,
    credits_image: Option<egui::TextureHandle>,
    shown_mode: crate::mode::OperatingMode,
    mode_selection: [Option<usize>; 2],
    audio_open: bool,
}

struct Notice {
    message: &'static str,
    expires_at: f64,
}

const POINT_COLORS: [Color32; BAND_COUNT] = [
    RED,
    Color32::from_rgb(196, 130, 45),
    Color32::from_rgb(49, 153, 120),
    Color32::from_rgb(80, 131, 200),
    Color32::from_rgb(151, 108, 192),
    Color32::from_rgb(203, 105, 151),
    Color32::from_rgb(140, 159, 61),
    Color32::from_rgb(50, 164, 182),
];
// This is only the initial visual layout; the detector still reports no F0.
const PREVIEW_F0: f32 = 150.0;

impl F0Editor {
    pub(crate) fn new(params: Arc<F0Params>, telemetry: Arc<Telemetry>) -> Self {
        let selected = (0..BAND_COUNT).find(|&i| params.band(i).exists.value());
        let shown_mode = params.mode.value();
        Self {
            params,
            telemetry,
            gui: None,
            selected,
            context_point: None,
            notice: None,
            about_open: false,
            about_credits: false,
            credits_image: None,
            shown_mode,
            mode_selection: [Some(0); 2],
            audio_open: false,
        }
    }

    pub(crate) fn draw(&mut self, ui: &mut egui::Ui) {
        let mode = self.params.mode.value();
        if mode != self.shown_mode {
            self.mode_selection[self.shown_mode as usize] = self.selected;
            self.selected = self.mode_selection[mode as usize];
            self.shown_mode = mode;
            self.context_point = None;
            self.notice = None;
        }
        if let Err(error) = self.restore_ui_scale(ui.ctx()) {
            ui.colored_label(RED, error.to_string());
            return;
        }

        if self
            .selected
            .is_some_and(|i| !self.params.band(i).exists.value())
        {
            self.selected = (0..BAND_COUNT).find(|&i| self.params.band(i).exists.value());
        }

        let v = self.telemetry.values();
        ui.ctx().style_mut_of(egui::Theme::Dark, |style| {
            style.visuals.selection.bg_fill = self.accent().gamma_multiply(0.3);
            style.visuals.selection.stroke = Stroke::new(1.0, self.accent());
        });

        let (canvas, _) = ui.allocate_exact_size(Vec2::new(900.0, 724.0), Sense::hover());
        let origin = canvas.min.to_vec2();
        let at = |x, y| Pos2::new(x, y) + origin;

        let p = ui.painter_at(canvas);
        gradient_rect(
            &p,
            Rect::from_min_max(at(0.0, 0.0), at(900.0, 314.0)),
            Color32::from_gray(47),
            Color32::from_gray(31),
        );
        gradient_rect(
            &p,
            Rect::from_min_max(at(0.0, 0.0), at(900.0, 64.0)),
            Color32::from_gray(66),
            Color32::from_gray(43),
        );
        p.line_segment(
            [at(0.0, 1.0), at(900.0, 1.0)],
            Stroke::new(1.0, Color32::from_gray(93)),
        );
        p.line_segment(
            [at(0.0, 64.0), at(900.0, 64.0)],
            Stroke::new(1.0, Color32::from_gray(15)),
        );
        p.text(
            at(28.0, 33.0),
            egui::Align2::LEFT_CENTER,
            "f0lock",
            FontId::new(26.0, egui::FontFamily::Name(GLOBAL_FONT.into())),
            Color32::from_black_alpha(136),
        );
        p.text(
            at(28.0, 31.0),
            egui::Align2::LEFT_CENTER,
            "f0lock",
            FontId::new(26.0, egui::FontFamily::Name(GLOBAL_FONT.into())),
            TEXT.gamma_multiply(0.85),
        );

        if ui
            .interact(
                Rect::from_min_max(at(24.0, 13.0), at(122.0, 51.0)),
                ui.make_persistent_id("about"),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            self.about_open = true;
        }

        level_bar(&p, at(701.0, 25.0), v[5]);
        level_bar(&p, at(701.0, 36.0), v[6]);

        self.spectrum(ui, Rect::from_min_max(at(20.0, 78.0), at(880.0, 278.0)), &v);
        self.processing_bar(ui, Rect::from_min_max(at(0.0, 282.0), at(900.0, 314.0)), &v);
        self.point_tabs(ui, Rect::from_min_max(at(0.0, 314.0), at(900.0, 346.0)));
        self.controls(ui, canvas, &v);
        self.settings_menu(ui, canvas, &v);

        self.show_notice(ui.ctx(), canvas.min);

        self.show_about(ui.ctx());
    }
}

impl NiceEguiApp for F0Editor {
    fn build(
        &mut self,
        ctx: egui::Context,
        gui: GuiContext,
        _: &mut nice_plug_egui::Frame,
    ) -> Result<(), nice_plug_egui::baseview::HandlerError> {
        configure_style(&ctx);
        self.restore_ui_scale(&ctx)?;

        #[cfg(all(feature = "dpi-trace", target_os = "windows"))]
        nice_plug_egui::diagnostics::event(
            "f0_build",
            format_args!(
                "api={:?} saved_percent={} zoom={}",
                gui.plugin_api(),
                self.params
                    .ui_scale_percent
                    .load(std::sync::atomic::Ordering::Relaxed),
                ctx.zoom_factor()
            ),
        );

        self.gui = Some(gui);
        Ok(())
    }

    fn editor_closed(&mut self) {
        self.gui = None;
        self.context_point = None;
        self.about_open = false;
        self.audio_open = false;
    }

    fn ui(&mut self, ui: &mut egui::Ui, _: &mut nice_plug_egui::Frame) {
        self.draw(ui);
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }
}

pub(crate) fn configure_style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        GLOBAL_FONT.to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Ubuntu-Bold.ttf"
        ))),
    );
    fonts.families.insert(
        egui::FontFamily::Name(GLOBAL_FONT.into()),
        vec![GLOBAL_FONT.to_owned()],
    );
    fonts.font_data.insert(
        "credits-cjk".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/NotoSansSC-Credits.ttf"
        ))),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .get_mut(&family)
            .unwrap()
            .push("credits-cjk".into());
    }
    ctx.set_fonts(fonts);

    let mut style = egui::Style {
        visuals: egui::Visuals::dark(),
        ..Default::default()
    };
    style.visuals.panel_fill = Color32::from_gray(32);
    style.visuals.override_text_color = Some(TEXT);
    ctx.set_style_of(egui::Theme::Dark, style);
    ctx.set_theme(egui::Theme::Dark);
}

fn set_value<P: Param>(setter: &ParamSetter<'_>, param: &P, value: P::Plain) {
    setter.begin_set_parameter(param);
    setter.set_parameter(param, value);
    setter.end_set_parameter(param);
}

fn note_name(hz: f32) -> String {
    let nearest = (69.0 + 12.0 * (hz / 440.0).log2()).round() as i32;
    let names = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        names[nearest.rem_euclid(12) as usize],
        nearest.div_euclid(12) - 1
    )
}

fn level_bar(p: &egui::Painter, origin: Pos2, peak: f32) {
    let rect = Rect::from_min_size(origin, Vec2::new(120.0, 4.0));
    p.rect_filled(rect, 0.0, Color32::from_gray(26));
    let fraction = if peak > 0.0 {
        ((20.0 * peak.log10() + 60.0) / 66.0).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let fill = Rect::from_min_size(rect.min, Vec2::new(rect.width() * fraction, rect.height()));
    p.rect_filled(
        fill,
        0.0,
        if peak > 1.0 {
            RED
        } else {
            Color32::from_gray(155)
        },
    );
}
