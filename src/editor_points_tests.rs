use super::tests::RecordingHost;
use super::*;

#[path = "editor_knob_tests.rs"]
mod knob_tests;

#[path = "editor_spectrum_tests.rs"]
mod spectrum_tests;

#[path = "editor_scale_tests.rs"]
mod scale_tests;

struct Fixture {
    ctx: egui::Context,
    editor: F0Editor,
    host: Arc<RecordingHost>,
    shapes: Vec<egui::epaint::ClippedShape>,
    viewport_commands: Vec<egui::ViewportCommand>,
    time: f64,
}

impl Fixture {
    fn new() -> Self {
        let ctx = egui::Context::default();
        configure_style(&ctx);

        let telemetry = Arc::new(Telemetry::default());
        let mut engine = crate::engine::Engine::new(48000.0);

        for i in 0..24000 {
            let x = 0.1 * (std::f32::consts::TAU * 220.0 * i as f32 / 48000.0).sin();
            engine.process([x; 2], crate::engine::Controls::default());
        }

        telemetry.publish(&engine);

        let mut editor = F0Editor::new(Arc::new(F0Params::default()), telemetry);
        let host = Arc::new(RecordingHost::default());
        editor.gui = Some(GuiContext::new(host.clone()));

        let mut fixture = Self {
            ctx,
            editor,
            host,
            shapes: Vec::new(),
            viewport_commands: Vec::new(),
            time: 0.0,
        };

        fixture.frame(vec![]);
        fixture
    }

    fn frame(&mut self, events: Vec<egui::Event>) {
        self.time += 1.0 / 60.0;
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 660.0))),
                events,
                time: Some(self.time),
                ..Default::default()
            },
            |ui| self.editor.draw(ui),
        );
        output.textures_delta.clear();
        self.viewport_commands = output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .clone();
        self.shapes = output.shapes;
    }

    fn click(&mut self, pos: Pos2, button: egui::PointerButton) {
        self.frame(vec![egui::Event::PointerMoved(pos)]);
        self.frame(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        self.frame(vec![egui::Event::PointerButton {
            pos,
            button,
            pressed: false,
            modifiers: Default::default(),
        }]);
    }

    fn click_text(&mut self, text: &str) {
        self.frame(vec![]);

        let position = self
            .shapes
            .iter()
            .rev()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(t) if t.galley.job.text == text => {
                    Some(shape.shape.visual_bounding_rect().center())
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!(
                    "Menu item missing: {text}; target={:?}; labels={:?}",
                    self.editor.context_point,
                    self.shapes
                        .iter()
                        .filter_map(|s| match &s.shape {
                            egui::Shape::Text(t) => Some(t.galley.job.text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                )
            });
        self.click(position, egui::PointerButton::Primary);
    }

    fn add(&mut self) {
        self.click_text("+");
    }

    fn handle(&self, index: usize) -> Pos2 {
        let v = self.editor.telemetry.values();
        let b = self.editor.params.band(index);
        let f0 = if v[2] > 0.0 { v[2] } else { PREVIEW_F0 };
        let hz = f0 * b.ratio.value();
        let gain = if b.kind.value().has_gain() {
            b.gain.value()
        } else {
            0.0
        };
        Pos2::new(
            57.0 + (hz / 20.0).ln() / 1000.0_f32.ln() * 786.0,
            181.5 - gain / 48.0 * 173.0,
        )
    }

    fn knob_position(&self, center: Pos2) -> f32 {
        let direction = self
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::LineSegment { points, stroke }
                    if stroke.width == 3.5 && points[1].distance(center) < 35.0 =>
                {
                    Some(points[1] - points[0])
                }
                _ => None,
            })
            .expect("Knob pointer was not rendered");
        let mut angle = direction.y.atan2(direction.x);

        if angle < std::f32::consts::PI * 0.75 {
            angle += std::f32::consts::TAU;
        }
        (angle / std::f32::consts::PI - 0.75) / 1.5
    }
}

