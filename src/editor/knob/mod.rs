use egui::{Color32, FontId, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};
use nice_plug::{context::gui::ParamSetter, prelude::*};
use std::f32::consts::{PI, TAU};

pub const RED: Color32 = Color32::from_rgb(201, 55, 65);
const INK: Color32 = Color32::from_rgb(58, 59, 60);

mod glow;
mod input;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Default)]
struct Motion {
    from: f32,
    target: f32,
    started: f64,
}

impl Motion {
    fn sample(&mut self, now: f64, active: bool) -> f32 {
        let t = ((now - self.started) / 0.95).clamp(0.0, 1.0) as f32;
        let value = self.from + (self.target - self.from) * (1.0 - (1.0 - t).powi(5));

        let target = f32::from(active);

        if target != self.target {
            self.from = value;
            self.target = target;
            self.started = now;
        }

        value
    }

    fn moving(&self, now: f64) -> bool {
        self.from != self.target && now - self.started < 0.95
    }
}

#[derive(Clone, Copy)]
struct ValueMotion {
    binding: usize,
    from: f32,
    target: f32,
    started: f64,
    switching: bool,
}

impl ValueMotion {
    fn value(&self, now: f64) -> f32 {
        let t = ((now - self.started) / 0.75).clamp(0.0, 1.0) as f32;
        self.from + (self.target - self.from) * (1.0 - (1.0 - t).powi(5))
    }

    fn update(&mut self, binding: usize, target: f32, now: f64, editing: bool) -> f32 {
        if self.binding != binding {
            // Retarget from the visible position, including during a previous transition.
            self.from = self.value(now);
            self.binding = binding;
            self.started = now;
            self.switching = true;
        } else if !self.switching || editing {
            self.from = target;
            self.switching = false;
        }

        self.target = target;

        if now - self.started >= 0.75 {
            self.switching = false;
            self.from = target;
        }

        if self.switching {
            self.value(now)
        } else {
            target
        }
    }
}

fn mix(a: Color32, b: Color32, amount: f32) -> Color32 {
    let channel = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * amount) as u8;
    Color32::from_rgb(
        channel(a.r(), b.r()),
        channel(a.g(), b.g()),
        channel(a.b(), b.b()),
    )
}

pub(super) fn light_colors(accent: Color32) -> (Color32, Color32) {
    let mut luminous = egui::ecolor::Hsva::from(accent);
    luminous.v = 1.0;
    luminous.s = (luminous.s * 1.1).min(1.0);
    let glow = Color32::from(luminous);
    (glow, mix(glow, Color32::WHITE, 0.22))
}

pub fn gradient_rect(p: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();

    for (pos, color) in [
        (rect.left_top(), top),
        (rect.right_top(), top),
        (rect.right_bottom(), bottom),
        (rect.left_bottom(), bottom),
    ] {
        mesh.colored_vertex(pos, color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(mesh));
}

fn disc(p: &egui::Painter, center: Pos2, radius: f32, top: Color32, bottom: Color32) {
    let scale = p.pixels_per_point();
    let radius = (radius * scale).round() / scale;
    let feather = 0.5 / scale;
    let segments = (8.0 * (radius * scale).sqrt()).ceil() as u32;
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center, mix(top, bottom, 0.5));

    for i in 0..=segments {
        let direction = Vec2::angled(i as f32 / segments as f32 * TAU);
        let light = (direction.y + 1.0) * 0.5;
        mesh.colored_vertex(
            center + direction * (radius - feather),
            mix(top, bottom, light),
        );
        mesh.colored_vertex(
            center + direction * (radius + feather),
            Color32::TRANSPARENT,
        );

        if i > 0 {
            let previous = 1 + (i - 1) * 2;
            let current = 1 + i * 2;
            mesh.add_triangle(0, previous, current);
            // A one-physical-pixel coverage fringe keeps raw meshes antialiased.
            mesh.add_triangle(previous, previous + 1, current);
            mesh.add_triangle(previous + 1, current + 1, current);
        }
    }
    p.add(egui::Shape::mesh(mesh));
}

