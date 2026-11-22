//! Zentrale Gestaltung der Oberfläche: Farben, Schriften und Abstände.
//!
//! Die Schrift wird – wenn vorhanden – aus dem Betriebssystem geladen.
//! Dadurch ist der Text deutlich besser lesbar als mit der sehr dünnen
//! Standardschrift von egui. Fehlt eine Systemschrift, bleibt der
//! egui-Standard als Rückfallebene aktiv.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke,
    TextStyle, Vec2,
};

/// Kräftige Akzentfarbe für aktive Elemente und den Wiedergabeknopf.
pub const ACCENT: Color32 = Color32::from_rgb(0x4f, 0x8c, 0xff);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x6d, 0xa2, 0xff);

/// Hintergründe von außen nach innen.
pub const BG_APP: Color32 = Color32::from_rgb(0x0e, 0x11, 0x16);
pub const BG_PANEL: Color32 = Color32::from_rgb(0x14, 0x18, 0x21);
pub const BG_CARD: Color32 = Color32::from_rgb(0x1c, 0x22, 0x30);
pub const BG_HOVER: Color32 = Color32::from_rgb(0x26, 0x2e, 0x3f);
pub const BORDER: Color32 = Color32::from_rgb(0x2a, 0x32, 0x44);

/// Textfarben mit ausreichendem Kontrast auf allen Hintergründen.
pub const TEXT: Color32 = Color32::from_rgb(0xf2, 0xf5, 0xfa);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0xae, 0xb9, 0xcc);

/// Name der Schriftfamilie für Überschriften (halbfett, falls vorhanden).
const HEADING_FAMILY: &str = "heading";

/// Überschriftenschrift in der gewünschten Größe.
pub fn heading_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(HEADING_FAMILY.into()))
}

/// Wendet Schriften und Stil einmalig auf den egui-Kontext an.
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);

    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(26.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(17.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(16.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(13.5, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(15.0, FontFamily::Monospace),
            ),
        ]
        .into();

        // Großzügigere Abstände und Schaltflächen.
        style.spacing.item_spacing = Vec2::new(10.0, 10.0);
        style.spacing.button_padding = Vec2::new(16.0, 9.0);
        style.spacing.interact_size = Vec2::new(44.0, 36.0);
        style.spacing.slider_width = 180.0;
        style.spacing.indent = 18.0;
        style.spacing.menu_margin = Margin::same(8);
        style.spacing.window_margin = Margin::same(12);

        style.visuals = visuals();
    });
}

fn visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();

    visuals.panel_fill = BG_PANEL;
    visuals.window_fill = BG_PANEL;
    visuals.faint_bg_color = BG_CARD;
    visuals.extreme_bg_color = Color32::from_rgb(0x09, 0x0b, 0x0f);
    visuals.code_bg_color = BG_CARD;
    visuals.window_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.window_corner_radius = CornerRadius::same(14);
    visuals.menu_corner_radius = CornerRadius::same(12);
    visuals.hyperlink_color = ACCENT_HOVER;
    visuals.slider_trailing_fill = true;

    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);

    let radius = CornerRadius::same(10);

    visuals.widgets.noninteractive.bg_fill = BG_CARD;
    visuals.widgets.noninteractive.weak_bg_fill = BG_CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.noninteractive.corner_radius = radius;

    visuals.widgets.inactive.bg_fill = BG_CARD;
    visuals.widgets.inactive.weak_bg_fill = BG_CARD;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.2_f32, TEXT);
    visuals.widgets.inactive.corner_radius = radius;

    visuals.widgets.hovered.bg_fill = BG_HOVER;
    visuals.widgets.hovered.weak_bg_fill = BG_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(0x3a, 0x46, 0x5e));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.4_f32, Color32::WHITE);
    visuals.widgets.hovered.corner_radius = radius;

    visuals.widgets.active.bg_fill = BG_HOVER;
    visuals.widgets.active.weak_bg_fill = BG_HOVER;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.4_f32, Color32::WHITE);
    visuals.widgets.active.corner_radius = radius;

    visuals.widgets.open.bg_fill = BG_HOVER;
    visuals.widgets.open.weak_bg_fill = BG_HOVER;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(0x3a, 0x46, 0x5e));
    visuals.widgets.open.fg_stroke = Stroke::new(1.4_f32, Color32::WHITE);
    visuals.widgets.open.corner_radius = radius;

    visuals
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    let mut proportional = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    if let Some(bytes) = read_first(&font_candidates(true)) {
        fonts.font_data.insert(
            "ui-regular".to_owned(),
            Arc::new(FontData::from_owned(bytes)),
        );
        proportional.insert(0, "ui-regular".to_owned());
    }

    let heading = if let Some(bytes) = read_first(&font_candidates(false)) {
        fonts
            .font_data
            .insert("ui-bold".to_owned(), Arc::new(FontData::from_owned(bytes)));
        let mut family = vec!["ui-bold".to_owned()];
        family.extend(proportional.iter().cloned());
        family
    } else {
        proportional.clone()
    };

    fonts
        .families
        .insert(FontFamily::Proportional, proportional);
    fonts
        .families
        .insert(FontFamily::Name(HEADING_FAMILY.into()), heading);

    ctx.set_fonts(fonts);
}

fn read_first(paths: &[String]) -> Option<Vec<u8>> {
    paths.iter().find_map(|path| std::fs::read(path).ok())
}

/// Kandidaten für eine gut lesbare Systemschrift, plattformabhängig sortiert.
fn font_candidates(regular: bool) -> Vec<String> {
    let windows_dir = std::env::var("WINDIR").unwrap_or_else(|_| "C:/Windows".to_owned());

    let windows_files: &[&str] = if regular {
        &["segoeui.ttf", "arial.ttf", "tahoma.ttf", "verdana.ttf"]
    } else {
        &["seguisb.ttf", "segoeuib.ttf", "arialbd.ttf", "tahomabd.ttf"]
    };

    let linux_paths: &[&str] = if regular {
        &[
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
        ]
    } else {
        &[
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
            "/usr/share/fonts/truetype/ubuntu/Ubuntu-B.ttf",
        ]
    };

    let mut paths: Vec<String> = windows_files
        .iter()
        .map(|file| format!("{windows_dir}/Fonts/{file}"))
        .collect();
    paths.extend(linux_paths.iter().map(|path| (*path).to_owned()));
    paths
}
