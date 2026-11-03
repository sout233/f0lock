use super::*;
use crate::{lookahead::LookaheadTime, midi::PitchSource, mode::OperatingMode};

impl F0Editor {
    pub(super) fn processing_bar(&mut self, ui: &mut egui::Ui, rect: Rect, values: &[f32; 10]) {
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

        // Audition control: a click starts or stops it, a sideways drag sets how
        // wide the listened window is. The width stays visible so it can be
        // preset without switching the audition on.
        let listen_on = self.listen;
        let listen_id = ui.make_persistent_id("listen-band");
        let listen_rect = Rect::from_min_size(
            rect.right_top() + Vec2::new(-268.0, 4.0),
            Vec2::new(116.0, 24.0),
        );
        let listen = ui
            .interact(listen_rect, listen_id, Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
            .on_hover_text("Click to audition the selected point, drag to change the width");

        let mut half_width = self.audition.half_width();
        if listen.dragged() {
            // `drag_delta` is this frame's movement, so the width follows the
            // pointer directly instead of tracking a drag origin.
            let [low, high] = crate::engine::LISTEN_HALF_WIDTH_RANGE;
            half_width = (half_width + listen.drag_delta().x * 2.0).clamp(low, high);
            self.audition.set_half_width(half_width);
        }

        painter.rect_filled(
            listen_rect,
            4.0,
            if listen_on {
                accent.gamma_multiply(0.16)
            } else {
                Color32::from_gray(40)
            },
        );
        painter.rect_stroke(
            listen_rect,
            4.0,
            Stroke::new(
                1.0,
                if listen_on {
                    accent.gamma_multiply(0.7)
                } else {
                    Color32::from_gray(73)
                },
            ),
            egui::StrokeKind::Inside,
        );
        painter.text(
            listen_rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("Listen ±{half_width:.0} Hz"),
            FontId::proportional(12.0),
            if listen_on { TEXT } else { MUTED },
        );

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

        let source = self.params.pitch_source.value();
        let button = strip_button(
            ui,
            button_rect(360.0),
            &format!("Pitch: {}", source.label()),
            accent,
            false,
            true,
        );
        egui::Popup::menu(&button).width(160.0).show(|ui| {
            for choice in PitchSource::ALL {
                if ui
                    .selectable_label(source == choice, choice.label())
                    .clicked()
                {
                    set_value(&setter, &self.params.pitch_source, choice);
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

        if listen.clicked() {
            self.listen = !listen_on;
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
