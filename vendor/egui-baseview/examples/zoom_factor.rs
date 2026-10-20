use egui::CentralPanel;
use egui_baseview::{EguiWindow, EguiWindowSettings, baseview::dpi::LogicalSize};

const WINDOW_SIZE: LogicalSize<f32> = LogicalSize::new(300.0, 250.0);
const MIN_WINDOW_SIZE: LogicalSize<f32> = LogicalSize::new(200.0, 150.0);

fn main() {
    // Used as a workaround for https://github.com/RustAudio/baseview/issues/321
    //
    // # Safety
    //
    // ONLY call this when running the application in standalone mode. Do *NOT* call this
    // when running as a plugin that is loaded into a host. For more information, see:
    // https://docs.rs/baseview/0.3.4/baseview/fn.assume_standalone_in_process.html
    unsafe { baseview::assume_standalone_in_process() };

    EguiWindow::create(
        EguiWindowSettings::new()
            .with_title("egui-baseview zoom factor")
            .with_size(WINDOW_SIZE)
            .with_min_size::<LogicalSize<f32>>(Some(MIN_WINDOW_SIZE))
            .with_resizable(true),
        MyApp { zoom_factor: 1.0 },
    )
    .unwrap()
    .run_until_closed()
    .unwrap();
}

struct MyApp {
    zoom_factor: f32,
}

impl egui_baseview::App for MyApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut egui_baseview::Frame) {
        CentralPanel::default().show(ui, |ui| {
            ui.label("Zoomin'!");

            let before = self.zoom_factor;
            egui::ComboBox::from_label("zoom factor")
                .selected_text(format!("{}%", (self.zoom_factor * 100.0).round() as u32))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.zoom_factor, 0.5, "50%");
                    ui.selectable_value(&mut self.zoom_factor, 0.75, "75%");
                    ui.selectable_value(&mut self.zoom_factor, 1.0, "100%");
                    ui.selectable_value(&mut self.zoom_factor, 1.25, "125%");
                    ui.selectable_value(&mut self.zoom_factor, 1.5, "150%");
                    ui.selectable_value(&mut self.zoom_factor, 1.75, "175%");
                    ui.selectable_value(&mut self.zoom_factor, 2.0, "200%");
                });
            if self.zoom_factor != before {
                ui.set_zoom_factor(self.zoom_factor);
            }
        });
    }
}
