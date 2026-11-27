//! Wiedergabe von Audio-CDs über das optische Laufwerk.
//!
//! Unter Windows wird die Media Control Interface (MCI) von Windows genutzt,
//! weil die mitgelieferte libmpv-Version keine `cdda://`-Unterstützung
//! (libcdio) enthält. Auf allen anderen Systemen übernimmt libmpv die
//! Wiedergabe über `cdda://`; die MCI-Implementierung ist dort nicht verfügbar.

use std::collections::HashMap;
#[cfg(not(windows))]
use std::path::Path;
use std::path::PathBuf;

/// Erkennt eine eingehängte DVD oder Blu-ray an ihrer Disc-Struktur.
pub fn detect_video_disc() -> Option<(bool, PathBuf)> {
    let roots = mounted_optical_roots();
    roots.iter().find(|root| root.join("BDMV").is_dir()).map(|path| (true, path.clone()))
        .or_else(|| roots.iter().find(|root| root.join("VIDEO_TS").is_dir()).map(|path| (false, path.clone())))
}

fn mounted_optical_roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    { return windows::optical_roots(); }
    #[cfg(not(windows))]
    {
        let mut roots = Vec::new();
        #[cfg(target_os = "macos")]
        let bases = [Path::new("/Volumes")];
        #[cfg(not(target_os = "macos"))]
        let bases = [Path::new("/media"), Path::new("/run/media"), Path::new("/mnt")];
        for base in bases {
            if let Ok(entries) = std::fs::read_dir(base) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.join("BDMV").is_dir() || path.join("VIDEO_TS").is_dir() { roots.push(path.clone()); }
                    if let Ok(nested) = std::fs::read_dir(&path) {
                        for child in nested.flatten().map(|entry| entry.path()) {
                            if child.join("BDMV").is_dir() || child.join("VIDEO_TS").is_dir() { roots.push(child); }
                        }
                    }
                }
            }
        }
        roots
    }
}

/// Ein Track einer Audio-CD.
#[derive(Clone, Debug, PartialEq)]
pub struct CdTrack {
    pub number: u32,
    pub length_seconds: f64,
    pub title: Option<String>,
}

/// Die direkt von der CD gelesenen Textinformationen (CD-Text).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CdText {
    pub album: Option<String>,
    pub artist: Option<String>,
    pub track_titles: HashMap<u32, String>,
}

/// Liefert die Buchstaben aller optischen Laufwerke.
#[cfg(windows)]
pub(crate) fn optical_drives() -> Vec<char> {
    windows::cd_drives()
}

#[cfg(not(windows))]
pub(crate) fn optical_drives() -> Vec<char> {
    Vec::new()
}

/// Liest CD-Text (Album, Interpret und Tracktitel) für ein Laufwerk.
#[cfg(windows)]
pub(crate) fn read_cd_text(drive: char) -> Option<CdText> {
    windows::read_cd_text(drive)
}

#[cfg(not(windows))]
pub(crate) fn read_cd_text(_drive: char) -> Option<CdText> {
    None
}

/// Wiedergabezustand einer Audio-CD.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CdMode {
    Playing,
    Paused,
    Stopped,
}

/// Momentaufnahme des CD-Laufwerks.
#[derive(Clone, Debug, PartialEq)]
pub struct CdStatus {
    pub mode: CdMode,
    pub track: u32,
    pub position_seconds: f64,
    pub track_length_seconds: f64,
}

/// Eine geöffnete Audio-CD.
// Ohne Windows wird die Struktur nie erzeugt, weil libmpv die Wiedergabe übernimmt.
#[cfg_attr(not(windows), allow(dead_code))]
pub struct AudioCd {
    drive: String,
    tracks: Vec<CdTrack>,
    disc_title: Option<String>,
    total_seconds: f64,
}

