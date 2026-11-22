use eframe::egui;
use egui_sharkplayer::{PlayerState, SharkPlayer};
use std::path::{Path, PathBuf};

use crate::icons::{self, Icon, IconButton};
use crate::theme;

const MEDIA_EXTENSIONS: &[&str] = &[
    "mp3", "mp4", "mkv", "flac", "wav", "ogg", "m4a", "aac", "opus", "wma", "avi",
    "mov", "webm", "mpeg", "mpg", "wmv", "m4v",
];

struct MediaFile {
    path: PathBuf,
}

impl MediaFile {
    fn name(&self) -> String {
        self.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
    }

    fn is_video(&self) -> bool {
        matches!(self.path.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase().as_str(),
            "mp4" | "mkv" | "avi" | "mov" | "webm" | "mpeg" | "mpg" | "wmv" | "m4v")
    }
}

pub struct MediaPlayerApp {
    player: Option<PlayerState>,
    player_error: Option<String>,
    playlist_visible: bool,
    playlist: Vec<MediaFile>,
    selected: Option<usize>,
    requested_path: Option<PathBuf>,
    loaded_path: Option<PathBuf>,
    status_message: Option<String>,
}

impl MediaPlayerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        crate::platform::enable_dark_titlebar(cc);

        let (player, player_error) = match PlayerState::new(cc) {
            Ok(player) => (Some(player), None),
            Err(error) => (None, Some(error.to_string())),
        };

        Self {
            player,
            player_error,
            playlist_visible: true,
            playlist: Vec::new(),
            selected: None,
            requested_path: None,
            loaded_path: None,
            status_message: None,
        }
    }

    fn is_media_file(path: &Path) -> bool {
        path.is_file() && path.extension().and_then(|ext| ext.to_str())
            .map(|ext| MEDIA_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
            .unwrap_or(false)
    }

    fn add_files(&mut self, paths: Vec<PathBuf>) {
        let mut added = 0;
        for path in paths {
            if !Self::is_media_file(&path) { continue; }
            let Ok(path) = path.canonicalize() else { continue; };
            if self.playlist.iter().any(|file| file.path == path) { continue; }
            self.playlist.push(MediaFile { path });
            added += 1;
        }
        self.playlist.sort_by_key(|file| file.name().to_lowercase());
        if self.selected.is_none() && !self.playlist.is_empty() { self.selected = Some(0); }
        self.status_message = Some(format!("{added} Datei(en) hinzugefügt"));
    }

    fn add_folder(&mut self, folder: &Path) {
        let entries = match std::fs::read_dir(folder) {
            Ok(entries) => entries,
            Err(error) => { self.status_message = Some(format!("Ordnerfehler: {error}")); return; }
        };
        let files = entries.flatten().map(|entry| entry.path())
            .filter(|path| Self::is_media_file(path)).collect();
        self.add_files(files);
    }

    fn open_files(&mut self) {
        if let Some(files) = rfd::FileDialog::new().add_filter("Mediendateien", MEDIA_EXTENSIONS).pick_files() {
            self.add_files(files);
        }
    }

    fn open_folder(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() { self.add_folder(&folder); }
    }

    fn play_selected(&mut self) {
        let Some(index) = self.selected else { return; };
        let Some(file) = self.playlist.get(index) else { return; };
        let path = file.path.clone();
        if self.requested_path.as_ref() == Some(&path) { return; }
        self.requested_path = Some(path.clone());

        let Some(player) = self.player.as_ref() else {
            self.status_message = self.player_error.clone();
            return;
        };
        let path_string = path.to_string_lossy().into_owned();
        match player.load_file(&path_string) {
            Ok(()) => { self.loaded_path = Some(path); self.status_message = None; }
            Err(error) => { self.loaded_path = None; self.status_message = Some(format!("Wiedergabefehler: {error}")); }
        }
    }

    fn select_relative(&mut self, offset: isize) {
        if self.playlist.is_empty() { return; }
        let current = self.selected.unwrap_or(0) as isize;
        self.selected = Some((current + offset).rem_euclid(self.playlist.len() as isize) as usize);
    }

    fn clear_playlist(&mut self) {
        self.playlist.clear();
        self.selected = None;
        self.requested_path = None;
        self.loaded_path = None;
        self.status_message = Some("Playlist geleert".into());
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        for dropped in ctx.input(|input| input.raw.dropped_files.clone()) {
            if let Some(path) = dropped.path {
                if path.is_dir() { self.add_folder(&path); } else { self.add_files(vec![path]); }
            }
        }
    }

    fn draw_header(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("header").exact_size(64.0)
            .frame(egui::Frame::new().fill(theme::BG_PANEL)
                .inner_margin(egui::Margin::symmetric(18, 0))
                .stroke(egui::Stroke::new(1.0, theme::BORDER)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let (logo_rect, _) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
                    ui.painter().rect_filled(logo_rect, egui::CornerRadius::same(10), theme::ACCENT);
                    icons::paint(ui.painter(), Icon::Play, logo_rect.shrink(9.0), egui::Color32::WHITE);
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new("MediaPlayer").font(theme::heading_font(21.0)).color(theme::TEXT));
                    ui.add_space(12.0);
                    icons::badge(ui, "AUDIO · VIDEO");

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::pill(ui, Icon::Playlist, "Playlist", self.playlist_visible).clicked() {
                            self.playlist_visible = !self.playlist_visible;
                        }
                        ui.add_space(8.0);
                        if ui.add_sized(
                            egui::vec2(148.0, 38.0),
                            egui::Button::new(
                                egui::RichText::new("＋ Dateien öffnen")
                                    .size(14.0)
                                    .strong()
                                    .color(theme::TEXT),
                            )
                            .fill(theme::BG_CARD)
                            .stroke(egui::Stroke::new(1.0, theme::BORDER))
                            .corner_radius(egui::CornerRadius::same(9)),
                        ).clicked() {
                            self.open_files();
                        }
                        ui.add_space(8.0);
                        if ui.add(IconButton::new(Icon::Next, "Nächster Titel").size(egui::vec2(38.0, 38.0))).clicked() {
                            self.select_relative(1);
                        }
                        if ui.add(IconButton::new(Icon::Previous, "Vorheriger Titel").size(egui::vec2(38.0, 38.0))).clicked() {
                            self.select_relative(-1);
                        }
                    });
                });
            });
    }

    fn draw_playlist(&mut self, ui: &mut egui::Ui) {
        if !self.playlist_visible { return; }
        egui::Panel::right("playlist").resizable(true).default_size(290.0).min_size(230.0).max_size(460.0)
            .frame(egui::Frame::new().fill(theme::BG_PANEL)
                .inner_margin(egui::Margin::same(16))
                .stroke(egui::Stroke::new(1.0, theme::BORDER)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Playlist").font(theme::heading_font(19.0)).color(theme::TEXT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(format!("{} Titel", self.playlist.len())).size(13.0).color(theme::TEXT_MUTED));
                    });
                });
                ui.add_space(2.0);
                ui.separator();

                if self.playlist.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(76.0, 76.0), egui::Sense::hover());
                            ui.painter().circle_filled(rect.center(), 38.0, theme::BG_CARD);
                            ui.painter().circle_stroke(rect.center(), 38.0, egui::Stroke::new(1.0, theme::BORDER));
                            icons::paint(ui.painter(), Icon::Music, rect.shrink(24.0), theme::TEXT_MUTED);
                            ui.add_space(12.0);
                            ui.label(egui::RichText::new("Noch keine Medien").color(theme::TEXT));
                            ui.label(egui::RichText::new("Füge Audio- oder Videodateien hinzu.").size(13.0).color(theme::TEXT_MUTED));
                        });
                    });
                } else {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        for (index, file) in self.playlist.iter().enumerate() {
                            let selected = self.selected == Some(index);
                            let icon = if file.is_video() { Icon::Play } else { Icon::Music };
                            ui.horizontal(|ui| {
                                let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 30.0), egui::Sense::hover());
                                icons::paint(ui.painter(), icon, icon_rect.shrink(2.0),
                                    if selected { theme::ACCENT } else { theme::TEXT_MUTED });
                                if ui.selectable_label(selected, file.name()).clicked() {
                                    self.selected = Some(index);
                                }
                            });
                        }
                    });
                }

                ui.add_space(8.0);
                ui.separator();
                if icons::wide_button(ui, Icon::Plus, "Dateien hinzufügen", true).clicked() { self.open_files(); }
                if icons::wide_button(ui, Icon::Folder, "Ordner hinzufügen", true).clicked() { self.open_folder(); }
                if !self.playlist.is_empty() {
                    ui.add_space(4.0);
                    if ui.button(egui::RichText::new("Playlist leeren").color(theme::TEXT_MUTED)).clicked() { self.clear_playlist(); }
                }
            });
    }

    fn draw_main(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG_APP).inner_margin(egui::Margin::same(24)))
            .show(ui, |ui| {
                if self.loaded_path.is_some() {
                    if let Some(player) = self.player.as_mut() {
                        ui.add(SharkPlayer::new(player).bar_height(48.0).skip_seconds(10.0));
                    }
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            let compact = ui.available_height() < 360.0;
                            let diameter = if compact { 96.0 } else { 140.0 };
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(diameter, diameter), egui::Sense::hover());
                            ui.painter().circle_filled(rect.center(), diameter * 0.5, theme::BG_CARD);
                            ui.painter().circle_stroke(rect.center(), diameter * 0.5, egui::Stroke::new(1.0, theme::BORDER));
                            icons::paint(ui.painter(), Icon::Play, rect.shrink(diameter * 0.29), theme::TEXT_MUTED);
                            ui.add_space(20.0);
                            let heading = if self.player_error.is_some() { "Wiedergabe nicht verfügbar" } else { "Noch keine Wiedergabe" };
                            ui.label(egui::RichText::new(heading).font(theme::heading_font(22.0)).color(theme::TEXT));
                            ui.add_space(6.0);
                            if let Some(error) = &self.player_error {
                                ui.label(egui::RichText::new(error).size(13.0).color(theme::TEXT_MUTED));
                            } else {
                                ui.label(egui::RichText::new("Wähle einen Titel aus der Playlist oder füge Medien hinzu.").size(15.0).color(theme::TEXT_MUTED));
                            }
                            ui.add_space(18.0);
                            if icons::wide_button(ui, Icon::Plus, "Dateien öffnen", true).clicked() { self.open_files(); }
                        });
                    });
                }
            });
    }
}

impl eframe::App for MediaPlayerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_dropped_files(&ctx);
        self.play_selected();
        self.draw_header(ui);
        self.draw_playlist(ui);
        self.draw_main(ui);

        if let Some(message) = &self.status_message {
            egui::Area::new(egui::Id::new("status_message"))
                .anchor(egui::Align2::LEFT_BOTTOM, [16.0, -16.0])
                .show(&ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| { ui.label(message); });
                });
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(player) = self.player.as_mut() { player.destroy_gl_resources(); }
    }
}
