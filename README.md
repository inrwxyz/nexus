# Nexus

<div align="center">
  <img src="assets/logo-512-clean.png" width="96" height="96" alt="Nexus Logo" />
  <p>Lightweight Windows desktop assistant and phone companion built with Rust, Tauri v2, and Kotlin.</p>
</div>

---

## Overview

Nexus is a fast, low-resource desktop companion for Windows that provides ambient voice automation, an offline Circle-to-Search overlay, and an Android companion app for wireless trackpad and zero-delay audio streaming.

## Features

- **Voice Automation**:
  - Offline wake-word detection (`Hey Google`, `Hey Inex`, `Hey PC`, `Alexa`, `Jarvis`).
  - Native media controls for Spotify and YouTube via Windows SMTC (play, pause, next, previous).
  - System volume and per-app audio mixer controls.
  - Window management (focus, minimize, close) and app launching.
- **Phone Link (Nexus Companion)**:
  - Wireless UDP trackpad for mouse movement, clicking, and scrolling from your phone.
  - Real-time UDP audio streaming from PC to phone with sub-40ms latency and no lip-sync delay.
  - Remote power management (Wake-on-LAN, PC lock, sleep, restart).
  - Quick pairing via local network QR code.
- **Circle to Search**:
  - Screen region capture with offline Windows Media OCR text copy.
  - In-place screen translation using Google Translate.
  - Google Lens image and visual search.
  - Desktop audio music recognition (WASAPI loopback + Shazam signature).
- **Inex Island Integration**:
  - Named pipe IPC (`\\.\pipe\inex_island_ipc`) for displaying voice status and animated states on Inex Island.

## Project Structure

```
nexus/
├── src-tauri/               # Rust backend & Tauri v2 core
│   ├── src/
│   │   ├── main.rs          # Application lifecycle, tray icon, Tauri commands
│   │   ├── audio_stream.rs  # WASAPI loopback audio capture and UDP streamer
│   │   ├── companion.rs     # Phone pairing and command handler
│   │   ├── executor.rs      # Voice command parsing and execution
│   │   ├── trackpad.rs      # UDP trackpad listener (port 41781)
│   │   ├── server.rs        # HTTP companion server (port 41780)
│   │   └── modules/
│   │       ├── circle_search.rs # Overlay window and search logic
│   │       └── cs/          # Capture, OCR, Shazam, translation submodules
│   ├── Cargo.toml
│   └── tauri.conf.json
├── NexusCompanion/          # Android companion application (Kotlin + Compose)
│   └── app/                 # Trackpad, AudioStream, and pairing screens
├── ui/                      # Desktop UI
│   ├── settings.html        # Desktop settings window
│   └── circle-search-overlay.html # Circle-to-Search screen overlay
└── README.md
```

## Build

### PC (Windows)
Prerequisites: [Rust](https://www.rust-lang.org/tools/install) (stable) and Tauri CLI v2.

```bash
cargo tauri build --bundles nsis
```
Output installer: `src-tauri/target/release/bundle/nsis/`

### Android Companion
Prerequisites: Android SDK & JDK 17.

```bash
cd NexusCompanion
./gradlew assembleRelease
```
Output APK: `NexusCompanion/app/build/outputs/apk/release/app-release.apk`

## License

[GPL-3.0](LICENSE)
