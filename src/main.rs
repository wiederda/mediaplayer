mod app;
mod icons;
mod platform;
mod theme;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("MediaPlayer")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([760.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "MediaPlayer",
        options,
        Box::new(|cc| Ok(Box::new(app::MediaPlayerApp::new(cc)))),
    )
}