use super::*;
use serde_json::json;

#[test]
fn editor_renders_real_signal_and_exports_meshes() {
    let ctx = egui::Context::default();
    configure_style(&ctx);
    let telemetry = Arc::new(Telemetry::default());

    let params = Arc::new(F0Params::default());

    let mut editor = F0Editor::new(params.clone(), telemetry.clone());
    let host = Arc::new(RecordingHost::default());
    editor.gui = Some(GuiContext::new(host.clone()));

    let gui = editor.gui.as_ref().unwrap();
    let setter = gui.param_setter();

    for (index, ratio, gain, kind) in [
        (1, 2.0, 3.0, FilterKind::Peak),
        (2, 3.0, -3.0, FilterKind::Peak),
    ] {
        let band = params.band(index);
        set_value(&setter, band.exists, true);
        set_value(&setter, band.ratio, ratio);
        set_value(&setter, band.gain, gain);
        set_value(&setter, band.kind, kind);
    }

    host.events.lock().unwrap().clear();
    editor.selected = Some(1);
    let initial_q = params.band(1).q.value();

    let controls = crate::engine::Controls {
        bands: std::array::from_fn(|i| params.band(i).control()),
        ..crate::engine::Controls::default()
    };

    let mut engine = crate::engine::Engine::new(48000.0);
    engine.visualize = true;
    // All plotted data comes from the production DSP.
    for i in 0..96000 {
        let hz = 146.83 * (1.0 + 0.012 * (i as f32 / 8000.0).sin());
        let phase = std::f32::consts::TAU * hz * i as f32 / 48000.0;
        let time = i as f32 / 48000.0;
        let x = 0.2 * phase.sin()
            + 0.08 * (phase * 2.0).sin()
            + 0.025 * (std::f32::consts::TAU * 8000.0 * time).sin()
            + 0.015 * (std::f32::consts::TAU * 14500.0 * time).sin();
        engine.process([x; 2], controls);
    }

    telemetry.publish(&engine);

    let mut textures = Vec::new();
    let mut meshes = Vec::new();

    let pos = Pos2::new(175.0, 465.0);
    let button = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };

    let menu_pos = Pos2::new(860.0, 32.0);
    let menu_button = |pressed| egui::Event::PointerButton {
        pos: menu_pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };

    let entry_pos = Pos2::new(65.0, 642.0);
    let entry_button = |pressed| egui::Event::PointerButton {
        pos: entry_pos,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: Default::default(),
    };

    for (time, events, snapshot) in [
        (0.0, vec![], None),
        (1.0, vec![], Some("idle")),
        (2.0, vec![egui::Event::PointerMoved(pos)], None),
        (3.0, vec![], Some("hover")),
        (4.0, vec![button(true)], None),
        (5.0, vec![], Some("pressed")),
        (6.0, vec![button(false), egui::Event::PointerGone], None),
        (7.0, vec![], None),
        (8.0, vec![egui::Event::PointerMoved(menu_pos)], None),
        (8.1, vec![menu_button(true)], None),
        (8.2, vec![menu_button(false)], None),
        (8.3, vec![], Some("menu")),
        (9.0, vec![egui::Event::PointerMoved(entry_pos)], None),
        (9.1, vec![entry_button(true)], None),
        (9.2, vec![entry_button(false)], None),
        (9.3, vec![], Some("numeric-entry")),
        (10.0, vec![], Some("target-range")),
        (11.0, vec![], Some("target-reference")),
        (11.5, vec![], None),
        (11.7, vec![], Some("about")),
        (12.0, vec![], None),
        (13.0, vec![], Some("credits")),
        (14.0, vec![], None),
        (15.0, vec![], Some("dynamics")),
        (16.0, vec![], Some("low-cut")),
        (17.0, vec![], Some("high-cut")),
        (
            18.0,
            vec![egui::Event::PointerMoved(Pos2::new(205.0, 373.0))],
            None,
        ),
        (
            18.1,
            vec![egui::Event::PointerButton {
                pos: Pos2::new(205.0, 373.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
            None,
        ),
        (
            18.2,
            vec![egui::Event::PointerButton {
                pos: Pos2::new(205.0, 373.0),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            None,
        ),
        (19.0, vec![], Some("cut-menu")),
        (20.0, vec![], Some("infinite-release")),
    ] {
        if time == 20.0 {
            egui::Popup::close_all(&ctx);
            let setter = editor.gui.as_ref().unwrap().param_setter();
            set_value(&setter, &params.active().infinite_release, true);
        }
        if time == 16.0 || time == 17.0 {
            let setter = editor.gui.as_ref().unwrap().param_setter();
            set_value(
                &setter,
                params.band(1).kind,
                if time == 16.0 {
                    FilterKind::Highpass
                } else {
                    FilterKind::Lowpass
                },
            );
            set_value(
                &setter,
                params.band(1).slope,
                if time == 16.0 {
                    crate::filter::CutSlope::Db24
                } else {
                    crate::filter::CutSlope::Db96
                },
            );
            let controls = crate::engine::Controls {
                bands: std::array::from_fn(|i| params.band(i).control()),
                ..crate::engine::Controls::default()
            };
            for i in 0..24000 {
                let x = 0.2 * (std::f32::consts::TAU * 146.83 * i as f32 / 48000.0).sin();
                engine.process([x; 2], controls);
            }
            telemetry.publish(&engine);
        }
        if time == 11.5 {
            editor.about_open = true;
            editor.about_credits = false;
        }
        if time == 14.0 {
            editor.about_open = false;
            let setter = editor.gui.as_ref().unwrap().param_setter();
            set_value(&setter, &params.band(1).dynamics.enabled, true);
        }
        if time == 12.0 {
            editor.about_open = true;
            editor.about_credits = true;
        }
        if time == 10.0 || time == 11.0 {
            egui::Popup::close_all(&ctx);
            let setter = editor.gui.as_ref().unwrap().param_setter();
            set_value(
                &setter,
                &params.active().target_mode,
                if time == 10.0 {
                    crate::pitch::TargetMode::Range
                } else {
                    crate::pitch::TargetMode::Reference
                },
            );
        }
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 724.0))),
            time: Some(time),
            events,
            ..Default::default()
        };

        let mut output = ctx.run_ui(input, |root| {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(Color32::from_rgb(15, 23, 30)))
                .show(root, |ui| {
                    editor.draw(ui);

                    assert!(
                        ui.min_rect().bottom() <= 724.0 && ui.min_rect().right() <= 900.0,
                        "Editor overflows: {:?}",
                        ui.min_rect()
                    );
                });
        });

        for (id, deltas) in &output.textures_delta.set {
            let egui::TextureId::Managed(id) = *id else {
                panic!("Unexpected external texture");
            };

            for delta in deltas {
                let egui::ImageData::Color(image) = &delta.image;
                textures.push(json!({ "id": id, "size": image.size, "pos": delta.pos,
                    "pixels": image.pixels.iter().map(|p| p.to_array()).collect::<Vec<_>>() }));
            }
        }
        output.textures_delta.clear();
        meshes.clear();

        for primitive in ctx.tessellate(output.shapes, output.pixels_per_point) {
            let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive else {
                panic!("Unexpected rendering callback");
            };

            let egui::TextureId::Managed(texture) = mesh.texture_id else {
                panic!("Unexpected external texture");
            };
            assert!(
                mesh.vertices
                    .iter()
                    .all(|v| v.pos.is_finite() && v.uv.is_finite())
            );
            let clip = primitive.clip_rect;
            meshes.push(json!({
                "clip": [clip.min.x, clip.min.y, clip.max.x, clip.max.y],
                "texture": texture,
                "indices": mesh.indices,
                "vertices": mesh.vertices.iter().map(|v| [v.pos.x, v.pos.y, v.uv.x, v.uv.y,
                    v.color.r() as f32, v.color.g() as f32, v.color.b() as f32, v.color.a() as f32]).collect::<Vec<_>>()
            }));
        }

        if let Some(snapshot) = snapshot {
            std::fs::create_dir_all("target/preview").unwrap();
            std::fs::write(
                format!("target/preview/{snapshot}.json"),
                serde_json::to_vec(&json!({
                    "width": 900, "height": 724, "textures": textures, "meshes": meshes
                }))
                .unwrap(),
            )
            .unwrap();
        }
    }

    assert!(!meshes.is_empty());
    assert!((params.band(1).q.value() - initial_q).abs() < 0.00001);
    let events = host.events.lock().unwrap();

    assert_eq!(
        events.iter().filter(|&&event| event == "begin").count(),
        events.iter().filter(|&&event| event == "end").count(),
        "Holding and releasing a knob must balance host gestures"
    );
    // Keep the default preview idle; hover/pressed exports share the same UTF-8 format.
    std::fs::copy("target/preview/idle.json", "target/preview/egui.json").unwrap();
}
