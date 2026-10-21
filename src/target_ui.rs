use super::*;
use crate::pitch::TargetMode;

const TARGET_COLOR: Color32 = Color32::from_rgb(95, 158, 184);

impl F0Editor {
    pub(super) fn target_controls(&self, ui: &mut egui::Ui, plot: Rect, max_hz: f32) {
        let Some(gui) = &self.gui else { return };
        let setter = gui.param_setter();
        let target = self.params.target();
        let (minimum, maximum) = target.bounds();
        let x = |hz: f32| plot.left() + (hz / 20.0).ln() / (max_hz / 20.0).ln() * plot.width();
        let hz = |x: f32| 20.0 * (max_hz / 20.0).powf((x - plot.left()) / plot.width());
        let p = ui.painter_at(plot);

        if target.mode == TargetMode::Range && minimum > maximum {
            p.text(
                plot.right_top() + Vec2::new(-6.0, 34.0),
                egui::Align2::RIGHT_TOP,
                "Invalid target range",
                FontId::proportional(12.0),
                RED,
            );
        }

        if target.mode != TargetMode::Auto && minimum <= maximum {
            let band = Rect::from_min_max(
                Pos2::new(x(minimum), plot.top()),
                Pos2::new(x(maximum), plot.bottom()),
            );
            p.rect_filled(band, 0.0, TARGET_COLOR.gamma_multiply(0.12));

            for (edge, frequency) in [("minimum", minimum), ("maximum", maximum)] {
                let position = x(frequency);
                let handle = Rect::from_min_max(
                    Pos2::new(position - 5.0, plot.top() + 32.0),
                    Pos2::new(position + 5.0, plot.bottom()),
                );
                let response = ui
                    .interact(
                        handle,
                        ui.make_persistent_id(("target-edge", edge)),
                        Sense::drag(),
                    )
                    .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                let param = match target.mode {
                    TargetMode::Range if edge == "minimum" => &self.params.target_min,
                    TargetMode::Range => &self.params.target_max,
                    TargetMode::Reference => &self.params.target_tolerance,
                    TargetMode::Auto => unreachable!(),
                };

                if response.drag_started() {
                    setter.begin_set_parameter(param);
                }

                if response.dragged() {
                    let pointer = response
                        .interact_pointer_pos()
                        .expect("Target drag without pointer");
                    let frequency = hz(pointer.x).clamp(50.0, 1000.0);
                    let value = if target.mode == TargetMode::Reference {
                        (12.0 * (frequency / target.reference).log2())
                            .abs()
                            .clamp(1.0, 24.0)
                    } else {
                        frequency
                    };
                    setter.set_parameter(param, value);
                }

                if response.drag_stopped() {
                    setter.end_set_parameter(param);
                }

                p.line_segment(
                    [
                        Pos2::new(position, plot.top()),
                        Pos2::new(position, plot.bottom()),
                    ],
                    Stroke::new(
                        if response.hovered() || response.dragged() {
                            2.0
                        } else {
                            1.0
                        },
                        TARGET_COLOR,
                    ),
                );
                p.text(
                    Pos2::new(position, plot.bottom() - 5.0),
                    egui::Align2::CENTER_BOTTOM,
                    format!("{frequency:.0} Hz"),
                    FontId::monospace(10.0),
                    TARGET_COLOR,
                );
            }

            if target.mode == TargetMode::Reference {
                let position = x(target.reference);
                let response = ui
                    .interact(
                        Rect::from_min_max(
                            Pos2::new(position - 5.0, plot.top() + 32.0),
                            Pos2::new(position + 5.0, plot.bottom() - 20.0),
                        ),
                        ui.make_persistent_id("target-reference"),
                        Sense::drag(),
                    )
                    .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);

                if response.drag_started() {
                    setter.begin_set_parameter(&self.params.target_reference);
                }

                if response.dragged() {
                    let pointer = response
                        .interact_pointer_pos()
                        .expect("Reference drag without pointer");
                    setter.set_parameter(
                        &self.params.target_reference,
                        hz(pointer.x).clamp(50.0, 1000.0),
                    );
                }

                if response.drag_stopped() {
                    setter.end_set_parameter(&self.params.target_reference);
                }

                p.line_segment(
                    [
                        Pos2::new(position, plot.top()),
                        Pos2::new(position, plot.bottom()),
                    ],
                    Stroke::new(2.0, TARGET_COLOR),
                );
            }
        }

        ui.scope_builder(
            egui::UiBuilder::new().max_rect(Rect::from_min_size(
                Pos2::new(plot.right() - 139.0, plot.top() + 6.0),
                Vec2::new(133.0, 25.0),
            )),
            |ui| {
                let button = ui.add_sized(
                    Vec2::new(133.0, 22.0),
                    egui::Button::new(format!("Target: {}", target.mode.label())),
                );
                egui::Popup::menu(&button)
                    .align(egui::emath::RectAlign::BOTTOM_END)
                    .width(260.0)
                    .show(|ui| {
                        ui.horizontal(|ui| {
                            for mode in TargetMode::ALL {
                                if ui
                                    .selectable_label(target.mode == mode, mode.label())
                                    .clicked()
                                {
                                    set_value(&setter, &self.params.target_mode, mode);
                                }
                            }
                        });

                        let mode = self.params.target_mode.value();
                        if mode == TargetMode::Auto {
                            return;
                        }

                        ui.separator();
                        egui::Grid::new("pitch-target-values")
                            .spacing([14.0, 8.0])
                            .show(ui, |ui| {
                                let fields = if mode == TargetMode::Range {
                                    [
                                        (&self.params.target_min, "Minimum"),
                                        (&self.params.target_max, "Maximum"),
                                    ]
                                } else {
                                    [
                                        (&self.params.target_reference, "Reference Hz"),
                                        (&self.params.target_tolerance, "Tolerance ±"),
                                    ]
                                };

                                for (param, label) in fields {
                                    ui.label(label);
                                    target_value(ui, param, &setter);
                                    ui.end_row();
                                }
                            });

                        if mode == TargetMode::Range
                            && self.params.target_min.value() > self.params.target_max.value()
                        {
                            ui.colored_label(RED, "Minimum exceeds maximum");
                        }
                    });
            },
        );
    }
}

fn target_value(ui: &mut egui::Ui, param: &FloatParam, setter: &ParamSetter<'_>) {
    let mut value = param.value();
    let response = ui.add(
        egui::DragValue::new(&mut value)
            .range(param.preview_plain(0.0)..=param.preview_plain(1.0))
            .speed(if param.unit() == " Hz" { 1.0 } else { 0.1 })
            .max_decimals(2)
            .suffix(param.unit()),
    );

    if response.drag_started() {
        setter.begin_set_parameter(param);
    }

    if response.changed() {
        if response.dragged() {
            setter.set_parameter(param, value);
        } else {
            set_value(setter, param, value);
        }
    }

    if response.drag_stopped() {
        setter.end_set_parameter(param);
    }
}
