use super::*;
#[cfg(target_os = "windows")]
#[path = "native_editor_tests.rs"]
mod native;
use nice_plug::{
    context::{PluginApi, gui::GuiContextInner},
    params::internals::ParamPtr,
    prelude::PluginState,
};
use serde_json::json;
use std::sync::Mutex;

#[derive(Default)]
pub(super) struct RecordingHost {
    pub(super) events: Mutex<Vec<&'static str>>,
}
impl GuiContextInner for RecordingHost {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Clap
    }

    unsafe fn raw_begin_set_parameter(&self, _: ParamPtr) {
        self.events.lock().unwrap().push("begin");
    }

    unsafe fn raw_set_parameter_normalized(&self, param: ParamPtr, value: f32) {
        self.events.lock().unwrap().push("set");
        unsafe {
            param._internal_set_normalized_value(value);
        }
    }

    unsafe fn raw_end_set_parameter(&self, _: ParamPtr) {
        self.events.lock().unwrap().push("end");
    }

    fn get_state(&self) -> PluginState {
        panic!("Unexpected state read");
    }

    fn set_state(&self, _: PluginState) {
        panic!("Unexpected state write");
    }

    fn request_restart(&self) {
        panic!("Unexpected restart");
    }
}

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

    let pos = Pos2::new(337.5, 424.5);
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

    let entry_pos = Pos2::new(65.0, 602.0);
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
        (12.0, vec![], None),
        (13.0, vec![], Some("credits")),
        (14.0, vec![], None),
        (15.0, vec![], Some("dynamics")),
    ] {
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
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 684.0))),
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
                        ui.min_rect().bottom() <= 684.0 && ui.min_rect().right() <= 900.0,
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
                    "width": 900, "height": 684, "textures": textures, "meshes": meshes
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

fn input_frame(
    ctx: &egui::Context,
    editor: &mut F0Editor,
    mut events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
) {
    events.insert(0, egui::Event::ModifiersChanged(modifiers));
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 684.0))),
            events,
            ..Default::default()
        },
        |ui| editor.draw(ui),
    );
    output.textures_delta.clear();
}

fn drag_gain(fine: bool) -> f32 {
    let ctx = egui::Context::default();
    configure_style(&ctx);

    let params = Arc::new(F0Params::default());
    let host = Arc::new(RecordingHost::default());

    let mut editor = F0Editor::new(params.clone(), Arc::new(Telemetry::default()));
    editor.gui = Some(GuiContext::new(host.clone()));

    let modifiers = egui::Modifiers {
        shift: fine,
        ..Default::default()
    };

    let center = Pos2::new(114.0, 440.0);

    input_frame(&ctx, &mut editor, vec![], modifiers);

    input_frame(
        &ctx,
        &mut editor,
        vec![
            egui::Event::PointerMoved(center),
            egui::Event::PointerButton {
                pos: center,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers,
            },
        ],
        modifiers,
    );

    for y in [420.0, 400.0] {
        input_frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerMoved(Pos2::new(114.0, y))],
            modifiers,
        );
    }

    input_frame(
        &ctx,
        &mut editor,
        vec![egui::Event::PointerButton {
            pos: Pos2::new(114.0, 400.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        }],
        modifiers,
    );

    let recorded = host.events.lock().unwrap();

    assert_eq!(recorded.first(), Some(&"begin"));
    assert_eq!(recorded.last(), Some(&"end"));
    assert_eq!(recorded.iter().filter(|&&e| e == "begin").count(), 1);
    assert_eq!(recorded.iter().filter(|&&e| e == "end").count(), 1);
    assert!(recorded.contains(&"set"));
    params.active().gain.value() + 6.0
}

#[test]
fn knobs_report_host_gestures_and_shift_drag_is_ten_times_finer() {
    let normal = drag_gain(false);
    let fine = drag_gain(true);

    assert!(normal > 0.0 && fine > 0.0);
    assert!((normal / fine - 10.0).abs() < 0.01);
}

#[test]
fn control_click_resets_knob_through_host_gesture() {
    let ctx = egui::Context::default();
    configure_style(&ctx);

    let params = Arc::new(F0Params::default());
    let host = Arc::new(RecordingHost::default());

    let gui = GuiContext::new(host.clone());
    let setter = gui.param_setter();

    setter.begin_set_parameter(&params.active().gain);
    setter.set_parameter(&params.active().gain, 12.0);
    setter.end_set_parameter(&params.active().gain);

    host.events.lock().unwrap().clear();

    let mut editor = F0Editor::new(params.clone(), Arc::new(Telemetry::default()));
    editor.gui = Some(gui);

    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };

    let pos = Pos2::new(114.0, 440.0);

    input_frame(&ctx, &mut editor, vec![], modifiers);

    input_frame(
        &ctx,
        &mut editor,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers,
            },
        ],
        modifiers,
    );

    input_frame(
        &ctx,
        &mut editor,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        }],
        modifiers,
    );

    assert_eq!(params.active().gain.value(), -6.0);
    assert_eq!(*host.events.lock().unwrap(), vec!["begin", "set", "end"]);
}
