//! Audio-CDs auslesen und die gewählten Tracks als MP3-Dateien speichern.
//!
//! Unter Windows werden die Audiodaten über SCSI-Pass-Through-Befehle
//! (`READ CD`, 2352 Byte je Sektor) gelesen. Die Codierung übernimmt ffmpeg
//! mit dem LAME-Encoder; die ID3-Tags werden beim Codieren gesetzt.
//!
//! Der eigentliche Auslesevorgang läuft in einem eigenen Thread. Die
//! Oberfläche bleibt dadurch bedienbar und kann den Vorgang abbrechen.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::egui;

use crate::theme;

#[cfg(windows)]
use crate::cd;

#[cfg(windows)]
use windows::Device;

const BYTES_PER_SECTOR: usize = 2352;
const SECTORS_PER_CHUNK: u32 = 26;
const MAX_AUDIO_CD_LBA: u32 = 450_000;
const FRAMES_PER_SECOND: f64 = 75.0;
const ERROR_COLOR: egui::Color32 = egui::Color32::from_rgb(0xff, 0x6b, 0x6b);

/// Ein Track, wie er auf der CD steht.
#[derive(Clone, Debug, PartialEq)]
pub struct RipTrack {
    pub number: u32,
    pub start_lba: u32,
    pub sectors: u32,
    pub title: Option<String>,
}

impl RipTrack {
    pub fn length_seconds(&self) -> f64 {
        f64::from(self.sectors) / FRAMES_PER_SECOND
    }
}

/// Die erkannte Audio-CD mit den direkt von der Disc gelesenen Angaben.
#[derive(Clone, Debug, PartialEq)]
pub struct RipDisc {
    pub drive: char,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub tracks: Vec<RipTrack>,
}

impl RipDisc {
    pub fn drive_label(&self) -> String {
        format!("{}:", self.drive)
    }

    pub fn total_seconds(&self) -> f64 {
        self.tracks.iter().map(RipTrack::length_seconds).sum()
    }
}

/// Sucht das erste Laufwerk mit einer eingelegten Audio-CD.
#[cfg(windows)]
pub fn detect_audio_cd() -> Result<RipDisc, String> {
    let drives = cd::optical_drives();
    if drives.is_empty() {
        return Err("Kein CD-/DVD-Laufwerk gefunden.".into());
    }

    let mut errors = Vec::new();
    for drive in drives {
        match windows::read_disc(drive) {
            Ok(disc) => return Ok(disc),
            Err(error) => errors.push(format!("{drive}: {error}")),
        }
    }
    Err(format!("Keine Audio-CD gefunden ({}).", errors.join("; ")))
}

#[cfg(not(windows))]
pub fn detect_audio_cd() -> Result<RipDisc, String> {
    Err("Das Auslesen von Audio-CDs wird derzeit nur unter Windows unterstützt.".into())
}