#[test]
fn plus_adds_all_harmonics_without_audio_and_uses_tabs() {
    let mut f = Fixture::new();
    f.editor.telemetry = Arc::new(Telemetry::default());

    assert_eq!(f.editor.selected, Some(0));

    for i in 1..BAND_COUNT {
        f.add();
        let b = f.editor.params.band(i);

        assert!(b.exists.value());
        assert!((b.ratio.value() - (i + 1) as f32).abs() < 1e-5);
        assert_eq!(b.gain.value(), 0.0);
        assert_eq!(f.editor.selected, Some(i));
        assert!(f.editor.notice.is_none());
    }
    f.add();

    assert_eq!(
        f.editor.notice.as_ref().unwrap().message,
        "All 8 points are in use"
    );
    assert_eq!(
        (0..BAND_COUNT)
            .filter(|&i| f.editor.params.band(i).exists.value())
            .count(),
        8
    );
    f.click_text("Point 1");

    assert_eq!(f.editor.selected, Some(0));
}

#[test]
fn tab_switch_changes_knob_theme_and_keeps_independent_gain() {
    let mut f = Fixture::new();
    f.add();
    f.add();
    f.click_text("Point 2");
    f.frame(vec![]);

    assert_eq!(f.editor.accent(), POINT_COLORS[1]);
    let q_region = Rect::from_center_size(Pos2::new(337.5, 424.5), Vec2::splat(100.0));

    assert!(f.shapes.iter().any(|s| match &s.shape {
        egui::Shape::Path(path) => path.stroke.width == 3.0
            && matches!(path.stroke.color, egui::epaint::ColorMode::Solid(c) if c == POINT_COLORS[1])
            && q_region.contains(path.visual_bounding_rect().center()),
        _ => false,
    }), "Q knob did not adopt Point 2's theme");

    let pos = Pos2::new(112.5, 424.5);
    f.frame(vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
    ]);
    f.frame(vec![egui::Event::PointerMoved(pos - Vec2::new(0.0, 20.0))]);
    f.frame(vec![egui::Event::PointerButton {
        pos: pos - Vec2::new(0.0, 20.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    }]);

    assert!(f.editor.params.band(1).gain.value() > 0.0);
    assert_eq!(f.editor.params.gain.value(), -6.0);
    f.click_text("Point 3");

    assert_eq!(f.editor.accent(), POINT_COLORS[2]);
    assert_eq!(f.editor.params.band(2).gain.value(), 0.0);
}

#[test]
fn global_controls_stay_neutral_and_ratio_edits_the_selected_point() {
    let mut f = Fixture::new();
    f.add();

    for tab in ["Point 1", "Point 2"] {
        f.click_text(tab);
        f.frame(vec![]);

        for x in [90.0, 270.0, 450.0, 630.0] {
            let region = Rect::from_center_size(Pos2::new(x, 583.0), Vec2::splat(100.0));

            assert!(f.shapes.iter().any(|s| match &s.shape {
                egui::Shape::Path(path) => path.stroke.width == 3.0
                    && matches!(path.stroke.color, egui::epaint::ColorMode::Solid(c) if c == GLOBAL_ACCENT)
                    && region.contains(path.visual_bounding_rect().center()),
                _ => false,
            }), "Global knob at {x} changed color when selecting {tab}");
        }
    }

    let pos = Pos2::new(562.5, 424.5);
    f.frame(vec![egui::Event::PointerMoved(pos)]);
    f.frame(vec![egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    }]);
    f.frame(vec![egui::Event::PointerMoved(pos - Vec2::new(0.0, 15.0))]);
    f.frame(vec![egui::Event::PointerButton {
        pos: pos - Vec2::new(0.0, 15.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    }]);

    assert!(f.editor.params.band(1).ratio.value() > 2.0);
    assert_eq!(f.editor.params.band(0).ratio.value(), 1.0);
    assert_eq!(f.editor.params.tracking.value(), 35.0);
}

