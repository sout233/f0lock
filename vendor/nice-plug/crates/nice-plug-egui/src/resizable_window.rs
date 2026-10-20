//! Resizable window wrapper for Egui editor.

use std::sync::LazyLock;

use egui::emath::GuiRounding;
use egui::{CentralPanel, Id, Rect, Response, Sense, Ui, Vec2, pos2};
use egui::{InnerResponse, UiBuilder};

static ID: LazyLock<egui::Id> = LazyLock::new(|| egui::Id::unique((file!(), 0)));

/// Adds a corner to the plugin window that can be dragged in order to resize it.
/// Resizing happens through plugin API, hence a custom implementation is needed.
pub struct ResizableWindow {
    id: Id,
    min_size: Vec2,
    max_size: Option<Vec2>,
}

impl ResizableWindow {
    pub fn new(id_source: impl egui::AsIdSalt) -> Self {
        Self {
            id: ID.with(id_source),
            min_size: Vec2 { x: 1.0, y: 1.0 },
            max_size: None,
        }
    }

    pub fn min_size(mut self, min_size: Vec2) -> Self {
        self.min_size = min_size;
        self
    }

    pub fn max_size(mut self, max_size: Vec2) -> Self {
        self.max_size = Some(max_size);
        self
    }

    pub fn show<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        CentralPanel::default().show(ui, move |ui| {
            let ui_rect = ui.clip_rect();
            let mut content_ui =
                ui.new_child(UiBuilder::new().max_rect(ui_rect).layout(*ui.layout()));

            let ret = add_contents(&mut content_ui);

            let corner_size = Vec2::splat(ui.visuals().resize_corner_size);
            let corner_rect = Rect::from_min_size(ui_rect.max - corner_size, corner_size);

            let corner_response = ui.interact(corner_rect, self.id, Sense::drag());

            #[derive(Clone, Copy)]
            struct State {
                start_window_size: Vec2,
            }

            if corner_response.dragged()
                && let Some(total_drag_delta) = corner_response.total_drag_delta()
            {
                let state = ui.data_mut(|d| {
                    *d.get_persisted_mut_or_default::<Option<State>>(self.id)
                        .get_or_insert_with(|| State {
                            start_window_size: ui_rect.max.to_vec2(),
                        })
                });

                let mut desired_size = state.start_window_size + total_drag_delta;

                desired_size = desired_size.max(self.min_size);
                if let Some(max_size) = self.max_size {
                    desired_size = desired_size.min(max_size);
                }

                ui.send_viewport_cmd(egui::ViewportCommand::InnerSize(desired_size));
            } else if corner_response.drag_stopped() {
                ui.data_mut(|d| *d.get_persisted_mut_or_default::<Option<State>>(self.id) = None);
            }

            paint_resize_corner(&content_ui, &corner_response);

            ret
        })
    }
}

pub fn paint_resize_corner(ui: &Ui, response: &Response) {
    let stroke = ui.style().interact(response).fg_stroke;

    let painter = ui.painter();
    let rect = response.rect.translate(-Vec2::splat(2.0)); // move away from the corner
    let cp = rect.max.round_to_pixels(painter.pixels_per_point());

    let mut w = 2.0;

    while w <= rect.width() && w <= rect.height() {
        painter.line_segment([pos2(cp.x - w, cp.y), pos2(cp.x, cp.y - w)], stroke);
        w += 4.0;
    }
}