/// Sucht die ffmpeg-Programmdatei.
pub fn find_ffmpeg() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FFMPEG") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }

    let executable = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths) {
            let candidate = directory.join(executable);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    let mut candidates = Vec::new();
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(PathBuf::from(local).join("Microsoft/WinGet/Links/ffmpeg.exe"));
        }
        if let Some(program_data) = std::env::var_os("ProgramData") {
            candidates.push(PathBuf::from(program_data).join("chocolatey/bin/ffmpeg.exe"));
        }
        candidates.push(PathBuf::from("C:/ffmpeg/bin/ffmpeg.exe"));
    }
    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn default_target_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let home = PathBuf::from(home);
        let music = home.join("Music");
        if music.is_dir() {
            return music;
        }
        return home;
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn is_reserved_name(name: &str) -> bool {
    if matches!(name, "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let bytes = name.as_bytes();
    bytes.len() == 4
        && (name.starts_with("COM") || name.starts_with("LPT"))
        && bytes[3].is_ascii_digit()
        && bytes[3] != b'0'
}

/// Ersetzt Zeichen, die in Windows-Dateinamen nicht erlaubt sind.
fn sanitize_component(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    for character in input.chars() {
        if character.is_control()
            || matches!(character, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        {
            result.push('_');
        } else {
            result.push(character);
        }
    }

    let mut result = result
        .trim()
        .trim_end_matches(['.', ' '])
        .to_string();
    if result.is_empty() {
        return result;
    }

    let stem = result
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if is_reserved_name(&stem) {
        result.insert(0, '_');
    }

    let mut limited = result.chars().take(120).collect::<String>();
    while limited.ends_with('.') || limited.ends_with(' ') {
        limited.pop();
    }
    limited
}

/// Bildet den Dateinamen eines Tracks nach dem vorgegebenen Schema.
fn track_file_name(number: u32, title: Option<&str>) -> String {
    let title = title
        .map(sanitize_component)
        .filter(|title| !title.is_empty());
    match title {
        Some(title) => format!("{number:02} - {title}.mp3"),
        None => format!("Track {number:02}.mp3"),
    }
}

fn format_duration(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TocEntry {
    number: u8,
    control: u8,
    lba: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Toc {
    first: u8,
    last: u8,
    entries: Vec<TocEntry>,
}

/// Liest das Inhaltsverzeichnis (TOC) aus dem Antwortpuffer von Windows.
fn parse_toc(buffer: &[u8]) -> Result<Toc, String> {
    if buffer.len() < 4 {
        return Err("Das Inhaltsverzeichnis der CD ist unvollständig.".into());
    }

    // Die Länge wird von Windows als Big-Endian-Wert geliefert. Zur Sicherheit
    // wird auch Little-Endian geprüft, falls ein Treiber abweicht.
    let declared_be = usize::from(u16::from_be_bytes([buffer[0], buffer[1]]));
    let declared_le = usize::from(u16::from_le_bytes([buffer[0], buffer[1]]));
    let data_length = if declared_be + 2 <= buffer.len() {
        declared_be
    } else if declared_le + 2 <= buffer.len() {
        declared_le
    } else {
        buffer.len() - 2
    };
    let end = (2 + data_length).min(buffer.len());

    let first = buffer[2];
    let last = buffer[3];
    let mut entries = Vec::new();
    let mut offset = 4;
    while offset + 8 <= end {
        let control = buffer[offset + 1] & 0x0F;
        let number = buffer[offset + 2];
        let lba = u32::from_be_bytes([
            buffer[offset + 4],
            buffer[offset + 5],
            buffer[offset + 6],
            buffer[offset + 7],
        ]);
        entries.push(TocEntry {
            number,
            control,
            lba,
        });
        offset += 8;
    }

    if entries.is_empty() {
        return Err("Das Inhaltsverzeichnis enthält keine Tracks.".into());
    }
    Ok(Toc {
        first,
        last,
        entries,
    })
}

/// Wandelt das TOC in Tracks mit Startadresse und Länge um.
fn build_tracks(toc: &Toc) -> Result<Vec<RipTrack>, String> {
    if toc.first == 0 || toc.last == 0 || toc.last < toc.first {
        return Err("keine Audio-Tracks gefunden (keine Audio-CD eingelegt?)".into());
    }
    if toc.entries.iter().any(|entry| entry.lba > MAX_AUDIO_CD_LBA) {
        return Err(
            "Die CD-Laufzeitdaten sind ungültig. Das TOC muss im LBA-Format gelesen werden.".into(),
        );
    }

    let lead_out = toc
        .entries
        .iter()
        .find(|entry| entry.number == 0xAA)
        .map(|entry| entry.lba);
    let mut tracks = Vec::new();
    for number in toc.first..=toc.last {
        let Some(index) = toc
            .entries
            .iter()
            .position(|entry| entry.number == number)
        else {
            return Err(format!("Track {number} fehlt im Inhaltsverzeichnis."));
        };
        let entry = toc.entries[index];
        if entry.control & 0x04 != 0 {
            return Err("keine Audio-CD (die Disc enthält Datentracks)".into());
        }

        let end = toc
            .entries
            .get(index + 1)
            .map(|next| next.lba)
            .or(lead_out);
        let Some(end) = end else {
            return Err(format!("Das Ende von Track {number} ist nicht bestimmbar."));
        };
        let sectors = end.saturating_sub(entry.lba);
        if sectors == 0 {
            return Err(format!("Track {number} hat keine Länge."));
        }

        tracks.push(RipTrack {
            number: u32::from(number),
            start_lba: entry.lba,
            sectors,
            title: None,
        });
    }

    if tracks.is_empty() {
        return Err("keine Audio-Tracks gefunden (keine Audio-CD eingelegt?)".into());
    }
    Ok(tracks)
}

#[derive(Clone, Debug)]
struct JobTrack {
    number: u32,
    start_lba: u32,
    sectors: u32,
    title: Option<String>,
}

struct RipJob {
    drive: char,
    artist: Option<String>,
    album: Option<String>,
    tracks: Vec<JobTrack>,
    total_tracks: u32,
    bitrate: u32,
    target_dir: PathBuf,
    overwrite: bool,
    ffmpeg: PathBuf,
}

#[derive(Debug)]
enum RipEvent {
    TrackStarted {
        index: usize,
        total: usize,
        title: String,
    },
    TrackProgress {
        track_fraction: f32,
        overall_fraction: f32,
    },
    TrackFinished,
    TrackFailed {
        number: u32,
        error: String,
    },
    Finished {
        saved: usize,
        failed: usize,
        cancelled: bool,
    },
    Fatal(String),
}

enum TrackOutcome {
    Done,
    Cancelled,
    Failed(String),
}

fn spawn(job: RipJob, cancel: Arc<AtomicBool>) -> Result<Receiver<RipEvent>, String> {
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("audio-cd-rip".into())
        .spawn(move || run(job, cancel, sender))
        .map_err(|error| format!("Der Auslesevorgang konnte nicht gestartet werden: {error}"))?;
    Ok(receiver)
}

fn run(job: RipJob, cancel: Arc<AtomicBool>, sender: Sender<RipEvent>) {
    let mut device = match Device::open(job.drive) {
        Ok(device) => device,
        Err(error) => {
            let _ = sender.send(RipEvent::Fatal(error));
            return;
        }
    };

    let total = job.tracks.len();
    let mut saved = 0_usize;
    let mut failed = 0_usize;

    for (index, track) in job.tracks.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        let title = track
            .title
            .clone()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| format!("Track {:02}", track.number));
        let _ = sender.send(RipEvent::TrackStarted {
            index,
            total,
            title,
        });

        let file_name = track_file_name(track.number, track.title.as_deref());
        let final_path = job.target_dir.join(&file_name);
        if !job.overwrite && final_path.exists() {
            failed += 1;
            let _ = sender.send(RipEvent::TrackFailed {
                number: track.number,
                error: format!("Die Datei ist bereits vorhanden: {}", final_path.display()),
            });
            continue;
        }

        // Erst in eine temporäre Datei schreiben. Dadurch bleibt eine bereits
        // vorhandene Datei erhalten, falls das Auslesen oder Codieren scheitert.
        let temp_path = job.target_dir.join(format!("{file_name}.part"));
        match rip_track(
            &mut device,
            track,
            &job,
            index,
            total,
            &temp_path,
            &cancel,
            &sender,
        ) {
            TrackOutcome::Done => {
                if cancel.load(Ordering::Relaxed) {
                    let _ = std::fs::remove_file(&temp_path);
                    break;
                }

                if final_path.exists() {
                    if !job.overwrite {
                        let _ = std::fs::remove_file(&temp_path);
                        failed += 1;
                        let _ = sender.send(RipEvent::TrackFailed {
                            number: track.number,
                            error: format!(
                                "Die Datei ist bereits vorhanden: {}",
                                final_path.display()
                            ),
                        });
                        continue;
                    }
                    if let Err(error) = std::fs::remove_file(&final_path) {
                        let _ = std::fs::remove_file(&temp_path);
                        failed += 1;
                        let _ = sender.send(RipEvent::TrackFailed {
                            number: track.number,
                            error: format!(
                                "Die vorhandene Datei konnte nicht ersetzt werden: {error}"
                            ),
                        });
                        continue;
                    }
                }

                match std::fs::rename(&temp_path, &final_path) {
                    Ok(()) => {
                        saved += 1;
                        let _ = sender.send(RipEvent::TrackFinished);
                    }
                    Err(error) => {
                        let _ = std::fs::remove_file(&temp_path);
                        failed += 1;
                        let _ = sender.send(RipEvent::TrackFailed {
                            number: track.number,
                            error: format!("Die MP3-Datei konnte nicht gespeichert werden: {error}"),
                        });
                    }
                }
            }
            TrackOutcome::Cancelled => {
                let _ = std::fs::remove_file(&temp_path);
                break;
            }
            TrackOutcome::Failed(error) => {
                let _ = std::fs::remove_file(&temp_path);
                failed += 1;
                let _ = sender.send(RipEvent::TrackFailed {
                    number: track.number,
                    error,
                });
            }
        }
    }

    let cancelled = cancel.load(Ordering::Relaxed);
    let _ = sender.send(RipEvent::Finished {
        saved,
        failed,
        cancelled,
    });
}

fn ffmpeg_command(job: &RipJob, track: &JobTrack, output: &Path) -> Command {
    let mut command = Command::new(&job.ffmpeg);
    command
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-f")
        .arg("s16be")
        .arg("-ar")
        .arg("44100")
        .arg("-ac")
        .arg("2")
        .arg("-i")
        .arg("pipe:0")
        .arg("-codec:a")
        .arg("libmp3lame")
        .arg("-b:a")
        .arg(format!("{}k", job.bitrate))
        .arg("-id3v2_version")
        .arg("3")
        .arg("-write_id3v1")
        .arg("0")
        .arg("-f")
        .arg("mp3");

    if let Some(artist) = job
        .artist
        .as_deref()
        .map(str::trim)
        .filter(|artist| !artist.is_empty())
    {
        command.arg("-metadata").arg(format!("artist={artist}"));
    }
    if let Some(album) = job
        .album
        .as_deref()
        .map(str::trim)
        .filter(|album| !album.is_empty())
    {
        command.arg("-metadata").arg(format!("album={album}"));
    }
    if let Some(title) = track
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
    {
        command.arg("-metadata").arg(format!("title={title}"));
    }
    command
        .arg("-metadata")
        .arg(format!("track={}/{}", track.number, job.total_tracks.max(1)));

    command.arg(output);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
}

#[allow(clippy::too_many_arguments)]
fn rip_track(
    device: &mut Device,
    track: &JobTrack,
    job: &RipJob,
    index: usize,
    total: usize,
    temp_path: &Path,
    cancel: &AtomicBool,
    sender: &Sender<RipEvent>,
) -> TrackOutcome {
    let mut child = match ffmpeg_command(job, track, temp_path).spawn() {
        Ok(child) => child,
        Err(error) => {
            return TrackOutcome::Failed(format!(
                "ffmpeg konnte nicht gestartet werden: {error}"
            ));
        }
    };

    let mut stderr_thread = child.stderr.take().map(|mut stderr| {
        thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        })
    });

    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return TrackOutcome::Failed("Die Eingabe des MP3-Encoders konnte nicht geöffnet werden.".into());
    };

    let mut remaining = track.sectors;
    let mut lba = track.start_lba;
    let mut buffer = vec![0_u8; SECTORS_PER_CHUNK as usize * BYTES_PER_SECTOR];

    while remaining > 0 {
        if cancel.load(Ordering::Relaxed) {
            drop(stdin);
            let _ = child.kill();
            let _ = child.wait();
            return TrackOutcome::Cancelled;
        }

        let sectors = remaining.min(SECTORS_PER_CHUNK);
        let bytes = sectors as usize * BYTES_PER_SECTOR;
        if let Err(error) = device.read_audio(lba, sectors, &mut buffer[..bytes]) {
            drop(stdin);
            let _ = child.kill();
            let _ = child.wait();
            let _ = stderr_thread.take().and_then(|handle| handle.join().ok());
            return TrackOutcome::Failed(error);
        }

        if let Err(error) = stdin.write_all(&buffer[..bytes]) {
            drop(stdin);
            let _ = child.kill();
            let _ = child.wait();
            let details = stderr_thread
                .take()
                .and_then(|handle| handle.join().ok())
                .unwrap_or_default();
            let details = details.trim();
            return TrackOutcome::Failed(if details.is_empty() {
                format!("Die Daten konnten nicht an den MP3-Encoder übergeben werden: {error}")
            } else {
                format!("Der MP3-Encoder wurde beendet: {details}")
            });
        }

        lba += sectors;
        remaining -= sectors;
        let fraction = 1.0 - (remaining as f32 / track.sectors.max(1) as f32);
        let overall = (index as f32 + fraction) / total.max(1) as f32;
        let _ = sender.send(RipEvent::TrackProgress {
            track_fraction: fraction,
            overall_fraction: overall,
        });
    }

    drop(stdin);
    let status = match child.wait() {
        Ok(status) => status,
        Err(error) => {
            let _ = stderr_thread.take().and_then(|handle| handle.join().ok());
            return TrackOutcome::Failed(format!(
                "Der MP3-Encoder konnte nicht abgeschlossen werden: {error}"
            ));
        }
    };
    let details = stderr_thread
        .take()
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();

    if !status.success() {
        let details = details.trim();
        return TrackOutcome::Failed(if details.is_empty() {
            format!(
                "Der MP3-Encoder wurde mit Fehlercode {} beendet.",
                status.code().unwrap_or(-1)
            )
        } else {
            format!("MP3-Encoder-Fehler: {details}")
        });
    }

    TrackOutcome::Done
}