#[test]
fn tab_switch_animates_point_knobs_with_ease_out_without_changing_parameters() {
    let mut f = Fixture::new();
    f.add();
    let gui = f.editor.gui.as_ref().unwrap();

    let setter = gui.param_setter();
    let band = f.editor.params.band(1);
    set_value(&setter, band.gain, 12.0);
    set_value(&setter, band.q, 9.0);
    set_value(&setter, band.ratio, 4.0);

    let targets = [band.gain, band.q, band.ratio].map(|p| p.modulated_normalized_value());
    f.click_text("Point 1");

    f.time += 1.0;
    f.frame(vec![]);

    let centers = [112.5, 337.5, 562.5].map(|x| Pos2::new(x, 424.5));
    let before = centers.map(|center| f.knob_position(center));
    let global = f.knob_position(Pos2::new(90.0, 583.0));

    f.host.events.lock().unwrap().clear();
    f.click_text("Point 2");

    for (center, start) in centers.into_iter().zip(before) {
        assert!((f.knob_position(center) - start).abs() < 0.00001);
    }

    let mut previous = before;

    let mut previous_step = [f32::INFINITY; 3];

    for _ in 0..3 {
        f.time += 0.1 - 1.0 / 60.0;
        f.frame(vec![]);

        for i in 0..3 {
            let visible = f.knob_position(centers[i]);
            let step = visible - previous[i];

            assert!(
                step > 0.0 && step < previous_step[i],
                "Knob {i} did not decelerate"
            );
            assert!(visible < targets[i]);
            previous[i] = visible;
            previous_step[i] = step;
        }
    }

    f.time += 1.0;
    f.frame(vec![]);

    for i in 0..3 {
        assert!((f.knob_position(centers[i]) - targets[i]).abs() < 0.00001);
    }

    assert_eq!(global, f.knob_position(Pos2::new(90.0, 583.0)));
    assert_eq!(f.editor.params.band(0).gain.value(), -6.0);
    assert!((f.editor.params.band(1).gain.value() - 12.0).abs() < 0.00001);
    assert!(f.host.events.lock().unwrap().is_empty());
}

#[test]
fn spectrum_does_not_add_or_drag_points() {
    let mut f = Fixture::new();
    let blank = Pos2::new(600.0, 140.0);
    f.click(blank, egui::PointerButton::Primary);
    f.click(blank, egui::PointerButton::Primary);

    assert!(!f.editor.params.band(1).exists.value());
    f.add();
    let position = f.handle(1);

    f.host.events.lock().unwrap().clear();
    f.frame(vec![
        egui::Event::PointerMoved(position),
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
    ]);
    let destination = position + Vec2::new(80.0, -40.0);
    f.frame(vec![egui::Event::PointerMoved(destination)]);
    f.frame(vec![egui::Event::PointerButton {
        pos: destination,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    }]);

    assert!((f.editor.params.band(1).ratio.value() - 2.0).abs() < 1e-5);
    assert_eq!(f.editor.params.band(1).gain.value(), 0.0);
    assert!(f.host.events.lock().unwrap().is_empty());
}

#[test]
fn type_menu_items_are_fully_visible_inside_plugin_window() {
    let window = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 660.0));

    for selected in FilterKind::ALL {
        let mut f = Fixture::new();
        let gui = f.editor.gui.as_ref().unwrap();
        set_value(&gui.param_setter(), f.editor.params.band(0).kind, selected);
        f.frame(vec![]);
        f.click_text(selected.label());
        f.frame(vec![]);

        for kind in FilterKind::ALL {
            let item = f.shapes.iter().rev().find(|shape| {
                matches!(&shape.shape, egui::Shape::Text(t) if t.galley.job.text == kind.label())
            }).unwrap_or_else(|| panic!("Missing type menu item: {}", kind.label()));
            let bounds = item.shape.visual_bounding_rect();

            assert!(
                window.contains_rect(bounds)
                    && item.clip_rect.contains_rect(bounds)
                    && bounds.bottom() < 411.0,
                "{} menu / {} item is clipped or not above its control: bounds={bounds:?}, clip={:?}",
                selected.label(),
                kind.label(),
                item.clip_rect
            );
        }
    }
}

#[test]
fn spectrum_type_submenu_fits_at_right_edge() {
    let mut f = Fixture::new();
    let gui = f.editor.gui.as_ref().unwrap();
    set_value(&gui.param_setter(), f.editor.params.band(0).ratio, 85.0);
    f.frame(vec![]);
    f.click(f.handle(0), egui::PointerButton::Secondary);
    f.click_text("Type");

    for _ in 0..2 {
        f.frame(vec![]);
    }

    let window = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 660.0));

    for kind in FilterKind::ALL {
        let item = f.shapes.iter().rev().find(|shape| {
            matches!(&shape.shape, egui::Shape::Text(t) if t.galley.job.text == kind.label())
        }).unwrap_or_else(|| panic!("Missing submenu item: {}", kind.label()));
        let bounds = item.shape.visual_bounding_rect();

        assert!(
            window.contains_rect(bounds) && item.clip_rect.contains_rect(bounds),
            "{} is clipped: bounds={bounds:?}, clip={:?}",
            kind.label(),
            item.clip_rect
        );
    }
}

