use super::*;

impl F0Editor {
    pub(super) fn accent(&self) -> Color32 {
        POINT_COLORS[self.selected.unwrap_or(0)]
    }

    pub(super) fn point_tabs(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let p = ui.painter_at(rect);
        gradient_rect(&p, rect, Color32::from_gray(48), Color32::from_gray(37));
        p.line_segment(
            [rect.left_top(), rect.right_top()],
            Stroke::new(1.0, Color32::from_gray(67)),
        );

        let mut x = rect.left() + 20.0;

        for (i, color) in POINT_COLORS.iter().copied().enumerate() {
            if !self.params.band(i).exists.value() {
                continue;
            }

            let tab = Rect::from_min_size(Pos2::new(x, rect.top() + 4.0), Vec2::new(96.0, 25.0));
            let response =
                ui.interact(tab, ui.make_persistent_id(("point-tab", i)), Sense::click());

            if response.clicked() {
                self.selected = Some(i);
            }

            let selected = self.selected == Some(i);
            p.rect_filled(
                tab,
                3.0,
                Color32::from_gray(if selected {
                    66
                } else if response.hovered() {
                    55
                } else {
                    43
                }),
            );
            p.rect_filled(
                Rect::from_min_size(tab.min + Vec2::new(6.0, 5.0), Vec2::new(3.0, 15.0)),
                1.0,
                color,
            );

            if selected {
                p.line_segment(
                    [
                        tab.left_bottom() + Vec2::new(3.0, 0.0),
                        tab.right_bottom() - Vec2::new(3.0, 0.0),
                    ],
                    Stroke::new(1.5, color),
                );
            }
            p.text(
                tab.center() + Vec2::new(3.0, 0.0),
                egui::Align2::CENTER_CENTER,
                format!("Point {}", i + 1),
                FontId::proportional(12.0),
                if selected { Color32::WHITE } else { TEXT },
            );
            x += 101.0;
        }

        let button = Rect::from_min_size(Pos2::new(x, rect.top() + 4.0), Vec2::new(30.0, 25.0));
        let response = ui.interact(button, ui.make_persistent_id("add-point"), Sense::click());
        p.rect_filled(
            button,
            3.0,
            Color32::from_gray(if response.hovered() { 62 } else { 43 }),
        );
        p.text(
            button.center(),
            egui::Align2::CENTER_CENTER,
            "+",
            FontId::proportional(19.0),
            self.accent(),
        );

        if response.clicked() {
            self.add_default_point(ui.ctx());
        }
    }

    fn add_default_point(&mut self, ctx: &egui::Context) {
        let Some(index) = (0..BAND_COUNT).find(|&i| !self.params.band(i).exists.value()) else {
            self.notify(ctx, "All 8 points are in use");
            return;
        };

        let Some(gui) = &self.gui else {
            return;
        };

        let setter = gui.param_setter();
        let band = self.params.band(index);
        set_value(&setter, band.ratio, (index + 1) as f32);
        set_value(&setter, band.gain, if index == 0 { -6.0 } else { 0.0 });
        set_value(&setter, band.q, 3.0);
        set_value(&setter, band.kind, FilterKind::Peak);
        set_value(&setter, band.enabled, true);
        set_value(&setter, band.exists, true);

        self.selected = Some(index);
        self.notice = None;
    }

    fn notify(&mut self, ctx: &egui::Context, message: &'static str) {
        self.notice = Some(Notice {
            message,
            expires_at: ctx.input(|i| i.time) + 4.0,
        });
    }

    pub(super) fn show_notice(&mut self, ctx: &egui::Context, origin: Pos2) {
        let Some(notice) = &self.notice else {
            return;
        };

        if ctx.input(|i| i.time) >= notice.expires_at {
            self.notice = None;
            return;
        }

        let message = notice.message;
        let accent = self.accent();
        let mut dismiss = false;

        egui::Area::new(egui::Id::unique("f0lock-notifications"))
            .order(egui::Order::Foreground)
            .fixed_pos(origin + Vec2::new(12.0, 10.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_gray(42))
                    .stroke(Stroke::new(1.0, accent))
                    .corner_radius(4.0)
                    .inner_margin(10)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 3],
                        blur: 10,
                        spread: 0,
                        color: Color32::from_black_alpha(100),
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(message).size(12.0).color(TEXT));
                            dismiss = ui.small_button("×").clicked();
                        });
                    });
            });

        if dismiss {
            self.notice = None;
        }
    }
}