#[cfg(windows)]
mod windows {
    use super::{build_tracks, parse_toc, RipDisc, BYTES_PER_SECTOR};
    use std::ffi::c_void;
    use std::ptr;

    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const OPEN_EXISTING: u32 = 3;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x0000_0080;
    const IOCTL_CDROM_READ_TOC_EX: u32 = 0x0002_4054;
    const IOCTL_SCSI_PASS_THROUGH_DIRECT: u32 = 0x0004_D004;
    const SCSI_IOCTL_DATA_IN: u8 = 1;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateFileW(
            file_name: *const u16,
            desired_access: u32,
            share_mode: u32,
            security_attributes: *mut c_void,
            creation_disposition: u32,
            flags_and_attributes: u32,
            template_file: *mut c_void,
        ) -> *mut c_void;
        fn DeviceIoControl(
            device: *mut c_void,
            io_control_code: u32,
            in_buffer: *const c_void,
            in_buffer_size: u32,
            out_buffer: *mut c_void,
            out_buffer_size: u32,
            bytes_returned: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn CloseHandle(object: *mut c_void) -> i32;
        fn GetLastError() -> u32;
    }

    #[repr(C)]
    struct ScsiPassThroughDirect {
        length: u16,
        scsi_status: u8,
        path_id: u8,
        target_id: u8,
        lun: u8,
        cdb_length: u8,
        sense_info_length: u8,
        data_in: u8,
        data_transfer_length: u32,
        time_out_value: u32,
        data_buffer: *mut c_void,
        sense_info_offset: u32,
        cdb: [u8; 16],
    }

