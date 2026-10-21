use super::*;
use crate::{lookahead::LookaheadTime, mode::OperatingMode};

impl F0Editor {
    pub(super) fn processing_bar(&self, ui: &mut egui::Ui, rect: Rect, values: &[f32; 10]) {
        let Some(gui) = &self.gui else { return };
        let setter = gui.param_setter();
        let painter = ui.painter_at(rect);
        gradient_rect(
            &painter,
            rect,
            Color32::from_gray(39),
            Color32::from_gray(30),
        );
        painter.line_segment(
            [rect.left_top(), rect.right_top()],
            Stroke::new(1.0, Color32::from_gray(58)),
        );
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            Stroke::new(1.0, Color32::from_gray(19)),
        );
        let button_rect =
            |x| Rect::from_min_size(rect.min + Vec2::new(x, 4.0), Vec2::new(160.0, 24.0));
        let accent = self.accent();
        let mode = self.params.mode.value();
        let button = strip_button(ui, button_rect(20.0), mode.label(), accent, false, true);

        egui::Popup::menu(&button).width(160.0).show(|ui| {
            for choice in OperatingMode::ALL {
                if ui
                    .selectable_label(mode == choice, choice.label())
                    .clicked()
                {
                    set_value(&setter, &self.params.mode, choice);
                    egui::Popup::close_all(ui.ctx());
                }
            }
        });

        let enabled = self.params.lookahead.value();
        let duration = self.params.lookahead_time.value();
        let latency = self
            .telemetry
            .latency_samples
            .load(std::sync::atomic::Ordering::Relaxed);
        let state = if !enabled {
            "Off".to_owned()
        } else if latency > 0 && values[7] > 0.0 {
            format!("{:.0} ms", latency as f32 * 1000.0 / values[7])
        } else {
            duration.label().to_owned()
        };
        let button = strip_button(
            ui,
            button_rect(190.0),
            &format!("Lookahead · {state}"),
            accent,
            enabled,
            true,
        );

        egui::Popup::menu(&button).width(160.0).show(|ui| {
            if ui.selectable_label(!enabled, "Off").clicked() {
                if enabled {
                    set_value(&setter, &self.params.lookahead, false);
                }
                egui::Popup::close_all(ui.ctx());
            }

            for choice in LookaheadTime::ALL {
                if ui
                    .selectable_label(enabled && duration == choice, choice.label())
                    .clicked()
                {
                    if duration != choice {
                        set_value(&setter, &self.params.lookahead_time, choice);
                    }
                    if !enabled {
                        set_value(&setter, &self.params.lookahead, true);
                    }
                    egui::Popup::close_all(ui.ctx());
                }
            }
        });

        let bypass = self.params.bypass.value();
        let button = strip_button(
            ui,
            Rect::from_min_size(
                rect.right_top() + Vec2::new(-140.0, 4.0),
                Vec2::new(120.0, 24.0),
            ),
            if bypass { "Bypass" } else { "Active" },
            accent,
            !bypass,
            false,
        );

        if button.clicked() {
            set_value(&setter, &self.params.bypass, !bypass);
        }
    }
}

fn strip_button(
    ui: &mut egui::Ui,
    rect: Rect,
    label: &str,
    accent: Color32,
    active: bool,
    dropdown: bool,
) -> egui::Response {
    let button = egui::Button::new(egui::RichText::new(label).size(12.0))
        .corner_radius(4.0)
        .fill(if active {
            accent.gamma_multiply(0.16)
        } else {
            Color32::from_gray(40)
        })
        .stroke(Stroke::new(
            1.0,
            if active {
                accent.gamma_multiply(0.7)
            } else {
                Color32::from_gray(73)
            },
        ));
    let response = ui
        .put(rect, button)
        .on_hover_cursor(egui::CursorIcon::PointingHand);

    if dropdown {
        let center = rect.right_center() - Vec2::new(11.0, 0.0);
        let color = if response.hovered() {
            Color32::WHITE
        } else {
            MUTED
        };
        ui.painter().line_segment(
            [center + Vec2::new(-2.5, -1.0), center + Vec2::new(0.0, 1.5)],
            Stroke::new(1.0, color),
        );
        ui.painter().line_segment(
            [center + Vec2::new(0.0, 1.5), center + Vec2::new(2.5, -1.0)],
            Stroke::new(1.0, color),
        );
    }

    response
}
