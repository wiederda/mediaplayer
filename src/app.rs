use eframe::egui;
use egui_sharkplayer::{ChapterInfo, DiscTitle, MediaTrack, PlayerState, SharkPlayer};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::cd;
use crate::icons::{self, Icon, IconButton};
use crate::ripper;
use crate::theme;

const MEDIA_EXTENSIONS: &[&str] = &[
    "mp3", "mp4", "mkv", "flac", "wav", "ogg", "m4a", "aac", "opus", "wma", "avi",
    "mov", "webm", "mpeg", "mpg", "wmv", "m4v", "iso",
];
#[derive(Clone, PartialEq, Eq)]
enum MediaSource {
    File(PathBuf),
    OpticalAuto,
    DvdDrive { device: Option<PathBuf> },
    BluRayDrive { device: Option<PathBuf> },
    AudioCd { drive: String },
}

impl MediaSource {
    fn is_dvd(&self) -> bool {
        match self {
            Self::DvdDrive { .. } => true,
            Self::BluRayDrive { .. } | Self::OpticalAuto => false,
            Self::File(path) => path.extension().and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("iso")),
            Self::AudioCd { .. } => false,
        }
    }

    fn is_audio_cd(&self) -> bool {
        matches!(self, Self::AudioCd { .. })
    }

    fn is_bluray(&self) -> bool {
        matches!(self, Self::BluRayDrive { .. })
    }

    fn is_disc(&self) -> bool {
        self.is_dvd() || self.is_bluray()
    }
}

struct MediaFile {
    source: MediaSource,
}

#[derive(Clone)]
struct SelectionOption {
    id: i64,
    label: String,
}

impl MediaFile {
    fn name(&self) -> String {
        match &self.source {
            MediaSource::File(path) => path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
            MediaSource::OpticalAuto => "Optisches Laufwerk".into(),
            MediaSource::DvdDrive { device } => device.as_ref().map(|path| format!("DVD · {}", path.display())).unwrap_or_else(|| "DVD-Laufwerk".into()),
            MediaSource::BluRayDrive { device } => device.as_ref().map(|path| format!("Blu-ray · {}", path.display())).unwrap_or_else(|| "Blu-ray-Laufwerk".into()),
            MediaSource::AudioCd { drive } if !drive.is_empty() => format!("Audio-CD · {drive}"),
            MediaSource::AudioCd { .. } => "Audio-CD".into(),
        }
    }

    fn is_video(&self) -> bool {
        match &self.source {
            MediaSource::OpticalAuto | MediaSource::DvdDrive { .. } | MediaSource::BluRayDrive { .. } => true,
            MediaSource::AudioCd { .. } => false,
            MediaSource::File(path) => matches!(path.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase().as_str(),
                "mp4" | "mkv" | "avi" | "mov" | "webm" | "mpeg" | "mpg" | "wmv" | "m4v" | "iso"),
        }
    }

    fn is_audio_cd(&self) -> bool {
        self.source.is_audio_cd()
    }
}

fn media_track_label(track: &MediaTrack) -> String {
    let mut details = Vec::new();
    if let Some(language) = &track.language { details.push(language.clone()); }
    if let Some(title) = &track.title { details.push(title.clone()); }
    if let Some(codec) = &track.codec { details.push(codec.clone()); }
    if details.is_empty() {
        format!("Track {}", track.id)
    } else {
        format!("Track {} · {}", track.id, details.join(" · "))
    }
}

fn selection_options(tracks: &[MediaTrack]) -> Vec<SelectionOption> {
    tracks.iter().map(|track| SelectionOption { id: track.id, label: media_track_label(track) }).collect()
}

fn disc_title_options(titles: &[DiscTitle]) -> Vec<SelectionOption> {
    titles.iter().map(|title| {
        let label = title.title.clone().filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| format!("DVD-Titel {}", title.id + 1));
        let label = if title.is_default { format!("{label} · Standard") } else { label };
        SelectionOption { id: title.id, label }
    }).collect()
}

fn chapter_options(chapters: &[ChapterInfo]) -> Vec<SelectionOption> {
    chapters.iter().map(|chapter| {
        let label = chapter.title.clone().filter(|name| !name.trim().is_empty())
            .map(|name| format!("Kapitel {} · {name}", chapter.index + 1))
            .unwrap_or_else(|| format!("Kapitel {}", chapter.index + 1));
        SelectionOption { id: chapter.index, label }
    }).collect()
}

fn cd_track_options(chapters: &[ChapterInfo]) -> Vec<SelectionOption> {
    chapters.iter().map(|chapter| {
        let label = chapter.title.clone().filter(|name| !name.trim().is_empty())
            .map(|name| format!("Track {} · {name}", chapter.index + 1))
            .unwrap_or_else(|| format!("Track {}", chapter.index + 1));
        SelectionOption { id: chapter.index, label }
    }).collect()
}

fn format_duration(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn resume_positions_file() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);

    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(PathBuf::from)
        .map(|path| path.join("Library").join("Application Support"));

    #[cfg(all(not(windows), not(target_os = "macos")))]
    let base = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from).map(|path| path.join(".local").join("state")));

    base.map(|path| path.join("MediaPlayer").join("resume.json"))
}

fn load_resume_positions() -> HashMap<String, f64> {
    let Some(path) = resume_positions_file() else { return HashMap::new(); };
    std::fs::read(path).ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

fn save_resume_positions(positions: &HashMap<String, f64>) {
    let Some(path) = resume_positions_file() else { return; };
    let Some(parent) = path.parent() else { return; };
    if std::fs::create_dir_all(parent).is_err() { return; }
    let Ok(data) = serde_json::to_vec(positions) else { return; };
    let _ = std::fs::write(path, data);
}

#[derive(Clone)]
struct PlaybackSettings {
    audio_language: String,
    subtitles_enabled: bool,
    subtitle_language: String,
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self { audio_language: "de,en".into(), subtitles_enabled: false, subtitle_language: "de,en".into() }
    }
}

fn playback_settings_file() -> Option<PathBuf> {
    resume_positions_file().and_then(|path| path.parent().map(|parent| parent.join("settings.json")))
}

fn load_playback_settings() -> PlaybackSettings {
    let mut settings = PlaybackSettings::default();
    let Some(path) = playback_settings_file() else { return settings; };
    let Ok(data) = std::fs::read(path) else { return settings; };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&data) else { return settings; };
    if let Some(language) = value.get("audio_language").and_then(|v| v.as_str()) { settings.audio_language = language.to_string(); }
    if let Some(enabled) = value.get("subtitles_enabled").and_then(|v| v.as_bool()) { settings.subtitles_enabled = enabled; }
    if let Some(language) = value.get("subtitle_language").and_then(|v| v.as_str()) { settings.subtitle_language = language.to_string(); }
    settings
}

fn save_playback_settings(settings: &PlaybackSettings) {
    let Some(path) = playback_settings_file() else { return; };
    let Some(parent) = path.parent() else { return; };
    if std::fs::create_dir_all(parent).is_err() { return; }
    let value = serde_json::json!({
        "audio_language": settings.audio_language,
        "subtitles_enabled": settings.subtitles_enabled,
        "subtitle_language": settings.subtitle_language,
    });
    if let Ok(data) = serde_json::to_vec_pretty(&value) { let _ = std::fs::write(path, data); }
}

#[derive(Clone)]
struct ResumePrompt {
    path: PathBuf,
    position_seconds: f64,
}

struct DuplicatePrompt {
    files: Vec<PathBuf>,
    replace_playlist: bool,
}

#[derive(Clone, Copy)]
enum DuplicateChoice {
    AddAll,
    AddNew,
    Cancel,
}