    #[repr(C)]
    struct PassThrough {
        header: ScsiPassThroughDirect,
        sense: [u8; 18],
    }

    fn invalid_handle() -> *mut c_void {
        (-1_isize) as *mut c_void
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn last_error() -> u32 {
        unsafe { GetLastError() }
    }

    pub(super) struct Device {
        handle: *mut c_void,
    }

    impl Device {
        pub(super) fn open(drive: char) -> Result<Self, String> {
            let path = wide(&format!("\\\\.\\{drive}:"));
            let share_mode = FILE_SHARE_READ | FILE_SHARE_WRITE;
            let handle = unsafe {
                CreateFileW(
                    path.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    share_mode,
                    ptr::null_mut(),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    ptr::null_mut(),
                )
            };
            if handle == invalid_handle() {
                return Err(format!(
                    "Laufwerk {drive}: konnte nicht mit Lese-/Schreibzugriff geöffnet werden (Windows-Fehler {}).",
                    last_error()
                ));
            }
            Ok(Self { handle })
        }

        pub(super) fn read_toc(&self) -> Result<super::Toc, String> {
            let mut buffer = vec![0_u8; 1024];
            let mut returned = 0_u32;
            // Format TOC (0), Msf = 0: ask Windows for LBA addresses.
            // IOCTL_CDROM_READ_TOC returns MSF addresses, which must not be
            // interpreted as integers by parse_toc/build_tracks.
            let input = [0_u8; 4];
            let success = unsafe {
                DeviceIoControl(
                    self.handle,
                    IOCTL_CDROM_READ_TOC_EX,
                    input.as_ptr().cast(),
                    input.len() as u32,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut returned,
                    ptr::null_mut(),
                )
            };
            if success == 0 {
                return Err(format!(
                    "Das Inhaltsverzeichnis der CD konnte nicht gelesen werden (Windows-Fehler {}). Ist eine CD eingelegt?",
                    last_error()
                ));
            }
            buffer.truncate((returned as usize).min(buffer.len()));
            parse_toc(&buffer)
        }

        pub(super) fn read_audio(
            &self,
            lba: u32,
            sectors: u32,
            out: &mut [u8],
        ) -> Result<(), String> {
            let expected = sectors as usize * BYTES_PER_SECTOR;
            if out.len() != expected {
                return Err("Interner Fehler: falsche Puffergröße beim CD-Lesen.".into());
            }

            let mut request = PassThrough {
                header: ScsiPassThroughDirect {
                    length: std::mem::size_of::<ScsiPassThroughDirect>() as u16,
                    scsi_status: 0,
                    path_id: 0,
                    target_id: 0,
                    lun: 0,
                    cdb_length: 12,
                    sense_info_length: 18,
                    data_in: SCSI_IOCTL_DATA_IN,
                    data_transfer_length: out.len() as u32,
                    time_out_value: 30,
                    data_buffer: out.as_mut_ptr().cast(),
                    sense_info_offset: std::mem::size_of::<ScsiPassThroughDirect>() as u32,
                    cdb: [0_u8; 16],
                },
                sense: [0_u8; 18],
            };

            // READ CD (0xBE): CD-DA, nur Audio-Nutzdaten. Bei CD-DA ergeben
            // diese 2352 Nutzdatenbytes den vollständigen PCM-Sektor.
            request.header.cdb[0] = 0xBE;
            request.header.cdb[1] = 0x04;
            request.header.cdb[2..6].copy_from_slice(&lba.to_be_bytes());
            request.header.cdb[6] = ((sectors >> 16) & 0xFF) as u8;
            request.header.cdb[7] = ((sectors >> 8) & 0xFF) as u8;
            request.header.cdb[8] = (sectors & 0xFF) as u8;
            request.header.cdb[9] = 0x10;

            let size = std::mem::size_of::<PassThrough>() as u32;
            let mut returned = 0_u32;
            let success = unsafe {
                DeviceIoControl(
                    self.handle,
                    IOCTL_SCSI_PASS_THROUGH_DIRECT,
                    (&request as *const PassThrough).cast(),
                    size,
                    (&mut request as *mut PassThrough).cast(),
                    size,
                    &mut returned,
                    ptr::null_mut(),
                )
            };
            if success == 0 {
                return Err(format!(
                    "CD-Lesefehler bei LBA {lba} (Windows-Fehler {}).",
                    last_error()
                ));
            }
            if request.header.scsi_status != 0 {
                return Err(describe_sense(lba, request.header.scsi_status, &request.sense));
            }
            Ok(())
        }
    }

    impl Drop for Device {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }

    fn describe_sense(lba: u32, status: u8, sense: &[u8; 18]) -> String {
        let (key, asc, ascq) = if sense[0] & 0x7F == 0x72 {
            (sense[1] & 0x0F, sense[2], sense[3])
        } else {
            (sense[2] & 0x0F, sense[12], sense[13])
        };
        format!(
            "CD-Lesefehler bei LBA {lba} (SCSI-Status 0x{status:02X}, Sense 0x{key:02X}/0x{asc:02X}/0x{ascq:02X})."
        )
    }

    pub(super) fn read_disc(drive: char) -> Result<RipDisc, String> {
        let tracks = {
            let device = Device::open(drive)?;
            let toc = device.read_toc()?;
            build_tracks(&toc)?
        };

        let text = crate::cd::read_cd_text(drive).unwrap_or_default();
        let mut tracks = tracks;
        for track in &mut tracks {
            track.title = text.track_titles.get(&track.number).cloned();
        }
        Ok(RipDisc {
            drive,
            artist: text.artist,
            album: text.album,
            tracks,
        })
    }
}

#[cfg(not(windows))]
struct Device;

#[cfg(not(windows))]
impl Device {
    fn open(_drive: char) -> Result<Self, String> {
        Err("Das Auslesen von Audio-CDs wird derzeit nur unter Windows unterstützt.".into())
    }

