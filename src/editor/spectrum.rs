use super::*;
use crate::mode::OperatingMode;
use crate::{
    analyzer::{BINS, bin_frequency, max_frequency},
    band::{BAND_COUNT, BAND_VALUES},
    filter::Coefficients,
};
use realfft::num_complex::Complex32;

struct Plot {
    rect: Rect,
    max_hz: f32,
}
impl Plot {
    fn new(outer: Rect, rate: f32) -> Self {
        Self {
            rect: Rect::from_min_max(
                outer.min + Vec2::new(37.0, 17.0),
                outer.max - Vec2::new(37.0, 28.0),
            ),
            max_hz: if rate > 0.0 {
                max_frequency(rate)
            } else {
                20000.0
            },
        }
    }

    fn x(&self, hz: f32) -> f32 {
        self.rect.left() + (hz / 20.0).ln() / (self.max_hz / 20.0).ln() * self.rect.width()
    }

    fn y(&self, db: f32) -> f32 {
        self.rect.center().y - db.clamp(-24.0, 24.0) / 48.0 * self.rect.height()
    }
}

impl F0Editor {
    pub(super) fn spectrum(&mut self, ui: &mut egui::Ui, outer: Rect, v: &[f32; 10]) {
        let p = ui.painter_at(outer);
        gradient_rect(&p, outer, Color32::from_gray(24), Color32::from_gray(32));
        p.rect_stroke(
            outer,
            2.0,
            Stroke::new(1.0, Color32::from_gray(17)),
            egui::StrokeKind::Inside,
        );

        let plot = Plot::new(outer, v[7]);
        draw_grid(&p, &plot, outer);

        if self.params.bypass.value() {
            self.context_point = None;
            p.text(
                plot.rect.center(),
                egui::Align2::CENTER_CENTER,
                "BYPASS",
                FontId::new(80.0, egui::FontFamily::Name(GLOBAL_FONT.into())),
                Color32::from_white_alpha(36),
            );
            return;
        }

        draw_spectrum(&p, &plot, &self.telemetry.spectrum());

        let bands = self.telemetry.bands();

        if v[7] > 0.0 {
            draw_response(&p, &plot, &bands, v[7], self.accent());
        }

        let layout_f0 = if v[2] > 0.0 { v[2] } else { PREVIEW_F0 };
        let markers: [Option<Pos2>; BAND_COUNT] = std::array::from_fn(|i| {
            let b = self.params.band(i);

            if !b.exists.value() {
                return None;
            }

            let frequency = (layout_f0 * b.ratio.value()).min(plot.max_hz).max(20.0);
            let db = if b.kind.value().has_gain() {
                b.gain.value()
            } else {
                0.0
            };

            let point = Pos2::new(plot.x(frequency), plot.y(db));

            let color = if !b.enabled.value() {
                Color32::from_gray(91)
            } else {
                POINT_COLORS[i]
            };
            p.circle_filled(point, 7.5, Color32::from_gray(29));
            p.circle_stroke(
                point,
                7.5,
                Stroke::new(if self.selected == Some(i) { 2.5 } else { 1.3 }, color),
            );
            p.text(
                point,
                egui::Align2::CENTER_CENTER,
                format!("{}", i + 1),
                FontId::monospace(10.0),
                color,
            );
            Some(point)
        });

        let response = ui.interact(
            plot.rect,
            ui.make_persistent_id("spectrum-points"),
            Sense::click(),
        );

        let nearest = |position: Pos2| {
            markers
                .iter()
                .enumerate()
                .filter_map(|(i, point)| point.map(|point| (i, point.distance(position))))
                .filter(|(_, distance)| *distance <= 12.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        };

        if let Some(position) = response.interact_pointer_pos() {
            if response.clicked()
                && let Some(index) = nearest(position)
            {
                self.selected = Some(index);
            }

            if response.secondary_clicked() {
                self.context_point = nearest(position);

                if self.context_point.is_some() {
                    self.selected = self.context_point;
                } else {
                    egui::Popup::close_all(ui.ctx());
                }
            }
        }

        if self.context_point.is_some() {
            response.context_menu(|ui| {
                if let Some(index) = self.context_point {
                    self.point_menu(ui, index);
                }
            });
        }

        self.target_controls(ui, plot.rect, plot.max_hz);

        let mut readout = if v[0] > 0.0 {
            format!("{:.1} Hz   {:.0}%", v[0], v[1] * 100.0)
        } else {
            "— Hz   —%".into()
        };

        let sidechain = self.params.mode.value() == OperatingMode::Sidechain;
        if sidechain {
            readout = format!("SC {readout}");
        }
        let note = if v[0] > 0.0 {
            note_name(v[0])
        } else if sidechain {
            "Waiting for sidechain pitch".into()
        } else {
            "—".into()
        };

        let anchor = plot.rect.min + Vec2::new(9.0, 12.0);

        for (text, y) in [(readout, 0.0), (note, 17.0)] {
            p.text(
                anchor + Vec2::new(0.0, y),
                egui::Align2::LEFT_TOP,
                text,
                FontId::monospace(12.0),
                TEXT,
            );
        }
    }
}

fn draw_grid(p: &egui::Painter, plot: &Plot, outer: Rect) {
    p.text(
        Pos2::new(plot.rect.left(), outer.top() + 8.0),
        egui::Align2::LEFT_CENTER,
        "EQ dB",
        FontId::proportional(10.0),
        MUTED,
    );
    p.text(
        Pos2::new(plot.rect.right(), outer.top() + 8.0),
        egui::Align2::RIGHT_CENTER,
        "dBFS",
        FontId::proportional(10.0),
        MUTED,
    );

    for f in [
        20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0,
    ] {
        if f > plot.max_hz {
            continue;
        }

        let x = plot.x(f);
        p.line_segment(
            [
                Pos2::new(x, plot.rect.top()),
                Pos2::new(x, plot.rect.bottom()),
            ],
            Stroke::new(1.0, GRID),
        );

        let label = if f >= 1000.0 {
            format!("{:.0}k", f / 1000.0)
        } else {
            format!("{f:.0}")
        };
        p.text(
            Pos2::new(x, plot.rect.bottom() + 15.0),
            egui::Align2::CENTER_CENTER,
            label,
            FontId::monospace(10.0),
            MUTED,
        );
    }

    for db in [-24.0, -12.0, 0.0, 12.0, 24.0] {
        let y = plot.y(db);
        p.line_segment(
            [
                Pos2::new(plot.rect.left(), y),
                Pos2::new(plot.rect.right(), y),
            ],
            Stroke::new(1.0, GRID),
        );
        p.text(
            Pos2::new(plot.rect.left() - 9.0, y),
            egui::Align2::RIGHT_CENTER,
            format!("{db:+.0}"),
            FontId::monospace(10.0),
            MUTED,
        );
    }

    for (db, fraction) in [(0, 0.0), (-40, 0.5), (-80, 1.0)] {
        p.text(
            Pos2::new(
                plot.rect.right() + 9.0,
                plot.rect.top() + fraction * plot.rect.height(),
            ),
            egui::Align2::LEFT_CENTER,
            format!("{db}"),
            FontId::monospace(10.0),
            MUTED,
        );
    }
}

fn draw_spectrum(p: &egui::Painter, plot: &Plot, spectrum: &[f32; BINS]) {
    let points: Vec<_> = spectrum
        .iter()
        .enumerate()
        .map(|(i, db)| {
            let hz = bin_frequency(i, plot.max_hz);
            Pos2::new(
                plot.x(hz),
                plot.rect.bottom() - ((db + 80.0) / 80.0).clamp(0.0, 1.0) * plot.rect.height(),
            )
        })
        .collect();
    let mut fill = egui::Mesh::default();

    for (i, point) in points.iter().enumerate() {
        fill.colored_vertex(*point, Color32::from_white_alpha(30));
        fill.colored_vertex(
            Pos2::new(point.x, plot.rect.bottom()),
            Color32::from_white_alpha(2),
        );

        if i > 0 {
            let j = i as u32 * 2;
            fill.add_triangle(j - 2, j - 1, j);
            fill.add_triangle(j - 1, j + 1, j);
        }
    }
    p.add(egui::Shape::mesh(fill));
    p.add(egui::Shape::line(
        points,
        Stroke::new(1.3, Color32::from_gray(128)),
    ));
}

fn draw_response(
    p: &egui::Painter,
    plot: &Plot,
    bands: &[[f32; BAND_VALUES]; BAND_COUNT],
    rate: f32,
    accent: Color32,
) {
    let points = (0..480)
        .map(|i| {
            let hz = 20.0 * (plot.max_hz / 20.0).powf(i as f32 / 479.0);
            let mut response = Complex32::new(1.0, 0.0);

            for band in bands {
                if band[3] == 0.0 {
                    continue;
                }

                let coefficients = Coefficients::new(band[4], band[5], [band[6], band[7], band[8]]);
                response *=
                    Complex32::new(1.0 - band[3], 0.0) + coefficients.response(rate, hz) * band[3];
            }
            Pos2::new(plot.x(hz), plot.y(20.0 * response.norm().max(1e-9).log10()))
        })
        .collect();
    p.add(egui::Shape::line(points, Stroke::new(2.0, accent)));
}
