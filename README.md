# MediaPlayer

Ein neuer MediaPlayer in Rust für Windows x64 und Linux x64.

## Aktueller Stand

- Dunkles Grunddesign mit ein- und ausblendbarer Playlist-Seitenleiste
- Wiedergabe von Audio- und Videodateien über libmpv
- DVD-Wiedergabe (ISO-Datei und optisches Laufwerk) mit Titel-, Kapitel-, Audio- und Untertitelauswahl
- Audio-CD-Wiedergabe direkt vom optischen Laufwerk inklusive Trackliste und Track-Navigation
  - **Audio-CD-Auslesen**: eigenes Fenster "Audio-CD auslesen" – Erkennung der Audio-CD, CD-Text (Interpret, Album, Track), Trackliste mit Auswahlmöglichkeiten, Bitratenauswahl (192/256/320 kbit/s), Zielordnerauswahl, **CD auslesen**-Schaltfläche, Fortschrittsanzeige, Abbrechen.
  - Windows: Wiedergabe über die Windows-MCI-Schnittstelle (kein libcdio erforderlich)
  - Linux/macOS: Wiedergabe über `cdda://`; libmpv muss dafür mit libcdio-Unterstützung (cdda://).

## Voraussetzungen

- Rust (stable) mit Cargo
- Für Linux gegebenenfalls die Systembibliotheken, die eframe/winit/wgpu benötigen
- Für Audio-CDs unter Linux/macOS: libmpv mit libcdio-Unterstützung

## Starten

```bash
cargo run
```

## Geplante nächste Schritte

1. GUI-Grundgerüst prüfen und stabilisieren
2. mpv-Engine integrieren
3. MP3-Wiedergabe
4. MP4-Videoausgabe
5. Playlist und Dateiauswahl
6. Vollbild und weitere Bedienfunktionen
