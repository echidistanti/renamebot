#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod app;
mod ui;

fn main() -> eframe::Result {
    // File e cartelle passati da riga di comando, come quando il sistema li apre con questo programma.
    let open: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1180.0, 760.0])
        .with_min_inner_size([960.0, 620.0])
        // La barra del titolo la disegna il programma, come in Camallo e Stiva.
        .with_decorations(false)
        .with_drag_and_drop(true)
        // Su Wayland collega la finestra a renamebot.desktop, e quindi all'icona.
        .with_app_id("renamebot");
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png")) {
        viewport = viewport.with_icon(icon);
    }
    eframe::run_native(
        "RenameBot",
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(move |cc| {
            ui::fonts(&cc.egui_ctx);
            ui::apply(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc, open)))
        }),
    )
}
