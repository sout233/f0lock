use super::*;
use crate::mode::OperatingMode;

impl F0Editor {
    pub(super) fn lookahead_button(&self, ui: &mut egui::Ui, canvas: Rect, values: &[f32; 10]) {
        let Some(gui) = &self.gui else { return };
        let enabled = self.params.lookahead.value();
        let latency = self
            .telemetry
            .latency_samples
            .load(std::sync::atomic::Ordering::Relaxed);
        let state = if !enabled {
            "Off".to_owned()
        } else if latency > 0 && values[7] > 0.0 {
            format!("{:.0} ms", latency as f32 * 1000.0 / values[7])
        } else {
            "On".to_owned()
        };

        ui.scope_builder(
            egui::UiBuilder::new().max_rect(Rect::from_min_size(
                canvas.min + Vec2::new(326.0, 20.0),
                Vec2::new(184.0, 25.0),
            )),
            |ui| {
                let button = egui::Button::new(
                    egui::RichText::new(format!("Lookahead · {state}")).size(12.0),
                )
                .fill(if enabled {
                    self.accent().gamma_multiply(0.18)
                } else {
                    Color32::from_gray(43)
                })
                .stroke(Stroke::new(
                    1.0,
                    if enabled {
                        self.accent()
                    } else {
                        Color32::from_gray(78)
                    },
                ));

                if ui.add_sized(Vec2::new(184.0, 24.0), button).clicked() {
                    set_value(&gui.param_setter(), &self.params.lookahead, !enabled);
                }
            },
        );
    }

    pub(super) fn mode_button(&self, ui: &mut egui::Ui, canvas: Rect) {
        let Some(gui) = &self.gui else { return };
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(Rect::from_min_size(
                canvas.min + Vec2::new(162.0, 20.0),
                Vec2::new(144.0, 25.0),
            )),
            |ui| {
                let mode = self.params.mode.value();
                let button = ui.add_sized(Vec2::new(144.0, 24.0), egui::Button::new(mode.label()));
                egui::Popup::menu(&button).width(144.0).show(|ui| {
                    for choice in OperatingMode::ALL {
                        if ui
                            .selectable_label(mode == choice, choice.label())
                            .clicked()
                        {
                            set_value(&gui.param_setter(), &self.params.mode, choice);
                            egui::Popup::close_all(ui.ctx());
                        }
                    }
                });
            },
        );
    }
}
