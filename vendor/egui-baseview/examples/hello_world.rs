use egui::CentralPanel;
use egui_baseview::{EguiWindow, EguiWindowSettings, baseview::dpi::LogicalSize};

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
            .with_title("egui-baseview hello world")
            .with_size(LogicalSize {
                width: 300.0,
                height: 110.0,
            }),
        MyApp,
    )
    .unwrap()
    .run_until_closed()
    .unwrap();
}

struct MyApp;

impl egui_baseview::App for MyApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut egui_baseview::Frame) {
        CentralPanel::default().show(ui, |ui| {
            ui.label("Hello World!");
        });
    }
}
