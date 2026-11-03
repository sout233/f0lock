use super::*;
use crate::filter::CutSlope;
use crate::mode::OperatingMode;

/// One wheel notch scales Q by this much. Multiplicative rather than additive,
/// so a notch feels the same at a narrow Q as at a wide one.
const Q_WHEEL_STEP: f32 = 1.15;

/// Wheel movement in notches, read straight from the events so the smoothing
/// egui applies to its scroll delta cannot keep editing after the wheel stops.
pub(super) fn wheel_notches(ui: &egui::Ui) -> f32 {
    ui.input(|i| {
        i.events
            .iter()
            .filter_map(|event| match event {
                egui::Event::MouseWheel { unit, delta, .. } => Some(match unit {
                    egui::MouseWheelUnit::Line => delta.y,
                    egui::MouseWheelUnit::Point => delta.y / 50.0,
                    egui::MouseWheelUnit::Page => delta.y * 8.0,
                }),
                _ => None,
            })
            .sum()
    })
}

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

    /// Scale a point's Q by a number of wheel notches, through the parameter's
    /// own range so the knob and the readout follow.
    pub(super) fn scroll_q(&self, index: usize, notches: f32) {
        let Some(gui) = &self.gui else {
            return;
        };
        let band = self.params.band(index);

        // First-order cuts have no Q control, so they keep the wheel too.
        if !band.exists.value()
            || (band.kind.value().is_cut() && band.slope.value() == CutSlope::Db6)
        {
            return;
        }

        let target = band.q.unmodulated_plain_value() * Q_WHEEL_STEP.powf(notches);
        let target = band
            .q
            .preview_plain(band.q.preview_normalized(target).clamp(0.0, 1.0));

        if target != band.q.unmodulated_plain_value() {
            let setter = gui.param_setter();
            setter.begin_set_parameter(band.q);
            setter.set_parameter(band.q, target);
            setter.end_set_parameter(band.q);
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
                    f0: self.point_pitch(index, f0),
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

        // The wheel edits Q while a point is held, so one drag can place the
        // point and size its width without letting go.
        if let Some(index) = self.point_drag.as_ref().map(|drag| drag.index) {
            let notches = wheel_notches(ui);

            if notches != 0.0 {
                self.scroll_q(index, notches);
            }
        }
    }
}
