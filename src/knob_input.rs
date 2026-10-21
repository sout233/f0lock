use egui::{Color32, Response, Ui};
use nice_plug::{context::gui::ParamSetter, prelude::*};

#[derive(Clone)]
struct NumericEntry {
    binding: usize,
    text: String,
    error: Option<String>,
    focus: bool,
}

pub(super) fn value_text(param: &FloatParam, normalized: f32, label: &str) -> String {
    let value = param.preview_plain(normalized);

    if label == "F0 Ratio" {
        format!("× {value:.2}")
    } else if label == "Clarity" {
        format!("{:.2}%", value * 100.0)
    } else {
        format!("{value:.2}{}", param.unit())
    }
}

fn parse_value(param: &FloatParam, text: &str, factor: f32) -> Result<f32, String> {
    let value = text
        .trim()
        .parse::<f32>()
        .map_err(|_| "Enter a valid number".to_owned())?;

    if !value.is_finite() {
        return Err("Enter a finite number".to_owned());
    }

    let a = param.preview_plain(0.0) * factor;
    let b = param.preview_plain(1.0) * factor;
    let (min, max) = (a.min(b), a.max(b));

    if !(min..=max).contains(&value) {
        return Err(format!("Range: {min:.2} to {max:.2}"));
    }

    Ok(value / factor)
}

pub(super) fn edit_value(
    ui: &mut Ui,
    response: &Response,
    param: &FloatParam,
    setter: &ParamSetter<'_>,
    label: &str,
) -> bool {
    let state_id = response.id.with("numeric-entry");
    let popup_id = egui::Popup::default_response_id(response);
    let binding = param as *const FloatParam as usize;
    let factor = if label == "Clarity" { 100.0 } else { 1.0 };
    let mut entry = ui.data(|data| data.get_temp::<NumericEntry>(state_id));

    if response.secondary_clicked() {
        entry = Some(NumericEntry {
            binding,
            text: (param.value() * factor).to_string(),
            error: None,
            focus: true,
        });
    }

    let Some(mut entry) = entry else { return false };

    if entry.binding != binding {
        egui::Popup::close_id(ui.ctx(), popup_id);
        ui.data_mut(|data| data.remove::<NumericEntry>(state_id));
        return false;
    }

    let mut committed = false;

    egui::Popup::context_menu(response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .frame(egui::Frame::popup(ui.style()).fill(Color32::from_gray(38)))
        .width(200.0)
        .show(|ui| {
            ui.label(label);

            let mut edit = egui::TextEdit::singleline(&mut entry.text)
                .id(state_id.with("text"))
                .desired_width(180.0)
                .show(ui);

            if entry.focus && ui.is_visible() {
                edit.response.request_focus();
                edit.state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::select_all_str(&entry.text)));
                edit.state.store(ui.ctx(), edit.response.id);
                entry.focus = false;
            }

            let unit = if factor == 100.0 {
                "%"
            } else if label == "F0 Ratio" {
                "× F0"
            } else {
                param.unit().trim()
            };

            if !unit.is_empty() {
                ui.label(unit);
            }

            if let Some(error) = &entry.error {
                ui.colored_label(Color32::from_rgb(230, 92, 92), error);
            }

            let enter = edit.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let mut apply = enter;
            let mut cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));

            ui.horizontal(|ui| {
                apply |= ui.button("Apply").clicked();
                cancel |= ui.button("Cancel").clicked();
            });

            if cancel {
                egui::Popup::close_id(ui.ctx(), popup_id);
            } else if apply {
                match parse_value(param, &entry.text, factor) {
                    Ok(value) => {
                        setter.begin_set_parameter(param);
                        setter.set_parameter(param, value);
                        setter.end_set_parameter(param);
                        committed = true;
                        egui::Popup::close_id(ui.ctx(), popup_id);
                    }
                    Err(error) => entry.error = Some(error),
                }
            }
        });

    if egui::Popup::is_id_open(ui.ctx(), popup_id) {
        ui.data_mut(|data| data.insert_temp(state_id, entry));
    } else {
        ui.data_mut(|data| data.remove::<NumericEntry>(state_id));
    }

    committed
}
