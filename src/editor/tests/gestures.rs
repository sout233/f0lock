use super::*;

fn input_frame(
    ctx: &egui::Context,
    editor: &mut F0Editor,
    mut events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
) {
    events.insert(0, egui::Event::ModifiersChanged(modifiers));
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 724.0))),
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

    let center = Pos2::new(65.0, 465.0);

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
            vec![egui::Event::PointerMoved(Pos2::new(65.0, y))],
            modifiers,
        );
    }

    input_frame(
        &ctx,
        &mut editor,
        vec![egui::Event::PointerButton {
            pos: Pos2::new(65.0, 400.0),
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

    let pos = Pos2::new(65.0, 465.0);

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
