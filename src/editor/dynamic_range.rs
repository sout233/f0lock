use super::spectrum::Plot;
use super::*;
use crate::filter::{Coefficients, FilterKind};
use crate::mode::OperatingMode;

/// A dynamic range grip being dragged. The value is anchored to where the press
/// started, so it follows the pointer instead of accumulating frame deltas.
pub(super) struct RangeDrag {
    mode: OperatingMode,
    index: usize,
    origin_y: f32,
    range: f32,
}

/// The grip uses the plot's own scale, so dragging it by one grid step changes
/// the range by as much as it moves on screen.
const PLOT_DB: f32 = 48.0;
const GRIP_HALF: f32 = 9.0;
const GRIP_ARROW: f32 = 5.0;
const GRIP_HIT: Vec2 = Vec2::new(22.0, 26.0);
/// Room kept between the grip and the point marker (radius 7.5 px) so both
/// stay draggable even when the range is close to zero.
const GRIP_CLEARANCE: f32 = 18.0;

fn band_db(rate: f32, kind: FilterKind, frequency: f32, q: f32, gain: f32, hz: f32) -> f32 {
    let coefficients = Coefficients::for_kind(rate, frequency, q, gain, kind);
    20.0 * coefficients.response(rate, hz).norm().max(1e-9).log10()
}

impl F0Editor {
    pub(super) fn finish_range_drag(&mut self) {
        if let Some(drag) = self.range_drag.take()
            && let Some(gui) = &self.gui
        {
            let band = self.params.for_mode(drag.mode).band(drag.index);
            gui.param_setter().end_set_parameter(&band.dynamics.range);
        }
    }

    /// The dynamic extent of every enabled point: a shaded band between the
    /// static curve and the curve at full range. It is drawn under the response
    /// so the response stays the brightest line on the plot.
    pub(super) fn dynamic_range_fills(&self, p: &egui::Painter, plot: &Plot, rate: f32, f0: f32) {
        for index in 0..BAND_COUNT {
            let band = self.params.band(index);

            if !band.exists.value()
                || !band.dynamics.enabled.value()
                || !band.kind.value().has_gain()
            {
                continue;
            }

            let range = band.dynamics.range.value();

            if range > -0.05 {
                continue;
            }

            let frequency = (self.point_pitch(index, f0) * band.ratio.value())
                .min(plot.max_hz)
                .max(20.0);
            let gain = band.gain.value();
            let q = band.q.value();
            let kind = band.kind.value();
            let color = POINT_COLORS[index];
            // Kept faint on purpose: the shading is context, and several
            // dynamic points can overlap in the same region.
            let (fill_alpha, edge_alpha) = if self.selected == Some(index) {
                (0.11, 0.40)
            } else {
                (0.055, 0.20)
            };
            let fill = color.gamma_multiply(fill_alpha);

            let steps = 192;
            let mut mesh = egui::Mesh::default();
            let mut edge = Vec::with_capacity(steps + 1);

            for i in 0..=steps {
                let hz = 20.0 * (plot.max_hz / 20.0).powf(i as f32 / steps as f32);
                let x = plot.x(hz);
                let top = plot.y(band_db(rate, kind, frequency, q, gain, hz));
                let bottom = plot.y(band_db(rate, kind, frequency, q, gain + range, hz));

                mesh.colored_vertex(Pos2::new(x, top), fill);
                mesh.colored_vertex(Pos2::new(x, bottom), fill);

                if i > 0 {
                    let j = i as u32 * 2;
                    mesh.add_triangle(j - 2, j - 1, j);
                    mesh.add_triangle(j - 1, j + 1, j);
                }

                edge.push(Pos2::new(x, bottom));
            }

            let clipped = p.with_clip_rect(plot.rect);
            clipped.add(egui::Shape::mesh(mesh));
            clipped.add(egui::Shape::line(
                edge,
                Stroke::new(1.0, color.gamma_multiply(edge_alpha)),
            ));
        }
    }

