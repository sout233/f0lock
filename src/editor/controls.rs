use super::*;
use crate::filter::CutSlope;

impl F0Editor {
    pub(super) fn controls(&mut self, ui: &mut egui::Ui, canvas: Rect, v: &[f32; 10]) {
        let at = |x, y| canvas.min + Vec2::new(x, y);
        let p = ui.painter_at(canvas);

        // The EQ column divider moves with the strip: 350 px is the original
        // width (three knobs), 450 px is the widened one that fits the fourth
        // knob when Q Follow is expanded. Everything in the header row is
        // pinned to it, so it has to be resolved before anything is painted.
        let index = self.selected.unwrap_or(0);
        let expanded = self.q_follow_expanded(index);
        let divider_x = if expanded { 450.0 } else { 350.0 };
        let header_x = divider_x + 25.0;
        let readout_right = divider_x - 10.0;
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
            (divider_x, 362.0, 542.0),
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
        let band = self.params.band(index);
        let selected = self.selected.is_some();
        let by_channel = self.params.pitch_source.value() == crate::midi::PitchSource::Midi
            && self.params.midi_mode.value() == crate::midi::MidiMode::ByChannel;
        let supported = selected && band.kind.value().has_gain();
        let dynamic_color = if supported && band.dynamics.enabled.value() {
            self.accent()
        } else {
            GLOBAL_ACCENT
        };

        // Collapsed keeps the pre-Q-Follow layout and knob size; expanded makes
        // room for the Q Follow knob and tightens the row to fit it.
        let eq_radius = if expanded { 27.0 } else { 35.0 * 0.85 };
        let eq_x: [f32; 4] = if expanded {
            [55.0, 165.0, 275.0, 385.0]
        } else {
            [65.0, 175.0, 285.0, 0.0]
        };
        let dyn_x: [f32; 4] = if expanded {
            [505.0, 615.0, 725.0, 835.0]
        } else {
            [420.0, 550.0, 680.0, 810.0]
        };

        let q_enabled =
            selected && !(band.kind.value().is_cut() && band.slope.value() == CutSlope::Db6);
        let mut knobs: Vec<(&FloatParam, &'static str, f32, f32, bool, bool, Color32)> = vec![
            (
                band.gain,
                "Gain",
                eq_x[0],
                465.0,
                true,
                supported,
                self.accent(),
            ),
            (band.q, "Q", eq_x[1], 465.0, false, q_enabled, self.accent()),
            (
                band.ratio,
                "F0 Ratio",
                if expanded { eq_x[3] } else { eq_x[2] },
                465.0,
                false,
                selected,
                self.accent(),
            ),
            (
                &band.dynamics.threshold,
                "Threshold",
                dyn_x[0],
                465.0,
                false,
                supported,
                dynamic_color,
            ),
            (
                &band.dynamics.range,
                "Range",
                dyn_x[1],
                465.0,
                false,
                supported,
                dynamic_color,
            ),
            (
                &band.dynamics.attack,
                "Dyn.Attack",
                dyn_x[2],
                465.0,
                false,
                supported,
                dynamic_color,
            ),
            (
                &band.dynamics.release,
                "Dyn.Release",
                dyn_x[3],
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
                !self.params.active().infinite_release.value(),
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
        ];

        if expanded {
            knobs.insert(
                2,
                (
                    band.q_follow,
                    "Q Follow",
                    eq_x[2],
                    465.0,
                    true,
                    q_enabled,
                    self.accent(),
                ),
            );
        }

        // EQ and dynamics stay in fixed positions, independently of enable state.
        for (param, label, x, y, bipolar, enabled, color) in knobs {
            let builder = egui::UiBuilder::new()
                .max_rect(Rect::from_center_size(at(x, y), Vec2::splat(110.0)));
            let builder = if enabled { builder } else { builder.disabled() };
            ui.scope_builder(builder, |ui| {
                ui.visuals_mut().selection.bg_fill = color.gamma_multiply(0.3);
                ui.visuals_mut().selection.stroke = Stroke::new(1.0, color);
                let radius = if y < 551.0 { eq_radius } else { 35.0 * 0.85 };
                let response = knob::knob(ui, param, &setter, at(x, y), radius, label, bipolar);
                if label == "Q Follow" {
                    response.on_hover_text("Q = 220 Hz × (pitch / 220 Hz)^Follow");
                }
            });
        }

        // Anchor to Q's outer ring in both layouts, rather than the column gap.
        let toggle = at(eq_x[1] + 36.0, 422.0);
        let response = ui
            .interact(
                Rect::from_center_size(toggle, Vec2::splat(24.0)),
                ui.make_persistent_id("q-follow-toggle"),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(if expanded {
                "Q Follow: hide the pitch-following Q knob"
            } else {
                "Q Follow: show the pitch-following Q knob"
            });
        let color = if expanded {
            self.accent()
        } else {
            Color32::from_gray(if response.hovered() { 65 } else { 112 })
        };
        if response.hovered() || response.is_pointer_button_down_on() {
            p.circle_filled(
                toggle,
                9.0,
                color.gamma_multiply(if response.is_pointer_button_down_on() {
                    0.16
                } else {
                    0.08
                }),
            );
        }
        p.circle_stroke(toggle, 9.0, Stroke::new(1.0, color.gamma_multiply(0.65)));
        p.line_segment(
            [toggle - Vec2::new(3.5, 0.0), toggle + Vec2::new(3.5, 0.0)],
            Stroke::new(1.3, color),
        );
        if !expanded {
            p.line_segment(
                [toggle - Vec2::new(0.0, 3.5), toggle + Vec2::new(0.0, 3.5)],
                Stroke::new(1.3, color),
            );
        }
        if response.clicked() {
            self.q_follow_open[index] = Some(!expanded);
            ui.ctx().request_repaint();
        }

        let infinite_release = &self.params.active().infinite_release;
        let center = at(596.0, 589.0);
        let response = ui
            .interact(
                Rect::from_center_size(center, Vec2::splat(24.0)),
                ui.make_persistent_id("infinite-release"),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Infinite Release: hold EQ when pitch is lost");
        let color = if infinite_release.value() {
            self.accent()
        } else {
            Color32::from_gray(if response.hovered() { 65 } else { 112 })
        };
        p.circle_stroke(center, 10.0, Stroke::new(1.0, color.gamma_multiply(0.65)));
        let points = (0..=64)
            .map(|i| {
                let t = std::f32::consts::TAU * i as f32 / 64.0;
                center + Vec2::new(6.2 * t.sin(), 3.0 * (2.0 * t).sin())
            })
            .collect();
        p.add(egui::Shape::line(points, Stroke::new(1.5, color)));
        if response.clicked() {
            set_value(&setter, infinite_release, !infinite_release.value());
        }

        let accent = self.accent();
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(Rect::from_min_max(at(52.0, 362.0), at(152.0, 384.0))),
            |ui| {
                *ui.visuals_mut() = egui::Visuals::light();
                ui.visuals_mut().selection.bg_fill = accent.gamma_multiply(0.25);
                ui.visuals_mut().selection.stroke = Stroke::new(1.0, accent);
                let button = ui
                    .add_enabled_ui(selected, |ui| {
                        ui.push_id("point-type", |ui| {
                            ui.add_sized(
                                Vec2::new(100.0, 22.0),
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

        if band.kind.value().is_cut() {
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_max(at(160.0, 362.0), at(252.0, 384.0))),
                |ui| {
                    *ui.visuals_mut() = egui::Visuals::light();
                    ui.spacing_mut().interact_size.y = 22.0;
                    ui.visuals_mut().selection.bg_fill = accent.gamma_multiply(0.25);
                    ui.visuals_mut().selection.stroke = Stroke::new(1.0, accent);
                    ui.add_enabled_ui(selected, |ui| {
                        egui::ComboBox::from_id_salt("cut-slope")
                            .width(92.0)
                            .height(240.0)
                            .selected_text(band.slope.value().label())
                            .show_ui(ui, |ui| {
                                for slope in CutSlope::ALL {
                                    if ui
                                        .selectable_label(
                                            band.slope.value() == slope,
                                            slope.label(),
                                        )
                                        .clicked()
                                    {
                                        set_value(&setter, band.slope, slope);
                                    }
                                }
                            });
                    });
                },
            );
        }

        if by_channel {
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_max(at(260.0, 362.0), at(340.0, 384.0))),
                |ui| {
                    *ui.visuals_mut() = egui::Visuals::light();
                    ui.visuals_mut().selection.bg_fill = accent.gamma_multiply(0.25);
                    ui.visuals_mut().selection.stroke = Stroke::new(1.0, accent);
                    let button = ui
                        .add_enabled_ui(selected, |ui| {
                            ui.add_sized(
                                Vec2::new(80.0, 22.0),
                                egui::Button::new(format!("Ch {}  ▾", band.midi_channel.value())),
                            )
                        })
                        .inner
                        .on_hover_text("MIDI Channel for this point");

                    egui::Popup::menu(&button)
                        .align(egui::emath::RectAlign::TOP_START)
                        .width(80.0)
                        .show(|ui| {
                            egui::ScrollArea::vertical()
                                .max_height(220.0)
                                .show(ui, |ui| {
                                    for channel in 1..=16 {
                                        if ui
                                            .selectable_label(
                                                band.midi_channel.value() == channel,
                                                format!("Ch {channel}"),
                                            )
                                            .clicked()
                                        {
                                            set_value(&setter, band.midi_channel, channel);
                                            egui::Popup::close_all(ui.ctx());
                                        }
                                    }
                                });
                        });
                },
            );
        }

        ui.scope_builder(
            egui::UiBuilder::new().max_rect(Rect::from_min_max(
                at(header_x, 362.0),
                at(header_x + 120.0, 384.0),
            )),
            |ui| {
                ui.add_enabled_ui(supported, |ui| {
                    let response = ui
                        .interact(
                            Rect::from_min_max(at(header_x, 362.0), at(header_x + 120.0, 384.0)),
                            ui.make_persistent_id("dynamics-switch"),
                            Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    let lit = supported && band.dynamics.enabled.value();
                    let lamp = at(header_x + 8.0, 373.0);
                    let painter = ui.painter();
                    let (glow, core) = knob::light_colors(accent);

                    if lit {
                        for i in (1..=10).rev() {
                            let t = i as f32 / 10.0;
                            painter.circle_filled(
                                lamp,
                                6.5 + 10.0 * t,
                                glow.gamma_multiply(0.004 + 0.10 * (1.0 - t).powi(2)),
                            );
                        }
                    }
                    painter.circle_filled(lamp + Vec2::new(0.0, 0.7), 6.5, Color32::from_gray(130));
                    painter.circle_filled(lamp, 6.0, Color32::from_gray(57));
                    painter.circle_stroke(lamp, 6.0, Stroke::new(0.8, Color32::from_gray(237)));
                    painter.circle_filled(
                        lamp,
                        4.3,
                        if lit { core } else { Color32::from_gray(110) },
                    );
                    painter.text(
                        at(header_x + 22.0, 373.0),
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
            // Right-aligned against the EQ column divider: only the left edge
            // moves when the digit count changes, so "Hz" stays in place.
            p.text(
                at(readout_right, if by_channel { 397.0 } else { 373.0 }),
                egui::Align2::RIGHT_CENTER,
                format!(
                    "{:.1} Hz",
                    (self.point_pitch(self.selected.unwrap(), v[2]) * band.ratio.value())
                        .min(v[7] * 0.49)
                ),
                FontId::monospace(11.0),
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