impl AudioCd {
    /// Öffnet das erste CD-Laufwerk mit einer eingelegten Audio-CD.
    #[cfg(windows)]
    pub fn open_default() -> Result<Self, String> {
        windows::open_default()
    }

    /// Auf Nicht-Windows-Systemen übernimmt libmpv die CD-Wiedergabe.
    #[cfg(not(windows))]
    pub fn open_default() -> Result<Self, String> {
        Err("Die native Audio-CD-Wiedergabe ist nur unter Windows verfügbar.".into())
    }

    pub fn drive_label(&self) -> &str {
        &self.drive
    }

    pub fn disc_title(&self) -> Option<&str> {
        self.disc_title.as_deref()
    }

    pub fn tracks(&self) -> &[CdTrack] {
        &self.tracks
    }

    pub fn total_seconds(&self) -> f64 {
        self.total_seconds
    }

    #[cfg(windows)]
    pub fn play_track(&mut self, number: u32) -> Result<(), String> {
        if !self.tracks.iter().any(|track| track.number == number) {
            return Err(format!("Track {number} ist nicht vorhanden."));
        }
        windows::play_track(number)
    }

    #[cfg(not(windows))]
    pub fn play_track(&mut self, _number: u32) -> Result<(), String> {
        Err("Die native Audio-CD-Wiedergabe ist nur unter Windows verfügbar.".into())
    }

    #[cfg(windows)]
    pub fn play(&mut self) -> Result<(), String> {
        windows::play()
    }

    #[cfg(not(windows))]
    pub fn play(&mut self) -> Result<(), String> {
        Err("Die native Audio-CD-Wiedergabe ist nur unter Windows verfügbar.".into())
    }

    #[cfg(windows)]
    pub fn pause(&mut self) -> Result<(), String> {
        windows::pause()
    }

    #[cfg(not(windows))]
    pub fn pause(&mut self) -> Result<(), String> {
        Err("Die native Audio-CD-Wiedergabe ist nur unter Windows verfügbar.".into())
    }

    #[cfg(windows)]
    pub fn stop(&mut self) -> Result<(), String> {
        windows::stop()
    }

    #[cfg(not(windows))]
    pub fn stop(&mut self) -> Result<(), String> {
        Err("Die native Audio-CD-Wiedergabe ist nur unter Windows verfügbar.".into())
    }

    #[cfg(windows)]
    pub fn status(&mut self) -> Result<CdStatus, String> {
        windows::status(&self.tracks)
    }

    #[cfg(not(windows))]
    pub fn status(&mut self) -> Result<CdStatus, String> {
        Err("Die native Audio-CD-Wiedergabe ist nur unter Windows verfügbar.".into())
    }
}

#[cfg(windows)]
impl Drop for AudioCd {
    fn drop(&mut self) {
        windows::close();
    }
}

#[cfg(windows)]
mod windows {
    use super::{AudioCd, CdMode, CdStatus, CdText, CdTrack};
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::ptr;

    const ALIAS: &str = "mediaplayer_cd";
    const DRIVE_CDROM: u32 = 5;

    const GENERIC_READ: u32 = 0x8000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const OPEN_EXISTING: u32 = 3;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x0000_0080;
    const IOCTL_CDROM_READ_TOC_EX: u32 = 0x0002_4054;
    const TITLE_PACK: u8 = 0x80;
    const PERFORMER_PACK: u8 = 0x81;

    #[link(name = "winmm")]
    unsafe extern "system" {
        fn mciSendStringW(
            command: *const u16,
            return_string: *mut u16,
            return_length: u32,
            callback: *mut c_void,
        ) -> u32;
        fn mciGetErrorStringW(error: u32, buffer: *mut u16, length: u32) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLogicalDrives() -> u32;
        fn GetDriveTypeW(root_path_name: *const u16) -> u32;
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
    }

