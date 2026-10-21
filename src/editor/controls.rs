use super::*;

impl F0Editor {
    pub(super) fn controls(&self, ui: &mut egui::Ui, canvas: Rect, v: &[f32; 10]) {
        let at = |x, y| canvas.min + Vec2::new(x, y);
        let p = ui.painter_at(canvas);
        gradient_rect(
            &p,
            Rect::from_min_max(at(0.0, 346.0), at(900.0, 551.0)),
            Color32::from_gray(213),
            Color32::from_gray(201),
        );
        gradient_rect(
            &p,
            Rect::from_min_max(at(0.0, 551.0), at(900.0, 724.0)),
            Color32::from_gray(200),
            Color32::from_gray(190),
        );

        for (y, color, width) in [
            (346.0, Color32::from_gray(19), 2.0),
            (348.0, Color32::from_gray(243), 1.0),
            (551.0, Color32::from_gray(151), 1.0),
            (552.0, Color32::from_gray(228), 1.0),
        ] {
            p.line_segment([at(0.0, y), at(900.0, y)], Stroke::new(width, color));
        }

        for (x, top, bottom) in [
            (350.0, 362.0, 542.0),
            (350.0, 568.0, 692.0),
            (630.0, 568.0, 692.0),
        ] {
            p.line_segment(
                [at(x, top), at(x, bottom)],
                Stroke::new(1.0, Color32::from_gray(172)),
            );
            p.line_segment(
                [at(x + 1.0, top), at(x + 1.0, bottom)],
                Stroke::new(1.0, Color32::from_gray(228)),
            );
        }

        p.text(
            at(0.0, 551.0),
            egui::Align2::LEFT_TOP,
            "GLOBAL",
            FontId::new(48.0, egui::FontFamily::Name(GLOBAL_FONT.into())),
            Color32::from_rgba_unmultiplied(91, 91, 91, 26),
        );
        for (x, label) in [(175.0, "DETECTION"), (490.0, "ENVELOPE"), (770.0, "OUTPUT")] {
            p.text(
                at(x, 566.0),
                egui::Align2::CENTER_CENTER,
                label,
                FontId::proportional(11.0),
                Color32::from_gray(112),
            );
        }
        p.text(
            at(20.0, 373.0),
            egui::Align2::LEFT_CENTER,
            "EQ",
            FontId::proportional(12.0),
            Color32::from_gray(91),
        );

        let Some(gui) = &self.gui else { return };
        let setter = gui.param_setter();
        let band = self.params.band(self.selected.unwrap_or(0));
        let selected = self.selected.is_some();
        let supported = selected && band.kind.value().has_gain();
        let dynamic_color = if supported && band.dynamics.enabled.value() {
            self.accent()
        } else {
            GLOBAL_ACCENT
        };

        // EQ and dynamics stay in fixed positions, independently of enable state.
        for (param, label, x, y, bipolar, enabled, color) in [
            (
                band.gain,
                "Gain",
                65.0,
                465.0,
                true,
                supported,
                self.accent(),
            ),
            (band.q, "Q", 175.0, 465.0, false, selected, self.accent()),
            (
                band.ratio,
                "F0 Ratio",
                285.0,
                465.0,
                false,
                selected,
                self.accent(),
            ),
            (
                &band.dynamics.threshold,
                "Threshold",
                420.0,
                465.0,
                false,
                supported,
                dynamic_color,
            ),
            (
                &band.dynamics.range,
                "Range",
                550.0,
                465.0,
                false,
                supported,
                dynamic_color,
            ),
            (
                &band.dynamics.attack,
                "Dyn.Attack",
                680.0,
                465.0,
                false,
                supported,
                dynamic_color,
            ),
            (
                &band.dynamics.release,
                "Dyn.Release",
                810.0,
                465.0,
                false,
                supported,
                dynamic_color,
            ),
            (
                &self.params.active().tracking,
                "Tracking",
                65.0,
                642.0,
                false,
                true,
                GLOBAL_ACCENT,
            ),
            (
                &self.params.active().gate,
                "Gate",
                175.0,
                642.0,
                false,
                true,
                GLOBAL_ACCENT,
            ),
            (
                &self.params.active().clarity,
                "Clarity",
                285.0,
                642.0,
                false,
                true,
                GLOBAL_ACCENT,
            ),
            (
                &self.params.active().attack,
                "Attack",
                435.0,
                642.0,
                false,
                true,
                GLOBAL_ACCENT,
            ),
            (
                &self.params.active().release,
                "Release",
                545.0,
                642.0,
                false,
                true,
                GLOBAL_ACCENT,
            ),
            (
                &self.params.active().mix,
                "Mix",
                715.0,
                642.0,
                false,
                true,
                GLOBAL_ACCENT,
            ),
            (
                &self.params.active().output,
                "Output",
                825.0,
                642.0,
                true,
                true,
                GLOBAL_ACCENT,
            ),
        ] {
            let builder = egui::UiBuilder::new()
                .max_rect(Rect::from_center_size(at(x, y), Vec2::splat(110.0)));
            let builder = if enabled { builder } else { builder.disabled() };
            ui.scope_builder(builder, |ui| {
                ui.visuals_mut().selection.bg_fill = color.gamma_multiply(0.3);
                ui.visuals_mut().selection.stroke = Stroke::new(1.0, color);
                knob::knob(ui, param, &setter, at(x, y), 35.0 * 0.85, label, bipolar);
            });
        }

        let accent = self.accent();
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(Rect::from_min_max(at(60.0, 362.0), at(200.0, 384.0))),
            |ui| {
                *ui.visuals_mut() = egui::Visuals::light();
                ui.visuals_mut().selection.bg_fill = accent.gamma_multiply(0.25);
                ui.visuals_mut().selection.stroke = Stroke::new(1.0, accent);
                let button = ui
                    .add_enabled_ui(selected, |ui| {
                        ui.push_id("point-type", |ui| {
                            ui.add_sized(
                                Vec2::new(140.0, 22.0),
                                egui::Button::new(band.kind.value().label()),
                            )
                        })
                        .inner
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

        ui.scope_builder(
            egui::UiBuilder::new().max_rect(Rect::from_min_max(at(375.0, 362.0), at(495.0, 384.0))),
            |ui| {
                ui.add_enabled_ui(supported, |ui| {
                    let response = ui
                        .interact(
                            Rect::from_min_max(at(375.0, 362.0), at(495.0, 384.0)),
                            ui.make_persistent_id("dynamics-switch"),
                            Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    let lit = supported && band.dynamics.enabled.value();
                    let lamp = at(383.0, 373.0);
                    let painter = ui.painter();

                    if lit {
                        painter.circle_filled(lamp, 9.0, accent.gamma_multiply(0.07));
                        painter.circle_filled(lamp, 7.5, accent.gamma_multiply(0.14));
                    }
                    painter.circle_filled(lamp + Vec2::new(0.0, 0.7), 6.5, Color32::from_gray(130));
                    painter.circle_filled(lamp, 6.0, Color32::from_gray(57));
                    painter.circle_stroke(lamp, 6.0, Stroke::new(0.8, Color32::from_gray(237)));
                    painter.circle_filled(
                        lamp,
                        4.3,
                        if lit { accent } else { Color32::from_gray(110) },
                    );
                    painter.text(
                        at(397.0, 373.0),
                        egui::Align2::LEFT_CENTER,
                        "Dynamics",
                        FontId::proportional(12.0),
                        Color32::from_gray(if !supported {
                            151
                        } else if response.hovered() {
                            38
                        } else {
                            75
                        }),
                    );

                    if response.clicked() {
                        set_value(
                            &setter,
                            &band.dynamics.enabled,
                            !band.dynamics.enabled.value(),
                        );
                    }
                })
                .inner;
            },
        );

        if selected && v[7] > 0.0 && v[2] > 0.0 {
            p.text(
                at(285.0, 373.0),
                egui::Align2::CENTER_CENTER,
                format!("{:.1} Hz", (v[2] * band.ratio.value()).min(v[7] * 0.49)),
                FontId::monospace(12.0),
                Color32::from_gray(91),
            );
        }
        if let Some(index) = self.selected {
            if supported && band.dynamics.enabled.value() {
                let reduction = self.telemetry.bands()[index][9];
                p.text(
                    at(850.0, 373.0),
                    egui::Align2::RIGHT_CENTER,
                    format!("{reduction:.2} dB"),
                    FontId::monospace(12.0),
                    Color32::from_gray(91),
                );
            }
        }
    }
}