pub struct MediaPlayerApp {
    player: Option<PlayerState>,
    player_error: Option<String>,
    playlist_visible: bool,
    playlist: Vec<MediaFile>,
    selected: Option<usize>,
    direct_source: Option<MediaSource>,
    requested_source: Option<MediaSource>,
    loaded_source: Option<MediaSource>,
    load_pending: bool,
    audio_track_options: Vec<SelectionOption>,
    selected_audio_track: Option<i64>,
    subtitle_track_options: Vec<SelectionOption>,
    selected_subtitle_track: Option<i64>,
    disc_title_options: Vec<SelectionOption>,
    selected_disc_title: Option<i64>,
    chapter_options: Vec<SelectionOption>,
    selected_chapter: Option<i64>,
    audio_cd_title: Option<String>,
    last_disc_state_poll: std::time::Instant,
    cd_player: Option<cd::AudioCd>,
    cd_status: Option<cd::CdStatus>,
    cd_last_poll: std::time::Instant,
    status_message: Option<String>,
    settings_open: bool,
    playback_settings: PlaybackSettings,
    duplicate_prompt: Option<DuplicatePrompt>,
    playlist_drag_press: Option<(usize, std::time::Instant)>,
    playlist_drag_activated: Option<usize>,
    about_open: bool,
    resume_positions: HashMap<String, f64>,
    resume_prompt: Option<ResumePrompt>,
    last_resume_save: std::time::Instant,
    rip_window: Arc<Mutex<ripper::RipWindow>>,
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
            direct_source: None,
            requested_source: None,
            loaded_source: None,
            load_pending: false,
            audio_track_options: Vec::new(),
            selected_audio_track: None,
            subtitle_track_options: Vec::new(),
            selected_subtitle_track: None,
            disc_title_options: Vec::new(),
            selected_disc_title: None,
            chapter_options: Vec::new(),
            selected_chapter: None,
            audio_cd_title: None,
            last_disc_state_poll: std::time::Instant::now(),
            cd_player: None,
            cd_status: None,
            cd_last_poll: std::time::Instant::now(),
            status_message: None,
            settings_open: false,
            playback_settings: load_playback_settings(),
            duplicate_prompt: None,
            playlist_drag_press: None,
            playlist_drag_activated: None,
            about_open: false,
            resume_positions: load_resume_positions(),
            resume_prompt: None,
            last_resume_save: std::time::Instant::now(),
            rip_window: Arc::new(Mutex::new(ripper::RipWindow::new())),
        }
    }

    fn is_media_file(path: &Path) -> bool {
        path.is_file() && path.extension().and_then(|ext| ext.to_str())
            .map(|ext| MEDIA_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
            .unwrap_or(false)
    }

    fn add_files(&mut self, paths: Vec<PathBuf>) {
        self.prepare_files(paths, false);
    }

    fn prepare_files(&mut self, paths: Vec<PathBuf>, replace_playlist: bool) {
        let files = paths.into_iter()
            .filter(|path| Self::is_media_file(path))
            .filter_map(|path| path.canonicalize().ok())
            .collect::<Vec<_>>();
        let existing = if replace_playlist { HashSet::new() } else {
            self.playlist.iter().filter_map(|file| match &file.source {
                MediaSource::File(path) => Self::file_identity(path),
                _ => None,
            }).collect::<HashSet<_>>()
        };
        let mut seen = HashSet::new();
        let mut duplicates = Vec::new();
        let mut duplicate_keys = HashSet::new();
        for path in &files {
            let key = Self::file_identity(path).unwrap_or_else(|| path.to_string_lossy().into_owned());
            if (existing.contains(&key) || !seen.insert(key.clone())) && duplicate_keys.insert(key) {
                duplicates.push(path.clone());
            }
        }
        if !duplicates.is_empty() {
            self.duplicate_prompt = Some(DuplicatePrompt { files: duplicates, replace_playlist });
            return;
        }
        self.commit_files(files, false, replace_playlist);
    }

    fn commit_files(&mut self, files: Vec<PathBuf>, add_duplicates: bool, replace_playlist: bool) {
        if replace_playlist {
            if let Some(player) = self.player.as_ref() { let _ = player.stop(); }
            self.clear_playlist();
        }
        let mut existing = self.playlist.iter().filter_map(|file| match &file.source {
            MediaSource::File(path) => Self::file_identity(path),
            _ => None,
        }).collect::<HashSet<_>>();
        let mut seen = HashSet::new();
        let mut added = 0;
        let mut skipped = 0;
        for path in files {
            let key = Self::file_identity(&path).unwrap_or_else(|| path.to_string_lossy().into_owned());
            let duplicate = existing.contains(&key) || !seen.insert(key.clone());
            if duplicate && !add_duplicates {
                skipped += 1;
                continue;
            }
            self.playlist.push(MediaFile { source: MediaSource::File(path) });
            added += 1;
            existing.insert(key);
        }
        if self.selected.is_none() && !self.playlist.is_empty() { self.selected = Some(0); }
        self.status_message = Some(format!("{added} Datei(en) hinzugefügt; {skipped} Duplikat(e) übersprungen"));
    }

    fn resolve_duplicate_prompt(&mut self, choice: DuplicateChoice) {
        let Some(prompt) = self.duplicate_prompt.take() else { return; };
        match choice {
            DuplicateChoice::AddAll => self.commit_files(prompt.files, true, prompt.replace_playlist),
            DuplicateChoice::AddNew => self.commit_files(prompt.files, false, prompt.replace_playlist),
            DuplicateChoice::Cancel => self.status_message = Some("Hinzufügen abgebrochen".into()),
        }
    }

    fn remove_playlist_entry(&mut self, index: usize) {
        if index >= self.playlist.len() { return; }
        let was_selected = self.selected == Some(index);
        if was_selected {
            self.capture_current_resume_position();
            save_resume_positions(&self.resume_positions);
            if let Some(player) = self.player.as_ref() { let _ = player.stop(); }
            self.loaded_source = None;
            self.requested_source = None;
            self.load_pending = false;
            self.cd_player = None;
            self.cd_status = None;
        }
        self.playlist.remove(index);
        self.selected = match self.selected {
            Some(selected) if selected > index => Some(selected - 1),
            Some(selected) if selected == index => (!self.playlist.is_empty()).then_some(index.min(self.playlist.len() - 1)),
            selected => selected,
        };
        if self.playlist.is_empty() {
            self.requested_source = None;
            self.loaded_source = None;
            self.load_pending = false;
            self.audio_track_options.clear();
            self.selected_audio_track = None;
            self.subtitle_track_options.clear();
            self.selected_subtitle_track = None;
            self.disc_title_options.clear();
            self.selected_disc_title = None;
            self.chapter_options.clear();
            self.selected_chapter = None;
        }
        self.status_message = Some("Titel aus der Playlist entfernt".into());
    }

    fn file_identity(path: &Path) -> Option<String> {
        let path = path.canonicalize().ok()?;
        let identity = path.to_string_lossy().into_owned();
        #[cfg(windows)]
        { Some(identity.to_lowercase()) }
        #[cfg(not(windows))]
        { Some(identity) }
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

    fn save_playlist_m3u(&mut self) {
        let Some(mut path) = rfd::FileDialog::new()
            .add_filter("M3U-Playlist", &["m3u"])
            .set_file_name("Playlist.m3u")
            .save_file()
        else { return; };

        if path.extension().is_none() { path.set_extension("m3u"); }
        let mut contents = String::from("#EXTM3U\n");
        for file in &self.playlist {
            if let MediaSource::File(media_path) = &file.source {
                contents.push_str(&media_path.to_string_lossy());
                contents.push('\n');
            }
        }

        match std::fs::write(&path, contents) {
            Ok(()) => self.status_message = Some(format!("Playlist gespeichert: {}", path.display())),
            Err(error) => self.status_message = Some(format!("Playlist konnte nicht gespeichert werden: {error}")),
        }
    }

    fn load_playlist_m3u(&mut self) {
        let Some(playlist_path) = rfd::FileDialog::new()
            .add_filter("M3U-Playlist", &["m3u", "m3u8"])
            .pick_file()
        else { return; };
        let contents = match std::fs::read_to_string(&playlist_path) {
            Ok(contents) => contents,
            Err(error) => {
                self.status_message = Some(format!("Playlist konnte nicht gelesen werden: {error}"));
                return;
            }
        };
        let base = playlist_path.parent().unwrap_or_else(|| Path::new("."));
        let paths = contents.lines()
            .map(|line| line.trim().trim_start_matches('\u{feff}'))
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(PathBuf::from)
            .map(|path| if path.is_absolute() { path } else { base.join(path) })
            .collect::<Vec<_>>();

        self.prepare_files(paths, true);
    }

    fn capture_current_resume_position(&mut self) {
        if self.loaded_source.as_ref().is_some_and(MediaSource::is_disc) { return; }
        let Some(MediaSource::File(path)) = self.loaded_source.as_ref() else { return; };
        let key = path.to_string_lossy().into_owned();
        let current = self.player.as_ref().and_then(|player| {
            let position = player.time_pos().ok().flatten()?;
            let duration = player.duration().ok().flatten();
            Some((position, duration))
        });
        let Some((position, duration)) = current else { return; };

        if duration.is_some_and(|duration| duration > 0.0 && position >= duration - 3.0) {
            self.resume_positions.remove(&key);
        } else if position > 2.0 {
            self.resume_positions.insert(key, position);
        }
    }

    fn maybe_prompt_resume(&mut self) {
        if self.resume_prompt.is_some() { return; }
        if self.loaded_source.as_ref().is_some_and(MediaSource::is_disc) { return; }
        let Some(MediaSource::File(path)) = self.loaded_source.as_ref() else { return; };
        let key = path.to_string_lossy().into_owned();
        let Some(position_seconds) = self.resume_positions.get(&key).copied() else { return; };
        if position_seconds < 10.0 { return; }

        let before_end = self.player.as_ref().and_then(|player| player.duration().ok().flatten())
            .is_none_or(|duration| position_seconds < duration - 5.0);
        if !before_end { return; }

        if let Some(player) = self.player.as_ref() { let _ = player.pause(); }
        self.resume_prompt = Some(ResumePrompt { path: path.clone(), position_seconds });
    }

    fn resolve_resume_prompt(&mut self, resume: bool) {
        let Some(prompt) = self.resume_prompt.take() else { return; };
        if let Some(player) = self.player.as_ref() {
            let seek_to = if resume { prompt.position_seconds } else { 0.0 };
            if let Err(error) = player.seek_to(seek_to) {
                self.status_message = Some(format!("Wiedergabeposition konnte nicht gesetzt werden: {error}"));
            }
            if let Err(error) = player.play() {
                self.status_message = Some(format!("Wiedergabe konnte nicht fortgesetzt werden: {error}"));
            }
        }
        if !resume {
            self.resume_positions.remove(&prompt.path.to_string_lossy().into_owned());
            save_resume_positions(&self.resume_positions);
        }
    }

    fn save_resume_position_if_due(&mut self) {
        if self.last_resume_save.elapsed() < std::time::Duration::from_secs(5) { return; }
        self.last_resume_save = std::time::Instant::now();
        if self.resume_prompt.is_none() {
            self.capture_current_resume_position();
            save_resume_positions(&self.resume_positions);
        }
    }

    fn draw_resume_dialog(&mut self, ctx: &egui::Context) {
        let Some(prompt) = self.resume_prompt.as_ref() else { return; };
        let filename = prompt.path.file_name().map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| prompt.path.to_string_lossy().into_owned());
        let position = format_duration(prompt.position_seconds);
        let mut decision = None;
        egui::Window::new("Wiedergabe fortsetzen?")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .show(ctx, |ui| {
                ui.heading("Wiedergabe fortsetzen?");
                ui.add_space(6.0);
                ui.label(filename);
                ui.label(format!("Zuletzt bei {position} unterbrochen."));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Von Anfang an").clicked() { decision = Some(false); }
                    if ui.button("Fortsetzen").clicked() { decision = Some(true); }
                });
            });
        if let Some(resume) = decision { self.resolve_resume_prompt(resume); }
    }

    fn open_folder(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() { self.add_folder(&folder); }
    }

    fn open_optical_drive(&mut self) {
        self.cd_player = None;
        self.cd_status = None;
        self.selected = None;
        self.direct_source = Some(MediaSource::OpticalAuto);
        self.requested_source = None;
        self.loaded_source = None;
        self.load_pending = false;
        self.audio_track_options.clear();
        self.selected_audio_track = None;
        self.subtitle_track_options.clear();
        self.selected_subtitle_track = None;
        self.disc_title_options.clear();
        self.selected_disc_title = None;
        self.chapter_options.clear();
        self.selected_chapter = None;
        self.status_message = None;
    }

    fn open_rip_window(&mut self) {
        // Ein laufendes CD-Laufwerk kann nicht gleichzeitig über MCI
        // wiedergeben und über SCSI ausgelesen werden.
        if let Some(cd_player) = self.cd_player.as_mut() {
            let _ = cd_player.stop();
        }
        if let Ok(mut window) = self.rip_window.lock() {
            window.open();
        }
    }

    fn start_audio_cd(&mut self, source: MediaSource) {
        if self.selected.is_none() { self.direct_source = Some(source.clone()); }
        self.requested_source = Some(source.clone());
        self.loaded_source = None;
        self.load_pending = false;
        self.audio_track_options.clear();
        self.selected_audio_track = None;
        self.subtitle_track_options.clear();
        self.selected_subtitle_track = None;
        self.disc_title_options.clear();
        self.selected_disc_title = None;
        self.chapter_options.clear();
        self.selected_chapter = None;
        self.audio_cd_title = None;

        // Bevorzuge libmpv, falls CDDA unterstützt wird. MCI bleibt der
        // Windows-Fallback und liefert CD-Text über die native TOC-Abfrage.
        let mpv_supports_cd = self.player.as_ref().is_some_and(PlayerState::supports_audio_cd);
        let mci_error = if !mpv_supports_cd && self.cd_player.is_none() {
            match cd::AudioCd::open_default() {
                Ok(player) => { self.cd_player = Some(player); None }
                Err(error) => Some(error),
            }
        } else {
            None
        };

        if let Some(cd_player) = self.cd_player.as_mut() {
            if let Some(player) = self.player.as_ref() {
                let _ = player.stop();
            }

            let actual_source = MediaSource::AudioCd { drive: cd_player.drive_label().to_string() };
            if self.selected.is_none() { self.direct_source = Some(actual_source.clone()); }
            if let Some(index) = self.selected
                && let Some(file) = self.playlist.get_mut(index)
                && file.source.is_audio_cd()
            {
                file.source = actual_source.clone();
            }
            self.requested_source = Some(actual_source.clone());
            self.loaded_source = Some(actual_source);

            match cd_player.play_track(1) {
                Ok(()) => {
                    self.cd_status = cd_player.status().ok();
                    self.status_message = None;
                }
                Err(error) => {
                    self.cd_status = None;
                    self.status_message = Some(format!("Audio-CD konnte nicht gestartet werden: {error}"));
                }
            }
            self.playlist_visible = true;
            return;
        }

        let Some(player) = self.player.as_ref() else {
            self.status_message = self.player_error.clone();
            return;
        };

        if !player.supports_audio_cd() {
            #[cfg(windows)]
            let message = match mci_error {
                Some(error) => format!("Audio-CD konnte nicht geöffnet werden: {error}"),
                None => "Audio-CD-Wiedergabe wird von dieser libmpv-Version nicht unterstützt. Unter Linux/macOS benötigt libmpv libcdio (cdda://).".into(),
            };
            #[cfg(not(windows))]
            let message = {
                let _ = mci_error;
                "Audio-CD-Wiedergabe wird von dieser libmpv-Version nicht unterstützt. Installiere libmpv mit libcdio-Unterstützung (cdda://).".to_string()
            };
            self.status_message = Some(message);
            return;
        }

        if let Err(error) = player.configure_audio_cd_buffering() {
            self.status_message = Some(format!("CD-Pufferung oder CD-Text konnte nicht vollständig aktiviert werden: {error}"));
        }
        match player.load_file("cdda://") {
            Ok(()) => { self.load_pending = true; }
            Err(error) => {
                self.load_pending = false;
                self.status_message = Some(format!(
                    "Audio-CD konnte nicht geöffnet werden: {error}. Prüfe das Laufwerk, eine eingelegte Audio-CD und die libcdio-Unterstützung (cdda://) in libmpv."
                ));
            }
        }
    }

    fn play_selected(&mut self) {
        let source = if let Some(index) = self.selected {
            self.direct_source = None;
            let Some(source) = self.playlist.get(index).map(|file| file.source.clone()) else { return; };
            source
        } else if let Some(source) = self.direct_source.clone() {
            source
        } else {
            return;
        };
        let is_dvd = source.is_dvd();
        let is_bluray = source.is_bluray();
        if self.requested_source.as_ref() == Some(&source) { return; }
        self.capture_current_resume_position();
        save_resume_positions(&self.resume_positions);

        if source.is_audio_cd() {
            self.start_audio_cd(source);
            return;
        }

        if matches!(source, MediaSource::OpticalAuto) {
            #[cfg(windows)]
            if let Ok(cd_player) = cd::AudioCd::open_default() {
                drop(cd_player);
                self.start_audio_cd(MediaSource::AudioCd { drive: String::new() });
                return;
            }
            if let Some((is_bluray, device)) = cd::detect_video_disc() {
                let detected = if is_bluray { MediaSource::BluRayDrive { device: Some(device) } }
                    else { MediaSource::DvdDrive { device: Some(device) } };
                if self.selected.is_none() {
                    self.direct_source = Some(detected.clone());
                } else if let Some(index) = self.selected
                    && let Some(file) = self.playlist.get_mut(index) { file.source = detected; }
                self.requested_source = None;
                self.play_selected();
                return;
            }
            #[cfg(not(windows))]
            if self.player.as_ref().is_some_and(PlayerState::supports_audio_cd) {
                self.start_audio_cd(MediaSource::AudioCd { drive: String::new() });
                return;
            }
            self.requested_source = None;
            self.status_message = Some("Kein Medium im optischen Laufwerk erkannt. Unterstützt werden DVD, Blu-ray und Audio-CD.".into());
            return;
        }

        // Beim Wechsel von einer Audio-CD zu einer Datei wird das Laufwerk
        // freigegeben.
        self.cd_player = None;
        self.cd_status = None;

        self.requested_source = Some(source.clone());
        self.loaded_source = None;
        self.audio_track_options.clear();
        self.selected_audio_track = None;
        self.subtitle_track_options.clear();
        self.selected_subtitle_track = None;
        self.disc_title_options.clear();
        self.selected_disc_title = None;
        self.chapter_options.clear();
        self.selected_chapter = None;

        let Some(player) = self.player.as_ref() else {
            self.status_message = self.player_error.clone();
            return;
        };
        // Explicitly unload the previous media before loading the next one so
        // libmpv cannot keep displaying its embedded cover-art video track.
        let _ = player.stop();
        if let Err(error) = player.configure_playback_languages(
            &self.playback_settings.audio_language,
            &self.playback_settings.subtitle_language,
            self.playback_settings.subtitles_enabled,
        ) {
            self.status_message = Some(format!("Sprach- und Untertiteleinstellungen konnten nicht angewendet werden: {error}"));
        }
        let dvd_device = match &source {
            MediaSource::File(path) if is_dvd => Some(path.to_string_lossy().into_owned()),
            MediaSource::DvdDrive { device } => device.as_ref().map(|path| path.to_string_lossy().into_owned()),
            _ => None,
        };
        if is_dvd && let Err(error) = player.configure_dvd(dvd_device.as_deref()) {
            self.status_message = Some(format!("DVD konnte nicht vorbereitet werden: {error}. Prüfe, ob libmpv mit DVD-Navigationsunterstützung (libdvdnav) gebaut wurde."));
            return;
        }
        let bluray_device = match &source { MediaSource::BluRayDrive { device } => device.as_ref().map(|path| path.to_string_lossy().into_owned()), _ => None };
        if is_bluray && let Err(error) = player.configure_bluray(bluray_device.as_deref()) {
            self.status_message = Some(format!("Blu-ray konnte nicht vorbereitet werden: {error}. Prüfe, ob libmpv mit libbluray gebaut wurde."));
            return;
        }
        let input = match &source {
            MediaSource::File(_) if is_dvd => "dvd://".into(),
            MediaSource::File(path) => path.to_string_lossy().into_owned(),
            MediaSource::DvdDrive { .. } => "dvd://".into(),
            MediaSource::BluRayDrive { .. } => "bd://".into(),
            MediaSource::OpticalAuto => unreachable!("automatische Laufwerkserkennung wurde oben behandelt"),
            MediaSource::AudioCd { .. } => "cdda://".into(),
        };
        match player.load_file(&input) {
            Ok(()) => { self.load_pending = true; self.status_message = None; }
            Err(error) => {
                self.load_pending = false;
                self.loaded_source = None;
                let message = if is_dvd {
                    format!("DVD konnte nicht geöffnet werden: {error}. Prüfe das Laufwerk, eine eingelegte DVD und die DVD-Navigationsunterstützung (libdvdnav) in libmpv.")
                } else if is_bluray {
                    format!("Blu-ray konnte nicht geöffnet werden: {error}. MPV meldet hier nur einen allgemeinen Ladefehler; daraus ist die genaue Ursache nicht erkennbar. Prüfe, ob diese libmpv-Version libbluray enthält und ob die Disc lesbar ist. Bei AACS-geschützten Discs werden libaacs und eine passende KEYDB.cfg benötigt. Java und die BD-J-Bibliothek sind nur für BD-J-Menüs nötig.")
                } else {
                    format!("Wiedergabefehler: {error}")
                };
                self.status_message = Some(message);
            }
        }
    }

    fn poll_load_status(&mut self, ctx: &egui::Context) {
        if self.load_pending { ctx.request_repaint_after(std::time::Duration::from_millis(120)); }

        let result = match self.player.as_ref() {
            Some(player) => player.poll_file_events(),
            None => return,
        };
        match result {
            Ok((file_loaded, reached_eof)) => {
                if reached_eof { self.advance_playlist_after_eof(); }
                if !self.load_pending || !file_loaded { return; }
                self.loaded_source = self.requested_source.clone();
                self.load_pending = false;
                self.status_message = None;
                if self.loaded_source.as_ref().is_some_and(|source| source.is_disc() || source.is_audio_cd()) {
                    self.playlist_visible = true;
                }
                self.refresh_tracks();
                self.refresh_disc_info();
                self.maybe_prompt_resume();
            }
            Err(error) => {
                self.loaded_source = None;
                self.load_pending = false;
                self.audio_track_options.clear();
                self.selected_audio_track = None;
                self.subtitle_track_options.clear();
                self.selected_subtitle_track = None;
                self.disc_title_options.clear();
                self.selected_disc_title = None;
                self.chapter_options.clear();
                self.selected_chapter = None;
                let is_dvd = self.requested_source.as_ref().is_some_and(MediaSource::is_dvd);
                let is_bluray = self.requested_source.as_ref().is_some_and(MediaSource::is_bluray);
                let is_cd = self.requested_source.as_ref().is_some_and(MediaSource::is_audio_cd);
                self.status_message = Some(if is_dvd {
                    format!("DVD konnte nicht geladen werden: {error}. Prüfe das Laufwerk, eine eingelegte DVD und die DVD-Navigationsunterstützung (libdvdnav) in libmpv.")
                } else if is_bluray {
                    format!("Blu-ray konnte nicht geladen werden: {error}. MPV meldet hier nur einen allgemeinen Ladefehler; daraus ist die genaue Ursache nicht erkennbar. Prüfe, ob diese libmpv-Version libbluray enthält und ob die Disc lesbar ist. Bei AACS-geschützten Discs werden libaacs und eine passende KEYDB.cfg benötigt. Java und die BD-J-Bibliothek sind nur für BD-J-Menüs nötig.")
                } else if is_cd {
                    format!("Audio-CD konnte nicht geladen werden: {error}. Prüfe das Laufwerk, eine eingelegte Audio-CD und die libcdio-Unterstützung (cdda://) in libmpv.")
                } else {
                    format!("Wiedergabefehler: {error}")
                });
            }
        }
    }

    fn advance_playlist_after_eof(&mut self) {
        let is_regular_file = self.loaded_source.as_ref().is_some_and(|source| {
            matches!(source, MediaSource::File(_)) && !source.is_dvd()
        });
        if !is_regular_file { return; }
        let Some(current) = self.selected else { return; };
        if current + 1 < self.playlist.len() {
            self.selected = Some(current + 1);
            self.requested_source = None;
        }
    }

    fn poll_cd_status(&mut self, ctx: &egui::Context) {
        if self.cd_player.is_none() { return; }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        if self.cd_last_poll.elapsed() < std::time::Duration::from_millis(250) { return; }
        self.cd_last_poll = std::time::Instant::now();
        let Some(cd_player) = self.cd_player.as_mut() else { return; };
        match cd_player.status() {
            Ok(status) => self.cd_status = Some(status),
            Err(error) => {
                self.cd_status = None;
                self.status_message = Some(format!("Audio-CD-Status nicht lesbar: {error}"));
            }
        }
    }

    fn refresh_tracks(&mut self) {
        let tracks = self.player.as_ref().map(|player| (player.audio_tracks(), player.subtitle_tracks()));
        let Some((audio_tracks, subtitle_tracks)) = tracks else { return; };
        match audio_tracks {
            Ok(tracks) => {
                self.selected_audio_track = tracks.iter().find(|track| track.selected).map(|track| track.id);
                self.audio_track_options = selection_options(&tracks);
            }
            Err(error) => {
                self.audio_track_options.clear();
                self.status_message = Some(format!("Audiospuren konnten nicht gelesen werden: {error}"));
            }
        }
        match subtitle_tracks {
            Ok(tracks) => {
                self.selected_subtitle_track = tracks.iter().find(|track| track.selected).map(|track| track.id);
                self.subtitle_track_options = selection_options(&tracks);
            }
            Err(error) => {
                self.subtitle_track_options.clear();
                self.status_message = Some(format!("Untertitelspuren konnten nicht gelesen werden: {error}"));
            }
        }
    }

    fn refresh_disc_info(&mut self) {
        let is_disc = self.loaded_source.as_ref().is_some_and(MediaSource::is_disc);
        let is_bluray = self.loaded_source.as_ref().is_some_and(MediaSource::is_bluray);
        let is_mpv_cd = self.cd_player.is_none()
            && self.loaded_source.as_ref().is_some_and(MediaSource::is_audio_cd);
        if !is_disc && !is_mpv_cd { return; }
        let Some(player) = self.player.as_ref() else { return; };

        if is_mpv_cd {
            self.audio_cd_title = player.audio_cd_title().ok().flatten();
        }

        if is_disc {
            match player.disc_titles() {
                Ok(titles) => {
                    self.disc_title_options = disc_title_options(&titles);
                }
                Err(error) => self.status_message = Some(format!("Disc-Titel konnten nicht gelesen werden: {error}")),
            }
            if let Ok(selected) = player.current_disc_title() { self.selected_disc_title = selected; }
        }

        match player.chapters() {
            Ok(chapters) => {
                self.chapter_options = if is_mpv_cd {
                    cd_track_options(&chapters)
                } else {
                    chapter_options(&chapters)
                };
            }
            Err(error) => self.status_message = Some(if is_mpv_cd {
                format!("CD-Tracks konnten nicht gelesen werden: {error}")
            } else if is_bluray {
                format!("Blu-ray-Kapitel konnten nicht gelesen werden: {error}")
            } else {
                format!("DVD-Kapitel konnten nicht gelesen werden: {error}")
            }),
        }
        if let Ok(selected) = player.current_chapter() { self.selected_chapter = selected; }
    }

    fn draw_track_selector(
        ui: &mut egui::Ui,
        selector_id: &'static str,
        label: &str,
        none_label: &str,
        choices: &[SelectionOption],
        selected: Option<i64>,
    ) -> Option<Option<i64>> {
        if choices.is_empty() { return None; }
        let selected_label = choices.iter().find(|choice| Some(choice.id) == selected)
            .map(|choice| choice.label.clone()).unwrap_or_else(|| none_label.into());
        let mut next_selected = selected;
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).color(theme::TEXT_MUTED));
            egui::ComboBox::from_id_salt(selector_id)
                .width(ui.available_width())
                .selected_text(selected_label)
                .show_ui(ui, |ui| {
                    changed |= ui.selectable_value(&mut next_selected, None, none_label).changed();
                    for choice in choices {
                        changed |= ui.selectable_value(
                            &mut next_selected,
                            Some(choice.id),
                            &choice.label,
                        ).changed();
                    }
                });
        });
        changed.then_some(next_selected)
    }

    fn draw_track_selectors(&mut self, ui: &mut egui::Ui) {
        if let Some(selected) = Self::draw_track_selector(
            ui, "audio_track_selector", "Audiospur", "Automatisch", &self.audio_track_options, self.selected_audio_track,
        ) && let Some(player) = self.player.as_ref()
        {
            match player.set_audio_track(selected) {
                Ok(()) => self.selected_audio_track = selected,
                Err(error) => self.status_message = Some(format!("Audiospur konnte nicht gewechselt werden: {error}")),
            }
        }
        if let Some(selected) = Self::draw_track_selector(
            ui, "subtitle_track_selector", "Untertitel", "Aus", &self.subtitle_track_options, self.selected_subtitle_track,
        ) && let Some(player) = self.player.as_ref()
        {
            match player.set_subtitle_track(selected) {
                Ok(()) => self.selected_subtitle_track = selected,
                Err(error) => self.status_message = Some(format!("Untertitel konnten nicht geändert werden: {error}")),
            }
        } else if self.loaded_source.as_ref().is_some_and(MediaSource::is_disc) {
            ui.label(egui::RichText::new("Untertitel: keine verfügbar").color(theme::TEXT_MUTED));
        }
    }

    fn draw_disc_selectors(&mut self, ui: &mut egui::Ui) {
        let is_disc = self.loaded_source.as_ref().is_some_and(MediaSource::is_disc);
        let is_bluray = self.loaded_source.as_ref().is_some_and(MediaSource::is_bluray);
        let is_mpv_cd = self.cd_player.is_none()
            && self.loaded_source.as_ref().is_some_and(MediaSource::is_audio_cd);
        if !is_disc && !is_mpv_cd { return; }

        if is_mpv_cd {
            if let Some(title) = &self.audio_cd_title {
                ui.label(egui::RichText::new(title).strong().color(theme::TEXT));
            }
        }

        if self.last_disc_state_poll.elapsed() >= std::time::Duration::from_millis(300) {
            self.last_disc_state_poll = std::time::Instant::now();
            if let Some(player) = self.player.as_ref() {
                if is_disc && let Ok(Some(title)) = player.current_disc_title() {
                    self.selected_disc_title = Some(title);
                }
                if let Ok(Some(chapter)) = player.current_chapter() { self.selected_chapter = Some(chapter); }
            }
        }

        if is_disc {
            let title_label = if is_bluray { "Blu-ray-Titel" } else { "DVD-Titel" };
            if self.disc_title_options.is_empty() {
                ui.label(egui::RichText::new(format!("{title_label}: keine Auswahl verfügbar")).color(theme::TEXT_MUTED));
            }
            if let Some(title) = Self::draw_index_selector(
                ui, "disc_title_selector", title_label, &self.disc_title_options, self.selected_disc_title,
            ) && let Some(player) = self.player.as_ref()
            {
                match player.set_disc_title(title) {
                    Ok(()) => {
                        self.selected_disc_title = Some(title);
                        self.refresh_disc_info();
                        self.refresh_tracks();
                    }
                    Err(error) => self.status_message = Some(format!("Disc-Titel konnte nicht gewechselt werden: {error}")),
                }
            }
        }

        if self.chapter_options.is_empty() {
            ui.label(egui::RichText::new(if is_mpv_cd { "Tracks: keine verfügbar" } else { "Kapitel: keine verfügbar" }).color(theme::TEXT_MUTED));
        }
        if let Some(chapter) = Self::draw_index_selector(
            ui,
            if is_mpv_cd { "cd_track_selector" } else { "disc_chapter_selector" },
            if is_mpv_cd { "Track" } else { "Kapitel" },
            &self.chapter_options,
            self.selected_chapter,
        ) && let Some(player) = self.player.as_ref()
        {
            match player.set_chapter(chapter) {
                Ok(()) => self.selected_chapter = Some(chapter),
                Err(error) => self.status_message = Some(if is_mpv_cd {
                    format!("CD-Track konnte nicht gewechselt werden: {error}")
                } else if is_bluray {
                    format!("Blu-ray-Kapitel konnten nicht gewechselt werden: {error}")
                } else {
                    format!("DVD-Kapitel konnte nicht gewechselt werden: {error}")
                }),
            }
        }

        // CDDA tracks are exposed by mpv as chapters. Provide explicit
        // track navigation alongside the selector on platforms using libmpv.
        if is_mpv_cd && self.chapter_options.len() > 1 {
            let selected_index = self.chapter_options.iter()
                .position(|choice| Some(choice.id) == self.selected_chapter)
                .unwrap_or(0);
            let previous = selected_index.checked_sub(1)
                .and_then(|index| self.chapter_options.get(index))
                .map(|choice| choice.id);
            let next = self.chapter_options.get(selected_index + 1).map(|choice| choice.id);
            let mut requested = None;
            ui.horizontal(|ui| {
                if ui.add_enabled(previous.is_some(), egui::Button::new("‹ Vorheriger Track")).clicked() {
                    requested = previous;
                }
                if ui.add_enabled(next.is_some(), egui::Button::new("Nächster Track ›")).clicked() {
                    requested = next;
                }
            });
            if let Some(chapter) = requested && let Some(player) = self.player.as_ref() {
                match player.set_chapter(chapter) {
                    Ok(()) => self.selected_chapter = Some(chapter),
                    Err(error) => self.status_message = Some(format!("CD-Track konnte nicht gewechselt werden: {error}")),
                }
            }
        }
    }

    fn draw_index_selector(
        ui: &mut egui::Ui,
        selector_id: &'static str,
        label: &str,
        choices: &[SelectionOption],
        selected: Option<i64>,
    ) -> Option<i64> {
        if choices.is_empty() { return None; }
        let selected_label = choices.iter().find(|choice| Some(choice.id) == selected)
            .map(|choice| choice.label.clone()).unwrap_or_else(|| choices[0].label.clone());
        let mut next_selected = selected;
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).color(theme::TEXT_MUTED));
            egui::ComboBox::from_id_salt(selector_id)
                .width(ui.available_width())
                .selected_text(selected_label)
                .show_ui(ui, |ui| {
                    for choice in choices {
                        changed |= ui.selectable_value(&mut next_selected, Some(choice.id), &choice.label).changed();
                    }
                });
        });
        changed.then_some(next_selected).flatten()
    }

    fn select_relative(&mut self, offset: isize) {
        if self.playlist.is_empty() { return; }
        let current = self.selected.unwrap_or(0) as isize;
        self.selected = Some((current + offset).rem_euclid(self.playlist.len() as isize) as usize);
    }

    fn step_chapter(&mut self, delta: i64) -> bool {
        let Some(player) = self.player.as_ref() else { return false; };
        let Ok(chapters) = player.chapters() else { return false; };
        if chapters.len() < 2 { return false; }

        let current = player.current_chapter().ok().flatten();
        let Some(current_index) = chapters.iter().position(|chapter| Some(chapter.index) == current) else {
            return false;
        };
        let target_index = (current_index as i64 + delta)
            .clamp(0, chapters.len() as i64 - 1) as usize;
        let target = chapters[target_index].index;
        if Some(target) == current { return false; }

        match player.set_chapter(target) {
            Ok(()) => {
                self.selected_chapter = Some(target);
                true
            }
            Err(error) => {
                self.status_message = Some(format!("Kapitel konnte nicht gewechselt werden: {error}"));
                true
            }
        }
    }

    fn navigate_previous_next(&mut self, delta: i64) {
        if self.cd_player.is_some() {
            self.cd_step(delta as i32);
            return;
        }

        let is_audio_cd = self.loaded_source.as_ref().is_some_and(MediaSource::is_audio_cd);
        if is_audio_cd {
            if !self.step_chapter(delta) {
                self.status_message = Some("CD-Tracknavigation ist derzeit nicht verfügbar.".into());
            }
            return;
        }

        if self.playlist.len() > 1 {
            self.select_relative(delta as isize);
        } else if !self.step_chapter(delta) {
            if let Some(player) = self.player.as_ref()
                && let Err(error) = player.seek_relative(delta as f64 * 10.0)
            {
                self.status_message = Some(format!("Wiedergabe konnte nicht verschoben werden: {error}"));
            }
        }
    }

    fn clear_playlist(&mut self) {
        self.playlist.clear();
        self.duplicate_prompt = None;
        self.selected = None;
        self.direct_source = None;
        self.requested_source = None;
        self.loaded_source = None;
        self.load_pending = false;
        self.cd_player = None;
        self.cd_status = None;
        self.audio_track_options.clear();
        self.selected_audio_track = None;
        self.subtitle_track_options.clear();
        self.selected_subtitle_track = None;
        self.disc_title_options.clear();
        self.selected_disc_title = None;
        self.chapter_options.clear();
        self.selected_chapter = None;
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
        let video_items = self.playlist.iter().enumerate()
            .filter(|(_, file)| file.is_video())
            .map(|(index, file)| (index, file.name(), self.selected == Some(index)))
            .collect::<Vec<_>>();
        egui::Panel::top("header").exact_size(112.0)
            .frame(egui::Frame::new().fill(theme::BG_PANEL)
                .inner_margin(egui::Margin::symmetric(18, 4))
                .stroke(egui::Stroke::new(1.0, theme::BORDER)))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.menu_button("Datei", |ui| {
                            if ui.button("Datei öffnen…").clicked() {
                                ui.close();
                                self.open_files();
                            }
                            if ui.button("Ordner öffnen…").clicked() {
                                ui.close();
                                self.open_folder();
                            }
                            ui.separator();
                            if ui.button("Playlist laden…").clicked() {
                                ui.close();
                                self.load_playlist_m3u();
                            }
                            if ui.add_enabled(!self.playlist.is_empty(), egui::Button::new("Playlist speichern…")).clicked() {
                                ui.close();
                                self.save_playlist_m3u();
                            }
                            ui.separator();
                            if ui.button("Laufwerk öffnen").clicked() {
                                ui.close();
                                self.open_optical_drive();
                            }
                            if ui.button("Audio-CD auslesen…").clicked() {
                                ui.close();
                                self.open_rip_window();
                            }
                            if !video_items.is_empty() {
                                ui.separator();
                                ui.label(egui::RichText::new("Videos in der Playlist").color(theme::TEXT_MUTED));
                                for (index, name, selected) in &video_items {
                                    if ui.selectable_label(*selected, name).clicked() {
                                        self.selected = Some(*index);
                                        ui.close();
                                    }
                                }
                            }
                            ui.separator();
                            if ui.button("Beenden").clicked() {
                                ui.close();
                                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        });
                        ui.menu_button("Einstellungen", |ui| {
                            if ui.button("Wiedergabe und Oberfläche…").clicked() {
                                self.settings_open = true;
                                ui.close();
                            }
                        });
                        ui.menu_button("Info", |ui| {
                            if ui.button("Über MediaPlayer…").clicked() {
                                self.about_open = true;
                                ui.close();
                            }
                        });
                    });
                    ui.separator();
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
                        if self.loaded_source.is_some()
                            && self.cd_player.is_none()
                            && (!self.audio_track_options.is_empty() || !self.subtitle_track_options.is_empty())
                        {
                            ui.menu_button("Spuren", |ui| {
                                self.draw_track_selectors(ui);
                            });
                        }
                        if self.loaded_source.as_ref().is_some_and(MediaSource::is_disc)
                            && ui.button(if self.loaded_source.as_ref().is_some_and(MediaSource::is_bluray) { "Blu-ray-Menü" } else { "DVD-Menü" }).clicked()
                            && let Some(player) = self.player.as_ref()
                            && let Err(error) = player.show_disc_menu()
                        {
                            self.status_message = Some(format!("Disc-Menü konnte nicht geöffnet werden: {error}"));
                        }
                        ui.add_space(8.0);
                        if ui.add(IconButton::new(Icon::Next, "Nächster Titel").size(egui::vec2(38.0, 38.0))).clicked() {
                            self.navigate_previous_next(1);
                        }
                        if ui.add(IconButton::new(Icon::Previous, "Vorheriger Titel").size(egui::vec2(38.0, 38.0))).clicked() {
                            self.navigate_previous_next(-1);
                        }
                    });
                    });
                });
            });
    }

    fn draw_app_windows(&mut self, ctx: &egui::Context) {
        let mut settings_open = self.settings_open;
        let mut playback_settings_changed = false;
        egui::Window::new("Einstellungen")
            .open(&mut settings_open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("Oberfläche");
                ui.checkbox(&mut self.playlist_visible, "Playlist rechts anzeigen");
                ui.add_space(8.0);
                ui.separator();
                ui.heading("Wiedergabe");
                ui.label("Bevorzugte Audiosprache");
                playback_settings_changed |= ui.add(egui::TextEdit::singleline(&mut self.playback_settings.audio_language)
                    .desired_width(220.0).hint_text("de,en,ja")).changed();
                ui.label(egui::RichText::new("Sprachcodes, mehrere Einträge mit Komma trennen; Reihenfolge = Priorität.")
                    .size(12.0).color(theme::TEXT_MUTED));
                ui.add_space(6.0);
                egui::Frame::new()
                    .fill(theme::BG_CARD)
                    .stroke(egui::Stroke::new(1.0, theme::ACCENT))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 7))
                    .show(ui, |ui| {
                        playback_settings_changed |= ui.add(egui::Checkbox::new(
                            &mut self.playback_settings.subtitles_enabled,
                            egui::RichText::new("Untertitel anzeigen").size(14.5).strong().color(theme::TEXT),
                        )).changed();
                    });
                ui.label("Bevorzugte Untertitelsprache");
                ui.add_enabled_ui(self.playback_settings.subtitles_enabled, |ui| {
                    playback_settings_changed |= ui.add(egui::TextEdit::singleline(&mut self.playback_settings.subtitle_language)
                        .desired_width(220.0).hint_text("de,en"))
                        .changed();
                });
                ui.label(egui::RichText::new("Die Untertitelsprache wird nur verwendet, wenn Untertitel aktiviert sind.")
                    .size(12.0).color(theme::TEXT_MUTED));
                ui.add_space(8.0);
                ui.separator();
                ui.label(egui::RichText::new("Audio-CDs werden mit Vorlesen und Nachpufferung wiedergegeben, sofern libmpv CDDA unterstützt.")
                    .color(theme::TEXT_MUTED));
            });
        self.settings_open = settings_open;
        if playback_settings_changed {
            save_playback_settings(&self.playback_settings);
            if let Some(player) = self.player.as_ref()
                && let Err(error) = player.configure_playback_languages(
                    &self.playback_settings.audio_language,
                    &self.playback_settings.subtitle_language,
                    self.playback_settings.subtitles_enabled,
                )
            {
                self.status_message = Some(format!("Wiedergabeeinstellungen konnten nicht angewendet werden: {error}"));
            }
        }

        if let Some(prompt) = self.duplicate_prompt.as_ref() {
            let duplicate_count = prompt.files.len();
            let mut duplicate_choice = None;
            egui::Window::new("Doppelte Dateien")
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .resizable(true)
                .default_width(560.0)
                .show(ctx, |ui| {
                    ui.label(format!("{duplicate_count} Datei(en) sind bereits in der Playlist oder doppelt ausgewählt:") );
                    ui.add_space(8.0);
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for path in &prompt.files {
                            ui.label(path.display().to_string());
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Trotzdem alle hinzufügen").clicked() {
                            duplicate_choice = Some(DuplicateChoice::AddAll);
                        }
                        if ui.button("Nur neue Dateien hinzufügen").clicked() {
                            duplicate_choice = Some(DuplicateChoice::AddNew);
                        }
                        if ui.button("Abbrechen").clicked() {
                            duplicate_choice = Some(DuplicateChoice::Cancel);
                        }
                    });
                });
            if let Some(choice) = duplicate_choice { self.resolve_duplicate_prompt(choice); }
        }

        let mut about_open = self.about_open;
        egui::Window::new("Über MediaPlayer")
            .open(&mut about_open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("MediaPlayer");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.add_space(6.0);
                ui.label("Audio- und Videowiedergabe für lokale Dateien, DVDs, Blu-rays und Audio-CDs.");
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Audio-CD-Titel werden angezeigt, wenn CD-Text auf der Disc vorhanden ist.")
                    .color(theme::TEXT_MUTED));
            });
        self.about_open = about_open;
    }

    fn draw_playlist(&mut self, ui: &mut egui::Ui) {
        if !self.playlist_visible { return; }
        egui::Panel::right("playlist").resizable(true).default_size(290.0).min_size(230.0).max_size(460.0)
            .frame(egui::Frame::new().fill(theme::BG_PANEL)
                .inner_margin(egui::Margin::symmetric(24, 16))
                .stroke(egui::Stroke::new(1.0, theme::BORDER)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Playlist").font(theme::heading_font(19.0)).color(theme::TEXT));
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(format!("{} Titel", self.playlist.len())).size(13.0).color(theme::TEXT_MUTED));
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
                    let mut reorder = None;
                    let mut remove_index = None;
                    if ui.input(|input| input.pointer.primary_down()) {
                        if let Some((index, started)) = self.playlist_drag_press {
                            if started.elapsed() >= std::time::Duration::from_millis(450)
                                && self.playlist_drag_activated.is_none()
                            {
                                egui::DragAndDrop::set_payload(ui.ctx(), index);
                                self.playlist_drag_activated = Some(index);
                            } else if self.playlist_drag_activated.is_none() {
                                ui.ctx().request_repaint_after(std::time::Duration::from_millis(20));
                            }
                        }
                    }
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        let row_width = ui.available_width();
                        for (index, file) in self.playlist.iter().enumerate() {
                            let selected = self.selected == Some(index);
                            let icon = if file.is_audio_cd() { Icon::Disc } else if file.is_video() { Icon::Play } else { Icon::Music };
                            let name = file.name();
                            let (_, dropped) = ui.dnd_drop_zone::<usize, _>(egui::Frame::NONE, |ui| {
                                ui.set_width(row_width);
                                ui.horizontal(|ui| {
                                    let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 30.0), egui::Sense::hover());
                                    icons::paint(ui.painter(), icon, icon_rect.shrink(2.0),
                                        if selected { theme::ACCENT } else { theme::TEXT_MUTED });
                                    let remove_width = 26.0;
                                    let name_width = (ui.available_width() - remove_width - ui.spacing().item_spacing.x).max(60.0);
                                    let response = ui.allocate_response(egui::vec2(name_width, 40.0), egui::Sense::click())
                                        .on_hover_text(format!("{name}\nKlicken zum Abspielen · halten und ziehen zum Sortieren"));
                                    let fill = if selected { theme::ACCENT } else if response.hovered() { theme::BG_CARD } else { theme::BG_PANEL };
                                    let text_color = if selected { egui::Color32::WHITE } else { theme::TEXT };
                                    ui.painter().rect(response.rect, egui::CornerRadius::same(8), fill,
                                        egui::Stroke::new(1.0, if selected { theme::ACCENT } else { theme::BORDER }),
                                        egui::StrokeKind::Inside);
                                    let text_rect = response.rect.shrink2(egui::vec2(10.0, 0.0));
                                    ui.painter().with_clip_rect(text_rect).text(
                                        text_rect.left_center(), egui::Align2::LEFT_CENTER, &name,
                                        egui::FontId::proportional(11.5), text_color,
                                    );
                                    if response.is_pointer_button_down_on() && self.playlist_drag_press.is_none() {
                                        self.playlist_drag_press = Some((index, std::time::Instant::now()));
                                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(20));
                                    }
                                    if response.clicked() && self.playlist_drag_activated != Some(index) {
                                        self.selected = Some(index);
                                    }
                                    if ui.add_sized(egui::vec2(remove_width, 32.0), egui::Button::new("×"))
                                        .on_hover_text("Aus Playlist entfernen").clicked()
                                    {
                                        remove_index = Some(index);
                                    }
                                });
                            });
                            if let Some(from) = dropped { reorder = Some((*from, index)); }
                        }
                    });
                    if !ui.input(|input| input.pointer.primary_down()) {
                        self.playlist_drag_press = None;
                        self.playlist_drag_activated = None;
                    }
                    if let Some(index) = remove_index {
                        self.remove_playlist_entry(index);
                    } else if let Some((from, target)) = reorder
                        && from < self.playlist.len()
                        && target < self.playlist.len()
                        && from != target
                    {
                        let selected_source = self.selected
                            .and_then(|index| self.playlist.get(index))
                            .map(|file| file.source.clone());
                        let file = self.playlist.remove(from);
                        // Dropping onto a later row should place the item after that row.
                        // The target index is already the correct insertion index after removal.
                        self.playlist.insert(target, file);
                        self.selected = selected_source.and_then(|source| self.playlist.iter()
                            .position(|file| file.source == source));
                    }
                }

                ui.add_space(8.0);
                ui.separator();
                if icons::wide_button(ui, Icon::Plus, "Datei öffnen", true).clicked() { self.open_files(); }
                if icons::wide_button(ui, Icon::Folder, "Ordner öffnen", true).clicked() { self.open_folder(); }
                if icons::wide_button(ui, Icon::Disc, "Laufwerk öffnen", true).clicked() { self.open_optical_drive(); }
                if self.loaded_source.is_some() {
                    if self.cd_player.is_some() {
                        ui.label(egui::RichText::new("Audio-CD wird im Hauptbereich gesteuert.").size(13.0).color(theme::TEXT_MUTED));
                    } else {
                        egui::ScrollArea::vertical().max_height(190.0).auto_shrink([false, true]).show(ui, |ui| {
                            self.draw_track_selectors(ui);
                            self.draw_disc_selectors(ui);
                        });
                    }
                }
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
                if self.cd_player.is_some() {
                    self.draw_cd_player(ui);
                } else if self.loaded_source.is_some() {
                    if let Some(player) = self.player.as_mut() {
                        let response = ui.add(SharkPlayer::new(player).bar_height(48.0).skip_seconds(10.0));
                        if self.loaded_source.as_ref().is_some_and(MediaSource::is_disc) {
                            Self::forward_dvd_pointer(ui.ctx(), response.rect, player);
                        }
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
                        });
                    });
                }
            });
    }

    fn forward_dvd_pointer(ctx: &egui::Context, mut video_rect: egui::Rect, player: &PlayerState) {
        if video_rect.width() <= 0.0 || video_rect.height() <= 0.0 {
            video_rect = ctx.content_rect();
        }
        let (position, moved, clicked) = ctx.input(|input| (
            input.pointer.hover_pos(),
            input.pointer.delta() != egui::Vec2::ZERO,
            input.pointer.button_pressed(egui::PointerButton::Primary),
        ));
        let Some(position) = position.filter(|position| video_rect.contains(*position)) else { return; };
        let x = f64::from((position.x - video_rect.left()) / video_rect.width());
        let y = f64::from((position.y - video_rect.top()) / video_rect.height());

        if moved { let _ = player.dvd_menu_pointer(x, y, false); }
        if clicked { let _ = player.dvd_menu_pointer(x, y, true); }
    }

    fn cd_step(&mut self, delta: i32) {
        let Some(status) = self.cd_status.clone() else { return; };
        let track_count = self.cd_player.as_ref().map(|cd_player| cd_player.tracks().len() as u32).unwrap_or(0);
        let current = status.track.max(1) as i32;
        let target = (current + delta).clamp(1, track_count.max(1) as i32) as u32;
        if let Some(cd_player) = self.cd_player.as_mut() {
            match cd_player.play_track(target) {
                Ok(()) => self.cd_status = cd_player.status().ok(),
                Err(error) => self.status_message = Some(format!("Audio-CD: {error}")),
            }
        }
    }

    fn draw_cd_player(&mut self, ui: &mut egui::Ui) {
        let action = {
            let Some(cd_player) = self.cd_player.as_ref() else { return; };
            draw_cd_panel(ui, cd_player, self.cd_status.as_ref())
        };
        let Some(action) = action else { return; };

        let status = self.cd_status.clone();
        let track_count = self.cd_player.as_ref().map(|cd_player| cd_player.tracks().len() as u32).unwrap_or(0);
        let Some(cd_player) = self.cd_player.as_mut() else { return; };
        let current_track = status.as_ref().map(|status| status.track).unwrap_or(1).max(1);

        let result = match action {
            CdAction::PlayPause => match status.as_ref().map(|status| status.mode) {
                Some(cd::CdMode::Playing) => cd_player.pause(),
                Some(cd::CdMode::Paused) => cd_player.play(),
                _ => cd_player.play_track(current_track),
            },
            CdAction::Stop => cd_player.stop(),
            CdAction::Next => cd_player.play_track((current_track + 1).min(track_count.max(1))),
            CdAction::Previous => cd_player.play_track(current_track.saturating_sub(1).max(1)),
            CdAction::Select(track) => cd_player.play_track(track),
        };

        if let Err(error) = result {
            self.status_message = Some(format!("Audio-CD: {error}"));
        } else {
            self.cd_status = cd_player.status().ok();
        }
    }
}

