# Nexus

<div align="center">
  <img src="assets/logo-512-clean.png" width="96" height="96" alt="Nexus Logo" />
  <p><strong>Intelligent, ultra-lightweight ambient desktop assistant and wireless mobile bridge for Windows.</strong></p>
  <p>Built with Rust, Tauri v2, Win32/WASAPI, and Kotlin Jetpack Compose.</p>
</div>

---

## 🌟 Key Features

### 🎙️ 1. Ambient Voice Assistant & System Automation
* **Snappy Multi-Wake Listener:** Supports `Hey Google`, `Hey Inex`, `Jarvis`, `Alexa`, and `Hey PC` with zero-lag activation.
* **Ambient Noise Calibration:** Calibrates room RMS on startup for instant, crisp triggers without false positives.
* **100% Native Media Controls:** Full control over YouTube, Spotify, and system media via Windows Global System Media Transport Controls (GSMTC) with zero hotkey polling.
* **Smart App & Window Management:** Natural voice commands for window minimizing, focusing, closing, scrolling, and launching 70+ installed desktop applications.
* **Per-App Audio Mixer:** Adjust individual application volumes or master system volume by voice or phone.

### 📱 2. Nexus Companion (Phone Link)
* **120Hz Ultra-Low Latency UDP Trackpad:** Hardware-accelerated cursor navigation, double-tap dragging, momentum inertia scrolling, and multi-touch gestures:
  * **3-Finger Swipe Up:** Task View (`Win + Tab`)
  * **3-Finger Swipe Down:** Show Desktop (`Win + D`)
  * **3-Finger Swipe Left / Right:** Switch Virtual Desktops
* **Zero-Delay UDP Audio Streaming:**
  * Real-time hardware synchronization with FastTrack bypass (`USAGE_GAME`), eliminating DAC underruns and crackles.
  * Sub-40ms latency with zero perceptible lip-sync delay during video streaming and movies.
* **Modern Material You Interface:** Dynamic themed Android app with sleek capsule pill-ring tab navigation.
* **Wake-on-LAN & Remote Power:** Instant Wake-on-LAN (Magic Packet) to turn on your PC remotely, plus lock, reboot, and sleep controls.
* **Zero-Friction QR Pairing:** One-click camera scan pairing secured with HMAC-SHA256 authentication tokens.

### 🔍 3. Circle to Search (Visual AI & Shazam)
* **Offline OCR (Windows Media OCR):** Draw or circle any screen area to instantly extract, copy, or search clean text offline.
* **Live In-Place Translation:** Fast multi-language screen translation with newline-chunk batching, automatic fallback endpoints, and high-contrast overlay rendering.
* **Google Lens Visual Search:** Instant visual product, object, and image lookup directly from any region of your screen.
* **Desktop Audio Shazam:** WASAPI Loopback audio capture with landmark peak signature generation to identify currently playing songs in videos, reels, or background music.
* **Clean Modular Architecture:** Separated HTML, CSS, and JS components for responsive 120Hz overlay rendering.

### 🏝️ 4. Inex Island Integration
* Seamless IPC bridge via named pipes (`\\.\pipe\inex_island_ipc`) for dynamic ambient status display, wake listening states, and command feedback.

---

## ⚡ Performance & Efficiency
* **Near-Zero Idle CPU:** Less than 0.5% CPU utilization at idle.
* **Ultra-Low Memory Footprint:** Consistently runs under ~35-40 MB RAM.
* **No Stray Processes:** Clean Windows lifecycle management with resilient auto-reconnecting network bindings.

---

## 🛠️ Project Structure

```
nexus/
├── src-tauri/               # Rust core engine & Tauri v2 backend
│   ├── src/
│   │   ├── main.rs          # Application entry point, tray, and commands
│   │   ├── audio_stream.rs  # Real-time WASAPI loopback audio streamer
│   │   ├── audio_switch.rs  # Audio endpoint switching & headphone sync
│   │   ├── companion.rs     # Phone Link API & HMAC pairing engine
│   │   ├── executor.rs      # Natural language voice command execution
│   │   ├── trackpad.rs      # 120Hz UDP trackpad & gesture handler
│   │   ├── server.rs        # Resilient local HTTP server (port 41780)
│   │   └── modules/
│   │       ├── circle_search.rs  # Circle-to-Search orchestrator
│   │       └── cs/               # Capture, OCR, Shazam, translate, & hotkeys
│   ├── Cargo.toml           # Rust package manifest
│   └── tauri.conf.json      # Tauri application & bundle configuration
├── NexusCompanion/          # Native Android companion application
│   └── app/                 # Kotlin Jetpack Compose source & audio player
├── ui/                      # Frontend UI assets
│   ├── settings.html        # Main desktop settings panel
│   ├── settings.css / .js   # Settings styling and IPC logic
│   └── circle-search-overlay.html / .css / .js # Draggable Circle-to-Search UI
└── README.md
```

---

## 🚀 Building from Source

### PC Application (Rust + Tauri v2)
**Prerequisites:** [Rust](https://www.rust-lang.org/tools/install) (stable) and Tauri CLI v2 (`cargo install tauri-cli --version "^2.0.0"`).

```bash
# Clone the repository
git clone https://github.com/inrwxyz/nexus.git
cd nexus

# Run in development mode
cargo tauri dev

# Build production NSIS Windows installer
cargo tauri build --bundles nsis
```
The installer is generated under `src-tauri/target/release/bundle/nsis/Nexus_1.0.4_x64-setup.exe`.

### Android Companion (Kotlin)
**Prerequisites:** Android Studio / Android SDK (API 34) & JDK 17.

```bash
cd NexusCompanion
./gradlew assembleRelease
```
The release APK is generated under `app/build/outputs/apk/release/app-release.apk`.

---

## 📄 License

Distributed under the [GPL-3.0](LICENSE) License. Copyright © 2026 Inex. All rights reserved.