    /// Draw the range grip of every dynamic point and let it set that point's
    /// Range. The grip sits where the range bottoms out, so pulling it down
    /// deepens the range and pulling it up shallows it.
    pub(super) fn dynamic_range_handles(&mut self, ui: &mut egui::Ui, plot: &Plot, f0: f32) {
        if self.range_drag.as_ref().is_some_and(|drag| {
            let band = self.params.for_mode(drag.mode).band(drag.index);

            !ui.input(|i| i.focused && i.pointer.primary_down())
                || drag.mode != self.params.mode.value()
                || self.selected != Some(drag.index)
                || !band.exists.value()
                || !band.dynamics.enabled.value()
                || !band.kind.value().has_gain()
        }) {
            self.finish_range_drag();
        }

        // The wheel edits Q while the grip is held, matching what it does
        // while an EQ point is held.
        if let Some(index) = self.range_drag.as_ref().map(|drag| drag.index) {
            let notches = super::point_drag::wheel_notches(ui);

            if notches != 0.0 {
                self.scroll_q(index, notches);
            }
        }

        // Only the selected point carries a grip: the strip's Range knob and
        // the grip always edit the same point, so keeping one on screen avoids
        // a row of handles competing for the pointer.
        let Some(index) = self.selected else {
            return;
        };

        // Copy what the grip needs so no borrow of the parameters is held
        // across the parameter edits below.
        let (present, dynamic, gain, range, enabled, ratio, kind) = {
            let band = self.params.band(index);

            (
                band.exists.value(),
                band.dynamics.enabled.value(),
                band.gain.value(),
                band.dynamics.range.value(),
                band.enabled.value(),
                band.ratio.value(),
                band.kind.value(),
            )
        };

        if !present || !dynamic || !kind.has_gain() {
            return;
        }
        let frequency = (self.point_pitch(index, f0) * ratio)
            .min(plot.max_hz)
            .max(20.0);
        let x = plot.x(frequency);
        let marker = plot.y(gain);
        // Keep clear of the point marker when the range is near zero, and
        // inside the plot when it is at full depth.
        let y = plot
            .y(gain + range)
            .max(marker + GRIP_CLEARANCE)
            .min(plot.rect.bottom() - GRIP_HALF - 2.0);
        let center = Pos2::new(x, y);

        let response = ui
            .interact(
                Rect::from_center_size(center, GRIP_HIT),
                ui.make_persistent_id(("spectrum-dynamic-range", index)),
                Sense::drag(),
            )
            .on_hover_cursor(egui::CursorIcon::ResizeVertical)
            .on_hover_text(format!("Dynamic range {range:.1} dB — drag up or down"));

        if response.drag_started_by(egui::PointerButton::Primary) {
            self.finish_range_drag();
            self.selected = Some(index);

            if let Some(gui) = &self.gui {
                let mode = self.params.mode.value();
                let band = self.params.for_mode(mode).band(index);
                let origin_y = ui
                    .input(|i| i.pointer.press_origin())
                    .map_or(center.y, |origin| origin.y);

                gui.param_setter().begin_set_parameter(&band.dynamics.range);
                self.range_drag = Some(RangeDrag {
                    mode,
                    index,
                    origin_y,
                    range: band.dynamics.range.unmodulated_plain_value(),
                });
            }
        }

        if response.dragged_by(egui::PointerButton::Primary)
            && let Some(drag) = self
                .range_drag
                .as_ref()
                .filter(|drag| drag.index == index && drag.mode == self.params.mode.value())
            && let Some(pointer) = response.interact_pointer_pos()
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);

            let value = (drag.range - (pointer.y - drag.origin_y) / plot.rect.height() * PLOT_DB)
                .clamp(-24.0, 0.0);
            let band = self.params.for_mode(drag.mode).band(index);
            let value = band.dynamics.range.preview_plain(
                band.dynamics
                    .range
                    .preview_normalized(value)
                    .clamp(0.0, 1.0),
            );

            if value != band.dynamics.range.unmodulated_plain_value()
                && let Some(gui) = &self.gui
            {
                gui.param_setter()
                    .set_parameter(&band.dynamics.range, value);
            }
        }

        if response.drag_stopped_by(egui::PointerButton::Primary) {
            self.finish_range_drag();
        }

        let color = if !enabled {
            Color32::from_gray(120)
        } else if response.hovered() {
            POINT_COLORS[index].gamma_multiply(1.25)
        } else {
            POINT_COLORS[index]
        };

        let p = ui.painter_at(plot.rect);

        for apex in [-1.0_f32, 1.0] {
            p.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(x, y + apex * GRIP_HALF),
                    Pos2::new(x - GRIP_ARROW, y + apex * 2.0),
                    Pos2::new(x + GRIP_ARROW, y + apex * 2.0),
                ],
                color,
                Stroke::NONE,
            ));
        }
    }
}