    fn invalid_handle() -> *mut c_void {
        (-1_isize) as *mut c_void
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn mci(command: &str) -> Result<String, String> {
        let command = wide(command);
        let mut buffer = vec![0_u16; 512];
        let code = unsafe {
            mciSendStringW(
                command.as_ptr(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                ptr::null_mut(),
            )
        };
        if code != 0 {
            return Err(mci_error(code));
        }
        let length = buffer.iter().position(|&value| value == 0).unwrap_or(buffer.len());
        Ok(String::from_utf16_lossy(&buffer[..length]).trim().to_string())
    }

    fn mci_error(code: u32) -> String {
        let mut buffer = vec![0_u16; 256];
        let success = unsafe { mciGetErrorStringW(code, buffer.as_mut_ptr(), buffer.len() as u32) };
        if success == 0 {
            return format!("MCI-Fehler {code}");
        }
        let length = buffer.iter().position(|&value| value == 0).unwrap_or(buffer.len());
        let message = String::from_utf16_lossy(&buffer[..length]);
        format!("{} (MCI {code})", message.trim())
    }

    pub(super) fn cd_drives() -> Vec<char> {
        let mask = unsafe { GetLogicalDrives() };
        let mut drives = Vec::new();
        for index in 0..26_u32 {
            if mask & (1 << index) == 0 {
                continue;
            }
            let letter = char::from(b'A' + index as u8);
            let root = wide(&format!("{letter}:\\"));
            if unsafe { GetDriveTypeW(root.as_ptr()) } == DRIVE_CDROM {
                drives.push(letter);
            }
        }
        drives
    }

    pub(super) fn open_default() -> Result<AudioCd, String> {
        let drives = cd_drives();
        if drives.is_empty() {
            return Err("Kein CD-Laufwerk gefunden.".into());
        }

        let mut errors = Vec::new();
        for drive in drives {
            match open_drive(drive) {
                Ok(cd) => return Ok(cd),
                Err(error) => errors.push(format!("{drive}: {error}")),
            }
        }
        Err(format!("Keine Audio-CD gefunden ({})", errors.join("; ")))
    }

    pub(super) fn optical_roots() -> Vec<std::path::PathBuf> {
        cd_drives().into_iter().map(|drive| std::path::PathBuf::from(format!("{drive}:\\"))).collect()
    }

    fn open_drive(drive: char) -> Result<AudioCd, String> {
        let _ = mci(&format!("close {ALIAS}"));
        mci(&format!("open {drive}: type cdaudio alias {ALIAS} shareable"))?;

        let result = (|| -> Result<AudioCd, String> {
            let _ = mci(&format!("set {ALIAS} time format tmsf"));

            let track_count_text = mci(&format!("status {ALIAS} number of tracks"))?;
            let track_count = track_count_text
                .trim()
                .parse::<u32>()
                .map_err(|_| format!("Trackanzahl nicht lesbar: {track_count_text}"))?;
            if track_count == 0 {
                let ready = mci(&format!("status {ALIAS} ready")).unwrap_or_default();
                if ready.eq_ignore_ascii_case("false") {
                    return Err("Laufwerk nicht bereit (keine CD eingelegt?)".into());
                }
                return Err("keine Audio-Tracks gefunden (keine Audio-CD eingelegt?)".into());
            }
            if let Ok(track_type) = mci(&format!("status {ALIAS} type track 1")) {
                let track_type = track_type.trim();
                if !track_type.is_empty() && !track_type.eq_ignore_ascii_case("audio") {
                    return Err("keine Audio-CD (erster Track ist kein Audio-Track)".into());
                }
            }

            let mut tracks = Vec::with_capacity(track_count as usize);
            for number in 1..=track_count {
                let length_seconds = mci(&format!("status {ALIAS} length track {number}"))
                    .ok()
                    .and_then(|value| parse_tmsf_seconds(&value))
                    .unwrap_or(0.0);
                tracks.push(CdTrack {
                    number,
                    length_seconds,
                    title: None,
                });
            }

            let total_seconds = mci(&format!("status {ALIAS} length"))
                .ok()
                .and_then(|value| parse_tmsf_seconds(&value))
                .unwrap_or_else(|| tracks.iter().map(|track| track.length_seconds).sum());

            let text = read_cd_text(drive).unwrap_or_default();
            for track in &mut tracks {
                if let Some(title) = text.track_titles.get(&track.number) {
                    track.title = Some(title.clone());
                }
            }

            Ok(AudioCd {
                drive: format!("{drive}:"),
                tracks,
                disc_title: text.album,
                total_seconds,
            })
        })();

        if result.is_err() {
            let _ = mci(&format!("close {ALIAS}"));
        }
        result
    }

    pub(super) fn play_track(number: u32) -> Result<(), String> {
        mci(&format!("play {ALIAS} from {number}"))?;
        Ok(())
    }

    pub(super) fn play() -> Result<(), String> {
        let mode = mci(&format!("status {ALIAS} mode")).unwrap_or_default();
        if mode.eq_ignore_ascii_case("paused") {
            mci(&format!("resume {ALIAS}"))?;
        } else if mode.eq_ignore_ascii_case("stopped") {
            // Nach "stop" an der aktuellen Position des aktuellen Tracks starten.
            let track = mci(&format!("status {ALIAS} current track"))
                .ok()
                .and_then(|value| value.trim().parse::<u32>().ok())
                .filter(|track| *track > 0)
                .unwrap_or(1);
            mci(&format!("play {ALIAS} from {track}"))?;
        } else {
            mci(&format!("play {ALIAS}"))?;
        }
        Ok(())
    }

    pub(super) fn pause() -> Result<(), String> {
        mci(&format!("pause {ALIAS}"))?;
        Ok(())
    }

    pub(super) fn stop() -> Result<(), String> {
        mci(&format!("stop {ALIAS}"))?;
        Ok(())
    }

    pub(super) fn close() {
        let _ = mci(&format!("close {ALIAS}"));
    }

    pub(super) fn status(tracks: &[CdTrack]) -> Result<CdStatus, String> {
        let mode_text = mci(&format!("status {ALIAS} mode"))?;
        let mode = match mode_text.to_ascii_lowercase().as_str() {
            "playing" => CdMode::Playing,
            "paused" => CdMode::Paused,
            _ => CdMode::Stopped,
        };

        let current_track = mci(&format!("status {ALIAS} current track"))
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok())
            .unwrap_or(0);

        let (position_track, position_seconds) = mci(&format!("status {ALIAS} position"))
            .ok()
            .and_then(|value| parse_position(&value))
            .unwrap_or((current_track, 0.0));

        let track = if position_track > 0 { position_track } else { current_track };
        let track_length_seconds = tracks
            .iter()
            .find(|candidate| candidate.number == track)
            .map(|candidate| candidate.length_seconds)
            .unwrap_or(0.0);

        Ok(CdStatus {
            mode,
            track,
            position_seconds,
            track_length_seconds,
        })
    }

    /// Parst `mm:ss:ff` bzw. `track:mm:ss:ff` (MCI-Zeitformat TMSF).
    fn parse_tmsf_seconds(value: &str) -> Option<f64> {
        let parts = value
            .split(':')
            .map(|part| part.trim().parse::<u32>().ok())
            .collect::<Option<Vec<_>>>()?;
        let (minutes, seconds, frames) = match parts.as_slice() {
            [minutes, seconds] => (*minutes, *seconds, 0),
            [minutes, seconds, frames] => (*minutes, *seconds, *frames),
            [_track, minutes, seconds, frames] => (*minutes, *seconds, *frames),
            _ => return None,
        };
        Some(f64::from(minutes) * 60.0 + f64::from(seconds) + f64::from(frames) / 75.0)
    }

    fn parse_position(value: &str) -> Option<(u32, f64)> {
        let parts = value
            .split(':')
            .map(|part| part.trim().parse::<u32>().ok())
            .collect::<Option<Vec<_>>>()?;
        match parts.as_slice() {
            [track, minutes, seconds, frames] => Some((
                *track,
                f64::from(*minutes) * 60.0 + f64::from(*seconds) + f64::from(*frames) / 75.0,
            )),
            [minutes, seconds, frames] => Some((
                0,
                f64::from(*minutes) * 60.0 + f64::from(*seconds) + f64::from(*frames) / 75.0,
            )),
            _ => None,
        }
    }

    struct TextPack {
        sequence: u8,
        unicode: bool,
        text: [u8; 12],
    }

    /// Liest die CD-Text-Informationen (Album, Interpret und Tracktitel), sofern vorhanden.
    pub(super) fn read_cd_text(drive: char) -> Option<CdText> {
        let path = wide(&format!("\\\\.\\{drive}:"));
        let share_mode = FILE_SHARE_READ | FILE_SHARE_WRITE;
        let mut handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ,
                share_mode,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                ptr::null_mut(),
            )
        };
        if handle == invalid_handle() {
            handle = unsafe {
                CreateFileW(
                    path.as_ptr(),
                    0,
                    share_mode,
                    ptr::null_mut(),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    ptr::null_mut(),
                )
            };
        }
        if handle == invalid_handle() {
            return None;
        }

