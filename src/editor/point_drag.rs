use super::*;
use crate::mode::OperatingMode;

pub(super) struct PointDrag {
    mode: OperatingMode,
    index: usize,
    origin: Pos2,
    ratio: f32,
    gain: f32,
    f0: f32,
    has_gain: bool,
}

impl F0Editor {
    pub(super) fn finish_point_drag(&mut self) {
        if let Some(drag) = self.point_drag.take() {
            let gui = self
                .gui
                .as_ref()
                .expect("Point drag requires a GUI context");
            let setter = gui.param_setter();
            let band = self.params.for_mode(drag.mode).band(drag.index);

            setter.end_set_parameter(band.ratio);

            if drag.has_gain {
                setter.end_set_parameter(band.gain);
            }
        }
    }

    pub(super) fn drag_spectrum_points(
        &mut self,
        ui: &mut egui::Ui,
        markers: &[Option<Pos2>; BAND_COUNT],
        plot: Rect,
        max_hz: f32,
        f0: f32,
    ) {
        if self.point_drag.as_ref().is_some_and(|drag| {
            let band = self.params.for_mode(drag.mode).band(drag.index);

            !ui.input(|i| i.focused && i.pointer.primary_down())
                || drag.mode != self.params.mode.value()
                || !band.exists.value()
                || drag.has_gain != band.kind.value().has_gain()
        }) {
            self.finish_point_drag();
        }

        // Register after the target handles so a point takes priority where they overlap.
        let mut order: [usize; BAND_COUNT] = std::array::from_fn(|i| i);
        order.sort_by_key(|&i| self.selected == Some(i));

        for index in order {
            let Some(position) = markers[index] else {
                continue;
            };

            // Drag-only sensing leaves clicks and the context menu to the spectrum.
            let response = ui
                .interact(
                    Rect::from_center_size(position, Vec2::splat(24.0)),
                    ui.make_persistent_id(("spectrum-point-drag", index)),
                    Sense::drag(),
                )
                .on_hover_cursor(egui::CursorIcon::Grab);

            if response.drag_started_by(egui::PointerButton::Primary) {
                self.finish_point_drag();
                self.selected = Some(index);

                let gui = self
                    .gui
                    .as_ref()
                    .expect("Point drag requires a GUI context");
                let setter = gui.param_setter();
                let band = self.params.band(index);
                let has_gain = band.kind.value().has_gain();

                setter.begin_set_parameter(band.ratio);

                if has_gain {
                    setter.begin_set_parameter(band.gain);
                }

                self.point_drag = Some(PointDrag {
                    mode: self.params.mode.value(),
                    index,
                    origin: ui
                        .input(|i| i.pointer.press_origin())
                        .expect("Drag has a press origin"),
                    ratio: band.ratio.unmodulated_plain_value(),
                    gain: band.gain.unmodulated_plain_value(),
                    f0,
                    has_gain,
                });
            }

            if response.dragged_by(egui::PointerButton::Primary)
                && let Some(drag) = self.point_drag.as_ref().filter(|d| d.index == index)
                && let Some(pointer) = response.interact_pointer_pos()
            {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);

                let delta = pointer - drag.origin;
                // Anchor to the original ratio: a changing live F0 must not edit it by itself.
                let ratio = (drag.ratio * (delta.x / plot.width() * (max_hz / 20.0).ln()).exp())
                    .clamp(20.0 / drag.f0, max_hz / drag.f0);
                let band = self.params.for_mode(drag.mode).band(index);
                let gui = self
                    .gui
                    .as_ref()
                    .expect("Point drag requires a GUI context");
                let setter = gui.param_setter();
                let ratio = band
                    .ratio
                    .preview_plain(band.ratio.preview_normalized(ratio).clamp(0.0, 1.0));

                if delta.x != 0.0 && ratio != band.ratio.unmodulated_plain_value() {
                    setter.set_parameter(band.ratio, ratio);
                }

                if drag.has_gain {
                    let gain = (drag.gain - delta.y / plot.height() * 48.0).clamp(-24.0, 24.0);

                    if delta.y != 0.0 && gain != band.gain.unmodulated_plain_value() {
                        setter.set_parameter(band.gain, gain);
                    }
                }
            }

            if response.drag_stopped_by(egui::PointerButton::Primary) {
                self.finish_point_drag();
            }
        }
    }
}
