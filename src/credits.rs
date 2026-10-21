use super::*;
use std::io::Cursor;

impl F0Editor {
    pub(super) fn show_about(&mut self, ctx: &egui::Context) {
        egui::Window::new("f0lock")
            .open(&mut self.about_open)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .default_width(330.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.about_credits, false, "About");
                    ui.selectable_value(&mut self.about_credits, true, "鸣谢");
                });
                ui.separator();
                ui.add_space(8.0);

                if !self.about_credits {
                    ui.label(egui::RichText::new("fl0lock...dev by sout").size(16.0));
                    ui.add_space(8.0);
                    return;
                }

                ui.label("Icewatermelon233（谭荣熙竣）");
                ui.add_space(8.0);
                ui.label("网友八顺千");
                ui.add_space(6.0);

                let image = self.credits_image.get_or_insert_with(|| {
                    let mut decoder = png::Decoder::new(Cursor::new(include_bytes!(
                        "../assets/credits/bashunqian.png"
                    )));
                    decoder.set_transformations(
                        png::Transformations::EXPAND | png::Transformations::STRIP_16,
                    );
                    let mut reader = decoder.read_info().expect("Invalid embedded credits PNG");
                    let mut bytes = vec![
                        0;
                        reader
                            .output_buffer_size()
                            .expect("Credits image too large")
                    ];
                    let info = reader
                        .next_frame(&mut bytes)
                        .expect("Could not decode credits PNG");
                    let size = [info.width as usize, info.height as usize];
                    let pixels = &bytes[..info.buffer_size()];
                    let image = match info.color_type {
                        png::ColorType::Rgb => egui::ColorImage::from_rgb(size, pixels),
                        png::ColorType::Rgba => {
                            egui::ColorImage::from_rgba_unmultiplied(size, pixels)
                        }
                        kind => panic!("Unsupported credits image color type: {kind:?}"),
                    };
                    ctx.load_texture("bashunqian-credits", image, egui::TextureOptions::LINEAR)
                });
                let width = ui.available_width().min(330.0);
                let size = image.size_vec2();
                ui.add(egui::Image::new(&*image).fit_to_exact_size(size * (width / size.x)));
                ui.add_space(8.0);
            });
    }
}