#[test]
fn context_menu_and_type_panel_manage_tab_points() {
    let mut f = Fixture::new();
    f.add();
    f.click_text("Peak");
    f.click_text("High Shelf");

    assert_eq!(f.editor.params.band(1).kind.value(), FilterKind::HighShelf);
    f.click(f.handle(1), egui::PointerButton::Secondary);
    f.click_text("Type");
    f.click_text("Notch");

    assert_eq!(f.editor.params.band(1).kind.value(), FilterKind::Notch);
    f.click(f.handle(1), egui::PointerButton::Secondary);
    f.click_text("Bypass");

    assert!(!f.editor.params.band(1).enabled.value());
    f.click(f.handle(1), egui::PointerButton::Secondary);
    f.click_text("Delete");

    assert!(!f.editor.params.band(1).exists.value());
    f.frame(vec![]);

    assert_eq!(f.editor.selected, Some(0));
    f.add();

    assert!((f.editor.params.band(1).ratio.value() - 2.0).abs() < 1e-5);
    assert_eq!(f.editor.params.band(1).kind.value(), FilterKind::Peak);
}

#[test]
fn header_menu_opens_audio_settings_and_about() {
    let mut f = Fixture::new();
    f.click(Pos2::new(860.0, 32.0), egui::PointerButton::Primary);
    f.click_text("Audio Settings");
    f.frame(vec![]);

    assert!(f.editor.audio_open);

    for text in [
        "Sample Rate",
        "48000 Hz",
        "Channels",
        "Buffer",
        "Latency",
        "0 samples",
    ] {
        assert!(
            f.shapes.iter().any(|s| matches!(&s.shape,
            egui::Shape::Text(t) if t.galley.job.text == text)),
            "Missing audio setting: {text}"
        );
    }
    f.click_text("Bypass");

    assert!(f.editor.params.bypass.value());
    assert_eq!(*f.host.events.lock().unwrap(), vec!["begin", "set", "end"]);
    f.editor.audio_open = false;
    f.frame(vec![]);
    f.click(Pos2::new(860.0, 32.0), egui::PointerButton::Primary);
    f.click_text("About");
    f.frame(vec![]);

    assert!(f.editor.about_open);
    assert!(f.shapes.iter().any(|s| matches!(&s.shape,
        egui::Shape::Text(t) if t.galley.job.text == "fl0lock...dev by sout")));
}

#[test]
fn header_menu_changes_ui_scale_without_editing_audio_parameters() {
    let mut f = Fixture::new();
    f.click(Pos2::new(860.0, 32.0), egui::PointerButton::Primary);
    f.click_text("UI Scale");
    f.frame(vec![]);

    let window = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 660.0));

    for text in ["75%", "90%", "100%", "110%", "125%", "150%", "175%", "200%"] {
        let item = f
            .shapes
            .iter()
            .find(|s| {
                matches!(&s.shape,
            egui::Shape::Text(t) if t.galley.job.text == text)
            })
            .expect("Missing scale preset");

        assert!(window.contains_rect(item.shape.visual_bounding_rect()));
    }
    f.click_text("125%");

    assert!(f.viewport_commands.iter().any(|command| matches!(command,
        egui::ViewportCommand::InnerSize(size) if *size == Vec2::new(900.0, 660.0))));
    f.frame(vec![]);

    assert_eq!(f.ctx.zoom_factor(), 1.25);
    assert!(f.host.events.lock().unwrap().is_empty());
    assert_eq!(f.editor.params.gain.value(), -6.0);
}

#[test]
fn notifications_popup_at_top_left_and_expire() {
    let mut f = Fixture::new();

    for _ in 1..BAND_COUNT {
        f.add();
    }
    f.add();
    f.frame(vec![]);
    let message = f
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == "All 8 points are in use" => {
                Some(s.shape.visual_bounding_rect())
            }
            _ => None,
        })
        .expect("Notification was not painted");

    assert!(
        message.left() < 40.0 && message.top() < 40.0,
        "Toast is not at the window's top left: {message:?}"
    );

    f.time += 4.1;
    f.frame(vec![]);

    assert!(f.editor.notice.is_none());
}
