# MediaPlayer

Ein neuer MediaPlayer in Rust für Windows x64 und Linux x64.

## Aktueller Stand

Erster GUI-Prototyp:
- Dunkles Grunddesign
- Hauptbereich für Audio-/Videoausgabe
- Ein- und ausblendbare Playlist-Seitenleiste
- Platzhalter für die Wiedergabesteuerung

Die Wiedergabefunktionen sind noch nicht implementiert. Die Schaltflächen sind deshalb bewusst deaktiviert.

## Voraussetzungen

- Rust (stable) mit Cargo
- Für Linux gegebenenfalls die Systembibliotheken, die eframe/winit/wgpu benötigen

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