fn arc(p: &egui::Painter, center: Pos2, radius: f32, from: f32, to: f32, stroke: Stroke) {
    if from == to {
        return;
    }

    let points = (0..=64)
        .map(|i| {
            let value = from + (to - from) * i as f32 / 64.0;
            center + Vec2::angled(PI * (0.75 + 1.5 * value)) * radius
        })
        .collect();
    p.add(egui::Shape::line(points, stroke));
}

/// All edits use host gestures and the parameter's own normalization curve.
pub fn knob(
    ui: &mut Ui,
    param: &FloatParam,
    setter: &ParamSetter<'_>,
    center: Pos2,
    radius: f32,
    label: &str,
    bipolar: bool,
) -> Response {
    let id = ui.make_persistent_id(("knob", label));
    let scale = radius / 35.0;

    let response = ui
        .interact(
            Rect::from_center_size(center, Vec2::splat((radius + 15.0 * scale) * 2.0)),
            id,
            Sense::click_and_drag(),
        )
        .on_hover_cursor(egui::CursorIcon::Grab);

    if response.clicked() {
        response.request_focus();
    }

    if response.drag_started_by(egui::PointerButton::Primary) {
        setter.begin_set_parameter(param);
        ui.data_mut(|data| data.insert_temp(id, param.unmodulated_normalized_value()));
    }

    if response.dragged_by(egui::PointerButton::Primary) {
        let sensitivity = if ui.input(|i| i.modifiers.shift) {
            0.0004
        } else {
            0.004
        };

        let delta = response.drag_delta();
        let previous = ui
            .data(|data| data.get_temp::<f32>(id))
            .expect("Knob drag started without a parameter gesture");
        let value = (previous + (delta.x - delta.y) * sensitivity).clamp(0.0, 1.0);
        ui.data_mut(|data| data.insert_temp(id, value));
        setter.set_parameter(param, param.preview_plain(value));
    }

    if response.drag_stopped_by(egui::PointerButton::Primary) {
        setter.end_set_parameter(param);
        ui.data_mut(|data| data.remove::<f32>(id));
    }

    if response.double_clicked() || (response.clicked() && ui.input(|i| i.modifiers.command)) {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, param.default_plain_value());
        setter.end_set_parameter(param);
    }

    let wheel = if response.hovered() {
        ui.input(|i| i.smooth_scroll_delta.y)
    } else {
        0.0
    };

    let key = if response.has_focus() {
        ui.input(|i| {
            f32::from(i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::ArrowRight))
                - f32::from(
                    i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowLeft),
                )
        })
    } else {
        0.0
    };

    if wheel != 0.0 || key != 0.0 {
        let fine = if ui.input(|i| i.modifiers.shift) {
            0.1
        } else {
            1.0
        };

        let value = (param.unmodulated_normalized_value() + (wheel * 0.001 + key * 0.01) * fine)
            .clamp(0.0, 1.0);
        setter.begin_set_parameter(param);
        setter.set_parameter(param, param.preview_plain(value));
        setter.end_set_parameter(param);
    }

    let entered_value = input::edit_value(ui, &response, param, setter, label);
    let target = param.modulated_normalized_value();
    let now = ui.input(|i| i.time);
    let binding = param as *const FloatParam as usize;
    let editing = entered_value
        || response.dragged()
        || response.double_clicked()
        || (response.clicked() && ui.input(|i| i.modifiers.command))
        || wheel != 0.0
        || key != 0.0;

    let (value, switching) = ui.data_mut(|data| {
        let motion_id = id.with("value-transition");
        let mut motion = data
            .get_temp::<ValueMotion>(motion_id)
            .unwrap_or(ValueMotion {
                binding,
                from: target,
                target,
                started: now,
                switching: false,
            });

        let value = motion.update(binding, target, now, editing);
        data.insert_temp(motion_id, motion);
        (value, motion.switching)
    });

    let ink = if ui.is_enabled() {
        INK
    } else {
        Color32::from_gray(143)
    };

    let accent = if ui.is_enabled() {
        ui.visuals().selection.stroke.color
    } else {
        Color32::from_gray(163)
    };

    let (hover, press, moving) = ui.data_mut(|data| {
        let motion_id = id.with("motion");
        let mut motion = data.get_temp::<[Motion; 2]>(motion_id).unwrap_or_default();
        let hover = motion[0].sample(now, ui.is_enabled() && response.hovered());
        let press = motion[1].sample(now, ui.is_enabled() && response.is_pointer_button_down_on());
        let moving = motion.iter().any(|m| m.moving(now));
        data.insert_temp(motion_id, motion);
        (hover, press, moving)
    });

    if moving || switching {
        ui.ctx().request_repaint();
    }

    let p = ui.painter();
    let body_radius = radius * 0.87;
    let body_center = Pos2::new(
        p.round_to_pixel_center(center.x),
        p.round_to_pixel_center(center.y),
    ) + Vec2::new(0.0, press * 0.8 * scale);
    let blur = ((11.0 + hover * 7.0) * (1.0 - press) + 4.0 * press) * scale;
    // Many low-opacity layers approximate a soft shadow without a hard outer edge.
    for i in (0..16).rev() {
        let t = i as f32 / 15.0;
        p.circle_filled(
            body_center + Vec2::new(1.5, (5.0 + hover * 2.0) * (1.0 - press) + 2.0 * press) * scale,
            body_radius + t * blur,
            Color32::from_black_alpha((2.0 + 6.0 * (1.0 - t).powi(2)) as u8),
        );
    }
    p.text(
        center - Vec2::new(0.0, radius + 28.0 * scale),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(15.0),
        ink,
    );

    for i in 0..=10 {
        let angle = PI * (0.75 + 1.5 * i as f32 / 10.0);
        let direction = Vec2::angled(angle);
        let outer = radius + if i % 5 == 0 { 17.0 } else { 14.0 } * scale;
        p.line_segment(
            [
                center + direction * (radius + 11.0 * scale),
                center + direction * outer,
            ],
            Stroke::new(1.0, Color32::from_gray(139)),
        );
    }
    arc(
        p,
        center,
        radius + 7.0 * scale,
        0.0,
        1.0,
        Stroke::new(2.0, Color32::from_gray(158)),
    );

    let start = if bipolar {
        param.preview_normalized(0.0)
    } else {
        0.0
    };

    let (glow, core) = light_colors(accent);

    if press > 0.001 {
        glow::arc(
            p,
            center,
            radius + 7.0 * scale,
            [start, value],
            scale,
            glow,
            press,
        );
    }
    arc(
        p,
        center,
        radius + 7.0 * scale,
        start,
        value,
        Stroke::new(3.0 * scale, mix(accent, core, press)),
    );

    let metal = |gray| {
        let lit = mix(
            Color32::from_gray(gray),
            Color32::from_rgb(245, 247, 250),
            hover * 0.32,
        );
        mix(lit, Color32::from_rgb(147, 151, 157), press * 0.18)
    };
    disc(p, body_center, body_radius + scale, metal(246), metal(124));
    disc(
        p,
        body_center,
        body_radius - 0.5 * scale,
        metal(132),
        metal(235),
    );
    disc(
        p,
        body_center,
        body_radius - 2.0 * scale,
        metal(230),
        metal(182),
    );
    disc(
        p,
        body_center - Vec2::new(0.0, 0.8 * scale),
        body_radius - 4.0 * scale,
        metal(226),
        metal(201),
    );

    let direction = Vec2::angled(PI * (0.75 + 1.5 * value));
    let a = body_center + direction * (body_radius * 0.53);
    let b = body_center + direction * (body_radius * 0.82);
    p.line_segment(
        [
            a + Vec2::new(0.8, 0.9) * scale,
            b + Vec2::new(0.8, 0.9) * scale,
        ],
        Stroke::new(4.0 * scale, Color32::from_gray(240)),
    );
    p.line_segment([a, b], Stroke::new(3.5 * scale, ink));

    if response.has_focus() {
        p.circle_stroke(
            center,
            radius + 21.0 * scale,
            Stroke::new(1.0, accent.gamma_multiply(0.6)),
        );
    }

    let display = input::value_text(param, value, label);
    p.text(
        center + Vec2::new(0.0, radius + 29.0 * scale),
        egui::Align2::CENTER_CENTER,
        display,
        FontId::monospace(13.0),
        if ui.is_enabled() {
            Color32::from_gray(88)
        } else {
            Color32::from_gray(151)
        },
    );

    response
}