    fn read_audio(&mut self, _lba: u32, _sectors: u32, _out: &mut [u8]) -> Result<(), String> {
        Err("Das Auslesen von Audio-CDs wird derzeit nur unter Windows unterstützt.".into())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Running,
    Finished,
}

struct Progress {
    track_index: usize,
    track_total: usize,
    track_title: String,
    track_fraction: f32,
    overall_fraction: f32,
}

struct PendingStart {
    job: RipJob,
    existing: Vec<PathBuf>,
}

/// Zustand und Oberfläche des Fensters „Audio-CD auslesen“.
pub struct RipWindow {
    open: bool,
    embedded: bool,
    disc: Option<RipDisc>,
    error: Option<String>,
    selected: Vec<bool>,
    bitrate: u32,
    target_dir: String,
    ffmpeg: Option<PathBuf>,
    phase: Phase,
    progress: Option<Progress>,
    receiver: Option<Receiver<RipEvent>>,
    cancel: Arc<AtomicBool>,
    pending_start: Option<PendingStart>,
    errors: Vec<String>,
    summary: Option<String>,
}

impl RipWindow {
    pub fn new() -> Self {
        Self {
            open: false,
            embedded: false,
            disc: None,
            error: None,
            selected: Vec::new(),
            bitrate: 192,
            target_dir: default_target_dir().display().to_string(),
            ffmpeg: find_ffmpeg(),
            phase: Phase::Idle,
            progress: None,
            receiver: None,
            cancel: Arc::new(AtomicBool::new(false)),
            pending_start: None,
            errors: Vec::new(),
            summary: None,
        }
    }

    /// Öffnet das Fenster und liest die eingelegte CD neu ein.
    pub fn open(&mut self) {
        self.open = true;
        if self.phase != Phase::Running {
            self.refresh();
        }
    }