#[derive(Clone, Copy)]
enum CdAction {
    PlayPause,
    Stop,
    Next,
    Previous,
    Select(u32),
}

fn draw_cd_panel(ui: &mut egui::Ui, cd_player: &cd::AudioCd, status: Option<&cd::CdStatus>) -> Option<CdAction> {
    let mut action = None;
    let tracks = cd_player.tracks();
    let current_track = status.map(|status| status.track).unwrap_or(0);
    let mode = status.map(|status| status.mode).unwrap_or(cd::CdMode::Stopped);
    let position = status.map(|status| status.position_seconds).unwrap_or(0.0);
    let track_length = status.map(|status| status.track_length_seconds).unwrap_or(0.0);
    let panel_width = ui.available_width().min(620.0);

    ui.vertical_centered(|ui| {
        egui::Frame::new()
            .fill(theme::BG_CARD)
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .corner_radius(egui::CornerRadius::same(14))
            .inner_margin(egui::Margin::same(24))
            .show(ui, |ui| {
                ui.set_width((panel_width - 48.0).max(240.0));

                let (rect, _) = ui.allocate_exact_size(egui::vec2(88.0, 88.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 44.0, theme::BG_APP);
                ui.painter().circle_stroke(rect.center(), 44.0, egui::Stroke::new(1.0, theme::BORDER));
                icons::paint(ui.painter(), Icon::Disc, rect.shrink(22.0), theme::ACCENT);

                ui.add_space(12.0);
                let disc_title = cd_player.disc_title().unwrap_or("Audio-CD");
                ui.label(egui::RichText::new(disc_title).font(theme::heading_font(20.0)).color(theme::TEXT));
                ui.label(egui::RichText::new(format!(
                    "Laufwerk {} · {} Titel · {}",
                    cd_player.drive_label(),
                    tracks.len(),
                    format_duration(cd_player.total_seconds()),
                )).size(13.0).color(theme::TEXT_MUTED));

                ui.add_space(16.0);
                let track_title = tracks.iter()
                    .find(|track| track.number == current_track)
                    .and_then(|track| track.title.clone())
                    .unwrap_or_else(|| if current_track > 0 { format!("Track {current_track}") } else { "Kein Track".into() });
                ui.label(egui::RichText::new(track_title).size(17.0).strong().color(theme::TEXT));

                ui.add_space(6.0);
                let fraction = if track_length > 0.0 { (position / track_length).clamp(0.0, 1.0) as f32 } else { 0.0 };
                ui.add(egui::ProgressBar::new(fraction)
                    .desired_width(ui.available_width())
                    .fill(theme::ACCENT)
                    .text(format!("{} / {}", format_duration(position), format_duration(track_length))));

                ui.add_space(14.0);
                let button_size = 42.0;
                let spacing = ui.spacing().item_spacing.x;
                let controls_width = button_size * 4.0 + spacing * 4.0;
                let left = ((ui.available_width() - controls_width) * 0.5).max(0.0);
                ui.horizontal(|ui| {
                    ui.add_space(left);
                    if ui.add(IconButton::new(Icon::Previous, "Vorheriger Track").size(egui::vec2(button_size, button_size))).clicked() {
                        action = Some(CdAction::Previous);
                    }
                    let (icon, tooltip) = if mode == cd::CdMode::Playing {
                        (Icon::Pause, "Pause")
                    } else {
                        (Icon::Play, "Wiedergabe")
                    };
                    if ui.add(IconButton::new(icon, tooltip).primary().size(egui::vec2(button_size, button_size))).clicked() {
                        action = Some(CdAction::PlayPause);
                    }
                    if ui.add(IconButton::new(Icon::Next, "Nächster Track").size(egui::vec2(button_size, button_size))).clicked() {
                        action = Some(CdAction::Next);
                    }
                    ui.add_space(8.0);
                    if ui.add(IconButton::new(Icon::Stop, "Stopp").size(egui::vec2(button_size, button_size))).clicked() {
                        action = Some(CdAction::Stop);
                    }
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);
                egui::ScrollArea::vertical().max_height(240.0).auto_shrink([false, true]).show(ui, |ui| {
                    for track in tracks {
                        let selected = track.number == current_track;
                        let label = match &track.title {
                            Some(title) if !title.is_empty() => {
                                format!("{:>2}. {}  ·  {}", track.number, title, format_duration(track.length_seconds))
                            }
                            _ => format!("{:>2}. Track  ·  {}", track.number, format_duration(track.length_seconds)),
                        };
                        if ui.selectable_label(selected, egui::RichText::new(label).size(14.5)).clicked() {
                            action = Some(CdAction::Select(track.number));
                        }
                    }
                });
            });
    });

    action
}

impl eframe::App for MediaPlayerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_dropped_files(&ctx);
        self.play_selected();
        self.poll_load_status(&ctx);
        self.poll_cd_status(&ctx);
        self.save_resume_position_if_due();
        self.draw_header(ui);
        self.draw_playlist(ui);
        self.draw_main(ui);
        self.draw_app_windows(&ctx);
        self.draw_resume_dialog(&ctx);
        ripper::show_viewport(&ctx, self.rip_window.clone());

        if let Some(message) = &self.status_message {
            egui::Area::new(egui::Id::new("status_message"))
                .anchor(egui::Align2::LEFT_BOTTOM, [16.0, -16.0])
                .show(&ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| { ui.label(message); });
                });
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.capture_current_resume_position();
        save_resume_positions(&self.resume_positions);
        self.cd_player = None;
        if let Ok(window) = self.rip_window.lock() {
            window.cancel_on_exit();
        }
        if let Some(player) = self.player.as_mut() { player.destroy_gl_resources(); }
    }
}