        // CDROM_READ_TOC_EX mit Format = CDROM_READ_TOC_EX_FORMAT_CDTEXT (0x05).
        let input = [5_u8, 0, 0, 0];
        let mut output = vec![0_u8; 64 * 1024];
        let mut returned = 0_u32;
        let success = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_CDROM_READ_TOC_EX,
                input.as_ptr().cast(),
                input.len() as u32,
                output.as_mut_ptr().cast(),
                output.len() as u32,
                &mut returned,
                ptr::null_mut(),
            )
        };
        unsafe {
            CloseHandle(handle);
        }
        if success == 0 {
            return None;
        }

        let returned = returned as usize;
        if returned < 4 {
            return None;
        }
        let declared_be = usize::from(u16::from_be_bytes([output[0], output[1]]));
        let declared_le = usize::from(u16::from_le_bytes([output[0], output[1]]));
        let data_length = if 4 + declared_be <= returned {
            declared_be
        } else if 4 + declared_le <= returned {
            declared_le
        } else {
            returned - 4
        };
        let data = &output[4..4 + data_length];
        Some(parse_cd_text(data))
    }

    /// Wertet die CD-TEXT-Packs aus und liefert Album, Interpret sowie Track-Titel.
    fn parse_cd_text(data: &[u8]) -> CdText {
        let mut packs: HashMap<(u8, u8, u8), Vec<TextPack>> = HashMap::new();
        for block in data.as_chunks::<18>().0 {
            let pack_type = block[0];
            let track = block[1];
            let sequence = block[2];
            let header = block[3];
            let block_number = (header >> 4) & 0x07;
            let unicode = (header & 0x80) != 0;
            let mut text = [0_u8; 12];
            text.copy_from_slice(&block[4..16]);
            packs
                .entry((pack_type, track, block_number))
                .or_default()
                .push(TextPack {
                    sequence,
                    unicode,
                    text,
                });
        }

        let mut text = CdText::default();
        for ((pack_type, track, block_number), mut entries) in packs {
            // Block 0 = erste Sprache. 0x80 = TITLE, 0x81 = PERFORMER.
            if block_number != 0 || (pack_type != TITLE_PACK && pack_type != PERFORMER_PACK) {
                continue;
            }
            entries.sort_by_key(|pack| pack.sequence);

            let mut value = String::new();
            for pack in entries {
                let bytes = &pack.text;
                if pack.unicode {
                    for pair in bytes.as_chunks::<2>().0 {
                        let unit = u16::from_be_bytes([pair[0], pair[1]]);
                        if unit == 0 {
                            break;
                        }
                        if unit == 0xFEFF {
                            continue;
                        }
                        if let Some(character) = char::from_u32(u32::from(unit)) {
                            value.push(character);
                        }
                    }
                } else {
                    for &byte in bytes {
                        if byte == 0 {
                            break;
                        }
                        value.push(char::from(byte));
                    }
                }
            }

            let value = value.trim().to_string();
            if value.is_empty() {
                continue;
            }
            match (pack_type, track) {
                (TITLE_PACK, 0) => text.album = Some(value),
                (PERFORMER_PACK, 0) => text.artist = Some(value),
                (TITLE_PACK, _) => {
                    text.track_titles.insert(u32::from(track), value);
                }
                _ => {}
            }
        }

        text
    }

    #[cfg(test)]
    mod tests {
        use super::{parse_cd_text, parse_position, parse_tmsf_seconds};

        /// Baut einen 18 Byte großen CD-TEXT-Pack (4 Byte Kopf, 12 Byte Text, 2 Byte CRC).
        fn pack(pack_type: u8, track: u8, sequence: u8, header: u8, text: &[u8]) -> Vec<u8> {
            let mut block = vec![pack_type, track, sequence, header];
            let mut field = [0_u8; 12];
            let length = text.len().min(12);
            field[..length].copy_from_slice(&text[..length]);
            block.extend_from_slice(&field);
            block.extend_from_slice(&[0, 0]);
            block
        }

        fn close(left: f64, right: f64) -> bool {
            (left - right).abs() < 1e-9
        }

        #[test]
        fn parses_tmsf_values() {
            assert!(close(parse_tmsf_seconds("0:04:12").unwrap(), 4.16));
            assert!(close(parse_tmsf_seconds("1:02:30:00").unwrap(), 150.0));
            assert!(close(parse_tmsf_seconds("3:07").unwrap(), 187.0));
            assert_eq!(parse_tmsf_seconds("ungueltig"), None);
        }

        #[test]
        fn parses_positions() {
            let (track, seconds) = parse_position("2:00:10:15").unwrap();
            assert_eq!(track, 2);
            assert!(close(seconds, 10.2));

            let (track, seconds) = parse_position("00:10:15").unwrap();
            assert_eq!(track, 0);
            assert!(close(seconds, 10.2));

            assert_eq!(parse_position("kaputt"), None);
        }

        #[test]
        fn reads_album_performer_and_track_titles() {
            let mut data = Vec::new();
            data.extend(pack(0x80, 0, 0, 0x00, b"Test Album"));
            data.extend(pack(0x81, 0, 0, 0x00, b"Test Artist"));
            data.extend(pack(0x80, 1, 0, 0x00, b"First Song"));
            data.extend(pack(0x81, 1, 0, 0x00, b"Track Performer"));

            let mut unicode = Vec::new();
            for unit in "Über".encode_utf16() {
                unicode.extend_from_slice(&unit.to_be_bytes());
            }
            data.extend(pack(0x80, 2, 0, 0x80, &unicode));

            let text = parse_cd_text(&data);
            assert_eq!(text.album.as_deref(), Some("Test Album"));
            assert_eq!(text.artist.as_deref(), Some("Test Artist"));
            assert_eq!(text.track_titles.get(&1).map(String::as_str), Some("First Song"));
            assert_eq!(text.track_titles.get(&2).map(String::as_str), Some("Über"));
            assert_eq!(text.track_titles.len(), 2);
        }

        #[test]
        fn joins_long_titles_in_sequence_order() {
            let mut data = Vec::new();
            data.extend(pack(0x80, 1, 1, 0x00, b"B"));
            data.extend(pack(0x80, 1, 0, 0x00, b"AAAAAAAAAAAA"));

            let text = parse_cd_text(&data);
            assert_eq!(text.track_titles.get(&1).map(String::as_str), Some("AAAAAAAAAAAAB"));
        }
    }
}