    pub fn cancel_on_exit(&self) {
        if self.is_running() {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn is_running(&self) -> bool {
        self.phase == Phase::Running
    }

    fn refresh(&mut self) {
        self.error = None;
        self.errors.clear();
        self.summary = None;
        self.progress = None;
        self.pending_start = None;
        self.phase = Phase::Idle;
        self.ffmpeg = find_ffmpeg();

        match detect_audio_cd() {
            Ok(disc) => {
                self.selected = vec![true; disc.tracks.len()];
                self.disc = Some(disc);
            }
            Err(error) => {
                self.disc = None;
                self.selected.clear();
                self.error = Some(error);
            }
        }
    }

    fn request_close(&mut self, ctx: &egui::Context) {
        if self.is_running() {
            self.cancel.store(true, Ordering::Relaxed);
        }
        self.open = false;
        if !self.embedded {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn accept_window_close(&mut self) {
        if self.is_running() {
            self.cancel.store(true, Ordering::Relaxed);
        }
        self.open = false;
    }

    fn poll(&mut self) {
        let Some(receiver) = self.receiver.take() else {
            return;
        };
        let mut keep = true;
        loop {
            match receiver.try_recv() {
                Ok(event) => {
                    keep = self.handle_event(event);
                    if !keep {
                        break;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.phase == Phase::Running {
                        self.phase = Phase::Finished;
                        self.summary =
                            Some("Das Auslesen wurde unerwartet beendet.".into());
                    }
                    keep = false;
                    break;
                }
            }
        }
        if keep {
            self.receiver = Some(receiver);
        }
    }

    fn handle_event(&mut self, event: RipEvent) -> bool {
        match event {
            RipEvent::TrackStarted {
                index,
                total,
                title,
            } => {
                if let Some(progress) = &mut self.progress {
                    progress.track_index = index;
                    progress.track_total = total;
                    progress.track_title = title;
                    progress.track_fraction = 0.0;
                    progress.overall_fraction =
                        index as f32 / total.max(1) as f32;
                }
                true
            }
            RipEvent::TrackProgress {
                track_fraction,
                overall_fraction,
            } => {
                if let Some(progress) = &mut self.progress {
                    progress.track_fraction = track_fraction;
                    progress.overall_fraction = overall_fraction;
                }
                true
            }
            RipEvent::TrackFinished => true,
            RipEvent::TrackFailed { number, error } => {
                self.errors.push(format!("Track {number:02}: {error}"));
                true
            }
            RipEvent::Finished {
                saved,
                failed,
                cancelled,
            } => {
                self.phase = Phase::Finished;
                if let Some(progress) = &mut self.progress
                    && !cancelled
                {
                    progress.track_fraction = if failed == 0 { 1.0 } else { 0.0 };
                    progress.overall_fraction =
                        saved as f32 / progress.track_total.max(1) as f32;
                }
                let mut summary = if cancelled {
                    format!("Auslesen abgebrochen: {saved} Track(s) gespeichert.")
                } else if failed > 0 {
                    format!(
                        "Auslesen beendet: {saved} Track(s) gespeichert, {failed} fehlgeschlagen."
                    )
                } else {
                    format!("Auslesen beendet: {saved} Track(s) gespeichert.")
                };
                if failed > 0
                    && let Some(error) = self.errors.first()
                {
                    summary.push_str(&format!(" Erster Fehler: {error}"));
                }
                self.summary = Some(summary);
                false
            }
            RipEvent::Fatal(error) => {
                self.phase = Phase::Finished;
                self.summary = Some(format!("Auslesen konnte nicht gestartet werden: {error}"));
                self.errors.push(error);
                false
            }
        }
    }

    fn begin(&mut self, job: RipJob) {
        self.errors.clear();
        self.summary = None;
        self.pending_start = None;
        self.phase = Phase::Running;
        self.cancel = Arc::new(AtomicBool::new(false));

        let track_total = job.tracks.len();
        self.progress = Some(Progress {
            track_index: 0,
            track_total,
            track_title: "Vorbereitung…".into(),
            track_fraction: 0.0,
            overall_fraction: 0.0,
        });

        match spawn(job, self.cancel.clone()) {
            Ok(receiver) => self.receiver = Some(receiver),
            Err(error) => {
                self.phase = Phase::Idle;
                self.progress = None;
                self.summary = Some(error);
            }
        }
    }

    fn request_start(&mut self) {
        if self.phase == Phase::Running {
            return;
        }

        let Some(disc) = self.disc.as_ref() else {
            self.summary = Some("Keine Audio-CD geladen.".into());
            return;
        };
        let Some(ffmpeg) = self.ffmpeg.clone() else {
            self.summary =
                Some("ffmpeg wurde nicht gefunden. Ohne ffmpeg kann keine MP3-Datei erzeugt werden.".into());
            return;
        };
        let target_dir = PathBuf::from(self.target_dir.trim());
        if !target_dir.is_dir() {
            self.summary = Some("Der Zielordner existiert nicht.".into());
            return;
        }

        let selected = disc
            .tracks
            .iter()
            .zip(self.selected.iter())
            .filter(|(_, selected)| **selected)
            .map(|(track, _)| JobTrack {
                number: track.number,
                start_lba: track.start_lba,
                sectors: track.sectors,
                title: track.title.clone(),
            })
            .collect::<Vec<_>>();
        if selected.is_empty() {
            self.summary = Some("Bitte mindestens einen Track auswählen.".into());
            return;
        }

        let job = RipJob {
            drive: disc.drive,
            artist: disc.artist.clone(),
            album: disc.album.clone(),
            tracks: selected,
            total_tracks: disc.tracks.len() as u32,
            bitrate: self.bitrate,
            target_dir,
            overwrite: false,
            ffmpeg,
        };

        let existing = job
            .tracks
            .iter()
            .map(|track| {
                job.target_dir
                    .join(track_file_name(track.number, track.title.as_deref()))
            })
            .filter(|path| path.exists())
            .collect::<Vec<_>>();

        if existing.is_empty() {
            self.begin(job);
        } else {
            self.pending_start = Some(PendingStart { job, existing });
        }
    }

    fn draw_overwrite_prompt(&mut self, ctx: &egui::Context) {
        let Some(mut pending) = self.pending_start.take() else {
            return;
        };

        let mut decision = None;
        egui::Window::new("Dateien bereits vorhanden")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Folgende Dateien sind im Zielordner bereits vorhanden:");
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .id_salt("rip_existing_files")
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for path in &pending.existing {
                            ui.label(path.display().to_string());
                        }
                    });
                ui.add_space(8.0);
                ui.label("Es wird nichts ohne Ihre Zustimmung überschrieben.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Überschreiben").clicked() {
                        decision = Some(0_u8);
                    }
                    if ui.button("Vorhandene überspringen").clicked() {
                        decision = Some(1_u8);
                    }
                    if ui.button("Abbrechen").clicked() {
                        decision = Some(2_u8);
                    }
                });
            });

