use baseview::WindowSize;
use egui::CentralPanel;
use egui_baseview::{
    EguiWindow, EguiWindowSettings, Frame, RepaintNotifier,
    baseview::{HandlerError, dpi::LogicalSize},
};

fn main() {
    // Used as a workaround for https://github.com/RustAudio/baseview/issues/321
    //
    // # Safety
    //
    // ONLY call this when running the application in standalone mode. Do *NOT* call this
    // when running as a plugin that is loaded into a host. For more information, see:
    // https://docs.rs/baseview/0.3.4/baseview/fn.assume_standalone_in_process.html
    unsafe { baseview::assume_standalone_in_process() };

    // A realtime-safe handle to request a repaint & update for an egui app.
    //
    // This can be used, for example, to notify the GUI that the value of a decibel
    // meter has changed.
    let repaint_notifier = RepaintNotifier::new();

    EguiWindow::create(
        EguiWindowSettings::new()
            .with_title("egui-baseview simple demo")
            .with_size(LogicalSize {
                width: 400.0,
                height: 200.0,
            })
            // A custom zoom factor can be set here. (The zoom factor can also be
            // changed later with `egui_ctx.set_zoom_factor()`.)
            .with_zoom_factor(1.0)
            .with_repaint_notifier(repaint_notifier.clone()),
        MyApp::new(),
    )
    .unwrap()
    .run_until_closed()
    .unwrap();
}

struct MyApp {
    pub name: String,
    pub age: u32,
}

impl MyApp {
    pub fn new() -> MyApp {
        MyApp {
            name: String::from(""),
            age: 30,
        }
    }
}

impl egui_baseview::App for MyApp {
    /// Called once before the first frame. Setup code such as `egui_ctx.set_fonts()`
    /// can be done here.
    ///
    /// If an error is returned, then the window will be closed.
    fn build(&mut self, _egui_ctx: egui::Context, _frame: &mut Frame) -> Result<(), HandlerError> {
        Ok(())
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut Frame) {
        CentralPanel::default().show(ui, |ui| {
            ui.heading("My Egui Application");
            ui.horizontal(|ui| {
                ui.label("Your name: ");
                ui.text_edit_singleline(&mut self.name);
            });
            ui.add(egui::Slider::new(&mut self.age, 0..=120).text("age"));
            if ui.button("Click each year").clicked() {
                self.age += 1;
            }
            ui.label(format!("Hello '{}', age {}", self.name, self.age));
            if ui.button("close window").clicked() {
                ui.send_viewport_cmd(egui::ViewportCommand::Close);
            }

            ui.hyperlink_to("free crouton", "https://crouton.net");
        });
    }

    /// Called when the window has been resized.
    ///
    /// The given size takes the zoom factor into account.
    fn resized(&mut self, size: WindowSize) {
        let _ = size;
    }

    /// Called when the zoom factor has changed.
    fn zoom_factor_changed(&mut self, zoom_factor: f32) {
        let _ = zoom_factor;
    }
}
