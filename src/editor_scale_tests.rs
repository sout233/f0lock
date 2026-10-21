use super::*;
use std::sync::atomic::Ordering;

#[test]
fn chosen_ui_scale_survives_editor_recreation_and_project_state_restore() {
    let mut f = Fixture::new();
    f.click(Pos2::new(860.0, 32.0), egui::PointerButton::Primary);
    f.click_text("UI Scale");
    f.click_text("150%");
    f.frame(vec![]);

    assert_eq!(f.ctx.zoom_factor(), 1.5);
    assert_eq!(
        f.editor.params.ui_scale_percent.load(Ordering::Relaxed),
        150
    );
    assert!(f.host.events.lock().unwrap().is_empty());

    let saved = f.editor.params.serialize_fields();
    assert_eq!(saved["ui_scale_percent"], "150");

    // A newly created context models closing and reopening the editor.
    f.ctx = egui::Context::default();
    configure_style(&f.ctx);
    f.frame(vec![]);
    assert!(f.viewport_commands.iter().any(|command| matches!(command,
        egui::ViewportCommand::InnerSize(size) if *size == Vec2::new(900.0, 684.0))));
    f.frame(vec![]);
    assert_eq!(f.ctx.zoom_factor(), 1.5);

    // A new parameter object models loading the saved project into a new instance.
    let params = Arc::new(F0Params::default());
    params.deserialize_fields(&saved);
    f.editor = F0Editor::new(params, Arc::new(Telemetry::default()));
    f.editor.gui = Some(GuiContext::new(f.host.clone()));
    f.ctx = egui::Context::default();
    configure_style(&f.ctx);
    f.frame(vec![]);
    f.frame(vec![]);

    assert_eq!(f.ctx.zoom_factor(), 1.5);
    assert_eq!(f.editor.params.param_map().len(), 125);
    assert_eq!(f.editor.params.active().gain.value(), -6.0);
}

#[test]
fn repeated_editor_reopens_do_not_multiply_the_saved_scale() {
    let mut f = Fixture::new();
    f.editor
        .params
        .ui_scale_percent
        .store(125, Ordering::Relaxed);

    for _ in 0..3 {
        f.ctx = egui::Context::default();
        configure_style(&f.ctx);
        f.frame(vec![]);
        f.frame(vec![]);

        assert_eq!(f.ctx.zoom_factor(), 1.25);
        assert_eq!(
            f.editor.params.ui_scale_percent.load(Ordering::Relaxed),
            125
        );
    }

    f.editor
        .params
        .ui_scale_percent
        .store(75, Ordering::Relaxed);
    f.frame(vec![]);
    f.frame(vec![]);

    assert_eq!(f.ctx.zoom_factor(), 0.75);
}

#[test]
fn invalid_saved_ui_scale_is_reported_without_applying_it() {
    let mut f = Fixture::new();
    f.editor.params.ui_scale_percent.store(0, Ordering::Relaxed);
    f.frame(vec![]);

    assert_eq!(f.ctx.zoom_factor(), 1.0);
    assert!(f.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.job.text == "Invalid saved UI Scale: 0%")));
    assert_eq!(f.editor.params.ui_scale_percent.load(Ordering::Relaxed), 0);
}