        match decision {
            Some(0) => {
                pending.job.overwrite = true;
                self.begin(pending.job);
            }
            Some(1) => {
                let existing = pending
                    .existing
                    .iter()
                    .cloned()
                    .collect::<HashSet<PathBuf>>();
                let target = pending.job.target_dir.clone();
                pending.job.tracks.retain(|track| {
                    !existing.contains(
                        &target.join(track_file_name(track.number, track.title.as_deref())),
                    )
                });
                if pending.job.tracks.is_empty() {
                    self.summary =
                        Some("Alle ausgewählten Dateien sind bereits vorhanden.".into());
                } else {
                    self.begin(pending.job);
                }
            }
            Some(2) => {
                self.summary = Some("Auslesen abgebrochen.".into());
            }
            _ => {
                self.pending_start = Some(pending);
            }
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        let disc = self.disc.clone();
        let running = self.is_running();
        let mut close = false;

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_APP)
                    .inner_margin(egui::Margin::same(18)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Audio-CD auslesen")
                            .font(theme::heading_font(24.0))
                            .color(theme::TEXT),
                    );
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui.button("Schließen").clicked() {
                                close = true;
                            }
                        },
                    );
                });
                ui.add_space(4.0);

                if let Some(disc) = &disc {
                    egui::Grid::new("rip_disc_info")
                        .num_columns(2)
                        .spacing([14.0, 5.0])
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("Laufwerk").color(theme::TEXT_MUTED));
                            ui.label(disc.drive_label());
                            ui.end_row();
                            ui.label(egui::RichText::new("Interpret").color(theme::TEXT_MUTED));
                            ui.label(disc.artist.as_deref().unwrap_or("—"));
                            ui.end_row();
                            ui.label(egui::RichText::new("Album").color(theme::TEXT_MUTED));
                            ui.label(disc.album.as_deref().unwrap_or("—"));
                            ui.end_row();
                            ui.label(egui::RichText::new("Tracks").color(theme::TEXT_MUTED));
                            ui.label(format!(
                                "{} · {}",
                                disc.tracks.len(),
                                format_duration(disc.total_seconds())
                            ));
                            ui.end_row();
                        });
                } else {
                    ui.colored_label(
                        ERROR_COLOR,
                        self.error
                            .as_deref()
                            .unwrap_or("Keine Audio-CD gefunden."),
                    );
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                if let Some(disc) = &disc {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!running, egui::Button::new("Alle auswählen"))
                            .clicked()
                        {
                            let count = disc.tracks.len();
                            self.selected = vec![true; count];
                        }
                        if ui
                            .add_enabled(!running, egui::Button::new("Keine auswählen"))
                            .clicked()
                        {
                            let count = disc.tracks.len();
                            self.selected = vec![false; count];
                        }
                    });

                    egui::ScrollArea::vertical()
                        .id_salt("rip_track_list")
                        .max_height(230.0)
                        .show(ui, |ui| {
                            for (index, track) in disc.tracks.iter().enumerate() {
                                let Some(selected) = self.selected.get_mut(index) else {
                                    continue;
                                };
                                ui.horizontal(|ui| {
                                    ui.add_enabled(
                                        !running,
                                        egui::Checkbox::new(selected, ""),
                                    );
                                    ui.label(
                                        egui::RichText::new(format!("{:02}", track.number))
                                            .monospace()
                                            .color(theme::TEXT_MUTED),
                                    );
                                    let title = track
                                        .title
                                        .as_deref()
                                        .filter(|title| !title.trim().is_empty())
                                        .unwrap_or("(kein Titel)");
                                    ui.label(title);
                                    ui.with_layout(
                                        egui::Layout::right_to_left(
                                            egui::Align::Center,
                                        ),
                                        |ui| {
                                            ui.label(
                                                egui::RichText::new(format_duration(
                                                    track.length_seconds(),
                                                ))
                                                .color(theme::TEXT_MUTED),
                                            );
                                        },
                                    );
                                });
                            }
                        });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label("Bitrate");
                        ui.add_enabled_ui(!running, |ui| {
                            egui::ComboBox::from_id_salt("rip_bitrate")
                                .selected_text(format!("{} kbit/s", self.bitrate))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.bitrate,
                                        192,
                                        "192 kbit/s",
                                    );
                                    ui.selectable_value(
                                        &mut self.bitrate,
                                        256,
                                        "256 kbit/s",
                                    );
                                    ui.selectable_value(
                                        &mut self.bitrate,
                                        320,
                                        "320 kbit/s",
                                    );
                                });
                        });
                    });

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Zielordner");
                        ui.add_enabled(
                            !running,
                            egui::TextEdit::singleline(&mut self.target_dir)
                                .desired_width(400.0),
                        );
                        if ui
                            .add_enabled(!running, egui::Button::new("Ordner wählen…"))
                            .clicked()
                        {
                            let mut dialog = rfd::FileDialog::new();
                            let current = PathBuf::from(self.target_dir.trim());
                            if current.is_dir() {
                                dialog = dialog.set_directory(current);
                            }
                            if let Some(folder) = dialog.pick_folder() {
                                self.target_dir = folder.display().to_string();
                            }
                        }
                    });
                }

                ui.add_space(6.0);
                match &self.ffmpeg {
                    Some(path) => {
                        ui.label(
                            egui::RichText::new(format!(
                                "MP3-Encoder: ffmpeg gefunden ({})",
                                path.display()
                            ))
                            .size(13.0)
                            .color(theme::TEXT_MUTED),
                        );
                    }
                    None => {
                        ui.colored_label(
                            ERROR_COLOR,
                            "ffmpeg wurde nicht gefunden. Bitte ffmpeg installieren oder die Umgebungsvariable FFMPEG auf ffmpeg.exe setzen.",
                        );
                    }
                }

                ui.add_space(8.0);
                if running {
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Abbrechen").clicked() {
                            self.cancel.store(true, Ordering::Relaxed);
                            self.summary = Some("Abbruch angefordert…".into());
                        }
                        if let Some(progress) = &self.progress {
                            ui.label(format!(
                                "Track {}/{}",
                                (progress.track_index + 1).min(progress.track_total.max(1)),
                                progress.track_total.max(1),
                            ));
                            ui.add(
                                egui::ProgressBar::new(progress.track_fraction)
                                    .desired_width(130.0)
                                    .show_percentage(),
                            );
                            ui.label("Gesamt");
                            ui.add(
                                egui::ProgressBar::new(progress.overall_fraction)
                                    .desired_width(130.0)
                                    .show_percentage(),
                            );
                        }
                    });
                } else {
                    ui.horizontal_wrapped(|ui| {
                        let can_start = self.pending_start.is_none()
                            && disc.is_some()
                            && self.selected.iter().any(|selected| *selected)
                            && !self.target_dir.trim().is_empty();
                        if ui
                            .add_enabled(
                                can_start,
                                egui::Button::new(
                                    egui::RichText::new("CD auslesen").strong(),
                                )
                                .fill(theme::ACCENT),
                            )
                            .clicked()
                        {
                            self.request_start();
                        }
                        if ui.button("Erneut suchen").clicked() {
                            self.refresh();
                        }
                        if let Some(summary) = &self.summary {
                            let color = if self.errors.is_empty() {
                                theme::TEXT
                            } else {
                                ERROR_COLOR
                            };
                            let response = ui.label(
                                egui::RichText::new(summary).size(12.0).color(color),
                            );
                            if !self.errors.is_empty() {
                                response.on_hover_text(self.errors.join("\n"));
                            }
                        }
                    });
                }
            });

        if close {
            self.request_close(ui.ctx());
        }
        self.draw_overwrite_prompt(ui.ctx());
    }
}

