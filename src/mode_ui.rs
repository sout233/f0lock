use super::*;
use crate::mode::OperatingMode;

impl F0Editor {
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
