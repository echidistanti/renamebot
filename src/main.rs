#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod app;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 650.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Bobina",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
