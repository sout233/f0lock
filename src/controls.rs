use super::*;

impl F0Editor {
    pub(super) fn controls(&self, ui: &mut egui::Ui, canvas: Rect, v: &[f32; 10]) {
        let at = |x, y| canvas.min + Vec2::new(x, y);
        let p = ui.painter_at(canvas);
        gradient_rect(
            &p,
            Rect::from_min_max(at(0.0, 346.0), at(900.0, 503.0)),
            Color32::from_gray(213),
            Color32::from_gray(201),
        );
        gradient_rect(
            &p,
            Rect::from_min_max(at(0.0, 503.0), at(900.0, 660.0)),
            Color32::from_gray(200),
            Color32::from_gray(190),
        );

        for (y, color, width) in [
            (346.0, Color32::from_gray(19), 2.0),
            (348.0, Color32::from_gray(243), 1.0),
            (503.0, Color32::from_gray(151), 1.0),
            (504.0, Color32::from_gray(228), 1.0),
        ] {
            p.line_segment([at(0.0, y), at(900.0, y)], Stroke::new(width, color));
        }

        for x in [225.0, 450.0, 675.0] {
            p.line_segment(
                [at(x, 349.0), at(x, 502.0)],
                Stroke::new(1.0, Color32::from_gray(166)),
            );
            p.line_segment(
                [at(x + 1.0, 349.0), at(x + 1.0, 502.0)],
                Stroke::new(1.0, Color32::from_gray(232)),
            );
        }
        p.text(
            at(0.0, 503.0),
            egui::Align2::LEFT_TOP,
            "GLOBAL",
            FontId::new(48.0, egui::FontFamily::Name(GLOBAL_FONT.into())),
            Color32::from_rgba_unmultiplied(91, 91, 91, 26),
        );

        let Some(gui) = &self.gui else { return };
        let setter = gui.param_setter();

        let band = self.params.band(self.selected.unwrap_or(0));

        for (param, label, x, y, bipolar, enabled, color) in [
            (
                band.gain,
                "Gain",
                112.5,
                424.5,
                true,
                self.selected.is_some() && band.kind.value().has_gain(),
                self.accent(),
            ),
            (
                band.q,
                "Q",
                337.5,
                424.5,
                false,
                self.selected.is_some(),
                self.accent(),
            ),
            (
                band.ratio,
                "F0 Ratio",
                562.5,
                424.5,
                false,
                self.selected.is_some(),
                self.accent(),
            ),
            (
                &self.params.tracking,
                "Tracking",
                90.0,
                583.0,
                false,
                true,
                GLOBAL_ACCENT,
            ),
            (
                &self.params.gate,
                "Gate",
                270.0,
                583.0,
                false,
                true,
                GLOBAL_ACCENT,
            ),
            (
                &self.params.clarity,
                "Clarity",
                450.0,
                583.0,
                false,
                true,
                GLOBAL_ACCENT,
            ),
            (
                &self.params.mix,
                "Mix",
                630.0,
                583.0,
                false,
                true,
                GLOBAL_ACCENT,
            ),
            (
                &self.params.output,
                "Output",
                810.0,
                583.0,
                true,
                true,
                GLOBAL_ACCENT,
            ),
        ] {
            let builder = egui::UiBuilder::new()
                .max_rect(Rect::from_center_size(at(x, y), Vec2::splat(140.0)));
            let builder = if enabled { builder } else { builder.disabled() };
            ui.scope_builder(builder, |ui| {
                ui.visuals_mut().selection.bg_fill = color.gamma_multiply(0.3);
                ui.visuals_mut().selection.stroke = Stroke::new(1.0, color);
                knob::knob(ui, param, &setter, at(x, y), 35.0, label, bipolar);
            });
        }
        p.text(
            at(787.5, 361.5),
            egui::Align2::CENTER_CENTER,
            "Type",
            FontId::proportional(15.0),
            Color32::from_gray(58),
        );

        let Some(index) = self.selected else {
            p.text(
                at(787.5, 424.5),
                egui::Align2::CENTER_CENTER,
                "No point",
                FontId::proportional(14.0),
                Color32::from_gray(91),
            );
            return;
        };

        let band = self.params.band(index);
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(Rect::from_min_max(at(703.5, 411.0), at(872.5, 494.0))),
            |ui| {
                *ui.visuals_mut() = egui::Visuals::light();
                ui.visuals_mut().selection.bg_fill = self.accent().gamma_multiply(0.25);
                ui.visuals_mut().selection.stroke = Stroke::new(1.0, self.accent());

                let button = ui
                    .push_id("point-type", |ui| {
                        ui.add_sized(
                            Vec2::new(168.0, 26.0),
                            egui::Button::new(band.kind.value().label()),
                        )
                    })
                    .inner;
                let arrow = button.rect.right_center() - Vec2::new(13.0, 0.0);
                ui.painter().add(egui::Shape::convex_polygon(
                    vec![
                        arrow + Vec2::new(-3.5, -2.0),
                        arrow + Vec2::new(3.5, -2.0),
                        arrow + Vec2::new(0.0, 2.5),
                    ],
                    Color32::from_gray(82),
                    Stroke::NONE,
                ));

                egui::Popup::menu(&button)
                    .align(egui::emath::RectAlign::TOP_START)
                    .align_alternatives(&[egui::emath::RectAlign::TOP_END])
                    .width(button.rect.width())
                    .show(|ui| {
                        for kind in FilterKind::ALL {
                            if ui
                                .selectable_label(band.kind.value() == kind, kind.label())
                                .clicked()
                            {
                                set_value(&setter, band.kind, kind);
                                egui::Popup::close_all(ui.ctx());
                            }
                        }
                    });
            },
        );

        if v[7] > 0.0 && v[2] > 0.0 {
            p.text(
                at(787.5, 472.0),
                egui::Align2::CENTER_CENTER,
                format!("{:.1} Hz", (v[2] * band.ratio.value()).min(v[7] * 0.49)),
                FontId::monospace(14.0),
                Color32::from_gray(73),
            );
        }
    }
}