/// Zeigt das Auslesefenster als eigenes Betriebssystem-Fenster an.
///
/// Auf Plattformen ohne Mehrfenster-Unterstützung wird es als eingebettetes
/// Fenster dargestellt.
pub fn show_viewport(ctx: &egui::Context, window: Arc<Mutex<RipWindow>>) {
    let open = window
        .lock()
        .map(|window| window.open)
        .unwrap_or(false);
    if !open {
        return;
    }

    let callback_window = window.clone();
    ctx.show_viewport_deferred(
        egui::ViewportId::from_hash_of("audio_cd_rip"),
        egui::ViewportBuilder::default()
            .with_title("Audio-CD auslesen")
            .with_inner_size([780.0, 720.0])
            .with_min_inner_size([640.0, 520.0])
            .with_resizable(true),
        move |ui, class| {
            let Ok(mut window) = callback_window.lock() else {
                return;
            };
            window.embedded = matches!(class, egui::ViewportClass::EmbeddedWindow);

            if ui.input(|input| input.viewport().close_requested()) {
                // The OS already requested the close. Do not queue another
                // Close command here; leaving it uncancelled lets the viewport
                // close, while the active read is asked to stop in the worker.
                window.accept_window_close();
                return;
            }

            window.poll();
            window.draw(ui);
            if window.is_running() {
                ui.ctx().request_repaint_after(Duration::from_millis(200));
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toc_entry(control: u8, number: u8, lba: u32) -> Vec<u8> {
        let mut entry = vec![0_u8, control, number, 0];
        entry.extend_from_slice(&lba.to_be_bytes());
        entry
    }

    fn toc(first: u8, last: u8, entries: &[(u8, u8, u32)]) -> Vec<u8> {
        let mut data = vec![0_u8; 4];
        for (control, number, lba) in entries {
            data.extend(toc_entry(*control, *number, *lba));
        }
        let length = (data.len() - 2) as u16;
        data[0..2].copy_from_slice(&length.to_be_bytes());
        data[2] = first;
        data[3] = last;
        data
    }

    #[test]
    fn sanitizes_invalid_file_name_characters() {
        assert_eq!(sanitize_component("A/B:C*D?E"), "A_B_C_D_E");
        assert_eq!(sanitize_component("  Titel.  "), "Titel");
        assert_eq!(sanitize_component("CON"), "_CON");
        assert_eq!(sanitize_component("..."), "");
    }

    #[test]
    fn builds_file_names_from_track_number_and_title() {
        assert_eq!(
            track_file_name(1, Some("Speak to Me")),
            "01 - Speak to Me.mp3"
        );
        assert_eq!(track_file_name(4, None), "Track 04.mp3");
        assert_eq!(track_file_name(4, Some("...")), "Track 04.mp3");
    }

    #[test]
    fn parses_toc_and_computes_track_lengths() {
        let data = toc(
            1,
            3,
            &[(0, 1, 0), (0, 2, 7500), (0, 3, 15000), (0, 0xAA, 22500)],
        );
        let toc = parse_toc(&data).unwrap();
        assert_eq!(toc.first, 1);
        assert_eq!(toc.last, 3);
        let tracks = build_tracks(&toc).unwrap();
        assert_eq!(tracks.len(), 3);
        assert_eq!(tracks[0].number, 1);
        assert_eq!(tracks[0].start_lba, 0);
        assert_eq!(tracks[0].sectors, 7500);
        assert!((tracks[0].length_seconds() - 100.0).abs() < 1e-9);
        assert_eq!(tracks[2].start_lba, 15000);
        assert_eq!(tracks[2].sectors, 7500);
    }

    #[test]
    fn rejects_data_tracks() {
        let data = toc(
            1,
            2,
            &[(0, 1, 0), (0x04, 2, 7500), (0, 0xAA, 15000)],
        );
        let toc = parse_toc(&data).unwrap();
        assert!(build_tracks(&toc).is_err());
    }

    #[test]
    fn formats_durations() {
        assert_eq!(format_duration(0.0), "0:00");
        assert_eq!(format_duration(65.4), "1:05");
        assert_eq!(format_duration(3599.0), "59:59");
    }
}
