use super::*;
use crate::pitch::DetectionMode;
use std::sync::atomic::Ordering;

const UI_SCALES: [u32; 8] = [75, 90, 100, 110, 125, 150, 175, 200];

impl F0Editor {
    pub(super) fn restore_ui_scale(&self, ctx: &egui::Context) -> Result<(), std::io::Error> {
        let percent = self.params.ui_scale_percent.load(Ordering::Relaxed);

        if !UI_SCALES.contains(&percent) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid saved UI Scale: {percent}%"),
            ));
        }

        let scale = percent as f32 / 100.0;

        if (ctx.zoom_factor() - scale).abs() > 0.001 {
            ctx.set_zoom_factor(scale);
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(900.0, 684.0)));
        }

        Ok(())
    }

    pub(super) fn settings_menu(&mut self, ui: &mut egui::Ui, canvas: Rect, v: &[f32; 10]) {
        let center = canvas.min + Vec2::new(860.0, 32.0);
        let rect = Rect::from_center_size(center, Vec2::new(36.0, 32.0));
        let response = ui
            .interact(rect, ui.make_persistent_id("settings-menu"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);

        let color = if response.hovered() {
            Color32::WHITE
        } else {
            TEXT
        };

        for y in [-5.0, 0.0, 5.0] {
            ui.painter().line_segment(
                [center + Vec2::new(-7.0, y), center + Vec2::new(7.0, y)],
                Stroke::new(1.5, color),
            );
        }

        egui::Popup::menu(&response)
            .align(egui::emath::RectAlign::BOTTOM_END)
            .width(168.0)
            .show(|ui| {
                if ui.button("Audio Settings").clicked() {
                    self.audio_open = true;
                    egui::Popup::close_all(ui.ctx());
                }

                ui.menu_button("Pitch Detection", |ui| {
                    let Some(gui) = &self.gui else { return };
                    let setter = gui.param_setter();

                    for mode in DetectionMode::ALL {
                        if ui
                            .selectable_label(
                                self.params.active().detection.value() == mode,
                                mode.label(),
                            )
                            .clicked()
                        {
                            set_value(&setter, &self.params.active().detection, mode);
                            egui::Popup::close_all(ui.ctx());
                        }
                    }
                });

                ui.menu_button("UI Scale", |ui| {
                    for percent in UI_SCALES {
                        let scale = percent as f32 / 100.0;

                        if ui
                            .selectable_label(
                                (ui.ctx().zoom_factor() - scale).abs() < 0.001,
                                format!("{percent}%"),
                            )
                            .clicked()
                        {
                            self.params
                                .ui_scale_percent
                                .store(percent, Ordering::Relaxed);
                            ui.set_zoom_factor(scale);
                            ui.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(
                                900.0, 684.0,
                            )));
                            egui::Popup::close_all(ui.ctx());
                        }
                    }
                });

                ui.separator();

                if ui.button("About").clicked() {
                    self.about_open = true;
                    egui::Popup::close_all(ui.ctx());
                }
            });

        let params = &self.params;
        let gui = &self.gui;
        let channels = self.telemetry.audio_channels.load(Ordering::Relaxed);
        let buffer = self.telemetry.buffer_samples.load(Ordering::Relaxed);
        let latency = self.telemetry.latency_samples.load(Ordering::Relaxed);

        egui::Window::new("Audio Settings")
            .open(&mut self.audio_open)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .default_width(280.0)
            .show(ui.ctx(), |ui| {
                egui::Grid::new("audio-info")
                    .num_columns(2)
                    .spacing([24.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Sample Rate");
                        ui.label(if v[7] > 0.0 {
                            format!("{:.0} Hz", v[7])
                        } else {
                            "--".into()
                        });
                        ui.end_row();

                        ui.label("Channels");
                        ui.label(match channels {
                            1 => "Mono",
                            2 => "Stereo",
                            _ => "--",
                        });
                        ui.end_row();

                        ui.label("Detection Input");
                        ui.label(match params.mode.value() {
                            crate::mode::OperatingMode::Track => "Main input",
                            crate::mode::OperatingMode::Sidechain => "External sidechain",
                        });
                        ui.end_row();

                        ui.label("Buffer");
                        ui.label(if buffer > 0 {
                            format!("{buffer} samples")
                        } else {
                            "--".into()
                        });
                        ui.end_row();

                        ui.label("Latency");
                        ui.label(if latency == 0 {
                            "0 samples".into()
                        } else if v[7] > 0.0 {
                            format!(
                                "{latency} samples ({:.1} ms)",
                                latency as f32 * 1000.0 / v[7]
                            )
                        } else {
                            format!("{latency} samples")
                        });
                        ui.end_row();
                    });

                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("Device, sample rate and buffer are set by the host.")
                        .size(11.0)
                        .color(MUTED),
                );

                ui.separator();

                let Some(gui) = gui else { return };
                let setter = gui.param_setter();
                let mut bypass = params.bypass.value();

                if ui.checkbox(&mut bypass, "Bypass").changed() {
                    set_value(&setter, &params.bypass, bypass);
                }

                egui::Grid::new("audio-processing")
                    .num_columns(2)
                    .spacing([24.0, 8.0])
                    .show(ui, |ui| {
                        for (param, label, min, max, speed, unit, factor) in [
                            (
                                &params.active().tracking,
                                "Tracking",
                                5.0,
                                300.0,
                                1.0,
                                " ms",
                                1.0,
                            ),
                            (
                                &params.active().attack,
                                "Attack",
                                0.1,
                                300.0,
                                0.1,
                                " ms",
                                1.0,
                            ),
                            (
                                &params.active().release,
                                "Release",
                                1.0,
                                2000.0,
                                1.0,
                                " ms",
                                1.0,
                            ),
                            (
                                &params.active().gate,
                                "Gate",
                                -80.0,
                                -20.0,
                                0.1,
                                " dBFS",
                                1.0,
                            ),
                            (
                                &params.active().clarity,
                                "Clarity",
                                50.0,
                                99.0,
                                0.1,
                                " %",
                                100.0,
                            ),
                        ] {
                            ui.label(label);
                            let mut value = param.value() * factor;
                            let edit = ui.add(
                                egui::DragValue::new(&mut value)
                                    .range(min..=max)
                                    .speed(speed)
                                    .suffix(unit),
                            );

                            if edit.drag_started() {
                                setter.begin_set_parameter(param);
                            }

                            if edit.changed() {
                                if edit.dragged() {
                                    setter.set_parameter(param, value / factor);
                                } else {
                                    set_value(&setter, param, value / factor);
                                }
                            }

                            if edit.drag_stopped() {
                                setter.end_set_parameter(param);
                            }
                            ui.end_row();
                        }
                    });
            });
    }
}
